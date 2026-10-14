//! One test per handler, over silod's real handler context and a fake platform.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use silo_api::{
    jobs::{ErrorCode, JobId, JobState},
    methods::MethodName,
};
use silo_engine::{
    context::{
        Audience, EngineContext, EngineIdentity, EnginePaths, MainThread, PackageKind,
        ProcessControl,
    },
    engine_update::{
        journal::{CaptureRef, ComputerKind, MacCapture, RuntimeIdentity},
        updater::{CheckFailure, Provider},
        HeldGuards, OriginalState, Platform, Reconciled, RestoreError, ResumeHeldError,
        RunningComputer, SaveAdapter, SaveContext,
    },
    updates::{CheckErrorKind, UpdateInfo},
};

use super::*;
use crate::{context::SilodContext, dispatcher::RequestContext, identity::LocalDeviceIdentity};

struct NoopMainThread;
impl MainThread for NoopMainThread {
    fn run(&self, _work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Default)]
struct RecordedExits(Mutex<Vec<i32>>);
impl ProcessControl for RecordedExits {
    fn exit(&self, code: i32) {
        self.0.lock().unwrap().push(code);
    }

    fn restart(&self) -> ! {
        panic!("restart is not expected in services handler tests")
    }
}

struct Fixture {
    silod: Arc<SilodContext>,
    handler: HandlerContext,
    exits: Arc<RecordedExits>,
    home: std::path::PathBuf,
    _directory: Option<tempfile::TempDir>,
    /// Held while the fixture lives: the engine update takes the process-wide operation gate.
    _gate: Option<crate::methods::ProcessGateLease>,
}

fn fixture() -> Fixture {
    let gate = crate::methods::process_gate_lock();
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = fixture_at(directory.path());
    fixture._directory = Some(directory);
    fixture._gate = Some(gate);
    fixture
}

/// A new engine instance over an existing home: its own process state, the same disk.
fn fixture_at(home: &std::path::Path) -> Fixture {
    let exits = Arc::new(RecordedExits::default());
    let silod = Arc::new(SilodContext::new(
        EnginePaths {
            home: home.to_path_buf(),
            app_data: home.join("data"),
            app_config: home.join("config"),
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
    let handler = HandlerContext::new(
        silod.clone(),
        RequestContext {
            client_id: "services-client".to_owned(),
            protocol: 6,
            client: None,
        },
    );
    Fixture {
        silod,
        handler,
        exits,
        home: home.to_path_buf(),
        _directory: None,
        _gate: None,
    }
}

/// Settings functions read the process-wide installed context, so every settings test shares
/// one installed fixture. Another module's tests may have installed their own context first in
/// this test binary; the settings store then lives under that context, which is equivalent here.
fn installed() -> &'static Fixture {
    static INSTALLED: OnceLock<Fixture> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        // Not `fixture()`: this one lives for the process and must not hold the gate lock.
        let directory = tempfile::tempdir().unwrap();
        let mut fixture = fixture_at(directory.path());
        fixture._directory = Some(directory);
        let _ = silo_engine::context::install(fixture.silod.clone());
        fixture
    })
}

// MARK: Fake platform

#[derive(Default)]
struct World {
    running: Mutex<Vec<(String, String)>>,
    calls: Mutex<Vec<String>>,
    stop_failure: Mutex<Option<String>>,
    restore_failure: Mutex<Option<String>>,
    release_failure: Mutex<Option<String>>,
}

impl World {
    fn with_running(names: &[&str]) -> Arc<Self> {
        let world = Arc::new(Self::default());
        *world.running.lock().unwrap() = names
            .iter()
            .enumerate()
            .map(|(index, name)| (format!("id-{index}"), (*name).to_owned()))
            .collect();
        world
    }

    fn calls(&self, prefix: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with(prefix))
            .count()
    }
}

struct FakeAdapter(Arc<World>);

impl SaveAdapter for FakeAdapter {
    fn kind(&self) -> ComputerKind {
        ComputerKind::Macos
    }

    fn running(&self) -> Result<Vec<RunningComputer>, String> {
        Ok(self
            .0
            .running
            .lock()
            .unwrap()
            .iter()
            .map(|(id, name)| RunningComputer {
                id: id.clone(),
                name: name.clone(),
                kind: ComputerKind::Macos,
            })
            .collect())
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
            .calls
            .lock()
            .unwrap()
            .push(format!("save:{}", computer.id));
        let directory = context
            .saves_root
            .join(context.transaction_id)
            .join(&computer.id);
        std::fs::create_dir_all(&directory).unwrap();
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
            .calls
            .lock()
            .unwrap()
            .push(format!("resume-held:{}", computer.id));
        Ok(())
    }

    fn release(
        &self,
        computer: &RunningComputer,
        _: &CaptureRef,
        _: &RuntimeIdentity,
    ) -> Result<(), String> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("release:{}", computer.id));
        match self.0.release_failure.lock().unwrap().clone() {
            Some(why) => Err(why),
            None => Ok(()),
        }
    }

    fn reconcile_unsaved(
        &self,
        computer: &RunningComputer,
        _: &RuntimeIdentity,
    ) -> Result<Reconciled, String> {
        Ok(Reconciled::Lost(format!("{} was lost", computer.name)))
    }

    fn restore(
        &self,
        computer: &RunningComputer,
        _: &CaptureRef,
        _: &RuntimeIdentity,
        _: &str,
        arm: &mut dyn FnMut(Option<&RuntimeIdentity>) -> Result<(), String>,
    ) -> Result<(), RestoreError> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("restore:{}", computer.id));
        if let Some(why) = self.0.restore_failure.lock().unwrap().clone() {
            return Err(RestoreError::Failed(why));
        }
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
        !self.0.running.lock().unwrap().is_empty()
    }
}

struct NoGuards(Arc<World>);
impl HeldGuards for NoGuards {}
impl Drop for NoGuards {
    fn drop(&mut self) {
        self.0.calls.lock().unwrap().push("guards released".into());
    }
}

struct FakeFeed {
    release: Mutex<Result<Option<UpdateInfo>, CheckFailure>>,
}

impl Provider for FakeFeed {
    fn check(&self) -> Result<Option<UpdateInfo>, CheckFailure> {
        self.release.lock().unwrap().clone()
    }

    fn download(&self, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<Vec<u8>, String> {
        progress(3, Some(6));
        Ok(vec![1, 2, 3, 4, 5, 6])
    }
}

struct FakePlatform {
    world: Arc<World>,
    feed: Mutex<Option<UpdateInfo>>,
}

impl Platform for FakePlatform {
    fn adapters(&self) -> Vec<Box<dyn SaveAdapter>> {
        vec![Box::new(FakeAdapter(self.world.clone()))]
    }

    fn acquire_guards(&self, job: Option<&JobId>) -> Result<Box<dyn HeldGuards>, String> {
        self.world
            .calls
            .lock()
            .unwrap()
            .push(format!("guards:{}", job.map_or("none", JobId::as_str)));
        Ok(Box::new(NoGuards(self.world.clone())))
    }

    fn begin_shutdown(&self) {
        self.world
            .calls
            .lock()
            .unwrap()
            .push("begin shutdown".into());
    }

    fn stop_computers(&self, deadline: Option<Instant>) -> Result<(), String> {
        self.world
            .calls
            .lock()
            .unwrap()
            .push(format!("stop-computers deadline={}", deadline.is_some()));
        match self.world.stop_failure.lock().unwrap().clone() {
            Some(why) => Err(why),
            None => {
                self.world.running.lock().unwrap().clear();
                Ok(())
            }
        }
    }

    fn flush_settings(&self) -> Result<(), String> {
        self.world
            .calls
            .lock()
            .unwrap()
            .push("flush settings".into());
        Ok(())
    }

    fn close_connections(&self) {
        self.world
            .calls
            .lock()
            .unwrap()
            .push("close connections".into());
    }

    fn reopen_admission(&self) {
        self.world
            .calls
            .lock()
            .unwrap()
            .push("reopen admission".into());
    }

    fn update_provider(&self) -> Box<dyn Provider> {
        Box::new(FakeFeed {
            release: Mutex::new(Ok(self.feed.lock().unwrap().clone())),
        })
    }

    fn update_readiness(&self) -> Result<Vec<String>, String> {
        self.adapters()[0]
            .running()
            .map(|running| running.into_iter().map(|computer| computer.name).collect())
    }
}

fn install_fake(fixture: &Fixture, world: &Arc<World>, feed: Option<UpdateInfo>) {
    fixture
        .silod
        .runtime_state()
        .update()
        .set_platform(Arc::new(FakePlatform {
            world: world.clone(),
            feed: Mutex::new(feed),
        }));
}

async fn call(
    fixture: &Fixture,
    method: MethodName,
    params: Value,
) -> Result<HandlerOutput, DispatchError> {
    handle(method, &params, &fixture.handler)
        .await
        .expect("services owns this method")
}

async fn ok(fixture: &Fixture, method: MethodName, params: Value) -> Value {
    call(fixture, method, params).await.unwrap().value
}

fn job_id(value: &Value) -> JobId {
    JobId::new(value["jobId"].as_str().unwrap()).unwrap()
}

/// Waits for a job's durable outcome.
async fn outcome(fixture: &Fixture, job: &JobId) -> silo_api::jobs::JobOutcome {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let record = silo_engine::jobs::get_at(fixture.silod.as_ref(), job, None)
            .unwrap()
            .expect("the job was accepted");
        if record.state == JobState::Finished {
            return record.outcome.expect("a finished job has an outcome");
        }
        assert!(Instant::now() < deadline, "{job} did not finish");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn engine_state(fixture: &Fixture) -> silo_api::methods::engine::EngineState {
    fixture.silod.engine_state()
}

// MARK: Settings

#[tokio::test]
async fn settings_read_update_and_flush_use_canonical_fields() {
    let fixture = installed();
    let mut receiver = fixture
        .silod
        .connect_event_stream(fixture.handler.client_id().clone());
    let subscription = fixture
        .silod
        .subscribe_events(fixture.handler.client_id(), vec!["settings.*".to_owned()])
        .unwrap();
    fixture
        .silod
        .activate_events(fixture.handler.client_id(), &subscription)
        .unwrap();

    let read = call(fixture, MethodName::SettingsRead, json!({}))
        .await
        .unwrap();
    assert_eq!(read.value["onboardingDraft"], Value::Null);
    assert!(read.event_barrier.is_some());
    assert!(read.value["settings"].get("notifyHealth").is_none());

    let updated = ok(
        fixture,
        MethodName::SettingsUpdate,
        json!({"patch": {"automaticCheckpointRetentionCount": 0, "theme": "dark"}}),
    )
    .await;
    assert_eq!(updated["settings"]["automaticCheckpointRetentionCount"], 0);
    assert_eq!(updated["settings"]["theme"], "dark");
    let three = ok(
        fixture,
        MethodName::SettingsUpdate,
        json!({"patch": {"automaticCheckpointRetentionCount": 3}}),
    )
    .await;
    assert_eq!(three["settings"]["automaticCheckpointRetentionCount"], 3);
    for invalid in [json!(-1), json!(1.5), json!("3"), json!(4_294_967_296u64)] {
        assert!(
            call(
                fixture,
                MethodName::SettingsUpdate,
                json!({"patch": {"automaticCheckpointRetentionCount": invalid}}),
            )
            .await
            .is_err(),
            "{invalid}"
        );
    }
    // The retired notification fallback keys are not accepted.
    assert!(call(
        fixture,
        MethodName::SettingsUpdate,
        json!({"patch": {"notifyBackup": false}}),
    )
    .await
    .is_err());

    let flushed = ok(fixture, MethodName::SettingsFlush, json!({})).await;
    assert!(flushed.is_null());

    let mut changed = 0;
    while let Some(delivery) = receiver.recv().await {
        if matches!(delivery, silo_engine::events::EventDelivery::Event(ref event) if event.event.name == "settings.changed")
        {
            changed += 1;
            if changed == 3 {
                break;
            }
        }
    }
    assert_eq!(changed, 3, "two updates and the flush each published");
}

// MARK: Engine lifecycle

#[tokio::test]
async fn engine_disconnect_replies_null_without_touching_lifecycle() {
    let fixture = fixture();
    let output = call(&fixture, MethodName::EngineDisconnect, json!({}))
        .await
        .unwrap();
    assert!(output.value.is_null());
    let state = engine_state(&fixture);
    assert!(!state.shutting_down && !state.update_pending);
    assert!(fixture.exits.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn engine_update_prepare_saves_the_running_set_and_attaches_repeats() {
    let fixture = fixture();
    let world = World::with_running(&["dev", "linux-1"]);
    install_fake(&fixture, &world, None);

    let first = ok(&fixture, MethodName::EngineUpdatePrepare, json!({})).await;
    let job = job_id(&first);
    assert_eq!(job.space(), silo_api::jobs::JobSpace::Job);
    let finished = outcome(&fixture, &job).await;
    assert!(finished.error.is_none(), "{finished:?}");
    let resuming = finished.result.unwrap();
    assert_eq!(
        resuming,
        json!({"resuming": [{"id": "id-0", "name": "dev"}, {"id": "id-1", "name": "linux-1"}]})
    );
    assert_eq!(world.calls("save:"), 2);
    assert!(engine_state(&fixture).update_pending);
    assert!(
        world.calls("guards:").eq(&1),
        "admission is held for the transaction"
    );
    // A repeated request attaches to the same job and saves nothing more.
    let again = ok(&fixture, MethodName::EngineUpdatePrepare, json!({})).await;
    assert_eq!(job_id(&again), job);
    assert_eq!(world.calls("save:"), 2);
    // Nothing else may shut down while the update is prepared.
    let busy = call(
        &fixture,
        MethodName::EngineShutdown,
        json!({"stopComputers": true, "reason": "user"}),
    )
    .await
    .unwrap_err();
    assert_eq!(busy.code, ErrorCode::Busy);
}

#[tokio::test]
async fn a_failed_preparation_ends_its_job_with_the_error_and_releases_admission() {
    let fixture = fixture();
    let world = World::with_running(&["dev"]);
    install_fake(&fixture, &world, None);
    // A journal from an earlier owner blocks a new preparation.
    let store = engine_update::store(fixture.silod.as_ref());
    store
        .save(&silo_engine::engine_update::journal::Journal::new(
            "old".into(),
            "job:0".into(),
            "0.9.0".into(),
            String::new(),
            vec![],
        ))
        .unwrap();
    let started = ok(&fixture, MethodName::EngineUpdatePrepare, json!({})).await;
    let finished = outcome(&fixture, &job_id(&started)).await;
    let error = finished.error.unwrap();
    assert!(
        error
            .message
            .contains("previous update still has computers"),
        "{error:?}"
    );
    // The earlier owner's journal is still there, so the update stays pending (and its
    // computers reserved) until abort settles it; the refused attempt itself saved nothing.
    assert!(engine_state(&fixture).update_pending);
    assert_eq!(world.calls("save:"), 0);
    assert!(world.calls("guards released") >= 1);
    ok(&fixture, MethodName::EngineUpdateAbort, json!({})).await;
    assert!(!engine_state(&fixture).update_pending);
}

#[tokio::test]
async fn engine_update_abort_resumes_the_saved_set_and_is_idempotent() {
    let fixture = fixture();
    let world = World::with_running(&["dev"]);
    install_fake(&fixture, &world, None);
    // With nothing prepared it succeeds at once.
    assert!(ok(&fixture, MethodName::EngineUpdateAbort, json!({}))
        .await
        .is_null());
    let job = job_id(&ok(&fixture, MethodName::EngineUpdatePrepare, json!({})).await);
    outcome(&fixture, &job).await;
    assert!(ok(&fixture, MethodName::EngineUpdateAbort, json!({}))
        .await
        .is_null());
    assert_eq!(world.calls("resume-held:"), 1);
    assert!(!engine_state(&fixture).update_pending);
    assert!(!engine_update::store(fixture.silod.as_ref()).exists());
    assert!(world.calls("guards released") >= 1);
    assert!(ok(&fixture, MethodName::EngineUpdateAbort, json!({}))
        .await
        .is_null());
    assert_eq!(world.calls("resume-held:"), 1);
}

#[tokio::test]
async fn engine_shutdown_for_an_update_releases_the_saved_set_and_exits() {
    let fixture = fixture();
    let world = World::with_running(&["dev"]);
    install_fake(&fixture, &world, None);
    // Refused before a completed save is held.
    let refused = call(
        &fixture,
        MethodName::EngineShutdown,
        json!({"stopComputers": false, "reason": "update"}),
    )
    .await
    .unwrap_err();
    assert!(refused.message.contains("not prepared"), "{refused:?}");
    assert!(!engine_state(&fixture).shutting_down);
    let stops = call(
        &fixture,
        MethodName::EngineShutdown,
        json!({"stopComputers": true, "reason": "update"}),
    )
    .await
    .unwrap_err();
    assert!(stops.message.contains("never stops computers"));

    let prepare = job_id(&ok(&fixture, MethodName::EngineUpdatePrepare, json!({})).await);
    outcome(&fixture, &prepare).await;
    let shutdown = ok(
        &fixture,
        MethodName::EngineShutdown,
        json!({"stopComputers": false, "reason": "update"}),
    )
    .await;
    let job = job_id(&shutdown);
    let finished = outcome(&fixture, &job).await;
    assert!(finished.error.is_none(), "{finished:?}");
    assert_eq!(world.calls("release:"), 1);
    assert_eq!(world.calls("stop-computers"), 0, "no guest shutdown");
    assert_eq!(
        world.calls("begin shutdown"),
        0,
        "admission was already held"
    );
    assert!(world.calls("flush settings") == 1 && world.calls("close connections") == 1);
    let journal = engine_update::store(fixture.silod.as_ref())
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(
        journal.phase,
        silo_engine::engine_update::journal::TransactionPhase::Released
    );
    // The exit follows the durable outcome.
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture.exits.0.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the engine never exited");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(*fixture.exits.0.lock().unwrap(), [0]);
    let state = engine_state(&fixture);
    assert!(state.shutting_down && state.update_pending && state.generation == 1);
    // An equivalent request attaches to the running shutdown.
    let again = ok(
        &fixture,
        MethodName::EngineShutdown,
        json!({"stopComputers": false, "reason": "update"}),
    )
    .await;
    assert_eq!(job_id(&again), job);
    let conflict = call(
        &fixture,
        MethodName::EngineShutdown,
        json!({"stopComputers": true, "reason": "user"}),
    )
    .await
    .unwrap_err();
    assert_eq!(conflict.code, ErrorCode::Busy);
}

#[tokio::test]
async fn engine_shutdown_stops_computers_then_exits_and_a_failure_reopens_service() {
    let fixture = fixture();
    let world = World::with_running(&["dev"]);
    install_fake(&fixture, &world, None);
    *world.stop_failure.lock().unwrap() = Some("Some local computers could not stop".into());
    let failing = job_id(
        &ok(
            &fixture,
            MethodName::EngineShutdown,
            json!({"stopComputers": true, "reason": "user"}),
        )
        .await,
    );
    let failed = outcome(&fixture, &failing).await;
    assert!(failed.error.unwrap().message.contains("could not stop"));
    assert!(fixture.exits.0.lock().unwrap().is_empty());
    assert!(world.calls("reopen admission") == 1);
    let state = engine_state(&fixture);
    assert!(!state.shutting_down && state.phase == silo_api::methods::engine::ShutdownPhase::Idle);

    *world.stop_failure.lock().unwrap() = None;
    let retry = job_id(
        &ok(
            &fixture,
            MethodName::EngineShutdown,
            json!({"stopComputers": true, "reason": "user", "deadlineMs": 5000}),
        )
        .await,
    );
    assert!(outcome(&fixture, &retry).await.error.is_none());
    assert_eq!(world.calls("stop-computers deadline=false"), 2);
    assert_eq!(engine_state(&fixture).generation, 2);
}

#[tokio::test]
async fn a_session_end_exits_even_when_computers_cannot_stop() {
    let fixture = fixture();
    let world = World::with_running(&["dev"]);
    install_fake(&fixture, &world, None);
    *world.stop_failure.lock().unwrap() = Some("stop failed".into());
    let job = job_id(
        &ok(
            &fixture,
            MethodName::EngineShutdown,
            json!({"stopComputers": true, "reason": "sessionEnd", "deadlineMs": 2000}),
        )
        .await,
    );
    assert!(engine_state(&fixture).session_ending);
    assert!(outcome(&fixture, &job).await.error.is_none());
    assert_eq!(world.calls("stop-computers deadline=true"), 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture.exits.0.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn sigterm_enters_a_session_end_that_stops_computers_and_exits_once() {
    let fixture = fixture();
    let world = World::with_running(&["dev"]);
    install_fake(&fixture, &world, None);
    session_end(fixture.silod.clone());
    assert!(engine_state(&fixture).session_ending);
    // A second signal attaches to the shutdown that is already under way.
    session_end(fixture.silod.clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture.exits.0.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the engine never exited");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(*fixture.exits.0.lock().unwrap(), [0]);
    assert_eq!(world.calls("stop-computers deadline=true"), 1);
}

#[tokio::test]
async fn a_shutdown_that_would_end_a_live_macos_machine_is_refused() {
    let fixture = fixture();
    let world = World::with_running(&["dev"]);
    install_fake(&fixture, &world, None);
    let refused = call(
        &fixture,
        MethodName::EngineShutdown,
        json!({"stopComputers": false, "reason": "user"}),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Busy);
    assert!(!engine_state(&fixture).shutting_down);
}

#[tokio::test]
async fn startup_recovery_resumes_the_saved_set_and_reports_what_could_not_resume() {
    // Engine one prepares and releases; engine two (a new instance over the same home) recovers.
    let first = fixture();
    let world = World::with_running(&["dev", "linux-1"]);
    install_fake(&first, &world, None);
    outcome(
        &first,
        &job_id(&ok(&first, MethodName::EngineUpdatePrepare, json!({})).await),
    )
    .await;
    let shutdown = job_id(
        &ok(
            &first,
            MethodName::EngineShutdown,
            json!({"stopComputers": false, "reason": "update"}),
        )
        .await,
    );
    outcome(&first, &shutdown).await;

    let second = fixture_at(&first.home);
    install_fake(&second, &world, None);
    *world.restore_failure.lock().unwrap() = Some("macOS refused the saved memory".into());
    let state = second.silod.runtime_state().update().clone();
    let silod = second.silod.clone();
    let failing = tokio::task::spawn_blocking({
        let state = state.clone();
        move || recover_saved_computers(&silod, &state)
    });
    failing.await.unwrap();
    assert_eq!(world.calls("restore:"), 2);
    // Both entries stay journaled, the update stays pending, and the failure is retained.
    assert!(second.silod.engine_state().update_pending);
    let notices = silo_engine::notices::list(second.silod.as_ref()).unwrap();
    let notice = notices
        .iter()
        .find(|notice| notice.key == "update:resume")
        .unwrap();
    assert!(
        notice.body.contains("macOS refused the saved memory"),
        "{notice:?}"
    );
    let snapshot = ok(&second, MethodName::UpdatesState, json!({})).await;
    assert_eq!(snapshot["phase"], "error");
    assert!(snapshot["error"]
        .as_str()
        .unwrap()
        .contains("could not resume"));
    // A new preparation is refused until they resolve; abort retries them.
    let blocked = call(&second, MethodName::EngineUpdatePrepare, json!({}))
        .await
        .unwrap_err();
    assert!(blocked
        .message
        .contains("previous update still has computers"));
    *world.restore_failure.lock().unwrap() = None;
    assert!(ok(&second, MethodName::EngineUpdateAbort, json!({}))
        .await
        .is_null());
    assert!(!second.silod.engine_state().update_pending);
    assert!(!engine_update::store(second.silod.as_ref()).exists());
}

#[tokio::test]
async fn startup_recovery_with_nothing_saved_changes_nothing() {
    let fixture = fixture();
    let world = World::with_running(&[]);
    install_fake(&fixture, &world, None);
    let state = fixture.silod.runtime_state().update().clone();
    let silod = fixture.silod.clone();
    tokio::task::spawn_blocking(move || recover_saved_computers(&silod, &state))
        .await
        .unwrap();
    assert!(silo_engine::notices::list(fixture.silod.as_ref())
        .unwrap()
        .is_empty());
    assert!(!fixture.silod.engine_state().update_pending);
}

// MARK: Updates

fn release(version: &str) -> Option<UpdateInfo> {
    Some(UpdateInfo {
        version: version.into(),
        body: Some("notes".into()),
    })
}

#[tokio::test]
async fn updates_state_reports_readiness_and_the_computers_an_update_would_save() {
    let fixture = fixture();
    let world = World::with_running(&["dev"]);
    install_fake(&fixture, &world, None);
    let snapshot = ok(&fixture, MethodName::UpdatesState, json!({})).await;
    assert_eq!(snapshot["phase"], "idle");
    assert_eq!(snapshot["currentVersion"], "1.0.0");
    assert_eq!(snapshot["packageKind"], "macos");
    assert_eq!(snapshot["canInstall"], true);
    assert_eq!(snapshot["runningComputers"], json!(["dev"]));
    assert!(snapshot.get("installStatus").is_none());
    // Once an update is being prepared, installing is refused.
    outcome(
        &fixture,
        &job_id(&ok(&fixture, MethodName::EngineUpdatePrepare, json!({})).await),
    )
    .await;
    let busy = ok(&fixture, MethodName::UpdatesState, json!({})).await;
    assert_eq!(busy["canInstall"], false);
    assert_eq!(
        busy["installBlockReason"],
        "An update operation is already running."
    );
}

#[tokio::test]
async fn updates_check_download_and_preferences_follow_the_engine_schedule() {
    let fixture = fixture();
    let world = World::with_running(&[]);
    install_fake(&fixture, &world, release("1.1.0"));
    let mut receiver = fixture
        .silod
        .connect_event_stream(fixture.handler.client_id().clone());
    let subscription = fixture
        .silod
        .subscribe_events(
            fixture.handler.client_id(),
            vec!["updates.*".to_owned(), "operations.*".to_owned()],
        )
        .unwrap();
    fixture
        .silod
        .activate_events(fixture.handler.client_id(), &subscription)
        .unwrap();

    // A download needs a found release.
    let early = call(&fixture, MethodName::UpdatesDownload, json!({}))
        .await
        .unwrap_err();
    assert!(early.message.contains("Check for an update"));

    let found = ok(&fixture, MethodName::UpdatesCheck, json!({})).await;
    assert_eq!(found["phase"], "available");
    assert_eq!(found["availableVersion"], "1.1.0");
    assert_eq!(found["releaseNotes"], "notes");

    let started = ok(&fixture, MethodName::UpdatesDownload, json!({})).await;
    let job = job_id(&started);
    let finished = outcome(&fixture, &job).await;
    let result = finished.result.unwrap();
    assert_eq!(result["phase"], "ready");
    assert_eq!(result["downloadedBytes"], 6);
    assert!(result.get("installStatus").is_none());

    let off = ok(
        &fixture,
        MethodName::UpdatesSetAutomaticChecks,
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(off["automaticChecks"], false);
    let on = ok(
        &fixture,
        MethodName::UpdatesSetAutomaticChecks,
        json!({"enabled": true}),
    )
    .await;
    assert_eq!(on["automaticChecks"], true);
    assert!(ok(&fixture, MethodName::UpdatesFocused, json!({}))
        .await
        .is_null());

    let mut changed = 0;
    let mut progress = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while changed < 5 || !progress {
        let delivery = tokio::time::timeout_at(deadline.into(), receiver.recv())
            .await
            .expect("events arrive")
            .expect("the stream stays open");
        if let silo_engine::events::EventDelivery::Event(event) = delivery {
            match event.event.name.as_str() {
                "updates.changed" => changed += 1,
                "operations.job.progress" => {
                    progress |= event.event.payload["source"] == "update";
                }
                _ => {}
            }
        }
    }
}

#[tokio::test]
async fn a_failed_update_check_is_a_typed_error_state_not_a_transport_failure() {
    let fixture = fixture();
    let world = World::with_running(&[]);
    let _ = world;
    fixture
        .silod
        .runtime_state()
        .update()
        .set_platform(Arc::new(FailingFeedPlatform));
    let snapshot = ok(&fixture, MethodName::UpdatesCheck, json!({})).await;
    assert_eq!(snapshot["phase"], "error");
    assert_eq!(snapshot["retryAction"], "check");
    assert!(snapshot["error"].as_str().unwrap().contains("unavailable"));
}

struct FailingFeedPlatform;

impl Platform for FailingFeedPlatform {
    fn adapters(&self) -> Vec<Box<dyn SaveAdapter>> {
        Vec::new()
    }

    fn acquire_guards(&self, _: Option<&JobId>) -> Result<Box<dyn HeldGuards>, String> {
        Err("unused".into())
    }

    fn stop_computers(&self, _: Option<Instant>) -> Result<(), String> {
        Ok(())
    }

    fn update_provider(&self) -> Box<dyn Provider> {
        struct Failing;
        impl Provider for Failing {
            fn check(&self) -> Result<Option<UpdateInfo>, CheckFailure> {
                Err(CheckFailure {
                    kind: CheckErrorKind::ReleaseUnavailable,
                    details: "offline".into(),
                })
            }

            fn download(&self, _: &mut dyn FnMut(u64, Option<u64>)) -> Result<Vec<u8>, String> {
                Err("offline".into())
            }
        }
        Box::new(Failing)
    }
}

#[test]
fn services_owns_exactly_its_prefixes_and_not_the_core_methods() {
    for method in [
        MethodName::SettingsRead,
        MethodName::UpdatesState,
        MethodName::EngineDisconnect,
        MethodName::EngineShutdown,
        MethodName::EngineUpdatePrepare,
        MethodName::EngineUpdateAbort,
    ] {
        assert!(owns(method), "{method:?}");
    }
    assert!(!owns(MethodName::EngineHandshake));
    assert!(!owns(MethodName::EngineState));
    assert!(!owns(MethodName::EventsSubscribe));
    let _ = Audience::All;
}
