//! Socket-level check of the guest domain: a real `engine.sock` session drives the guest
//! methods against a fake runtime that runs guest commands on this machine.
use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc};

use serde_json::{json, Value};
use silo_engine::{
    context::{EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl},
    runtime::RuntimePaths,
};
use silod::{
    context::SilodContext,
    dispatcher::Dispatcher,
    framing::{read_json_frame, write_json_frame},
    identity::LocalDeviceIdentity,
    methods::guest::use_runtime_paths,
    socket::{EngineServer, SystemPeerEuidProvider},
};
use tokio::{
    net::UnixStream,
    sync::oneshot,
    time::{timeout, Duration},
};

const COMPUTER: &str = "00000000-0000-4000-8000-0000000000aa";
const FAKE_MSB: &str = r#"#!/bin/sh
case "$1" in
  inspect)
    printf '%s\n' '{"name":"dev","status":"Running","config":{"labels":{"silo.managed":"true"}},"runtime_instance_id":"instance-1"}'
    exit 0 ;;
  --silo-working-account-protocol)
    printf '1\n'; exit 0 ;;
  exec)
    shift
    while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do
      if [ "$1" = "--env" ]; then shift; export "$1"; fi
      shift
    done
    shift
    exec "$@" ;;
esac
exit 2
"#;

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

fn fake_runtime(root: &Path) -> RuntimePaths {
    let executable = root.join("msb");
    std::fs::write(&executable, FAKE_MSB).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(root.join("libkrunfw"), b"").unwrap();
    std::fs::create_dir_all(root.join("home")).unwrap();
    let paths = RuntimePaths {
        guest_image: root.join("guest-image"),
        executable,
        home: root.join("home"),
        storage_home: None,
        library: root.join("libkrunfw"),
        metadata: root.join("computers.json"),
        volumes: root.join("volumes"),
    };
    std::fs::write(
        &paths.metadata,
        json!({
            "schemaVersion": 1,
            "computers": [{
                "id": COMPUTER, "name": "dev", "cpus": 1, "maxCPUs": 2,
                "memoryGiB": 2, "maxMemoryGiB": 4,
                "workspaceStorageGiB": 10, "runtimeStorageGiB": 10
            }]
        })
        .to_string(),
    )
    .unwrap();
    paths
}

/// One session: sends a request and collects every frame until its response arrives.
struct Session {
    client: UnixStream,
    next: u32,
    events: Vec<Value>,
}

impl Session {
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = format!("request-{}", self.next);
        write_json_frame(
            &mut self.client,
            &json!({"id": id, "method": method, "params": params}),
        )
        .await
        .unwrap();
        loop {
            let frame: Value = timeout(Duration::from_secs(30), read_json_frame(&mut self.client))
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            if frame["id"] == id {
                return frame;
            }
            self.events.push(frame);
        }
    }

    async fn event(&mut self, name: &str) -> Value {
        if let Some(position) = self.events.iter().position(|event| event["event"] == name) {
            return self.events.remove(position);
        }
        loop {
            let frame: Value = timeout(Duration::from_secs(30), read_json_frame(&mut self.client))
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            if frame["event"] == name {
                return frame;
            }
            self.events.push(frame);
        }
    }
}

fn bytes(frame: &Value) -> Vec<u8> {
    frame
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| byte.as_u64().unwrap() as u8)
        .collect()
}

#[tokio::test]
async fn the_guest_domain_works_end_to_end_over_the_engine_socket() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let app_data = root.join("app-data");
    use_runtime_paths(app_data.clone(), fake_runtime(root));
    let context = Arc::new(SilodContext::new(
        EnginePaths {
            app_data,
            home: root.join("user"),
            ..EnginePaths::default()
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
    let socket_path = root.join("engine.sock");
    let euid = unsafe { libc::geteuid() };
    let server = EngineServer::bind(
        socket_path.clone(),
        euid,
        Arc::new(SystemPeerEuidProvider),
        Arc::new(Dispatcher::core(context)),
    )
    .unwrap();
    let (shutdown, shutdown_receiver) = oneshot::channel();
    let server_task = tokio::spawn(server.run(shutdown_receiver));

    let mut session = Session {
        client: UnixStream::connect(&socket_path).await.unwrap(),
        next: 0,
        events: Vec::new(),
    };
    let handshake = session
        .request(
            "engine.handshake",
            json!({"protocol": {"min": 6, "max": 6},
                   "client": {"name": "guest-integration-test", "version": "1.0"}}),
        )
        .await;
    assert_eq!(handshake["result"]["deviceId"], "fixture-device");

    // Files: a create-only write with an explicit null, a conflict, then a ranged read.
    let target = root
        .join("inbox/message.json")
        .to_string_lossy()
        .into_owned();
    let write = |data: &str, replace: &str, expected: Option<Value>| {
        let mut params = json!({
            "computerId": COMPUTER, "path": target, "data": data.as_bytes(),
            "mode": 0o600, "createParents": true, "replace": replace
        });
        if let Some(expected) = expected {
            params["expectedSha256"] = expected;
        }
        params
    };
    let created = session
        .request(
            "guest.files.write",
            write("first", "ifSha256", Some(Value::Null)),
        )
        .await;
    assert_eq!(created["result"]["size"], 5);
    let conflict = session
        .request(
            "guest.files.write",
            write("second", "ifSha256", Some(Value::Null)),
        )
        .await;
    assert_eq!(conflict["error"]["code"], "changed");
    let read = session
        .request(
            "guest.files.read",
            json!({"computerId": COMPUTER, "path": target, "offset": 1, "length": 3}),
        )
        .await;
    assert_eq!(bytes(&read["result"]["data"]), b"irs");
    assert_eq!(read["result"]["mode"], 0o600);

    // Bounded exec returns the exit status and raw bytes.
    let exec = session
        .request(
            "guest.exec",
            json!({"computerId": COMPUTER, "argv": ["sh", "-c", "cat; exit 9"],
                   "stdin": [0, 255, 10], "timeoutMs": 20000}),
        )
        .await;
    assert_eq!(exec["result"]["exitCode"], 9);
    assert_eq!(bytes(&exec["result"]["stdout"]), vec![0, 255, 10]);

    // An exec job streams its output as events and finishes with its exit.
    let subscribed = session
        .request(
            "events.subscribe",
            json!({"events": ["guest.exec.output", "guest.exec.exited", "operations.job.finished"]}),
        )
        .await;
    assert!(subscribed["result"]["subscription"].as_str().is_some());
    let started = session
        .request(
            "guest.exec.start",
            json!({"computerId": COMPUTER, "argv": ["sh", "-c", "echo streamed; exit 3"],
                   "stdinOpen": false, "retainOutputBytes": 4096}),
        )
        .await;
    let job = started["result"]["jobId"].as_str().unwrap().to_owned();
    let output = session.event("guest.exec.output").await;
    assert_eq!(output["payload"]["jobId"], job);
    assert_eq!(bytes(&output["payload"]["data"]), b"streamed\n");
    let exited = session.event("guest.exec.exited").await;
    assert_eq!(exited["payload"]["exitCode"], 3);
    let finished = session.event("operations.job.finished").await;
    assert_eq!(finished["payload"]["jobId"], job);
    assert_eq!(finished["payload"]["result"]["exitCode"], 3);
    let replay = session
        .request(
            "guest.exec.get",
            json!({"jobId": job, "deviceId": "fixture-device"}),
        )
        .await;
    assert_eq!(replay["result"]["state"], "exited");
    assert_eq!(replay["result"]["exitCode"], 3);
    assert_eq!(bytes(&replay["result"]["output"][0]["data"]), b"streamed\n");

    // A watch is a subscription of its own: events arrive without any events.subscribe.
    let folder = root.join("watched");
    std::fs::create_dir(&folder).unwrap();
    let watch = session
        .request(
            "guest.files.watch",
            json!({"computerId": COMPUTER, "paths": [folder]}),
        )
        .await;
    let subscription = watch["result"]["subscriptionId"]
        .as_str()
        .unwrap()
        .to_owned();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    std::fs::write(folder.join("note"), b"hi").unwrap();
    let changed = session.event("guest.files.changed").await;
    assert_eq!(changed["subscription"], subscription);
    assert_eq!(changed["payload"]["subscriptionId"], subscription);
    assert_eq!(
        changed["payload"]["path"],
        folder.to_string_lossy().as_ref()
    );
    let ended = session
        .request("events.unsubscribe", json!({"subscription": subscription}))
        .await;
    assert!(ended.get("error").is_none());

    // Errors carry stable codes over the wire.
    let missing = session
        .request(
            "guest.files.read",
            json!({"computerId": COMPUTER, "path": root.join("nope").to_string_lossy()}),
        )
        .await;
    assert_eq!(missing["error"]["code"], "not_found");
    let other_device = session
        .request(
            "guest.account",
            json!({"computerId": COMPUTER, "deviceId": "another-device"}),
        )
        .await;
    assert_eq!(
        other_device["error"]["code"],
        "unsupported_remote_operation"
    );

    drop(session);
    let _ = shutdown.send(());
    server_task.await.unwrap();
}
