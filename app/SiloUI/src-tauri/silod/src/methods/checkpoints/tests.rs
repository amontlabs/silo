//! One test per handler, over a throwaway engine context. Everything that reaches a global
//! (the macOS inventory, the operation gate, the export controller) runs under one lock.

use std::{
    sync::{Arc, MutexGuard},
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use silo_api::{
    jobs::{ErrorCode, JobId, JobState},
    methods::MethodName,
};
use silo_engine::context::{
    ClientId, EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl,
};

use crate::{
    context::SilodContext,
    dispatcher::{DispatchError, RequestContext},
    identity::LocalDeviceIdentity,
    methods::HandlerContext,
};

use super::handle;

const LINUX_ID: &str = "11111111-1111-4111-8111-111111111111";

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
        panic!("handler tests must not restart the process")
    }
}

struct Fixture {
    _lock: MutexGuard<'static, ()>,
    directory: tempfile::TempDir,
    silod: Arc<SilodContext>,
    context: HandlerContext,
    macos_id: String,
    macos_checkpoint: String,
}

fn fixture() -> Fixture {
    fixture_with(false)
}

/// `converted_backup`: the device was migrated by conversion, so `runtime/` is a pre-upgrade
/// backup instead of the clean generation being in use.
fn fixture_with(converted_backup: bool) -> Fixture {
    let lock = crate::methods::MACOS_REGISTRY_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let paths = EnginePaths {
        app_data: root.join("data"),
        app_config: root.join("config"),
        app_cache: root.join("cache"),
        app_log: root.join("log"),
        resources: root.join("resources"),
        home: root.join("home"),
    };
    for path in [&paths.app_data, &paths.app_config, &paths.home] {
        std::fs::create_dir_all(path).unwrap();
    }
    // A migrated device (the clean generation is selected and complete) whose inventory holds
    // a Linux computer, and a macOS computer with a checkpoint.
    let generation = if converted_backup {
        "runtime-checkpoints-converted"
    } else {
        "runtime-checkpoints-clean"
    };
    let storage = paths.app_data.join(generation);
    std::fs::create_dir_all(&storage).unwrap();
    if converted_backup {
        let previous = paths.app_data.join("runtime");
        std::fs::create_dir_all(&previous).unwrap();
        std::fs::write(previous.join("computers.json"), b"previous").unwrap();
    }
    std::fs::write(
        paths.app_data.join("runtime-generation.json"),
        json!({"version": 1, "directory": generation}).to_string(),
    )
    .unwrap();
    std::fs::write(
        paths.app_data.join("runtime-migration.json"),
        json!({
            "version": 1, "status": "complete", "stage": "Migration complete", "logs": [],
            "migratedCount": 0, "failedCount": 0, "totalCount": 0, "canContinue": false
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        storage.join("computers.json"),
        json!({"schemaVersion": 1, "computers": [{
            "id": LINUX_ID, "name": "linux-dev", "cpus": 1, "maxCPUs": 1, "memoryGiB": 1,
            "maxMemoryGiB": 1, "workspaceStorageGiB": 1, "runtimeStorageGiB": 1
        }]})
        .to_string(),
    )
    .unwrap();
    silo_engine::macos_computers_engine::reset_registry_for_test();
    let (macos_id, macos_checkpoint) =
        silo_engine::macos_computers_engine::seed_test_computer(&paths.app_data, "mac-dev");
    let silod = Arc::new(SilodContext::new(
        paths,
        EngineIdentity {
            channel: silo_engine::channel::Channel::Production,
            version: "test".into(),
            package_kind: PackageKind::MacApp,
        },
        LocalDeviceIdentity {
            device_id: "test-device".into(),
            name: "Test Mac".into(),
        },
        Arc::new(NoopMainThread),
        Arc::new(NoopProcess),
    ));
    let context = HandlerContext::new(
        silod.clone(),
        RequestContext {
            client_id: "test-client".into(),
            protocol: 8,
            client: None,
            caller: silo_engine::policy::Caller::owner("test"),
        },
    );
    Fixture {
        _lock: lock,
        directory,
        silod,
        context,
        macos_id,
        macos_checkpoint,
    }
}

async fn call(
    fixture: &Fixture,
    method: MethodName,
    params: Value,
) -> Result<Value, DispatchError> {
    handle(method, &params, &fixture.context)
        .await
        .expect("the domain owns the method")
        .map(|output| output.value)
}

fn job_id(result: Value) -> JobId {
    JobId::new(result["jobId"].as_str().expect("a {jobId} result")).unwrap()
}

async fn finished(fixture: &Fixture, job_id: &JobId) -> silo_api::jobs::JobLookupResult {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let found = silo_engine::jobs::get_at(fixture.silod.as_ref(), job_id, None)
            .unwrap()
            .expect("the job was accepted durably");
        if found.state == JobState::Finished {
            return found;
        }
        assert!(Instant::now() < deadline, "{job_id} never finished");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn next_event(
    events: &mut tokio::sync::mpsc::Receiver<silo_engine::context::Event>,
    name: &str,
) -> silo_engine::context::Event {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("the event arrives")
            .expect("the subscription stays open");
        if event.name == name {
            return event;
        }
    }
}

fn error_code(result: Result<Value, DispatchError>) -> ErrorCode {
    result.expect_err("the request is refused").code
}

// MARK: checkpoints.*

#[tokio::test]
async fn every_checkpoint_method_refuses_a_request_for_another_device() {
    let fixture = fixture();
    let other = json!("some-other-device");
    for (method, params) in [
        (
            MethodName::CheckpointsCreate,
            json!({"computerId": LINUX_ID, "deviceId": other, "name": "x"}),
        ),
        (
            MethodName::CheckpointsRestore,
            json!({"computerId": LINUX_ID, "deviceId": other, "checkpointId": "c"}),
        ),
        (
            MethodName::CheckpointsFork,
            json!({"computerId": LINUX_ID, "deviceId": other, "checkpointId": null, "newName": "x"}),
        ),
        (
            MethodName::CheckpointsDelete,
            json!({"computerId": LINUX_ID, "deviceId": other, "checkpointId": "c"}),
        ),
        (
            MethodName::CheckpointsAbandonRestore,
            json!({"computerId": LINUX_ID, "deviceId": other}),
        ),
        (
            MethodName::CheckpointsUsage,
            json!({"computerId": LINUX_ID, "deviceId": other}),
        ),
    ] {
        assert_eq!(
            error_code(call(&fixture, method, params).await),
            ErrorCode::UnsupportedRemoteOperation,
            "{method:?}"
        );
    }
}

#[tokio::test]
async fn checkpoints_usage_returns_the_canonical_shape_for_both_platforms() {
    let fixture = fixture();
    // macOS computers are loaded only on an Apple silicon host.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        let macos = call(
            &fixture,
            MethodName::CheckpointsUsage,
            json!({"computerId": fixture.macos_id}),
        )
        .await
        .unwrap();
        let macos: silo_api::methods::checkpoints::CheckpointUsage =
            serde_json::from_value(macos).unwrap();
        assert_eq!(macos.total_bytes, None);
        assert_eq!(macos.checkpoints[0].id, fixture.macos_checkpoint);
    }

    let linux = call(
        &fixture,
        MethodName::CheckpointsUsage,
        json!({"computerId": LINUX_ID, "deviceId": "test-device"}),
    )
    .await
    .unwrap();
    assert_eq!(linux, json!({"totalBytes": 0, "checkpoints": []}));

    assert_eq!(
        error_code(
            call(
                &fixture,
                MethodName::CheckpointsUsage,
                json!({"computerId": "22222222-2222-4222-8222-222222222222"}),
            )
            .await
        ),
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn checkpoints_create_starts_a_durable_job_whose_outcome_is_readable() {
    let fixture = fixture();
    // The Linux runtime is not bundled in this fixture, so the capture itself fails; the
    // point is the contract: an immediate {jobId}, then one persisted terminal outcome.
    let started = call(
        &fixture,
        MethodName::CheckpointsCreate,
        json!({"computerId": LINUX_ID, "name": "Before work"}),
    )
    .await
    .unwrap();
    let id = job_id(started);
    assert!(id.as_str().starts_with("job:"));
    let outcome = finished(&fixture, &id).await.outcome.unwrap();
    assert!(outcome.result.is_none());
    assert_eq!(outcome.error.unwrap().code, ErrorCode::Internal);
}

#[tokio::test]
async fn an_automatic_create_is_refused_while_the_retention_setting_is_zero() {
    let fixture = fixture();
    std::fs::write(
        fixture.directory.path().join("config/settings.json"),
        json!({"schemaVersion": 1, "settings": {"automaticCheckpointRetentionCount": 0}, "onboardingDraft": null})
            .to_string(),
    )
    .unwrap();
    for computer_id in [LINUX_ID.to_owned(), fixture.macos_id.clone()] {
        let refused = call(
            &fixture,
            MethodName::CheckpointsCreate,
            json!({"computerId": computer_id, "name": "Before run: x, deleted automatically", "kind": "automatic"}),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::PermissionDenied);
        assert!(refused.message.contains("turned off"));
    }
}

// macOS computers are loaded only on an Apple silicon host.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
async fn an_automatic_create_with_the_default_setting_starts_a_job() {
    let fixture = fixture();
    // No settings file: the default of three applies, so the capture is accepted.
    let started = call(
        &fixture,
        MethodName::CheckpointsCreate,
        json!({"computerId": fixture.macos_id, "name": "Before run: x, deleted automatically", "kind": "automatic"}),
    )
    .await
    .unwrap();
    let id = job_id(started);
    finished(&fixture, &id).await;
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
async fn macos_restore_delete_and_abandon_each_start_a_job() {
    let fixture = fixture();
    for (method, params) in [
        (
            MethodName::CheckpointsRestore,
            json!({"computerId": fixture.macos_id, "checkpointId": fixture.macos_checkpoint}),
        ),
        (
            MethodName::CheckpointsDelete,
            json!({"computerId": fixture.macos_id, "checkpointId": fixture.macos_checkpoint}),
        ),
        (
            MethodName::CheckpointsAbandonRestore,
            json!({"computerId": fixture.macos_id}),
        ),
    ] {
        let id = job_id(call(&fixture, method, params).await.unwrap());
        assert!(id.as_str().starts_with("job:"), "{method:?}");
        finished(&fixture, &id).await;
    }
}

#[tokio::test]
async fn linux_restore_delete_and_abandon_each_start_a_job_that_ends_in_the_runtime_error() {
    let fixture = fixture();
    for (method, params) in [
        (
            MethodName::CheckpointsRestore,
            json!({"computerId": LINUX_ID, "checkpointId": "c000000000000000000000000000000"}),
        ),
        (
            MethodName::CheckpointsDelete,
            json!({"computerId": LINUX_ID, "checkpointId": "c000000000000000000000000000000"}),
        ),
        (
            MethodName::CheckpointsAbandonRestore,
            json!({"computerId": LINUX_ID}),
        ),
    ] {
        let id = job_id(call(&fixture, method, params).await.unwrap());
        let outcome = finished(&fixture, &id).await.outcome.unwrap();
        assert!(outcome.error.is_some(), "{method:?}");
    }
}

#[tokio::test]
async fn checkpoints_fork_starts_a_job_on_both_platforms_with_or_without_a_checkpoint() {
    let fixture = fixture();
    // macOS computers are loaded only on an Apple silicon host.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        let id = job_id(
            call(
                &fixture,
                MethodName::CheckpointsFork,
                json!({"computerId": fixture.macos_id, "checkpointId": null, "newName": "current"}),
            )
            .await
            .unwrap(),
        );
        finished(&fixture, &id).await;

        let id = job_id(
            call(
                &fixture,
                MethodName::CheckpointsFork,
                json!({"computerId": fixture.macos_id, "checkpointId": fixture.macos_checkpoint, "newName": "branch"}),
            )
            .await
            .unwrap(),
        );
        finished(&fixture, &id).await;
    }

    let id = job_id(
        call(
            &fixture,
            MethodName::CheckpointsFork,
            json!({"computerId": LINUX_ID, "checkpointId": null, "newName": "branch"}),
        )
        .await
        .unwrap(),
    );
    assert!(finished(&fixture, &id)
        .await
        .outcome
        .unwrap()
        .error
        .is_some());
}

#[tokio::test]
async fn changes_request_records_an_open_request_and_answers_with_it() {
    let fixture = fixture();
    super::requests::tests_rows::set(Vec::new());
    super::requests::tests_rows::set_sources(&[(LINUX_ID, "main-id")]);
    let made = call(
        &fixture,
        MethodName::ChangesRequest,
        json!({"forkId": LINUX_ID, "targetId": "main-id", "reviewDigest": "d1", "summary": "2 files"}),
    )
    .await
    .unwrap();
    let request = &made["request"];
    assert_eq!(request["status"], "open");
    assert_eq!(request["forkId"], LINUX_ID);
    assert_eq!(request["target"], "main-id");
    assert_eq!(request["reviewDigest"], "d1");
    assert!(request["id"].as_str().unwrap().starts_with("fr-"));
    let again = call(
        &fixture,
        MethodName::ChangesRequest,
        json!({"forkId": LINUX_ID, "targetId": "main-id", "summary": "3 files"}),
    )
    .await
    .unwrap();
    assert_eq!(again["request"]["id"], request["id"]);
    assert_eq!(again["request"]["summary"], "3 files");
    // Without a target and without a fork-time record there is nowhere to keep it.
    super::requests::tests_rows::set_sources(&[]);
    assert!(call(
        &fixture,
        MethodName::ChangesRequest,
        json!({"forkId": LINUX_ID})
    )
    .await
    .is_err());
}

// MARK: changes.requests.*

/// An engine whose computers are `linux-dev` (stopped) and `main` (`main-id`, running).
fn request_fixture() -> Fixture {
    use super::requests::{tests_rows, Row};
    let fixture = fixture();
    tests_rows::set(vec![
        Row {
            id: LINUX_ID.into(),
            name: "linux-dev".into(),
            state: "stopped".into(),
            configuration: Some(json!({"id": LINUX_ID, "name": "linux-dev"})),
            checkpoints: Vec::new(),
        },
        Row {
            id: "main-id".into(),
            name: "main".into(),
            state: "running".into(),
            configuration: None,
            checkpoints: vec!["cp-main".into()],
        },
    ]);
    // Both forks were forked from main; the CLI recorded a folder for the first.
    tests_rows::set_sources(&[(LINUX_ID, "main-id"), ("no-such-fork", "main-id")]);
    let state = fixture.directory.path().join("home/.silo");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(
        state.join("cli-folders.json"),
        json!({ LINUX_ID: {"path": "/work/app"} }).to_string(),
    )
    .unwrap();
    fixture
}

async fn make_request(fixture: &Fixture, fork: &str, target: &str, digest: &str) -> Value {
    call(
        fixture,
        MethodName::ChangesRequest,
        json!({"forkId": fork, "targetId": target, "reviewDigest": digest, "summary": "2 files",
               "counts": {"code": 2}}),
    )
    .await
    .unwrap()["request"]
        .clone()
}

#[tokio::test]
async fn requests_list_filters_by_status_and_names_the_fork() {
    let fixture = request_fixture();
    let made = make_request(&fixture, LINUX_ID, "main-id", "d1").await;
    let gone = make_request(&fixture, "no-such-fork", "main-id", "d1").await;
    let listed = call(&fixture, MethodName::ChangesRequestsList, json!({}))
        .await
        .unwrap();
    // The request of the missing fork is closed by the listing, so it is no longer open.
    let entries = listed["requests"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    let all = call(
        &fixture,
        MethodName::ChangesRequestsList,
        json!({"status": "all"}),
    )
    .await
    .unwrap();
    let entries = all["requests"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    let known = entries
        .iter()
        .find(|entry| entry["request"]["id"] == made["id"])
        .unwrap();
    assert_eq!(known["forkName"], "linux-dev");
    assert_eq!(known["forkState"], "stopped");
    assert_eq!(known["request"]["counts"]["code"], 2);
    let missing = entries
        .iter()
        .find(|entry| entry["request"]["id"] == gone["id"])
        .unwrap();
    assert!(missing.get("forkName").is_none());
    let kept = call(
        &fixture,
        MethodName::ChangesRequestsList,
        json!({"status": "kept"}),
    )
    .await
    .unwrap();
    assert!(kept["requests"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn requests_keep_refuses_what_the_engine_cannot_keep() {
    let fixture = request_fixture();
    let folder = make_request(&fixture, LINUX_ID, "folder:/work/app", "d1").await;
    let error = call(
        &fixture,
        MethodName::ChangesRequestsKeep,
        json!({"requestId": folder["id"]}),
    )
    .await
    .expect_err("a folder fork is kept with silo fold");
    assert!(error.message.contains("silo fold"), "{}", error.message);
    let undigested = call(
        &fixture,
        MethodName::ChangesRequest,
        json!({"forkId": LINUX_ID, "targetId": "main-id"}),
    )
    .await
    .unwrap()["request"]
        .clone();
    let error = call(
        &fixture,
        MethodName::ChangesRequestsKeep,
        json!({"requestId": undigested["id"]}),
    )
    .await
    .expect_err("a request without a digest cannot pin a keep");
    assert!(error.message.contains("no review digest"));
    let error = call(
        &fixture,
        MethodName::ChangesRequestsKeep,
        json!({"requestId": "fr-nothing"}),
    )
    .await
    .expect_err("an unknown request");
    assert!(error.message.contains("no review request"));
    let gone = make_request(&fixture, "no-such-fork", "main-id", "d1").await;
    let error = call(
        &fixture,
        MethodName::ChangesRequestsKeep,
        json!({"requestId": gone["id"]}),
    )
    .await
    .expect_err("the fork is gone");
    assert!(error.message.contains("no longer exists"));
}

#[tokio::test]
async fn requests_throw_away_never_drops_the_target() {
    let fixture = request_fixture();
    // A request is never made for a fork that is its own target, or for main.
    for (fork, target) in [(LINUX_ID, LINUX_ID), ("main-id", LINUX_ID)] {
        let refused = call(
            &fixture,
            MethodName::ChangesRequest,
            json!({"forkId": fork, "targetId": target, "reviewDigest": "d1"}),
        )
        .await
        .expect_err("not a fork of that target");
        assert!(
            refused.message.contains("was not forked from"),
            "{}",
            refused.message
        );
    }
    // A request written some other way is checked again before anything is dropped.
    let store = silo_engine::fold_requests::Store::new(&fixture.directory.path().join("data"));
    for (fork, target, message) in [
        (LINUX_ID, LINUX_ID, "was not forked from"),
        ("main-id", LINUX_ID, "was not forked from"),
        (LINUX_ID, "other-id", "was not forked from"),
    ] {
        let request = store
            .create(silo_engine::fold_requests::NewRequest {
                fork_id: fork.into(),
                target: target.into(),
                review_digest: "d1".into(),
                summary: String::new(),
                counts: Default::default(),
            })
            .unwrap();
        let started = call(
            &fixture,
            MethodName::ChangesRequestsThrowAway,
            json!({"requestId": request.id}),
        )
        .await
        .unwrap();
        let outcome = finished(&fixture, &job_id(started)).await.outcome.unwrap();
        assert!(outcome.error.unwrap().message.contains(message));
        assert_eq!(
            store.get(&request.id).unwrap().unwrap().status,
            silo_api::methods::keep::FoldRequestStatus::Open
        );
    }
}

#[tokio::test]
async fn one_job_at_a_time_works_on_a_request() {
    let fixture = request_fixture();
    let request = make_request(&fixture, LINUX_ID, "main-id", "d1").await;
    let id = request["id"].as_str().unwrap().to_owned();
    let _held = silo_engine::fold_requests::claim(&id).unwrap();
    for method in [
        MethodName::ChangesRequestsKeep,
        MethodName::ChangesRequestsThrowAway,
    ] {
        let error = call(&fixture, method, json!({"requestId": id}))
            .await
            .expect_err("a job already holds the request");
        assert!(error.message.contains("right now"), "{}", error.message);
    }
}

#[tokio::test]
async fn throw_away_does_not_conclude_a_fork_is_gone_from_an_incomplete_list() {
    let fixture = request_fixture();
    let request = make_request(&fixture, "no-such-fork", "main-id", "d1").await;
    super::requests::tests_rows::set_complete(false);
    let error = call(
        &fixture,
        MethodName::ChangesRequestsThrowAway,
        json!({"requestId": request["id"]}),
    )
    .await
    .expect_err("the list may be missing the fork");
    assert!(error.message.contains("incomplete"), "{}", error.message);
    let listed = call(&fixture, MethodName::ChangesRequestsList, json!({}))
        .await
        .unwrap();
    assert_eq!(listed["requests"][0]["request"]["status"], "open");
}

#[tokio::test]
async fn listing_closes_open_requests_whose_fork_is_gone_from_a_complete_list() {
    let fixture = request_fixture();
    make_request(&fixture, "no-such-fork", "main-id", "d1").await;
    let listed = call(
        &fixture,
        MethodName::ChangesRequestsList,
        json!({"status": "all"}),
    )
    .await
    .unwrap();
    let request = &listed["requests"][0]["request"];
    assert_eq!(request["status"], "thrown-away");
    assert_eq!(request["closedReason"], "fork-deleted");
}

#[tokio::test]
async fn show_reads_an_answered_request_whose_fork_is_gone() {
    let fixture = request_fixture();
    let request = make_request(&fixture, "no-such-fork", "main-id", "d1").await;
    let id = request["id"].clone();
    // Open: the fork is needed.
    assert!(call(
        &fixture,
        MethodName::ChangesRequestsShow,
        json!({"requestId": id})
    )
    .await
    .is_err());
    let closed = call(
        &fixture,
        MethodName::ChangesRequestsThrowAway,
        json!({"requestId": id}),
    )
    .await
    .unwrap();
    finished(&fixture, &job_id(closed)).await;
    let shown = call(
        &fixture,
        MethodName::ChangesRequestsShow,
        json!({"requestId": id}),
    )
    .await
    .unwrap();
    let result = finished(&fixture, &job_id(shown))
        .await
        .outcome
        .unwrap()
        .result
        .unwrap();
    assert_eq!(result["diff"], Value::Null);
    assert_eq!(result["forkState"], "gone");
    assert_eq!(result["request"]["status"], "thrown-away");
}

#[tokio::test]
async fn open_starts_only_a_stopped_fork_and_gives_up_on_one_that_never_runs() {
    let fixture = request_fixture();
    use super::requests::{tests_rows, Row};
    let row = |state: &str| Row {
        id: LINUX_ID.into(),
        name: "linux-dev".into(),
        state: state.into(),
        configuration: None,
        checkpoints: Vec::new(),
    };
    let request = make_request(&fixture, LINUX_ID, "main-id", "d1").await;
    // Already running: reported as is, nothing started.
    tests_rows::set(vec![row("running")]);
    tests_rows::set_sources(&[(LINUX_ID, "main-id")]);
    let generation_before = silo_engine::generation::generation(LINUX_ID);
    let started = call(
        &fixture,
        MethodName::ChangesRequestsOpen,
        json!({"requestId": request["id"]}),
    )
    .await
    .unwrap();
    let result = finished(&fixture, &job_id(started))
        .await
        .outcome
        .unwrap()
        .result
        .unwrap();
    assert_eq!(
        (result["state"].as_str(), result["started"].as_bool()),
        (Some("running"), Some(false))
    );
    assert_eq!(result["sshAlias"], "linux-dev.silo");
    assert!(silo_engine::generation::generation(LINUX_ID) > generation_before);
    // Starting, and never running: not started again, and the wait ends with an error.
    tests_rows::set(vec![row("starting")]);
    let started = call(
        &fixture,
        MethodName::ChangesRequestsOpen,
        json!({"requestId": request["id"]}),
    )
    .await
    .unwrap();
    let outcome = finished(&fixture, &job_id(started)).await.outcome.unwrap();
    assert!(outcome
        .error
        .unwrap()
        .message
        .contains("did not reach running"));
}

#[tokio::test]
async fn waiting_for_a_job_ends_when_its_record_is_missing_or_it_never_finishes() {
    let fixture = request_fixture();
    let missing =
        super::requests::wait_for(&fixture.silod, "job:999999", Duration::from_secs(5)).await;
    assert!(missing.unwrap_err().contains("is gone"));
    let accepted = silo_engine::jobs::allocate(fixture.silod.as_ref(), "test.slow").unwrap();
    let slow = super::requests::wait_for(
        &fixture.silod,
        accepted.job_id.as_str(),
        Duration::from_millis(600),
    )
    .await;
    assert!(slow.unwrap_err().contains("did not finish"));
}

#[tokio::test]
async fn requests_throw_away_closes_a_request_whose_fork_is_gone() {
    let fixture = request_fixture();
    let request = make_request(&fixture, "no-such-fork", "main-id", "d1").await;
    let started = call(
        &fixture,
        MethodName::ChangesRequestsThrowAway,
        json!({"requestId": request["id"]}),
    )
    .await
    .unwrap();
    let outcome = finished(&fixture, &job_id(started)).await.outcome.unwrap();
    assert_eq!(outcome.result.unwrap()["request"]["status"], "thrown-away");
    let error = call(
        &fixture,
        MethodName::ChangesRequestsThrowAway,
        json!({"requestId": request["id"]}),
    )
    .await
    .expect_err("an answered request cannot be answered again");
    assert!(error.message.contains("already thrown away"));
}

#[tokio::test]
async fn requests_show_a_folder_fork_with_its_own_summary_and_no_credentials() {
    let fixture = request_fixture();
    let request = make_request(&fixture, LINUX_ID, "folder:/work/app", "d1").await;
    let started = call(
        &fixture,
        MethodName::ChangesRequestsShow,
        json!({"requestId": request["id"]}),
    )
    .await
    .unwrap();
    let result = finished(&fixture, &job_id(started))
        .await
        .outcome
        .unwrap()
        .result
        .unwrap();
    assert_eq!(result["text"], "2 files");
    assert_eq!(result["diff"], Value::Null);
    assert!(result.get("stale").is_none());
    assert_eq!(result["credentials"], json!({"used": [], "blocked": []}));
    assert_eq!(result["forkName"], "linux-dev");
    assert_eq!(result["request"]["id"], request["id"]);
}

#[tokio::test]
async fn a_fork_whose_source_is_a_checkpoint_of_the_target_may_have_a_request() {
    let fixture = request_fixture();
    // The record names a checkpoint of main (no origin was stored): it stands for main.
    super::requests::tests_rows::set_sources(&[("fork-cp", "cp-main"), ("fork-x", "cp-other")]);
    let made = call(
        &fixture,
        MethodName::ChangesRequest,
        json!({"forkId": "fork-cp", "targetId": "main-id", "reviewDigest": "d1"}),
    )
    .await
    .unwrap();
    assert_eq!(made["request"]["target"], "main-id");
    let error = call(
        &fixture,
        MethodName::ChangesRequest,
        json!({"forkId": "fork-x", "targetId": "main-id", "reviewDigest": "d1"}),
    )
    .await
    .expect_err("a checkpoint no computer owns is not main");
    assert!(error.message.contains("was not forked from"));
}

#[tokio::test]
async fn mark_kept_closes_the_requests_of_a_folder_fork_folded_by_the_cli() {
    let fixture = request_fixture();
    let folder = make_request(&fixture, LINUX_ID, "folder:/work/app", "d1").await;
    let other = make_request(&fixture, LINUX_ID, "main-id", "d1").await;
    let marked = call(
        &fixture,
        MethodName::ChangesRequestsMarkKept,
        json!({"forkId": LINUX_ID, "target": "folder:/work/app", "keepId": "refs/silo/backup/1"}),
    )
    .await
    .unwrap();
    assert_eq!(marked["requests"][0]["id"], folder["id"]);
    assert_eq!(marked["requests"][0]["status"], "kept");
    assert_eq!(marked["requests"][0]["keepId"], "refs/silo/backup/1");
    // The request for the computer target stays open; a pair that is no fork is refused.
    let listed = call(&fixture, MethodName::ChangesRequestsList, json!({}))
        .await
        .unwrap();
    assert_eq!(listed["requests"][0]["request"]["id"], other["id"]);
    assert!(call(
        &fixture,
        MethodName::ChangesRequestsMarkKept,
        json!({"forkId": "main-id", "target": "folder:/work/app"}),
    )
    .await
    .is_err());
}

// MARK: steps.*

#[tokio::test]
async fn steps_are_numbered_per_computer_and_listed_oldest_first() {
    let fixture = fixture();
    let record = |command: Value, checkpoint: Option<&str>| {
        json!({"computerId": LINUX_ID, "command": command, "exitCode": 2,
            "startedAt": "2026-01-01T00:00:00Z", "finishedAt": "2026-01-01T00:00:01Z",
            "checkpointId": checkpoint})
    };
    let first = call(
        &fixture,
        MethodName::StepsRecord,
        record(json!(["make", "test"]), Some("cp-1")),
    )
    .await
    .unwrap();
    assert_eq!(first["step"]["number"], 1);
    assert_eq!(first["step"]["command"], "make test");
    assert_eq!(first["step"]["exitCode"], 2);
    let second = call(
        &fixture,
        MethodName::StepsRecord,
        record(json!(["ls"]), None),
    )
    .await
    .unwrap();
    assert_eq!(second["step"]["number"], 2);
    assert_eq!(second["step"]["parentStep"], 1);
    let listed = call(
        &fixture,
        MethodName::StepsList,
        json!({"computerId": LINUX_ID}),
    )
    .await
    .unwrap();
    let numbers: Vec<u64> = listed["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["number"].as_u64().unwrap())
        .collect();
    assert_eq!(numbers, [1, 2]);
    assert_eq!(listed["steps"][0]["checkpointId"], "cp-1");
    // A computer that does not exist, and a record with no command, are refused.
    let missing = json!({"computerId": "22222222-2222-4222-8222-222222222222",
        "command": ["ls"], "startedAt": "a", "finishedAt": "b"});
    assert!(call(&fixture, MethodName::StepsRecord, missing)
        .await
        .is_err());
    assert!(
        call(&fixture, MethodName::StepsRecord, record(json!([]), None))
            .await
            .is_err()
    );
}

// MARK: exports.*

#[tokio::test]
async fn exports_destination_set_remembers_a_canonical_folder_and_state_reports_it() {
    let fixture = fixture();
    let folder = fixture.directory.path().join("exports");
    std::fs::create_dir_all(&folder).unwrap();
    let mut events = fixture.silod.subscribe(ClientId::new("test-client"));
    let set = call(
        &fixture,
        MethodName::ExportsDestinationSet,
        json!({"path": folder.join("../exports")}),
    )
    .await
    .unwrap();
    let canonical = std::fs::canonicalize(&folder).unwrap();
    assert_eq!(set, json!({"destination": canonical.to_str().unwrap()}));

    // The change is announced with the same state a read returns, destination included.
    let event = next_event(&mut events, "exports.changed").await;
    assert_eq!(event.payload["destination"], canonical.to_str().unwrap());

    let state = call(&fixture, MethodName::ExportsState, json!({}))
        .await
        .unwrap();
    let state: silo_api::methods::exports::BackupState = serde_json::from_value(state).unwrap();
    assert_eq!(state.destination.as_deref(), canonical.to_str());
    assert_eq!(state.operation, None);

    let not_a_folder = fixture.directory.path().join("file");
    std::fs::write(&not_a_folder, b"x").unwrap();
    let refused = call(
        &fixture,
        MethodName::ExportsDestinationSet,
        json!({"path": not_a_folder}),
    )
    .await
    .unwrap_err();
    assert!(refused
        .message
        .starts_with("Silo could not use the selected destination"));
    let missing = call(
        &fixture,
        MethodName::ExportsDestinationSet,
        json!({"path": fixture.directory.path().join("missing")}),
    )
    .await
    .unwrap_err();
    assert!(missing
        .message
        .starts_with("Silo could not use the selected destination"));
}

#[tokio::test]
async fn exports_inspect_returns_an_inspect_job_that_reports_an_invalid_file_as_data() {
    let fixture = fixture();
    let archive = fixture.directory.path().join("missing.silo-backup");
    let started = call(
        &fixture,
        MethodName::ExportsInspect,
        json!({"archivePath": archive, "requestId": "review-1"}),
    )
    .await
    .unwrap();
    assert_eq!(started, json!({"jobId": "inspect:review-1"}));
    let id = job_id(started);
    let outcome = finished(&fixture, &id).await.outcome.unwrap();
    let result: silo_api::methods::exports::ArchiveInspectionResult =
        serde_json::from_value(outcome.result.expect("an invalid file is a result")).unwrap();
    assert!(!result.valid);
    assert!(result.reason.is_some());

    // Repeating the request id returns the job it already has instead of starting a second.
    let repeated = call(
        &fixture,
        MethodName::ExportsInspect,
        json!({"archivePath": archive, "requestId": "review-1"}),
    )
    .await
    .unwrap();
    assert_eq!(repeated, json!({"jobId": "inspect:review-1"}));
}

#[tokio::test]
async fn exports_inspect_finishes_an_accepted_job_whose_start_cannot_be_saved() {
    let fixture = fixture();
    let registry = silo_engine::context::EngineContext::job_registry(fixture.silod.as_ref())
        .expect("the job registry opens");
    rusqlite::Connection::open(registry.path())
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_running BEFORE UPDATE OF state ON jobs
             WHEN NEW.state = 'running'
             BEGIN SELECT RAISE(ABORT, 'injected mark_running failure'); END;",
        )
        .unwrap();

    let started = call(
        &fixture,
        MethodName::ExportsInspect,
        json!({
            "archivePath": fixture.directory.path().join("any.silo-backup"),
            "requestId": "stranded-1"
        }),
    )
    .await
    .unwrap();
    assert_eq!(started, json!({"jobId": "inspect:stranded-1"}));
    let outcome = finished(&fixture, &job_id(started)).await.outcome.unwrap();
    let error = outcome.error.expect("the startup failure is the outcome");
    assert!(
        error.message.contains("injected mark_running failure"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn exports_create_needs_the_remembered_destination() {
    let fixture = fixture();
    let refused = call(
        &fixture,
        MethodName::ExportsCreate,
        json!({"destination": fixture.directory.path(), "computerIds": [LINUX_ID]}),
    )
    .await
    .unwrap_err();
    assert!(refused
        .message
        .contains("Choose the export destination again"));

    let unknown = call(
        &fixture,
        MethodName::ExportsCreate,
        json!({
            "destination": fixture.directory.path(),
            "computerIds": ["99999999-9999-4999-8999-999999999999"]
        }),
    )
    .await
    .unwrap_err();
    assert!(unknown.message.contains("is not managed by Silo"));
}

#[tokio::test]
async fn exports_create_resolves_a_macos_computer_and_hands_it_to_the_archive_writer() {
    let fixture = fixture();
    let folder = fixture.directory.path().join("exports");
    std::fs::create_dir_all(&folder).unwrap();
    call(
        &fixture,
        MethodName::ExportsDestinationSet,
        json!({"path": folder}),
    )
    .await
    .unwrap();

    // The macOS id is found in the Apple Virtualization registry, not refused by platform
    // and not mistaken for a missing Linux computer: an export job is accepted.
    let started = call(
        &fixture,
        MethodName::ExportsCreate,
        json!({"destination": folder, "computerIds": [fixture.macos_id]}),
    )
    .await
    .unwrap();
    let id = job_id(started);
    assert!(id.as_str().starts_with("exports:"));
    let outcome = finished(&fixture, &id).await.outcome.unwrap();
    // The test binary is not signed with the virtualization entitlement, so the writer
    // itself reports the failure; it was reached for the named macOS computer.
    let result = outcome.result.expect("the export ends in a result");
    assert_eq!(result["outcome"], "failed");
    assert_eq!(result["archive"]["computers"], json!(["mac-dev"]));
    assert!(!result.to_string().contains("cannot be exported yet"));
}

#[tokio::test]
async fn the_logins_check_says_whether_the_export_leaves_them_out() {
    let fixture = fixture();
    let check = |include: Option<bool>| {
        let mut params = json!({"computerId": LINUX_ID});
        if let Some(include) = include {
            params["includeLogins"] = json!(include);
        }
        params
    };
    let left_out = job_id(
        call(&fixture, MethodName::ExportsCheck, check(None))
            .await
            .unwrap(),
    );
    let found = finished(&fixture, &left_out).await.outcome.unwrap();
    assert_eq!(found.result.unwrap()["leavesOut"], json!(true));
    let included = job_id(
        call(&fixture, MethodName::ExportsCheck, check(Some(true)))
            .await
            .unwrap(),
    );
    let found = finished(&fixture, &included).await.outcome.unwrap();
    assert!(found.result.unwrap().get("leavesOut").is_none());
    // A macOS computer's logins stay in its file, whatever the choice.
    let macos = job_id(
        call(
            &fixture,
            MethodName::ExportsCheck,
            json!({"computerId": fixture.macos_id}),
        )
        .await
        .unwrap(),
    );
    let found = finished(&fixture, &macos).await.outcome.unwrap();
    assert!(found
        .result
        .map_or(true, |result| result.get("leavesOut").is_none()));
    let refused = call(
        &fixture,
        MethodName::ExportsCheck,
        json!({"computerId": LINUX_ID, "includeLogins": "yes"}),
    )
    .await
    .unwrap_err();
    assert!(refused.message.contains("Invalid params"), "{}", refused.message);
}

#[tokio::test]
async fn exports_create_takes_the_choice_to_include_logins_as_a_boolean() {
    let fixture = fixture();
    let folder = fixture.directory.path().join("exports");
    std::fs::create_dir_all(&folder).unwrap();
    call(
        &fixture,
        MethodName::ExportsDestinationSet,
        json!({"path": folder}),
    )
    .await
    .unwrap();
    let refused = call(
        &fixture,
        MethodName::ExportsCreate,
        json!({"destination": folder, "computerIds": [fixture.macos_id], "includeLogins": "yes"}),
    )
    .await
    .unwrap_err();
    assert!(refused.message.contains("Invalid params"), "{}", refused.message);
    let started = call(
        &fixture,
        MethodName::ExportsCreate,
        json!({"destination": folder, "computerIds": [fixture.macos_id], "includeLogins": true}),
    )
    .await
    .unwrap();
    let id = job_id(started);
    assert!(id.as_str().starts_with("exports:"));
    let _ = finished(&fixture, &id).await;
}

#[tokio::test]
async fn exports_import_validates_its_inputs_before_it_starts_a_job() {
    let fixture = fixture();
    let relative = call(
        &fixture,
        MethodName::ExportsImport,
        json!({"archivePath": "relative.silo-backup", "newName": "copy"}),
    )
    .await
    .unwrap_err();
    assert_eq!(relative.message, "Choose the export file again.");
}

#[tokio::test]
async fn exports_dismiss_and_acknowledge_report_false_when_nothing_matches() {
    let fixture = fixture();
    let dismissed = call(
        &fixture,
        MethodName::ExportsDismiss,
        json!({
            "expectedOperation": {
                "kind": "result", "operation": "backup",
                "archive": {"name": "a", "archivePath": "/a", "completedLabel": "", "size": "", "destination": "/", "computers": []},
                "runningNames": [], "outcome": "success", "title": "t", "message": "m"
            },
            "expectedOperationId": "none"
        }),
    )
    .await
    .unwrap();
    assert_eq!(dismissed, json!(false));
    let acknowledged = call(
        &fixture,
        MethodName::ExportsAcknowledge,
        json!({"expectedOperationId": "none"}),
    )
    .await
    .unwrap();
    assert_eq!(acknowledged, json!(false));
}

// MARK: migration.*

#[tokio::test]
async fn migration_state_is_the_canonical_launch_upgrade_state() {
    let fixture = fixture();
    let state = call(&fixture, MethodName::MigrationState, json!({}))
        .await
        .unwrap();
    let state: silo_api::methods::migration::MigrationState =
        serde_json::from_value(state).unwrap();
    assert_eq!(state.status, "complete");
    assert_eq!(state.failed_count, 0);
}

#[tokio::test]
async fn the_pre_upgrade_backup_methods_are_null_without_a_backup_and_refuse_another_device() {
    let fixture = fixture();
    assert_eq!(
        call(
            &fixture,
            MethodName::MigrationPreUpgradeBackupGet,
            json!({})
        )
        .await
        .unwrap(),
        Value::Null
    );
    for method in [
        MethodName::MigrationPreUpgradeBackupMeasure,
        MethodName::MigrationPreUpgradeBackupAcknowledgeNotice,
    ] {
        assert_eq!(
            call(&fixture, method, json!({})).await.unwrap(),
            Value::Null,
            "{method:?}"
        );
    }
    // Deleting what is not a backup is refused with the engine's reason, never attempted.
    let refused = call(
        &fixture,
        MethodName::MigrationPreUpgradeBackupDelete,
        json!({}),
    )
    .await
    .unwrap_err();
    assert!(refused.message.contains("deletable backup"));
    assert_eq!(
        error_code(
            call(
                &fixture,
                MethodName::MigrationPreUpgradeBackupGet,
                json!({"deviceId": "other"})
            )
            .await
        ),
        ErrorCode::UnsupportedRemoteOperation
    );
}

#[tokio::test]
async fn the_pre_upgrade_backup_is_read_measured_acknowledged_and_deleted_with_its_topic() {
    let fixture = fixture_with(true);
    let mut events = fixture.silod.subscribe(ClientId::new("test-client"));
    let backup = call(
        &fixture,
        MethodName::MigrationPreUpgradeBackupGet,
        json!({}),
    )
    .await
    .unwrap();
    let backup: silo_api::methods::migration::PreUpgradeBackup =
        serde_json::from_value(backup).unwrap();
    assert_eq!(
        backup.path,
        fixture
            .directory
            .path()
            .join("data/runtime")
            .to_str()
            .unwrap()
    );
    assert!(backup.notice_pending);
    assert!(backup.delete_at.is_some());

    let bytes = call(
        &fixture,
        MethodName::MigrationPreUpgradeBackupMeasure,
        json!({}),
    )
    .await
    .unwrap();
    assert!(bytes.as_u64().is_some_and(|bytes| bytes > 0));

    let acknowledged = call(
        &fixture,
        MethodName::MigrationPreUpgradeBackupAcknowledgeNotice,
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(acknowledged, Value::Null);
    let event = next_event(&mut events, "migration.preUpgradeBackup.changed").await;
    assert_eq!(event.payload["noticePending"], false);

    call(
        &fixture,
        MethodName::MigrationPreUpgradeBackupDelete,
        json!({}),
    )
    .await
    .unwrap();
    // The legacy hint and the canonical topic both fire; the last payload is "no backup".
    let mut last = None;
    while let Ok(event) = events.try_recv() {
        if event.name == "migration.preUpgradeBackup.changed" {
            last = Some(event.payload);
        }
    }
    assert_eq!(last, Some(Value::Null));
    assert_eq!(
        call(
            &fixture,
            MethodName::MigrationPreUpgradeBackupGet,
            json!({})
        )
        .await
        .unwrap(),
        Value::Null
    );
}

// MARK: preparation.* and device.*

#[tokio::test]
async fn preparation_status_and_retry_return_the_canonical_status() {
    let fixture = fixture();
    for method in [MethodName::PreparationStatus, MethodName::PreparationRetry] {
        let status = call(&fixture, method, json!({})).await.unwrap();
        let status: silo_api::methods::preparation::PreparationStatus =
            serde_json::from_value(status).unwrap();
        assert!(status.image.fraction.is_none() || status.image.fraction <= Some(100));
    }
}

#[tokio::test]
async fn preparation_status_is_canonical_and_a_remote_device_is_refused() {
    let fixture = fixture();
    let status = call(&fixture, MethodName::PreparationStatus, json!({}))
        .await
        .unwrap();
    let _: silo_api::methods::preparation::PreparationStatus =
        serde_json::from_value(status).unwrap();
    for method in [MethodName::PreparationStatus, MethodName::PreparationRetry] {
        assert_eq!(
            error_code(call(&fixture, method, json!({"deviceId": "other"})).await),
            ErrorCode::UnsupportedRemoteOperation
        );
    }
}

#[tokio::test]
async fn device_facts_reads_capacity_limits_and_remote_login() {
    let fixture = fixture();
    let facts = call(&fixture, MethodName::DeviceFacts, json!({}))
        .await
        .unwrap();
    let facts: silo_api::methods::device::DeviceFacts = serde_json::from_value(facts).unwrap();
    assert!(facts
        .device_capacity
        .is_some_and(|capacity| capacity.logical_cpus > 0));
    assert_eq!(
        error_code(
            call(
                &fixture,
                MethodName::DeviceFacts,
                json!({"deviceId": "other"})
            )
            .await
        ),
        ErrorCode::UnsupportedRemoteOperation
    );
}

#[tokio::test]
async fn device_dependencies_validates_the_request_id_and_reports_each_check() {
    let fixture = fixture();
    let blank = call(
        &fixture,
        MethodName::DeviceDependencies,
        json!({"requestId": "  "}),
    )
    .await
    .unwrap_err();
    assert_eq!(blank.message, "Invalid dependency-check request ID");
    let report = call(
        &fixture,
        MethodName::DeviceDependencies,
        json!({"requestId": "check-1"}),
    )
    .await
    .unwrap();
    let report: silo_api::methods::device::DependencyReport =
        serde_json::from_value(report).unwrap();
    assert_eq!(report.request_id, "check-1");
    assert!(!report.checks.is_empty());
}

// MARK: recipes.*

#[tokio::test(flavor = "multi_thread")]
async fn a_recipe_build_reads_the_computers_without_an_event_client() {
    let fixture = fixture();
    let env = super::recipes::DispatchEnv {
        silod: fixture.silod.clone(),
        handle: tokio::runtime::Handle::current(),
    };
    let read = tokio::task::spawn_blocking(move || env.rows())
        .await
        .unwrap();
    // The test device has no runtime, so the read may fail; it must not fail for want of an
    // event client session.
    if let Err(message) = read {
        assert!(!message.contains("event client"), "{message}");
    }
}

// MARK: exports.transfer, exports.discard and the transfer area

mod transfer {
    use super::super::export_transfer::{install_test_link, FileLink, Opened, TestLink, TOO_OLD};
    use super::*;
    use silo_engine::transfer_area::{self, Area};
    use std::{
        io::{BufReader, Read, Write},
        os::unix::net::UnixStream,
        sync::Mutex,
    };

    const DEVICE: &str = "mini-0000";

    /// Shuts a socket down when the controller drops its half, as a killed ssh child does.
    struct Closing(UnixStream);
    impl Drop for Closing {
        fn drop(&mut self) {
            let _ = self.0.shutdown(std::net::Shutdown::Both);
        }
    }

    /// The owner: its own transfer area, served through the real owner-side stream code.
    struct Owner {
        area: Area,
        capabilities: Vec<String>,
        /// Hold the owner's reply after the header, so a copy can be stopped mid-way.
        stall: bool,
        _directory: tempfile::TempDir,
        opened: Mutex<Vec<serde_json::Value>>,
    }

    impl Owner {
        fn new(capabilities: &[&str]) -> Arc<Self> {
            let directory = tempfile::tempdir().unwrap();
            Arc::new(Self {
                area: Area::at(directory.path()).unwrap(),
                capabilities: capabilities.iter().map(|c| (*c).to_owned()).collect(),
                stall: false,
                _directory: directory,
                opened: Mutex::new(Vec::new()),
            })
        }

        fn stalled(capabilities: &[&str]) -> Arc<Self> {
            let directory = tempfile::tempdir().unwrap();
            Arc::new(Self {
                area: Area::at(directory.path()).unwrap(),
                capabilities: capabilities.iter().map(|c| (*c).to_owned()).collect(),
                stall: true,
                _directory: directory,
                opened: Mutex::new(Vec::new()),
            })
        }
    }

    impl FileLink for Owner {
        fn capabilities(&self, _device: &str) -> Result<Vec<String>, String> {
            Ok(self.capabilities.clone())
        }

        fn open(
            &self,
            _device: &str,
            params: serde_json::Value,
            _cancelled: &dyn Fn() -> bool,
        ) -> Result<Opened, String> {
            self.opened.lock().unwrap().push(params.clone());
            let prepared = transfer_area::prepare(&self.area, &params)?;
            let (near, far) = UnixStream::pair().unwrap();
            let stall = self.stall;
            let mut far = far;
            std::thread::spawn(move || {
                if stall {
                    // The header, then nothing, until the controller leaves.
                    let _ = far.write_all(b"{\"name\":\"x\",\"bytes\":100000000}\n");
                    let mut sink = [0u8; 1];
                    let _ = far.read(&mut sink);
                    return;
                }
                let _ = prepared.run(&mut far, &mut |_| {}, &|| false);
            });
            Ok(Opened {
                input: Box::new(near.try_clone().unwrap()),
                output: Box::new(BufReader::new(near.try_clone().unwrap())),
                guard: Box::new(Closing(near)),
            })
        }
    }

    /// Installs `owner` as the link for the test and removes it afterwards.
    struct Installed(#[allow(dead_code)] TestLink);
    impl Installed {
        fn new(owner: Arc<Owner>) -> Self {
            Self(install_test_link(owner))
        }
    }

    fn sha(data: &[u8]) -> String {
        // Through a file, as the engine hashes: the digest helper is the engine's own.
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), data).unwrap();
        transfer_area::digest(file.path(), &|| false).unwrap().1
    }

    fn receive_params(path: &std::path::Path, name: &str, data: &[u8], sha256: &str) -> Value {
        json!({
            "deviceId": DEVICE, "direction": "receive", "path": path.to_str().unwrap(),
            "name": name, "bytes": data.len(), "sha256": sha256, "computer": "main"
        })
    }

    #[tokio::test]
    async fn an_export_file_is_copied_here_whole_and_the_job_reports_what_the_owner_said() {
        let fixture = fixture();
        let owner = Owner::new(&["exports.file"]);
        let data: Vec<u8> = (0..400_000u32).map(|n| (n % 253) as u8).collect();
        let (name, staged) = owner.area.export_target();
        std::fs::write(&staged, &data).unwrap();
        let _installed = Installed::new(owner.clone());
        let destination = fixture.directory.path().join("copy.silo-backup");
        let started = call(
            &fixture,
            MethodName::ExportsTransfer,
            receive_params(&destination, &name, &data, &sha(&data)),
        )
        .await
        .unwrap();
        let job = job_id(started);
        assert!(job.as_str().starts_with("transfer:"));
        let outcome = finished(&fixture, &job).await.outcome.unwrap();
        assert_eq!(
            outcome.result.unwrap(),
            json!({"name": name, "bytes": data.len(), "sha256": sha(&data)})
        );
        assert_eq!(std::fs::read(&destination).unwrap(), data);
        assert!(parts_of(&destination).is_empty());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&destination)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        // The owner's file stays until the client discards it, and cannot be read twice.
        assert!(staged.exists());
        assert!(owner.area.open_for_read(&name).is_err());
    }

    #[tokio::test]
    async fn a_copy_whose_hash_differs_fails_and_leaves_no_file_here() {
        let fixture = fixture();
        let owner = Owner::new(&["exports.file"]);
        let data = b"archive bytes".to_vec();
        let (name, staged) = owner.area.export_target();
        std::fs::write(&staged, &data).unwrap();
        let _installed = Installed::new(owner);
        let destination = fixture.directory.path().join("copy.silo-backup");
        let started = call(
            &fixture,
            MethodName::ExportsTransfer,
            receive_params(&destination, &name, &data, &sha(b"something else")),
        )
        .await
        .unwrap();
        let outcome = finished(&fixture, &job_id(started)).await.outcome.unwrap();
        assert!(outcome
            .error
            .unwrap()
            .message
            .contains("does not match what the other machine exported"));
        assert!(!destination.exists());
        assert!(parts_of(&destination).is_empty());
    }

    #[tokio::test]
    async fn a_copy_never_replaces_a_file_that_is_already_at_the_path() {
        let fixture = fixture();
        let owner = Owner::new(&["exports.file"]);
        let (name, staged) = owner.area.export_target();
        std::fs::write(&staged, b"new").unwrap();
        let _installed = Installed::new(owner);
        let destination = fixture.directory.path().join("copy.silo-backup");
        std::fs::write(&destination, b"mine").unwrap();
        let started = call(
            &fixture,
            MethodName::ExportsTransfer,
            receive_params(&destination, &name, b"new", &sha(b"new")),
        )
        .await;
        // Refused when the request is made or when the job runs; either way the file is mine.
        if let Ok(started) = started {
            let outcome = finished(&fixture, &job_id(started)).await.outcome.unwrap();
            assert!(outcome.error.is_some());
        }
        assert_eq!(std::fs::read(&destination).unwrap(), b"mine");
        assert!(parts_of(&destination).is_empty());
    }

    #[tokio::test]
    async fn a_file_is_sent_and_the_owner_names_it() {
        let fixture = fixture();
        let owner = Owner::new(&["exports.file"]);
        let _installed = Installed::new(owner.clone());
        let data: Vec<u8> = (0..300_000u32).map(|n| (n % 239) as u8).collect();
        let source = fixture.directory.path().join("friend.silo-backup");
        std::fs::write(&source, &data).unwrap();
        let started = call(
            &fixture,
            MethodName::ExportsTransfer,
            json!({"deviceId": DEVICE, "direction": "send", "path": source.to_str().unwrap()}),
        )
        .await
        .unwrap();
        let result = finished(&fixture, &job_id(started))
            .await
            .outcome
            .unwrap()
            .result
            .unwrap();
        let name = result["name"].as_str().unwrap();
        assert_eq!(
            transfer_area::parse_name(name),
            Some(transfer_area::Role::Import)
        );
        assert_eq!(
            std::fs::read(owner.area.import_path(name).unwrap()).unwrap(),
            data
        );
        assert_eq!(result["sha256"], sha(&data));
        // The declared size and hash were read from the file here, never taken from a caller.
        let opened = owner.opened.lock().unwrap();
        assert_eq!(opened[0]["bytes"], data.len());
        assert_eq!(opened[0]["sha256"], sha(&data));
    }

    #[tokio::test]
    async fn an_owner_that_refuses_the_file_is_the_reason_and_nothing_is_kept() {
        let fixture = fixture();
        let owner = Owner::new(&["exports.file"]);
        let _installed = Installed::new(owner.clone());
        let source = fixture.directory.path().join("big.silo-backup");
        std::fs::write(&source, b"x").unwrap();
        // A name the owner never handed out.
        let destination = fixture.directory.path().join("copy.silo-backup");
        let name = format!("x-{}.silo-backup", "a".repeat(32));
        let started = call(
            &fixture,
            MethodName::ExportsTransfer,
            receive_params(&destination, &name, b"x", &sha(b"x")),
        )
        .await
        .unwrap();
        let outcome = finished(&fixture, &job_id(started)).await.outcome.unwrap();
        assert!(outcome.error.unwrap().message.contains("no longer here"));
        assert!(!destination.exists());
    }

    #[tokio::test]
    async fn an_owner_without_the_stream_is_too_old_and_nothing_starts() {
        let fixture = fixture();
        let owner = Owner::new(&["handshake", "guest.ssh"]);
        let _installed = Installed::new(owner);
        let source = fixture.directory.path().join("x.silo-backup");
        std::fs::write(&source, b"x").unwrap();
        let refused = call(
            &fixture,
            MethodName::ExportsTransfer,
            json!({"deviceId": DEVICE, "direction": "send", "path": source.to_str().unwrap()}),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::UnsupportedRemoteOperation);
        assert_eq!(refused.message, TOO_OLD);
        assert!(super::super::export_transfer::require_file_sharing(DEVICE).is_err());
    }

    #[tokio::test]
    async fn the_request_is_checked_before_a_job_exists() {
        let fixture = fixture();
        let owner = Owner::new(&["exports.file"]);
        let _installed = Installed::new(owner);
        let existing = fixture.directory.path().join("here.silo-backup");
        std::fs::write(&existing, b"x").unwrap();
        let name = format!("x-{}.silo-backup", "b".repeat(32));
        let good = sha(b"x");
        for (label, params) in [
            (
                "this device",
                json!({"deviceId": "test-device", "direction": "send", "path": existing.to_str().unwrap()}),
            ),
            (
                "relative path",
                json!({"deviceId": DEVICE, "direction": "send", "path": "relative.silo-backup"}),
            ),
            (
                "send with a name",
                json!({"deviceId": DEVICE, "direction": "send", "path": existing.to_str().unwrap(), "name": name}),
            ),
            (
                "not a file",
                json!({"deviceId": DEVICE, "direction": "send", "path": fixture.directory.path().to_str().unwrap()}),
            ),
            (
                "receive onto an existing file",
                receive_params(&existing, &name, b"x", &good),
            ),
            (
                "receive with an import name",
                receive_params(
                    &fixture.directory.path().join("a.silo-backup"),
                    &format!("i-{}.silo-backup", "c".repeat(32)),
                    b"x",
                    &good,
                ),
            ),
            (
                "receive with a path outside the name",
                receive_params(&fixture.directory.path().join("a.tar"), &name, b"x", &good),
            ),
            (
                "receive over the cap",
                json!({"deviceId": DEVICE, "direction": "receive",
                "path": fixture.directory.path().join("b.silo-backup").to_str().unwrap(),
                "name": name, "bytes": transfer_area::MAX_BYTES + 1, "sha256": good}),
            ),
            (
                "receive with a bad hash",
                receive_params(
                    &fixture.directory.path().join("c.silo-backup"),
                    &name,
                    b"x",
                    "nope",
                ),
            ),
        ] {
            let result = call(&fixture, MethodName::ExportsTransfer, params).await;
            assert!(result.is_err(), "{label} was accepted");
        }
    }

    #[tokio::test]
    async fn cancelling_stops_a_copy_that_stalled_and_removes_the_partial_file() {
        let fixture = fixture();
        let owner = Owner::stalled(&["exports.file"]);
        let (name, staged) = owner.area.export_target();
        std::fs::write(&staged, b"x").unwrap();
        let _installed = Installed::new(owner);
        let destination = fixture.directory.path().join("copy.silo-backup");
        let data = vec![1u8; 100_000_000];
        let started = call(
            &fixture,
            MethodName::ExportsTransfer,
            json!({"deviceId": DEVICE, "direction": "receive", "path": destination.to_str().unwrap(),
                   "name": name, "bytes": data.len(), "sha256": sha(&[1u8])}),
        )
        .await
        .unwrap();
        let job = job_id(started);
        tokio::time::sleep(Duration::from_millis(300)).await;
        // While it copies, the path the user chose does not exist: only the copy's own
        // temporary file does.
        assert!(!destination.exists());
        assert_eq!(parts_of(&destination).len(), 1);
        silo_engine::context::EngineContext::job_registry(fixture.silod.as_ref())
            .unwrap()
            .cancel(&job)
            .unwrap();
        let outcome = finished(&fixture, &job).await.outcome.unwrap();
        let error = outcome.error.unwrap();
        assert_eq!(error.code, ErrorCode::Cancelled, "{}", error.message);
        assert!(error.message.contains("cancelled"));
        assert!(!destination.exists() && parts_of(&destination).is_empty());
    }

    /// The temporary files of the copies to `destination`: `<destination>.<random>.part`.
    fn parts_of(destination: &std::path::Path) -> Vec<std::path::PathBuf> {
        let prefix = format!("{}.", destination.file_name().unwrap().to_str().unwrap());
        std::fs::read_dir(destination.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".part"))
            })
            .collect()
    }

    fn area_of(fixture: &Fixture) -> Area {
        Area::of_engine(fixture.silod.as_ref()).unwrap()
    }

    #[tokio::test]
    async fn the_sweep_empties_the_area_at_start_and_later_only_removes_old_entries() {
        let fixture = fixture();
        let area = area_of(&fixture);
        let old = area.dir().join(format!("i-{}.silo-backup", "1".repeat(32)));
        let fresh = area.dir().join(format!("x-{}.silo-backup", "2".repeat(32)));
        std::fs::write(&old, b"x").unwrap();
        std::fs::write(&fresh, b"x").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(25 * 3600))
            .unwrap();
        assert_eq!(crate::timers::sweep_transfer_area(&fixture.silod, false), 1);
        assert!(!old.exists() && fresh.exists());
        assert_eq!(crate::timers::sweep_transfer_area(&fixture.silod, true), 1);
        assert!(!fresh.exists());
    }

    #[tokio::test]
    async fn exports_discard_removes_one_file_of_the_area_and_only_those() {
        let fixture = fixture();
        let area = area_of(&fixture);
        let (name, path) = area.export_target();
        std::fs::write(&path, b"x").unwrap();
        let removed = call(&fixture, MethodName::ExportsDiscard, json!({"name": name}))
            .await
            .unwrap();
        assert_eq!(removed, json!(true));
        assert!(!path.exists());
        let again = call(&fixture, MethodName::ExportsDiscard, json!({"name": name}))
            .await
            .unwrap();
        assert_eq!(again, json!(false));
        for bad in ["../../etc/passwd", "notes.silo-backup", ""] {
            assert!(
                call(&fixture, MethodName::ExportsDiscard, json!({"name": bad}))
                    .await
                    .is_err(),
                "{bad}"
            );
        }
        let other = call(
            &fixture,
            MethodName::ExportsDiscard,
            json!({"name": name, "deviceId": "some-other-device"}),
        )
        .await;
        assert_eq!(error_code(other), ErrorCode::UnsupportedRemoteOperation);
    }

    #[tokio::test]
    async fn an_export_to_the_transfer_area_is_named_by_the_machine_not_the_caller() {
        let fixture = fixture();
        let result = call(
            &fixture,
            MethodName::ExportsCreate,
            json!({
                "destination": "/somewhere/else", "computerIds": [fixture.macos_id],
                "archivePath": "/somewhere/else/mine.silo-backup", "transferArea": true
            }),
        )
        .await
        .unwrap();
        let outcome = finished(&fixture, &job_id(result)).await.outcome.unwrap();
        let archive = outcome.result.unwrap()["archive"].clone();
        let path = std::path::PathBuf::from(archive["archivePath"].as_str().unwrap());
        let area = area_of(&fixture);
        assert_eq!(path.parent().unwrap(), area.dir());
        assert_eq!(
            transfer_area::parse_name(path.file_name().unwrap().to_str().unwrap()),
            Some(transfer_area::Role::Export)
        );
    }

    #[tokio::test]
    async fn an_export_cannot_be_aimed_into_the_transfer_area_by_a_caller() {
        let fixture = fixture();
        let area = area_of(&fixture);
        let inside = area.dir().join(format!("x-{}.silo-backup", "c".repeat(32)));
        for params in [
            json!({
                "destination": "/somewhere/else", "computerIds": [fixture.macos_id],
                "archivePath": inside.to_str().unwrap()
            }),
            json!({
                "destination": area.dir().to_str().unwrap(), "computerIds": [fixture.macos_id]
            }),
            json!({
                "destination": area.dir().to_str().unwrap(), "computerIds": [fixture.macos_id],
                "archivePath": area.dir().join("mine.silo-backup").to_str().unwrap()
            }),
        ] {
            let error = call(&fixture, MethodName::ExportsCreate, params)
                .await
                .unwrap_err();
            assert!(error.message.contains("transfer"), "{}", error.message);
        }
        assert_eq!(std::fs::read_dir(area.dir()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn an_import_reads_only_a_file_this_machine_received_by_its_name() {
        let fixture = fixture();
        let area = area_of(&fixture);
        let (export, staged) = area.export_target();
        std::fs::write(staged, b"x").unwrap();
        let missing = format!("i-{}.silo-backup", "e".repeat(32));
        for (label, params) in [
            (
                "an export name",
                json!({"newName": "copy", "transferName": export}),
            ),
            (
                "a name with a path",
                json!({"newName": "copy", "transferName": "../x.silo-backup"}),
            ),
            (
                "a file that is not there",
                json!({"newName": "copy", "transferName": missing}),
            ),
            (
                "both a path and a name",
                json!({"newName": "copy", "archivePath": "/a.silo-backup", "transferName": missing}),
            ),
        ] {
            let result = call(&fixture, MethodName::ExportsImport, params).await;
            assert!(result.is_err(), "{label} was accepted");
        }
        // A received file is imported like any file of unknown origin: the archive checks decide.
        let writer = area.begin_write(5, &sha(b"hello")).unwrap();
        let received = writer
            .receive(
                &mut std::io::Cursor::new(b"hello".to_vec()),
                &mut |_| {},
                &|| false,
            )
            .unwrap()
            .name;
        let started = call(
            &fixture,
            MethodName::ExportsImport,
            json!({"newName": "copy", "transferName": received}),
        )
        .await
        .unwrap();
        let outcome = finished(&fixture, &job_id(started)).await.outcome.unwrap();
        assert_eq!(outcome.result.unwrap()["outcome"], "failed");
    }
}
