//! The engine session the commands share: socket discovery, typed calls, job waiting and
//! computer lookup.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde_json::Value;
use silo_api::{
    methods::{
        computers::ComputerListParams,
        kind,
        operations::{AcknowledgeJobParams, GetJobParams},
        requests, Kind, Request,
    },
    transport::ClientMetadata,
};
use silo_client::{channel, Client, ConnectOptions};

use crate::error::CliError;

const JOB_POLL: Duration = Duration::from_millis(300);
const PROGRESS_EVERY: Duration = Duration::from_secs(5);

/// Where progress lines go (stderr in the CLI, nowhere for MCP).
pub type Progress<'a> = &'a (dyn Fn(&str) + Sync);

/// The channel this binary belongs to: a debug build is the development build, like the app.
pub fn build_channel() -> channel::Channel {
    if cfg!(debug_assertions) {
        channel::Channel::Development
    } else {
        channel::Channel::Production
    }
}

/// `--socket`, then `SILO_SOCKET`, then the channel's own `engine.sock` under `home`.
pub fn resolve_socket(
    flag: Option<PathBuf>,
    environment: Option<OsString>,
    home: &Path,
    channel: channel::Channel,
) -> PathBuf {
    flag.or_else(|| {
        environment
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    })
    .unwrap_or_else(|| channel.engine_socket(home))
}

/// Refuses a socket outside the build channel's home. A development build never talks to the
/// production home, even when `allow_other` is set.
pub fn check_socket(
    socket: &Path,
    home: &Path,
    channel: channel::Channel,
    allow_other: bool,
) -> Result<(), CliError> {
    let inside = |directory: PathBuf| {
        socket.starts_with(&directory)
            && !socket
                .components()
                .any(|part| part == std::path::Component::ParentDir)
    };
    if inside(channel.state_dir(home)) {
        return Ok(());
    }
    if channel.is_development()
        && (inside(channel::Channel::Production.state_dir(home))
            || socket.starts_with(channel::Channel::Production.state_dir(home)))
    {
        return Err(CliError::new(format!(
            "{} belongs to the production Silo; this development build never uses it.",
            socket.display()
        )));
    }
    if allow_other {
        return Ok(());
    }
    Err(CliError::new(format!(
        "{} is not under {}. Pass --allow-other-channel to use it anyway.",
        socket.display(),
        channel.state_dir(home).display()
    )))
}

pub fn default_socket(flag: Option<PathBuf>, allow_other: bool) -> Result<PathBuf, CliError> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| CliError::new("HOME is not set."))?;
    let channel = build_channel();
    let socket = resolve_socket(flag, std::env::var_os("SILO_SOCKET"), &home, channel);
    check_socket(&socket, &home, channel, allow_other)?;
    Ok(socket)
}

/// A connection to the engine. Clones share it; it closes when the last clone is dropped.
#[derive(Clone)]
pub struct Engine {
    client: Client,
    last_job: Arc<Mutex<Option<String>>>,
    /// Where the CLI keeps what it remembers between commands (beside the socket).
    memory: std::path::PathBuf,
    _closer: Arc<Closer>,
}

/// A computer's step lock; released when dropped.
pub struct StepLock(#[allow(dead_code)] std::fs::File);

struct Closer(Client);

impl Drop for Closer {
    fn drop(&mut self) {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let client = self.0.clone();
            runtime.spawn(async move { client.close().await });
        }
    }
}

/// A computer as `computers.list` reports it, reduced to what the commands show.
#[derive(Clone, Debug, PartialEq)]
pub struct Computer {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub state: String,
    pub detail: Option<String>,
    pub cpus: u64,
    pub memory_gib: u64,
    pub disk_gib: Option<u64>,
    pub checkpoints: Vec<Value>,
    /// The Linux configuration exactly as listed (a delete must quote it).
    pub configuration: Option<Value>,
    /// The fork's recorded quiet state, as listed.
    pub quiet: Option<Value>,
    /// How many times the engine has booted this macOS computer.
    pub boots: Option<u64>,
    /// The engine's "changed since" count for the computer.
    pub generation: Option<u64>,
}

impl Computer {
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "kind": self.kind,
            "state": self.state,
            "detail": self.detail,
            "cpus": self.cpus,
            "memoryGiB": self.memory_gib,
            "diskGiB": self.disk_gib,
            "checkpoints": self.checkpoints.len(),
            "generation": self.generation,
        })
    }
}

/// Reads the computers out of a `computers.list` reply.
pub fn computers_from(listed: &Value) -> Vec<Computer> {
    let mut found = Vec::new();
    let text = |value: &Value, key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    let number = |value: &Value, key: &str| value.get(key).and_then(Value::as_u64);
    let checkpoints = |value: &Value| {
        value
            .get("checkpoints")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let quiet = |value: &Value| {
        value
            .get("quiet")
            .filter(|quiet| quiet.is_object())
            .cloned()
    };
    for computer in listed
        .pointer("/linux/computers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(configuration) = computer.get("configuration") else {
            continue;
        };
        found.push(Computer {
            id: text(configuration, "id").unwrap_or_default(),
            name: text(configuration, "name").unwrap_or_default(),
            kind: "linux",
            state: text(computer, "state").unwrap_or_else(|| "unknown".into()),
            detail: text(computer, "stateDetail").filter(|detail| !detail.is_empty()),
            cpus: number(configuration, "cpus").unwrap_or(0),
            memory_gib: number(configuration, "memoryGiB").unwrap_or(0),
            disk_gib: number(configuration, "workspaceStorageGiB"),
            checkpoints: checkpoints(computer),
            configuration: Some(configuration.clone()),
            quiet: quiet(computer),
            boots: None,
            generation: number(computer, "generation"),
        });
    }
    for computer in listed
        .pointer("/macos/computers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        found.push(Computer {
            id: text(computer, "id").unwrap_or_default(),
            name: text(computer, "name").unwrap_or_default(),
            kind: "macos",
            state: text(computer, "state").unwrap_or_else(|| "unknown".into()),
            detail: text(computer, "detail").filter(|detail| !detail.is_empty()),
            cpus: number(computer, "cpus").unwrap_or(0),
            memory_gib: number(computer, "memoryGiB").unwrap_or(0),
            disk_gib: number(computer, "diskGiB"),
            checkpoints: checkpoints(computer),
            configuration: None,
            quiet: quiet(computer),
            boots: number(computer, "boots"),
            generation: number(computer, "generation"),
        });
    }
    found
}

/// The computer called `name` (or addressed by id).
pub fn find<'a>(computers: &'a [Computer], name: &str) -> Result<&'a Computer, CliError> {
    computers
        .iter()
        .find(|computer| computer.name == name)
        .or_else(|| computers.iter().find(|computer| computer.id == name))
        .ok_or_else(|| CliError::new(format!("No computer is called {name}.")))
}

impl Engine {
    pub async fn connect(socket: &Path) -> Result<Self, CliError> {
        let options = ConnectOptions::new(ClientMetadata {
            name: "silo-cli".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        });
        let (client, events) = Client::connect(socket, options).await.map_err(|error| {
            CliError::new(format!(
                "Cannot reach the Silo engine at {}: {error}. Open Silo and try again.",
                socket.display()
            ))
        })?;
        // The commands read replies only; the event feed is dropped.
        drop(events);
        Ok(Self {
            _closer: Arc::new(Closer(client.clone())),
            client,
            last_job: Arc::new(Mutex::new(None)),
            memory: socket.parent().map(Path::to_path_buf).unwrap_or_default(),
        })
    }

    /// Holds the computer's step lock until the guard is dropped. A step (the command, the
    /// checkpoint after it and its record) runs under it, so on one computer step N's
    /// checkpoint is always taken after step N-1's and the numbers follow that order, whichever
    /// `silo` process runs the step. The lock is a file beside the socket.
    pub async fn lock_steps(&self, computer_id: &str) -> Result<StepLock, CliError> {
        let fine = !computer_id.is_empty()
            && computer_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !fine {
            return Err(CliError::new(
                "The computer id is not usable as a lock name.",
            ));
        }
        let path = self.memory.join(format!("cli-steps-{computer_id}.lock"));
        let locked = tokio::task::spawn_blocking(move || -> std::io::Result<std::fs::File> {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(path)?;
            file.lock()?;
            Ok(file)
        })
        .await
        .map_err(|_| CliError::new("The step lock could not be taken."))?
        .map_err(|error| CliError::new(format!("Cannot lock the computer's steps: {error}")))?;
        Ok(StepLock(locked))
    }

    /// Remembers the review digest of the last `silo diff` of a computer.
    pub fn remember_review(&self, computer_id: &str, digest: &str) {
        let path = self.memory.join("cli-reviewed.json");
        let mut all: serde_json::Map<String, Value> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        all.insert(computer_id.to_owned(), Value::String(digest.to_owned()));
        let _ = serde_json::to_vec(&all).map(|bytes| std::fs::write(&path, bytes));
    }

    /// The digest the last `silo diff` of this computer showed, if any.
    pub fn recall_review(&self, computer_id: &str) -> Option<String> {
        let bytes = std::fs::read(self.memory.join("cli-reviewed.json")).ok()?;
        let all: serde_json::Map<String, Value> = serde_json::from_slice(&bytes).ok()?;
        all.get(computer_id)?.as_str().map(str::to_owned)
    }

    /// Saves `value` under `key` in the CLI's memory file `file`.
    pub fn remember(&self, file: &str, key: &str, value: Value) -> Result<(), CliError> {
        let path = self.memory.join(file);
        let mut all = self.recall_all(file);
        all.insert(key.to_owned(), value);
        let bytes = serde_json::to_vec_pretty(&all)
            .map_err(|error| CliError::new(format!("Cannot save {file}: {error}")))?;
        std::fs::write(&path, bytes)
            .map_err(|error| CliError::new(format!("Cannot save {}: {error}", path.display())))
    }

    /// What was saved under `key` in the memory file `file`.
    pub fn recall(&self, file: &str, key: &str) -> Option<Value> {
        self.recall_all(file).remove(key)
    }

    /// Every entry of the memory file `file`.
    pub fn recall_all(&self, file: &str) -> serde_json::Map<String, Value> {
        std::fs::read(self.memory.join(file))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn is_closed(&self) -> bool {
        self.client.is_closed()
    }

    /// Records the job Ctrl-C should name when it is the one running.
    pub fn note_job(&self, job_id: &str) {
        *self.last_job.lock().unwrap() = Some(job_id.to_owned());
    }

    /// The job this connection most recently started.
    pub fn last_job(&self) -> Option<String> {
        self.last_job.lock().unwrap().clone()
    }

    /// What to tell someone whose wait ended before the job did.
    pub fn reattach_hint(job_id: &str) -> String {
        format!("The job {job_id} keeps running in the engine. Re-attach with `silo job {job_id}`.")
    }

    pub async fn call<K: Kind>(&self, request: Request<K>) -> Result<Value, CliError> {
        Ok(self
            .client
            .request(request.name(), request.params_value())
            .await?)
    }

    /// The computers a person works with: the throwaway computers that measure baselines are
    /// left out.
    pub async fn computers(&self) -> Result<Vec<Computer>, CliError> {
        let mut computers = self.all_computers().await?;
        computers.retain(|computer| {
            !computer.name.starts_with(crate::ops::BASELINE_PREFIX)
                && !computer.name.starts_with(crate::ops::BUILDER_PREFIX)
        });
        Ok(computers)
    }

    /// Every computer, throwaway baseline computers included.
    pub async fn all_computers(&self) -> Result<Vec<Computer>, CliError> {
        let listed = self
            .call(requests::computers_list(ComputerListParams {
                device_id: None,
                refresh_repositories: None,
            }))
            .await?;
        Ok(computers_from(&listed))
    }

    pub async fn computer(&self, name: &str) -> Result<Computer, CliError> {
        let computers = self.computers().await?;
        find(&computers, name).cloned()
    }

    /// Starts a job and waits for its outcome, reporting progress now and then.
    pub async fn run_job(
        &self,
        label: &str,
        request: Request<kind::Job>,
        progress: Progress<'_>,
    ) -> Result<Value, CliError> {
        let started = self.call(request).await?;
        let job_id = started
            .get("jobId")
            .and_then(Value::as_str)
            .ok_or_else(|| CliError::new("The engine did not accept the job."))?
            .to_owned();
        self.wait_job(&job_id, label, progress).await
    }

    /// Waits for a job already started (here or by another client) and acknowledges it.
    pub async fn wait_job(
        &self,
        job_id: &str,
        label: &str,
        progress: Progress<'_>,
    ) -> Result<Value, CliError> {
        *self.last_job.lock().unwrap() = Some(job_id.to_owned());
        let begun = Instant::now();
        let mut last_note = begun;
        progress(&format!("{label}..."));
        loop {
            let reply = match self.job(job_id, None).await {
                Ok(reply) => reply,
                Err(error) if self.is_closed() => {
                    return Err(CliError::new(format!(
                        "{error} {}",
                        Self::reattach_hint(job_id)
                    )))
                }
                Err(error) => return Err(error),
            };
            if reply.get("state").and_then(Value::as_str) == Some("finished") {
                let outcome = self.finished_outcome(job_id, reply).await?;
                let acknowledged = self
                    .call(requests::operations_job_acknowledge(AcknowledgeJobParams {
                        job_id: silo_api::JobId::new(job_id)
                            .map_err(|_| CliError::new("The engine sent an invalid job id."))?,
                        device_id: None,
                    }))
                    .await;
                if let Err(error) = acknowledged {
                    progress(&format!("warning: could not acknowledge {job_id}: {error}"));
                }
                return outcome_result(&outcome);
            }
            if last_note.elapsed() >= PROGRESS_EVERY {
                progress(&format!("{label}: {}s", begun.elapsed().as_secs()));
                last_note = Instant::now();
            }
            tokio::time::sleep(JOB_POLL).await;
        }
    }

    async fn job(&self, job_id: &str, offset: Option<u64>) -> Result<Value, CliError> {
        self.call(requests::operations_job_get(GetJobParams {
            job_id: silo_api::JobId::new(job_id)
                .map_err(|_| CliError::new("The engine sent an invalid job id."))?,
            device_id: None,
            offset_bytes: offset,
        }))
        .await
    }

    /// The outcome of a finished job, joining chunks when it was retained in parts.
    async fn finished_outcome(&self, job_id: &str, mut reply: Value) -> Result<Value, CliError> {
        let mut text = String::new();
        loop {
            let Some(chunk) = reply.get("outcomeChunk").cloned() else {
                return Ok(reply.get("outcome").cloned().unwrap_or(Value::Null));
            };
            text.push_str(
                chunk
                    .get("data")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            match chunk.get("nextOffsetBytes").and_then(Value::as_u64) {
                Some(next) => reply = self.job(job_id, Some(next)).await?,
                None => {
                    return serde_json::from_str(&text).map_err(|error| {
                        CliError::new(format!("The job outcome was unreadable: {error}"))
                    })
                }
            }
        }
    }
}

fn outcome_result(outcome: &Value) -> Result<Value, CliError> {
    if let Some(error) = outcome.get("error").filter(|error| !error.is_null()) {
        return Err(CliError::new(
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("The operation failed."),
        ));
    }
    Ok(outcome.get("result").cloned().unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn socket_precedence_is_flag_then_environment_then_channel_home() {
        let home = Path::new("/h");
        let flag = Some(PathBuf::from("/flag.sock"));
        let environment = Some(OsString::from("/env.sock"));
        assert_eq!(
            resolve_socket(
                flag,
                environment.clone(),
                home,
                channel::Channel::Production
            ),
            Path::new("/flag.sock")
        );
        assert_eq!(
            resolve_socket(None, environment, home, channel::Channel::Production),
            Path::new("/env.sock")
        );
        assert_eq!(
            resolve_socket(None, None, home, channel::Channel::Production),
            Path::new("/h/.silo/engine.sock")
        );
        assert_eq!(
            resolve_socket(
                None,
                Some(OsString::new()),
                home,
                channel::Channel::Development
            ),
            Path::new("/h/.silo-dev/engine.sock")
        );
    }

    #[test]
    fn sockets_outside_the_channel_home_need_the_flag() {
        use channel::Channel::{Development, Production};
        let home = Path::new("/h");
        let own = Path::new("/h/.silo-dev/engine.sock");
        let prod = Path::new("/h/.silo/engine.sock");
        let other = Path::new("/tmp/x/engine.sock");
        assert!(check_socket(own, home, Development, false).is_ok());
        assert!(check_socket(prod, home, Production, false).is_ok());
        assert!(check_socket(other, home, Development, false).is_err());
        assert!(check_socket(other, home, Development, true).is_ok());
        assert!(check_socket(prod, home, Development, false).is_err());
        assert!(check_socket(prod, home, Development, true).is_err());
        assert!(check_socket(own, home, Production, true).is_ok());
        assert!(check_socket(own, home, Production, false).is_err());
        let sneaky = Path::new("/h/.silo-dev/../.silo/engine.sock");
        assert!(check_socket(sneaky, home, Development, false).is_err());
    }

    #[test]
    fn computers_are_read_from_both_platforms() {
        let listed = json!({
            "linux": {"computers": [{
                "configuration": {"id": "l1", "name": "main", "cpus": 2, "memoryGiB": 4, "workspaceStorageGiB": 10},
                "state": "running", "stateDetail": "", "checkpoints": [{"id": "c1"}]
            }]},
            "macos": {"computers": [{
                "id": "m1", "name": "mac", "cpus": 4, "memoryGiB": 8, "diskGiB": 64,
                "state": "stopped", "detail": null, "checkpoints": []
            }]}
        });
        let computers = computers_from(&listed);
        assert_eq!(computers.len(), 2);
        assert_eq!(computers[0].kind, "linux");
        assert_eq!(computers[0].checkpoints.len(), 1);
        assert_eq!(computers[1].disk_gib, Some(64));
        assert_eq!(find(&computers, "m1").unwrap().name, "mac");
        assert!(find(&computers, "nope").is_err());
    }
}
