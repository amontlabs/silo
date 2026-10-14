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
        EnginePaths {
            home: temporary.path().to_path_buf(),
            app_config: temporary.path().join("config"),
            ..Default::default()
        },
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
    silo_engine::context::install(context.clone()).unwrap();
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
                && result["capabilities"].as_array().is_some_and(|capabilities| {
                    [
                        "engine.handshake",
                        "engine.state",
                        "events.subscribe",
                        "events.unsubscribe",
                        "notifications.list",
                        "operations.cancel",
                        "operations.job.acknowledge",
                        "operations.job.get",
                        "operations.list",
                        "computers.list",
                        "exports.state",
                        "display.show",
                    ]
                    .iter()
                    .all(|expected| capabilities.iter().any(|value| value == expected))
                })
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

    write_json_frame(
        &mut client,
        &serde_json::json!({ "id": "settings-read-1", "method": "settings.read", "params": {} }),
    )
    .await
    .unwrap();
    let settings: ResponseEnvelope<Value> =
        timeout(Duration::from_secs(2), read_json_frame(&mut client))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    assert!(matches!(
        settings,
        ResponseEnvelope::Success { id, result }
            if id == "settings-read-1"
                && result["revision"].as_u64().is_some()
                && result["settings"].is_object()
                && result["onboardingDraft"].is_null()
                && result["writeProtected"] == false
    ));

    write_json_frame(
        &mut client,
        &serde_json::json!({ "id": "disconnect-1", "method": "engine.disconnect", "params": {} }),
    )
    .await
    .unwrap();
    let disconnected: ResponseEnvelope<Value> =
        timeout(Duration::from_secs(2), read_json_frame(&mut client))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    assert!(matches!(
        disconnected,
        ResponseEnvelope::Success { id, result }
            if id == "disconnect-1" && result.is_null()
    ));
    assert!(matches!(
        timeout(
            Duration::from_secs(2),
            read_json_frame::<_, Value>(&mut client)
        )
        .await
        .unwrap(),
        Ok(None)
    ));

    let _ = shutdown_tx.send(());
    server_task.await.unwrap();
}
