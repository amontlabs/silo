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
            protocol: 6,
            client: None,
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
async fn checkpoints_fork_needs_a_checkpoint_on_macos_and_starts_a_job_on_linux() {
    let fixture = fixture();
    let refused = call(
        &fixture,
        MethodName::CheckpointsFork,
        json!({"computerId": fixture.macos_id, "checkpointId": null, "newName": "branch"}),
    )
    .await
    .unwrap_err();
    assert!(refused.message.contains("Choose the checkpoint"));

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
async fn chatgpt_status_is_canonical_and_a_remote_device_is_refused() {
    let fixture = fixture();
    let status = call(&fixture, MethodName::PreparationChatgptStatus, json!({}))
        .await
        .unwrap();
    let _: silo_api::methods::preparation::ChatGptPreparationStatus =
        serde_json::from_value(status).unwrap();
    for method in [
        MethodName::PreparationChatgptStatus,
        MethodName::PreparationChatgptRetry,
    ] {
        assert_eq!(
            error_code(call(&fixture, method, json!({"deviceId": "other"})).await),
            ErrorCode::UnsupportedRemoteOperation
        );
    }
}

#[tokio::test]
async fn the_chatgpt_publisher_sends_the_status_to_every_client_on_its_topic() {
    let fixture = fixture();
    let mut events = fixture.silod.subscribe(ClientId::new("test-client"));
    let publisher = super::preparation::status_publisher(super::engine_context(&fixture.context));
    publisher(silo_engine::chatgpt_app::Status::Verifying);
    let event = events.recv().await.unwrap();
    assert_eq!(event.name, "preparation.chatgpt.changed");
    assert_eq!(event.payload, json!({"state": "verifying"}));
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
