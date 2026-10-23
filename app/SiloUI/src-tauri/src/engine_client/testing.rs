//! A fake `engine.sock` server for command-forwarding tests: it answers the handshake,
//! records every request and answers each one through a closure.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{json, Value};
use silo_api::{
    methods::{common::EmptyParams, requests, MethodName},
    transport::ClientMetadata,
};
use silo_client::{read_frame, write_frame};
use tokio::{
    net::UnixListener,
    sync::{broadcast, Notify},
};

use super::{
    launcher::{LaunchError, Launcher},
    EngineClient, EventSink,
};

pub(crate) type Handler =
    Box<dyn FnMut(MethodName, &Value) -> Result<Value, (&'static str, String)> + Send>;

struct Shared {
    capabilities: Mutex<Vec<MethodName>>,
    handler: Mutex<Handler>,
    requests: Mutex<Vec<(MethodName, Value)>>,
    events: broadcast::Sender<Value>,
    kill: Notify,
    connections: Mutex<u32>,
}

pub(crate) struct FakeEngine {
    pub socket: PathBuf,
    _directory: tempfile::TempDir,
    shared: Arc<Shared>,
    acceptor: tokio::task::JoinHandle<()>,
}

impl FakeEngine {
    pub(crate) fn start(
        handler: impl FnMut(MethodName, &Value) -> Result<Value, (&'static str, String)>
            + Send
            + 'static,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("engine.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let shared = Arc::new(Shared {
            // Like a complete engine: every canonical method is advertised.
            capabilities: Mutex::new(
                silo_api::methods::method_registry()
                    .iter()
                    .map(|descriptor| descriptor.method)
                    .collect(),
            ),
            handler: Mutex::new(Box::new(handler)),
            requests: Mutex::new(Vec::new()),
            events: broadcast::channel(64).0,
            kill: Notify::new(),
            connections: Mutex::new(0),
        });
        let accept = shared.clone();
        let acceptor = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                *accept.connections.lock().unwrap() += 1;
                tokio::spawn(serve(stream, accept.clone()));
            }
        });
        Self {
            socket,
            _directory: directory,
            shared,
            acceptor,
        }
    }

    /// Every request received so far, `engine.handshake` excluded.
    pub(crate) fn requests(&self) -> Vec<(MethodName, Value)> {
        self.shared.requests.lock().unwrap().clone()
    }

    pub(crate) fn calls(&self, method: MethodName) -> Vec<Value> {
        self.requests()
            .into_iter()
            .filter(|(name, _)| *name == method)
            .map(|(_, params)| params)
            .collect()
    }

    pub(crate) fn push_event(&self, event: &str, payload: Value) {
        let _ = self.shared.events.send(json!({
            "event": event, "subscription": "sub", "deviceId": "fake-device", "seq": 1,
            "payload": payload,
        }));
    }

    pub(crate) fn drop_connections(&self) {
        self.shared.kill.notify_waiters();
    }

    /// Restricts what the engine advertises in its handshake (for later connections).
    pub(crate) fn advertise(&self, methods: &[MethodName]) {
        *self.shared.capabilities.lock().unwrap() = methods.to_vec();
    }

    /// The engine exits: connections end and nothing listens any more.
    pub(crate) fn stop(&self) {
        self.acceptor.abort();
        let _ = std::fs::remove_file(&self.socket);
        self.drop_connections();
    }

    pub(crate) fn connections(&self) -> u32 {
        *self.shared.connections.lock().unwrap()
    }
}

async fn serve(stream: tokio::net::UnixStream, shared: Arc<Shared>) {
    let (mut reader, writer) = tokio::io::split(stream);
    let writer = Arc::new(tokio::sync::Mutex::new(writer));
    let Ok(Some(first)) = read_frame::<_, Value>(&mut reader).await else {
        return;
    };
    let capabilities: Vec<&str> = shared
        .capabilities
        .lock()
        .unwrap()
        .iter()
        .map(|method| method.name())
        .collect();
    let reply = json!({"id": first["id"], "result": {
        "deviceId": "fake-device", "name": "Fake Mac", "protocol": silo_api::transport::PROTOCOL_VERSION,
        "capabilities": capabilities, "clientId": "fake-client"}});
    if write_frame(&mut *writer.lock().await, &reply)
        .await
        .is_err()
    {
        return;
    }
    let mut events = shared.events.subscribe();
    let pusher = writer.clone();
    let push = tokio::spawn(async move {
        while let Ok(frame) = events.recv().await {
            if write_frame(&mut *pusher.lock().await, &frame)
                .await
                .is_err()
            {
                return;
            }
        }
    });
    loop {
        tokio::select! {
            _ = shared.kill.notified() => break,
            frame = read_frame::<_, Value>(&mut reader) => {
                let Ok(Some(request)) = frame else { break };
                let name = request["method"].as_str().unwrap_or_default();
                let Some(method) = MethodName::from_name(name) else {
                    panic!("the fake engine received {name}, which is not in the registry");
                };
                let params = request["params"].clone();
                shared.requests.lock().unwrap().push((method, params.clone()));
                let result = (shared.handler.lock().unwrap())(method, &params);
                let reply = match result {
                    Ok(result) => json!({"id": request["id"], "result": result}),
                    Err((code, message)) => json!({"id": request["id"], "error": {"code": code, "message": message}}),
                };
                if write_frame(&mut *writer.lock().await, &reply).await.is_err() {
                    break;
                }
            }
        }
    }
    push.abort();
}

pub(crate) struct ReadyLauncher;

impl Launcher for ReadyLauncher {
    fn ensure_running(&self) -> Result<(), LaunchError> {
        Ok(())
    }
}

pub(crate) struct FailingLauncher(pub &'static str);

impl Launcher for FailingLauncher {
    fn ensure_running(&self) -> Result<(), LaunchError> {
        Err(LaunchError {
            message: self.0.to_owned(),
            requires_approval: false,
        })
    }
}

#[derive(Default)]
pub(crate) struct RecordingSink {
    pub events: Mutex<Vec<Value>>,
    pub connections: Mutex<Vec<bool>>,
}

impl EventSink for RecordingSink {
    fn event(&self, frame: &Value) {
        self.events.lock().unwrap().push(frame.clone());
    }

    fn connection(&self, connected: bool, _message: Option<&str>) {
        self.connections.lock().unwrap().push(connected);
    }
}

pub(crate) fn metadata() -> ClientMetadata {
    ClientMetadata {
        name: "test".into(),
        version: "0".into(),
    }
}

/// A client wired to `fake`, connected and subscribed.
pub(crate) async fn connected_client(fake: &FakeEngine) -> (Arc<EngineClient>, Arc<RecordingSink>) {
    let sink = Arc::new(RecordingSink::default());
    let client = EngineClient::new(
        fake.socket.clone(),
        metadata(),
        Arc::new(ReadyLauncher),
        sink.clone(),
    )
    .with_backoff(Duration::from_millis(20));
    tokio::spawn(client.clone().run());
    // The first request waits for the connection; subscription happens before it is shared.
    client
        .call(requests::engine_state(EmptyParams {}))
        .await
        .unwrap();
    (client, sink)
}
