use std::{
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

use clap::Parser;
use serde_json::{json, Value};
use silo_api::methods::guest_client::{GuestRequestFrame, GuestResponseFrame};

use super::*;
use crate::cli::Cli;

type Seen = Arc<Mutex<Vec<(String, Value)>>>;

/// A bridge folder served by a thread that answers each request from `answer`.
struct Endpoint {
    dir: tempfile::TempDir,
    seen: Seen,
    stop: Arc<AtomicBool>,
}

impl Endpoint {
    fn start(answer: impl Fn(&str, &Value) -> Result<Value, String> + Send + 'static) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for part in ["req", "resp"] {
            fs::create_dir_all(dir.path().join("spool").join(part)).unwrap();
        }
        fs::write(
            dir.path().join("guest-client.json"),
            json!({"version": 1, "token": "silo_gc_test", "computerId": "main-id",
                   "spool": dir.path().join("spool")})
            .to_string(),
        )
        .unwrap();
        let (seen, stop): (Seen, Arc<AtomicBool>) = Default::default();
        let (spool, log, flag) = (dir.path().join("spool"), seen.clone(), stop.clone());
        thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                for entry in fs::read_dir(spool.join("req")).unwrap().flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if !name.ends_with(".json") {
                        continue;
                    }
                    let frame: GuestRequestFrame =
                        serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap();
                    fs::remove_file(entry.path()).unwrap();
                    assert_eq!(frame.token, "silo_gc_test");
                    log.lock()
                        .unwrap()
                        .push((frame.method.clone(), frame.params.clone()));
                    let reply = match answer(&frame.method, &frame.params) {
                        Ok(result) => GuestResponseFrame {
                            v: 1,
                            id: frame.id.clone(),
                            ok: true,
                            result: Some(result),
                            error: None,
                        },
                        Err(message) => GuestResponseFrame {
                            v: 1,
                            id: frame.id.clone(),
                            ok: false,
                            result: None,
                            error: Some(silo_api::methods::guest_client::GuestErrorBody {
                                code: "permission_denied".into(),
                                message,
                            }),
                        },
                    };
                    fs::write(
                        spool.join("resp").join(format!("{}.json", frame.id)),
                        serde_json::to_vec(&reply).unwrap(),
                    )
                    .unwrap();
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
        Self { dir, seen, stop }
    }

    fn link(&self) -> Link {
        Link::load(&self.dir.path().join("guest-client.json")).unwrap()
    }

    fn methods(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(method, _)| method.clone())
            .collect()
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn command(args: &[&str]) -> Command {
    let mut all = vec!["silo", "--in-guest"];
    all.extend(args);
    Cli::parse_from(all).command
}

fn world(method: &str, _params: &Value) -> Result<Value, String> {
    Ok(match method {
        "computers.tree" => json!({"nodes": [
            {"id": "main-id", "name": "main", "parent": null, "state": "running"},
            {"id": "fork-id", "name": "main-a1", "parent": "main-id", "state": "running"}]}),
        "checkpoints.fork" | "computers.start" | "changes.diff" => {
            json!({"jobId": format!("job:{method}")})
        }
        "operations.job.get" => {
            json!({"state": "finished", "outcome": {"result": {"text": "1 file changed", "reviewDigest": "dg", "code": [1]}}})
        }
        "changes.request" => json!({"request": {"id": "fr-1"}}),
        _ => json!({}),
    })
}

#[tokio::test]
async fn a_fork_of_self_sends_no_secrets_and_starts_the_new_fork() {
    let endpoint = Endpoint::start(world);
    let code = run(
        command(&["fork", "main", "--name", "main-a1"]),
        true,
        &endpoint.link(),
    )
    .await
    .unwrap();
    assert_eq!(code, 0);
    let seen = endpoint.seen.lock().unwrap().clone();
    let fork = seen
        .iter()
        .find(|(method, _)| method == "checkpoints.fork")
        .unwrap();
    assert_eq!(fork.1["computerId"], "main-id");
    assert_eq!(fork.1["newName"], "main-a1");
    assert_eq!(fork.1["secrets"], json!([]));
    let start = seen
        .iter()
        .find(|(method, _)| method == "computers.start")
        .unwrap();
    assert_eq!(start.1["computerId"], "fork-id");
}

#[tokio::test]
async fn forking_another_computer_or_with_a_recipe_never_reaches_the_bridge() {
    let endpoint = Endpoint::start(world);
    let other = run(command(&["fork", "elsewhere"]), true, &endpoint.link())
        .await
        .unwrap_err();
    assert!(other.to_string().contains("only that computer"));
    let recipe = run(
        command(&["fork", "x", "--recipe", "linux-dev"]),
        true,
        &endpoint.link(),
    )
    .await
    .unwrap_err();
    assert!(recipe.to_string().contains("not available from inside"));
    assert!(!endpoint.methods().contains(&"checkpoints.fork".to_owned()));
}

#[tokio::test]
async fn keep_promote_undo_drop_and_secrets_are_refused_before_anything_is_sent() {
    let endpoint = Endpoint::start(world);
    for args in [
        &["fold", "main-a1"][..],
        &["fold", "main-a1", "--promote"],
        &["fold", "main-a1", "--pr"],
        &["undo", "main"],
        &["drop", "main-a1", "--force"],
        &["secrets", "list"],
        &["exec", "main", "--", "id"],
        &["requests", "keep", "fr-1"],
        &["requests", "throw-away", "fr-1"],
    ] {
        let error = run(command(args), true, &endpoint.link())
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("not available from inside"),
            "{args:?}: {error}"
        );
    }
    assert!(endpoint.methods().is_empty());
}

#[tokio::test]
async fn asking_for_review_diffs_the_fork_then_records_a_request() {
    let endpoint = Endpoint::start(world);
    let code = run(
        command(&["fold", "main-a1", "--request"]),
        true,
        &endpoint.link(),
    )
    .await
    .unwrap();
    assert_eq!(code, 0);
    let seen = endpoint.seen.lock().unwrap().clone();
    let request = seen
        .iter()
        .find(|(method, _)| method == "changes.request")
        .unwrap();
    assert_eq!(request.1["forkId"], "fork-id");
    assert_eq!(request.1["reviewDigest"], "dg");
    assert!(request.1.get("targetId").is_none());
}

#[tokio::test]
async fn an_engine_refusal_reaches_the_agent_as_the_error() {
    let endpoint = Endpoint::start(|method, params| {
        if method == "changes.diff" {
            Err("That computer is not one of this computer's forks.".into())
        } else {
            world(method, params)
        }
    });
    let error = run(command(&["diff", "main-a1"]), true, &endpoint.link())
        .await
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("not one of this computer's forks"));
}

#[test]
fn status_warns_when_the_settings_no_longer_match_the_token() {
    use crate::guest_install::stale_warning;
    use silo_api::methods::guest_client::GuestClientPolicy;
    let status = json!({"policy": {"forkSecrets": ["github"]}});
    let same = GuestClientPolicy {
        fork_secrets: vec!["github".into()],
        agent: None,
    };
    let changed = GuestClientPolicy {
        fork_secrets: vec![],
        agent: None,
    };
    assert!(stale_warning(&status, &same).is_none());
    assert!(stale_warning(&status, &changed)
        .unwrap()
        .contains("install"));
}

#[test]
fn a_missing_token_file_says_how_to_get_one() {
    let error = Link::load(Path::new("/nonexistent/guest-client.json"))
        .err()
        .unwrap();
    assert!(error.to_string().contains("silo guest-client install"));
}
