use std::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    sync::Mutex,
};

use serde_json::{json, Value};
use silo_api::methods::guest_client::{GuestClientPolicy, GuestRequestFrame, GuestResponseFrame};
use silo_engine::guest_client::{now_secs, OwnedJob, TokenStore, MAX_JOBS};

use super::*;
use crate::dispatcher::DispatchError;

#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<(String, Value)>>,
    job_fails: AtomicBool,
}

impl Runner for Fake {
    fn run<'a>(
        &'a self,
        method: &'a str,
        params: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, DispatchError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((method.to_owned(), params));
            Ok(match method {
                "checkpoints.fork" => json!({"jobId": "job:fork"}),
                "computers.start" | "changes.diff" => json!({"jobId": "job:other"}),
                "operations.job.get" if self.job_fails.load(Ordering::Relaxed) => {
                    json!({"state": "finished", "outcome": {"error": {"message": "no"}}})
                }
                "operations.job.get" => json!({"state": "finished", "outcome": {"result": {}}}),
                "computers.list" => json!({
                    "linux": {"computers": [
                        {"configuration": {"id": "main"}},
                        {"configuration": {"id": "stranger"}}]},
                    "macos": {"computers": []}
                }),
                _ => json!({}),
            })
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
    token: String,
    fake: Fake,
    reserved: Reserved,
}

fn setup(fork_secrets: &[&str]) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let store = TokenStore::new(dir.path());
    let minted = store
        .mint(
            "main",
            GuestClientPolicy {
                fork_secrets: fork_secrets.iter().map(|name| (*name).to_owned()).collect(),
                agent: None,
            },
        )
        .unwrap();
    Setup {
        token: minted.token,
        store,
        _dir: dir,
        fake: Fake::default(),
        reserved: Reserved::default(),
    }
}

impl Setup {
    async fn ask(
        &self,
        snapshot: &Snapshot,
        computer: &str,
        method: &str,
        params: Value,
    ) -> Option<GuestResponseFrame> {
        let requests = |_: &str| Some("fork-1".to_owned());
        let env = Frames {
            runner: &self.fake,
            store: &self.store,
            snapshot,
            requests: &requests,
            reserved: &self.reserved,
        };
        handle_frame(&env, computer, frame(&self.token, method, params)).await
    }

    async fn ok(&self, snapshot: &Snapshot, method: &str, params: Value) -> bool {
        self.ask(snapshot, "main", method, params)
            .await
            .is_some_and(|reply| reply.ok)
    }

    fn fork_ids(&self) -> Vec<String> {
        self.store.status("main").unwrap().unwrap().fork_ids
    }
}

fn fork(name: &str) -> Value {
    json!({"computerId": "main", "newName": name})
}

#[test]
fn descendants_follow_forks_of_forks_and_never_include_the_root() {
    let found = snapshot_with(&[]).descendants_of("main");
    assert_eq!(found, ["fork-1", "grand"].map(String::from).into());
    assert!(snapshot_with(&[]).descendants_of("stranger").is_empty());
}

#[tokio::test]
async fn a_fork_is_the_tokens_only_once_its_job_finished_and_the_fork_exists() {
    let setup = setup(&[]);
    let before = snapshot_with(&[]);
    assert!(setup.ok(&before, "checkpoints.fork", fork("made")).await);
    let call = setup.fake.calls.lock().unwrap()[0].clone();
    assert_eq!(
        call.1["quiet"]["scrub"],
        json!(["/etc/silo/guest-client.json"])
    );
    let record = setup.store.status("main").unwrap().unwrap();
    assert_eq!(record.pending.len(), 1);
    assert_eq!(record.pending[0].job_id.as_deref(), Some("job:fork"));
    assert!(record.fork_ids.is_empty() && record.owns_job("job:fork"));
    let after = snapshot_with(&[("new-id", "made", "stopped")]);
    assert!(setup.ok(&after, "computers.list", json!({})).await);
    assert_eq!(setup.fork_ids(), ["new-id"]);
    assert!(setup
        .store
        .status("main")
        .unwrap()
        .unwrap()
        .pending
        .is_empty());
    assert!(
        setup
            .ok(&after, "computers.start", json!({"computerId": "new-id"}))
            .await
    );
    // The forks the person made are not the agent's.
    for person_made in ["fork-1", "grand"] {
        let params = json!({"computerId": person_made});
        assert!(
            !setup.ok(&after, "computers.start", params.clone()).await,
            "{person_made}"
        );
        assert!(
            !setup.ok(&after, "changes.diff", params).await,
            "{person_made}"
        );
    }
}

#[tokio::test]
async fn a_failed_fork_is_dropped_and_never_claims_a_computer_of_that_name() {
    let setup = setup(&[]);
    assert!(
        setup
            .ok(&snapshot_with(&[]), "checkpoints.fork", fork("made"))
            .await
    );
    setup.fake.job_fails.store(true, Ordering::Relaxed);
    // The name now belongs to a computer the person made.
    let after = snapshot_with(&[("theirs", "made", "stopped")]);
    assert!(setup.ok(&after, "computers.list", json!({})).await);
    let record = setup.store.status("main").unwrap().unwrap();
    assert!(record.pending.is_empty() && record.fork_ids.is_empty());
    let params = json!({"computerId": "theirs"});
    assert!(!setup.ok(&after, "computers.start", params).await);
    // A name that exists, or is reserved for Silo, is refused up front.
    assert!(!setup.ok(&after, "checkpoints.fork", fork("main-a1")).await);
    assert!(
        !setup
            .ok(&after, "checkpoints.fork", fork("silo-build-1"))
            .await
    );
    assert_eq!(
        setup
            .fake
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.0 == "checkpoints.fork")
            .count(),
        1
    );
}

#[tokio::test]
async fn keep_promote_undo_and_drop_never_reach_the_engine() {
    let setup = setup(&[]);
    let snapshot = snapshot_with(&[]);
    for method in [
        "changes.keep",
        "changes.promote",
        "changes.undo",
        "computers.delete",
        "changes.requests.keep",
        "secrets.save",
    ] {
        let params = json!({"computerId": "fork-1", "forkId": "fork-1"});
        let reply = setup.ask(&snapshot, "main", method, params).await.unwrap();
        assert_eq!(reply.error.unwrap().code, "permission_denied", "{method}");
    }
    assert!(setup.fake.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_bad_or_foreign_token_gets_no_answer_and_a_revoked_one_stops() {
    let setup = setup(&[]);
    let snapshot = snapshot_with(&[]);
    // The token file copied into a fork: the fork's folder is served as the fork.
    assert!(setup
        .ask(&snapshot, "fork-1", "computers.list", json!({}))
        .await
        .is_none());
    let forged = frame("silo_gc_forged", "computers.list", json!({}));
    let requests = |_: &str| None;
    let env = Frames {
        runner: &setup.fake,
        store: &setup.store,
        snapshot: &snapshot,
        requests: &requests,
        reserved: &setup.reserved,
    };
    assert!(handle_frame(&env, "main", forged).await.is_none());
    let listed = setup
        .ask(&snapshot, "main", "computers.list", json!({}))
        .await
        .unwrap();
    assert_eq!(
        listed.result.unwrap()["linux"]["computers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    setup.store.revoke(&["main"]).unwrap();
    assert!(setup
        .ask(&snapshot, "main", "computers.list", json!({}))
        .await
        .is_none());
}

#[tokio::test]
async fn secrets_are_refused_unless_the_settings_name_them() {
    let params = json!({"computerId": "main", "newName": "x", "secrets": ["github"]});
    let denied = setup(&[]);
    assert!(
        !denied
            .ok(&snapshot_with(&[]), "checkpoints.fork", params.clone())
            .await
    );
    let allowed = setup(&["github"]);
    assert!(
        allowed
            .ok(&snapshot_with(&[]), "checkpoints.fork", params)
            .await
    );
}

#[tokio::test]
async fn starts_in_flight_count_toward_the_running_cap_until_stopped() {
    let setup = setup(&[]);
    let forks = [("f1", "n1"), ("f2", "n2"), ("f3", "n3"), ("f4", "n4")];
    let extra: Vec<(&str, &str, &str)> = forks
        .iter()
        .map(|(id, name)| (*id, *name, "stopped"))
        .collect();
    let snapshot = snapshot_with(&extra);
    setup
        .store
        .update("main", |record| {
            record.fork_ids = forks.iter().map(|(id, _)| (*id).to_owned()).collect();
            Ok(())
        })
        .unwrap();
    for id in ["f1", "f2", "f3"] {
        assert!(
            setup
                .ok(&snapshot, "computers.start", json!({"computerId": id}))
                .await,
            "{id}"
        );
    }
    // None shows running yet, but three starts are under way.
    assert!(
        !setup
            .ok(&snapshot, "computers.start", json!({"computerId": "f4"}))
            .await
    );
    assert!(
        setup
            .ok(&snapshot, "computers.stop", json!({"computerId": "f1"}))
            .await
    );
    assert!(
        setup
            .ok(&snapshot, "computers.start", json!({"computerId": "f4"}))
            .await
    );
}

#[tokio::test]
async fn jobs_are_capped_survive_in_the_token_and_old_ones_are_reaped() {
    let setup = setup(&[]);
    let snapshot = snapshot_with(&[]);
    let now = now_secs();
    setup
        .store
        .update("main", |record| {
            for number in 0..MAX_JOBS {
                record.jobs.push(OwnedJob {
                    id: format!("job:{number}"),
                    at: now - 4000,
                });
            }
            Ok(())
        })
        .unwrap();
    assert!(!setup.ok(&snapshot, "checkpoints.fork", fork("x")).await);
    // A job this token owns can be followed and acknowledged, which frees its place.
    assert!(
        setup
            .ok(&snapshot, "operations.job.get", json!({"jobId": "job:0"}))
            .await
    );
    assert!(
        setup
            .ok(
                &snapshot,
                "operations.job.acknowledge",
                json!({"jobId": "job:0"})
            )
            .await
    );
    assert!(setup.store.status("main").unwrap().unwrap().jobs.len() < MAX_JOBS);
    reap(&setup.fake, &setup.store, "main").await;
    assert!(setup.store.status("main").unwrap().unwrap().jobs.is_empty());
}
