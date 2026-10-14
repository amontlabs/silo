//! Socket-level checks of the computers domain over `engine.sock`: reads, job lifecycles
//! with their pushed events, typed refusals, and a saved macOS computer. Nothing here starts
//! a VM or a runtime; a macOS computer is only a saved record on disk.

use std::{path::Path, sync::Arc};

use serde_json::{json, Value};
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

const MACOS: &str = "22222222-2222-4222-8222-222222222222";
const LINUX: &str = "00000000-0000-4000-8000-000000000001";
const UNKNOWN: &str = "99999999-9999-4999-8999-999999999999";

struct NoopMainThread;
impl MainThread for NoopMainThread {
    fn run(&self, work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
        work();
        Ok(())
    }
}

struct NoopProcess;
impl ProcessControl for NoopProcess {
    fn exit(&self, _code: i32) {}
    fn restart(&self) -> ! {
        panic!("restart is not expected in a computers socket test")
    }
}

struct Session {
    client: UnixStream,
    next: u32,
    pushed: Vec<Value>,
}

impl Session {
    async fn open(socket: &Path) -> Self {
        let client = timeout(Duration::from_secs(2), UnixStream::connect(socket))
            .await
            .unwrap()
            .unwrap();
        let mut session = Self {
            client,
            next: 0,
            pushed: Vec::new(),
        };
        let hello = session
            .request(
                "engine.handshake",
                json!({
                    "protocol": {"min": 6, "max": 6},
                    "client": {"name": "computers-socket-test", "version": "1"}
                }),
            )
            .await;
        assert!(hello.get("result").is_some(), "{hello}");
        session
    }

    /// Sends a request and returns its response; events that arrive first are kept.
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
            let frame: Value = timeout(Duration::from_secs(20), read_json_frame(&mut self.client))
                .await
                .expect("the engine answered in time")
                .unwrap()
                .expect("the connection stayed open");
            if frame["id"] == id {
                return frame;
            }
            self.pushed.push(frame);
        }
    }

    async fn result(&mut self, method: &str, params: Value) -> Value {
        let response = self.request(method, params).await;
        assert!(response.get("error").is_none(), "{method}: {response}");
        response["result"].clone()
    }

    async fn error_code(&mut self, method: &str, params: Value) -> String {
        let response = self.request(method, params).await;
        response["error"]["code"]
            .as_str()
            .unwrap_or_else(|| panic!("{method} should fail: {response}"))
            .to_owned()
    }

    /// The next pushed event named `name`.
    async fn event(&mut self, name: &str) -> Value {
        if let Some(index) = self.pushed.iter().position(|frame| frame["event"] == name) {
            return self.pushed.remove(index);
        }
        loop {
            let frame: Value = timeout(Duration::from_secs(20), read_json_frame(&mut self.client))
                .await
                .unwrap_or_else(|_| panic!("no {name} event arrived"))
                .unwrap()
                .expect("the connection stayed open");
            if frame["event"] == name {
                return frame;
            }
            self.pushed.push(frame);
        }
    }

    async fn finished(&mut self, job_id: &str) -> Value {
        loop {
            let event = self.event("operations.job.finished").await;
            if event["payload"]["jobId"] == job_id {
                return event["payload"].clone();
            }
        }
    }
}

fn write_macos_record(app_data: &Path) {
    let directory = app_data.join("macos-computers").join(MACOS);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("computer.json"),
        json!({
            "schemaVersion": 1, "id": MACOS, "name": "studio", "cpus": 4, "memoryGiB": 8,
            "diskGiB": 64, "createdAt": "2026-01-01T00:00:00Z",
            "macAddress": "02:00:00:00:00:01", "restoreImage": null, "installed": true
        })
        .to_string(),
    )
    .unwrap();
}

#[tokio::test]
async fn the_computers_domain_over_the_engine_socket() {
    let root = tempfile::tempdir().unwrap();
    let path = |name: &str| root.path().join(name);
    let paths = EnginePaths {
        home: path("home"),
        app_data: path("data"),
        app_config: path("config"),
        app_cache: path("cache"),
        app_log: path("log"),
        resources: path("resources"),
    };
    write_macos_record(&paths.app_data);
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
    silo_engine::context::install(context.clone()).unwrap();
    context.install_queue_events();

    let socket = path("run").join("engine.sock");
    let euid = unsafe { libc::geteuid() };
    let server = EngineServer::bind(
        socket.clone(),
        euid,
        Arc::new(SystemPeerEuidProvider),
        Arc::new(Dispatcher::core(context)),
    )
    .unwrap();
    let (shutdown, shutdown_signal) = oneshot::channel();
    let server_task = tokio::spawn(server.run(shutdown_signal));

    let mut engine = Session::open(&socket).await;
    let subscription = engine
        .result(
            "events.subscribe",
            json!({"events": ["operations.*", "computers.*"]}),
        )
        .await;
    assert!(subscription["subscription"].is_string());

    // Reads: both kinds in one list, with the saved macOS computer and its kind.
    let list = engine.result("computers.list", json!({})).await;
    assert_eq!(list["linux"]["computers"], json!([]));
    assert_eq!(list["macos"]["computers"][0]["id"], MACOS);
    assert_eq!(list["macos"]["computers"][0]["kind"], "macos");
    assert_eq!(list["macos"]["computers"][0]["state"], "stopped");
    assert_eq!(
        engine
            .result("computers.configuration.read", json!({}))
            .await,
        json!({"schemaVersion": 1, "computers": []})
    );
    assert_eq!(
        engine.result("activity.setup.read", json!({})).await,
        json!([])
    );
    assert_eq!(
        engine.result("computers.launch", json!({})).await,
        json!({"enabled": false, "computerIds": [], "lastPass": null})
    );

    // The quit dialog: running computers come from the list, and each stops as a job. A
    // stopped computer is refused with a typed code, never started.
    let running: Vec<_> = list["macos"]["computers"]
        .as_array()
        .unwrap()
        .iter()
        .chain(list["linux"]["computers"].as_array().unwrap())
        .filter(|row| row["state"] == "running")
        .collect();
    assert!(running.is_empty());
    let stop = engine
        .result("computers.stop", json!({"computerId": MACOS}))
        .await;
    let stop_job = stop["jobId"].as_str().unwrap().to_owned();
    assert!(stop_job.starts_with("job:"));
    let stopped = engine.finished(&stop_job).await;
    assert_eq!(stopped["error"]["code"], "not_running", "{stopped}");

    // Typed refusals that need no job.
    assert_eq!(
        engine
            .error_code("computers.forceStop", json!({"computerId": MACOS}))
            .await,
        "not_running"
    );
    assert_eq!(
        engine
            .error_code("computers.restart", json!({"computerId": MACOS}))
            .await,
        "unsupported_remote_operation"
    );
    assert_eq!(
        engine
            .error_code("computers.dismissError", json!({"computerId": MACOS}))
            .await,
        "unsupported_remote_operation"
    );
    assert_eq!(
        engine
            .error_code("computers.start", json!({"computerId": UNKNOWN}))
            .await,
        "not_found"
    );
    assert_eq!(
        engine
            .error_code("computers.list", json!({"deviceId": "elsewhere"}))
            .await,
        "not_found"
    );
    assert_eq!(
        engine
            .error_code("macos.display.stream", json!({"computerId": MACOS}))
            .await,
        "unsupported_remote_operation"
    );
    assert_eq!(
        engine
            .error_code("storage.read", json!({"computerId": MACOS}))
            .await,
        "unsupported_remote_operation"
    );
    assert_eq!(
        engine.error_code("computers.start", json!({})).await,
        "internal"
    );

    // The CPU and memory edit is a `job:` like every macOS job, not a `setup:` job; this
    // test engine cannot run macOS computers, and the refusal ends the job.
    let edit = engine
        .result(
            "computers.configuration.change",
            json!({"change": {
                "kind": "macosResources", "computerId": MACOS,
                "expected": {"cpus": 4, "memoryGiB": 8, "diskGiB": 64},
                "cpus": 6, "memoryGiB": 12, "requestId": "resources-1"
            }}),
        )
        .await;
    let edit_job = edit["jobId"].as_str().unwrap().to_owned();
    assert!(edit_job.starts_with("job:"), "{edit_job}");
    let edited = engine.finished(&edit_job).await;
    assert!(edited["error"]["message"].is_string(), "{edited}");

    // A Linux computer that is not saved is not found, before any job exists.
    assert_eq!(
        engine
            .error_code(
                "computers.delete",
                json!({
                    "computerId": LINUX,
                    "expected": {"id": LINUX, "name": "dev", "cpus": 2, "maxCPUs": 2,
                        "memoryGiB": 2, "maxMemoryGiB": 2, "workspaceStorageGiB": 10,
                        "runtimeStorageGiB": 10}
                }),
            )
            .await,
        "not_found"
    );

    // Deleting the macOS computer is a job that removes its folder, then the list is empty.
    let removal = engine
        .result("computers.delete", json!({"computerId": MACOS}))
        .await;
    let removal_job = removal["jobId"].as_str().unwrap().to_owned();
    let removed = engine.finished(&removal_job).await;
    assert!(removed["error"].is_null(), "{removed}");
    let after = engine.result("computers.list", json!({})).await;
    assert_eq!(after["macos"]["computers"], json!([]));
    assert!(!root
        .path()
        .join("data/macos-computers")
        .join(MACOS)
        .exists());
    // The outcome stays until acknowledged.
    let queue = engine.result("operations.list", json!({})).await;
    assert!(queue["unacknowledgedJobIds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|id| *id == removal_job));
    engine
        .result("operations.job.acknowledge", json!({"jobId": removal_job}))
        .await;

    drop(engine);
    let _ = shutdown.send(());
    server_task.await.unwrap();
}
