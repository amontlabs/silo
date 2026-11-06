use std::sync::Mutex;

use serde_json::json;
use silo_api::methods::guest_client::GuestClientPolicy;

use super::*;

#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<(String, Value)>>,
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
                "checkpoints.fork" => json!({"jobId": "job:fork-1"}),
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

fn snapshot() -> Snapshot {
    Snapshot::from_values(
        &json!({"linux": {"computers": [{"configuration": {"id": "main"}}]}}),
        &json!({"nodes": [
            {"id": "main", "name": "main", "parent": null},
            {"id": "fork-1", "name": "main-a1", "parent": "main"},
            {"id": "grand", "name": "main-a2", "parent": "fork-1"},
            {"id": "stranger", "name": "other", "parent": null}]}),
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
    }
}

async fn ask(
    setup: &Setup,
    fake: &Fake,
    jobs: &Jobs,
    computer: &str,
    request: GuestRequestFrame,
) -> GuestResponseFrame {
    handle_frame(
        fake,
        &setup.store,
        jobs,
        &snapshot(),
        &|_| Some("fork-1".into()),
        computer,
        request,
    )
    .await
}

#[test]
fn descendants_follow_forks_of_forks_and_never_include_the_root() {
    let found = snapshot().descendants_of("main");
    assert_eq!(found, ["fork-1", "grand"].map(String::from).into());
    assert!(snapshot().descendants_of("stranger").is_empty());
}

#[tokio::test]
async fn a_fork_of_self_runs_without_the_token_file_and_its_job_is_followable() {
    let setup = setup(&[]);
    let (fake, jobs) = (Fake::default(), Jobs::default());
    let reply = ask(
        &setup,
        &fake,
        &jobs,
        "main",
        frame(
            &setup.token,
            "checkpoints.fork",
            json!({"computerId": "main", "newName": "x"}),
        ),
    )
    .await;
    assert!(reply.ok, "{:?}", reply.error);
    let calls = fake.calls.lock().unwrap();
    assert_eq!(
        calls[0].1["quiet"]["scrub"],
        json!(["/etc/silo/guest-client.json"])
    );
    drop(calls);
    let followed = ask(
        &setup,
        &fake,
        &jobs,
        "main",
        frame(
            &setup.token,
            "operations.job.get",
            json!({"jobId": "job:fork-1"}),
        ),
    )
    .await;
    assert!(followed.ok);
    let other = ask(
        &setup,
        &fake,
        &jobs,
        "main",
        frame(
            &setup.token,
            "operations.job.get",
            json!({"jobId": "job:other"}),
        ),
    )
    .await;
    assert_eq!(other.error.unwrap().code, "permission_denied");
}

#[tokio::test]
async fn keep_promote_undo_and_drop_never_reach_the_engine() {
    let setup = setup(&[]);
    let (fake, jobs) = (Fake::default(), Jobs::default());
    for method in [
        "changes.keep",
        "changes.promote",
        "changes.undo",
        "computers.delete",
        "changes.requests.keep",
        "secrets.save",
    ] {
        let reply = ask(
            &setup,
            &fake,
            &jobs,
            "main",
            frame(
                &setup.token,
                method,
                json!({"computerId": "fork-1", "forkId": "fork-1"}),
            ),
        )
        .await;
        assert!(!reply.ok, "{method}");
        assert_eq!(reply.error.unwrap().code, "permission_denied");
    }
    assert!(fake.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_token_is_valid_only_for_its_own_computer_and_stops_when_revoked() {
    let setup = setup(&[]);
    let (fake, jobs) = (Fake::default(), Jobs::default());
    let list = || frame(&setup.token, "computers.list", json!({}));
    // The token file copied into a fork: the fork's folder is served as the fork.
    let in_fork = ask(&setup, &fake, &jobs, "fork-1", list()).await;
    assert_eq!(in_fork.error.unwrap().code, "permission_denied");
    let forged = ask(
        &setup,
        &fake,
        &jobs,
        "main",
        frame("silo_gc_forged", "computers.list", json!({})),
    )
    .await;
    assert!(!forged.ok);
    let allowed = ask(&setup, &fake, &jobs, "main", list()).await;
    let listed = allowed.result.unwrap();
    assert_eq!(listed["linux"]["computers"].as_array().unwrap().len(), 1);
    setup.store.revoke(&["main"]).unwrap();
    assert!(!ask(&setup, &fake, &jobs, "main", list()).await.ok);
}

#[tokio::test]
async fn secrets_are_refused_unless_the_settings_name_them() {
    let denied = setup(&[]);
    let (fake, jobs) = (Fake::default(), Jobs::default());
    let params = json!({"computerId": "main", "newName": "x", "secrets": ["github"]});
    assert!(
        !ask(
            &denied,
            &fake,
            &jobs,
            "main",
            frame(&denied.token, "checkpoints.fork", params.clone())
        )
        .await
        .ok
    );
    let allowed = setup(&["github"]);
    assert!(
        ask(
            &allowed,
            &fake,
            &jobs,
            "main",
            frame(&allowed.token, "checkpoints.fork", params)
        )
        .await
        .ok
    );
}

#[tokio::test]
async fn only_forks_the_token_asked_for_can_be_started() {
    let setup = setup(&[]);
    let (fake, jobs) = (Fake::default(), Jobs::default());
    let start = |id: &str| frame(&setup.token, "computers.start", json!({"computerId": id}));
    // fork-1 and grand exist, but the person made them: the token has not asked for them.
    assert!(!ask(&setup, &fake, &jobs, "main", start("fork-1")).await.ok);
    let fork = frame(
        &setup.token,
        "checkpoints.fork",
        json!({"computerId": "main", "newName": "main-a1"}),
    );
    assert!(ask(&setup, &fake, &jobs, "main", fork).await.ok);
    assert_eq!(
        setup.store.status("main").unwrap().unwrap().fork_names,
        ["main-a1"]
    );
    assert!(ask(&setup, &fake, &jobs, "main", start("fork-1")).await.ok);
    assert!(!ask(&setup, &fake, &jobs, "main", start("grand")).await.ok);
}
