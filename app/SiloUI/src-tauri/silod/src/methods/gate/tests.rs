use std::collections::BTreeSet;

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
                super::reserve_if_fits(engine.as_ref(), &host, &floors, Some(id), 2 * GIB)
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

/// A finished job of the computer `id`, which the computer has not acknowledged.
fn finished_job_of(id: &str) -> String {
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
    PolicyStore::new(&engine.paths().app_data)
        .note_job(id, job.as_str())
        .unwrap();
    job.as_str().to_owned()
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
