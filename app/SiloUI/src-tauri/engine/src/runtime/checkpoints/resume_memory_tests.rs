//! The memory a pending restore of a full checkpoint resumes a computer at.
use super::*;

const ID: &str = "00000000-0000-4000-8000-000000000002";
const GROUP: &str = "dev";
const MEMBER: &str = "c000000000000000000000000000000";

/// A runtime whose checkpoint list names the artifact folders given, and counts its calls.
struct Listing {
    members: Vec<serde_json::Value>,
    calls: Mutex<usize>,
}

impl RuntimeRunner for Listing {
    fn run(
        &self,
        _paths: &RuntimePaths,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        *self.calls.lock().unwrap() += 1;
        assert_eq!(&args[..2], ["snapshot", "list"], "{args:?}");
        Ok(CommandOutput {
            stdout: serde_json::Value::Array(self.members.clone()).to_string(),
            stderr: String::new(),
        })
    }
}

fn pending(paths: &RuntimePaths, state: Option<&str>) {
    let mut record = Record {
        version: RECORD_VERSION,
        snapshot_group: Some(GROUP.into()),
        ..Record::default()
    };
    record.pending_checkpoint_restore = state.map(|state| PendingRestore {
        checkpoint_id: MEMBER.into(),
        source_computer: GROUP.into(),
        state: state.into(),
    });
    save(paths, ID, &record).unwrap();
}

/// A capture of `memory_mib` stored under the runtime's snapshots, and the list that names it.
fn capture(paths: &RuntimePaths, descriptor: serde_json::Value) -> Listing {
    let artifact = paths.home.join("snapshots").join(GROUP).join("snap_1");
    fs::create_dir_all(&artifact).unwrap();
    fs::write(artifact.join("snapshot.json"), descriptor.to_string()).unwrap();
    listing(&artifact)
}

fn listing(artifact: &Path) -> Listing {
    Listing {
        members: vec![serde_json::json!({
            "group": GROUP, "name": MEMBER, "snapshot_id": "snap_1",
            "availability": "ready", "scope": "full",
            "artifact_path": artifact,
        })],
        calls: Mutex::new(0),
    }
}

fn full_capture(memory_mib: u64) -> serde_json::Value {
    serde_json::json!({"scope": "full", "state": {"kind": "checkpoint",
        "requirements_summary": {"vcpus": 2, "memory_mib": memory_mib}}})
}

#[test]
fn a_pending_full_restore_resumes_at_the_memory_the_capture_recorded() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    pending(&paths, Some("full"));
    let runner = capture(&paths, full_capture(16 * 1024));
    assert_eq!(
        pending_resume_memory_mib(&runner, &paths, ID, "dev").unwrap(),
        Some(16 * 1024)
    );
}

#[test]
fn a_restore_that_boots_cold_or_is_not_pending_asks_the_runtime_nothing() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    let runner = capture(&paths, full_capture(16 * 1024));
    for state in [Some("disk"), None] {
        pending(&paths, state);
        assert_eq!(
            pending_resume_memory_mib(&runner, &paths, ID, "dev").unwrap(),
            None
        );
    }
    assert_eq!(*runner.calls.lock().unwrap(), 0);
}

#[test]
fn a_capture_whose_memory_cannot_be_read_is_an_error() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    pending(&paths, Some("full"));
    // No memory recorded, a memory of nothing, and a descriptor that is not JSON.
    for descriptor in [
        serde_json::json!({"scope": "full", "state": {"kind": "checkpoint"}}),
        full_capture(0),
        serde_json::json!("not a descriptor"),
    ] {
        let runner = capture(&paths, descriptor);
        assert!(pending_resume_memory_mib(&runner, &paths, ID, "dev").is_err());
    }
    // The list does not name the checkpoint.
    let runner = Listing {
        members: Vec::new(),
        calls: Mutex::new(0),
    };
    assert!(pending_resume_memory_mib(&runner, &paths, ID, "dev").is_err());
    // A capture outside the runtime's snapshot folder is never read.
    let outside = directory.path().join("elsewhere");
    fs::create_dir_all(&outside).unwrap();
    fs::write(
        outside.join("snapshot.json"),
        full_capture(1024).to_string(),
    )
    .unwrap();
    let runner = listing(&outside);
    assert!(pending_resume_memory_mib(&runner, &paths, ID, "dev").is_err());
}

fn saved(paths: &RuntimePaths, scope: &str) {
    let record = Record {
        version: RECORD_VERSION,
        snapshot_group: Some(GROUP.into()),
        checkpoints: vec![Checkpoint {
            id: "chk-1".into(),
            native_id: Some(MEMBER.into()),
            name: "before".into(),
            created_at: 1,
            scope: scope.into(),
            reason: "manual".into(),
            kind: "manual".into(),
        }],
        ..Record::default()
    };
    save(paths, ID, &record).unwrap();
}

#[test]
fn a_saved_full_checkpoint_resumes_at_the_memory_the_capture_recorded() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    saved(&paths, "full");
    let runner = capture(&paths, full_capture(8 * 1024));
    assert_eq!(
        checkpoint_resume_memory_mib(&runner, &paths, ID, "chk-1").unwrap(),
        Some(8 * 1024)
    );
}

#[test]
fn a_saved_disk_checkpoint_or_an_unknown_one_resumes_nothing_and_asks_the_runtime_nothing() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    let runner = capture(&paths, full_capture(8 * 1024));
    saved(&paths, "disk");
    assert_eq!(
        checkpoint_resume_memory_mib(&runner, &paths, ID, "chk-1").unwrap(),
        None
    );
    saved(&paths, "full");
    assert_eq!(
        checkpoint_resume_memory_mib(&runner, &paths, ID, "other").unwrap(),
        None
    );
    assert_eq!(*runner.calls.lock().unwrap(), 0);
}

#[test]
fn a_saved_full_checkpoint_whose_memory_cannot_be_read_is_an_error() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    saved(&paths, "full");
    let runner = capture(&paths, full_capture(0));
    assert!(checkpoint_resume_memory_mib(&runner, &paths, ID, "chk-1").is_err());
}
