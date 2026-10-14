//! The calls Silo.app's client makes, against the real socket server: the event selectors it
//! subscribes with, and the settings and update shapes its commands read.

use std::sync::Arc;

use serde_json::json;
use silo_api::transport::ClientMetadata;
use silo_client::{Client, ConnectOptions, APP_EVENTS};
use silo_engine::context::{EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl};
use silod::{
    context::SilodContext,
    dispatcher::Dispatcher,
    identity::LocalDeviceIdentity,
    socket::{EngineServer, SystemPeerEuidProvider},
};
use tokio::{
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
async fn the_clients_subscription_settings_and_update_calls_match_the_engine() {
    let temporary = tempfile::tempdir().unwrap();
    let socket_path = temporary.path().join(".silo/engine.sock");
    let context = Arc::new(SilodContext::new(
        EnginePaths {
            home: temporary.path().to_path_buf(),
            app_data: temporary.path().join("data"),
            app_config: temporary.path().join("config"),
            ..Default::default()
        },
        EngineIdentity {
            channel: silo_engine::channel::Channel::Development,
            version: "1.0.0".to_owned(),
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
        unsafe { libc::geteuid() },
        Arc::new(SystemPeerEuidProvider),
        Arc::new(Dispatcher::core(context)),
    )
    .unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server_task = tokio::spawn(server.run(shutdown_rx));

    let (client, mut events) = Client::connect(
        &socket_path,
        ConnectOptions::new(ClientMetadata {
            name: "Silo".into(),
            version: "0.11.1".into(),
        }),
    )
    .await
    .unwrap();

    // The one subscription the app makes is accepted, whatever the engine's catalogue holds.
    let subscription = client
        .request("events.subscribe", json!({"events": APP_EVENTS}))
        .await
        .unwrap();
    assert!(subscription["subscription"].as_str().is_some());

    // The settings snapshot is what the webview's store parses.
    let snapshot = client.request("settings.read", json!({})).await.unwrap();
    assert!(snapshot["revision"].as_u64().is_some());
    assert!(snapshot["settings"].is_object());
    assert!(snapshot.get("saveError").is_some());
    let updated = client
        .request("settings.update", json!({"patch": {"reduceMotion": true}}))
        .await
        .unwrap();
    assert_eq!(updated["settings"]["reduceMotion"], true);
    assert!(updated["revision"].as_u64() > snapshot["revision"].as_u64());
    // The change reaches the subscribed client as an event, with the snapshot as payload.
    let changed = timeout(Duration::from_secs(5), async {
        loop {
            let frame = events.next().await.expect("the connection stays open");
            if frame["event"] == "settings.changed" {
                return frame;
            }
        }
    })
    .await
    .expect("settings.changed arrives");
    assert_eq!(changed["payload"]["settings"]["reduceMotion"], true);
    client.request("settings.flush", json!({})).await.unwrap();

    // Update state carries the fields the update panel renders.
    let state = client.request("updates.state", json!({})).await.unwrap();
    for field in ["phase", "currentVersion", "automaticChecks", "releaseUrl"] {
        assert!(state.get(field).is_some(), "updates.state lacks {field}");
    }

    // The queue the app shows and cancels from.
    let queue = client.request("operations.list", json!({})).await.unwrap();
    assert!(queue["running"].is_array() && queue["waiting"].is_array());

    // The app's shutdown overlay reads the lifecycle phase.
    let engine = client.request("engine.state", json!({})).await.unwrap();
    assert_eq!(engine["shuttingDown"], false);

    client.close().await;
    let _ = shutdown_tx.send(());
    let _ = server_task.await;
}
