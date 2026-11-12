use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use serde_json::{json, Value};
use silo_api::{
    jobs::ErrorCode,
    methods::{
        audit::{AuditListParams, AuditOutcome},
        MethodName,
    },
    transport::{ApprovalKind, PROTOCOL_VERSION},
};
use silo_engine::{
    approvals,
    audit::{AuditLog, Scope},
    context::EngineContext,
    fold_requests,
    policy::{store::PolicyStore, Caller},
};

use super::{request_answered, result_goes_to_computer, TestRun, FOR_COMPUTER, TEST_RUN};
use crate::{
    dispatcher::{DispatchError, RequestContext},
    methods::{access::tests::fixture, dispatch, gate::test_nodes, HandlerContext},
};

fn context(caller: Caller) -> HandlerContext {
    HandlerContext::new(
        fixture().context.clone(),
        RequestContext {
            client_id: "approvals-test".into(),
            protocol: PROTOCOL_VERSION,
            client: None,
            caller,
        },
    )
}

fn app_data() -> std::path::PathBuf {
    fixture().context.paths().app_data.clone()
}

/// A computer of its own for each test, `me`, which made `fork`. Both exist.
struct Tree {
    me: String,
    fork: String,
}

impl Tree {
    fn new() -> Self {
        let me = format!("apt-{}", uuid::Uuid::new_v4());
        let fork = format!("{me}-fork");
        PolicyStore::new(&app_data())
            .import_legacy(&me, std::slice::from_ref(&fork), &[])
            .unwrap();
        test_nodes::add(&me, &format!("{me}-name"), None);
        test_nodes::add(&fork, &format!("{fork}-name"), Some(&me));
        for id in [&me, &fork] {
            crate::methods::gate::test_resources::add(
                id,
                silo_engine::policy::Resources {
                    linux: true,
                    cpus: 2,
                    max_cpus: 2,
                    memory_gib: 1,
                    max_memory_gib: 1,
                    workspace_gib: 10,
                    runtime_gib: 10,
                },
            );
        }
        Self { me, fork }
    }

    fn me(&self) -> Caller {
        Caller::Computer {
            id: self.me.clone(),
        }
    }

    fn child(&self) -> Caller {
        Caller::Computer {
            id: self.fork.clone(),
        }
    }
}

/// Tests that answer approvals run one at a time: the engine-start recovery a test runs ends
/// every allowed call still waiting for its outcome in the shared store.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn owner() -> Caller {
    Caller::owner("app")
}

async fn call(caller: &Caller, method: MethodName, params: Value) -> Result<Value, DispatchError> {
    dispatch(&context(caller.clone()), method, &params)
        .await
        .map(|output| output.value)
}

/// Asks for a held call and returns the approval's id.
async fn ask(caller: &Caller, method: MethodName, params: Value) -> String {
    let error = call(caller, method, params).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::ApprovalRequired, "{}", error.message);
    error.details.unwrap().approval_id.unwrap()
}

async fn get(caller: &Caller, id: &str) -> Result<Value, DispatchError> {
    call(caller, MethodName::ApprovalsGet, json!({"id": id})).await
}

async fn decide(id: &str, decision: &str) -> Result<Value, DispatchError> {
    call(
        &owner(),
        MethodName::ApprovalsDecide,
        json!({"id": id, "decision": decision}),
    )
    .await
}

type Runs = Arc<Mutex<Vec<(MethodName, Value)>>>;

/// A stand-in for the engine running allowed calls: it records each and answers with
/// `answer`.
fn runner(
    answer: impl Fn(MethodName, &Value) -> Result<Value, DispatchError> + Send + Sync + 'static,
) -> (TestRun, Runs) {
    let runs: Runs = Arc::default();
    let recorded = runs.clone();
    let run: TestRun = Arc::new(move |method, params: &Value| {
        recorded.lock().unwrap().push((method, params.clone()));
        answer(method, params)
    });
    (run, runs)
}

fn job_runner() -> (TestRun, Runs) {
    runner(|method, _| {
        Ok(match method {
            MethodName::NetworkPublish | MethodName::SshAccessSave | MethodName::SecretsSave => {
                json!({"computers": []})
            }
            _ => {
                json!({"jobId": format!("job:{}", 700_000 + uuid::Uuid::new_v4().as_u128() % 99_999)})
            }
        })
    })
}

fn notice_keys() -> Vec<String> {
    silo_engine::notices::list(fixture().context.as_ref())
        .unwrap()
        .into_iter()
        .map(|notice| notice.key)
        .collect()
}

fn ids(list: &Value) -> BTreeSet<String> {
    list["approvals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|approval| approval["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn a_held_call_is_stored_found_again_and_noticed() {
    let tree = Tree::new();
    let params = json!({"computerId": tree.fork, "port": 8080});
    let id = ask(&tree.me(), MethodName::NetworkPublish, params.clone()).await;
    assert_eq!(
        ask(&tree.me(), MethodName::NetworkPublish, params.clone()).await,
        id
    );
    let other = ask(
        &tree.me(),
        MethodName::NetworkPublish,
        json!({"computerId": tree.fork, "port": 8081}),
    )
    .await;
    assert_ne!(other, id);
    let approval = get(&owner(), &id).await.unwrap();
    assert_eq!(approval["state"], "pending");
    assert_eq!(approval["kind"], "port");
    assert_eq!(approval["computerId"], tree.me);
    assert_eq!(approval["computerName"], format!("{}-name", tree.me));
    assert_eq!(approval["method"], "network.publish");
    assert_eq!(approval["params"], params);
    assert!(approval["summary"].as_str().unwrap().contains("8080"));
    let notice = silo_engine::notices::list(fixture().context.as_ref())
        .unwrap()
        .into_iter()
        .find(|notice| notice.key == format!("approval:{id}"))
        .unwrap();
    assert!(
        notice.title.contains(&format!("{}-name", tree.me)),
        "{}",
        notice.title
    );
    assert!(notice.title.contains("8080"), "{}", notice.title);
    // The ask is in the audit log with the approval it made.
    let visible: BTreeSet<String> = [tree.fork.clone()].into();
    let rows = AuditLog::shared(&app_data())
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
        .entries;
    assert!(rows
        .iter()
        .any(|row| row.outcome == AuditOutcome::Asked
            && row.approval_id.as_deref() == Some(id.as_str())));
}

#[tokio::test]
async fn a_computer_sees_its_approvals_and_those_of_computers_it_made_only() {
    let tree = Tree::new();
    let stranger = Tree::new();
    let mine = ask(
        &tree.me(),
        MethodName::NetworkPublish,
        json!({"computerId": tree.me, "port": 1}),
    )
    .await;
    let childs = ask(
        &tree.child(),
        MethodName::NetworkPublish,
        json!({"computerId": tree.fork, "port": 2}),
    )
    .await;
    let others = ask(
        &stranger.me(),
        MethodName::NetworkPublish,
        json!({"computerId": stranger.me, "port": 3}),
    )
    .await;
    let seen = ids(&call(&tree.me(), MethodName::ApprovalsList, json!({}))
        .await
        .unwrap());
    assert!(seen.contains(&mine) && seen.contains(&childs) && !seen.contains(&others));
    let seen = ids(&call(&tree.child(), MethodName::ApprovalsList, json!({}))
        .await
        .unwrap());
    assert!(seen.contains(&childs) && !seen.contains(&mine));
    let error = get(&stranger.me(), &mine).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(get(&tree.me(), &childs).await.unwrap()["state"], "pending");
    let all = ids(&call(&owner(), MethodName::ApprovalsList, json!({}))
        .await
        .unwrap());
    assert!(all.contains(&mine) && all.contains(&childs) && all.contains(&others));
    let only_child = ids(&call(
        &owner(),
        MethodName::ApprovalsList,
        json!({"computerId": tree.fork, "state": "pending"}),
    )
    .await
    .unwrap());
    assert_eq!(only_child, [childs].into());
}

#[tokio::test]
async fn only_the_owner_answers_and_only_the_asker_writes_its_note_or_withdraws() {
    let _serial = serial();
    let tree = Tree::new();
    let id = ask(
        &tree.child(),
        MethodName::NetworkPublish,
        json!({"computerId": tree.fork, "port": 9}),
    )
    .await;
    let error = call(
        &tree.me(),
        MethodName::ApprovalsDecide,
        json!({"id": id, "decision": "allow"}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    // The computer that made the asker sees it but does not speak for it.
    assert!(
        call(&tree.me(), MethodName::ApprovalsCancel, json!({"id": id}))
            .await
            .is_err()
    );
    assert!(call(
        &tree.me(),
        MethodName::ApprovalsNote,
        json!({"id": id, "note": "x"})
    )
    .await
    .is_err());
    assert!(call(
        &owner(),
        MethodName::ApprovalsNote,
        json!({"id": id, "note": "x"})
    )
    .await
    .is_err());
    assert!(call(
        &tree.child(),
        MethodName::ApprovalsNote,
        json!({"id": id, "note": "y".repeat(501)})
    )
    .await
    .is_err());
    let noted = call(
        &tree.child(),
        MethodName::ApprovalsNote,
        json!({"id": id, "note": "Publish port 22 of every computer."}),
    )
    .await
    .unwrap();
    assert_eq!(noted["note"], "Publish port 22 of every computer.");
    assert!(noted["summary"].as_str().unwrap().contains("port 9"));
    assert_eq!(noted["params"]["port"], 9);
    assert!(notice_keys().contains(&format!("approval:{id}")));
    let cancelled = call(
        &tree.child(),
        MethodName::ApprovalsCancel,
        json!({"id": id}),
    )
    .await
    .unwrap();
    assert_eq!(cancelled["state"], "cancelled");
    assert!(!notice_keys().contains(&format!("approval:{id}")));
    // The computer's own withdrawal removes the approval at once.
    assert_eq!(
        decide(&id, "allow").await.unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        get(&owner(), &id).await.unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn allowing_runs_the_stored_call_once_as_the_owner_and_records_it() {
    let _serial = serial();
    let tree = Tree::new();
    let id = ask(
        &tree.me(),
        MethodName::SecretsSave,
        json!({"request": {"operation": "add", "name": "deploy", "value": "s3cr3t",
            "computerIds": [tree.fork], "allowedDomains": ["deploy.example.com"]}}),
    )
    .await;
    let shown = get(&owner(), &id).await.unwrap();
    assert!(!shown.to_string().contains("s3cr3t"));
    let (run, runs) = runner(|_, _| Ok(json!([])));
    let allowed = TEST_RUN
        .scope(run.clone(), decide(&id, "allow"))
        .await
        .unwrap();
    assert_eq!(allowed["state"], "allowed");
    // The outcome holds what concerns the call, never the list of secrets the engine returned.
    assert_eq!(allowed["outcome"]["result"], json!({"name": "deploy"}));
    assert_eq!(allowed["decidedBy"], "app");
    {
        let runs = runs.lock().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].0, MethodName::SecretsSave);
        assert_eq!(runs[0].1["request"]["value"], "s3cr3t");
        assert_eq!(runs[0].1["request"]["computerIds"], json!([tree.fork]));
    }
    for again in ["allow", "deny"] {
        let error = TEST_RUN
            .scope(run.clone(), decide(&id, again))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Changed);
    }
    assert_eq!(runs.lock().unwrap().len(), 1);
    assert!(!notice_keys().contains(&format!("approval:{id}")));
    let rows = AuditLog::shared(&app_data())
        .list(
            &AuditListParams {
                computer_id: None,
                caller_kind: None,
                before_id: None,
                limit: Some(500),
            },
            Scope::All,
        )
        .unwrap()
        .entries;
    let ran: Vec<_> = rows
        .iter()
        .filter(|row| {
            row.approval_id.as_deref() == Some(id.as_str())
                && row.method == "secrets.save"
                && row.caller_kind == silo_api::methods::audit::AuditCallerKind::Owner
        })
        .collect();
    assert_eq!(ran.len(), 1, "{rows:?}");
    assert_eq!(ran[0].outcome, AuditOutcome::Ok);
}

#[tokio::test]
async fn a_denied_call_never_runs_and_the_agent_reads_the_note() {
    let tree = Tree::new();
    let id = ask(
        &tree.me(),
        MethodName::SshAccessSave,
        json!({"computerId": tree.me, "enabled": true, "port": 2222, "bindAddress": "192.168.1.20"}),
    )
    .await;
    let (run, runs) = job_runner();
    let denied = TEST_RUN
        .scope(
            run,
            call(
                &owner(),
                MethodName::ApprovalsDecide,
                json!({"id": id, "decision": "deny", "note": "Not from outside."}),
            ),
        )
        .await
        .unwrap();
    assert_eq!(denied["state"], "denied");
    assert!(runs.lock().unwrap().is_empty());
    let read = get(&tree.me(), &id).await.unwrap();
    assert_eq!(read["state"], "denied");
    assert_eq!(read["decisionNote"], "Not from outside.");
    assert!(read.get("outcome").is_none());
}

fn push_params(computer: &str) -> Value {
    json!({"computerId": computer, "repositoryPath": "/workspace/app",
        "operationId": "0b9f6f57-6d4b-4c8e-8a39-2a3cf8a5c0d1",
        "target": {"repository": "acme/app", "branch": "main",
            "commit": "0123456789abcdef0123456789abcdef01234567"}})
}

/// A device with plenty of memory and disk, so the second check of an allowed call that takes
/// memory or disk does not depend on the machine the test runs on.
fn ample() -> crate::methods::gate::FixedDevice {
    use silo_engine::resources::{Host, GIB};
    crate::methods::gate::fixed_device(Host {
        logical_cpus: 8,
        memory_total: 64 * GIB,
        memory_available: Some(48 * GIB),
        pressure: None,
        disk_free: 800 * GIB,
        disk_total: 900 * GIB,
    })
}

/// Each kind of call the policy holds, as `tree`'s computer asks it.
fn every_kind(tree: &Tree) -> Vec<(ApprovalKind, MethodName, Value)> {
    let unique = &tree.me[4..12];
    vec![
        (
            ApprovalKind::Port,
            MethodName::NetworkPublish,
            json!({"computerId": tree.fork, "port": 3000}),
        ),
        (
            ApprovalKind::Ssh,
            MethodName::SshAccessSave,
            json!({"computerId": tree.fork, "enabled": true, "port": 2201,
                "bindAddress": "127.0.0.1"}),
        ),
        (
            ApprovalKind::Github,
            MethodName::PushStart,
            push_params(&tree.fork),
        ),
        (
            ApprovalKind::Secret,
            MethodName::SecretsSave,
            json!({"request": {"operation": "add", "name": format!("KEY_{}", unique.replace('-', "_")),
                "value": "v", "computerIds": [tree.fork], "allowedDomains": ["a.example.com"]}}),
        ),
        (
            ApprovalKind::ReplaceMain,
            MethodName::ChangesPromote,
            json!({"forkId": tree.fork}),
        ),
        (
            ApprovalKind::Merge,
            MethodName::ChangesUndo,
            json!({"computerId": tree.me}),
        ),
        (
            ApprovalKind::Access,
            MethodName::CheckpointsFork,
            json!({"computerId": tree.me, "newName": format!("try-{unique}"),
                "secrets": [format!("absent-{unique}")]}),
        ),
        (
            ApprovalKind::Container,
            MethodName::ComputersCreate,
            json!({"kind": "linux", "configuration": {"name": format!("box-{unique}"),
                "cpus": 2, "maxCPUs": 2, "memoryGiB": 4, "maxMemoryGiB": 4, "workspaceStorageGiB": 10, "runtimeStorageGiB": 10, "runtime": "container"}}),
        ),
    ]
}

#[tokio::test]
async fn every_kind_runs_on_allow_never_on_deny_and_not_when_the_second_check_fails() {
    let _serial = serial();
    let _host = ample();
    let kinds = every_kind(&Tree::new()).len();
    for index in 0..kinds {
        let tree = Tree::new();
        let (kind, method, params) = every_kind(&tree).remove(index);
        let (run, runs) = job_runner();
        // Allowed: the stored call runs once, with what the person saw.
        let id = ask(&tree.me(), method, params.clone()).await;
        let stored = get(&owner(), &id).await.unwrap();
        assert_eq!(
            stored["kind"],
            serde_json::to_value(kind).unwrap(),
            "{method:?}"
        );
        let allowed = TEST_RUN
            .scope(run.clone(), decide(&id, "allow"))
            .await
            .unwrap();
        assert_eq!(allowed["state"], "allowed");
        assert!(
            allowed["outcome"].get("error").is_none(),
            "{method:?}: {allowed}"
        );
        {
            let runs = runs.lock().unwrap();
            assert_eq!(runs.len(), 1, "{method:?}");
            assert_eq!(runs[0].0, method);
            if method == MethodName::SecretsSave {
                assert_eq!(runs[0].1["request"]["value"], "v");
            } else {
                assert_eq!(runs[0].1, stored["params"], "{method:?}");
            }
        }
        // Denied: nothing runs.
        let created = |value: &Value| value.pointer("/configuration/name").cloned();
        let again = match created(&params) {
            // A computer of that name is being made, so ask under another name.
            Some(name) => {
                let mut other = params.clone();
                other["configuration"]["name"] = json!(format!("{}-2", name.as_str().unwrap()));
                other
            }
            None if method == MethodName::CheckpointsFork => {
                let mut other = params.clone();
                other["newName"] = json!(format!("{}-2", params["newName"].as_str().unwrap()));
                other
            }
            None => params.clone(),
        };
        let id = ask(&tree.me(), method, again.clone()).await;
        TEST_RUN
            .scope(run.clone(), decide(&id, "deny"))
            .await
            .unwrap();
        assert_eq!(runs.lock().unwrap().len(), 1, "{method:?}");
        // The computer that asked is gone by the time of the answer: nothing runs.
        let id = ask(&tree.me(), method, again).await;
        test_nodes::remove(&tree.me);
        let ended = TEST_RUN
            .scope(run.clone(), decide(&id, "allow"))
            .await
            .unwrap();
        assert_eq!(ended["state"], "allowed");
        let error = ended["outcome"]["error"].as_str().unwrap();
        assert!(error.contains("no longer exists"), "{method:?}: {error}");
        assert_eq!(runs.lock().unwrap().len(), 1, "{method:?}");
    }
}

#[tokio::test]
async fn the_second_check_refuses_a_target_no_longer_in_the_askers_tree() {
    let _serial = serial();
    let tree = Tree::new();
    let id = ask(
        &tree.me(),
        MethodName::NetworkPublish,
        json!({"computerId": tree.fork, "port": 4000}),
    )
    .await;
    // The fork stops being one `me` made (a replace main moves the record this way).
    PolicyStore::new(&app_data())
        .remove_computers(&[tree.fork.as_str()])
        .unwrap();
    let (run, runs) = job_runner();
    let ended = TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
    let error = ended["outcome"]["error"].as_str().unwrap();
    assert!(error.starts_with("Nothing was run:"), "{error}");
    assert!(error.contains("not this one or one it made"), "{error}");
    assert!(runs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_call_that_would_now_ask_for_something_else_is_not_run() {
    let _serial = serial();
    let tree = Tree::new();
    let id = ask(
        &tree.me(),
        MethodName::NetworkPublish,
        json!({"computerId": tree.fork, "port": 4100}),
    )
    .await;
    // The fork was renamed: the summary the engine would write now differs from the one shown.
    test_nodes::remove(&tree.fork);
    test_nodes::add(&tree.fork, "renamed", Some(&tree.me));
    let (run, runs) = job_runner();
    let ended = TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
    let error = ended["outcome"]["error"].as_str().unwrap();
    assert!(
        error.contains("something other than what you allowed"),
        "{error}"
    );
    assert!(runs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_allowed_call_runs_knowing_its_result_goes_to_a_computer() {
    let _serial = serial();
    let tree = Tree::new();
    let id = ask(&tree.me(), MethodName::PushStart, push_params(&tree.fork)).await;
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let noted = seen.clone();
    let (run, _runs) = runner(move |_, _| {
        noted
            .lock()
            .unwrap()
            .push(FOR_COMPUTER.try_with(|asked| *asked).unwrap_or(false));
        Ok(json!({"jobId": "job:765433"}))
    });
    TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
    assert_eq!(*seen.lock().unwrap(), [true]);
}

#[tokio::test]
async fn only_a_computer_caller_or_an_allowed_call_for_one_hides_recipe_commands() {
    let tree = Tree::new();
    let owner = context(owner());
    let computer = context(tree.me());
    assert!(!result_goes_to_computer(&owner));
    assert!(
        FOR_COMPUTER
            .scope(true, async { result_goes_to_computer(&owner) })
            .await
    );
    assert!(result_goes_to_computer(&computer));
}

#[tokio::test]
async fn the_job_an_allowed_call_starts_is_the_askers_to_read() {
    let _serial = serial();
    let tree = Tree::new();
    let id = ask(&tree.me(), MethodName::PushStart, push_params(&tree.fork)).await;
    let (run, _runs) = runner(|_, _| Ok(json!({"jobId": "job:765432"})));
    let allowed = TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
    assert_eq!(allowed["outcome"]["jobId"], "job:765432");
    let snapshot = PolicyStore::new(&app_data()).snapshot().unwrap();
    assert!(snapshot.owns_job(&tree.me, "job:765432"));
    // The policy lets it read the job; the job itself is not in this test's registry.
    let read = call(
        &tree.me(),
        MethodName::OperationsJobGet,
        json!({"jobId": "job:765432"}),
    )
    .await;
    assert!(read
        .err()
        .is_none_or(|error| error.code != ErrorCode::PermissionDenied));
    assert_eq!(
        get(&tree.me(), &id).await.unwrap()["outcome"]["jobId"],
        "job:765432"
    );
}

#[tokio::test]
async fn a_computer_an_allowed_call_makes_is_the_askers() {
    let _serial = serial();
    let _host = ample();
    let tree = Tree::new();
    let unique = &tree.me[4..12];
    let name = format!("wide-{unique}");
    let id = ask(
        &tree.me(),
        MethodName::CheckpointsFork,
        json!({"computerId": tree.me, "newName": name, "secrets": ["not-held"]}),
    )
    .await;
    let (run, runs) = runner(|_, _| Ok(json!({"jobId": "job:765433"})));
    TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
    // The fork's token file is scrubbed in the call that ran.
    assert!(runs.lock().unwrap()[0].1["quiet"]["scrub"]
        .as_array()
        .unwrap()
        .contains(&json!(silo_api::methods::guest_client::GUEST_ENDPOINT_PATH)));
    let snapshot = PolicyStore::new(&app_data()).snapshot().unwrap();
    let pending = snapshot
        .pending
        .iter()
        .find(|pending| pending.name == name)
        .unwrap();
    assert_eq!(pending.created_by, tree.me);
    assert_eq!(pending.source.as_deref(), Some(tree.me.as_str()));
    assert_eq!(pending.job_id.as_deref(), Some("job:765433"));
    assert!(snapshot.owns_job(&tree.me, "job:765433"));

    let box_name = format!("box-{unique}");
    let id = ask(
        &tree.me(),
        MethodName::ComputersCreate,
        json!({"kind": "linux", "configuration": {"name": box_name, "cpus": 2, "maxCPUs": 2, "memoryGiB": 4, "maxMemoryGiB": 4, "workspaceStorageGiB": 10, "runtimeStorageGiB": 10, "runtime": "container"}}),
    )
    .await;
    let stored = get(&owner(), &id).await.unwrap();
    let new_id = stored["params"]["configuration"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let made = new_id.clone();
    let (run, _) = runner(move |_, _| Ok(json!({"computer": {"id": made}})));
    TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
    let snapshot = PolicyStore::new(&app_data()).snapshot().unwrap();
    assert!(snapshot.made_by(&tree.me).contains(&new_id));
}

#[tokio::test]
async fn an_allow_the_engine_stopped_during_is_not_run_again() {
    let _serial = serial();
    let tree = Tree::new();
    let id = ask(
        &tree.me(),
        MethodName::NetworkPublish,
        json!({"computerId": tree.fork, "port": 5000}),
    )
    .await;
    // The engine recorded the allow and stopped before the call's outcome was stored.
    silo_engine::approvals::Store::new(&app_data())
        .allow(&id, "app", None)
        .unwrap();
    super::recover(fixture().context.as_ref());
    let (run, runs) = job_runner();
    let read = get(&tree.me(), &id).await.unwrap();
    assert_eq!(read["state"], "allowed");
    assert!(read["outcome"]["error"]
        .as_str()
        .unwrap()
        .contains("not run again"));
    let error = TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::Changed);
    assert!(runs.lock().unwrap().is_empty());
}

fn review_request(tree: &Tree, digest: &str) -> String {
    fold_requests::Store::new(&app_data())
        .create(fold_requests::NewRequest {
            fork_id: tree.fork.clone(),
            target: tree.me.clone(),
            review_digest: digest.into(),
            summary: "2 files".into(),
            counts: Default::default(),
        })
        .unwrap()
        .id
}

#[tokio::test]
async fn a_merge_is_linked_to_its_review_request_both_ways_and_keeps_it_on_allow() {
    let _serial = serial();
    let tree = Tree::new();
    let request = review_request(&tree, "d1");
    let id = ask(
        &tree.me(),
        MethodName::ChangesRequestsKeep,
        json!({"requestId": request, "choices": {"file:/etc/x": "main"}, "skip": ["cron:root"]}),
    )
    .await;
    let approval = get(&tree.me(), &id).await.unwrap();
    assert_eq!(approval["kind"], "merge");
    assert_eq!(approval["reviewRequestId"], request);
    assert_eq!(approval["reviewDigest"], "d1");
    let linked = fold_requests::Store::new(&app_data())
        .get(&request)
        .unwrap()
        .unwrap();
    assert_eq!(linked.approval_id.as_deref(), Some(id.as_str()));
    let (run, runs) = runner(|_, _| Ok(json!({"jobId": "job:765434"})));
    let allowed = TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
    assert_eq!(allowed["outcome"]["jobId"], "job:765434");
    let runs = runs.lock().unwrap();
    assert_eq!(runs[0].0, MethodName::ChangesRequestsKeep);
    assert_eq!(
        runs[0].1,
        json!({"requestId": request, "choices": {"file:/etc/x": "main"}, "skip": ["cron:root"]})
    );
}

#[tokio::test]
async fn a_merge_into_a_computer_outside_the_askers_tree_is_refused() {
    let tree = Tree::new();
    // The person asked for a review of the fork into a computer the agent did not make.
    let request = fold_requests::Store::new(&app_data())
        .create(fold_requests::NewRequest {
            fork_id: tree.fork.clone(),
            target: "someone-elses-main".into(),
            review_digest: "d1".into(),
            summary: "2 files".into(),
            counts: Default::default(),
        })
        .unwrap()
        .id;
    let error = call(
        &tree.me(),
        MethodName::ChangesRequestsKeep,
        json!({"requestId": request}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert!(
        error.message.contains("only into this computer"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn a_merge_whose_changes_were_asked_for_again_is_not_kept() {
    let _serial = serial();
    let tree = Tree::new();
    let request = review_request(&tree, "d1");
    let id = ask(
        &tree.me(),
        MethodName::ChangesRequestsKeep,
        json!({"requestId": request}),
    )
    .await;
    // The agent asked for the review again with other changes.
    assert_eq!(review_request(&tree, "d2"), request);
    let (run, runs) = job_runner();
    let ended = TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
    let error = ended["outcome"]["error"].as_str().unwrap();
    assert!(error.contains("asked for again"), "{error}");
    assert!(runs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_merge_asked_again_with_other_changes_replaces_the_earlier_ask_even_at_the_limit() {
    let tree = Tree::new();
    let request = review_request(&tree, "d1");
    let first = ask(
        &tree.me(),
        MethodName::ChangesRequestsKeep,
        json!({"requestId": request}),
    )
    .await;
    for port in 0..19 {
        ask(
            &tree.me(),
            MethodName::NetworkPublish,
            json!({"computerId": tree.fork, "port": 8200 + port}),
        )
        .await;
    }
    // The agent asked for the review again with other changes, then for the merge again.
    review_request(&tree, "d2");
    let second = ask(
        &tree.me(),
        MethodName::ChangesRequestsKeep,
        json!({"requestId": request}),
    )
    .await;
    assert_ne!(second, first);
    assert_eq!(get(&owner(), &first).await.unwrap()["state"], "cancelled");
    assert_eq!(get(&owner(), &second).await.unwrap()["reviewDigest"], "d2");
}

#[tokio::test]
async fn keeping_or_throwing_away_the_review_request_in_the_app_closes_the_approval() {
    let tree = Tree::new();
    let request = review_request(&tree, "d1");
    let kept = ask(
        &tree.me(),
        MethodName::ChangesRequestsKeep,
        json!({"requestId": request}),
    )
    .await;
    request_answered(&context(owner()), &request, Some("job:765435"));
    let read = get(&tree.me(), &kept).await.unwrap();
    assert_eq!(read["state"], "allowed");
    assert_eq!(read["outcome"]["jobId"], "job:765435");
    assert!(PolicyStore::new(&app_data())
        .snapshot()
        .unwrap()
        .owns_job(&tree.me, "job:765435"));
    let other = Tree::new();
    let request = review_request(&other, "d1");
    let thrown = ask(
        &other.me(),
        MethodName::ChangesRequestsKeep,
        json!({"requestId": request}),
    )
    .await;
    request_answered(&context(owner()), &request, None);
    assert_eq!(get(&other.me(), &thrown).await.unwrap()["state"], "denied");
    // A request closed some other way ends its approval when it is next read.
    let third = Tree::new();
    let request = review_request(&third, "d1");
    let closed = ask(
        &third.me(),
        MethodName::ChangesRequestsKeep,
        json!({"requestId": request}),
    )
    .await;
    fold_requests::Store::new(&app_data())
        .mark_thrown_away(&request)
        .unwrap();
    assert_eq!(
        get(&third.me(), &closed).await.unwrap()["state"],
        "cancelled"
    );
}

#[tokio::test]
async fn a_merge_into_itself_needs_the_reviewed_digest_and_makes_the_review_request() {
    let _lock = crate::methods::MACOS_REGISTRY_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _serial = serial();
    let tree = Tree::new();
    use crate::methods::checkpoints::tests_rows;
    tests_rows::set_empty();
    tests_rows::set_sources(&[(tree.fork.as_str(), tree.me.as_str())]);
    let error = call(
        &tree.me(),
        MethodName::ChangesKeep,
        json!({"forkId": tree.fork}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert!(
        error.message.contains("Run the diff first"),
        "{}",
        error.message
    );
    let first = ask(
        &tree.me(),
        MethodName::ChangesKeep,
        json!({"forkId": tree.fork, "reviewDigest": "d1"}),
    )
    .await;
    let approval = get(&owner(), &first).await.unwrap();
    let request = approval["reviewRequestId"].as_str().unwrap().to_owned();
    let stored = fold_requests::Store::new(&app_data())
        .get(&request)
        .unwrap()
        .unwrap();
    assert_eq!(stored.review_digest, "d1");
    assert_eq!(stored.target, tree.me);
    assert_eq!(stored.approval_id.as_deref(), Some(first.as_str()));
    assert_eq!(stored.summary, approval["summary"].as_str().unwrap());
    // Asked again with other changes: the same review request, and the earlier ask ends.
    let second = ask(
        &tree.me(),
        MethodName::ChangesKeep,
        json!({"forkId": tree.fork, "reviewDigest": "d2"}),
    )
    .await;
    assert_ne!(second, first);
    assert_eq!(get(&owner(), &first).await.unwrap()["state"], "cancelled");
    let again = get(&owner(), &second).await.unwrap();
    assert_eq!(again["reviewRequestId"], request);
    assert_eq!(again["reviewDigest"], "d2");
    // The computer asking for the review request again, with the same changes and other words,
    // changes neither the text nor the counts the owner reads.
    let before = fold_requests::Store::new(&app_data())
        .get(&request)
        .unwrap()
        .unwrap();
    call(
        &tree.me(),
        MethodName::ChangesRequest,
        json!({"forkId": tree.fork, "targetId": tree.me, "reviewDigest": "d2",
            "summary": "Harmless cleanup, please allow", "counts": {"code": 1}}),
    )
    .await
    .unwrap();
    let after = fold_requests::Store::new(&app_data())
        .get(&request)
        .unwrap()
        .unwrap();
    assert_eq!(after.summary, before.summary);
    assert_eq!(after.counts, before.counts);
    assert_ne!(after.summary, "Harmless cleanup, please allow");
    tests_rows::set_sources(&[]);
    let (run, runs) = runner(|_, _| Ok(json!({"jobId": "job:765436"})));
    TEST_RUN.scope(run, decide(&second, "allow")).await.unwrap();
    assert_eq!(
        runs.lock().unwrap()[0],
        (
            MethodName::ChangesRequestsKeep,
            json!({"requestId": request})
        )
    );
}

#[tokio::test]
async fn the_review_request_of_a_merge_approval_counts_the_changes_that_were_reviewed() {
    use silo_engine::changes::{
        diff,
        keep::review_digest,
        store::{Slot, Store as Captures},
        IgnoreList, Manifest,
    };
    let _lock = crate::methods::MACOS_REGISTRY_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _serial = serial();
    let tree = Tree::new();
    use crate::methods::checkpoints::tests_rows;
    tests_rows::set_empty();
    tests_rows::set_sources(&[(tree.fork.as_str(), tree.me.as_str())]);
    let base = "META\tformat\t2\nF\t/srv/data/a\t1\t1\t-\n";
    let now = "META\tformat\t2\nF\t/srv/data/a\t2\t2\t-\nF\t/srv/data/c\t1\t1\t-\n";
    let captures = Captures::for_engine(fixture().context.as_ref());
    captures.put(&tree.fork, Slot::Base, base, None).unwrap();
    captures.put(&tree.fork, Slot::Latest, now, None).unwrap();
    let digest = review_digest(&diff(
        &Manifest::parse(base).unwrap(),
        &Manifest::parse(now).unwrap(),
        None,
        &IgnoreList::builtin(),
    ));
    let asked = ask(
        &tree.me(),
        MethodName::ChangesKeep,
        json!({"forkId": tree.fork, "reviewDigest": digest}),
    )
    .await;
    let approval = get(&owner(), &asked).await.unwrap();
    let request = fold_requests::Store::new(&app_data())
        .get(approval["reviewRequestId"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(request.counts["other"], 2);
    tests_rows::set_sources(&[]);
}

#[tokio::test]
async fn merging_from_the_review_page_applies_the_choices_the_ask_recorded() {
    use silo_api::methods::keep::KeepAnswer;
    let _lock = crate::methods::MACOS_REGISTRY_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _serial = serial();
    let tree = Tree::new();
    use crate::methods::checkpoints::tests_rows;
    tests_rows::set_empty();
    tests_rows::set_sources(&[(tree.fork.as_str(), tree.me.as_str())]);
    let asked = ask(
        &tree.me(),
        MethodName::ChangesKeep,
        json!({"forkId": tree.fork, "reviewDigest": "d1",
            "choices": {"file:/etc/silo/endpoint.json": "fork"}, "skip": ["cron:root"]}),
    )
    .await;
    let approval = get(&owner(), &asked).await.unwrap();
    let request = fold_requests::Store::new(&app_data())
        .get(approval["reviewRequestId"].as_str().unwrap())
        .unwrap()
        .unwrap();
    let (choices, skip) = crate::methods::approvals::recorded_answers(&context(owner()), &request);
    assert_eq!(choices["file:/etc/silo/endpoint.json"], KeepAnswer::Fork);
    assert_eq!(skip, ["cron:root"]);
    // The computer itself is never handed them back as if it were the owner.
    let (none, _) = crate::methods::approvals::recorded_answers(&context(tree.me()), &request);
    assert!(none.is_empty());
    tests_rows::set_sources(&[]);
}

#[tokio::test]
async fn denying_a_merge_approval_closes_its_review_request() {
    let _lock = crate::methods::MACOS_REGISTRY_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _serial = serial();
    let tree = Tree::new();
    use crate::methods::checkpoints::tests_rows;
    tests_rows::set_empty();
    tests_rows::set_sources(&[(tree.fork.as_str(), tree.me.as_str())]);
    let ask_id = ask(
        &tree.me(),
        MethodName::ChangesKeep,
        json!({"forkId": tree.fork, "reviewDigest": "d1"}),
    )
    .await;
    let approval = get(&owner(), &ask_id).await.unwrap();
    let request_id = approval["reviewRequestId"].as_str().unwrap().to_owned();
    let before = fold_requests::Store::new(&app_data())
        .get(&request_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        before.status,
        silo_api::methods::keep::FoldRequestStatus::Open
    );

    decide(&ask_id, "deny").await.unwrap();
    assert_eq!(get(&owner(), &ask_id).await.unwrap()["state"], "denied");

    let after = fold_requests::Store::new(&app_data())
        .get(&request_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        after.status,
        silo_api::methods::keep::FoldRequestStatus::ThrownAway
    );
    tests_rows::set_sources(&[]);
}

#[tokio::test]
async fn a_computer_holds_at_most_twenty_waiting_requests() {
    let tree = Tree::new();
    for port in 0..20 {
        ask(
            &tree.me(),
            MethodName::NetworkPublish,
            json!({"computerId": tree.fork, "port": 6000 + port}),
        )
        .await;
    }
    let error = call(
        &tree.me(),
        MethodName::NetworkPublish,
        json!({"computerId": tree.fork, "port": 7000}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert!(
        error.message.contains("20 requests waiting"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn deleting_a_computer_cancels_what_it_asked() {
    let tree = Tree::new();
    let id = ask(
        &tree.child(),
        MethodName::NetworkPublish,
        json!({"computerId": tree.fork, "port": 81}),
    )
    .await;
    silo_engine::policy::store::computer_removed(&tree.fork);
    let read = get(&owner(), &id).await.unwrap();
    assert_eq!(read["state"], "cancelled");
    assert!(!notice_keys().contains(&format!("approval:{id}")));
}

#[tokio::test]
async fn access_shows_what_a_computer_in_the_tree_holds_and_nothing_of_others() {
    let tree = Tree::new();
    let stranger = Tree::new();
    let held = call(
        &tree.me(),
        MethodName::ComputersAccess,
        json!({"computerId": tree.fork}),
    )
    .await
    .unwrap();
    assert_eq!(held["computerId"], tree.fork);
    assert_eq!(held["name"], format!("{}-name", tree.fork));
    assert!(held["secrets"].is_array() && held["ports"].is_array());
    let error = call(
        &tree.me(),
        MethodName::ComputersAccess,
        json!({"computerId": stranger.me}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    let owner_view = call(
        &owner(),
        MethodName::ComputersAccess,
        json!({"computerId": stranger.me}),
    )
    .await
    .unwrap();
    assert_eq!(owner_view["name"], format!("{}-name", stranger.me));
}

#[tokio::test]
async fn an_outcome_never_holds_what_the_call_returned_about_the_device() {
    let _serial = serial();
    let _host = ample();
    let leak = "LEAKED-DEVICE-STATE";
    for index in 0..every_kind(&Tree::new()).len() {
        let tree = Tree::new();
        let (_, method, params) = every_kind(&tree).remove(index);
        let id = ask(&tree.me(), method, params).await;
        // The engine answers with the whole device, as the real calls do.
        let (run, _runs) = runner(move |_, _| {
            Ok(
                json!({"computers": [{"computerId": "stranger", "computer": leak,
                    "ports": [{"port": 1}], "keys": [leak]}],
                "secrets": [{"name": leak}], "github": {"account": leak},
                "name": leak, "computer_extra": leak}),
            )
        });
        let allowed = TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
        let read = get(&tree.me(), &id).await.unwrap();
        for text in [allowed.to_string(), read.to_string()] {
            assert!(!text.contains(leak), "{method:?}: {text}");
        }
        if method == MethodName::SecretsSave {
            assert_eq!(
                read["outcome"]["result"]["name"],
                format!("KEY_{}", tree.me[4..12].replace('-', "_"))
            );
        }
        // An outcome is stored for every method, as a small object.
        assert!(read["outcome"]["result"].is_object(), "{method:?}: {read}");
    }
}

#[test]
fn a_network_policy_outcome_is_the_new_policy_and_when_it_applies() {
    let returned = json!({
        "computerId": "fork-1",
        "policy": {"mode": "allowlist", "hosts": ["a.example.com"], "presets": []},
        "allowedHosts": [{"host": "api.github.com", "ports": [443], "from": "github"}],
        "timing": "nextStart",
        "presets": [{"name": "npm", "title": "npm", "hosts": ["registry.npmjs.org"]}],
    });
    let outcome = super::outcome_result(
        MethodName::NetworkPolicySet,
        &json!({"computerId": "fork-1"}),
        &returned,
    );
    assert_eq!(
        outcome,
        json!({
            "computerId": "fork-1",
            "policy": returned["policy"],
            "timing": "nextStart",
        })
    );
}

#[tokio::test]
async fn a_computer_that_asks_too_often_is_refused_for_the_rest_of_the_hour() {
    let tree = Tree::new();
    for port in 0..approvals::ASKS_PER_HOUR {
        let id = ask(
            &tree.me(),
            MethodName::NetworkPublish,
            json!({"computerId": tree.fork, "port": 7000 + port}),
        )
        .await;
        // Withdrawing frees the waiting slot, not the hour's count.
        call(&tree.me(), MethodName::ApprovalsCancel, json!({"id": id}))
            .await
            .unwrap();
    }
    let error = call(
        &tree.me(),
        MethodName::NetworkPublish,
        json!({"computerId": tree.fork, "port": 8999}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert!(error.message.contains("last hour"), "{}", error.message);
    // Another computer is not held back by it.
    ask(
        &tree.child(),
        MethodName::NetworkPublish,
        json!({"computerId": tree.fork, "port": 8999}),
    )
    .await;
}

#[tokio::test]
async fn cancelling_clears_the_notice_without_a_new_notification() {
    let tree = Tree::new();
    let id = ask(
        &tree.me(),
        MethodName::NetworkPublish,
        json!({"computerId": tree.fork, "port": 6500}),
    )
    .await;
    assert!(notice_keys().contains(&format!("approval:{id}")));
    let key = format!("approval:{id}");
    let mut events = fixture()
        .context
        .subscribe(silo_engine::context::ClientId::new(format!("cancel-{id}")));
    call(&tree.me(), MethodName::ApprovalsCancel, json!({"id": id}))
        .await
        .unwrap();
    assert!(!notice_keys().contains(&key));
    let mut seen: Vec<String> = Vec::new();
    while let Ok(event) = events.try_recv() {
        if event.payload.get("key").and_then(Value::as_str) == Some(key.as_str()) {
            seen.push(event.name);
        }
    }
    assert_eq!(seen, ["notifications.cleared"], "{seen:?}");
}

#[tokio::test]
async fn an_allowed_call_that_takes_memory_or_disk_is_refused_when_the_device_has_become_short() {
    use silo_engine::resources::{FixedHost, Host, GIB};
    let _serial = serial();
    let tree = Tree::new();
    let container = json!({"kind": "linux", "configuration": {"name": format!("short-{}", &tree.me[4..12]),
        "cpus": 2, "maxCPUs": 2, "memoryGiB": 4, "maxMemoryGiB": 4,
        "workspaceStorageGiB": 10, "runtimeStorageGiB": 10, "runtime": "container"}});
    let id = {
        let _host = ample();
        ask(&tree.me(), MethodName::ComputersCreate, container).await
    };
    let _short = FixedHost::new(Host {
        logical_cpus: 8,
        memory_total: 32 * GIB,
        memory_available: Some(GIB),
        pressure: None,
        disk_free: 400 * GIB,
        disk_total: 500 * GIB,
    });
    let (run, runs) = job_runner();
    let ended = TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
    let error = ended["outcome"]["error"].as_str().unwrap();
    assert!(
        error.starts_with("Nothing was run:") && error.contains("Silo keeps"),
        "{error}"
    );
    assert!(runs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_allowed_call_holds_its_memory_while_it_runs() {
    use silo_engine::resources::GIB;
    let _serial = serial();
    let tree = Tree::new();
    let container = json!({"kind": "linux", "configuration": {"name": format!("held-{}", &tree.me[4..12]),
        "cpus": 2, "maxCPUs": 2, "memoryGiB": 4, "maxMemoryGiB": 4,
        "workspaceStorageGiB": 10, "runtimeStorageGiB": 10, "runtime": "container"}});
    let _host = ample();
    let id = ask(&tree.me(), MethodName::ComputersCreate, container).await;
    let seen: Arc<Mutex<Vec<u64>>> = Arc::default();
    let (run, _runs) = {
        let seen = seen.clone();
        runner(move |_, _| {
            seen.lock()
                .unwrap()
                .push(crate::methods::gate::reserved_memory());
            Ok(json!({"computer": {"id": uuid::Uuid::new_v4().to_string()}}))
        })
    };
    TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
    let seen = seen.lock().unwrap().clone();
    assert!(seen.iter().any(|reserved| *reserved >= 4 * GIB), "{seen:?}");
}

#[tokio::test]
async fn a_promote_is_refused_when_the_fork_changed_after_it_was_asked_and_asking_again_makes_a_new_one(
) {
    let _serial = serial();
    let _host = ample();
    let tree = Tree::new();
    let params = json!({"forkId": tree.fork, "targetId": tree.me});
    let first = ask(&tree.me(), MethodName::ChangesPromote, params.clone()).await;
    // Asked again while nothing changed: the same approval.
    assert_eq!(
        ask(&tree.me(), MethodName::ChangesPromote, params.clone()).await,
        first
    );
    silo_engine::generation::bump(&tree.fork);
    let (run, runs) = job_runner();
    let ended = TEST_RUN
        .scope(run.clone(), decide(&first, "allow"))
        .await
        .unwrap();
    let error = ended["outcome"]["error"].as_str().unwrap();
    assert!(
        error.contains("fork changed") && error.contains("ask again"),
        "{error}"
    );
    assert!(runs.lock().unwrap().is_empty());
    // Asked again after a change, a waiting approval about the old state is replaced.
    let stale = ask(&tree.me(), MethodName::ChangesPromote, params.clone()).await;
    silo_engine::generation::bump(&tree.fork);
    let fresh = ask(&tree.me(), MethodName::ChangesPromote, params.clone()).await;
    assert_ne!(stale, fresh);
    assert_eq!(get(&owner(), &stale).await.unwrap()["state"], "cancelled");
    TEST_RUN.scope(run, decide(&fresh, "allow")).await.unwrap();
    assert_eq!(runs.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn an_allowed_call_carries_the_state_it_was_approved_for_into_its_job() {
    let _serial = serial();
    let _host = ample();
    let tree = Tree::new();
    let id = ask(
        &tree.me(),
        MethodName::ChangesPromote,
        json!({"forkId": tree.fork, "targetId": tree.me}),
    )
    .await;
    let pinned = get(&owner(), &id).await.unwrap();
    assert!(pinned["state"] == "pending");
    let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let record = seen.clone();
    let (run, _runs) = runner(move |_, _| {
        record.lock().unwrap().push(super::expected_pin());
        Ok(json!({"jobId": "job:700001"}))
    });
    TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
    let expected = silo_engine::changes::promote::service::promote_pin(&tree.fork);
    assert_eq!(*seen.lock().unwrap(), vec![Some(expected)]);
    assert_eq!(super::expected_pin(), None);
}

#[tokio::test]
async fn an_allowed_merge_carries_the_changes_it_was_approved_for_into_its_job() {
    let _serial = serial();
    let tree = Tree::new();
    let request = review_request(&tree, "d1");
    let id = ask(
        &tree.me(),
        MethodName::ChangesRequestsKeep,
        json!({"requestId": request}),
    )
    .await;
    let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let record = seen.clone();
    let (run, _runs) = runner(move |_, _| {
        record.lock().unwrap().push(super::expected_review_digest());
        Ok(json!({"jobId": "job:700002"}))
    });
    TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![Some("d1".to_owned())]);
    assert_eq!(super::expected_review_digest(), None);
}

#[tokio::test]
async fn a_merge_saved_before_computer_summaries_still_runs_and_is_worded_for_the_computer() {
    let _serial = serial();
    let tree = Tree::new();
    let request = review_request(&tree, "d1");
    let id = ask(
        &tree.me(),
        MethodName::ChangesRequestsKeep,
        json!({"requestId": request}),
    )
    .await;
    // The same ask as an earlier version of the engine saved it: no summary for the computer.
    let modern = get(&owner(), &id).await.unwrap();
    call(&tree.me(), MethodName::ApprovalsCancel, json!({"id": id}))
        .await
        .unwrap();
    let params = json!({"requestId": request});
    let (legacy, _) = approvals::Store::new(&app_data())
        .ask(approvals::NewApproval {
            kind: ApprovalKind::Merge,
            computer_id: tree.me.clone(),
            computer_name: format!("{}-name", tree.me),
            method: "changes.requests.keep".into(),
            params: params.clone(),
            asked: params,
            summary: modern["summary"].as_str().unwrap().to_owned(),
            computer_summary: None,
            creates: None,
            pin: None,
            review_request_id: Some(request),
            review_digest: Some("d1".into()),
        })
        .unwrap();
    let id = legacy.id().to_owned();
    let seen = get(&tree.me(), &id).await.unwrap();
    assert_eq!(seen["summary"], silo_engine::policy::WAITING_MERGE_SUMMARY);
    let (run, runs) = job_runner();
    let ended = TEST_RUN.scope(run, decide(&id, "allow")).await.unwrap();
    assert!(ended["outcome"]["error"].is_null(), "{ended}");
    assert_eq!(runs.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn an_approval_saved_without_a_network_policy_runs_with_the_one_the_second_check_chooses() {
    let _serial = serial();
    let _host = ample();
    let tree = Tree::new();
    let (_, method, params) = every_kind(&tree)
        .into_iter()
        .find(|(kind, ..)| *kind == ApprovalKind::Container)
        .unwrap();
    let id = ask(&tree.me(), method, params.clone()).await;
    let store = approvals::Store::new(&app_data());
    let modern = store.get(&id).unwrap().unwrap();
    let chosen = modern.approval.params["networkPolicy"].clone();
    assert!(chosen.is_object(), "{chosen}");
    call(&tree.me(), MethodName::ApprovalsCancel, json!({"id": id}))
        .await
        .unwrap();
    // The same ask as an earlier version of the engine saved it: no network policy in the call.
    let mut legacy_params = modern.approval.params.clone();
    legacy_params
        .as_object_mut()
        .unwrap()
        .remove("networkPolicy");
    let (legacy, _) = store
        .ask(approvals::NewApproval {
            kind: modern.approval.kind,
            computer_id: tree.me.clone(),
            computer_name: modern.approval.computer_name.clone(),
            method: modern.approval.method.clone(),
            params: legacy_params,
            asked: params,
            summary: modern.approval.summary.clone(),
            computer_summary: modern.computer_summary.clone(),
            creates: modern.creates.clone(),
            pin: None,
            review_request_id: None,
            review_digest: None,
        })
        .unwrap();
    let (run, runs) = job_runner();
    let ended = TEST_RUN
        .scope(run, decide(legacy.id(), "allow"))
        .await
        .unwrap();
    assert!(ended["outcome"]["error"].is_null(), "{ended}");
    let runs = runs.lock().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].1["networkPolicy"], chosen);
    // The computer it makes keeps the id the approval recorded for it.
    assert_eq!(
        runs[0].1["configuration"]["id"],
        modern.approval.params["configuration"]["id"]
    );
}

#[tokio::test]
async fn an_undo_is_refused_when_another_merge_happened_after_it_was_asked() {
    use silo_engine::changes::keep::{env::CheckpointRef, journal::State, service::journal_for};
    let _serial = serial();
    let _host = ample();
    let tree = Tree::new();
    let checkpoint = |id: &str| CheckpointRef {
        id: id.into(),
        name: id.into(),
        created_at: "2026-11-11T10:00:00Z".into(),
        scope: "disk".into(),
    };
    let journal = journal_for(fixture().context.as_ref());
    let merge = |id: &str| {
        journal.record_pending(&tree.me, &checkpoint(id)).unwrap();
        journal.set_state(&tree.me, id, State::Undoable).unwrap();
    };
    merge("cp-1");
    let params = json!({"computerId": tree.me});
    let first = ask(&tree.me(), MethodName::ChangesUndo, params.clone()).await;
    assert_eq!(
        ask(&tree.me(), MethodName::ChangesUndo, params.clone()).await,
        first
    );
    merge("cp-2");
    let (run, runs) = job_runner();
    let ended = TEST_RUN
        .scope(run.clone(), decide(&first, "allow"))
        .await
        .unwrap();
    let error = ended["outcome"]["error"].as_str().unwrap();
    assert!(
        error.contains("another merge") && error.contains("ask again"),
        "{error}"
    );
    assert!(runs.lock().unwrap().is_empty());
    let second = ask(&tree.me(), MethodName::ChangesUndo, params).await;
    assert_ne!(second, first);
    TEST_RUN.scope(run, decide(&second, "allow")).await.unwrap();
    assert_eq!(runs.lock().unwrap().len(), 1);
}

#[test]
fn the_second_check_runs_what_it_rewrites_except_for_a_merge_the_owner_was_shown() {
    use super::call_to_run;
    let stored = || Ok((MethodName::NetworkPublish, json!({"port": 1})));
    let rewritten = || Some(json!({"port": 2}));
    assert_eq!(
        call_to_run(stored(), rewritten(), MethodName::NetworkPublish, false),
        Ok((MethodName::NetworkPublish, json!({"port": 2})))
    );
    assert_eq!(
        call_to_run(stored(), None, MethodName::NetworkPublish, false),
        Ok((MethodName::NetworkPublish, json!({"port": 1})))
    );
    // A merge keeps the call built from its review request.
    assert_eq!(
        call_to_run(stored(), rewritten(), MethodName::ChangesRequestsKeep, true),
        Ok((MethodName::NetworkPublish, json!({"port": 1})))
    );
    assert_eq!(
        call_to_run(
            Err("gone".into()),
            rewritten(),
            MethodName::NetworkPublish,
            false
        ),
        Err("gone".to_owned())
    );
}
