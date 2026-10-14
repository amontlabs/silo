//! The thin native client (`silo-client`) against the real socket server.

use std::sync::Arc;

use serde_json::json;
use silo_api::transport::ClientMetadata;
use silo_client::{Client, ConnectOptions};
use silo_engine::context::{EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl};
use silod::{
    context::SilodContext,
    dispatcher::Dispatcher,
    identity::LocalDeviceIdentity,
    socket::{EngineServer, SystemPeerEuidProvider},
};
use tokio::sync::oneshot;

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
async fn the_client_negotiates_requests_and_subscribes_against_silod() {
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

    let (client, _events) = Client::connect(
        &socket_path,
        ConnectOptions::new(ClientMetadata {
            name: "client-test".into(),
            version: "1".into(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(client.handshake().device_id, "fixture-device");
    assert!(client.handshake().client_id.is_some());
    assert!(client.supports("engine.state"));
    let state = client.request("engine.state", json!({})).await.unwrap();
    assert_eq!(state["shuttingDown"], false);
    let subscribed = client
        .request("events.subscribe", json!({"events": ["computers.changed"]}))
        .await
        .unwrap();
    assert!(subscribed["subscription"].as_str().is_some());
    let missing = client
        .request("operations.job.get", json!({"jobId": "job:999"}))
        .await;
    assert!(matches!(
        missing,
        Err(silo_client::ClientError::Remote(error))
            if error.code == silo_api::jobs::ErrorCode::NotFound
    ));
    client.close().await;
    let _ = shutdown_tx.send(());
    let _ = server_task.await;
}
