use std::collections::{BTreeSet, HashSet};

use serde_json::{json, Value};
use silo_api::{
    jobs::ErrorCode,
    methods::{
        audit::{AuditCallerKind, AuditEntry, AuditListParams, AuditOutcome},
        MethodName,
    },
    transport::{ApprovalKind, PROTOCOL_VERSION},
};
use silo_engine::{
    audit::{AuditLog, Scope},
    context::EngineContext,
    policy::{store::PolicyStore, Caller},
    resources::{Host, GIB},
};

use crate::{
    dispatcher::{DispatchError, RequestContext},
    methods::{access::tests::fixture, dispatch, HandlerContext},
};

fn context(caller: Caller) -> HandlerContext {
    HandlerContext::new(
        fixture().context.clone(),
        RequestContext {
            client_id: "gate-test".into(),
            protocol: PROTOCOL_VERSION,
            client: None,
            caller,
        },
    )
}

/// A computer of its own for each test (the fixture's data folder is shared), which made
/// `fork`.
fn computer() -> (String, String) {
    let id = format!("gate-{}", uuid::Uuid::new_v4());
    let fork = format!("{id}-fork");
    PolicyStore::new(&fixture().context.paths().app_data)
        .import_legacy(&id, std::slice::from_ref(&fork), &[])
        .unwrap();
    (id, fork)
}

async fn call(caller: &Caller, method: MethodName, params: Value) -> Result<Value, DispatchError> {
    dispatch(&context(caller.clone()), method, &params)
        .await
        .map(|output| output.value)
}

fn rows_about(ids: &[&str]) -> Vec<AuditEntry> {
    let visible: BTreeSet<String> = ids.iter().map(|id| (*id).to_owned()).collect();
    AuditLog::shared(&fixture().context.paths().app_data)
        .list(
            &AuditListParams {
                computer_id: None,
                caller_kind: None,
                before_id: None,
                limit: Some(500),
            },
            Scope::Computers(&visible),
        )
        .unwrap()
        .entries
}

#[tokio::test]
async fn a_computers_call_outside_its_rules_is_refused_before_any_handler_and_recorded() {
    let (id, _fork) = computer();
    let caller = Caller::Computer { id: id.clone() };
    let error = call(&caller, MethodName::SettingsRead, json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert!(
        error.message.contains("person who runs Silo"),
        "{}",
        error.message
    );
    assert!(
        !error.message.contains("settings.read"),
        "{}",
        error.message
    );
    // A computer it did not make, such as one the owner forked by hand.
    let error = call(
        &caller,
        MethodName::GuestExec,
        json!({"computerId": "someone-else", "argv": ["true"], "timeoutMs": 1000}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    let error = call(
        &caller,
        MethodName::OperationsJobGet,
        json!({"jobId": "job:424242"}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    let rows = rows_about(&[&id]);
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(rows.iter().all(|row| row.outcome == AuditOutcome::Refused
        && row.caller_kind == AuditCallerKind::Computer
        && row.caller_id.as_deref() == Some(id.as_str())
        && row.error_code.as_deref() == Some("permission_denied")));
    assert_eq!(
        rows[1].computer_id, None,
        "a computer outside its tree is not named in the log"
    );
}

#[tokio::test]
async fn a_computers_row_names_only_computers_it_may_reach_and_repeats_are_counted() {
    let (id, fork) = computer();
    let caller = Caller::Computer { id: id.clone() };
    for _ in 0..3 {
        let _ = call(
            &caller,
            MethodName::GuestExec,
            json!({"computerId": "guess-a-victim", "argv": ["true"], "timeoutMs": 1000}),
        )
        .await;
    }
    let _ = call(
        &caller,
        MethodName::GuestExec,
        json!({"computerId": fork, "argv": ["true"], "timeoutMs": 1000}),
    )
    .await;
    let rows = AuditLog::shared(&fixture().context.paths().app_data)
        .list(
            &AuditListParams {
                computer_id: None,
                caller_kind: Some(AuditCallerKind::Computer),
                before_id: None,
                limit: Some(500),
            },
            Scope::All,
        )
        .unwrap()
        .entries;
    let mine: Vec<&AuditEntry> = rows
        .iter()
        .filter(|row| row.caller_id.as_deref() == Some(id.as_str()))
        .collect();
    assert_eq!(mine.len(), 2, "{mine:?}");
    assert_eq!(mine[1].computer_id, None);
    assert_eq!(
        mine[1].count, 3,
        "the same refused probe is one counted row"
    );
    assert_eq!(mine[0].computer_id.as_deref(), Some(fork.as_str()));
}

#[tokio::test]
async fn a_widening_call_asks_with_an_engine_written_summary() {
    let (id, fork) = computer();
    let caller = Caller::Computer { id: id.clone() };
    let error = call(
        &caller,
        MethodName::SecretsSave,
        json!({"request": {"operation": "add", "name": "deploy", "value": "very-secret",
            "computerIds": [fork], "allowedDomains": ["deploy.example.com"]}}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::ApprovalRequired);
    let details = error.details.unwrap();
    assert_eq!(details.kind, ApprovalKind::Secret);
    assert!(details.summary.contains("deploy.example.com"));
    assert!(!details.summary.contains("very-secret"));
    let approval = details.approval_id.unwrap();
    let rows = rows_about(&[&id]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, AuditOutcome::Asked);
    assert_eq!(rows[0].error_code.as_deref(), Some("approval_required"));
    assert_eq!(rows[0].approval_id.as_deref(), Some(approval.as_str()));
}

#[tokio::test]
async fn a_computer_reads_the_audit_rows_about_itself_and_what_it_made_only() {
    let (id, fork) = computer();
    let (other, _) = computer();
    for caller in [&id, &other] {
        let _ = call(
            &Caller::Computer { id: caller.clone() },
            MethodName::SettingsRead,
            json!({}),
        )
        .await;
    }
    let _ = call(
        &Caller::owner("app"),
        MethodName::OperationsCancel,
        json!({"jobId": "job:424243", "computerId": fork}),
    )
    .await;
    let page = call(
        &Caller::Computer { id: id.clone() },
        MethodName::AuditList,
        json!({"limit": 500}),
    )
    .await
    .unwrap();
    let entries: Vec<AuditEntry> = serde_json::from_value(page["entries"].clone()).unwrap();
    assert!(entries.iter().all(|row| row.client.is_none()));
    assert!(entries
        .iter()
        .any(|row| row.caller_id.as_deref() == Some(id.as_str())));
    assert!(entries
        .iter()
        .any(|row| row.computer_id.as_deref() == Some(fork.as_str())));
    for row in &entries {
        let about_tree = [Some(id.as_str()), Some(fork.as_str())];
        assert!(
            about_tree.contains(&row.computer_id.as_deref())
                || about_tree.contains(&row.caller_id.as_deref()),
            "{row:?}"
        );
    }
}

#[tokio::test]
async fn an_owners_command_is_recorded_and_a_query_is_not() {
    let marker = format!(
        "job:{}",
        900_000 + (uuid::Uuid::new_v4().as_u128() % 99_999) as u64
    );
    let target = format!("owner-target-{}", uuid::Uuid::new_v4());
    let failed = call(
        &Caller::owner("app"),
        MethodName::OperationsCancel,
        json!({"jobId": marker, "computerId": target}),
    )
    .await;
    assert!(failed.is_err());
    let _ = call(
        &Caller::owner("app"),
        MethodName::AuditList,
        json!({"computerId": target}),
    )
    .await
    .unwrap();
    let rows = AuditLog::shared(&fixture().context.paths().app_data)
        .list(
            &AuditListParams {
                computer_id: Some(target.clone()),
                caller_kind: None,
                before_id: None,
                limit: Some(500),
            },
            Scope::All,
        )
        .unwrap()
        .entries;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].method, "operations.cancel");
    assert_eq!(rows[0].caller_kind, AuditCallerKind::Owner);
    assert_eq!(rows[0].client.as_deref(), Some("app"));
    assert_eq!(rows[0].outcome, AuditOutcome::Error);
    assert!(rows[0].error_code.is_some());
}

#[tokio::test]
async fn another_device_reaches_only_the_connections_methods() {
    let device = Caller::Device {
        key: Some("SHA256:gate-test".into()),
    };
    let error = call(&device, MethodName::AuditList, json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    let error = call(&device, MethodName::SettingsUpdate, json!({"patch": {}}))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
}

/// A device with `memory` bytes available and `disk` bytes free, whose default floors are
/// 4 GiB of memory and 25 GiB of disk.
fn device(memory: u64, disk: u64) -> super::FixedDevice {
    super::fixed_device(Host {
        logical_cpus: 8,
        memory_total: 32 * GIB,
        memory_available: Some(memory),
        pressure: None,
        disk_free: disk,
        disk_total: 500 * GIB,
    })
}

/// Each call an agent can make that starts, creates or checkpoints a computer, as the
/// computer `id` that made `fork`.
fn headroom_calls(id: &str, fork: &str) -> Vec<(MethodName, Value)> {
    vec![
        (MethodName::ComputersStart, json!({"computerId": fork})),
        (MethodName::ComputersRestart, json!({"computerId": fork})),
        (MethodName::MacosSetupRetry, json!({"computerId": fork})),
        (
            MethodName::ComputersCreate,
            json!({"kind": "linux", "configuration": {"id": "x", "name": "gate-created",
                "cpus": 2, "maxCPUs": 2, "memoryGiB": 1, "maxMemoryGiB": 1,
                "workspaceStorageGiB": 10, "runtimeStorageGiB": 10}}),
        ),
        (
            MethodName::CheckpointsFork,
            json!({"computerId": id, "checkpointId": null, "newName": "gate-fork"}),
        ),
        (
            MethodName::CheckpointsCreate,
            json!({"computerId": id, "name": "gate-checkpoint"}),
        ),
        (
            MethodName::CheckpointsRestore,
            json!({"computerId": fork, "checkpointId": "none"}),
        ),
    ]
}

#[tokio::test]
async fn an_agent_cannot_start_create_or_checkpoint_while_the_device_is_below_a_floor() {
    for (memory, disk, fact) in [
        (
            3 * GIB,
            400 * GIB,
            "3.0 GiB of memory available; Silo keeps 4.0 GiB free",
        ),
        (
            24 * GIB,
            20 * GIB,
            "20.0 GiB of disk space free; Silo keeps 25.0 GiB free",
        ),
    ] {
        let _device = device(memory, disk);
        let (id, fork) = computer();
        let caller = Caller::Computer { id: id.clone() };
        let calls = headroom_calls(&id, &fork);
        for (method, params) in &calls {
            let error = call(&caller, *method, params.clone()).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", method.name());
            assert!(error.message.contains(fact), "{}", error.message);
        }
        let rows = rows_about(&[&id, &fork]);
        assert_eq!(rows.len(), calls.len(), "{rows:?}");
        assert!(rows.iter().all(|row| row.outcome == AuditOutcome::Refused
            && row.error_code.as_deref() == Some("resources_low")));
    }
}

#[tokio::test]
async fn the_owner_is_never_held_back_by_the_floors() {
    let _device = device(GIB, GIB);
    let owner = Caller::owner("app");
    // Computers that do not exist, so nothing is started or made.
    for (method, params) in [
        (
            MethodName::ComputersStart,
            json!({"computerId": "no-such-computer"}),
        ),
        (
            MethodName::ComputersRestart,
            json!({"computerId": "no-such-computer"}),
        ),
        (
            MethodName::CheckpointsCreate,
            json!({"computerId": "no-such-computer", "name": "c"}),
        ),
        (
            MethodName::CheckpointsRestore,
            json!({"computerId": "no-such-computer", "checkpointId": "none"}),
        ),
    ] {
        if let Err(error) = call(&owner, method, params).await {
            assert_ne!(error.code, ErrorCode::ResourcesLow, "{}", method.name());
        }
    }
}

#[tokio::test]
async fn an_agent_may_go_on_while_the_device_is_low_and_can_still_read_its_resources_when_short() {
    // Between the floor and twice the floor.
    let _device = device(5 * GIB, 30 * GIB);
    let (id, fork) = computer();
    sized(&id, 1);
    sized(&fork, 1);
    let caller = Caller::Computer { id: id.clone() };
    for (method, params) in headroom_calls(&id, &fork) {
        if let Err(error) = call(&caller, method, params).await {
            assert_ne!(error.code, ErrorCode::ResourcesLow, "{}", method.name());
        }
    }
    drop(_device);
    let _short = device(GIB, GIB);
    let read = call(&caller, MethodName::DeviceResources, json!({}))
        .await
        .unwrap();
    // A query that takes nothing is answered even when the device is short.
    assert_eq!(read["state"], "critical");
}

#[tokio::test]
async fn device_resources_tells_every_caller_the_numbers_and_the_state() {
    let _device = device(6 * GIB, 400 * GIB);
    let (id, _fork) = computer();
    for caller in [Caller::owner("app"), Caller::Computer { id }] {
        let read = call(&caller, MethodName::DeviceResources, json!({}))
            .await
            .unwrap();
        assert_eq!(read["logicalCpus"], 8);
        assert_eq!(read["memoryTotalBytes"], 32 * GIB);
        assert_eq!(read["memoryAvailableBytes"], 6 * GIB);
        assert_eq!(read["diskFreeBytes"], 400 * GIB);
        assert_eq!(read["diskTotalBytes"], 500 * GIB);
        assert_eq!(read["memoryFloorBytes"], 4 * GIB);
        assert_eq!(read["diskFloorBytes"], 25 * GIB);
        assert_eq!(read["state"], "low");
        assert_eq!(
            read["message"],
            "About 6.0 GiB of memory is free. Starting another computer may slow this device."
        );
        assert!(read["computers"].is_array());
    }
}

#[tokio::test]
async fn a_call_that_would_ask_for_approval_is_refused_first_when_the_device_is_short() {
    let (id, _fork) = computer();
    let caller = Caller::Computer { id };
    let container = json!({"kind": "linux", "configuration": {"id": "x", "name": "gate-container",
        "cpus": 2, "maxCPUs": 2, "memoryGiB": 4, "maxMemoryGiB": 4,
        "workspaceStorageGiB": 10, "runtimeStorageGiB": 10, "runtime": "container"}});
    let error = {
        let _ample = device(32 * GIB, 400 * GIB);
        call(&caller, MethodName::ComputersCreate, container.clone())
            .await
            .unwrap_err()
    };
    assert_eq!(error.code, ErrorCode::ApprovalRequired);
    let _short = device(GIB, 400 * GIB);
    let error = call(&caller, MethodName::ComputersCreate, container)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourcesLow);
}

#[test]
fn a_pending_computer_with_neither_an_id_nor_a_source_is_never_matched_by_its_name() {
    use silo_engine::policy::store::Pending;
    let names = std::collections::HashMap::from([("existing".to_owned(), "Foo".to_owned())]);
    let parents = std::collections::HashMap::from([("existing".to_owned(), None)]);
    let pending = |id: Option<&str>, source: Option<&str>| Pending {
        created_by: "asker".into(),
        name: "foo".into(),
        id: id.map(str::to_owned),
        source: source.map(str::to_owned),
        job_id: Some("job:1".into()),
        at: 0,
    };
    assert_eq!(super::resolve(&names, &parents, &pending(None, None)), None);
    assert_eq!(
        super::resolve(&names, &parents, &pending(Some("existing"), None)),
        Some("existing".to_owned())
    );
    assert_eq!(
        super::resolve(&names, &parents, &pending(Some("other"), None)),
        None
    );
    // A fork still matches by name and parent.
    let parents =
        std::collections::HashMap::from([("existing".to_owned(), Some("src".to_owned()))]);
    assert_eq!(
        super::resolve(&names, &parents, &pending(None, Some("src"))),
        Some("existing".to_owned())
    );
    assert_eq!(
        super::resolve(&names, &parents, &pending(None, Some("elsewhere"))),
        None
    );
}

#[test]
fn a_macos_create_records_the_id_it_returned_and_a_same_named_computer_is_not_the_askers() {
    use crate::dispatcher::HandlerOutput;
    use silo_engine::policy::Creation;
    let (id, _fork) = computer();
    let store = PolicyStore::new(&fixture().context.paths().app_data);
    let creation = Creation {
        name: "same-name".into(),
        id: None,
        source: None,
    };
    super::begin_creation(&fixture().context.paths().app_data, &id, &creation).unwrap();
    let returned = HandlerOutput {
        value: json!({"jobId": "job:5", "computer": {"id": "mac-new", "name": "same-name"}}),
        event_barrier: None,
    };
    super::settle_creation(&store, &id, &creation, &Ok(returned), Some("job:5")).unwrap();
    let document = store.snapshot().unwrap();
    assert!(document.made_by(&id).contains("mac-new"));
    assert!(document
        .pending
        .iter()
        .all(|pending| pending.created_by != id));
    assert!(!document.made_by(&id).contains("existing-same-name"));
}

fn sized(id: &str, memory_gib: u64) {
    super::test_resources::add(
        id,
        silo_engine::policy::Resources {
            linux: true,
            cpus: 2,
            max_cpus: 2,
            memory_gib,
            max_memory_gib: memory_gib,
            workspace_gib: 10,
            runtime_gib: 10,
        },
    );
}

#[tokio::test]
async fn a_calls_memory_comes_from_the_computer_and_not_from_fields_the_call_adds() {
    // 6 GiB above the 4 GiB floor, and a computer of 16 GiB.
    let _device = device(10 * GIB, 400 * GIB);
    let (id, fork) = computer();
    sized(&fork, 16);
    let caller = Caller::Computer { id };
    for extra in [
        json!({"computerId": fork, "memoryGiB": 0}),
        json!({"computerId": fork, "id": "someone-else", "memoryGiB": 0}),
        json!({"computerId": fork, "configuration": {"memoryGiB": 0}}),
        json!({"computerId": fork, "request": {"memoryGiB": 0}}),
    ] {
        for method in [
            MethodName::ComputersStart,
            MethodName::ComputersRestart,
            MethodName::MacosSetupRetry,
            MethodName::CheckpointsCreate,
            MethodName::CheckpointsRestore,
        ] {
            let mut params = extra.clone();
            if method == MethodName::CheckpointsCreate {
                params["name"] = json!("c");
            }
            if method == MethodName::CheckpointsRestore {
                params["checkpointId"] = json!("none");
            }
            let error = call(&caller, method, params).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", method.name());
        }
    }
}

#[tokio::test]
async fn a_new_computers_memory_is_the_size_it_is_made_with() {
    let _device = device(10 * GIB, 400 * GIB);
    let (id, _fork) = computer();
    let caller = Caller::Computer { id };
    // Asking is refused first when the computer would not fit, whatever else is in the call.
    let error = call(
        &caller,
        MethodName::ComputersCreate,
        json!({"kind": "macos", "memoryGiB": 0, "request": {"name": "gate-mac", "cpus": 2,
            "memoryGiB": 16, "diskGiB": 60}}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourcesLow);
}

#[test]
fn memory_set_aside_for_calls_made_at_the_same_moment_cannot_all_fit_the_same_headroom() {
    use silo_engine::resources::{Floors, Host};
    let host = Host {
        logical_cpus: 8,
        memory_total: 32 * GIB,
        memory_available: Some(10 * GIB),
        pressure: None,
        disk_free: 400 * GIB,
        disk_total: 500 * GIB,
    };
    let floors = Floors::resolve(&host, None, None);
    let _exclusive = super::test_exclusive();
    let engine = fixture().context.clone();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(12));
    let (id, _fork) = computer();
    let threads: Vec<_> = (0..12)
        .map(|_| {
            let (engine, barrier, host, id) =
                (engine.clone(), barrier.clone(), host.clone(), id.clone());
            std::thread::spawn(move || {
                barrier.wait();
                super::reserve_if_fits(
                    engine.as_ref(),
                    &super::Residency::default(),
                    &host,
                    &floors,
                    Some(id),
                    None,
                    2 * GIB,
                )
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let fitted: Vec<u64> = results
        .iter()
        .filter_map(|r| r.clone().ok().flatten())
        .collect();
    // 10 GiB free less the 4 GiB floor leaves 6 GiB, which fits three of 2 GiB.
    assert_eq!(fitted.len(), 3, "{results:?}");
    assert_eq!(results.iter().filter(|r| r.is_err()).count(), 9);
    for id in fitted {
        super::release_reservation(id);
    }
}

#[tokio::test]
async fn a_refused_call_gives_back_the_memory_it_was_going_to_reserve() {
    let _device = device(10 * GIB, 400 * GIB);
    let (id, fork) = computer();
    sized(&fork, 5);
    let caller = Caller::Computer { id: id.clone() };
    // Fits once; the call then fails in its handler (no such computer), which gives the memory back.
    for _ in 0..4 {
        let error = call(
            &caller,
            MethodName::ComputersStart,
            json!({"computerId": fork}),
        )
        .await
        .unwrap_err();
        assert_ne!(error.code, ErrorCode::ResourcesLow);
    }
}

/// A job the registry holds as finished, which nobody has acknowledged.
fn finished_job() -> String {
    let engine = fixture().context.as_ref();
    let job = silo_engine::jobs::allocate(engine, "changes.merge")
        .unwrap()
        .job_id;
    engine
        .job_registry()
        .unwrap()
        .finish(
            &job,
            &silo_api::jobs::JobFinished {
                job_id: job.clone(),
                result: Some(json!({"merged": true})),
                error: None,
                outcome_chunked: false,
            },
        )
        .unwrap();
    job.as_str().to_owned()
}

/// `count` finished jobs, created without flushing each commit to disk.
fn finished_jobs(count: usize) -> Vec<String> {
    engine()
        .job_registry()
        .unwrap()
        .without_flushing(|| (0..count).map(|_| finished_job()).collect())
}

/// A finished job of the computer `id`, which the computer has not acknowledged.
fn finished_job_of(id: &str) -> String {
    let job = finished_job();
    PolicyStore::new(&fixture().context.paths().app_data)
        .note_job(id, &job)
        .unwrap();
    job
}

#[tokio::test]
async fn acknowledging_a_job_does_not_free_its_place_while_the_owner_can_still_read_the_result() {
    let _device = device(32 * GIB, 400 * GIB);
    let (id, fork) = computer();
    sized(&fork, 1);
    let caller = Caller::Computer { id: id.clone() };
    let jobs: Vec<String> = (0..silo_engine::guest_client::MAX_JOBS)
        .map(|_| finished_job_of(&id))
        .collect();
    for job in &jobs {
        call(
            &caller,
            MethodName::OperationsJobAcknowledge,
            json!({"jobId": job}),
        )
        .await
        .unwrap();
    }
    // The charge is read from what was saved, so an engine that restarted still holds it.
    let saved = PolicyStore::new(&fixture().context.paths().app_data)
        .snapshot()
        .unwrap();
    assert!(jobs
        .iter()
        .all(|job| saved.kept.iter().any(|kept| &kept.id == job)));
    // The computer has nothing open, but the owner has not read any of the results.
    let error = call(
        &caller,
        MethodName::ComputersStart,
        json!({"computerId": fork}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert!(error.message.contains("still kept"), "{}", error.message);
    // The owner reads a result, then acknowledges them all, which releases them.
    let owner = Caller::owner("app");
    let read = call(
        &owner,
        MethodName::OperationsJobGet,
        json!({"jobId": jobs[0]}),
    )
    .await
    .unwrap();
    assert_eq!(read["state"], "finished");
    for job in &jobs {
        call(
            &owner,
            MethodName::OperationsJobAcknowledge,
            json!({"jobId": job}),
        )
        .await
        .unwrap();
    }
    let error = call(
        &caller,
        MethodName::ComputersStart,
        json!({"computerId": fork}),
    )
    .await
    .unwrap_err();
    assert_ne!(error.code, ErrorCode::PermissionDenied, "{}", error.message);
}

#[test]
fn computers_together_leave_the_owner_room_in_the_job_registry() {
    use silo_engine::guest_client::MAX_JOBS;
    assert!(super::job_refusal(0, 0, 0).is_none());
    assert!(super::job_refusal(MAX_JOBS - 1, 0, 0).is_none());
    assert!(super::job_refusal(MAX_JOBS - 1, 1, 0).is_some());
    assert!(super::job_refusal(0, MAX_JOBS, 0).is_some());
    assert!(super::job_refusal(1, 1, super::MAX_COMPUTER_JOBS - 1).is_none());
    let all = super::job_refusal(1, 1, super::MAX_COMPUTER_JOBS).unwrap();
    assert!(all.contains("together"), "{all}");
}

#[tokio::test]
async fn a_computer_whose_size_cannot_be_read_cannot_start_on_nothing() {
    let _device = device(10 * GIB, 400 * GIB);
    let (id, fork) = computer();
    let caller = Caller::Computer { id };
    for method in [
        MethodName::ComputersStart,
        MethodName::ComputersRestart,
        MethodName::MacosSetupRetry,
        MethodName::CheckpointsRestore,
    ] {
        let mut params = json!({"computerId": fork});
        if method == MethodName::CheckpointsRestore {
            params["checkpointId"] = json!("none");
        }
        let error = call(&caller, method, params).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", method.name());
        assert!(
            error.message.contains("can't read this computer's size"),
            "{}",
            error.message
        );
    }
}

#[tokio::test]
async fn memory_reserved_for_a_call_is_held_until_its_guard_is_dropped() {
    let _device = device(32 * GIB, 400 * GIB);
    let (_id, fork) = computer();
    sized(&fork, 3);
    let context = context(Caller::owner("app"));
    let held = super::reserve_now(
        &context,
        MethodName::ComputersStart,
        &json!({"computerId": fork}),
    )
    .await
    .unwrap();
    assert_eq!(super::reserved_memory_of(&fork), 3 * GIB);
    drop(held);
    assert_eq!(super::reserved_memory_of(&fork), 0);
}

#[tokio::test]
async fn a_call_dropped_before_it_finishes_gives_back_its_memory() {
    let _device = device(32 * GIB, 400 * GIB);
    let (id, fork) = computer();
    sized(&fork, 3);
    let caller = Caller::Computer { id };
    let admitted = super::admit(
        &context(caller),
        MethodName::ComputersStart,
        &json!({"computerId": fork}),
    )
    .await
    .unwrap();
    assert_eq!(super::reserved_memory_of(&fork), 3 * GIB);
    drop(admitted);
    assert_eq!(super::reserved_memory_of(&fork), 0);
}

/// A record of its own, so a test that fills it up leaves the other tests alone.
fn own_store() -> (tempfile::TempDir, PolicyStore) {
    let directory = tempfile::tempdir().unwrap();
    let store = PolicyStore::new(directory.path());
    (directory, store)
}

fn engine() -> &'static dyn EngineContext {
    fixture().context.as_ref()
}

#[tokio::test]
async fn a_start_that_resumes_a_full_checkpoint_reserves_the_memory_the_capture_recorded() {
    // 6 GiB above the 4 GiB floor.
    let _device = device(10 * GIB, 400 * GIB);
    let (id, fork) = computer();
    // Saved at 2 GiB now, but the checkpoint was captured at 16 GiB.
    sized(&fork, 2);
    super::test_resumes::add(&fork, Ok(16 * 1024));
    let caller = Caller::Computer { id: id.clone() };
    for method in [MethodName::ComputersStart, MethodName::ComputersRestart] {
        let error = call(&caller, method, json!({"computerId": fork}))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", method.name());
    }
    // One that was captured small enough fits.
    let (_id, small) = computer();
    sized(&small, 16);
    super::test_resumes::add(&small, Ok(2 * 1024));
    let error = call(
        &caller,
        MethodName::ComputersStart,
        json!({"computerId": small}),
    )
    .await
    .unwrap_err();
    assert_ne!(error.code, ErrorCode::ResourcesLow, "{}", error.message);
}

#[tokio::test]
async fn a_start_whose_checkpoint_memory_cannot_be_read_is_refused() {
    let _device = device(32 * GIB, 400 * GIB);
    let (id, fork) = computer();
    sized(&fork, 1);
    super::test_resumes::add(&fork, Err("The checkpoint could not be read.".into()));
    let error = call(
        &Caller::Computer { id },
        MethodName::ComputersStart,
        json!({"computerId": fork}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourcesLow);
    assert!(
        error.message.contains("could not be read"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn an_undo_or_a_promote_reserves_for_every_computer_it_can_start() {
    // 6 GiB above the floor: 4 GiB for either computer, 8 GiB for both.
    let _device = device(10 * GIB, 400 * GIB);
    let (id, fork) = computer();
    sized(&id, 4);
    sized(&fork, 4);
    let caller = Caller::Computer { id: id.clone() };
    let promote = json!({"forkId": fork, "targetId": id});
    let error = call(&caller, MethodName::ChangesPromote, promote.clone())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", error.message);
    // Asked for without a target, the computer it was forked from is the one replaced.
    super::test_nodes::add(&fork, "gate-fork-name", Some(&id));
    let error = call(&caller, MethodName::ChangesPromote, json!({"forkId": fork}))
        .await
        .unwrap_err();
    super::test_nodes::remove(&fork);
    assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", error.message);
    // A check that only reports starts nothing.
    let error = call(
        &caller,
        MethodName::ChangesPromote,
        json!({"forkId": fork, "targetId": id, "dryRun": true}),
    )
    .await
    .unwrap_err();
    assert_ne!(error.code, ErrorCode::ResourcesLow, "{}", error.message);
    // An undo of a computer that is too big for the device.
    sized(&fork, 7);
    let error = call(
        &caller,
        MethodName::ChangesUndo,
        json!({"computerId": fork}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", error.message);
}

#[tokio::test]
async fn an_undo_or_a_promote_of_a_computer_with_no_readable_size_is_refused() {
    let _device = device(32 * GIB, 400 * GIB);
    let (id, fork) = computer();
    let caller = Caller::Computer { id: id.clone() };
    for (method, params) in [
        (MethodName::ChangesUndo, json!({"computerId": fork})),
        (
            MethodName::ChangesPromote,
            json!({"forkId": fork, "targetId": id}),
        ),
    ] {
        let error = call(&caller, method, params).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", method.name());
        assert!(error.message.contains("can't read"), "{}", error.message);
    }
}

#[tokio::test]
async fn an_approved_undo_or_promote_reserves_memory_for_both_computers() {
    let _device = device(32 * GIB, 400 * GIB);
    let (id, fork) = computer();
    sized(&id, 2);
    sized(&fork, 3);
    let context = context(Caller::owner("app"));
    let held = super::reserve_now(
        &context,
        MethodName::ChangesPromote,
        &json!({"forkId": fork, "targetId": id}),
    )
    .await
    .unwrap();
    assert_eq!(super::reserved_memory(), 5 * GIB);
    drop(held);
    assert_eq!(super::reserved_memory(), 0);
}

#[test]
fn a_reservation_tied_to_a_running_job_outlasts_the_longest_a_start_takes_and_ends_with_the_job() {
    use silo_engine::resources::{Floors, Host};
    let host = Host {
        logical_cpus: 8,
        memory_total: 32 * GIB,
        memory_available: Some(20 * GIB),
        pressure: None,
        disk_free: 400 * GIB,
        disk_total: 500 * GIB,
    };
    let floors = Floors::resolve(&host, None, None);
    let _exclusive = super::test_exclusive();
    let (id, _fork) = computer();
    let reserve = |bytes: u64| {
        super::reserve_if_fits(
            engine(),
            &super::Residency::default(),
            &host,
            &floors,
            Some(id.clone()),
            None,
            bytes,
        )
        .unwrap()
        .unwrap()
    };
    let registry = engine().job_registry().unwrap();
    let job = silo_engine::jobs::allocate(engine(), "computers.start")
        .unwrap()
        .job_id;
    let tied = reserve(2 * GIB);
    super::attach_reservation_job(tied, job.as_str(), None);
    let untied = reserve(GIB);
    super::backdate_reservation(tied, 2 * super::RESERVATION_LIFE);
    super::backdate_reservation(untied, 2 * super::RESERVATION_LIFE);

    // The job is still running: its reservation stays, and the one nothing settled is dropped.
    super::current_reserved_memory(engine(), &super::Residency::default());
    assert_eq!(super::reserved_memory_of(&id), 2 * GIB);
    let next = reserve(GIB);
    assert_eq!(super::reserved_memory_of(&id), 3 * GIB);
    super::release_reservation(next);

    // The job ends: the memory goes back, and the same when the registry lets the job go.
    registry
        .finish(
            &job,
            &silo_api::jobs::JobFinished {
                job_id: job.clone(),
                result: Some(json!({"started": true})),
                error: None,
                outcome_chunked: false,
            },
        )
        .unwrap();
    super::current_reserved_memory(engine(), &super::Residency::default());
    assert_eq!(super::reserved_memory_of(&id), 0);
    let released = reserve(GIB);
    super::attach_reservation_job(released, job.as_str(), None);
    registry.acknowledge(&job).unwrap();
    super::current_reserved_memory(engine(), &super::Residency::default());
    assert_eq!(super::reserved_memory_of(&id), 0);
    let unknown = reserve(GIB);
    super::attach_reservation_job(unknown, "job:700000099", None);
    super::current_reserved_memory(engine(), &super::Residency::default());
    assert_eq!(super::reserved_memory_of(&id), 0);
}

/// A computer a test says holds its memory on the device, until the guard is dropped.
struct Resident(String);

impl Resident {
    fn new(id: &str) -> Self {
        super::test_resident::add(id);
        Self(id.to_owned())
    }
}

impl Drop for Resident {
    fn drop(&mut self) {
        super::test_resident::remove(&self.0);
    }
}

fn running_job() -> String {
    silo_engine::jobs::allocate(engine(), "computers.start")
        .unwrap()
        .job_id
        .as_str()
        .to_owned()
}

fn residency(ids: HashSet<String>) -> super::Residency {
    super::Residency {
        ids,
        loaded_at: std::time::Instant::now(),
    }
}

fn resident_set(ids: &[&str]) -> super::Residency {
    residency(ids.iter().map(|id| (*id).to_owned()).collect())
}

#[test]
fn a_reservation_for_a_job_ends_once_the_computer_it_boots_is_resident() {
    use silo_engine::resources::{Floors, Host};
    let host = Host {
        logical_cpus: 8,
        memory_total: 32 * GIB,
        memory_available: Some(20 * GIB),
        pressure: None,
        disk_free: 400 * GIB,
        disk_total: 500 * GIB,
    };
    let floors = Floors::resolve(&host, None, None);
    let _exclusive = super::test_exclusive();
    let (id, _fork) = computer();
    let booting = format!("{id}-booting");
    let reserve = |boots: Option<&str>| {
        super::reserve_if_fits(
            engine(),
            &super::Residency::default(),
            &host,
            &floors,
            Some(id.clone()),
            boots.map(str::to_owned),
            GIB,
        )
        .unwrap()
        .unwrap()
    };
    let job = running_job();
    let for_booting = reserve(Some(&booting));
    super::attach_reservation_job(for_booting, &job, None);
    let for_other = reserve(None);
    super::attach_reservation_job(for_other, &job, None);
    // Neither computer is resident yet: both reservations hold their memory.
    super::current_reserved_memory(engine(), &resident_set(&["someone-else"]));
    assert_eq!(super::reserved_memory_of(&id), 2 * GIB);
    // The booting computer is resident: its memory is in the device's measurement now, so only
    // the reservation that is not for it stays, even though the job still runs.
    super::current_reserved_memory(engine(), &resident_set(&[&booting]));
    assert_eq!(super::reserved_memory_of(&id), GIB);
    super::release_reservation(for_other);
}

#[test]
fn a_reservation_for_a_computer_a_call_created_is_released_when_that_computer_is_resident() {
    use silo_engine::resources::{Floors, Host};
    let host = Host {
        logical_cpus: 8,
        memory_total: 32 * GIB,
        memory_available: Some(20 * GIB),
        pressure: None,
        disk_free: 400 * GIB,
        disk_total: 500 * GIB,
    };
    let floors = Floors::resolve(&host, None, None);
    let _exclusive = super::test_exclusive();
    let (id, _fork) = computer();
    let made = format!("{id}-mac");
    // A macOS creation names no computer until its answer does.
    let reservation = super::reserve_if_fits(
        engine(),
        &super::Residency::default(),
        &host,
        &floors,
        None,
        None,
        4 * GIB,
    )
    .unwrap()
    .unwrap();
    super::attach_reservation_job(reservation, &running_job(), Some(&made));
    super::current_reserved_memory(engine(), &super::Residency::default());
    assert_eq!(super::reserved_memory(), 4 * GIB);
    super::current_reserved_memory(engine(), &resident_set(&[&made]));
    assert_eq!(super::reserved_memory(), 0);
}

#[test]
fn a_reservation_for_a_job_that_never_finished_is_dropped_at_the_ceiling() {
    use silo_engine::resources::{Floors, Host};
    let host = Host {
        logical_cpus: 8,
        memory_total: 32 * GIB,
        memory_available: Some(20 * GIB),
        pressure: None,
        disk_free: 400 * GIB,
        disk_total: 500 * GIB,
    };
    let floors = Floors::resolve(&host, None, None);
    let _exclusive = super::test_exclusive();
    let (id, _fork) = computer();
    let reservation = super::reserve_if_fits(
        engine(),
        &super::Residency::default(),
        &host,
        &floors,
        Some(id.clone()),
        None,
        GIB,
    )
    .unwrap()
    .unwrap();
    // The job's worker died: its record still says it runs.
    super::attach_reservation_job(reservation, &running_job(), None);
    let ceiling = super::JOB_RESERVATION_CEILING;
    super::backdate_reservation(reservation, ceiling - std::time::Duration::from_secs(60));
    super::current_reserved_memory(engine(), &super::Residency::default());
    assert_eq!(super::reserved_memory_of(&id), GIB);
    super::backdate_reservation(reservation, ceiling + std::time::Duration::from_secs(60));
    super::current_reserved_memory(engine(), &super::Residency::default());
    assert_eq!(super::reserved_memory_of(&id), 0);
}

fn big_host() -> (silo_engine::resources::Host, silo_engine::resources::Floors) {
    use silo_engine::resources::{Floors, Host};
    let host = Host {
        logical_cpus: 8,
        memory_total: 32 * GIB,
        memory_available: Some(20 * GIB),
        pressure: None,
        disk_free: 400 * GIB,
        disk_total: 500 * GIB,
    };
    let floors = Floors::resolve(&host, None, None);
    (host, floors)
}

#[test]
fn the_ceiling_of_a_job_reservation_counts_from_when_the_job_runs_and_not_from_admission() {
    let (host, floors) = big_host();
    let _exclusive = super::test_exclusive();
    let (id, _fork) = computer();
    let reservation = super::reserve_if_fits(
        engine(),
        &super::Residency::default(),
        &host,
        &floors,
        Some(id.clone()),
        None,
        GIB,
    )
    .unwrap()
    .unwrap();
    let job = running_job();
    super::attach_reservation_job(reservation, &job, None);
    let ceiling = super::JOB_RESERVATION_CEILING;
    let minute = std::time::Duration::from_secs(60);
    // A job queued for a long time behind another still holds its memory, and the clock
    // has not started while the registry has it waiting.
    super::backdate_reservation(reservation, ceiling - minute);
    super::current_reserved_memory(engine(), &super::Residency::default());
    assert_eq!(super::reserved_memory_of(&id), GIB);
    // Its turn comes: the ceiling counts from when the registry shows it running, however long
    // ago the reservation was made.
    let registry = engine().job_registry().unwrap();
    registry
        .mark_running(&silo_api::jobs::JobId::new(job.clone()).unwrap())
        .unwrap();
    super::current_reserved_memory(engine(), &super::Residency::default());
    super::backdate_reservation(reservation, 2 * ceiling);
    super::current_reserved_memory(engine(), &super::Residency::default());
    assert_eq!(super::reserved_memory_of(&id), GIB);
    super::backdate_running(reservation, ceiling - minute);
    super::current_reserved_memory(engine(), &super::Residency::default());
    assert_eq!(super::reserved_memory_of(&id), GIB);
    // A job that runs past the ceiling had its worker die.
    super::backdate_running(reservation, ceiling + minute);
    super::current_reserved_memory(engine(), &super::Residency::default());
    assert_eq!(super::reserved_memory_of(&id), 0);
}

#[test]
fn a_computer_list_loaded_before_a_reservation_cannot_end_it() {
    let (host, floors) = big_host();
    let _exclusive = super::test_exclusive();
    let (id, _fork) = computer();
    let booting = format!("{id}-restarted");
    // The list was read while the computer ran; the owner stopped it, and a call started it.
    let stale = resident_set(&[&booting]);
    let reservation = super::reserve_if_fits(
        engine(),
        &super::Residency::default(),
        &host,
        &floors,
        Some(id.clone()),
        Some(booting.clone()),
        GIB,
    )
    .unwrap()
    .unwrap();
    super::attach_reservation_job(reservation, &running_job(), None);
    super::current_reserved_memory(engine(), &stale);
    assert_eq!(super::reserved_memory_of(&id), GIB);
    // A list read after the reservation was made is evidence.
    super::current_reserved_memory(engine(), &resident_set(&[&booting]));
    assert_eq!(super::reserved_memory_of(&id), 0);
}

#[tokio::test]
async fn a_started_job_stops_holding_memory_once_its_computer_is_resident_and_a_restart_holds_nothing(
) {
    let _device = device(32 * GIB, 400 * GIB);
    let (id, fork) = computer();
    sized(&fork, 3);
    let context = context(Caller::owner("app"));
    let reserve = |method| {
        let (context, params) = (&context, json!({"computerId": fork}));
        async move {
            let held = super::reserve_now(context, method, &params).await.unwrap();
            held.settle(Some(&running_job()), None);
            held
        }
    };
    // A start of a computer that is not resident boots it: the reservation ends with it.
    let start = reserve(MethodName::ComputersStart).await;
    assert_eq!(super::reserved_memory_of(&fork), 3 * GIB);
    {
        let _resident = Resident::new(&fork);
        // Any later call prunes against what is resident then.
        super::current_reserved_memory(engine(), &residency(super::test_resident::all()));
        assert_eq!(super::reserved_memory_of(&fork), 0);
    }
    drop(start);
    // A restart of a computer that is resident now holds the memory it boots with already, so it
    // sets aside nothing, or only what a larger saved state takes beyond that.
    let restart = {
        let _resident = Resident::new(&fork);
        reserve(MethodName::ComputersRestart).await
    };
    {
        let _resident = Resident::new(&fork);
        super::current_reserved_memory(engine(), &residency(super::test_resident::all()));
        assert_eq!(super::reserved_memory_of(&fork), 0);
    }
    drop(restart);
    super::clear_reservations();
    // A creation names its computer, which is not resident before it exists.
    let created = format!("{id}-created");
    let held = super::reserve_now(
        &context,
        MethodName::ComputersCreate,
        &json!({"kind": "linux", "configuration": {"id": created, "name": "gate-created",
            "cpus": 2, "maxCPUs": 2, "memoryGiB": 1, "maxMemoryGiB": 1,
            "workspaceStorageGiB": 10, "runtimeStorageGiB": 10}}),
    )
    .await
    .unwrap();
    held.settle(Some(&running_job()), None);
    assert_eq!(super::reserved_memory(), GIB);
    super::current_reserved_memory(engine(), &resident_set(&[&created]));
    assert_eq!(super::reserved_memory(), 0);
}

#[tokio::test]
async fn a_restart_keeps_its_computers_memory_reserved_while_it_is_stopped() {
    // 10 GiB above the 4 GiB floor.
    let _device = device(14 * GIB, 400 * GIB);
    let (id, big) = computer();
    let other = format!("{id}-other");
    sized(&big, 8);
    sized(&other, 8);
    let context = context(Caller::owner("app"));
    let restart = {
        let _resident = Resident::new(&big);
        let held = super::reserve_now(
            &context,
            MethodName::ComputersRestart,
            &json!({"computerId": big}),
        )
        .await
        .unwrap();
        held.settle(Some(&running_job()), None);
        held
    };
    let start_other = || async {
        super::reserve_now(
            &context,
            MethodName::ComputersStart,
            &json!({"computerId": other}),
        )
        .await
    };
    // The list the restart was admitted on shows the computer running: nothing is counted twice.
    assert_eq!(super::reserved_memory_of(&big), 0);
    {
        let _resident = Resident::new(&big);
        let held = start_other().await.unwrap();
        held.settle(Some(&running_job()), None);
    }
    super::clear_reservations();
    let restart_again = {
        let _resident = Resident::new(&big);
        let held = super::reserve_now(
            &context,
            MethodName::ComputersRestart,
            &json!({"computerId": big}),
        )
        .await
        .unwrap();
        held.settle(Some(&running_job()), None);
        held
    };
    // Between the stop and the boot a list read afterwards no longer shows it: its memory is
    // taken from the headroom, so another start of that size is refused.
    let error = start_other()
        .await
        .err()
        .expect("refused while the computer is stopped");
    assert!(error.contains("memory"), "{error}");
    // Resident again: its memory is in the device's measurement and counts once.
    {
        let _resident = Resident::new(&big);
        let held = start_other().await.unwrap();
        held.settle(Some(&running_job()), None);
    }
    drop((restart, restart_again));
    super::clear_reservations();
}

#[tokio::test]
async fn a_macos_creation_reservation_follows_the_computer_its_answer_names() {
    let _device = device(32 * GIB, 400 * GIB);
    let (id, _fork) = computer();
    let made = format!("{id}-mac");
    let context = context(Caller::owner("app"));
    let held = super::reserve_now(
        &context,
        MethodName::ComputersCreate,
        &json!({"kind": "macos", "request": {"name": "gate-mac", "cpus": 2,
            "memoryGiB": 8, "diskGiB": 60}}),
    )
    .await
    .unwrap();
    let answer = Ok(crate::dispatcher::HandlerOutput {
        value: json!({"jobId": "job:1", "computer": {"id": made}}),
        event_barrier: None,
    });
    assert_eq!(
        super::computer_made(&answer).as_deref(),
        Some(made.as_str())
    );
    held.settle(
        Some(&running_job()),
        super::computer_made(&answer).as_deref(),
    );
    assert_eq!(super::reserved_memory(), 8 * GIB);
    // The installer is running: the computer holds its memory.
    super::current_reserved_memory(engine(), &resident_set(&[&made]));
    assert_eq!(super::reserved_memory(), 0);
}

#[tokio::test]
async fn a_checkpoint_of_a_computer_that_is_resident_reserves_no_more_memory() {
    // 6 GiB above the 4 GiB floor, and a computer of 16 GiB.
    let _device = device(10 * GIB, 400 * GIB);
    let (id, fork) = computer();
    sized(&fork, 16);
    let caller = Caller::Computer { id: id.clone() };
    let params = json!({"computerId": fork, "name": "c"});
    let error = call(&caller, MethodName::CheckpointsCreate, params.clone())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", error.message);
    let _resident = Resident::new(&fork);
    let error = call(&caller, MethodName::CheckpointsCreate, params)
        .await
        .unwrap_err();
    assert_ne!(error.code, ErrorCode::ResourcesLow, "{}", error.message);
    assert_eq!(super::reserved_memory_of(&fork), 0);
}

fn full_checkpoint(id: &str, scope: &str) -> silo_engine::changes::keep::CheckpointRef {
    silo_engine::changes::keep::CheckpointRef {
        id: id.into(),
        name: format!("before-keep-{id}"),
        created_at: String::new(),
        scope: scope.into(),
    }
}

/// A keep journal in which the checkpoint `id` of `scope` is the one an undo of "main" restores.
fn journal_with(
    directory: &tempfile::TempDir,
    id: &str,
    scope: &str,
) -> silo_engine::changes::keep::Journal {
    let journal = silo_engine::changes::keep::Journal::new(directory.path());
    journal
        .record_pending("main", &full_checkpoint(id, scope))
        .unwrap();
    journal
        .set_state(
            "main",
            id,
            silo_engine::changes::keep::journal::State::Undoable,
        )
        .unwrap();
    journal
}

#[test]
fn an_undo_of_a_full_checkpoint_resumes_at_the_memory_the_capture_recorded() {
    let directory = tempfile::tempdir().unwrap();
    let journal = journal_with(&directory, "chk-full", "full");
    let read = |id: &str| {
        assert_eq!(id, "chk-full");
        Ok(Some(8 * 1024))
    };
    assert_eq!(
        super::keep_undo_resume(&journal, "main", None, &read),
        Some(Ok(8 * 1024))
    );
    // A promote made after the keep is what the undo takes back, which resumes no checkpoint.
    let promoted_later = Some(silo_engine::changes::promote::journal::now_millis() + 60_000);
    assert_eq!(
        super::keep_undo_resume(&journal, "main", promoted_later, &read),
        None
    );
}

#[test]
fn an_undo_of_a_macos_computer_resumes_at_its_configured_memory_without_reading_a_linux_record() {
    let directory = tempfile::tempdir().unwrap();
    let journal = journal_with(&directory, "chk-mac", "full");
    let mac = silo_engine::policy::Resources::of_macos(
        &json!({"cpus": 4, "memoryGiB": 12, "diskGiB": 100}),
    )
    .unwrap();
    let linux_record = || -> Result<Option<u64>, String> { Ok(None) };
    let resume = super::keep_undo_resume(&journal, "main", None, &|_| {
        super::undo_checkpoint_memory(Some(&mac), linux_record)
    });
    assert_eq!(resume, Some(Ok(12 * 1024)));
    // A Linux computer is read from the checkpoint it resumes.
    let linux = silo_engine::policy::Resources::of_linux(&json!({
        "cpus": 2, "maxCPUs": 2, "memoryGiB": 2, "maxMemoryGiB": 2,
        "workspaceStorageGiB": 10, "runtimeStorageGiB": 10
    }))
    .unwrap();
    assert_eq!(
        super::undo_checkpoint_memory(Some(&linux), || Ok(Some(8 * 1024))),
        Ok(Some(8 * 1024))
    );
    assert_eq!(
        super::undo_checkpoint_memory(None, || Ok(Some(6 * 1024))),
        Ok(Some(6 * 1024))
    );
}

#[test]
fn an_undo_of_a_disk_checkpoint_or_of_nothing_resumes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let journal = journal_with(&directory, "chk-disk", "disk");
    let never = |_: &str| -> Result<Option<u64>, String> { panic!("no capture is read") };
    assert_eq!(
        super::keep_undo_resume(&journal, "main", None, &never),
        None
    );
    assert_eq!(
        super::keep_undo_resume(&journal, "other", None, &never),
        None
    );
}

#[test]
fn an_undo_whose_checkpoint_memory_cannot_be_read_cannot_be_sized() {
    let directory = tempfile::tempdir().unwrap();
    let journal = journal_with(&directory, "chk-full", "full");
    for read in [
        Err("The runtime did not answer.".to_owned()),
        // A full checkpoint the computer's record does not list.
        Ok(None),
    ] {
        let resume = super::keep_undo_resume(&journal, "main", None, &|_| read.clone());
        let Some(Err(message)) = resume else {
            panic!("{resume:?}");
        };
        assert!(message.contains("can't read"), "{message}");
    }
    // An interrupted keep that is waiting to be put back is the checkpoint the undo restores.
    let pending = silo_engine::changes::keep::journal::Entry {
        fork: "fork".into(),
        target: "main".into(),
        checkpoint_id: "chk-pending".into(),
        checkpoint_name: "before-keep".into(),
        checkpoint_scope: "full".into(),
    };
    journal.begin(&pending).unwrap();
    let asked = std::cell::RefCell::new(String::new());
    let resume = super::keep_undo_resume(&journal, "main", None, &|id| {
        *asked.borrow_mut() = id.to_owned();
        Ok(Some(2 * 1024))
    });
    assert_eq!(resume, Some(Ok(2 * 1024)));
    assert_eq!(*asked.borrow(), "chk-pending");
}

#[tokio::test]
async fn an_undo_is_sized_by_the_memory_its_checkpoint_resumes_at_and_not_the_saved_size() {
    // 6 GiB above the 4 GiB floor.
    let _device = device(10 * GIB, 400 * GIB);
    let (id, fork) = computer();
    // Kept at 8 GiB, lowered to 2 GiB since.
    sized(&fork, 2);
    super::test_resumes::add(&fork, Ok(8 * 1024));
    let caller = Caller::Computer { id: id.clone() };
    let error = call(
        &caller,
        MethodName::ChangesUndo,
        json!({"computerId": fork}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", error.message);
    // One whose checkpoint cannot be read is refused rather than sized by the saved size.
    let (other, unread) = computer();
    sized(&unread, 1);
    super::test_resumes::add(&unread, Err("The checkpoint could not be read.".into()));
    let error = call(
        &Caller::Computer { id: other },
        MethodName::ChangesUndo,
        json!({"computerId": unread}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", error.message);
    assert!(
        error.message.contains("could not be read"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn a_running_computer_restarted_or_undone_needs_none_of_the_memory_it_already_holds() {
    // 6 GiB above the 4 GiB floor.
    let _device = device(10 * GIB, 400 * GIB);
    let (id, big) = computer();
    sized(&big, 7);
    let caller = Caller::Computer { id: id.clone() };
    let calls = [
        (MethodName::ComputersRestart, json!({"computerId": big})),
        (MethodName::ChangesUndo, json!({"computerId": big})),
    ];
    // Stopped, the computer is sized in full.
    for (method, params) in calls.clone() {
        let error = call(&caller, method, params).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", method.name());
    }
    // Running, it already holds that memory.
    let _resident = Resident::new(&big);
    for (method, params) in calls {
        let Err(error) = call(&caller, method, params).await else {
            continue;
        };
        assert_ne!(
            error.code,
            ErrorCode::ResourcesLow,
            "{}: {}",
            method.name(),
            error.message
        );
    }
}

#[tokio::test]
async fn a_running_computer_resuming_a_larger_saved_state_needs_only_the_difference() {
    // 6 GiB above the 4 GiB floor.
    let _device = device(10 * GIB, 400 * GIB);
    let (id, running) = computer();
    sized(&running, 4);
    let _resident = Resident::new(&running);
    let caller = Caller::Computer { id };
    // 4 GiB held, 9 GiB resumed: 5 GiB more fits.
    super::test_resumes::add(&running, Ok(9 * 1024));
    let error = call(
        &caller,
        MethodName::ComputersRestart,
        json!({"computerId": running}),
    )
    .await
    .unwrap_err();
    assert_ne!(error.code, ErrorCode::ResourcesLow, "{}", error.message);
    // 4 GiB held, 11 GiB resumed: 7 GiB more does not.
    super::test_resumes::add(&running, Ok(11 * 1024));
    let error = call(
        &caller,
        MethodName::ComputersRestart,
        json!({"computerId": running}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", error.message);
    // A resume that cannot be read is still refused.
    super::test_resumes::add(&running, Err("The checkpoint could not be read.".into()));
    let error = call(
        &caller,
        MethodName::ComputersRestart,
        json!({"computerId": running}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourcesLow, "{}", error.message);
}

#[tokio::test]
async fn a_running_computer_with_no_readable_size_is_still_refused() {
    let _device = device(32 * GIB, 400 * GIB);
    let (id, running) = computer();
    let _resident = Resident::new(&running);
    let error = call(
        &Caller::Computer { id },
        MethodName::ComputersRestart,
        json!({"computerId": running}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourcesLow);
    assert!(error.message.contains("can't read"), "{}", error.message);
}

#[test]
fn a_kept_result_from_before_a_restart_gets_a_deadline_again_and_its_charge_clears_with_it() {
    let (_directory, store) = own_store();
    let job = finished_job();
    store.import_legacy("gate-kept", &[], &[]).unwrap();
    store.keep_job("gate-kept", &job).unwrap();
    let registry = engine().job_registry().unwrap();
    let id = silo_api::jobs::JobId::new(&job).unwrap();
    // The engine that kept the result is gone, and with it the registry's deadline.
    assert_eq!(registry.retention_deadline_ms(&id), None);
    let document = super::current_charges(engine(), &store, store.snapshot().unwrap());
    assert_eq!(document.kept.len(), 1);
    assert!(registry.retention_deadline_ms(&id).is_some());
    // The owner acknowledging it releases the result and the charge together.
    registry.acknowledge(&id).unwrap();
    let document = super::current_charges(engine(), &store, store.snapshot().unwrap());
    assert!(document.kept.is_empty());
    assert!(store.snapshot().unwrap().kept.is_empty());
}

#[test]
fn open_jobs_the_registry_has_released_stop_counting_toward_any_limit() {
    let (_directory, store) = own_store();
    // More than all computers together may hold, none of them in the registry any more.
    let stale: Vec<(String, u64)> = (0..super::MAX_COMPUTER_JOBS + 5)
        .map(|n| (format!("job:{}", 800_000_000 + n), 1))
        .collect();
    store.import_legacy("gate-stale", &[], &stale).unwrap();
    let slot = super::reserve_job_slot(engine(), &store, "gate-fresh").unwrap();
    super::release_job_slot(slot);
    // The computer that held them is not held back by them either.
    let slot = super::reserve_job_slot(engine(), &store, "gate-stale").unwrap();
    super::release_job_slot(slot);
    assert!(store.snapshot().unwrap().jobs.is_empty());
}

#[test]
fn the_jobs_of_a_deleted_computer_count_until_the_registry_releases_them() {
    let _exclusive = super::test_exclusive();
    let (_directory, store) = own_store();
    let jobs = finished_jobs(super::MAX_COMPUTER_JOBS);
    let owned: Vec<(String, u64)> = jobs.iter().map(|job| (job.clone(), 1)).collect();
    store.import_legacy("gate-gone", &[], &owned).unwrap();
    store.remove_computers(&["gate-gone"]).unwrap();
    let error = super::reserve_job_slot(engine(), &store, "gate-other").unwrap_err();
    assert!(error.contains("together"), "{error}");
    // The registry letting some go makes room for another computer's job. Places other tests
    // hold for a moment count too, so more than one is freed.
    let registry = engine().job_registry().unwrap();
    for job in &jobs[..8] {
        registry
            .acknowledge(&silo_api::jobs::JobId::new(job).unwrap())
            .unwrap();
    }
    let slot = super::reserve_job_slot(engine(), &store, "gate-other").unwrap();
    super::release_job_slot(slot);
    registry.without_flushing(|| {
        for job in &jobs {
            registry
                .acknowledge(&silo_api::jobs::JobId::new(job).unwrap())
                .unwrap();
        }
    });
}

#[test]
fn the_jobs_of_deleted_computers_stop_counting_once_they_are_over() {
    let _exclusive = super::test_exclusive();
    let (_directory, store) = own_store();
    let registry = engine().job_registry().unwrap();
    let state_of = |job: &str| {
        registry
            .state_of(&silo_api::jobs::JobId::new(job).unwrap())
            .unwrap()
    };
    let mut jobs = finished_jobs(super::MAX_COMPUTER_JOBS - 1);
    let running = silo_engine::jobs::allocate(engine(), "changes.merge")
        .unwrap()
        .job_id;
    jobs.push(running.as_str().to_owned());
    let owned: Vec<(String, u64)> = jobs.iter().map(|job| (job.clone(), 1)).collect();
    store.import_legacy("gate-gone", &[], &owned).unwrap();
    let live = finished_job();
    store.note_job("gate-live", &live).unwrap();
    store.remove_computers(&["gate-gone"]).unwrap();
    let error = super::reserve_job_slot(engine(), &store, "gate-other").unwrap_err();
    assert!(error.contains("together"), "{error}");

    let gone = |computer: &str| computer == "gate-gone";
    crate::methods::operations::release_jobs_of_gone_computers(engine(), &store, &gone);
    // The finished jobs are acknowledged and forgotten; the one still running keeps counting,
    // and so does a job of a computer that still exists.
    for job in &jobs[..jobs.len() - 1] {
        assert_eq!(state_of(job), None, "{job}");
    }
    assert!(state_of(running.as_str()).is_some());
    assert!(state_of(&live).is_some());
    let held: Vec<String> = store
        .snapshot()
        .unwrap()
        .jobs
        .into_iter()
        .map(|job| job.id)
        .collect();
    assert_eq!(held.len(), 2);
    assert!(held.contains(&live) && held.contains(&running.as_str().to_owned()));
    let slot = super::reserve_job_slot(engine(), &store, "gate-other").unwrap();
    super::release_job_slot(slot);

    // Once it has finished, the last job of the deleted computer goes too.
    registry
        .finish(
            &running,
            &silo_api::jobs::JobFinished {
                job_id: running.clone(),
                result: Some(json!({"merged": true})),
                error: None,
                outcome_chunked: false,
            },
        )
        .unwrap();
    crate::methods::operations::release_jobs_of_gone_computers(engine(), &store, &gone);
    assert_eq!(state_of(running.as_str()), None);
    let held = store.snapshot().unwrap().jobs;
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].id, live);
    registry
        .acknowledge(&silo_api::jobs::JobId::new(&live).unwrap())
        .unwrap();
}

#[test]
fn computers_starting_jobs_at_the_same_moment_cannot_all_take_the_last_places() {
    use silo_engine::guest_client::MAX_JOBS;
    let _exclusive = super::test_exclusive();
    let (_directory, store) = own_store();
    let store = std::sync::Arc::new(store);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3 * MAX_JOBS));
    let threads: Vec<_> = (0..3 * MAX_JOBS)
        .map(|_| {
            let (store, barrier) = (store.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                super::reserve_job_slot(engine(), &store, "gate-racer")
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let places: Vec<u64> = results.iter().filter_map(|r| r.clone().ok()).collect();
    assert_eq!(places.len(), MAX_JOBS, "{results:?}");
    for place in places {
        super::release_job_slot(place);
    }
    assert_eq!(super::job_slots_of("gate-racer"), 0);
}

#[tokio::test]
async fn a_job_call_holds_its_place_until_the_record_has_its_job_and_gives_it_back_otherwise() {
    let _device = device(32 * GIB, 400 * GIB);
    let (id, fork) = computer();
    sized(&fork, 1);
    let caller = Caller::Computer { id: id.clone() };
    let admitted = super::admit(
        &context(caller.clone()),
        MethodName::ComputersStart,
        &json!({"computerId": fork}),
    )
    .await
    .unwrap();
    assert_eq!(super::job_slots_of(&id), 1);
    // The call is dropped before it finishes, as when its caller goes away.
    drop(admitted);
    assert_eq!(super::job_slots_of(&id), 0);
    // A call that fails starts no job and takes no place.
    let error = call(
        &caller,
        MethodName::ComputersStart,
        json!({"computerId": fork}),
    )
    .await
    .unwrap_err();
    assert_ne!(error.code, ErrorCode::ResourcesLow);
    assert_eq!(super::job_slots_of(&id), 0);
    // A job the record could not take keeps its place; one it holds gives the place back.
    let (_directory, store) = own_store();
    for (recorded, kept) in [(true, 0), (false, 1)] {
        let slot = super::reserve_job_slot(engine(), &store, "gate-settled").unwrap();
        let mut held = super::Held::of(None);
        held.slot = Some(slot);
        held.settle_slot(Some("job:700000001"), recorded);
        drop(held);
        assert_eq!(super::job_slots_of("gate-settled"), kept);
        super::release_job_slot(slot);
    }
}

#[test]
fn a_held_computer_counts_until_a_list_loaded_after_the_reservation_shows_it_resident() {
    let before = std::time::Instant::now();
    let listed_before = residency(HashSet::new());
    std::thread::sleep(std::time::Duration::from_millis(5));
    let reservation = super::Reservation {
        id: 1,
        computer_id: None,
        boots: None,
        job_id: None,
        memory_bytes: GIB,
        held: vec![("held".to_owned(), 2 * GIB)],
        created_at: std::time::Instant::now(),
        running_since: None,
    };
    assert!(before < reservation.created_at);
    // A list read before the reservation says nothing: the host measured after may show the
    // computer's memory free already.
    assert_eq!(reservation.counted(&listed_before), 3 * GIB);
    let mut with_it = resident_set(&["held"]);
    with_it.loaded_at = before;
    assert_eq!(reservation.counted(&with_it), 3 * GIB);
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert_eq!(reservation.counted(&resident_set(&[])), 3 * GIB);
    assert_eq!(reservation.counted(&resident_set(&["held"])), GIB);
}
