use std::sync::Arc;

use serde_json::Value;
use silo_api::transport::ResponseEnvelope;
use silo_engine::context::{EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl};
use silod::{
    context::SilodContext,
    dispatcher::Dispatcher,
    framing::{read_json_frame, write_json_frame},
    identity::LocalDeviceIdentity,
    socket::{EngineServer, SystemPeerEuidProvider},
};
use tokio::{
    net::UnixStream,
    sync::oneshot,
    time::{timeout, Duration},
};

struct NoopMainThread;
impl MainThread for NoopMainThread {
    fn run(&self, _work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
        Ok(())
    }
}

struct NoopProcess;
impl ProcessControl for NoopProcess {
    fn exit(&self, _code: i32) {}

    fn restart(&self) -> ! {
        panic!("restart is not expected in a socket integration test")
    }
}

#[tokio::test]
async fn temporary_socket_accepts_a_p1_handshake_and_state_request() {
    let temporary = tempfile::tempdir().unwrap();
    let socket_path = temporary.path().join(".silo/engine.sock");
    let euid = unsafe { libc::geteuid() };
    let context = Arc::new(SilodContext::new(
        EnginePaths::default(),
        EngineIdentity {
            channel: silo_engine::channel::Channel::Development,
            version: "0.1.0".to_owned(),
            package_kind: PackageKind::MacApp,
        },
        LocalDeviceIdentity {
            device_id: "fixture-device".to_owned(),
            name: "Fixture Mac".to_owned(),
        },
        Arc::new(NoopMainThread),
        Arc::new(NoopProcess),
    ));
    let server = EngineServer::bind(
        socket_path.clone(),
        euid,
        Arc::new(SystemPeerEuidProvider),
        Arc::new(Dispatcher::core(context)),
    )
    .unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server_task = tokio::spawn(server.run(shutdown_rx));

    let mut client = timeout(Duration::from_secs(2), UnixStream::connect(&socket_path))
        .await
        .unwrap()
        .unwrap();
    write_json_frame(
        &mut client,
        &serde_json::json!({
            "id": "handshake-1",
            "method": "engine.handshake",
            "params": {
                "protocol": { "min": 6, "max": 6 },
                "client": { "name": "integration-test", "version": "1.0" }
            }
        }),
    )
    .await
    .unwrap();
    let handshake: ResponseEnvelope<Value> =
        timeout(Duration::from_secs(2), read_json_frame(&mut client))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    assert!(matches!(
        handshake,
        ResponseEnvelope::Success { id, result }
            if id == "handshake-1"
                && result["deviceId"] == "fixture-device"
                && result["protocol"] == 6
                && result["clientId"].as_str().is_some()
                && result["capabilities"] == serde_json::json!(
                    ["engine.handshake", "engine.state", "guest.ssh"]
                )
    ));

    write_json_frame(
        &mut client,
        &serde_json::json!({ "id": "state-1", "method": "engine.state", "params": {} }),
    )
    .await
    .unwrap();
    let state: ResponseEnvelope<Value> =
        timeout(Duration::from_secs(2), read_json_frame(&mut client))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    assert!(matches!(
        state,
        ResponseEnvelope::Success { id, result }
            if id == "state-1"
                && result["shuttingDown"] == false
                && result["phase"] == "idle"
                && result["updatePending"] == false
    ));

    write_json_frame(
        &mut client,
        &serde_json::json!({ "id": "state-2", "method": "engine.state", "params": {} }),
    )
    .await
    .unwrap();
    let state: ResponseEnvelope<Value> =
        timeout(Duration::from_secs(2), read_json_frame(&mut client))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    assert!(matches!(
        state,
        ResponseEnvelope::Success { id, .. } if id == "state-2"
    ));

    drop(client);
    let _ = shutdown_tx.send(());
    server_task.await.unwrap();
}
