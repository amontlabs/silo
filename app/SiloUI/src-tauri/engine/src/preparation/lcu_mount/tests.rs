use super::*;
use crate::runtime::{CommandOutput, RuntimeError};
use serde_json::json;
use std::{
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

const ARCHIVE: &str = "lcu-0.11.0-linux-x64.tar.gz";

fn spec() -> ArchiveSpec {
    ArchiveSpec {
        version: "0.11.0".into(),
        stable_folder: Some(FOLDER),
        archive: ARCHIVE.into(),
        url: format!("https://example.test/{ARCHIVE}"),
        sha256: String::new(),
        bytes: 0,
        what: "LCU",
        file: "LCU file",
    }
}

fn part(root: &Path, body: &[u8]) -> PathBuf {
    let downloads = root.join(DOWNLOAD_DIR);
    fs::create_dir_all(&downloads).unwrap();
    let part = downloads.join("0.11.0-lcu.part");
    fs::write(&part, body).unwrap();
    part
}

/// Computers as the runtime reports them: name, status and recorded LCU mount source.
struct Computers {
    entries: Vec<(&'static str, &'static str, Option<PathBuf>)>,
    /// Inspections that succeed with a null configuration.
    unreadable: Option<&'static str>,
    /// A mount only the running guest's active configuration records, for the first entry.
    active: Option<PathBuf>,
    /// Running computers whose active configuration is null.
    active_unreadable: bool,
    fail_inspect: Option<&'static str>,
    fail_list: bool,
}

impl Computers {
    fn new(entries: Vec<(&'static str, &'static str, Option<PathBuf>)>) -> Self {
        Self {
            entries,
            unreadable: None,
            active: None,
            active_unreadable: false,
            fail_inspect: None,
            fail_list: false,
        }
    }
}

impl RuntimeRunner for Computers {
    fn run(
        &self,
        _: &RuntimePaths,
        args: &[String],
        _: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        let unavailable = || RuntimeError::Unavailable("runtime".into());
        let stdout = match args[0].as_str() {
            "list" if self.fail_list => return Err(unavailable()),
            "list" => json!(self
                .entries
                .iter()
                .map(|(name, ..)| json!({"name": name}))
                .collect::<Vec<_>>()),
            "inspect" => {
                let (name, status, source) = self
                    .entries
                    .iter()
                    .find(|(name, ..)| *name == args[1])
                    .ok_or_else(unavailable)?;
                if self.fail_inspect == Some(*name) {
                    return Err(unavailable());
                }
                let mounts: Vec<Value> = source
                    .iter()
                    .map(|source| json!({"type": "Bind", "guest": "/opt/silo/lcu", "host": source}))
                    .collect();
                let config = if self.unreadable == Some(*name) {
                    Value::Null
                } else {
                    json!({"mounts": mounts})
                };
                let running = *status == "Running";
                let active = if self.active_unreadable || !running {
                    Value::Null
                } else if let Some(source) =
                    self.active.as_ref().filter(|_| *name == self.entries[0].0)
                {
                    json!({"mounts": [{"type": "Bind", "guest": "/opt/silo/lcu", "host": source}]})
                } else {
                    json!({"mounts": mounts})
                };
                json!({"name": name, "status": status, "config": config, "active_config": active})
            }
            _ => return Err(unavailable()),
        };
        Ok(CommandOutput {
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }
}

fn paths(directory: &tempfile::TempDir) -> RuntimePaths {
    crate::test_support::paths(directory.path())
}

fn collect_with(runner: &Computers, root: &Path, gate: &OperationGate) -> bool {
    let directory = tempfile::tempdir().unwrap();
    collect(gate, runner, &paths(&directory), root, "0.11.0")
}

fn legacy(root: &Path, version: &str) -> PathBuf {
    let folder = root.join(version);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join(format!("lcu-{version}-linux-x64.tar.gz")),
        b"old",
    )
    .unwrap();
    folder
}

#[test]
fn publishing_keeps_the_mount_folder_and_replaces_the_archive_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lcu");
    publish(&root, &spec(), &part(&root, b"first")).unwrap();
    let folder = root.join(FOLDER);
    let identity = fs::metadata(&folder).unwrap().ino();
    fs::write(folder.join("lcu-0.10.0-linux-x64.tar.gz"), b"older").unwrap();
    publish(&root, &spec(), &part(&root, b"second")).unwrap();
    assert_eq!(fs::metadata(&folder).unwrap().ino(), identity);
    assert_eq!(fs::read(folder.join(ARCHIVE)).unwrap(), b"second");
    assert_eq!(
        fs::metadata(folder.join(ARCHIVE))
            .unwrap()
            .permissions()
            .mode()
            & 0o222,
        0
    );
    let names: Vec<_> = fs::read_dir(&folder).unwrap().flatten().collect();
    assert_eq!(names.len(), 1);
}

#[test]
fn a_reader_never_finds_the_folder_missing_or_the_archive_partial_while_publishing_and_repairing() {
    let dir = tempfile::tempdir().unwrap();
    let root = Arc::new(dir.path().join("lcu"));
    publish(&root, &spec(), &part(&root, &[7; 4096])).unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let reader = {
        let (root, done) = (root.clone(), done.clone());
        std::thread::spawn(move || {
            let file = root.join(FOLDER).join(ARCHIVE);
            while !done.load(Ordering::SeqCst) {
                let body = fs::read(&file).expect("the mounted archive vanished");
                assert_eq!(body, vec![7; 4096], "a partial archive was visible");
            }
        })
    };
    let repairer = {
        let (root, done) = (root.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                restore_missing(&root.join(FOLDER), &root);
                restore_missing(&root.join("0.8.1"), &root);
            }
        })
    };
    for _ in 0..200 {
        publish(&root, &spec(), &part(&root, &[7; 4096])).unwrap();
    }
    done.store(true, Ordering::SeqCst);
    reader.join().unwrap();
    repairer.join().unwrap();
    assert!(root.join(FOLDER).join(ARCHIVE).is_file());
}

#[test]
fn a_missing_recorded_folder_is_recreated_empty_once_and_only_directly_under_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lcu");
    fs::create_dir_all(&root).unwrap();
    let source = root.join("0.8.1");
    let created: usize = std::thread::scope(|scope| {
        (0..8)
            .map(|_| scope.spawn(|| restore_missing(&source, &root)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| usize::from(handle.join().unwrap()))
            .sum()
    });
    assert_eq!(created, 1);
    assert!(source.is_dir());
    assert_eq!(fs::read_dir(&source).unwrap().count(), 0);
    assert!(!restore_missing(&source, &root));
    assert!(!restore_missing(&root.join("a").join("b"), &root));
    assert!(!restore_missing(&dir.path().join("elsewhere"), &root));
    assert!(!restore_missing(&root.join(".hidden"), &root));
    assert!(!restore_missing(&root.join("..").join("escape"), &root));
    assert!(!dir.path().join("escape").exists());
}

#[test]
fn collection_removes_unreferenced_folders_and_keeps_what_computers_record() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lcu");
    publish(&root, &spec(), &part(&root, b"current")).unwrap();
    let unused = legacy(&root, "0.7.0");
    let stopped = legacy(&root, "0.8.1");
    let running = legacy(&root, "0.9.0");
    fs::create_dir_all(root.join(".publish-1-1")).unwrap();
    let older = root.join(DOWNLOAD_DIR);
    fs::write(older.join("0.9.0-lcu.part"), b"partial").unwrap();
    fs::write(older.join("0.11.0-lcu.part"), b"resumable").unwrap();
    let runner = Computers::new(vec![
        ("a", "Stopped", Some(stopped.clone())),
        ("b", "Running", Some(running.clone())),
        ("c", "Running", Some(root.join(FOLDER))),
        ("d", "Stopped", None),
    ]);
    assert!(collect_with(&runner, &root, &OperationGate::new()));
    assert!(!unused.exists());
    assert!(!root.join(".publish-1-1").exists());
    assert!(root.join(FOLDER).join(ARCHIVE).is_file());
    assert!(
        stopped.is_dir(),
        "a stopped computer still records its folder"
    );
    assert_eq!(fs::read_dir(&stopped).unwrap().count(), 0);
    assert!(running.join("lcu-0.9.0-linux-x64.tar.gz").is_file());
    assert!(!older.join("0.9.0-lcu.part").exists());
    assert!(older.join("0.11.0-lcu.part").exists());
}

#[test]
fn collection_defers_without_deleting_when_references_cannot_be_determined() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lcu");
    let unused = legacy(&root, "0.7.0");
    let mut runner = Computers::new(vec![("a", "Stopped", None), ("b", "Stopped", None)]);
    runner.fail_inspect = Some("b");
    assert!(!collect_with(&runner, &root, &OperationGate::new()));
    runner.fail_inspect = None;
    runner.fail_list = true;
    assert!(!collect_with(&runner, &root, &OperationGate::new()));
    assert!(unused.join("lcu-0.7.0-linux-x64.tar.gz").is_file());
    runner.fail_list = false;
    assert!(collect_with(&runner, &root, &OperationGate::new()));
    assert!(!unused.exists());
}

#[test]
fn collection_defers_while_lifecycle_work_is_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lcu");
    let unused = legacy(&root, "0.7.0");
    let runner = Computers::new(Vec::new());
    let gate = OperationGate::new();
    // Work on other threads: a running start, a queued device operation.
    std::thread::scope(|scope| {
        let (held, release) = std::sync::mpsc::channel::<()>();
        let (started, ready) = std::sync::mpsc::channel::<()>();
        let gate = &gate;
        let work = scope.spawn(move || {
            let _start = gate.computer("id", "dev", "Starting the computer").unwrap();
            started.send(()).unwrap();
            release.recv().unwrap();
        });
        ready.recv().unwrap();
        assert!(!collect_with(&runner, &root, &gate));
        held.send(()).unwrap();
        work.join().unwrap();
    });
    std::thread::scope(|scope| {
        let (held, release) = std::sync::mpsc::channel::<()>();
        let (started, ready) = std::sync::mpsc::channel::<()>();
        let gate = &gate;
        let work = scope.spawn(move || {
            let _device = gate.device("Updating").unwrap();
            started.send(()).unwrap();
            release.recv().unwrap();
        });
        ready.recv().unwrap();
        assert!(!collect_with(&runner, &root, &gate));
        held.send(()).unwrap();
        work.join().unwrap();
    });
    assert!(unused.exists());
    assert!(collect_with(&runner, &root, &gate));
    assert!(!unused.exists());
}

#[test]
fn collection_and_repair_racing_leave_the_recorded_folder_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lcu");
    fs::create_dir_all(&root).unwrap();
    let recorded = root.join("0.8.1");
    let runner = Computers::new(vec![("a", "Stopped", Some(recorded.clone()))]);
    let gate = OperationGate::new();
    for _ in 0..50 {
        std::thread::scope(|scope| {
            scope.spawn(|| restore_missing(&recorded, &root));
            scope.spawn(|| collect_with(&runner, &root, &gate));
        });
        // Either the repair ran first and the collection kept the recorded folder, or the
        // collection ran first and found nothing to remove.
        assert!(recorded.is_dir(), "the recorded folder was lost");
        fs::remove_dir(&recorded).unwrap();
    }
}

#[test]
fn collection_defers_when_a_configuration_is_unreadable_and_keeps_active_mounts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lcu");
    let recorded = legacy(&root, "0.8.1");
    let mut runner = Computers::new(vec![("a", "Running", None), ("b", "Stopped", None)]);
    runner.unreadable = Some("a");
    assert!(!collect_with(&runner, &root, &OperationGate::new()));
    assert!(recorded.join("lcu-0.8.1-linux-x64.tar.gz").is_file());
    // Readable, but only the running guest's active configuration records the folder.
    runner.unreadable = None;
    runner.active = Some(recorded.clone());
    assert!(collect_with(&runner, &root, &OperationGate::new()));
    assert!(recorded.join("lcu-0.8.1-linux-x64.tar.gz").is_file());
    // A running guest whose active configuration cannot be read leaves references unknown.
    runner.active = None;
    runner.active_unreadable = true;
    assert!(!collect_with(&runner, &root, &OperationGate::new()));
    assert!(recorded.join("lcu-0.8.1-linux-x64.tar.gz").is_file());
    runner.entries[0].1 = "Stopped";
    assert!(collect_with(&runner, &root, &OperationGate::new()));
    assert!(!recorded.exists());
}
