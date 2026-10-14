//! The checkpoints/exports/migration/preparation/device domain over a real `engine.sock`:
//! handshake, then domain requests, a job started with a method and read back with
//! `operations.job.get`, and the canonical error codes.

use std::sync::Arc;

use serde_json::{json, Value};
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
    time::{sleep, timeout, Duration},
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

async fn request(
    client: &mut UnixStream,
    id: &str,
    method: &str,
    params: Value,
) -> ResponseEnvelope<Value> {
    write_json_frame(
        client,
        &json!({"id": id, "method": method, "params": params}),
    )
    .await
    .unwrap();
    timeout(Duration::from_secs(10), read_json_frame(client))
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}

fn success(response: ResponseEnvelope<Value>) -> Value {
    match response {
        ResponseEnvelope::Success { result, .. } => result,
        ResponseEnvelope::Error { error, .. } => panic!("unexpected error: {error:?}"),
    }
}

#[tokio::test]
async fn the_domain_answers_over_the_socket_with_canonical_results_jobs_and_errors() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let paths = EnginePaths {
        app_data: root.join("data"),
        app_config: root.join("config"),
        app_cache: root.join("cache"),
        app_log: root.join("log"),
        resources: root.join("resources"),
        home: root.join("home"),
    };
    std::fs::create_dir_all(&paths.app_data).unwrap();
    std::fs::create_dir_all(&paths.home).unwrap();
    let exports = root.join("exports");
    std::fs::create_dir_all(&exports).unwrap();

    let socket_path = root.join(".silo/engine.sock");
    let context = Arc::new(SilodContext::new(
        paths,
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
        unsafe { libc::geteuid() },
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

    let handshake = request(
        &mut client,
        "h",
        "engine.handshake",
        json!({"protocol": {"min": 6, "max": 6}, "client": {"name": "integration-test", "version": "1.0"}}),
    )
    .await;
    assert_eq!(success(handshake)["deviceId"], "fixture-device");

    // A command, then the query that shows its effect.
    let set = success(
        request(
            &mut client,
            "1",
            "exports.destination.set",
            json!({"path": exports}),
        )
        .await,
    );
    let canonical = std::fs::canonicalize(&exports).unwrap();
    assert_eq!(set["destination"], canonical.to_str().unwrap());
    let state = success(request(&mut client, "2", "exports.state", json!({})).await);
    assert_eq!(state["destination"], canonical.to_str().unwrap());
    assert_eq!(state["availability"], "available");

    // Queries of the other domain areas.
    let migration = success(request(&mut client, "3", "migration.state", json!({})).await);
    assert_eq!(migration["status"], "not-required");
    let facts = success(request(&mut client, "4", "device.facts", json!({})).await);
    assert!(facts["remoteLogin"].is_string());
    let preparation = success(request(&mut client, "5", "preparation.status", json!({})).await);
    assert!(preparation["image"]["state"].is_string());

    // A job: the method answers at once with its id, and the outcome is read back from the
    // durable registry, then acknowledged.
    let started = success(
        request(
            &mut client,
            "6",
            "exports.inspect",
            json!({"archivePath": root.join("missing.silo-backup"), "requestId": "socket-1"}),
        )
        .await,
    );
    assert_eq!(started, json!({"jobId": "inspect:socket-1"}));
    let mut outcome = Value::Null;
    for attempt in 0..100 {
        let lookup = success(
            request(
                &mut client,
                &format!("get-{attempt}"),
                "operations.job.get",
                json!({"jobId": "inspect:socket-1"}),
            )
            .await,
        );
        if lookup["state"] == "finished" {
            outcome = lookup["outcome"].clone();
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(outcome["result"]["valid"], false);
    success(
        request(
            &mut client,
            "7",
            "operations.job.acknowledge",
            json!({"jobId": "inspect:socket-1"}),
        )
        .await,
    );

    // Errors carry canonical codes: an unknown computer, and another device.
    let missing = request(
        &mut client,
        "8",
        "checkpoints.usage",
        json!({"computerId": "22222222-2222-4222-8222-222222222222"}),
    )
    .await;
    assert!(matches!(
        missing,
        ResponseEnvelope::Error { error, .. } if error.code == silo_api::jobs::ErrorCode::NotFound
    ));
    let remote = request(
        &mut client,
        "9",
        "checkpoints.create",
        json!({"computerId": "x", "deviceId": "another-device", "name": "n"}),
    )
    .await;
    // Another device is reached through its owner; one this engine is not connected to is
    // reported as such.
    assert!(matches!(
        remote,
        ResponseEnvelope::Error { error, .. }
            if error.code == silo_api::jobs::ErrorCode::NotFound
                && error.message.contains("no longer connected")
    ));

    drop(client);
    let _ = shutdown_tx.send(());
    server_task.await.unwrap();
}
