//! The retry of a restore whose attempt is already running: `start_pending` accepts the
//! running computer through `running_child_matches`, and a built-in computer must still pass
//! `require_mount` first. Kept apart from the main checkpoint tests.
use super::*;

const ID: &str = "00000000-0000-4000-8000-000000000001";
const ATTEMPT: &str = "6b79cf8f-70b3-4d2f-93d1-3b8b7a7c0001";

fn built_in_computer() -> ComputerConfiguration {
    ComputerConfiguration {
        id: ID.into(),
        name: "dev".into(),
        cpus: 1,
        max_cpus: 1,
        memory_gib: 1,
        max_memory_gib: 1,
        workspace_storage_gib: 1,
        runtime_storage_gib: 1,
        desktop: Some(crate::desktop::DesktopConfiguration {
            start_with_computer: true,
            built_in: true,
        }),
    }
}

/// A runtime where the earlier restore attempt is up and running with the computer's labels,
/// the saved network policy and the GitHub secret entry; `mounts` is what it reports.
struct RunningAttempt {
    mounts: serde_json::Value,
    calls: Mutex<Vec<Vec<String>>>,
}

impl RuntimeRunner for RunningAttempt {
    fn run(
        &self,
        _paths: &RuntimePaths,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.calls.lock().unwrap().push(args.to_vec());
        let stdout = match args.first().map(String::as_str) {
            Some("snapshot") => serde_json::json!([{
                "group":"silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
                "name":"silo-backup-0-330418-1790360984903",
                "scope":"disk", "availability":"ready"
            }])
            .to_string(),
            Some("list") => serde_json::json!([{"name":"dev"}]).to_string(),
            Some("inspect") => serde_json::json!({"name":"dev","status":"Running","config":{
                "labels":{"silo.managed":"true","silo.machine-id":ID,
                    "silo.restore-attempt":ATTEMPT},
                "network":{
                    "policy":{"default_egress":"deny","default_ingress":"deny","rules":[]},
                    "secrets":{"secrets":[{"env_var":"SILO_GITHUB","value":"",
                        "source":{"kind":"env","var":"SILO_GITHUB"}}]},
                },
                "mounts": self.mounts,
            }})
            .to_string(),
            _ => panic!("accepting a running attempt must not change the computer: {args:?}"),
        };
        Ok(CommandOutput {
            stdout,
            stderr: String::new(),
        })
    }
}

fn pending_attempt(paths: &RuntimePaths) {
    import_pending_restore(
        paths,
        ID,
        "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
        "silo-backup-0-330418-1790360984903",
    )
    .unwrap();
    let mut record = load(paths, ID).unwrap();
    record.restore_attempted = true;
    record.restore_attempt_id = Some(ATTEMPT.into());
    save(paths, ID, &record).unwrap();
}

fn mount(readonly: bool) -> serde_json::Value {
    serde_json::json!([
        {"type":"Owned","guest":"/workspace"},
        {"type":"Bind","host":"/h/published","guest":"/opt/silo/chatgpt",
            "options":{"readonly":readonly}},
    ])
}

fn only_reads(runner: &RunningAttempt) -> bool {
    runner
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|args| matches!(args[0].as_str(), "snapshot" | "list" | "inspect"))
}

#[test]
fn a_running_attempt_without_the_computer_use_mount_is_not_accepted() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    let published = directory.path().join("published");
    std::fs::create_dir(&published).unwrap();
    crate::computer_use::set_test_published_dir(Some(published));
    pending_attempt(&paths);
    for mounts in [
        serde_json::json!([{"type":"Owned","guest":"/workspace"}]),
        mount(false),
    ] {
        let runner = RunningAttempt {
            mounts,
            calls: Mutex::new(Vec::new()),
        };
        let failure = start_pending(&runner, &paths, &built_in_computer())
            .unwrap_err()
            .to_string();
        assert!(failure.contains("computer-use folder"), "{failure}");
        assert!(only_reads(&runner));
        // The recovery state survives, so the next Retry sees the same attempt.
        let kept = load(&paths, ID).unwrap();
        assert!(kept.pending_checkpoint_restore.is_some());
        assert!(kept.restore_attempted);
        assert_eq!(kept.restore_attempt_id.as_deref(), Some(ATTEMPT));
    }
    crate::computer_use::set_test_published_dir(None);
}

#[test]
fn a_running_attempt_with_the_read_only_mount_is_accepted_and_the_restore_completes() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    let published = directory.path().join("published");
    std::fs::create_dir(&published).unwrap();
    crate::computer_use::set_test_published_dir(Some(published));
    pending_attempt(&paths);
    let runner = RunningAttempt {
        mounts: mount(true),
        calls: Mutex::new(Vec::new()),
    };
    start_pending(&runner, &paths, &built_in_computer()).unwrap();
    assert!(only_reads(&runner));
    let record = load(&paths, ID).unwrap();
    assert!(record.pending_checkpoint_restore.is_none());
    assert!(!record.restore_attempted && !record.restore_attempt_ran);
    assert!(record.restore_attempt_id.is_none());
    crate::computer_use::set_test_published_dir(None);
}
