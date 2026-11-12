//! `silo fork ./dir`, fold and undo against a real git repository on the host and a "guest"
//! that is a folder on this machine: the fake engine runs `guest.exec` as a local process
//! with that folder as its home and copies files for uploads and downloads.

mod fake_engine;

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};

use fake_engine::{finished, FakeEngine};
use serde_json::{json, Value};
use silo_cli::{
    engine::Engine,
    flows,
    folder::{self, is_folder},
    ops::MergeOptions,
};

fn ignore(_: &str) {}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Host")
        .env("GIT_AUTHOR_EMAIL", "host@example.com")
        .env("GIT_COMMITTER_NAME", "Host")
        .env("GIT_COMMITTER_EMAIL", "host@example.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A repository on branch `main` with one commit holding `a.txt`.
fn host_repo(root: &Path) -> PathBuf {
    let dir = root.join("repo");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    // Keep merges made by the code under test independent of the machine's git identity:
    // a Linux runner's account has no name, so git refuses to commit without one.
    git(&dir, &["config", "user.name", "Host"]);
    git(&dir, &["config", "user.email", "host@example.com"]);
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    std::fs::canonicalize(dir).unwrap()
}

struct Lab {
    engine: FakeEngine,
    host: PathBuf,
    guest_home: PathBuf,
    computers: Arc<Mutex<Vec<(String, String)>>>,
    requests: Arc<Mutex<Vec<Value>>>,
    diff: Arc<Mutex<Value>>,
    _root: tempfile::TempDir,
}

fn lab() -> Lab {
    let root = tempfile::tempdir().unwrap();
    let host = host_repo(root.path());
    let guest_home = root.path().join("guest-home");
    std::fs::create_dir_all(guest_home.join("workspace")).unwrap();
    let guest_home = std::fs::canonicalize(guest_home).unwrap();
    let computers: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (home, seen) = (guest_home.clone(), requests.clone());
    let shared = computers.clone();
    let diff = Arc::new(Mutex::new(json!({})));
    let shared_diff = diff.clone();
    // The guest's /workspace is a folder of the fake guest's home.
    let workspace = home.join("workspace").to_string_lossy().into_owned();
    let engine = FakeEngine::start(move |method, params| {
        let mut world = shared.lock().unwrap();
        match method {
            "computers.list" => {
                let linux: Vec<Value> = world
                    .iter()
                    .map(|(name, state)| {
                        json!({
                            "configuration": {
                                "id": format!("id-{name}"), "name": name, "cpus": 2, "maxCPUs": 2,
                                "memoryGiB": 4, "maxMemoryGiB": 4,
                                "workspaceStorageGiB": 10, "runtimeStorageGiB": 10
                            },
                            "state": state, "stateDetail": "",
                            "checkpoints": [],
                        })
                    })
                    .collect();
                Ok(json!({"linux": {"computers": linux}, "macos": {"computers": []}}))
            }
            "recipes.build" => {
                if !world.iter().any(|(name, _)| name == "silo-build-linux-dev") {
                    world.push(("silo-build-linux-dev".into(), "stopped".into()));
                }
                Ok(json!({"jobId": "job:2"}))
            }
            "operations.job.get" if params["jobId"] == "job:2" => Ok(finished(json!({
                "built": true,
                "recipe": {
                    "name": "linux-dev", "hash": "abc123abc123abc123",
                    "imageVersion": "v5", "os": "linux",
                    "computerId": "id-silo-build-linux-dev",
                    "computerName": "silo-build-linux-dev",
                    "checkpointId": "cp-recipe", "builtAt": "2026-11-05T00:00:00Z"
                }
            }))),
            "checkpoints.fork" => {
                world.push((
                    params["newName"].as_str().unwrap().to_owned(),
                    "stopped".into(),
                ));
                Ok(json!({"jobId": "job:1"}))
            }
            "computers.start" => {
                let id = params["computerId"].as_str().unwrap();
                for (name, state) in world.iter_mut() {
                    if format!("id-{name}") == id {
                        *state = "running".into();
                    }
                }
                Ok(json!({"jobId": "job:1"}))
            }
            "computers.stop" => {
                let id = params["computerId"].as_str().unwrap();
                for (name, state) in world.iter_mut() {
                    if format!("id-{name}") == id {
                        *state = "stopped".into();
                    }
                }
                Ok(json!({"jobId": "job:1"}))
            }
            "network.urls" => Ok(json!({"computerId": "x", "urls": []})),
            "guest.exec" => {
                let argv: Vec<String> = params["argv"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|word| word.as_str().unwrap().replace("/workspace", &workspace))
                    .collect();
                let output = Command::new(&argv[0])
                    .args(&argv[1..])
                    .current_dir(&home)
                    .env("HOME", &home)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .output()
                    .unwrap();
                Ok(json!({
                    "exitCode": output.status.code(),
                    "signal": null,
                    "stdout": output.stdout,
                    "stderr": output.stderr,
                }))
            }
            "files.upload" => {
                let directory = params["directory"].as_str().unwrap();
                assert!(directory.starts_with("/workspace/"), "{directory}");
                let directory = PathBuf::from(directory.replace("/workspace", &workspace));
                for path in params["paths"].as_array().unwrap() {
                    let path = Path::new(path.as_str().unwrap());
                    std::fs::copy(path, directory.join(path.file_name().unwrap())).unwrap();
                }
                Ok(json!({"jobId": "job:3"}))
            }
            "operations.job.get" if params["jobId"] == "job:3" => {
                Ok(finished(json!({"status": "done", "names": ["x"]})))
            }
            "files.download" => {
                let remote = params["path"].as_str().unwrap();
                assert!(remote.starts_with("/workspace/"), "{remote}");
                std::fs::copy(
                    remote.replace("/workspace", &workspace),
                    params["destination"].as_str().unwrap(),
                )
                .unwrap();
                Ok(json!({"jobId": "job:4"}))
            }
            "operations.job.get" if params["jobId"] == "job:4" => {
                Ok(finished(json!({"status": "done", "path": "x"})))
            }
            "changes.diff" => Ok(json!({"jobId": "job:5"})),
            "operations.job.get" if params["jobId"] == "job:5" => {
                let diff = shared_diff.lock().unwrap().clone();
                Ok(finished(json!({
                    "diff": diff, "text": "Code\n  ~/repo: a..b",
                    "reviewDigest": "d", "baselineApplied": false, "baselineAvailable": false,
                })))
            }
            "changes.request" => {
                seen.lock().unwrap().push(params.clone());
                Ok(json!({"request": {
                    "id": "fr-1", "forkId": params["forkId"], "target": params["targetId"],
                    "createdAt": "t", "reviewDigest": params["reviewDigest"],
                    "summary": params["summary"], "status": "open"
                }}))
            }
            "changes.requests.markKept" => Ok(json!({"requests": []})),
            "operations.job.get" => Ok(finished(Value::Null)),
            "operations.job.acknowledge" => Ok(Value::Null),
            other => Err(("internal", format!("unexpected {other}"))),
        }
    });
    Lab {
        engine,
        host,
        guest_home,
        computers,
        requests,
        diff,
        _root: root,
    }
}

impl Lab {
    async fn connect(&self) -> Engine {
        Engine::connect(&self.engine.socket).await.unwrap()
    }

    async fn fork(&self, engine: &Engine) -> (String, PathBuf) {
        let value = folder::fork_folder(engine, &self.host, None, &[], &ignore)
            .await
            .unwrap();
        let name = value["name"].as_str().unwrap().to_owned();
        (name, PathBuf::from(value["guestPath"].as_str().unwrap()))
    }

    async fn fold(&self, engine: &Engine, fork: &str, dry_run: bool) -> Value {
        flows::merge(
            engine,
            fork,
            None,
            MergeOptions {
                dry_run,
                ..MergeOptions::default()
            },
            &ignore,
        )
        .await
        .unwrap()
    }
}

fn commit(dir: &Path, file: &str, text: &str, message: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", message]);
}

fn backup_refs(dir: &Path) -> Vec<String> {
    git(
        dir,
        &["for-each-ref", "--format=%(refname)", "refs/silo/backup"],
    )
    .lines()
    .map(str::to_owned)
    .collect()
}

#[test]
fn only_paths_are_folders() {
    for folder in [".", "..", "./repo", "../repo", "/tmp/repo", "~/repo", "a/b"] {
        assert!(is_folder(folder), "{folder}");
    }
    for computer in ["main", "main-a1b2c3", "silo_build"] {
        assert!(!is_folder(computer), "{computer}");
    }
}

#[tokio::test]
async fn a_folder_is_copied_into_a_started_fork_as_a_repository() {
    let lab = lab();
    let engine = lab.connect().await;
    let (name, guest) = lab.fork(&engine).await;
    assert!(name.starts_with("repo-"), "{name}");
    assert_eq!(guest, lab.guest_home.join("repo"));
    assert_eq!(
        std::fs::read_to_string(guest.join("a.txt")).unwrap(),
        "one\n"
    );
    assert_eq!(git(&guest, &["log", "--format=%s"]), "first");
    assert_eq!(git(&guest, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    // The bundle is not a remote any more and nothing is left in transit.
    assert_eq!(git(&guest, &["remote"]), "");
    let left: Vec<_> = std::fs::read_dir(lab.guest_home.join("workspace"))
        .unwrap()
        .collect();
    assert!(left.is_empty(), "{left:?}");
    assert!(!lab.host.join(".git/silo").exists());
    // The fork is a Linux fork of the linux-dev recipe, started, and recorded against the folder.
    let forks = lab.engine.calls("checkpoints.fork");
    assert_eq!(forks[0]["checkpointId"], "cp-recipe");
    assert_eq!(forks[0]["newName"], name.as_str());
    assert_eq!(lab.engine.calls("computers.start").len(), 1);
    let computer = engine.computer(&name).await.unwrap();
    let record = folder::record_for(&engine, &computer.id).unwrap();
    assert_eq!(record.path, lab.host.to_string_lossy());
    assert_eq!(record.base, git(&lab.host, &["rev-parse", "HEAD"]));
    assert_eq!(record.branch.as_deref(), Some("main"));
}

#[tokio::test]
async fn a_folder_that_is_not_a_whole_repository_is_refused() {
    let lab = lab();
    let engine = lab.connect().await;
    let plain = lab.host.parent().unwrap().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let error = folder::fork_folder(&engine, &plain, None, &[], &ignore)
        .await
        .unwrap_err();
    assert!(
        error.message.contains("not a git repository"),
        "{}",
        error.message
    );

    let inside = lab.host.join("sub");
    std::fs::create_dir_all(&inside).unwrap();
    let error = folder::fork_folder(&engine, &inside, None, &[], &ignore)
        .await
        .unwrap_err();
    assert!(
        error.message.contains("top of the repository"),
        "{}",
        error.message
    );

    let empty = lab.host.parent().unwrap().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    git(&empty, &["init", "-q"]);
    let error = folder::fork_folder(&engine, &empty, None, &[], &ignore)
        .await
        .unwrap_err();
    assert!(error.message.contains("no commits"), "{}", error.message);
    assert!(lab.engine.calls("checkpoints.fork").is_empty());
}

#[tokio::test]
async fn folding_fast_forwards_the_folder_and_undo_puts_it_back() {
    let lab = lab();
    let engine = lab.connect().await;
    let (fork, guest) = lab.fork(&engine).await;
    let before = git(&lab.host, &["rev-parse", "HEAD"]);
    commit(&guest, "b.txt", "two\n", "add b");
    // Work that was never committed in the fork comes along too.
    std::fs::write(guest.join("a.txt"), "one\nmore\n").unwrap();

    let planned = lab.fold(&engine, &fork, true).await;
    assert_eq!(planned["status"], "dry-run");
    assert_eq!(git(&lab.host, &["rev-parse", "HEAD"]), before);
    assert!(backup_refs(&lab.host).is_empty());
    // Looking does not close the fork's review requests.
    assert!(lab.engine.calls("changes.requests.markKept").is_empty());

    let kept = lab.fold(&engine, &fork, false).await;
    assert_eq!(kept["status"], "kept", "{kept}");
    // The fold closes the requests for that fork and folder, naming the folder's backup ref.
    let closed = &lab.engine.calls("changes.requests.markKept")[0];
    assert_eq!(
        closed["target"],
        format!("folder:{}", lab.host.to_string_lossy())
    );
    assert!(closed["keepId"]
        .as_str()
        .unwrap()
        .starts_with("refs/silo/backup/"));
    assert_eq!(
        kept["report"]["target"],
        lab.host.to_string_lossy().as_ref()
    );
    assert_eq!(
        std::fs::read_to_string(lab.host.join("b.txt")).unwrap(),
        "two\n"
    );
    assert_eq!(
        std::fs::read_to_string(lab.host.join("a.txt")).unwrap(),
        "one\nmore\n"
    );
    assert_eq!(
        git(&lab.host, &["rev-parse", "HEAD"]),
        git(&guest, &["rev-parse", "HEAD"])
    );
    assert_eq!(git(&lab.host, &["rev-list", "--count", "HEAD"]), "3");
    // The fork's work stays on a ref, and the folder's old HEAD on a backup ref.
    assert_eq!(
        git(&lab.host, &["rev-parse", &format!("refs/silo/{fork}/head")]),
        git(&guest, &["rev-parse", "HEAD"])
    );
    let backups = backup_refs(&lab.host);
    assert_eq!(backups.len(), 1);
    assert!(backups[0].starts_with("refs/silo/backup/"));
    assert_eq!(git(&lab.host, &["rev-parse", &backups[0]]), before);
    assert!(!lab.host.join(".git/silo").exists());
    assert!(git(&lab.host, &["status", "--porcelain"]).is_empty());

    // Nothing new in the fork: nothing to bring back.
    let again = lab.fold(&engine, &fork, false).await;
    assert_eq!(again["status"], "unchanged");

    // Undo by the fork's name puts the branch back; a second undo has nothing to do.
    let undone = flows::undo(&engine, &fork, &lab.host, &ignore)
        .await
        .unwrap();
    assert_eq!(undone["status"], "undone");
    assert_eq!(git(&lab.host, &["rev-parse", "HEAD"]), before);
    assert!(!lab.host.join("b.txt").exists());
    let nothing = flows::undo(&engine, &fork, &lab.host, &ignore)
        .await
        .unwrap();
    assert_eq!(nothing["status"], "nothing-to-undo");
}

#[tokio::test]
async fn a_dirty_folder_refuses_the_fold_and_nothing_changes() {
    let lab = lab();
    let engine = lab.connect().await;
    let (fork, guest) = lab.fork(&engine).await;
    commit(&guest, "b.txt", "two\n", "add b");
    std::fs::write(lab.host.join("a.txt"), "edited on the host\n").unwrap();
    let before = git(&lab.host, &["rev-parse", "HEAD"]);
    let refused = lab.fold(&engine, &fork, false).await;
    assert_eq!(refused["status"], "refused");
    assert_eq!(refused["refusal"]["reason"], "dirty");
    assert_eq!(git(&lab.host, &["rev-parse", "HEAD"]), before);
    assert!(backup_refs(&lab.host).is_empty());
    assert_eq!(
        std::fs::read_to_string(lab.host.join("a.txt")).unwrap(),
        "edited on the host\n"
    );
}

#[tokio::test]
async fn a_folder_that_moved_on_is_merged_and_undo_undoes_the_merge() {
    let lab = lab();
    let engine = lab.connect().await;
    let (fork, guest) = lab.fork(&engine).await;
    commit(&guest, "from-fork.txt", "fork\n", "fork work");
    commit(&lab.host, "from-host.txt", "host\n", "host work");
    let before = git(&lab.host, &["rev-parse", "HEAD"]);
    let kept = lab.fold(&engine, &fork, false).await;
    assert_eq!(kept["status"], "kept", "{kept}");
    assert!(lab.host.join("from-fork.txt").exists() && lab.host.join("from-host.txt").exists());
    assert_eq!(
        git(&lab.host, &["rev-list", "--merges", "--count", "HEAD"]),
        "1"
    );
    let backups = backup_refs(&lab.host);
    assert_eq!(git(&lab.host, &["rev-parse", &backups[0]]), before);

    // Undo can name the folder instead of the fork.
    let by_path = lab.host.to_string_lossy().into_owned();
    let undone = flows::undo(&engine, &by_path, Path::new("/"), &ignore)
        .await
        .unwrap();
    assert_eq!(undone["status"], "undone");
    assert_eq!(git(&lab.host, &["rev-parse", "HEAD"]), before);
    assert!(!lab.host.join("from-fork.txt").exists());
}

#[tokio::test]
async fn conflicting_commits_are_refused_and_the_folder_is_left_alone() {
    let lab = lab();
    let engine = lab.connect().await;
    let (fork, guest) = lab.fork(&engine).await;
    commit(&guest, "a.txt", "fork version\n", "fork edit");
    commit(&lab.host, "a.txt", "host version\n", "host edit");
    let before = git(&lab.host, &["rev-parse", "HEAD"]);
    let refused = lab.fold(&engine, &fork, false).await;
    assert_eq!(refused["status"], "refused", "{refused}");
    assert_eq!(refused["refusal"]["reason"], "divergent");
    assert_eq!(refused["refusal"]["conflicts"][0]["item"], "a.txt");
    assert_eq!(git(&lab.host, &["rev-parse", "HEAD"]), before);
    assert!(backup_refs(&lab.host).is_empty());
    assert!(git(&lab.host, &["status", "--porcelain"]).is_empty());
    assert!(!lab.host.join(".git/MERGE_HEAD").exists());
    assert_eq!(
        std::fs::read_to_string(lab.host.join("a.txt")).unwrap(),
        "host version\n"
    );
}

#[tokio::test]
async fn undo_refuses_when_the_folder_has_new_commits_since_the_fold() {
    let lab = lab();
    let engine = lab.connect().await;
    let (fork, guest) = lab.fork(&engine).await;
    commit(&guest, "b.txt", "two\n", "add b");
    lab.fold(&engine, &fork, false).await;
    commit(&lab.host, "later.txt", "later\n", "later work");
    let refused = flows::undo(&engine, &fork, &lab.host, &ignore)
        .await
        .unwrap();
    assert_eq!(refused["status"], "refused");
    assert!(refused["message"].as_str().unwrap().contains("new commits"));
    assert!(lab.host.join("later.txt").exists());
}

#[tokio::test]
async fn a_folder_fork_cannot_replace_main_or_be_merged_into_a_computer() {
    let lab = lab();
    let engine = lab.connect().await;
    let (fork, _) = lab.fork(&engine).await;
    let promoted = silo_cli::ops::replace_main(&engine, &fork, None, false, &ignore).await;
    assert!(promoted
        .unwrap_err()
        .message
        .contains("Merge it into that folder"));
    let into = flows::merge(
        &engine,
        &fork,
        Some("main"),
        MergeOptions::default(),
        &ignore,
    )
    .await;
    assert!(into.unwrap_err().message.contains("--into does not apply"));
    assert!(lab.engine.calls("changes.keep").is_empty());
}

#[tokio::test]
async fn a_review_request_records_the_fork_its_digest_and_a_summary() {
    let lab = lab();
    let engine = lab.connect().await;
    let (fork, guest) = lab.fork(&engine).await;
    commit(&guest, "b.txt", "two\n", "add b");
    let made = flows::request(&engine, &fork, None, &ignore).await.unwrap();
    assert_eq!(made["status"], "requested");
    assert_eq!(made["request"]["id"], "fr-1");
    assert_eq!(made["request"]["status"], "open");
    let sent = lab.requests.lock().unwrap()[0].clone();
    assert_eq!(sent["forkId"], format!("id-{fork}").as_str());
    assert_eq!(
        sent["targetId"],
        format!("folder:{}", lab.host.display()).as_str()
    );
    let digest = sent["reviewDigest"].as_str().unwrap();
    assert!(
        digest.starts_with(&git(&guest, &["rev-parse", "HEAD"])),
        "{digest}"
    );
    assert!(sent["summary"].as_str().unwrap().contains("add b"));
    // Requesting changes nothing: the host and the fork keep their commits.
    assert_eq!(git(&lab.host, &["rev-list", "--count", "HEAD"]), "1");
    assert!(backup_refs(&lab.host).is_empty());
}

async fn request_digest(lab: &Lab, engine: &Engine, fork: &str) -> String {
    flows::request(engine, fork, None, &ignore).await.unwrap();
    let sent = lab.requests.lock().unwrap().last().unwrap().clone();
    sent["reviewDigest"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn the_review_digest_follows_untracked_and_binary_contents_and_pins_the_fold() {
    let lab = lab();
    let engine = lab.connect().await;
    let (fork, guest) = lab.fork(&engine).await;
    git(&guest, &["config", "diff.external", "true"]);
    std::fs::write(guest.join("blob.bin"), [0u8, 1, 2, 3]).unwrap();
    let first = request_digest(&lab, &engine, &fork).await;
    assert_eq!(first, request_digest(&lab, &engine, &fork).await);
    std::fs::write(guest.join("blob.bin"), [0u8, 1, 2, 4]).unwrap();
    let second = request_digest(&lab, &engine, &fork).await;
    assert_ne!(
        first, second,
        "an untracked file's contents change the digest"
    );
    let pinned = |digest: String| MergeOptions {
        reviewed: Some(digest),
        ..MergeOptions::default()
    };
    let stale = flows::merge(&engine, &fork, None, pinned(first), &ignore)
        .await
        .unwrap();
    assert_eq!(stale["status"], "refused");
    assert_eq!(stale["refusal"]["reason"], "changed");
    let kept = flows::merge(&engine, &fork, None, pinned(second), &ignore)
        .await
        .unwrap();
    assert_ne!(kept["status"], "refused", "{kept}");
    assert!(lab.host.join("blob.bin").exists());
}

#[tokio::test]
async fn folding_a_stopped_fork_starts_it_and_stops_it_again() {
    let lab = lab();
    let engine = lab.connect().await;
    let (fork, guest) = lab.fork(&engine).await;
    commit(&guest, "b.txt", "two\n", "add b");
    let set = |state: &str| {
        for (name, current) in lab.computers.lock().unwrap().iter_mut() {
            if *name == fork {
                *current = state.into();
            }
        }
    };
    let state = || {
        lab.computers
            .lock()
            .unwrap()
            .iter()
            .find(|(name, _)| *name == fork)
            .map(|(_, state)| state.clone())
            .unwrap()
    };
    set("stopped");
    let kept = lab.fold(&engine, &fork, false).await;
    assert_eq!(kept["status"], "kept", "{kept}");
    assert!(lab.host.join("b.txt").exists());
    assert_eq!(state(), "stopped");
    // A running fork stays running.
    commit(&guest, "c.txt", "three\n", "add c");
    assert_eq!(state(), "stopped");
    set("running");
    lab.fold(&engine, &fork, false).await;
    assert_eq!(state(), "running");
}

/// A one-request HTTP server that answers `status` with `reply` and hands back the request
/// it got (head, then body).
fn mock_github(status: u16, reply: Value) -> (String, std::sync::mpsc::Receiver<(String, String)>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        let (head, body) = loop {
            let read = stream.read(&mut chunk).unwrap();
            bytes.extend_from_slice(&chunk[..read]);
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                let length = head
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if body.len() >= length {
                    break (head.to_owned(), body.to_owned());
                }
            }
            assert!(read > 0, "the request ended early");
        };
        let payload = reply.to_string();
        write!(
            stream,
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
        .unwrap();
        send.send((head, body)).unwrap();
    });
    (url, receive)
}

/// A bare repository that is the host folder's `origin`, holding its `main`.
fn with_origin(lab: &Lab) -> PathBuf {
    let remote = lab.host.parent().unwrap().join("remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-q", "--bare", "-b", "main"]);
    git(
        &lab.host,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&lab.host, &["push", "-q", "origin", "main"]);
    remote
}

fn pr_mode(token: Option<&str>, api: String) -> flows::Mode {
    flows::Mode::PullRequest(
        silo_cli::pr::PrArgs::default(),
        silo_cli::pr::PrEnv {
            token: token.map(str::to_owned),
            api_url: api,
            repository: Some("acme/app".into()),
            git_base: "https://github.com".into(),
        },
    )
}

#[tokio::test]
async fn fold_pr_pushes_a_branch_and_opens_a_pull_request_with_the_summary() {
    let lab = lab();
    let remote = with_origin(&lab);
    let engine = lab.connect().await;
    let (name, guest) = lab.fork(&engine).await;
    commit(&guest, "b.txt", "two\n", "second");
    let (api, request) = mock_github(
        201,
        json!({"html_url": "https://github.com/acme/app/pull/7", "number": 7}),
    );
    let value = flows::run(
        &engine,
        &name,
        None,
        MergeOptions::default(),
        pr_mode(Some("tok"), api),
        &ignore,
    )
    .await
    .unwrap();
    assert_eq!(value["status"], "pull-request");
    assert_eq!(value["prUrl"], "https://github.com/acme/app/pull/7");
    assert_eq!(
        value["pullRequest"]["url"],
        "https://github.com/acme/app/pull/7"
    );
    let branch = value["branch"].as_str().unwrap().to_owned();
    assert!(branch.starts_with(&format!("silo/{name}-")), "{branch}");
    // The bare remote received the branch with the fork's commit; main there is unchanged.
    assert_eq!(
        git(&remote, &["rev-parse", &format!("refs/heads/{branch}")]),
        git(&guest, &["rev-parse", "HEAD"])
    );
    assert_eq!(git(&remote, &["log", "--format=%s", "main"]), "first");
    // The local branch made for the push is gone.
    assert_eq!(git(&lab.host, &["branch", "--list", "silo/*"]), "");
    // The host folder keeps its branch and tree.
    assert_eq!(
        git(&lab.host, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "main"
    );
    assert_eq!(git(&lab.host, &["status", "--porcelain"]), "");
    assert!(!lab.host.join("b.txt").exists());
    // The API call.
    let (head, body) = request.recv().unwrap();
    assert!(head.starts_with("POST /repos/acme/app/pulls "), "{head}");
    assert!(
        head.to_lowercase().contains("authorization: bearer tok"),
        "{head}"
    );
    let sent: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(sent["head"], branch.as_str());
    assert_eq!(sent["base"], "main");
    let text = sent["body"].as_str().unwrap();
    assert!(
        text.starts_with("Code\n") && text.contains("second"),
        "{text}"
    );
    assert!(silo_cli::render::pull_request(&value).contains("pull/7"));
}

#[tokio::test]
async fn fold_pr_without_a_token_pushes_nothing_and_a_refused_call_names_the_branch() {
    let lab = lab();
    let remote = with_origin(&lab);
    let engine = lab.connect().await;
    let (name, guest) = lab.fork(&engine).await;
    commit(&guest, "b.txt", "two\n", "second");
    let error = flows::run(
        &engine,
        &name,
        None,
        MergeOptions::default(),
        pr_mode(None, "http://127.0.0.1:1".into()),
        &ignore,
    )
    .await
    .unwrap_err();
    assert!(error.message.contains("GITHUB_TOKEN"), "{}", error.message);
    assert_eq!(
        git(
            &remote,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"]
        ),
        "main"
    );

    let (api, _request) = mock_github(
        422,
        json!({"message": "Validation Failed", "errors": [{"message": "A pull request already exists"}]}),
    );
    let error = flows::run(
        &engine,
        &name,
        None,
        MergeOptions::default(),
        pr_mode(Some("tok"), api),
        &ignore,
    )
    .await
    .unwrap_err();
    assert!(
        error.message.contains("422") && error.message.contains("already exists"),
        "{}",
        error.message
    );
    assert!(error.message.contains("was pushed"), "{}", error.message);
}

/// A computer fork `plain` whose repository `~/repo` has the remote `origin` set by the fork.
fn computer_fork(lab: &Lab, origin: &str) -> PathBuf {
    let guest = lab.guest_home.join("repo");
    std::fs::create_dir_all(&guest).unwrap();
    git(&guest, &["init", "-q", "-b", "main"]);
    commit(&guest, "a.txt", "one\n", "first");
    git(&guest, &["remote", "add", "origin", origin]);
    lab.computers
        .lock()
        .unwrap()
        .push(("plain".into(), "running".into()));
    *lab.diff.lock().unwrap() =
        json!({"code": [{"repo": guest.to_string_lossy()}], "reviewDigest": "d"});
    guest
}

fn pr_env(repository: Option<&str>) -> silo_cli::pr::PrEnv {
    silo_cli::pr::PrEnv {
        token: Some("secret-token".into()),
        api_url: "http://127.0.0.1:1".into(),
        repository: repository.map(str::to_owned),
        git_base: "https://127.0.0.1:1".into(),
    }
}

#[tokio::test]
async fn a_computer_fork_never_chooses_where_the_token_goes() {
    let lab = lab();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}/evil/app.git", listener.local_addr().unwrap());
    let guest = computer_fork(&lab, &origin);
    std::fs::write(guest.join("new.txt"), "x\n").unwrap();
    let engine = lab.connect().await;
    // The destination is built on this device and cannot be reached, so the push fails;
    // the fork's own remote is not asked.
    let error = flows::run(
        &engine,
        "plain",
        None,
        MergeOptions::default(),
        flows::Mode::PullRequest(
            silo_cli::pr::PrArgs {
                base: Some("main".into()),
                ..Default::default()
            },
            pr_env(Some("acme/app")),
        ),
        &ignore,
    )
    .await
    .unwrap_err();
    assert!(
        error.message.contains("origin is evil/app") && error.message.contains("acme/app"),
        "{}",
        error.message
    );
    assert!(!error.message.contains("secret-token"));
    assert!(
        listener.accept().is_err(),
        "the fork's remote was contacted"
    );
}

#[tokio::test]
async fn a_computer_fork_with_a_matching_origin_pushes_to_the_host_built_url() {
    let lab = lab();
    let guest = computer_fork(&lab, "https://github.com/acme/app.git");
    std::fs::write(guest.join("new.txt"), "x\n").unwrap();
    let engine = lab.connect().await;
    let error = flows::run(
        &engine,
        "plain",
        None,
        MergeOptions::default(),
        flows::Mode::PullRequest(
            silo_cli::pr::PrArgs {
                base: Some("main".into()),
                ..Default::default()
            },
            pr_env(Some("acme/app")),
        ),
        &ignore,
    )
    .await
    .unwrap_err();
    assert!(
        error.message.contains("https://127.0.0.1:1/acme/app.git"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn an_untracked_credential_file_in_a_computer_fork_stops_the_pull_request() {
    let lab = lab();
    let guest = computer_fork(&lab, "https://github.com/acme/app.git");
    std::fs::write(guest.join("deploy.pem"), "key\n").unwrap();
    let engine = lab.connect().await;
    let error = flows::run(
        &engine,
        "plain",
        None,
        MergeOptions::default(),
        flows::Mode::PullRequest(
            silo_cli::pr::PrArgs {
                base: Some("main".into()),
                ..Default::default()
            },
            pr_env(Some("acme/app")),
        ),
        &ignore,
    )
    .await
    .unwrap_err();
    assert!(
        error.message.contains("deploy.pem") && error.message.contains("credentials"),
        "{}",
        error.message
    );
    assert_eq!(git(&guest, &["log", "--format=%s"]), "first");
}

#[tokio::test]
async fn fold_pr_for_a_folder_refuses_a_repository_other_than_its_origin() {
    let lab = lab();
    git(
        &lab.host,
        &["remote", "add", "origin", "https://github.com/acme/app.git"],
    );
    git(
        &lab.host,
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    let engine = lab.connect().await;
    let (name, guest) = lab.fork(&engine).await;
    commit(&guest, "b.txt", "two\n", "second");
    let error = flows::run(
        &engine,
        &name,
        None,
        MergeOptions::default(),
        flows::Mode::PullRequest(
            silo_cli::pr::PrArgs {
                repo: Some("other/thing".into()),
                ..Default::default()
            },
            silo_cli::pr::PrEnv {
                token: Some("tok".into()),
                api_url: "http://127.0.0.1:1".into(),
                repository: None,
                git_base: "https://github.com".into(),
            },
        ),
        &ignore,
    )
    .await
    .unwrap_err();
    assert!(
        error.message.contains("origin is acme/app") && error.message.contains("other/thing"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn a_computer_fork_needs_a_host_named_repository_and_no_credentials_in_it() {
    let lab = lab();
    let guest = computer_fork(&lab, "https://example.invalid/evil/app.git");
    let engine = lab.connect().await;
    let engine = &engine;
    let run = |repository: Option<&str>| {
        let repository = repository.map(str::to_owned);
        async move {
            flows::run(
                engine,
                "plain",
                None,
                MergeOptions::default(),
                flows::Mode::PullRequest(
                    silo_cli::pr::PrArgs::default(),
                    pr_env(repository.as_deref()),
                ),
                &ignore,
            )
            .await
        }
    };
    let error = run(None).await.unwrap_err();
    assert!(error.message.contains("--repo"), "{}", error.message);
    let error = run(Some("-oProxy/x")).await.unwrap_err();
    assert!(error.message.contains("OWNER/NAME"), "{}", error.message);
    let ssh = format!("{}/.ssh/", guest.display());
    *lab.diff.lock().unwrap() = json!({
        "code": [{"repo": guest.to_string_lossy()}],
        "not_kept": {"credentials": {ssh.clone(): [format!("{ssh}id")]}},
        "reviewDigest": "d"
    });
    let error = run(Some("acme/app")).await.unwrap_err();
    assert!(error.message.contains("credential"), "{}", error.message);
}

#[tokio::test]
async fn fold_pr_refuses_host_commits_that_origin_does_not_have() {
    let lab = lab();
    let remote = with_origin(&lab);
    let engine = lab.connect().await;
    commit(&lab.host, "private.txt", "x\n", "not pushed yet");
    let (name, guest) = lab.fork(&engine).await;
    commit(&guest, "b.txt", "two\n", "second");
    let error = flows::run(
        &engine,
        &name,
        None,
        MergeOptions::default(),
        pr_mode(Some("tok"), "http://127.0.0.1:1".into()),
        &ignore,
    )
    .await
    .unwrap_err();
    assert!(
        error.message.contains("not pushed yet"),
        "{}",
        error.message
    );
    assert_eq!(
        git(
            &remote,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"]
        ),
        "main"
    );
}

#[tokio::test]
async fn fold_pr_ignores_host_commits_made_after_the_fork_started() {
    let lab = lab();
    let remote = with_origin(&lab);
    let engine = lab.connect().await;
    let (name, guest) = lab.fork(&engine).await;
    commit(&guest, "b.txt", "two\n", "second");
    commit(&lab.host, "later.txt", "x\n", "made later");
    let error = flows::run(
        &engine,
        &name,
        None,
        MergeOptions::default(),
        pr_mode(Some("tok"), "http://127.0.0.1:1".into()),
        &ignore,
    )
    .await
    .unwrap_err();
    assert!(
        !error.message.contains("does not, and the pull request"),
        "{}",
        error.message
    );
    drop(remote);
}
