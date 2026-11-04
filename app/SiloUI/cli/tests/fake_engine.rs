//! A fake `engine.sock`: answers the handshake, records requests and answers each through
//! a closure.
#![allow(dead_code)]

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde_json::{json, Value};
use silo_client::{read_frame, write_frame};
use tokio::net::UnixListener;

pub type Handler = Box<dyn FnMut(&str, &Value) -> Result<Value, (&'static str, String)> + Send>;

pub struct FakeEngine {
    pub socket: PathBuf,
    _directory: tempfile::TempDir,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
}

impl FakeEngine {
    pub fn start(
        handler: impl FnMut(&str, &Value) -> Result<Value, (&'static str, String)> + Send + 'static,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("engine.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler: Arc<Mutex<Handler>> = Arc::new(Mutex::new(Box::new(handler)));
        let log = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(serve(stream, handler.clone(), log.clone()));
            }
        });
        Self {
            socket,
            _directory: directory,
            requests,
        }
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
) {
    let (mut reader, mut writer) = tokio::io::split(stream);
    let Ok(Some(first)) = read_frame::<_, Value>(&mut reader).await else {
        return;
    };
    let reply = json!({"id": first["id"], "result": {
        "deviceId": "fake", "name": "Fake Mac",
        "protocol": silo_api::transport::PROTOCOL_VERSION,
        "capabilities": [], "clientId": "fake-client"}});
    if write_frame(&mut writer, &reply).await.is_err() {
        return;
    }
    while let Ok(Some(request)) = read_frame::<_, Value>(&mut reader).await {
        let method = request["method"].as_str().unwrap_or_default().to_owned();
        let params = request["params"].clone();
        log.lock().unwrap().push((method.clone(), params.clone()));
        let result = (handler.lock().unwrap())(&method, &params);
        let reply = match result {
            Ok(result) => json!({"id": request["id"], "result": result}),
            Err((code, message)) => {
                json!({"id": request["id"], "error": {"code": code, "message": message}})
            }
        };
        if write_frame(&mut writer, &reply).await.is_err() {
            return;
        }
    }
}

/// A `computers.list` reply with these Linux computers (name, state).
pub fn listing(computers: &[(&str, &str)]) -> Value {
    let linux: Vec<Value> = computers
        .iter()
        .map(|(name, state)| {
            json!({
                "configuration": {
                    "id": format!("id-{name}"), "name": name, "cpus": 2, "maxCPUs": 2,
                    "memoryGiB": 4, "maxMemoryGiB": 4,
                    "workspaceStorageGiB": 10, "runtimeStorageGiB": 10
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
