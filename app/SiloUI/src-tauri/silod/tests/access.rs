//! The access domain over a real `engine.sock`: handshake, requests, errors and events.

use std::{sync::Arc, time::Duration};

use serde_json::{json, Value};
use silo_engine::context::{EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl};
use silod::{
    context::SilodContext,
    dispatcher::Dispatcher,
    framing::{read_json_frame, write_json_frame},
    identity::LocalDeviceIdentity,
    socket::{EngineServer, SystemPeerEuidProvider},
};
use tokio::{net::UnixStream, sync::oneshot, time::timeout};

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

struct Client {
    stream: UnixStream,
    next: u64,
    events: Vec<Value>,
}

impl Client {
    async fn send(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = format!("request-{}", self.next);
        write_json_frame(
            &mut self.stream,
            &json!({"id": id, "method": method, "params": params}),
        )
        .await
        .unwrap();
        loop {
            let frame: Value = timeout(Duration::from_secs(20), read_json_frame(&mut self.stream))
                .await
                .expect("the engine answered in time")
                .unwrap()
                .unwrap();
            if frame.get("event").is_some() {
                self.events.push(frame);
            } else if frame["id"] == id.as_str() {
                return frame;
            }
        }
    }

    async fn ok(&mut self, method: &str, params: Value) -> Value {
        let reply = self.send(method, params).await;
        assert!(reply.get("error").is_none(), "{method} failed: {reply}");
        reply["result"].clone()
    }

    async fn error(&mut self, method: &str, params: Value) -> Value {
        let reply = self.send(method, params).await;
        assert!(
            reply.get("result").is_none(),
            "{method} should fail: {reply}"
        );
        reply["error"].clone()
    }

    async fn event(&mut self, name: &str) -> Value {
        for _ in 0..50 {
            if let Some(index) = self.events.iter().position(|event| event["event"] == name) {
                return self.events.remove(index);
            }
            let frame: Value = timeout(Duration::from_secs(5), read_json_frame(&mut self.stream))
                .await
                .expect("an event arrived")
                .unwrap()
                .unwrap();
            self.events.push(frame);
        }
        panic!("no {name} event arrived");
    }
}

#[tokio::test]
async fn access_methods_work_over_the_engine_socket() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().to_path_buf();
    let paths = EnginePaths {
        app_data: base.join("data"),
        app_config: base.join("config"),
        app_cache: base.join("cache"),
        app_log: base.join("log"),
        resources: base.join("resources"),
        home: base.join("home"),
    };
    for directory in [
        &paths.app_data,
        &paths.app_config,
        &paths.app_cache,
        &paths.app_log,
        &paths.resources,
        &paths.home,
    ] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let home = paths.home.clone();
    let context = Arc::new(SilodContext::new(
        paths,
        EngineIdentity {
            channel: silo_engine::channel::Channel::Development,
            version: "0.1.0-test".to_owned(),
            package_kind: PackageKind::MacApp,
        },
        LocalDeviceIdentity {
            device_id: "55555555-5555-4555-8555-555555555555".to_owned(),
            name: "Fixture Mac".to_owned(),
        },
        Arc::new(NoopMainThread),
        Arc::new(NoopProcess),
    ));
    silo_engine::context::install(context.clone()).unwrap();
    // The full `install` also starts the GitHub worker, which reads the system credential store.
    silod::methods::access::install_host(context.clone()).unwrap();
    silo_engine::secrets::install().unwrap();

    let socket_path = root.path().join(".silo/engine.sock");
    let server = EngineServer::bind(
        socket_path.clone(),
        unsafe { libc::geteuid() },
        Arc::new(SystemPeerEuidProvider),
        Arc::new(Dispatcher::core(context)),
    )
    .unwrap();
    let (shutdown, shutdown_receiver) = oneshot::channel();
    let server_task = tokio::spawn(server.run(shutdown_receiver));

    let stream = timeout(Duration::from_secs(2), UnixStream::connect(&socket_path))
        .await
        .unwrap()
        .unwrap();
    let mut client = Client {
        stream,
        next: 0,
        events: Vec::new(),
    };
    let handshake = client
        .ok(
            "engine.handshake",
            json!({
                "protocol": {"min": 6, "max": 6},
                "client": {"name": "access-test", "version": "1.0"},
            }),
        )
        .await;
    assert_eq!(
        handshake["deviceId"],
        "55555555-5555-4555-8555-555555555555"
    );
    client
        .ok(
            "events.subscribe",
            json!({"events": ["connections.devices.changed", "secrets.changed"]}),
        )
        .await;

    // Events from a device this engine is not connected to cannot be relayed. (A saved device
    // would start an SSH stream to its address, so this test names none.)
    let unknown = client
        .error(
            "events.subscribe",
            json!({
                "events": ["secrets.changed"],
                "deviceIds": ["88888888-8888-4888-8888-888888888888"],
            }),
        )
        .await;
    assert_eq!(unknown["code"], "not_found");
    // This engine's own id is always a valid target.
    client
        .ok(
            "events.subscribe",
            json!({
                "events": ["secrets.changed"],
                "deviceIds": ["55555555-5555-4555-8555-555555555555"],
            }),
        )
        .await;

    // Connections: this engine's status, no devices yet, and a side-effect-free health read.
    let status = client.ok("connections.status", json!({})).await;
    assert!(status["deviceId"].as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(
        client.ok("connections.devices.list", json!({})).await,
        json!([])
    );
    assert_eq!(
        client.ok("connections.devices.health", json!({})).await,
        json!({"devices": []})
    );

    // A saved device appears in health and is removed with its event. This engine never set
    // up a Connections key, so there is no key to revoke and nothing to contact.
    let config_path = silo_engine::channel::current()
        .state_dir(&home)
        .join("desktop-remote/config.json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    let device = "66666666-6666-4666-8666-666666666666";
    config["devices"] =
        json!([{"id": device, "name": "Office Mac", "address": "user@office.example"}]);
    std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let health = client.ok("connections.devices.health", json!({})).await;
    assert_eq!(
        health,
        json!({"devices": [{
            "deviceId": device,
            "connected": false,
            "busy": false,
            "error": null,
            "lastSeen": null,
        }]})
    );
    assert_eq!(
        client
            .ok("connections.devices.remove", json!({"deviceId": device}))
            .await,
        Value::Null
    );
    let changed = client.event("connections.devices.changed").await;
    assert_eq!(changed["payload"], json!({"devices": []}));
    assert_eq!(
        client.ok("connections.devices.list", json!({})).await,
        json!([])
    );

    // GitHub, secrets, network and SSH access answer in the canonical shapes.
    let github = client.ok("github.state", json!({})).await;
    assert_eq!(github["state"], "disconnected");
    assert_eq!(github["pendingAuthorizationUrl"], Value::Null);
    assert_eq!(client.ok("secrets.list", json!({})).await, json!([]));
    assert_eq!(
        client.ok("network.state", json!({})).await,
        json!({"computers": []})
    );
    assert_eq!(
        client.ok("sshAccess.state", json!({})).await,
        json!({"computers": []})
    );

    // Errors keep their canonical codes and say what to do.
    let wildcard = client
        .error(
            "secrets.save",
            json!({"request": {
                "operation": "add",
                "name": "SYNTHETIC_TOKEN",
                "value": "synthetic-value",
                "computerIds": ["22222222-2222-4222-8222-222222222222"],
                "allowedDomains": ["*"],
            }}),
        )
        .await;
    assert!(wildcard["message"]
        .as_str()
        .unwrap()
        .contains("allowAnyDestination"));
    let remote = client
        .error(
            "github.state",
            json!({"deviceId": "77777777-7777-4777-8777-777777777777"}),
        )
        .await;
    assert!(remote["message"]
        .as_str()
        .unwrap()
        .contains("no longer connected"));
    let callback = client
        .error("github.connect.callback", json!({"jobId": "job:1"}))
        .await;
    assert_eq!(callback["code"], "unsupported_remote_operation");
    let missing = client
        .error(
            "push.status",
            json!({
                "computerId": "22222222-2222-4222-8222-222222222222",
                "repositoryPath": "/work/repo",
                "operationId": "88888888-8888-4888-8888-888888888888",
            }),
        )
        .await;
    assert!(missing["message"]
        .as_str()
        .unwrap()
        .contains("no longer exists"));

    // `github.connect` is a durable job: cancel it and read the retained outcome.
    let started = client.ok("github.connect", json!({})).await;
    let job_id = started["jobId"].as_str().unwrap().to_owned();
    client
        .ok("operations.cancel", json!({"jobId": job_id}))
        .await;
    let outcome = loop {
        let found = client
            .ok("operations.job.get", json!({"jobId": job_id}))
            .await;
        if found["state"] == "finished" {
            break found["outcome"].clone();
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert!(outcome["error"]["code"].is_string(), "{outcome}");
    client
        .ok("operations.job.acknowledge", json!({"jobId": job_id}))
        .await;

    let _ = shutdown.send(());
    timeout(Duration::from_secs(5), server_task)
        .await
        .unwrap()
        .unwrap();
}
