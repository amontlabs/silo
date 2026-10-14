//! Silo.app as a client of `silod` (95 A-1).
//!
//! The app keeps one connection to `engine.sock`, reconnects when it drops, relays the
//! engine's events to the webview, and forwards Tauri commands to canonical engine methods.
//! No engine runs in this process: every operation here is a request over the socket.

pub(crate) mod launcher;
pub(crate) mod relay;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use serde_json::{json, Value};
use silo_api::transport::ClientMetadata;
use silo_client::{Client, ClientError, ConnectOptions, RequestOptions};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{watch, Notify};

use launcher::Launcher;

/// The engine refuses request ids after 65,536 per connection; reconnect well before.
const REQUEST_ID_BUDGET: u64 = 60_000;
/// How long a command waits for the engine to be reachable before it reports failure.
const CONNECT_PATIENCE: Duration = Duration::from_secs(20);
const JOB_POLL: Duration = Duration::from_millis(750);

/// An engine failure in the shape the UI's bridge-error contract parses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct EngineError {
    pub code: String,
    pub message: String,
}

impl EngineError {
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            code: "internal".to_owned(),
            message: message.into(),
        }
    }

    pub(crate) fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: "unsupported_remote_operation".to_owned(),
            message: message.into(),
        }
    }

    pub(crate) fn is(&self, code: &str) -> bool {
        self.code == code
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl From<EngineError> for String {
    fn from(error: EngineError) -> Self {
        error.message
    }
}

impl From<ClientError> for EngineError {
    fn from(error: ClientError) -> Self {
        match error {
            ClientError::Remote(remote) => {
                // The UI's contract knows eight codes; the engine's newer ones are plain failures.
                let code = serde_json::to_value(remote.code)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .filter(|code| UI_CODES.contains(&code.as_str()))
                    .unwrap_or_else(|| "internal".to_owned());
                Self {
                    code,
                    message: remote.message,
                }
            }
            ClientError::Disconnected => {
                Self::internal("Silo's engine connection ended. Silo is reconnecting; retry.")
            }
            ClientError::TimedOut => Self::internal("The engine did not answer in time."),
            other => Self::internal(other.to_string()),
        }
    }
}

const UI_CODES: &[&str] = &[
    "update_in_progress",
    "unsupported_remote_operation",
    "cancelled",
    "already_queued",
    "busy",
    "not_found",
    "incompatible_version",
    "internal",
];

/// Where connection changes and engine events go. The app emits to the webview.
pub(crate) trait EventSink: Send + Sync + 'static {
    fn event(&self, frame: &Value);
    fn connection(&self, connected: bool, message: Option<&str>);
}

pub(crate) struct TauriSink(pub AppHandle);

impl EventSink for TauriSink {
    fn event(&self, frame: &Value) {
        let _ = self.0.emit(relay::ENGINE_EVENT, frame);
        for (name, payload) in relay::legacy_events(frame) {
            let _ = self.0.emit(name, payload);
        }
    }

    fn connection(&self, connected: bool, message: Option<&str>) {
        let _ = self
            .0
            .emit(relay::ENGINE_CONNECTION, relay::connection_payload(connected, message));
        // A fresh connection means state may have changed while it was down.
        if connected {
            let _ = self.0.emit(relay::APPLICATION_STATE, Value::Null);
        }
    }
}

#[derive(Clone)]
struct Connected {
    client: Client,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Connecting,
    Connected,
    Unavailable(String),
}

pub(crate) struct EngineClient {
    socket: PathBuf,
    metadata: ClientMetadata,
    launcher: Arc<dyn Launcher>,
    sink: Arc<dyn EventSink>,
    current: watch::Sender<Option<Connected>>,
    status: Mutex<Status>,
    finished: Notify,
    backoff: Duration,
}

impl EngineClient {
    pub(crate) fn new(
        socket: PathBuf,
        metadata: ClientMetadata,
        launcher: Arc<dyn Launcher>,
        sink: Arc<dyn EventSink>,
    ) -> Arc<Self> {
        Arc::new(Self {
            socket,
            metadata,
            launcher,
            sink,
            current: watch::channel(None).0,
            status: Mutex::new(Status::Connecting),
            finished: Notify::new(),
            backoff: Duration::from_millis(500),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_backoff(mut self: Arc<Self>, backoff: Duration) -> Arc<Self> {
        Arc::get_mut(&mut self).expect("not shared yet").backoff = backoff;
        self
    }

    pub(crate) fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    fn set_status(&self, status: Status) {
        *self.status.lock().unwrap() = status;
    }

    /// Keeps a connection to the engine for the life of the app: ensure it is running,
    /// connect, subscribe, relay events, and start over when the connection ends.
    pub(crate) async fn run(self: Arc<Self>) {
        let mut delay = self.backoff;
        loop {
            self.set_status(Status::Connecting);
            match self.connect_once().await {
                Ok((connected, mut feed)) => {
                    delay = self.backoff;
                    self.set_status(Status::Connected);
                    self.current.send_replace(Some(connected.clone()));
                    self.sink.connection(true, None);
                    while let Some(frame) = feed.next().await {
                        if frame.get("event").and_then(Value::as_str)
                            == Some("operations.job.finished")
                        {
                            self.finished.notify_waiters();
                        }
                        self.sink.event(&frame);
                    }
                    self.current.send_replace(None);
                    // Wake job waiters so they notice the loss and wait for the next connection.
                    self.finished.notify_waiters();
                    let message = "Silo's engine connection ended.".to_owned();
                    self.set_status(Status::Unavailable(message.clone()));
                    self.sink.connection(false, Some(&message));
                }
                Err(message) => {
                    self.set_status(Status::Unavailable(message.clone()));
                    self.sink.connection(false, Some(&message));
                    delay = (delay * 2).min(Duration::from_secs(10));
                }
            }
            tokio::time::sleep(delay).await;
        }
    }

    async fn connect_once(&self) -> Result<(Connected, silo_client::EventFeed), String> {
        let launcher = self.launcher.clone();
        tokio::task::spawn_blocking(move || launcher.ensure_running())
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.message)?;
        let (client, feed) = Client::connect(
            &self.socket,
            ConnectOptions::new(self.metadata.clone()),
        )
        .await
        .map_err(|error| match error {
            ClientError::Remote(remote) => remote.message,
            other => other.to_string(),
        })?;
        client
            .request("events.subscribe", json!({"events": relay::SUBSCRIBED}))
            .await
            .map_err(|error| EngineError::from(error).message)?;
        Ok((Connected { client }, feed))
    }

    /// A live connection, waiting up to `CONNECT_PATIENCE` for the supervisor to make one.
    async fn connected(&self) -> Result<Connected, EngineError> {
        let waited = tokio::time::timeout(CONNECT_PATIENCE, async {
            let mut receiver = self.current.subscribe();
            let connection = receiver
                .wait_for(|connection| {
                    connection
                        .as_ref()
                        .is_some_and(|connected| !connected.client.is_closed())
                })
                .await
                .ok()?;
            connection.clone()
        })
        .await;
        match waited {
            Ok(Some(connected)) => Ok(connected),
            _ => Err(EngineError::internal(match self.status() {
                Status::Unavailable(message) => message,
                _ => "Silo's engine is not reachable yet. Retry in a moment.".to_owned(),
            })),
        }
    }

    pub(crate) async fn call(&self, method: &str, params: Value) -> Result<Value, EngineError> {
        self.call_with(method, params, RequestOptions::default()).await
    }

    pub(crate) async fn call_with(
        &self,
        method: &str,
        params: Value,
        options: RequestOptions,
    ) -> Result<Value, EngineError> {
        let connected = self.connected().await?;
        if connected.client.requests_sent() >= REQUEST_ID_BUDGET {
            // Start a new session before the engine refuses more ids on this one.
            connected.client.close().await;
            return Err(EngineError::internal("Silo is renewing its engine connection; retry."));
        }
        Ok(connected.client.request_with(method, params, options).await?)
    }

    /// Starts a job method and waits for its terminal outcome, then acknowledges it.
    /// Losing the connection while waiting never invents a failure: the job continues in
    /// the engine, so the wait resumes on the next connection.
    pub(crate) async fn run_job(&self, method: &str, params: Value) -> Result<Value, EngineError> {
        let started = self.call(method, params).await?;
        let job_id = started
            .get("jobId")
            .and_then(Value::as_str)
            .ok_or_else(|| EngineError::internal("The engine did not accept the job."))?
            .to_owned();
        self.wait_job(&job_id).await
    }

    pub(crate) async fn wait_job(&self, job_id: &str) -> Result<Value, EngineError> {
        loop {
            let wake = self.finished.notified();
            tokio::pin!(wake);
            wake.as_mut().enable();
            match self.job_outcome(job_id).await {
                Ok(Some(outcome)) => {
                    // The outcome is presented to the caller; the engine may now drop it.
                    let _ = self
                        .call("operations.job.acknowledge", json!({"jobId": job_id}))
                        .await;
                    return outcome;
                }
                Ok(None) => {}
                // A lost connection is not a failed job; wait for the next connection.
                Err(error) if error.message.contains("connection") || error.message.contains("reachable") => {}
                Err(error) => return Err(error),
            }
            tokio::select! {
                _ = &mut wake => {}
                _ = tokio::time::sleep(JOB_POLL) => {}
            }
        }
    }

    /// `Ok(None)` while the job runs; `Ok(Some(..))` once it has finished.
    async fn job_outcome(
        &self,
        job_id: &str,
    ) -> Result<Option<Result<Value, EngineError>>, EngineError> {
        let mut text = String::new();
        let mut offset: Option<u64> = None;
        loop {
            let mut params = json!({"jobId": job_id});
            if let Some(offset) = offset {
                params["offsetBytes"] = json!(offset);
            }
            let reply = self.call("operations.job.get", params).await?;
            if reply.get("state").and_then(Value::as_str) != Some("finished") {
                return Ok(None);
            }
            if let Some(chunk) = reply.get("outcomeChunk") {
                text.push_str(chunk.get("data").and_then(Value::as_str).unwrap_or_default());
                match chunk.get("nextOffsetBytes").and_then(Value::as_u64) {
                    Some(next) => {
                        offset = Some(next);
                        continue;
                    }
                    None => {
                        let outcome: Value = serde_json::from_str(&text).map_err(|error| {
                            EngineError::internal(format!("The job outcome was unreadable: {error}"))
                        })?;
                        return Ok(Some(outcome_result(&outcome)));
                    }
                }
            }
            let outcome = reply.get("outcome").cloned().unwrap_or(Value::Null);
            return Ok(Some(outcome_result(&outcome)));
        }
    }
}

fn outcome_result(outcome: &Value) -> Result<Value, EngineError> {
    if let Some(error) = outcome.get("error").filter(|error| !error.is_null()) {
        let code = error
            .get("code")
            .and_then(Value::as_str)
            .filter(|code| UI_CODES.contains(code))
            .unwrap_or("internal");
        return Err(EngineError {
            code: code.to_owned(),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("The operation failed.")
                .to_owned(),
        });
    }
    Ok(outcome.get("result").cloned().unwrap_or(Value::Null))
}

/// The managed state handle commands receive.
pub(crate) type Engine<'a> = tauri::State<'a, Arc<EngineClient>>;

/// The shared client for code that has only an `AppHandle`.
pub(crate) fn of(app: &AppHandle) -> Arc<EngineClient> {
    app.state::<Arc<EngineClient>>().inner().clone()
}

/// Builds the client for this app, registers it as managed state and starts keeping the
/// connection. Nothing here constructs or starts an engine.
pub(crate) fn install(app: &AppHandle) -> Result<(), String> {
    let channel = silo_client::channel::current();
    let home = app.path().home_dir().map_err(|error| error.to_string())?;
    let socket = channel.engine_socket(&home);
    let launcher = production_launcher(app, &home);
    let metadata = ClientMetadata {
        name: channel.product_name().to_owned(),
        version: app.package_info().version.to_string(),
    };
    let client = EngineClient::new(socket, metadata, launcher, Arc::new(TauriSink(app.clone())));
    app.manage(client.clone());
    tauri::async_runtime::spawn(client.run());
    Ok(())
}

fn production_launcher(app: &AppHandle, home: &std::path::Path) -> Arc<dyn Launcher> {
    let channel = silo_client::channel::current();
    let resources = app.path().resource_dir().unwrap_or_default();
    #[cfg(target_os = "macos")]
    let service: Arc<dyn launcher::ServiceControl> = Arc::new(launcher::SmAppService {
        plist_name: channel.launch_agent_plist(),
    });
    #[cfg(not(target_os = "macos"))]
    let service: Arc<dyn launcher::ServiceControl> = Arc::new(launcher::UnsupportedService);
    Arc::new(launcher::BundleLauncher {
        socket: channel.engine_socket(home),
        owner_record: channel.engine_owner_record(home),
        bundled: launcher::bundled_manifest(&resources),
        service,
        product: channel.product_name(),
        patience: Duration::from_secs(15),
    })
}

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
