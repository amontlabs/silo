//! A socket-level run of the services domain: settings, the update schedule, update
//! preparation and abort, a shutdown job and `engine.disconnect`, over a real Unix socket with
//! the production dispatcher and a fake computer platform.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use serde_json::{json, Value};
use silo_engine::{
    context::{
        EngineContext, EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl,
    },
    engine_update::{
        journal::{CaptureRef, ComputerKind, MacCapture, RuntimeIdentity},
        HeldGuards, OriginalState, Platform, Reconciled, RestoreError, ResumeHeldError,
        RunningComputer, SaveAdapter, SaveContext,
    },
};
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

#[derive(Default)]
struct Exits(Mutex<Vec<i32>>);
impl ProcessControl for Exits {
    fn exit(&self, code: i32) {
        self.0.lock().unwrap().push(code);
    }

    fn restart(&self) -> ! {
        panic!("restart is not expected in a socket integration test")
    }
}

#[derive(Default)]
struct Calls(Mutex<Vec<String>>);

struct Adapter(Arc<Calls>);
impl SaveAdapter for Adapter {
    fn kind(&self) -> ComputerKind {
        ComputerKind::Macos
    }

    fn running(&self) -> Result<Vec<RunningComputer>, String> {
        Ok(vec![RunningComputer {
            id: "mac-1".into(),
            name: "dev".into(),
            kind: ComputerKind::Macos,
        }])
    }

    fn identity(&self, _: &RunningComputer) -> Result<RuntimeIdentity, String> {
        Ok(RuntimeIdentity::default())
    }

    fn check_support(&self, _: &RunningComputer) -> Result<(), String> {
        Ok(())
    }

    fn save_and_hold(
        &self,
        computer: &RunningComputer,
        context: &SaveContext<'_>,
    ) -> Result<CaptureRef, String> {
        self.0
             .0
            .lock()
            .unwrap()
            .push(format!("save:{}", computer.id));
        let directory = context
            .saves_root
            .join(context.transaction_id)
            .join(&computer.id);
        Ok(CaptureRef::Macos(MacCapture {
            state_file: directory.join("state"),
            disk: directory.join("disk"),
            auxiliary_storage: directory.join("aux"),
            directory,
            source_generation: 1,
            host_build: "25F71".into(),
            save_format_version: 1,
            artifact_sizes: BTreeMap::new(),
        }))
    }

    fn resume_held(
        &self,
        computer: &RunningComputer,
        _: &RuntimeIdentity,
    ) -> Result<(), ResumeHeldError> {
        self.0
             .0
            .lock()
            .unwrap()
            .push(format!("resume:{}", computer.id));
        Ok(())
    }

    fn release(
        &self,
        computer: &RunningComputer,
        _: &CaptureRef,
        _: &RuntimeIdentity,
    ) -> Result<(), String> {
        self.0
             .0
            .lock()
            .unwrap()
            .push(format!("release:{}", computer.id));
        Ok(())
    }

    fn reconcile_unsaved(
        &self,
        _: &RunningComputer,
        _: &RuntimeIdentity,
    ) -> Result<Reconciled, String> {
        Ok(Reconciled::Lost("lost".into()))
    }

    fn restore(
        &self,
        _: &RunningComputer,
        _: &CaptureRef,
        _: &RuntimeIdentity,
        _: &str,
        arm: &mut dyn FnMut(Option<&RuntimeIdentity>) -> Result<(), String>,
    ) -> Result<(), RestoreError> {
        Ok(arm(None)?)
    }

    fn probe_original(
        &self,
        computer: &RunningComputer,
        _: &RuntimeIdentity,
    ) -> Result<OriginalState, String> {
        Ok(OriginalState::Held)
    }

    fn probe_armed(
        &self,
        _: &RunningComputer,
        _: &CaptureRef,
        _: &RuntimeIdentity,
        _: &str,
        _: Option<&RuntimeIdentity>,
    ) -> Result<bool, String> {
        Ok(true)
    }

    fn discard(&self, _: &RunningComputer, _: &CaptureRef) -> Result<(), String> {
        Ok(())
    }

    fn owns_live_machines(&self) -> bool {
        true
    }
}

struct Guards;
impl HeldGuards for Guards {}

struct FakePlatform(Arc<Calls>);
impl Platform for FakePlatform {
    fn adapters(&self) -> Vec<Box<dyn SaveAdapter>> {
        vec![Box::new(Adapter(self.0.clone()))]
    }

    fn acquire_guards(
        &self,
        _: Option<&silo_api::jobs::JobId>,
    ) -> Result<Box<dyn HeldGuards>, String> {
        Ok(Box::new(Guards))
    }

    fn stop_computers(&self, _: Option<Instant>) -> Result<(), String> {
        self.0 .0.lock().unwrap().push("stop-computers".into());
        Ok(())
    }

    fn update_readiness(&self) -> Result<Vec<String>, String> {
        Ok(vec!["dev".into()])
    }
}

struct Client {
    stream: UnixStream,
    next: u32,
    events: Vec<Value>,
}

impl Client {
    async fn connect(path: &std::path::Path) -> Self {
        let stream = timeout(Duration::from_secs(2), UnixStream::connect(path))
            .await
            .unwrap()
            .unwrap();
        let mut client = Self {
            stream,
            next: 0,
            events: Vec::new(),
        };
        let handshake = client
            .request(
                "engine.handshake",
                json!({
                    "protocol": {"min": 6, "max": 6},
                    "client": {"name": "services-test", "version": "1.0"}
                }),
            )
            .await;
        assert_eq!(handshake["protocol"], 6);
        client
    }

    /// Sends one request and returns its result (or `{"error": …}`), keeping events aside.
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = format!("request-{}", self.next);
        write_json_frame(
            &mut self.stream,
            &json!({"id": id, "method": method, "params": params}),
        )
        .await
        .unwrap();
        loop {
            let frame: Value = timeout(Duration::from_secs(10), read_json_frame(&mut self.stream))
                .await
                .expect("the engine answers")
                .unwrap()
                .expect("the connection stays open");
            if frame["id"] == id {
                return match frame.get("error") {
                    Some(error) => json!({"error": error}),
                    None => frame["result"].clone(),
                };
            }
            assert!(frame.get("event").is_some(), "unexpected frame {frame}");
            self.events.push(frame);
        }
    }

    async fn finished(&mut self, job: &Value) -> Value {
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let record = self
                .request("operations.job.get", json!({"jobId": job["jobId"]}))
                .await;
            if record["state"] == "finished" {
                return record["outcome"].clone();
            }
            assert!(Instant::now() < deadline, "{job} did not finish: {record}");
            sleep(Duration::from_millis(20)).await;
        }
    }

    async fn closed(&mut self) -> bool {
        matches!(
            timeout(
                Duration::from_secs(2),
                read_json_frame::<_, Value>(&mut self.stream)
            )
            .await,
            Ok(Ok(None))
        )
    }
}

#[tokio::test]
async fn the_services_domain_works_end_to_end_over_the_socket() {
    let temporary = tempfile::tempdir().unwrap();
    let socket_path = temporary.path().join(".silo/engine.sock");
    let exits = Arc::new(Exits::default());
    let context = Arc::new(SilodContext::new(
        EnginePaths {
            home: temporary.path().to_path_buf(),
            app_data: temporary.path().join("data"),
            app_config: temporary.path().join("config"),
            ..Default::default()
        },
        EngineIdentity {
            channel: silo_engine::channel::Channel::Production,
            version: "1.0.0".to_owned(),
            package_kind: PackageKind::MacApp,
        },
        LocalDeviceIdentity {
            device_id: "fixture-device".to_owned(),
            name: "Fixture Mac".to_owned(),
        },
        Arc::new(NoopMainThread),
        exits.clone(),
    ));
    silo_engine::context::install(context.clone()).unwrap();
    let calls = Arc::new(Calls::default());
    context
        .runtime_state()
        .update()
        .set_platform(Arc::new(FakePlatform(calls.clone())));
    let server = EngineServer::bind(
        socket_path.clone(),
        unsafe { libc::geteuid() },
        Arc::new(SystemPeerEuidProvider),
        Arc::new(Dispatcher::core(context)),
    )
    .unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server_task = tokio::spawn(server.run(shutdown_rx));
    let mut client = Client::connect(&socket_path).await;

    let subscription = client
        .request(
            "events.subscribe",
            json!({"events": ["settings.changed", "updates.changed", "engine.shutdown.changed"]}),
        )
        .await;
    assert!(subscription["subscription"].is_string());

    // Settings: the automatic-checkpoint count is a nonnegative integer; 0 turns it off.
    let updated = client
        .request(
            "settings.update",
            json!({"patch": {"automaticCheckpointRetentionCount": 0}}),
        )
        .await;
    assert_eq!(updated["settings"]["automaticCheckpointRetentionCount"], 0);
    let invalid = client
        .request(
            "settings.update",
            json!({"patch": {"automaticCheckpointRetentionCount": -1}}),
        )
        .await;
    assert!(invalid["error"].is_object(), "{invalid}");
    let read = client.request("settings.read", json!({})).await;
    assert_eq!(read["settings"]["automaticCheckpointRetentionCount"], 0);
    assert_eq!(read["onboardingDraft"], Value::Null);
    assert!(client.request("settings.flush", json!({})).await.is_null());

    // Updates: the engine's own state, with the computers an update would save.
    let state = client.request("updates.state", json!({})).await;
    assert_eq!(state["currentVersion"], "1.0.0");
    assert_eq!(state["canInstall"], true);
    assert_eq!(state["runningComputers"], json!(["dev"]));
    let off = client
        .request("updates.setAutomaticChecks", json!({"enabled": false}))
        .await;
    assert_eq!(off["automaticChecks"], false);
    assert!(client.request("updates.focused", json!({})).await.is_null());
    let checked = client.request("updates.check", json!({})).await;
    assert_eq!(checked["phase"], "idle", "no feed is configured");

    // Update preparation holds the running set; abort resumes it.
    let prepare = client.request("engine.update.prepare", json!({})).await;
    let outcome = client.finished(&prepare).await;
    assert_eq!(
        outcome["result"],
        json!({"resuming": [{"id": "mac-1", "name": "dev"}]})
    );
    let engine_state = client.request("engine.state", json!({})).await;
    assert_eq!(engine_state["updatePending"], true);
    assert!(client
        .request("engine.update.abort", json!({}))
        .await
        .is_null());
    assert_eq!(
        client.request("engine.state", json!({})).await["updatePending"],
        false
    );
    assert_eq!(*calls.0.lock().unwrap(), ["save:mac-1", "resume:mac-1"]);

    // A shutdown that stops computers is a job; the process exits after its outcome is saved.
    let shutdown = client
        .request(
            "engine.shutdown",
            json!({"stopComputers": true, "reason": "user"}),
        )
        .await;
    let outcome = client.finished(&shutdown).await;
    assert!(outcome.get("error").is_none(), "{outcome}");
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    while exits.0.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the engine never exited");
        sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(*exits.0.lock().unwrap(), [0]);
    assert!(calls
        .0
        .lock()
        .unwrap()
        .contains(&"stop-computers".to_owned()));
    let state = client.request("engine.state", json!({})).await;
    assert_eq!(state["shuttingDown"], true);

    // Pushed events arrived in order on the same connection.
    let names: Vec<_> = client
        .events
        .iter()
        .map(|event| event["event"].as_str().unwrap().to_owned())
        .collect();
    assert!(names.contains(&"settings.changed".to_owned()), "{names:?}");
    assert!(names.contains(&"updates.changed".to_owned()), "{names:?}");
    assert!(
        names.contains(&"engine.shutdown.changed".to_owned()),
        "{names:?}"
    );

    // engine.disconnect: a null reply, then only this connection closes.
    let mut second = Client::connect(&socket_path).await;
    assert!(second
        .request("engine.disconnect", json!({}))
        .await
        .is_null());
    assert!(second.closed().await);
    assert_eq!(
        client.request("engine.state", json!({})).await["generation"],
        1,
        "the other client and the engine are unaffected"
    );

    let _ = shutdown_tx.send(());
    server_task.await.unwrap();
}
