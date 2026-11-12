//! A fake `engine.sock`: answers the handshake, records requests and answers each through
//! a closure.
#![allow(dead_code)]

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use serde_json::{json, Value};
use silo_client::{read_frame, write_frame};
use tokio::net::UnixListener;

pub type Handler = Box<dyn FnMut(&str, &Value) -> Result<Value, (&'static str, String)> + Send>;

pub struct FakeEngine {
    pub socket: PathBuf,
    _directory: tempfile::TempDir,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    accepted: Arc<AtomicUsize>,
    open: Arc<AtomicUsize>,
}

impl FakeEngine {
    pub fn start(
        handler: impl FnMut(&str, &Value) -> Result<Value, (&'static str, String)> + Send + 'static,
    ) -> Self {
        Self::start_with(&["device.routing.v1"], handler)
    }

    /// An engine that advertises exactly these capabilities in its handshake.
    pub fn start_with(
        capabilities: &[&str],
        handler: impl FnMut(&str, &Value) -> Result<Value, (&'static str, String)> + Send + 'static,
    ) -> Self {
        let capabilities: Vec<String> = capabilities.iter().map(|c| (*c).to_owned()).collect();
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("engine.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler: Arc<Mutex<Handler>> = Arc::new(Mutex::new(Box::new(handler)));
        let log = requests.clone();
        let accepted = Arc::new(AtomicUsize::new(0));
        let open = Arc::new(AtomicUsize::new(0));
        let (counted, live) = (accepted.clone(), open.clone());
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                counted.fetch_add(1, Ordering::SeqCst);
                live.fetch_add(1, Ordering::SeqCst);
                let (handler, log, live) = (handler.clone(), log.clone(), live.clone());
                let capabilities = capabilities.clone();
                tokio::spawn(async move {
                    serve(stream, handler, log, capabilities).await;
                    live.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Self {
            socket,
            _directory: directory,
            requests,
            accepted,
            open,
        }
    }

    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    pub fn open(&self) -> usize {
        self.open.load(Ordering::SeqCst)
    }

    pub fn calls(&self, method: &str) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == method)
            .map(|(_, params)| params.clone())
            .collect()
    }
}

async fn serve(
    stream: tokio::net::UnixStream,
    handler: Arc<Mutex<Handler>>,
    log: Arc<Mutex<Vec<(String, Value)>>>,
    capabilities: Vec<String>,
) {
    let (mut reader, mut writer) = tokio::io::split(stream);
    let Ok(Some(first)) = read_frame::<_, Value>(&mut reader).await else {
        return;
    };
    let reply = json!({"id": first["id"], "result": {
        "deviceId": "fake", "name": "Fake Mac",
        "protocol": silo_api::transport::PROTOCOL_VERSION,
        "capabilities": capabilities, "clientId": "fake-client"}});
    if write_frame(&mut writer, &reply).await.is_err() {
        return;
    }
    while let Ok(Some(request)) = read_frame::<_, Value>(&mut reader).await {
        let method = request["method"].as_str().unwrap_or_default().to_owned();
        let params = request["params"].clone();
        log.lock().unwrap().push((method.clone(), params.clone()));
        let result = (handler.lock().unwrap())(&method, &params);
        if matches!(&result, Err(("__drop", _))) {
            return;
        }
        let reply = match result {
            Ok(result) => json!({"id": request["id"], "result": result}),
            Err((code, message)) => {
                // A message that is a JSON object may carry `details`, as an
                // `approval_required` error does.
                match serde_json::from_str::<Value>(&message) {
                    Ok(body) if body.is_object() => json!({"id": request["id"], "error": {
                        "code": code, "message": body["message"], "details": body["details"]}}),
                    _ => json!({"id": request["id"], "error": {"code": code, "message": message}}),
                }
            }
        };
        if write_frame(&mut writer, &reply).await.is_err() {
            return;
        }
    }
}

/// A `computers.list` reply with these Linux computers (name, state).
pub fn listing(computers: &[(&str, &str)]) -> Value {
    listing_without_desktop(computers, &[])
}

/// As [`listing`], with the desktop of the computers named in `off` turned off.
pub fn listing_without_desktop(computers: &[(&str, &str)], off: &[&str]) -> Value {
    let linux: Vec<Value> = computers
        .iter()
        .map(|(name, state)| {
            json!({
                "configuration": {
                    "id": format!("id-{name}"), "name": name, "cpus": 2, "maxCPUs": 2,
                    "memoryGiB": 4, "maxMemoryGiB": 4,
                    "workspaceStorageGiB": 10, "runtimeStorageGiB": 10,
                    "desktop": {"startWithComputer": !off.contains(name), "builtIn": true}
                },
                "state": state, "stateDetail": "",
                "checkpoints": [{"id": "cp1", "name": "base", "kind": "manual", "createdAt": "now"}]
            })
        })
        .collect();
    json!({"linux": {"computers": linux}, "macos": {"computers": []}})
}

pub fn finished(result: Value) -> Value {
    json!({"jobId": "job:1", "state": "finished", "outcome": {"result": result}})
}
