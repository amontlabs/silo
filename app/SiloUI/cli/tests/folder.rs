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
    config::Project,
    engine::Engine,
    flows,
    folder::{self, is_folder},
    ops::FoldOptions,
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
        _root: root,
    }
}

impl Lab {
    async fn connect(&self) -> Engine {
        Engine::connect(&self.engine.socket).await.unwrap()
    }

    async fn fork(&self, engine: &Engine) -> (String, PathBuf) {
        let recipe = Project::default().named_recipe("linux-dev").unwrap();
        let value = folder::fork_folder(engine, &self.host, None, &recipe, None, &[], &ignore)
            .await
            .unwrap();
        let name = value["name"].as_str().unwrap().to_owned();
        (name, PathBuf::from(value["guestPath"].as_str().unwrap()))
    }

    async fn fold(&self, engine: &Engine, fork: &str, dry_run: bool) -> Value {
        flows::fold(
            engine,
            fork,
            None,
            None,
            FoldOptions {
                dry_run,
                ..FoldOptions::default()
            },
            false,
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
    let recipe = Project::default().named_recipe("linux-dev").unwrap();
    let plain = lab.host.parent().unwrap().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let error = folder::fork_folder(&engine, &plain, None, &recipe, None, &[], &ignore)
        .await
        .unwrap_err();
    assert!(
        error.message.contains("not a git repository"),
        "{}",
        error.message
    );

    let inside = lab.host.join("sub");
    std::fs::create_dir_all(&inside).unwrap();
    let error = folder::fork_folder(&engine, &inside, None, &recipe, None, &[], &ignore)
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
    let error = folder::fork_folder(&engine, &empty, None, &recipe, None, &[], &ignore)
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
async fn a_folder_fork_cannot_be_promoted_or_kept_into_a_computer() {
    let lab = lab();
    let engine = lab.connect().await;
    let (fork, _) = lab.fork(&engine).await;
    let promoted = silo_cli::ops::promote(&engine, &fork, None, false, &ignore).await;
    assert!(promoted
        .unwrap_err()
        .message
        .contains("Fold it into that folder"));
    let into = flows::fold(
        &engine,
        &fork,
        Some("main"),
        None,
        FoldOptions::default(),
        false,
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
    let made = flows::fold(
        &engine,
        &fork,
        None,
        None,
        FoldOptions::default(),
        true,
        &ignore,
    )
    .await
    .unwrap();
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
