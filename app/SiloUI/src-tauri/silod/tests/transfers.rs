//! Socket-level check that a macOS transfer has its job before it waits for anything: the
//! computer's turn in the operation queue and the login can take longer than a client waits
//! for the response, and the response needs only the job.
use std::sync::Arc;

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

struct Session {
    client: UnixStream,
    next: u32,
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
        }
    }

    /// Polls `operations.job.get` until the job has an outcome.
    async fn finished(&mut self, job_id: &str) -> Value {
        loop {
            let frame = self
                .request("operations.job.get", json!({"jobId": job_id}))
                .await;
            if frame["result"]["outcome"].is_object() {
                return frame["result"].clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

#[tokio::test]
async fn a_macos_transfer_answers_with_its_job_while_it_waits_for_the_computers_turn() {
    const NO_ACCESS: &str = "This computer's access is missing.";
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let paths = EnginePaths {
        app_data: root.join("app-data"),
        home: root.join("user"),
        ..EnginePaths::default()
    };
    let app_data = paths.app_data.clone();
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
    // The engine reads its context from here when a transfer accepts its job.
    silo_engine::context::install(context.clone()).unwrap();
    let id = silo_engine::macos_computers_engine::install_test_computer(
        &app_data,
        "mac-fixture",
        silo_engine::macos_computers_engine::TestComputerState::Running,
        silo_engine::computer_use::Approval::Ask,
    );
    let socket_path = root.join("engine.sock");
    let server = EngineServer::bind(
        socket_path.clone(),
        unsafe { libc::geteuid() },
        Arc::new(SystemPeerEuidProvider),
        Arc::new(Dispatcher::core(context)),
    )
    .unwrap();
    let (shutdown, shutdown_receiver) = oneshot::channel();
    let server_task = tokio::spawn(server.run(shutdown_receiver));
    let mut session = Session {
        client: UnixStream::connect(&socket_path).await.unwrap(),
        next: 0,
    };
    session
        .request(
            "engine.handshake",
            json!({"protocol": {"min": 6, "max": 6},
                   "client": {"name": "transfers-test", "version": "1.0"}}),
        )
        .await;

    // Another operation holds the computer, so each transfer has to queue behind it.
    let hold = silo_engine::runtime::OPERATIONS
        .computer(&id, "mac-fixture", "A test operation")
        .unwrap();
    let upload = session
        .request(
            "files.upload",
            json!({
                "transferId": "queued-up", "computerId": id,
                "directory": "/Users/silo/workspace", "conflict": "ask", "paths": ["/tmp/a"]
            }),
        )
        .await;
    assert_eq!(upload["result"]["jobId"], "transfer:queued-up", "{upload}");
    let download = session
        .request(
            "files.download",
            json!({
                "transferId": "queued-down", "computerId": id,
                "path": "/Users/silo/workspace/a"
            }),
        )
        .await;
    assert_eq!(
        download["result"]["jobId"], "transfer:queued-down",
        "{download}"
    );
    // Both are still waiting: no outcome while the computer is busy.
    for job in ["transfer:queued-up", "transfer:queued-down"] {
        let lookup = session
            .request("operations.job.get", json!({"jobId": job}))
            .await;
        assert!(
            lookup["result"]["outcome"].is_null(),
            "{job} must still be waiting: {lookup}"
        );
    }
    drop(hold);
    // Once the turn comes, the missing login ends them through their jobs.
    for job in ["transfer:queued-up", "transfer:queued-down"] {
        let lookup = session.finished(job).await;
        assert_eq!(lookup["outcome"]["error"]["message"], NO_ACCESS, "{job}");
    }

    shutdown.send(()).unwrap();
    server_task.await.unwrap();
}
