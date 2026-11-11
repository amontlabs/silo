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

/// The hidden argument that prints the build channel's bundle identifier, so the engine links
/// only a `silo` of its own channel.
pub const CHANNEL_FLAG: &str = "--print-channel-identifier";

/// The channel this binary belongs to, fixed at build time by the bundle identifier the build
/// script passes in `SILO_CHANNEL_IDENTIFIER`. Only the exact production identifier selects
/// production, so a plain `cargo build` (either profile) is the development build.
pub fn build_channel() -> channel::Channel {
    channel_for(option_env!("SILO_CHANNEL_IDENTIFIER"))
}

fn channel_for(identifier: Option<&str>) -> channel::Channel {
    identifier.map_or(
        channel::Channel::Development,
        channel::Channel::from_identifier,
    )
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

/// Where the CLI keeps what it remembers between commands inside a computer, which has no
/// engine socket beside it: `$XDG_STATE_HOME/silo`, else `~/.local/state/silo`.
fn channel_memory() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    let memory = base.join("silo");
    {
        use std::os::unix::fs::DirBuilderExt;
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&memory);
    }
    memory
}

/// A connection to the engine. Clones share it; it closes when the last clone is dropped.
#[derive(Clone)]
pub struct Engine {
    client: Client,
    last_job: Arc<Mutex<Option<String>>>,
    /// Where the CLI keeps what it remembers between commands (beside the socket).
    memory: std::path::PathBuf,
    /// The connected device the commands act on; `None` is this device.
    device: Option<String>,
    /// Whether the engine serves requests for another device (older engines ignore `deviceId`).
    routes_devices: bool,
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
}

impl Computer {
    /// Whether this Linux computer starts its built-in desktop with itself: the setting
    /// `silo desktop` changes. `None` for macOS, whose display is the computer itself, and for
    /// a Linux computer whose desktop is not built in (installed by hand, or none), which has no
    /// such setting.
    pub fn desktop_on(&self) -> Option<bool> {
        let desktop = &self.configuration.as_ref()?["desktop"];
        if desktop["builtIn"] != true {
            return None;
        }
        Some(desktop["startWithComputer"] == true)
    }

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
            "desktop": self.desktop_on(),
        })
    }
}

/// Whether a `computers.list` reply says the engine supports macOS computers.
pub fn macos_supported(listed: &Value) -> bool {
    listed.pointer("/macos/supported").and_then(Value::as_bool) == Some(true)
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
            kind: if computer.get("kind").and_then(Value::as_str) == Some("container")
                || configuration.get("runtime").and_then(Value::as_str) == Some("container")
            {
                "container"
            } else {
                "linux"
            },
            state: text(computer, "state").unwrap_or_else(|| "unknown".into()),
            detail: text(computer, "stateDetail").filter(|detail| !detail.is_empty()),
            cpus: number(configuration, "cpus").unwrap_or(0),
            memory_gib: number(configuration, "memoryGiB").unwrap_or(0),
            disk_gib: number(configuration, "workspaceStorageGiB"),
            checkpoints: checkpoints(computer),
            configuration: Some(configuration.clone()),
            quiet: quiet(computer),
            boots: None,
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

/// How long a client waits before each new try when the engine refuses its handshake without
/// a new endpoint file: the engine writes one soon after it starts or ends a token.
const CHANNEL_WAITS: &[Duration] = &[
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];

/// The endpoint this process last reached the engine with, newer than the one it started with.
static LATEST_ENDPOINT: Mutex<Option<crate::endpoint::Endpoint>> = Mutex::new(None);

/// The endpoint file as it is now, from where this process found it.
fn reread_endpoint() -> Option<crate::endpoint::Endpoint> {
    let path = std::env::var_os(crate::endpoint::ENV)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(silo_api::methods::guest_client::GUEST_ENDPOINT_PATH)
        });
    crate::endpoint::Endpoint::load(&path).ok()
}

/// Connects over a computer's channel. A handshake the engine refuses (it closes without an
/// answer) usually means the token changed: the engine started again, or ended the token. The
/// endpoint file is read again and the connection tried again, waiting `waits` in turn while
/// the file has not changed.
async fn connect_channel(
    given: &crate::endpoint::Endpoint,
    mut options: ConnectOptions,
    reread: &(dyn Fn() -> Option<crate::endpoint::Endpoint> + Sync),
    waits: &[Duration],
) -> Result<(Client, silo_client::EventFeed), (crate::endpoint::Endpoint, silo_client::ClientError)>
{
    let latest = LATEST_ENDPOINT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .filter(|latest| latest.computer_id == given.computer_id);
    let mut endpoint = latest.unwrap_or_else(|| given.clone());
    let mut waits = waits.iter();
    loop {
        options.token = Some(endpoint.token.clone());
        match Client::connect_channel(&endpoint.host, endpoint.port, options.clone()).await {
            Err(silo_client::ClientError::Disconnected) => {
                let fresh = reread().filter(|fresh| fresh != &endpoint);
                if let Some(fresh) = fresh {
                    endpoint = fresh;
                    continue;
                }
                let Some(wait) = waits.next() else {
                    return Err((endpoint, silo_client::ClientError::Disconnected));
                };
                tokio::time::sleep(*wait).await;
                if let Some(fresh) = reread() {
                    endpoint = fresh;
                }
            }
            Err(error) => return Err((endpoint, error)),
            Ok(connected) => {
                if &endpoint != given {
                    *LATEST_ENDPOINT
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(endpoint);
                }
                return Ok(connected);
            }
        }
    }
}

impl Engine {
    pub async fn connect(socket: &Path) -> Result<Self, CliError> {
        Self::connect_to(&crate::endpoint::Transport::Socket(socket.to_owned())).await
    }

    /// Connects to the engine over the socket, or over a computer's channel with its token.
    pub async fn connect_to(transport: &crate::endpoint::Transport) -> Result<Self, CliError> {
        use crate::endpoint::Transport;
        let mut options = ConnectOptions::new(ClientMetadata {
            name: "silo-cli".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        });
        let (connected, memory) = match transport {
            Transport::Socket(socket) => (
                Client::connect(socket, options).await.map_err(|error| {
                    CliError::new(format!(
                        "Cannot reach the Silo engine at {}: {error}. Open Silo and try again.",
                        socket.display()
                    ))
                }),
                socket.parent().map(Path::to_path_buf).unwrap_or_default(),
            ),
            Transport::Channel(endpoint) => {
                options.token = Some(endpoint.token.clone());
                (
                    connect_channel(endpoint, options, &reread_endpoint, CHANNEL_WAITS)
                        .await
                        .map_err(|(endpoint, error)| {
                            CliError::new(format!(
                                "Cannot reach Silo at {}:{}: {error}. Silo may be closed, or this computer's tools were turned off or need a restart of the computer.",
                                endpoint.host, endpoint.port
                            ))
                        }),
                    channel_memory(),
                )
            }
        };
        let (client, events) = connected?;
        // The commands read replies only; the event feed is dropped.
        drop(events);
        let routes_devices = client.supports("device.routing.v1");
        Ok(Self {
            _closer: Arc::new(Closer(client.clone())),
            client,
            last_job: Arc::new(Mutex::new(None)),
            memory,
            device: None,
            routes_devices,
        })
    }

    /// Refuses to go on when the engine would answer a request for another device itself.
    pub fn require_routing(&self) -> Result<(), CliError> {
        if self.routes_devices {
            Ok(())
        } else {
            Err(CliError::new(
                "This Silo engine is older than the command line and cannot work on another device. Update Silo.",
            ))
        }
    }

    pub fn routes_devices(&self) -> bool {
        self.routes_devices
    }

    /// The same connection, acting on another device (`None`: this one). Commands that run
    /// on it send the device with every request the other device's engine can answer.
    pub fn on_device(&self, device: Option<String>) -> Engine {
        Engine {
            device,
            ..self.clone()
        }
    }

    /// The device the commands act on, when it is not this one.
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    /// The id of the connected device called `name` (its name, its id, or an id prefix).
    pub async fn resolve_device(&self, name: &str) -> Result<String, CliError> {
        self.require_routing()?;
        let listed = self
            .on_device(None)
            .call(requests::connections_devices_list(
                silo_api::methods::EmptyParams {},
            ))
            .await?;
        pick_device(&listed, name)
    }

    /// Devices whose last health probe failed (not merely busy), with the reason. Empty when
    /// the engine cannot say.
    pub async fn known_offline(&self) -> std::collections::HashMap<String, String> {
        let Ok(health) = self
            .on_device(None)
            .call(requests::connections_devices_health(
                silo_api::methods::EmptyParams {},
            ))
            .await
        else {
            return Default::default();
        };
        health["devices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|device| device["connected"] == false && device["busy"] == false)
            .filter_map(|device| {
                Some((
                    device["deviceId"].as_str()?.to_owned(),
                    device["error"].as_str()?.to_owned(),
                ))
            })
            .collect()
    }

    /// The devices this one is connected to, as `(id, name)`.
    pub async fn devices(&self) -> Result<Vec<(String, String)>, CliError> {
        let listed = self
            .on_device(None)
            .call(requests::connections_devices_list(
                silo_api::methods::EmptyParams {},
            ))
            .await?;
        Ok(device_rows(&listed))
    }

    /// Holds the computer's step lock until the guard is dropped. A step (the command, the
    /// checkpoint after it and its record) runs under it, so on one computer step N's
    /// checkpoint is always taken after step N-1's and the numbers follow that order, whichever
    /// `silo` process runs the step. The lock is a file beside the socket.
    pub async fn lock_steps(&self, computer_id: &str) -> Result<StepLock, CliError> {
        if self.memory.as_os_str().is_empty() {
            return Err(CliError::new(
                "There is no folder beside the socket for the lock.",
            ));
        }
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

    /// Removes the step lock file of a computer that no longer exists.
    pub fn forget_step_lock(&self, computer_id: &str) {
        if !self.memory.as_os_str().is_empty() {
            let _ = std::fs::remove_file(self.memory.join(format!("cli-steps-{computer_id}.lock")));
        }
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

    /// The same connection, but the jobs it starts are not recorded for Ctrl-C: background
    /// housekeeping never becomes the job an interrupted command names.
    pub fn without_job_notes(&self) -> Self {
        Self {
            last_job: Arc::new(Mutex::new(None)),
            ..self.clone()
        }
    }

    /// Whether the job this connection most recently started has finished. False when it is
    /// still running, queued, or cannot be asked about, e.g. while the other device is out of
    /// reach: a wait that failed says nothing about the job itself.
    pub async fn last_job_finished(&self) -> bool {
        let Some(job_id) = self.last_job() else {
            return false;
        };
        self.job(&job_id, None)
            .await
            .is_ok_and(|reply| reply.get("state").and_then(Value::as_str) == Some("finished"))
    }

    /// What to tell someone whose wait ended before the job did.
    pub fn reattach_hint(job_id: &str) -> String {
        format!("The job {job_id} keeps running in the engine. Re-attach with `silo job {job_id}`.")
    }

    pub async fn call<K: Kind>(&self, request: Request<K>) -> Result<Value, CliError> {
        if self.device.is_some()
            && !is_routed(request.name())
            && !request.name().starts_with("connections.")
        {
            return Err(CliError::new(format!(
                "{} cannot run on another device.",
                request.name()
            )));
        }
        let params = routed_params(request.name(), request.params_value(), self.device());
        Ok(self.client.request(request.name(), params).await?)
    }

    /// The computers a person works with: the throwaway computers that measure baselines, build
    /// recipes and carry exports are left out.
    pub async fn computers(&self) -> Result<Vec<Computer>, CliError> {
        let mut computers = self.all_computers().await?;
        computers.retain(|computer| {
            !computer.name.starts_with(crate::ops::BASELINE_PREFIX)
                && !computer.name.starts_with(crate::ops::BUILDER_PREFIX)
                && !computer.name.starts_with(crate::ops::EXPORT_COPY_PREFIX)
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

    /// Whether the engine that runs the commands reports that it runs macOS computers, which
    /// only a Mac does.
    pub async fn runs_macos_computers(&self) -> Result<bool, CliError> {
        let listed = self
            .call(requests::computers_list(ComputerListParams {
                device_id: None,
                refresh_repositories: None,
            }))
            .await?;
        Ok(macos_supported(&listed))
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

/// The methods a command sends to another device's engine when it acts on that device: the
/// ones `silod` routes through Connections (`silo_engine::remote::OWNER_METHODS`), the
/// commands, desktop and computer-use methods it routes to the owner's own jobs, and the job
/// reads that follow them. Everything else (secrets, ports, SSH access and names, GitHub,
/// pushes, the guest client, `desktop.connect`) has controller-side steps that stay on this
/// device.
const ROUTED_PREFIXES: &[&str] = &[
    "changes.",
    "checkpoints.",
    "computerUse.",
    "computers.",
    "macos.setup.",
    "operations.job.",
    "recipes.",
    "steps.",
];

/// Routed methods named exactly: `guest.exec` and its job (`.start`, `.get`, `.input`), and
/// the desktop's status and actions. Other `guest.` and `desktop.` methods stay here.
const ROUTED_METHODS: &[&str] = &[
    "desktop.action",
    "desktop.status",
    "exports.check",
    "exports.create",
    "exports.discard",
    "exports.import",
    "guest.exec",
    "guest.exec.get",
    "guest.exec.input",
    "guest.exec.start",
];

fn is_routed(method: &str) -> bool {
    ROUTED_METHODS.contains(&method)
        || ROUTED_PREFIXES
            .iter()
            .any(|prefix| method.starts_with(prefix))
}

/// `params` with the `deviceId` of `device` added when `method` is routed.
pub fn routed_params(method: &str, mut params: Value, device: Option<&str>) -> Value {
    let routed = is_routed(method);
    if let (Some(device), true, Some(object)) = (device, routed, params.as_object_mut()) {
        object
            .entry("deviceId")
            .or_insert_with(|| Value::String(device.to_owned()));
    }
    params
}

fn device_rows(listed: &Value) -> Vec<(String, String)> {
    listed
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|device| {
            Some((
                device.get("id")?.as_str()?.to_owned(),
                device.get("name")?.as_str()?.to_owned(),
            ))
        })
        .collect()
}

/// The device `name` means among the connected ones: an exact id, else a name (ignoring case),
/// else an id prefix. Several matches are an error.
pub fn pick_device(listed: &Value, name: &str) -> Result<String, CliError> {
    let devices = device_rows(listed);
    let by = |test: &dyn Fn(&(String, String)) -> bool| -> Vec<&(String, String)> {
        devices.iter().filter(|device| test(device)).collect()
    };
    let lower = name.to_lowercase();
    for found in [
        by(&|device| device.0 == name),
        by(&|device| device.1.to_lowercase() == lower),
        by(&|device| !name.is_empty() && device.0.starts_with(name)),
    ] {
        match found.as_slice() {
            [] => continue,
            [one] => return Ok(one.0.clone()),
            _ => {
                return Err(CliError::new(format!(
                    "More than one connected device matches {name}. Use its id."
                )))
            }
        }
    }
    let known: Vec<_> = devices.iter().map(|device| device.1.as_str()).collect();
    Err(CliError::new(if known.is_empty() {
        format!("No device is called {name}: this device has no connections. Add one with silo devices add <user@host>.")
    } else {
        format!(
            "No device is called {name}. Connected devices: {}.",
            known.join(", ")
        )
    }))
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

    /// A channel on `port` that serves only `token`, answering like the engine: a challenge,
    /// then a reply to a valid proof or a close without one.
    async fn channel_serving(
        listener: tokio::net::TcpListener,
        token: &'static str,
    ) -> tokio::task::JoinHandle<usize> {
        use silo_api::transport::{channel_key, channel_proof_matches, from_hex, to_hex};
        use silo_client::{read_frame, write_frame};
        tokio::spawn(async move {
            let port = listener.local_addr().unwrap().port();
            let mut refused = 0;
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let nonce = [refused as u8; 32];
                write_frame(&mut stream, &json!({ "nonce": to_hex(&nonce) }))
                    .await
                    .unwrap();
                let request: Value = read_frame(&mut stream).await.unwrap().unwrap();
                let proof = request["params"]["proof"]
                    .as_str()
                    .and_then(from_hex::<32>)
                    .unwrap_or_default();
                if !channel_proof_matches(&channel_key(token), &nonce, port, &proof) {
                    refused += 1;
                    continue;
                }
                let reply = json!({"id": "handshake", "result": {
                    "deviceId": "dev", "name": "Mac", "protocol": 8,
                    "capabilities": [], "clientId": "c1"
                }});
                write_frame(&mut stream, &reply).await.unwrap();
                return refused;
            }
        })
    }

    fn endpoint(port: u16, token: &str, computer: &str) -> crate::endpoint::Endpoint {
        serde_json::from_value(json!({
            "host": "127.0.0.1", "port": port, "token": token, "computerId": computer
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn a_refused_handshake_reads_the_endpoint_file_again_and_recovers() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = channel_serving(listener, "silo_gc_new").await;
        let options = ConnectOptions::new(ClientMetadata {
            name: "test".into(),
            version: "0".into(),
        });
        let old = endpoint(port, "silo_gc_old", "c-recover");
        let fresh = endpoint(port, "silo_gc_new", "c-recover");
        let reread = || Some(fresh.clone());
        let (client, _) = connect_channel(&old, options.clone(), &reread, &[])
            .await
            .map_err(|(_, error)| error)
            .unwrap();
        assert_eq!(client.handshake().client_id.as_deref(), Some("c1"));
        assert_eq!(server.await.unwrap(), 1, "one refusal, then the new token");
        // Without a new file and with no waits left, the refusal is the answer.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let _server = channel_serving(listener, "silo_gc_other").await;
        let stale = endpoint(port, "silo_gc_stale", "c-stale");
        let same = || Some(stale.clone());
        assert!(matches!(
            connect_channel(&stale, options, &same, &[]).await,
            Err((_, silo_client::ClientError::Disconnected))
        ));
    }

    #[test]
    fn the_channel_is_development_unless_built_with_the_production_identifier() {
        use channel::Channel::{Development, Production};
        assert_eq!(channel_for(None), Development);
        assert_eq!(channel_for(Some("")), Development);
        assert_eq!(channel_for(Some("org.silo.dev")), Development);
        assert_eq!(channel_for(Some("org.silo.preview.x")), Development);
        assert_eq!(channel_for(Some("org.silo.preview")), Production);
        // A test build gets no identifier from the build script.
        if option_env!("SILO_CHANNEL_IDENTIFIER").is_none() {
            assert_eq!(build_channel(), Development);
        }
    }

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
    fn the_device_is_added_to_routed_methods_only() {
        let routed = routed_params("changes.diff", json!({ "computerId": "c" }), Some("d1"));
        assert_eq!(routed["deviceId"], "d1");
        let job = routed_params("operations.job.get", json!({ "jobId": "j" }), Some("d1"));
        assert_eq!(job["deviceId"], "d1");
        for routed in [
            "guest.exec",
            "guest.exec.start",
            "guest.exec.get",
            "guest.exec.input",
            "desktop.status",
            "desktop.action",
            "computerUse.state",
            "computerUse.approval",
        ] {
            assert_eq!(
                routed_params(routed, json!({}), Some("d1"))["deviceId"],
                "d1",
                "{routed}"
            );
        }
        for local in [
            "connections.devices.list",
            "secrets.list",
            "network.state",
            "network.urls",
            "guest.sshAlias.ensure",
            "guest.client.enable",
            "guest.client.status",
            "desktop.connect",
        ] {
            assert!(routed_params(local, json!({}), Some("d1"))
                .get("deviceId")
                .is_none());
        }
        assert!(routed_params("steps.list", json!({}), None)
            .get("deviceId")
            .is_none());
        // A request that already names a device keeps it.
        let named = routed_params("steps.list", json!({ "deviceId": "d2" }), Some("d1"));
        assert_eq!(named["deviceId"], "d2");
    }

    #[test]
    fn a_device_is_found_by_name_id_or_prefix() {
        let listed = json!([
            {"id": "aaaa-1", "name": "Mac mini", "address": "a"},
            {"id": "bbbb-2", "name": "Laptop", "address": "b"},
            {"id": "bbbc-3", "name": "laptop", "address": "c"}
        ]);
        assert_eq!(pick_device(&listed, "mac MINI").unwrap(), "aaaa-1");
        assert_eq!(pick_device(&listed, "bbbb-2").unwrap(), "bbbb-2");
        assert_eq!(pick_device(&listed, "aaa").unwrap(), "aaaa-1");
        assert!(pick_device(&listed, "laptop").is_err());
        assert!(pick_device(&listed, "bbb").is_err());
        let error = pick_device(&listed, "nas").unwrap_err();
        assert!(error.message.contains("Mac mini"), "{}", error.message);
        assert!(pick_device(&json!([]), "nas").is_err());
    }

    #[test]
    fn a_container_row_reads_as_container() {
        let listed = json!({"linux": {"computers": [
            {"kind": "container", "state": "running",
             "configuration": {"id": "c", "name": "box", "runtime": "container"}},
            {"kind": "linux", "state": "running", "configuration": {"id": "l", "name": "vm"}}
        ]}});
        let computers = computers_from(&listed);
        assert_eq!(computers[0].kind, "container");
        assert_eq!(computers[0].to_json()["kind"], "container");
        assert_eq!(computers[1].kind, "linux");
    }

    #[test]
    fn only_an_engine_that_supports_macos_computers_is_a_mac() {
        assert!(macos_supported(&json!({"macos": {"supported": true}})));
        assert!(!macos_supported(&json!({"macos": {"supported": false}})));
        assert!(!macos_supported(&json!({"linux": {}})));
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
