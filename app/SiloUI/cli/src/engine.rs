//! The engine session the commands share: socket discovery, typed calls, job waiting and
//! computer lookup.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
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

pub fn default_socket(flag: Option<PathBuf>) -> Result<PathBuf, CliError> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| CliError::new("HOME is not set."))?;
    Ok(resolve_socket(
        flag,
        std::env::var_os("SILO_SOCKET"),
        &home,
        build_channel(),
    ))
}

pub struct Engine {
    client: Client,
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
        Ok(Self { client })
    }

    pub async fn call<K: Kind>(&self, request: Request<K>) -> Result<Value, CliError> {
        Ok(self
            .client
            .request(request.name(), request.params_value())
            .await?)
    }

    pub async fn computers(&self) -> Result<Vec<Computer>, CliError> {
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
        let begun = Instant::now();
        let mut last_note = begun;
        progress(&format!("{label}..."));
        loop {
            let reply = self.job(&job_id, None).await?;
            if reply.get("state").and_then(Value::as_str) == Some("finished") {
                let outcome = self.finished_outcome(&job_id, reply).await?;
                let _ = self
                    .call(requests::operations_job_acknowledge(AcknowledgeJobParams {
                        job_id: silo_api::JobId::new(&job_id)
                            .map_err(|_| CliError::new("The engine sent an invalid job id."))?,
                        device_id: None,
                    }))
                    .await;
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
