use std::{future::Future, pin::Pin, sync::Mutex};

use serde_json::{json, Value};
use silo_api::{
    jobs::ErrorCode,
    methods::guest_client::{GuestClientPolicy, GuestRequestFrame, GuestResponseFrame},
    transport::{ApprovalKind, ApprovalRequiredDetails},
};
use silo_engine::{
    guest_client::{now_secs, TokenStore, MAX_JOBS},
    policy::store::PolicyStore,
};

use super::*;
use crate::dispatcher::DispatchError;

/// The engine's answers, with the caller each call was made as.
#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<(Option<String>, String, Value)>>,
}

impl Runner for Fake {
    fn run<'a>(
        &'a self,
        computer: Option<&'a str>,
        method: &'a str,
        params: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, DispatchError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((
                computer.map(str::to_owned),
                method.to_owned(),
                params,
            ));
            match method {
                "changes.keep" => Err(DispatchError {
                    code: ErrorCode::ApprovalRequired,
                    message: "Needs approval.".into(),
                    details: Some(ApprovalRequiredDetails {
                        kind: ApprovalKind::Merge,
                        summary: "Merge f1 into main.".into(),
                        approval_id: None,
                    }),
                }),
                "settings.read" => Err(DispatchError::new(
                    ErrorCode::PermissionDenied,
                    "Not for a computer.",
                )),
                "checkpoints.fork" => Ok(json!({"jobId": "job:fork"})),
                "computers.start" => Ok(json!({"jobId": "job:other"})),
                "operations.job.get" => Ok(json!({"state": "finished", "outcome": {"result": {}}})),
                _ => Ok(json!({})),
            }
        })
    }
}

/// `main` with the forks the person made (`fork-1`, then `grand` below it), a stranger, and
/// any further forks named in `extra` as (id, name, state).
fn snapshot_with(extra: &[(&str, &str, &str)]) -> Snapshot {
    let mut nodes = vec![
        json!({"id": "main", "name": "main", "parent": null, "state": "running"}),
        json!({"id": "fork-1", "name": "main-a1", "parent": "main", "state": "stopped"}),
        json!({"id": "grand", "name": "main-a2", "parent": "fork-1", "state": "stopped"}),
        json!({"id": "stranger", "name": "other", "parent": null, "state": "running"}),
    ];
    for (id, name, state) in extra {
        nodes.push(json!({"id": id, "name": name, "parent": "main", "state": state}));
    }
    Snapshot::from_values(
        &json!({"linux": {"computers": [{"configuration": {"id": "main"}}]}}),
        &json!({ "nodes": nodes }),
    )
}

fn frame(token: &str, method: &str, params: Value) -> GuestRequestFrame {
    GuestRequestFrame {
        v: 1,
        id: "0123456789abcdef0123456789abcdef".into(),
        token: token.into(),
        method: method.into(),
        params,
    }
}

struct Setup {
    _dir: tempfile::TempDir,
    store: TokenStore,
    policy: PolicyStore,
    token: String,
    fake: Fake,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let store = TokenStore::new(dir.path());
    let minted = store
        .mint(
            "main",
            GuestClientPolicy {
                fork_secrets: Vec::new(),
                agent: None,
            },
        )
        .unwrap();
    Setup {
        token: minted.token,
        store,
        policy: PolicyStore::new(dir.path()),
        _dir: dir,
        fake: Fake::default(),
    }
}

impl Setup {
    async fn ask_with(
        &self,
        snapshot: &Snapshot,
        computer: &str,
        frame: GuestRequestFrame,
    ) -> Option<GuestResponseFrame> {
        let env = Frames {
            runner: &self.fake,
            store: &self.store,
            policy: &self.policy,
            snapshot,
        };
        handle_frame(&env, computer, frame).await
    }

    async fn ask(
        &self,
        snapshot: &Snapshot,
        computer: &str,
        method: &str,
        params: Value,
    ) -> Option<GuestResponseFrame> {
        self.ask_with(snapshot, computer, frame(&self.token, method, params))
            .await
    }

    async fn ok(&self, snapshot: &Snapshot, method: &str, params: Value) -> bool {
        self.ask(snapshot, "main", method, params)
            .await
            .is_some_and(|reply| reply.ok)
    }

    fn made(&self, ids: &[&str]) {
        let forks: Vec<String> = ids.iter().map(|id| (*id).to_owned()).collect();
        self.policy.import_legacy("main", &forks, &[]).unwrap();
    }
}

#[test]
fn descendants_follow_forks_of_forks_and_never_include_the_root() {
    let found = snapshot_with(&[]).descendants_of("main");
    assert_eq!(found, ["fork-1", "grand"].map(String::from).into());
    assert!(snapshot_with(&[]).descendants_of("stranger").is_empty());
}

#[tokio::test]
async fn a_call_runs_as_the_computer_whose_folder_it_came_from() {
    let setup = setup();
    let snapshot = snapshot_with(&[]);
    let params = json!({"computerId": "main", "newName": "x", "recipeHash": "h"});
    assert!(
        setup
            .ok(&snapshot, "checkpoints.fork", params.clone())
            .await
    );
    let calls = setup.fake.calls.lock().unwrap();
    // The bridge passes the call on as it came; the policy judges and rewrites it there.
    assert_eq!(
        calls[0],
        (Some("main".into()), "checkpoints.fork".into(), params)
    );
}

#[tokio::test]
async fn a_refusal_or_a_held_call_comes_back_with_its_code_and_details() {
    let setup = setup();
    let snapshot = snapshot_with(&[]);
    let refused = setup
        .ask(&snapshot, "main", "settings.read", json!({}))
        .await
        .unwrap();
    let error = refused.error.unwrap();
    assert_eq!(error.code, "permission_denied");
    assert!(error.details.is_none());
    let held = setup
        .ask(&snapshot, "main", "changes.keep", json!({"forkId": "f1"}))
        .await
        .unwrap();
    let error = held.error.unwrap();
    assert_eq!(error.code, "approval_required");
    let details = error.details.unwrap();
    assert_eq!(details.kind, ApprovalKind::Merge);
    assert_eq!(details.summary, "Merge f1 into main.");
}

#[tokio::test]
async fn a_bad_or_foreign_token_gets_no_answer_and_a_revoked_one_stops() {
    let setup = setup();
    let snapshot = snapshot_with(&[]);
    // The token file copied into a fork: the fork's folder is served as the fork.
    assert!(setup
        .ask(&snapshot, "fork-1", "computers.list", json!({}))
        .await
        .is_none());
    let forged = frame("silo_gc_forged", "computers.list", json!({}));
    assert!(setup.ask_with(&snapshot, "main", forged).await.is_none());
    assert!(setup.ok(&snapshot, "computers.list", json!({})).await);
    setup.store.revoke(&["main"]).unwrap();
    assert!(setup
        .ask(&snapshot, "main", "computers.list", json!({}))
        .await
        .is_none());
    assert_eq!(setup.fake.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn no_count_of_made_or_running_computers_holds_a_start_or_a_fork_back() {
    let setup = setup();
    let ids: Vec<String> = (0..40).map(|number| format!("f{number}")).collect();
    let extra: Vec<(&str, &str, &str)> = ids
        .iter()
        .map(|id| (id.as_str(), id.as_str(), "running"))
        .collect();
    let snapshot = snapshot_with(&extra);
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    setup.made(&refs);
    for id in &ids {
        assert!(
            setup
                .ok(&snapshot, "computers.start", json!({"computerId": id}))
                .await,
            "{id}"
        );
    }
    assert!(
        setup
            .ok(
                &snapshot,
                "checkpoints.fork",
                json!({"computerId": "main", "newName": "x"})
            )
            .await
    );
}

#[tokio::test]
async fn jobs_are_capped_and_old_ones_are_reaped() {
    let setup = setup();
    let snapshot = snapshot_with(&[]);
    let now = now_secs();
    let jobs: Vec<(String, u64)> = (0..MAX_JOBS)
        .map(|number| (format!("job:{number}"), now - 4000))
        .collect();
    setup.policy.import_legacy("main", &[], &jobs).unwrap();
    assert!(
        !setup
            .ok(
                &snapshot,
                "checkpoints.fork",
                json!({"computerId": "main", "newName": "x"})
            )
            .await
    );
    // A call that starts no job is not held back.
    assert!(setup.ok(&snapshot, "computers.list", json!({})).await);
    reap(&setup.fake, &setup.policy, "main").await;
    assert!(setup.policy.snapshot().unwrap().jobs_of("main").is_empty());
    let calls = setup.fake.calls.lock().unwrap();
    // The bridge's own housekeeping runs as the engine, not as the computer.
    assert!(calls
        .iter()
        .filter(|call| call.1 == "operations.job.acknowledge")
        .all(|call| call.0.is_none()));
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.1 == "operations.job.acknowledge")
            .count(),
        MAX_JOBS
    );
}
