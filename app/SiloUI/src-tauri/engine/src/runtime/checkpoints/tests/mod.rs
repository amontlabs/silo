use super::*;

#[test]
fn a_migrated_checkpoint_record_loads() {
    let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
    let record = load(
        &migrated.runtime_paths(),
        crate::runtime_migration::vocabulary_tests::ID,
    )
    .unwrap();
    let pending = record.pending_checkpoint_restore.unwrap();
    assert_eq!(pending.source_computer, "dev");
    assert_eq!(pending.state, "full");
}

mod failed_capture_tests;

const ID: &str = "00000000-0000-4000-8000-000000000001";

#[test]
fn full_snapshot_satisfies_a_disk_only_restore_but_not_the_reverse() {
    let _test_state = crate::test_support::global_state();
    use crate::test_support::runner::{ExpectedCommand, ScriptedRunner};
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    for (available, requested, ready) in [
        ("full", "disk", true),
        ("full", "full", true),
        ("disk", "disk", true),
        ("disk", "full", false),
    ] {
        let runner = ScriptedRunner::new([ExpectedCommand::ok(
                ["snapshot", "list", "--format", "json"],
                serde_json::json!([{"group":"g", "name":"c", "scope":available, "availability":"ready"}]).to_string(),
            ).with_timeout(READ_TIMEOUT)]);
        assert_eq!(
            snapshot_ready(&runner, &paths, "g", "c", requested).is_ok(),
            ready
        );
        runner.assert_finished();
    }
}

#[test]
fn imported_snapshot_intent_accepts_native_selectors_but_rejects_paths() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let group = "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9";
    let member = "silo-backup-0-330418-1790360984903";
    import_pending_restore(&paths, ID, group, member).unwrap();
    let pending = load(&paths, ID)
        .unwrap()
        .pending_checkpoint_restore
        .unwrap();
    assert_eq!(pending.source_computer, group);
    assert_eq!(
        load(&paths, ID).unwrap().snapshot_group.as_deref(),
        Some(group)
    );
    assert_eq!(pending.checkpoint_id, member);
    assert_eq!(pending.state, "disk");
    for (bad_group, bad_member) in [
        ("../silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9", member),
        (group, "../silo-backup-0-330418-1790360984903"),
        (group, "silo-backup-0:330418-1790360984903"),
        ("silo-import-not-a-uuid", member),
    ] {
        assert!(import_pending_restore(&paths, ID, bad_group, bad_member).is_err());
    }
}

#[test]
fn imported_snapshot_intent_accepts_a_checkpoint_native_member() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let group = "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9";
    // A `c` + 31 hex-digit member, as produced by a checkpoint export.
    let member = "c0123456789abcdef0123456789abcde";
    import_pending_restore(&paths, ID, group, member).unwrap();
    let pending = load(&paths, ID)
        .unwrap()
        .pending_checkpoint_restore
        .unwrap();
    assert_eq!(pending.checkpoint_id, member);
    assert_eq!(pending.state, "disk");
    // Uppercase hex is not a form Silo produces and stays rejected.
    assert!(import_pending_restore(&paths, ID, group, "c0123456789ABCDEF0123456789abcde").is_err());
    // A bare word that is neither a backup member nor a checkpoint id is rejected.
    assert!(import_pending_restore(&paths, ID, group, "imported-member").is_err());
}

#[test]
fn export_preflight_does_not_migrate_lineage_while_checkpoint_work_holds_gate() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer_configuration()],
        },
    )
    .unwrap();
    let checkpoint_id = "c000000000000000000000000000001";
    let mut record = Record::default();
    record.checkpoints.push(Checkpoint {
        id: checkpoint_id.into(),
        native_id: None,
        name: "Legacy milestone".into(),
        created_at: 1,
        scope: "disk".into(),
        reason: "manual".into(),
    });
    save(&paths, ID, &record).unwrap();
    let before = fs::read(path(&paths, ID)).unwrap();
    let checkpoint_guard = super::super::OPERATIONS
        .kind(super::super::operation_gate::OperationKind::CheckpointCapture)
        .computer(ID, "dev", "Creating checkpoint")
        .unwrap();
    let (result, during_preflight) = std::thread::scope(|scope| {
        let (send, receive) = std::sync::mpsc::channel();
        let preflight_paths = &paths;
        let worker = scope.spawn(move || {
            send.send(export_source(preflight_paths, ID, checkpoint_id))
                .unwrap();
        });
        let result = receive.recv_timeout(Duration::from_secs(5));
        let during_preflight = fs::read(path(&paths, ID)).unwrap();
        drop(checkpoint_guard);
        worker.join().unwrap();
        (result, during_preflight)
    });
    let (group, member, scope, name) = result.unwrap().unwrap();
    assert_eq!(group, "dev");
    assert_eq!(member, checkpoint_id);
    assert_eq!(scope, "disk");
    assert_eq!(name, "Legacy milestone");
    assert!(
        during_preflight == before,
        "Export preflight rewrote checkpoint history while checkpoint work held the gate."
    );
    assert_eq!(load(&paths, ID).unwrap().snapshot_group, None);

    let _export_guard = super::super::OPERATIONS
        .kind(super::super::operation_gate::OperationKind::Export)
        .device("Exporting computer")
        .unwrap();
    assert_eq!(ensure_snapshot_group(&paths, ID, "dev").unwrap(), "dev");
    assert_eq!(
        load(&paths, ID).unwrap().snapshot_group.as_deref(),
        Some("dev")
    );
}

#[test]
fn failed_computer_use_cleanup_keeps_checkpoint_history_for_retry() {
    let _test_state = crate::test_support::global_state();
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    save(&paths, ID, &Record::default()).unwrap();
    crate::computer_use::set_approval(&paths, ID, crate::computer_use::Approval::Auto).unwrap();
    let policy_directory = paths.metadata.with_file_name("computer-use");
    fs::set_permissions(&policy_directory, fs::Permissions::from_mode(0o500)).unwrap();
    let result = forget_removed(&paths, ID);
    fs::set_permissions(&policy_directory, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err(), "cleanup reported success: {result:?}");
    assert!(path(&paths, ID).is_file(), "cleanup lost its retry record");
    assert_eq!(
        crate::computer_use::settings(&paths, ID).approval,
        crate::computer_use::Approval::Auto
    );
    forget_removed(&paths, ID).unwrap();
    assert!(!path(&paths, ID).exists());
    assert!(!policy_directory.join(format!("{ID}.json")).exists());
    forget_removed(&paths, ID).unwrap();
}

#[test]
fn export_source_resolves_checkpoint_member_and_rejects_unknown_or_inflight() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer_configuration()],
        },
    )
    .unwrap();
    let mut record = Record::default();
    record.snapshot_group = Some("dev".into());
    record.checkpoints.push(Checkpoint {
        id: "c000000000000000000000000000001".into(),
        native_id: Some("c0000000000000000000000000000aa".into()),
        name: "Milestone".into(),
        created_at: 1,
        scope: "full".into(),
        reason: "manual".into(),
    });
    save(&paths, ID, &record).unwrap();
    let (group, member, scope, name) =
        export_source(&paths, ID, "c000000000000000000000000000001").unwrap();
    assert_eq!(group, "dev");
    assert_eq!(member, "c0000000000000000000000000000aa");
    assert_eq!(scope, "full");
    assert_eq!(name, "Milestone");
    // Unknown ids are rejected.
    assert!(export_source(&paths, ID, "c000000000000000000000000000999").is_err());
    // An inflight checkpoint cannot be exported until capture finishes.
    let mut inflight = record;
    inflight.inflight_checkpoint = Some(Checkpoint {
        id: "c000000000000000000000000000002".into(),
        native_id: None,
        name: "Capturing".into(),
        created_at: 2,
        scope: "disk".into(),
        reason: "manual".into(),
    });
    save(&paths, ID, &inflight).unwrap();
    assert!(export_source(&paths, ID, "c000000000000000000000000000002").is_err());
}

#[test]
fn imported_disk_snapshot_uses_native_cold_boot_without_full_checkpoint_flag() {
    let _test_state = crate::test_support::global_state();
    struct DiskRunner(Mutex<Vec<Vec<String>>>);
    impl RuntimeRunner for DiskRunner {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.0.lock().unwrap().push(args.to_vec());
            let stdout = match args.first().map(String::as_str) {
                Some("snapshot") => serde_json::json!([{
                    "group":"silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
                    "name":"silo-backup-0-330418-1790360984903",
                    "scope":"disk", "availability":"ready"
                }])
                .to_string(),
                Some("list") => "[]".into(),
                Some("restore") => return Err(error("synthetic restore failure")),
                // After a failed restore Silo checks whether the attempt ran.
                Some("inspect") => return Err(error("computer not found: dev")),
                _ => panic!("unexpected command: {args:?}"),
            };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    import_pending_restore(
        &paths,
        ID,
        "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
        "silo-backup-0-330418-1790360984903",
    )
    .unwrap();
    let runner = DiskRunner(Mutex::new(Vec::new()));
    assert!(start_pending(&runner, &paths, &computer_configuration())
        .unwrap_err()
        .to_string()
        .contains("synthetic restore failure"));
    let calls = runner.0.lock().unwrap();
    let restore = calls
        .iter()
        .find(|args| args.first().is_some_and(|arg| arg == "restore"))
        .unwrap();
    assert_eq!(
        restore[1],
        "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9:silo-backup-0-330418-1790360984903"
    );
    assert!(!restore
        .iter()
        .any(|arg| arg == "--disk-only" || arg == "--cow-mem"));
    assert!(restore.windows(2).any(|args| args == ["--cpus", "1"]));
    assert!(restore.windows(2).any(|args| args == ["--memory", "1G"]));
    assert_eq!(
        load(&paths, ID).unwrap().snapshot_group.as_deref(),
        Some("silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9")
    );
}

struct ScopeProbe {
    scope: &'static str,
    probe: RestoreProbe,
}
impl RuntimeRunner for ScopeProbe {
    fn run(
        &self,
        paths: &RuntimePaths,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        let mut output = self.probe.run(paths, args, timeout)?;
        if args[0] == "snapshot" {
            let mut entries: Value = serde_json::from_str(&output.stdout).unwrap();
            entries[0]["scope"] = self.scope.into();
            output.stdout = entries.to_string();
        }
        Ok(output)
    }
}

#[test]
fn checkpoint_start_selects_native_scope_and_desired_restore_mode_separately() {
    let _test_state = crate::test_support::global_state();
    for (native_scope, desired_mode, disk_only, cow_mem) in [
        ("full", "disk", true, false),
        ("disk", "disk", false, false),
        ("full", "full", false, true),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        pending_import(&paths);
        let mut record = load(&paths, ID).unwrap();
        record.pending_checkpoint_restore.as_mut().unwrap().state = desired_mode.into();
        save(&paths, ID, &record).unwrap();
        let runner = ScopeProbe {
            scope: native_scope,
            probe: RestoreProbe(Mutex::new(Vec::new())),
        };
        assert!(start_pending(&runner, &paths, &computer_configuration())
            .unwrap_err()
            .to_string()
            .contains("synthetic restore failure"));
        let calls = runner.probe.0.lock().unwrap();
        let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
        assert_eq!(
            restore.iter().any(|arg| arg == "--disk-only"),
            disk_only,
            "native={native_scope}, desired={desired_mode}: {restore:?}"
        );
        assert_eq!(restore.iter().any(|arg| arg == "--cow-mem"), cow_mem);
        assert_eq!(
            restore.iter().any(|arg| arg == "--cpus"),
            desired_mode == "disk"
        );
    }
}

#[test]
fn checkpoint_start_reapplies_github_environment_for_current_host_profiles() {
    let _test_state = crate::test_support::global_state();
    for scope in ["disk", "full"] {
        for authorized in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            pending_import(&paths);
            let mut record = load(&paths, ID).unwrap();
            record.pending_checkpoint_restore.as_mut().unwrap().state = scope.into();
            save(&paths, ID, &record).unwrap();
            let profile = if authorized {
                serde_json::json!({"enabled":true,"readToken":"synthetic-current-token"})
                    .to_string()
            } else {
                DISABLED_GITHUB_PROFILE.into()
            };
            GITHUB_PROFILES
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .unwrap()
                .insert((paths.home.clone(), "dev".into()), profile.clone());
            let runner = ScopeProbe {
                scope,
                probe: RestoreProbe(Mutex::new(Vec::new())),
            };
            assert!(start_pending(&runner, &paths, &computer_configuration())
                .unwrap_err()
                .to_string()
                .contains("synthetic restore failure"));
            let calls = runner.probe.0.lock().unwrap();
            let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
            assert!(
                restore
                    .windows(2)
                    .any(|pair| pair == ["--env", "GH_TOKEN=$MSB_SILO_GITHUB"]),
                "scope={scope}, authorized={authorized}: {restore:?}"
            );
            assert_eq!(github_environment(&paths, restore), profile);
            assert!(!restore
                .iter()
                .any(|arg| arg.contains("synthetic-current-token")));
            GITHUB_PROFILES
                .get()
                .unwrap()
                .lock()
                .unwrap()
                .remove(&(paths.home.clone(), "dev".into()));
        }
    }
}

#[test]
fn an_import_preserves_environment_until_an_identity_is_saved() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let config = serde_json::json!({"env":[
        {"key":"PROJECT_MODE","value":"portable value=with spaces"},
        {"key":"-PORTABLE_FLAG","value":"literal"},
        {"key":"GIT_AUTHOR_NAME","value":"Old Author"},
        {"key":"GIT_COMMITTER_EMAIL","value":"old@example.test"},
        {"key":"JJ_USER","value":"Old Author"},
        {"key":"GH_TOKEN","value":"synthetic-old-token"}
    ]});
    import_pending_restore_with_environment(
        &paths,
        ID,
        "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
        "silo-backup-0-330418-1790360984903",
        &config,
    )
    .unwrap();
    assert_eq!(load(&paths, ID).unwrap().desired_environment.len(), 5);
    let runner = RestoreProbe(Mutex::new(Vec::new()));
    assert!(start_pending(&runner, &paths, &computer_configuration())
        .unwrap_err()
        .to_string()
        .contains("synthetic restore failure"));
    let calls = runner.0.lock().unwrap();
    let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
    for identity in [
        "--env=GIT_AUTHOR_NAME=Old Author",
        "--env=GIT_COMMITTER_EMAIL=old@example.test",
        "--env=JJ_USER=Old Author",
    ] {
        assert!(restore.iter().any(|arg| arg == identity));
    }
    drop(calls);
    forget_identity_environment(&paths, ID).unwrap();
    assert_eq!(load(&paths, ID).unwrap().desired_environment.len(), 2);
    let runner = RestoreProbe(Mutex::new(Vec::new()));
    assert!(start_pending(&runner, &paths, &computer_configuration()).is_err());
    let calls = runner.0.lock().unwrap();
    let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
    assert!(restore
        .iter()
        .any(|arg| arg == "--env=PROJECT_MODE=portable value=with spaces"));
    assert!(restore
        .iter()
        .any(|arg| arg == "--env=-PORTABLE_FLAG=literal"));
    assert!(restore
        .windows(2)
        .any(|pair| pair == ["--env", "GH_TOKEN=$MSB_SILO_GITHUB"]));
    assert!(!restore.iter().any(|arg| arg.contains("Old Author")
        || arg.contains("old@example.test")
        || arg.contains("GIT_")
        || arg.contains("JJ_")));
    assert!(!fs::read_to_string(path(&paths, ID))
        .unwrap()
        .contains("synthetic-old-token"));
    assert!(!restore
        .iter()
        .any(|arg| arg.contains("synthetic-old-token")));
    for env in [
        serde_json::json!([{"key":"BAD=KEY","value":"value"}]),
        serde_json::json!([{"key":"KEY","value":"bad\u{0000}value"}]),
        serde_json::json!([{"key":"KEY","value":"value","extra":true}]),
    ] {
        assert!(import_pending_restore_with_environment(
            &paths,
            ID,
            "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
            "silo-backup-0-330418-1790360984903",
            &serde_json::json!({"env":env})
        )
        .is_err());
    }
}

#[test]
fn a_fork_after_an_identity_save_inherits_the_cleaned_environment() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, fork) = fork_fixture(&directory);
    let mut record = load(&paths, ID).unwrap();
    record.desired_environment = vec![
        Environment {
            key: "PROJECT_MODE".into(),
            value: "kept".into(),
        },
        Environment {
            key: "GIT_AUTHOR_NAME".into(),
            value: "Old Author".into(),
        },
        Environment {
            key: "JJ_EMAIL".into(),
            value: "old@example.test".into(),
        },
    ];
    save(&paths, ID, &record).unwrap();
    forget_identity_environment(&paths, ID).unwrap();
    forget_identity_environment(&paths, "11111111-1111-4111-8111-111111111111").unwrap();
    fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &FakeAssignments::new(&[]),
        &fork,
        "branch",
    )
    .unwrap();
    let metadata = read_metadata(&paths.metadata).unwrap();
    let child = metadata
        .computers
        .iter()
        .find(|configuration| configuration.name() == "branch")
        .unwrap();
    for id in [ID, child.id()] {
        let keys: Vec<String> = load(&paths, id)
            .unwrap()
            .desired_environment
            .into_iter()
            .map(|entry| entry.key)
            .collect();
        assert_eq!(keys, ["PROJECT_MODE"]);
    }
}

/// Records every command, answers like a runtime whose restore fails.
struct RestoreProbe(Mutex<Vec<Vec<String>>>);
impl RuntimeRunner for RestoreProbe {
    fn run(
        &self,
        _paths: &RuntimePaths,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.0.lock().unwrap().push(args.to_vec());
        let stdout = match args.first().map(String::as_str) {
            Some("snapshot") => serde_json::json!([{
                "group":"silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
                "name":"silo-backup-0-330418-1790360984903",
                "scope":"disk", "availability":"ready"
            }])
            .to_string(),
            Some("list") => "[]".into(),
            Some("restore") => return Err(error("synthetic restore failure")),
            Some("inspect") => return Err(error("computer not found: dev")),
            _ => panic!("unexpected command: {args:?}"),
        };
        Ok(CommandOutput {
            stdout,
            stderr: String::new(),
        })
    }
}

fn built_in_computer() -> ComputerConfiguration {
    let mut configuration = computer_configuration();
    {
        let ComputerConfiguration { desktop, .. } = &mut configuration;
        *desktop = Some(crate::desktop::DesktopConfiguration {
            start_with_computer: true,
            built_in: true,
        });
    }
    configuration
}

fn pending_import(paths: &RuntimePaths) {
    import_pending_restore(
        paths,
        ID,
        "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
        "silo-backup-0-330418-1790360984903",
    )
    .unwrap();
}

#[test]
fn restoring_a_built_in_computer_passes_the_computer_use_mount_again() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let published = directory.path().join("published");
    std::fs::create_dir(&published).unwrap();
    // Empty on purpose: the ChatGPT app may still be downloading.
    assert_eq!(std::fs::read_dir(&published).unwrap().count(), 0);
    crate::computer_use::set_test_published_dir(Some(published.clone()));
    pending_import(&paths);
    let runner = RestoreProbe(Mutex::new(Vec::new()));
    assert!(start_pending(&runner, &paths, &built_in_computer()).is_err());
    let calls = runner.0.lock().unwrap();
    let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
    let mount = format!("{}:/opt/silo/chatgpt:ro,uid=0,gid=0", published.display());
    assert!(
        restore
            .windows(2)
            .any(|pair| pair == ["-v", mount.as_str()]),
        "{restore:?}"
    );
    crate::computer_use::set_test_published_dir(None);
}

#[test]
fn restoring_a_computer_without_built_in_computer_use_adds_no_mount() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    crate::computer_use::set_test_published_dir(Some(directory.path().to_path_buf()));
    pending_import(&paths);
    let runner = RestoreProbe(Mutex::new(Vec::new()));
    assert!(start_pending(&runner, &paths, &computer_configuration()).is_err());
    let calls = runner.0.lock().unwrap();
    let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
    assert!(!restore
        .iter()
        .any(|arg| arg == "-v" || arg.contains("/opt/silo")));
    crate::computer_use::set_test_published_dir(None);
}

const ATTEMPT: &str = "6b79cf8f-70b3-4d2f-93d1-3b8b7a7c0001";

/// A runtime where the rejected restore left a stopped, already-run attempt with
/// the computer's labels but without the computer-use mount.
struct RejectedRestore(Mutex<Vec<Vec<String>>>);
impl RuntimeRunner for RejectedRestore {
    fn run(
        &self,
        _paths: &RuntimePaths,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.0.lock().unwrap().push(args.to_vec());
        let stdout = match args.first().map(String::as_str) {
            Some("snapshot") => serde_json::json!([{
                "group":"silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
                "name":"silo-backup-0-330418-1790360984903",
                "scope":"disk", "availability":"ready"
            }])
            .to_string(),
            Some("list") => serde_json::json!([{"name":"dev"}]).to_string(),
            Some("inspect") => serde_json::json!({"name":"dev","status":"Stopped","config":{
                "labels":{"silo.managed":"true","silo.machine-id":ID,
                    "silo.restore-attempt":ATTEMPT},
                "mounts":[{"type":"Owned","guest":"/workspace"}],
            }})
            .to_string(),
            _ => panic!("a retry without the mount must not change the computer: {args:?}"),
        };
        Ok(CommandOutput {
            stdout,
            stderr: String::new(),
        })
    }
}

#[test]
fn retrying_a_rejected_restore_still_requires_the_computer_use_mount() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let published = directory.path().join("published");
    std::fs::create_dir(&published).unwrap();
    crate::computer_use::set_test_published_dir(Some(published));
    pending_import(&paths);
    let mut record = load(&paths, ID).unwrap();
    record.restore_attempted = true;
    record.restore_attempt_ran = true;
    record.restore_attempt_id = Some(ATTEMPT.into());
    save(&paths, ID, &record).unwrap();
    let runner = RejectedRestore(Mutex::new(Vec::new()));
    let failure = start_pending(&runner, &paths, &built_in_computer())
        .unwrap_err()
        .to_string();
    assert!(failure.contains("computer-use folder"), "{failure}");
    // No command changed the computer, and the recovery state survives for the next Retry.
    assert!(runner
        .0
        .lock()
        .unwrap()
        .iter()
        .all(|args| matches!(args[0].as_str(), "snapshot" | "list" | "inspect")));
    let kept = load(&paths, ID).unwrap();
    assert!(kept.pending_checkpoint_restore.is_some());
    assert!(kept.restore_attempted && kept.restore_attempt_ran);
    assert_eq!(kept.restore_attempt_id.as_deref(), Some(ATTEMPT));
    crate::computer_use::set_test_published_dir(None);
}

#[test]
fn a_built_in_computer_cannot_restore_without_the_shared_folder() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    crate::computer_use::set_test_published_dir(None);
    pending_import(&paths);
    let runner = RestoreProbe(Mutex::new(Vec::new()));
    let failure = start_pending(&runner, &paths, &built_in_computer()).unwrap_err();
    assert!(failure.to_string().contains("shared ChatGPT folder"));
    // Nothing ran and the pending restore is untouched.
    assert!(runner.0.lock().unwrap().is_empty());
    assert!(load(&paths, ID)
        .unwrap()
        .pending_checkpoint_restore
        .is_some());
}

#[test]
fn snapshot_group_is_persisted_and_import_provenance_is_not_replaced() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    assert_eq!(ensure_snapshot_group(&paths, ID, "dev").unwrap(), "dev");
    assert_eq!(ensure_snapshot_group(&paths, ID, "renamed").unwrap(), "dev");

    let imported_id = "00000000-0000-4000-8000-000000000002";
    let imported_group = "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9";
    import_pending_restore(
        &paths,
        imported_id,
        imported_group,
        "silo-backup-0-330418-1790360984903",
    )
    .unwrap();
    assert_eq!(
        ensure_snapshot_group(&paths, imported_id, "archive-copy").unwrap(),
        imported_group
    );
}

#[test]
fn original_v1_checkpoints_migrate_to_default_group_but_lost_import_group_fails_closed() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let mut original = Record::default();
    original.checkpoints.push(Checkpoint {
        id: "c000000000000000000000000000000".into(),
        native_id: None,
        name: "Before update".into(),
        created_at: 1,
        scope: "disk".into(),
        reason: "manual".into(),
    });
    save(&paths, ID, &original).unwrap();
    assert_eq!(ensure_snapshot_group(&paths, ID, "dev").unwrap(), "dev");
    assert_eq!(
        load(&paths, ID).unwrap().snapshot_group.as_deref(),
        Some("dev")
    );

    let imported_id = "00000000-0000-4000-8000-000000000003";
    let lost_origin = Record::default();
    save(&paths, imported_id, &lost_origin).unwrap();
    assert!(ensure_snapshot_group(&paths, imported_id, "renamed-import").is_err());
    assert_eq!(load(&paths, imported_id).unwrap().snapshot_group, None);
}

struct Runner {
    mount_type: &'static str,
    calls: Mutex<Vec<Vec<String>>>,
}
impl RuntimeRunner for Runner {
    fn run(
        &self,
        _paths: &RuntimePaths,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.calls.lock().unwrap().push(args.to_vec());
        let stdout = if args[0] == "inspect" {
            serde_json::json!({
                "name":"dev", "status":"Running", "config":{
                    "labels":{"silo.managed":"true","silo.machine-id":ID},
                    "mounts":[{"guest":"/workspace","type":self.mount_type,
                        "storage":{"kind":"disk","capacity_mib":1024}}]
                }
            })
            .to_string()
        } else if args[0] == "snapshot" && args[1] == "list" {
            let calls = self.calls.lock().unwrap();
            let name = calls
                .iter()
                .find(|call| call.get(1).map(String::as_str) == Some("create"))
                .and_then(|call| call.get(2))
                .cloned()
                .unwrap_or_default();
            serde_json::json!([{"group":"dev","name":name,"scope":"full","availability":"ready"}])
                .to_string()
        } else {
            String::new()
        };
        Ok(CommandOutput {
            stdout,
            stderr: String::new(),
        })
    }
}
fn paths(directory: &tempfile::TempDir) -> RuntimePaths {
    crate::test_support::paths(directory.path())
}
fn computer_configuration() -> ComputerConfiguration {
    ComputerConfiguration {
        id: ID.into(),
        name: "dev".into(),
        cpus: 1,
        max_cpus: 1,
        memory_gib: 1,
        max_memory_gib: 1,
        workspace_storage_gib: 1,
        runtime_storage_gib: 1,
        desktop: None,
    }
}
#[test]
fn fork_from_recovery_uses_saved_policy_only_while_source_is_pending_and_absent() {
    let _test_state = crate::test_support::global_state();
    struct SourceRunner {
        present: bool,
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl RuntimeRunner for SourceRunner {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let stdout = match args[0].as_str() {
                "list" => serde_json::json!([{"name":"dev","status":"Stopped"}]).to_string(),
                "inspect" => {
                    return Err(RuntimeError::Unavailable("computer not found: dev".into()))
                }
                _ => panic!("unexpected runtime command: {args:?}"),
            };
            Ok(CommandOutput {
                stdout: if self.present { stdout } else { "[]".into() },
                stderr: String::new(),
            })
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let mut record = Record::default();
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: "c000000000000000000000000000000".into(),
        source_computer: "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9".into(),
        state: "full".into(),
    });
    let policy = serde_json::json!({"default_egress":"deny","default_ingress":"allow","rules":[]});
    record.desired_network_policy = Some(policy.clone());
    let absent = SourceRunner {
        present: false,
        calls: Mutex::new(Vec::new()),
    };
    assert_eq!(
        fork_source_policy(&absent, &paths, &computer_configuration(), &record).unwrap(),
        policy
    );
    assert_eq!(
        absent
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|args| args[0].as_str())
            .collect::<Vec<_>>(),
        ["list"]
    );
    let present = SourceRunner {
        present: true,
        calls: Mutex::new(Vec::new()),
    };
    assert!(fork_source_policy(&present, &paths, &computer_configuration(), &record).is_err());
    assert_eq!(
        present
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|args| args[0].as_str())
            .collect::<Vec<_>>(),
        ["list", "inspect"]
    );
    record.desired_network_policy = None;
    assert!(fork_source_policy(&absent, &paths, &computer_configuration(), &record).is_err());
    record.desired_network_policy = Some(serde_json::json!({"default_egress":"invalid"}));
    assert!(fork_source_policy(&absent, &paths, &computer_configuration(), &record).is_err());
}
#[test]
fn captures_full_state_only_with_an_owned_workspace_disk() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer_configuration()],
        },
    )
    .unwrap();
    let legacy = Runner {
        mount_type: "DiskImage",
        calls: Mutex::new(Vec::new()),
    };
    assert!(capture_with(&legacy, &paths, ID, "Before work", "manual")
        .unwrap_err()
        .to_string()
        .contains("migration"));
    assert_eq!(legacy.calls.lock().unwrap().len(), 1);
    let owned = Runner {
        mount_type: "Owned",
        calls: Mutex::new(Vec::new()),
    };
    capture_with(&owned, &paths, ID, "Before work", "manual").unwrap();
    let calls = owned.calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[1][..2], ["snapshot", "create"]);
    assert!(calls[1].iter().any(|arg| arg == "--full"));
    assert!(calls[1].windows(2).any(|pair| pair == ["--group", "dev"]));
    assert_eq!(load(&paths, ID).unwrap().checkpoints[0].scope, "full");
}

#[test]
fn pending_restore_checkpoint_creation_aliases_immutable_full_snapshot() {
    let _test_state = crate::test_support::global_state();
    struct SnapshotInventory {
        present: bool,
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl RuntimeRunner for SnapshotInventory {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let stdout = match args[0].as_str() {
                    "list" if self.present => serde_json::json!([{"name":"dev"}]).to_string(),
                    "list" => "[]".into(),
                    "snapshot" if args.get(1).is_some_and(|value| value == "list") => serde_json::json!([
                        {"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"}
                    ]).to_string(),
                    _ => panic!("unexpected runtime command: {args:?}"),
                };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer_configuration()],
        },
    )
    .unwrap();
    let mut pending = Record::default();
    pending.snapshot_group = Some("dev".into());
    pending.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: "c000000000000000000000000000000".into(),
        source_computer: "dev".into(),
        state: "full".into(),
    });
    pending.desired_network_policy = Some(serde_json::json!({
        "default_egress":"deny", "default_ingress":"deny", "rules":[]
    }));
    save(&paths, ID, &pending).unwrap();
    let runner = SnapshotInventory {
        present: false,
        calls: Mutex::new(Vec::new()),
    };
    capture_with(&runner, &paths, ID, "After restore", "manual").unwrap();
    let stored = load(&paths, ID).unwrap();
    let alias = &stored.checkpoints[0];
    assert_ne!(alias.id, "c000000000000000000000000000000");
    assert_eq!(
        alias.native_id.as_deref(),
        Some("c000000000000000000000000000000")
    );
    assert_eq!(alias.scope, "full");
    assert_eq!(
        stored.pending_checkpoint_restore.unwrap().checkpoint_id,
        "c000000000000000000000000000000"
    );
    assert!(runner
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|args| args[0] != "inspect" && args.get(1).map(String::as_str) != Some("create")));

    let mut legacy = Record::default();
    legacy.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: "c000000000000000000000000000000".into(),
        source_computer: "dev".into(),
        state: "full".into(),
    });
    legacy.desired_network_policy = stored.desired_network_policy;
    save(&paths, ID, &legacy).unwrap();
    capture_with(&runner, &paths, ID, "Legacy alias", "manual").unwrap();
    assert_eq!(
        load(&paths, ID).unwrap().snapshot_group.as_deref(),
        Some("dev")
    );

    let before_collision = load(&paths, ID).unwrap();
    let collision = SnapshotInventory {
        present: true,
        calls: Mutex::new(Vec::new()),
    };
    assert!(capture_with(&collision, &paths, ID, "Unsafe alias", "manual").is_err());
    assert_eq!(
        load(&paths, ID).unwrap().checkpoints.len(),
        before_collision.checkpoints.len()
    );
    assert!(!collision
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|args| args[0] == "inspect"));
}

#[test]
fn pending_full_checkpoint_can_be_aliased_switched_recovered_and_started_explicitly() {
    let _test_state = crate::test_support::global_state();
    struct Inventory {
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl RuntimeRunner for Inventory {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let stdout = match args[0].as_str() {
                    "list" => "[]".into(),
                    "snapshot" if args.get(1).is_some_and(|value| value == "list") => serde_json::json!([
                        {"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"},
                        {"group":"dev","name":"c111111111111111111111111111111","scope":"full","availability":"ready"}
                    ]).to_string(),
                    "snapshot" if args.get(1).is_some_and(|value| value == "verify") => "{}".into(),
                    "restore" => return Err(error("synthetic restore failure after request capture")),
                    "inspect" => return Err(error("computer not found: dev")),
                    _ => panic!("unexpected runtime command: {args:?}"),
                };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer_configuration()],
        },
    )
    .unwrap();
    let first = Checkpoint {
        id: "c000000000000000000000000000000".into(),
        native_id: None,
        name: "First full state".into(),
        created_at: 1,
        scope: "full".into(),
        reason: "manual".into(),
    };
    let second = Checkpoint {
        id: "c111111111111111111111111111111".into(),
        native_id: None,
        name: "Second full state".into(),
        created_at: 2,
        scope: "full".into(),
        reason: "manual".into(),
    };
    let mut record = Record::default();
    record.snapshot_group = Some("dev".into());
    record.checkpoints = vec![first.clone(), second.clone()];
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: first.id.clone(),
        source_computer: "dev".into(),
        state: "full".into(),
    });
    record.desired_network_policy = Some(serde_json::json!({
        "default_egress":"deny", "default_ingress":"allow", "rules":[]
    }));
    save(&paths, ID, &record).unwrap();
    let runner = Inventory {
        calls: Mutex::new(Vec::new()),
    };

    capture_with(&runner, &paths, ID, "Saved first state", "manual").unwrap();
    let alias = load(&paths, ID).unwrap().checkpoints[0].clone();
    assert_eq!(alias.native_id(), first.id);
    assert_eq!(alias.scope, "full");

    restore_with(&runner, &paths, ID, &second.id).unwrap();
    let after_switch = load(&paths, ID).unwrap();
    let recovery_of_first = after_switch.checkpoints[0].clone();
    assert_ne!(recovery_of_first.id, first.id);
    assert_eq!(recovery_of_first.name, "Before restore");
    assert_eq!(recovery_of_first.reason, "before-restore");
    assert_eq!(recovery_of_first.native_id(), first.id);
    assert_eq!(recovery_of_first.scope, "full");
    assert_eq!(
        after_switch
            .pending_checkpoint_restore
            .unwrap()
            .checkpoint_id,
        second.id
    );

    restore_with(&runner, &paths, ID, &recovery_of_first.id).unwrap();
    let restored = load(&paths, ID).unwrap();
    assert_eq!(
        restored
            .pending_checkpoint_restore
            .as_ref()
            .unwrap()
            .checkpoint_id,
        first.id
    );
    assert!(restored.checkpoints.iter().any(|checkpoint| {
        checkpoint.native_id() == second.id && checkpoint.reason == "before-restore"
    }));

    let device = super::super::DeviceResources {
        logical_cpus: 8,
        physical_memory_bytes: Some(16 * 1024 * 1024 * 1024),
    };
    let failure =
        super::super::explicit_computer_action_with(&runner, &paths, &device, "start", "dev")
            .unwrap_err();
    assert!(failure.to_string().contains("synthetic restore failure"));
    let calls = runner.calls.lock().unwrap();
    let start = calls.iter().find(|args| args[0] == "restore").unwrap();
    assert_eq!(start[1], format!("dev:{}", first.id));
    assert!(start.iter().any(|arg| arg == "--cow-mem"));
    assert!(!start.iter().any(|arg| arg == "--disk-only"));
    assert!(load(&paths, ID)
        .unwrap()
        .pending_checkpoint_restore
        .is_some());
}

#[test]
fn current_state_fork_from_pending_full_snapshot_reuses_reference_without_starting() {
    let _test_state = crate::test_support::global_state();
    struct ForkInventory {
        present: bool,
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl RuntimeRunner for ForkInventory {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let stdout = match args[0].as_str() {
                    "list" if self.present => serde_json::json!([{"name":"dev"}]).to_string(),
                    "list" => "[]".into(),
                    "snapshot" if args.get(1).is_some_and(|value| value == "list") => serde_json::json!([
                        {"group":"dev","name":"silo-backup-0-330418-1790360984903","scope":"full","availability":"ready"}
                    ]).to_string(),
                    _ => panic!("unexpected runtime command: {args:?}"),
                };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let mut record = Record::default();
    record.snapshot_group = Some("dev".into());
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: "silo-backup-0-330418-1790360984903".into(),
        source_computer: "dev".into(),
        state: "full".into(),
    });
    record.desired_network_policy = Some(serde_json::json!({
        "default_egress":"deny", "default_ingress":"allow", "rules":[]
    }));
    let runner = ForkInventory {
        present: false,
        calls: Mutex::new(Vec::new()),
    };
    let point = pending_current_fork_point(&runner, &paths, "dev", &record, "dev")
        .unwrap()
        .unwrap();
    assert_eq!(point.checkpoint_id, "silo-backup-0-330418-1790360984903");
    assert_eq!(point.state, "full");
    assert_eq!(
        runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|args| args[0].as_str())
            .collect::<Vec<_>>(),
        ["list", "snapshot"]
    );

    let collision = ForkInventory {
        present: true,
        calls: Mutex::new(Vec::new()),
    };
    assert!(pending_current_fork_point(&collision, &paths, "dev", &record, "dev").is_err());
    assert_eq!(collision.calls.lock().unwrap().len(), 1);
}

struct Observed {
    status: &'static str,
    missing: bool,
    calls: Mutex<usize>,
}
impl RuntimeRunner for Observed {
    fn run(
        &self,
        _: &RuntimePaths,
        args: &[String],
        _: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        *self.calls.lock().unwrap() += 1;
        assert_eq!(args[0], "inspect");
        if self.missing {
            return Err(RuntimeError::Failed {
                operation: "Inspecting the computer".into(),
                exit_code: Some(1),
                detail: "computer 'dev' not found".into(),
            });
        }
        Ok(CommandOutput {
                stdout: serde_json::json!({"name":"dev","status":self.status,"config":{"labels":{"silo.managed":"true","silo.machine-id":ID}}}).to_string(),
                stderr: String::new(),
            })
    }
}
#[test]
fn pending_and_stopped_computers_read_as_stopped_and_ask_to_be_started() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer_configuration()],
        },
    )
    .unwrap();

    // Normally stopped: inspected, present, and actions say to start it.
    let stopped = Observed {
        status: "Stopped",
        missing: false,
        calls: Mutex::new(0),
    };
    assert!(matches!(
        observe_computer(&stopped, &paths, "dev").unwrap(),
        ComputerRuntime::Present(_)
    ));
    assert_eq!(
        crate::terminal::running_computer_with(&stopped, &paths, "dev")
            .err()
            .unwrap(),
        "Start dev first."
    );

    // A runtime that does not know the computer is stopped, not an error.
    let missing = Observed {
        status: "",
        missing: true,
        calls: Mutex::new(0),
    };
    assert!(matches!(
        observe_computer(&missing, &paths, "dev").unwrap(),
        ComputerRuntime::Absent
    ));

    // Pending restore: decided from Silo's record; the runtime is never asked.
    let mut record = Record::default();
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: "c000000000000000000000000000000".into(),
        source_computer: "source".into(),
        state: "full".into(),
    });
    save(&paths, ID, &record).unwrap();
    let pending = Observed {
        status: "Running",
        missing: false,
        calls: Mutex::new(0),
    };
    assert!(matches!(
        observe_computer(&pending, &paths, "dev").unwrap(),
        ComputerRuntime::Absent
    ));
    assert!(is_pending_restore(&paths, "dev"));
    assert_eq!(
        crate::terminal::running_computer_with(&pending, &paths, "dev")
            .err()
            .unwrap(),
        "Start dev first."
    );
    assert_eq!(*pending.calls.lock().unwrap(), 0);
    // Network reads its saved ports as stopped: no error, no raw inspect failure.
    let (error, ports) = crate::network::observe_saved_port_for_test(&paths, "dev", 3000);
    assert_eq!(error, None);
    assert_eq!(ports, vec![("waiting", None)]);

    // A failed restore may already own a live computer. Its pending record must
    // not hide that computer from terminal access or secret revocation checks.
    record.restore_attempted = true;
    save(&paths, ID, &record).unwrap();
    assert!(matches!(
        observe_computer(&pending, &paths, "dev").unwrap(),
        ComputerRuntime::Present(_)
    ));
    assert!(crate::terminal::running_computer_with(&pending, &paths, "dev").is_ok());
    assert_eq!(*pending.calls.lock().unwrap(), 2);
    assert!(matches!(
        observe_computer(&missing, &paths, "dev").unwrap(),
        ComputerRuntime::Absent
    ));

    // Other runtime failures still surface.
    let broken = crate::test_support::runner::ScriptedRunner::new([
        crate::test_support::runner::ExpectedCommand::error(
            ["inspect", "dev", "--format", "json"],
            RuntimeError::Unavailable("runtime down".into()),
        )
        .with_timeout(READ_TIMEOUT),
    ]);
    delete_record_for_test(&paths);
    assert!(
        matches!(observe_computer(&broken, &paths, "dev"), Err(RuntimeError::Unavailable(message)) if message == "runtime down")
    );
    broken.assert_finished();
}
fn delete_record_for_test(paths: &RuntimePaths) {
    save(paths, ID, &Record::default()).unwrap();
}
#[test]
fn pending_fork_survives_reload_and_cannot_auto_start_or_use_lifecycle_start() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer_configuration()],
        },
    )
    .unwrap();
    let mut record = Record::default();
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: "c000000000000000000000000000000".into(),
        source_computer: "source".into(),
        state: "full".into(),
    });
    save(&paths, ID, &record).unwrap();
    assert!(is_pending(&paths, ID).unwrap());
    let runner = Runner {
        mount_type: "Owned",
        calls: Mutex::new(Vec::new()),
    };
    let device = DeviceResources {
        logical_cpus: 8,
        physical_memory_bytes: Some(16 * 1024 * 1024 * 1024),
    };
    start_at_launch_with(&runner, &paths, &device, ID).unwrap();
    assert!(runner.calls.lock().unwrap().is_empty());
    assert!(computer_action_with(&runner, &paths, &device, "start", "dev").is_err());
    assert!(runner.calls.lock().unwrap().is_empty());
    assert!(run_msb(
        &paths,
        &["exec".into(), "dev".into(), "--".into(), "true".into()],
        READ_TIMEOUT
    )
    .unwrap_err()
    .to_string()
    .contains("starts from a checkpoint first"));
    let view = pending_computer(computer_configuration()).unwrap();
    assert!(matches!(view.state, ComputerState::Stopped));
}
#[test]
fn first_start_network_flags_preserve_current_asymmetric_policy() {
    let _test_state = crate::test_support::global_state();
    let config = serde_json::json!({"network":{"policy":{
        "default_egress":"deny", "default_ingress":"allow", "rules":[
            {"action":"allow","direction":"egress","destination":{"group":"host"},
                "protocols":["udp","tcp"],"ports":[{"start":53,"end":53}]},
            {"action":"allow","direction":"egress","destination":{"group":"public"},
                "protocols":[],"ports":[]}
        ]
    }}});
    let args = current_network_args(&config).unwrap();
    assert_eq!(
        args,
        [
            "--net-default-egress",
            "deny",
            "--net-default-ingress",
            "allow",
            "--net-rule",
            "allow@dns",
            "--net-rule",
            "allow@public",
        ]
    );
}
#[test]
fn failed_first_start_preserves_pending_checkpoint_and_source() {
    let _test_state = crate::test_support::global_state();
    struct FailedRestore {
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl RuntimeRunner for FailedRestore {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let stdout = match args[0].as_str() {
                    "inspect" => serde_json::json!({"name":"dev","status":"Running","config":{
                        "labels":{"silo.managed":"true","silo.machine-id":ID},
                        "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]}}
                    }}).to_string(),
                    "snapshot" => serde_json::json!([{"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"}]).to_string(),
                    "list" => serde_json::json!([{"name":"dev"}]).to_string(),
                    "restore" => return Err(RuntimeError::Failed {operation:"restore".into(),exit_code:None,detail:"synthetic failure".into()}),
                    _ => panic!("unexpected runtime command"),
                };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let mut child = computer_configuration();
    let child_id = "00000000-0000-4000-8000-000000000002";
    {
        let ComputerConfiguration { id, name, .. } = &mut child;
        *id = child_id.into();
        *name = "fork".into();
    }
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer_configuration(), child.clone()],
        },
    )
    .unwrap();
    let mut record = Record::default();
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: "c000000000000000000000000000000".into(),
        source_computer: "dev".into(),
        state: "full".into(),
    });
    record.desired_network_policy =
        Some(serde_json::json!({"default_egress":"deny","default_ingress":"allow","rules":[]}));
    save(&paths, child_id, &record).unwrap();
    let runner = FailedRestore {
        calls: Mutex::new(Vec::new()),
    };
    // The runtime's own explanation is the failure's diagnostic, not its summary.
    assert!(
        super::failure_report(&start_pending(&runner, &paths, &child).unwrap_err())
            .diagnostic
            .is_some_and(|text| text.contains("synthetic failure"))
    );
    let stored = load(&paths, child_id).unwrap();
    assert!(stored.pending_checkpoint_restore.is_some());
    assert_eq!(stored.checkpoint_operation.unwrap().status, "failed");
    assert!(read_metadata(&paths.metadata)
        .unwrap()
        .computers
        .iter()
        .any(|m| m.name() == "dev"));
    assert_eq!(
        runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|args| args[0] == "restore")
            .count(),
        1
    );
}
#[test]
fn restore_journal_distinguishes_original_before_and_after_runtime_removal() {
    let _test_state = crate::test_support::global_state();
    struct Listing {
        present: bool,
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl RuntimeRunner for Listing {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let stdout = if args[0] == "list" {
                if self.present {
                    serde_json::json!([{"name":"dev"}]).to_string()
                } else {
                    "[]".into()
                }
            } else if args[0] == "snapshot" && args[1] == "list" {
                serde_json::json!([{"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"}]).to_string()
            } else if args[0] == "restore" {
                return Err(error("synthetic restore failure"));
            } else if args[0] == "inspect" && !self.present {
                return Err(error("computer not found: dev"));
            } else {
                panic!("unexpected runtime command")
            };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer_configuration()],
        },
    )
    .unwrap();
    let target = Checkpoint {
        id: "c000000000000000000000000000000".into(),
        native_id: None,
        name: "target".into(),
        created_at: 1,
        scope: "full".into(),
        reason: "manual".into(),
    };
    let recovery = Checkpoint {
        id: "c111111111111111111111111111111".into(),
        native_id: None,
        name: "Before restore".into(),
        created_at: 2,
        scope: "full".into(),
        reason: "before-restore".into(),
    };
    let mut record = Record::default();
    record.checkpoints = vec![recovery.clone(), target.clone()];
    record.restore_journal = Some(RestoreJournal {
        target_checkpoint_id: target.id.clone(),
        recovery_checkpoint: recovery,
        prior_running: true,
        phase: "secured".into(),
    });
    record.desired_network_policy =
        Some(serde_json::json!({"default_egress":"deny","default_ingress":"allow","rules":[]}));
    save(&paths, ID, &record).unwrap();
    assert!(!pending_view(&paths, ID, true).unwrap());
    assert!(pending_view(&paths, ID, false).unwrap());
    let original = Listing {
        present: true,
        calls: Mutex::new(Vec::new()),
    };
    assert!(start_pending(&original, &paths, &computer_configuration())
        .unwrap_err()
        .to_string()
        .contains("is unfinished"));
    assert_eq!(original.calls.lock().unwrap().len(), 1);
    assert!(load(&paths, ID)
        .unwrap()
        .pending_checkpoint_restore
        .is_none());
    let removed = Listing {
        present: false,
        calls: Mutex::new(Vec::new()),
    };
    let failure = start_pending(&removed, &paths, &computer_configuration()).unwrap_err();
    assert!(
        failure.to_string().contains("synthetic restore failure"),
        "{failure}"
    );
    let after = load(&paths, ID).unwrap();
    assert!(after.pending_checkpoint_restore.is_some());
    assert!(after.restore_journal.is_none());
    assert!(!removed
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|args| args[0] == "remove"));

    let mut fork = load(&paths, ID).unwrap();
    fork.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: target.id,
        source_computer: "dev".into(),
        state: "full".into(),
    });
    save(&paths, ID, &fork).unwrap();
    let fork_failure = start_pending(&removed, &paths, &computer_configuration()).unwrap_err();
    assert!(
        fork_failure
            .to_string()
            .contains("synthetic restore failure"),
        "{fork_failure}"
    );
    let calls = removed.calls.lock().unwrap();
    assert!(calls
        .iter()
        .any(|args| args.first().map(String::as_str) == Some("restore")
            && args.get(1).map(String::as_str) == Some("dev:c000000000000000000000000000000")));
}
#[test]
fn failed_child_cleanup_requires_the_saved_attempt_label() {
    let _test_state = crate::test_support::global_state();
    struct WrongChild {
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl RuntimeRunner for WrongChild {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let stdout = match args[0].as_str() {
                    "snapshot" => serde_json::json!([{"group":"source","name":"c000000000000000000000000000000","scope":"full","availability":"ready"}]).to_string(),
                    "list" => serde_json::json!([{"name":"fork"}]).to_string(),
                    "inspect" => serde_json::json!({"name":"fork","status":"Stopped","config":{
                        "labels":{"silo.managed":"true","silo.machine-id":"00000000-0000-4000-8000-000000000002"},
                        "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]}}
                    }}).to_string(),
                    _ => panic!("cleanup must not touch an unverified child"),
                };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let mut child = computer_configuration();
    let child_id = "00000000-0000-4000-8000-000000000002";
    {
        let ComputerConfiguration { id, name, .. } = &mut child;
        *id = child_id.into();
        *name = "fork".into();
    }
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![child.clone()],
        },
    )
    .unwrap();
    let mut record = Record::default();
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: "c000000000000000000000000000000".into(),
        source_computer: "source".into(),
        state: "full".into(),
    });
    record.desired_network_policy =
        Some(serde_json::json!({"default_egress":"deny","default_ingress":"allow","rules":[]}));
    record.restore_attempted = true;
    record.restore_attempt_id = Some(uuid::Uuid::new_v4().to_string());
    save(&paths, child_id, &record).unwrap();
    let runner = WrongChild {
        calls: Mutex::new(Vec::new()),
    };
    assert!(start_pending(&runner, &paths, &child)
        .unwrap_err()
        .to_string()
        .contains("unverified"));
    assert!(!runner
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|args| args[0] == "remove"));
}
#[test]
fn restore_secures_recovery_before_retiring_original_and_keeps_stable_identity() {
    let _test_state = crate::test_support::global_state();
    struct RestoreRunner {
        state: Mutex<&'static str>,
        recovery: Mutex<Option<String>>,
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl RuntimeRunner for RestoreRunner {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let stdout=match args[0].as_str() {
                    "inspect" => serde_json::json!({"name":"dev","status":*self.state.lock().unwrap(),"config":{
                        "labels":{"silo.managed":"true","silo.machine-id":ID},
                        "mounts":[{"guest":"/workspace","type":"Owned","storage":{"kind":"disk","capacity_mib":1024}}],
                        "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]}}
                    }}).to_string(),
                    "snapshot" if args[1]=="list" => {
                        let mut entries=vec![serde_json::json!({"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"})];
                        if let Some(id)=self.recovery.lock().unwrap().as_ref() {
                            entries.push(serde_json::json!({"group":"dev","name":id,"scope":"full","availability":"ready"}));
                        }
                        serde_json::to_string(&entries).unwrap()
                    }
                    "snapshot" if args[1]=="create" => { *self.recovery.lock().unwrap()=Some(args[2].clone()); String::new() }
                    "snapshot" if args[1]=="verify" => String::new(),
                    "pause" => { *self.state.lock().unwrap()="Paused"; String::new() },
                    "stop" => { assert!(args.iter().any(|arg| arg=="--force")); *self.state.lock().unwrap()="Stopped"; String::new() },
                    "remove" => { *self.state.lock().unwrap()="Removed"; String::new() },
                    "list" => "[]".into(),
                    _ => panic!("unexpected runtime command: {args:?}"),
                };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer_configuration()],
        },
    )
    .unwrap();
    let mut record = Record::default();
    record.checkpoints.push(Checkpoint {
        id: "c000000000000000000000000000000".into(),
        native_id: None,
        name: "Selected".into(),
        created_at: 1,
        scope: "full".into(),
        reason: "manual".into(),
    });
    save(&paths, ID, &record).unwrap();
    let runner = RestoreRunner {
        state: Mutex::new("Running"),
        recovery: Mutex::new(None),
        calls: Mutex::new(Vec::new()),
    };
    restore_with(&runner, &paths, ID, "c000000000000000000000000000000").unwrap();
    let calls = runner.calls.lock().unwrap();
    let position = |command: &str| calls.iter().position(|args| args[0] == command).unwrap();
    assert!(position("pause") < position("stop"));
    assert!(position("snapshot") < position("remove"));
    let verify_recovery = calls
        .iter()
        .rposition(|args| args.get(1).map(String::as_str) == Some("verify"))
        .unwrap();
    assert!(verify_recovery < position("remove"));
    let stored = load(&paths, ID).unwrap();
    assert_eq!(stored.checkpoints[0].reason, "before-restore");
    assert_eq!(
        stored.pending_checkpoint_restore.unwrap().checkpoint_id,
        "c000000000000000000000000000000"
    );
    assert!(stored.restore_journal.is_none());
    assert_eq!(
        read_metadata(&paths.metadata).unwrap().computers[0].id(),
        ID
    );
    assert_eq!(
        read_metadata(&paths.metadata).unwrap().computers[0].name(),
        "dev"
    );
    drop(calls);
    let source = read_application_state_with(&runner, &paths).unwrap();
    assert!(matches!(source.computers[0].state, ComputerState::Stopped));
    assert!(source.computers[0].pending_checkpoint_restore.is_some());
    let calls = runner.calls.lock().unwrap();
    let remove = calls.iter().position(|args| args[0] == "remove").unwrap();
    assert!(calls[remove + 1..].iter().all(|args| args[0] == "list"));
}

struct JournalRunner {
    state: Mutex<&'static str>,
    fail: &'static str,
    listed: bool,
    recovery: Mutex<Option<String>>,
    calls: Mutex<Vec<Vec<String>>>,
}
impl RuntimeRunner for JournalRunner {
    fn run(
        &self,
        _paths: &RuntimePaths,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.calls.lock().unwrap().push(args.to_vec());
        let command = if args[0] == "snapshot" {
            args[1].as_str()
        } else {
            args[0].as_str()
        };
        if self.fail.split('|').any(|failing| failing == command) {
            return Err(RuntimeError::Unavailable(format!(
                "{command} failed on this host."
            )));
        }
        let stdout = match command {
                "inspect" => serde_json::json!({"name":"dev","status":*self.state.lock().unwrap(),"config":{
                    "labels":{"silo.managed":"true","silo.machine-id":ID},
                    "mounts":[{"guest":"/workspace","type":"Owned","storage":{"kind":"disk","capacity_mib":1024}}],
                    "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]}}
                }}).to_string(),
                "list" if args[0] == "snapshot" => {
                    let full = self.calls.lock().unwrap().iter().any(|call| call.iter().any(|arg| arg == "--full"));
                    let mut entries = vec![serde_json::json!({"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"})];
                    if let Some(id) = self.recovery.lock().unwrap().as_ref() {
                        let scope = if full { "full" } else { "disk" };
                        entries.push(serde_json::json!({"group":"dev","name":id,"scope":scope,"availability":"ready"}));
                    }
                    serde_json::to_string(&entries).unwrap()
                }
                "create" => {
                    *self.recovery.lock().unwrap() = Some(args[2].clone());
                    String::new()
                }
                "pause" => {
                    *self.state.lock().unwrap() = "Paused";
                    String::new()
                }
                "resume" => {
                    *self.state.lock().unwrap() = "Running";
                    String::new()
                }
                "stop" => {
                    *self.state.lock().unwrap() = "Stopped";
                    String::new()
                }
                "remove" => {
                    *self.state.lock().unwrap() = "Removed";
                    String::new()
                }
                "list" if self.listed => r#"[{"name":"dev"}]"#.into(),
                "list" => "[]".into(),
                _ => String::new(),
            };
        Ok(CommandOutput {
            stdout,
            stderr: String::new(),
        })
    }
}

fn journal_runner(state: &'static str, fail: &'static str) -> JournalRunner {
    JournalRunner {
        state: Mutex::new(state),
        fail,
        listed: false,
        recovery: Mutex::new(None),
        calls: Mutex::new(Vec::new()),
    }
}

fn restore_fixture(directory: &tempfile::TempDir, journal: Option<(&str, bool)>) -> RuntimePaths {
    let paths = paths(directory);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer_configuration()],
        },
    )
    .unwrap();
    let mut record = Record::default();
    record.checkpoints.push(Checkpoint {
        id: "c000000000000000000000000000000".into(),
        native_id: None,
        name: "Selected".into(),
        created_at: 1,
        scope: "full".into(),
        reason: "manual".into(),
    });
    if let Some((phase, prior_running)) = journal {
        record.restore_journal = Some(RestoreJournal {
            target_checkpoint_id: "c000000000000000000000000000000".into(),
            recovery_checkpoint: Checkpoint {
                id: "c111111111111111111111111111111".into(),
                native_id: None,
                name: "Before restore".into(),
                created_at: 2,
                scope: if prior_running { "full" } else { "disk" }.into(),
                reason: "before-restore".into(),
            },
            prior_running,
            phase: phase.into(),
        });
        record.checkpoint_operation = Some(Operation {
            kind: "restore".into(),
            status: "running".into(),
            stage: "Creating recovery checkpoint".into(),
            error: None,
        });
    }
    save(&paths, ID, &record).unwrap();
    paths
}

#[test]
fn capturing_journal_for_a_stopped_computer_secures_a_disk_recovery_and_finishes() {
    let _test_state = crate::test_support::global_state();
    for status in ["Stopped", "Created", "Crashed"] {
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, Some(("capturing", true)));
        let runner = journal_runner(status, "");
        restore_with(&runner, &paths, ID, "c000000000000000000000000000000").unwrap();
        let calls = runner.calls.lock().unwrap();
        let create = calls
            .iter()
            .find(|call| call[0] == "snapshot" && call[1] == "create")
            .unwrap();
        assert!(
            !create.iter().any(|arg| arg == "--full"),
            "{status}: {create:?}"
        );
        assert!(!calls.iter().any(|call| call[0] == "pause"));
        let stored = load(&paths, ID).unwrap();
        assert!(stored.restore_journal.is_none());
        assert!(stored.pending_checkpoint_restore.is_some());
        let recovery = stored
            .checkpoints
            .iter()
            .find(|checkpoint| checkpoint.reason == "before-restore")
            .unwrap();
        assert_eq!(recovery.scope, "disk");
    }
}

#[test]
fn failed_pause_clears_the_capturing_journal_and_records_the_real_error() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let runner = journal_runner("Running", "pause");
    let failure = restore_with(&runner, &paths, ID, "c000000000000000000000000000000")
        .unwrap_err()
        .to_string();
    assert_eq!(failure, "pause failed on this host.");
    let stored = load(&paths, ID).unwrap();
    assert!(stored.restore_journal.is_none());
    let operation = stored.checkpoint_operation.unwrap();
    assert_eq!(operation.status, "failed");
    assert_eq!(
        operation.error.as_deref(),
        Some("pause failed on this host.")
    );
    assert!(!needs_explicit_start(&paths, ID).unwrap());
}

#[test]
fn failed_capture_of_a_stopped_computer_leaves_no_journal() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, Some(("capturing", true)));
    let runner = journal_runner("Stopped", "create");
    restore_with(&runner, &paths, ID, "c000000000000000000000000000000").unwrap_err();
    let stored = load(&paths, ID).unwrap();
    assert!(stored.restore_journal.is_none());
    assert_eq!(stored.checkpoint_operation.unwrap().status, "failed");
}

#[test]
fn secured_restore_errors_persist_a_failed_status_with_the_real_error() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, Some(("secured", true)));
    let mut runner = journal_runner("Running", "");
    runner.listed = true;
    let failure = restore_with(&runner, &paths, ID, "c000000000000000000000000000000")
        .unwrap_err()
        .to_string();
    assert!(
        failure.contains("resumed after its recovery checkpoint"),
        "{failure}"
    );
    let operation = load(&paths, ID).unwrap().checkpoint_operation.unwrap();
    assert_eq!(operation.status, "failed");
    assert_eq!(operation.error.as_deref(), Some(failure.as_str()));
}

#[test]
fn crashed_computer_can_retry_a_secured_restore() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, Some(("secured", true)));
    let mut runner = journal_runner("Crashed", "");
    runner.listed = true;
    restore_with(&runner, &paths, ID, "c000000000000000000000000000000").unwrap();
    let stored = load(&paths, ID).unwrap();
    assert!(stored.restore_journal.is_none());
    assert!(stored.pending_checkpoint_restore.is_some());
}

#[test]
fn interrupted_checkpoint_is_kept_when_the_snapshot_list_fails() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let mut record = load(&paths, ID).unwrap();
    record.snapshot_group = Some("dev".into());
    record.inflight_checkpoint = Some(Checkpoint {
        id: "c222222222222222222222222222222".into(),
        native_id: None,
        name: "Interrupted".into(),
        created_at: 3,
        scope: "full".into(),
        reason: "manual".into(),
    });
    save(&paths, ID, &record).unwrap();
    let runner = journal_runner("Running", "list");
    capture_with(&runner, &paths, ID, "Next", "manual").unwrap_err();
    let stored = load(&paths, ID).unwrap();
    assert_eq!(
        stored
            .inflight_checkpoint
            .map(|checkpoint| checkpoint.id)
            .as_deref(),
        Some("c222222222222222222222222222222")
    );
}

#[test]
fn attempted_restore_with_a_listed_runtime_reads_as_a_present_computer() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let mut record = load(&paths, ID).unwrap();
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: "c000000000000000000000000000000".into(),
        source_computer: "dev".into(),
        state: "full".into(),
    });
    save(&paths, ID, &record).unwrap();
    assert!(pending_view(&paths, ID, false).unwrap());
    assert!(pending_view(&paths, ID, true).unwrap());
    record.restore_attempted = true;
    record.restore_attempt_id = Some(uuid::Uuid::new_v4().to_string());
    save(&paths, ID, &record).unwrap();
    assert!(pending_view(&paths, ID, false).unwrap());
    assert!(!pending_view(&paths, ID, true).unwrap());
    let record = load(&paths, ID).unwrap();
    assert!(view_pending(&record, "dev").is_some());
    assert!(needs_explicit_start(&paths, ID).unwrap());
}

#[test]
fn failure_record_save_errors_keep_the_original_error() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    fs::write(directory.path().join("checkpoints"), b"not a directory").unwrap();
    let failure = save_failure(
        &paths,
        ID,
        &Record::default(),
        RuntimeError::Unavailable("No space left on device.".into()),
    )
    .to_string();
    assert!(failure.starts_with("No space left on device."), "{failure}");
    assert!(failure.contains("could not be updated"));
}

struct FakeAssignments {
    fail: &'static [&'static str],
    github: Mutex<Vec<String>>,
    secrets: Mutex<Vec<String>>,
}
impl FakeAssignments {
    fn new(fail: &'static [&'static str]) -> Self {
        Self {
            fail,
            github: Mutex::new(Vec::new()),
            secrets: Mutex::new(Vec::new()),
        }
    }
    fn check(&self, step: &str) -> Result<(), String> {
        if self.fail.contains(&step) {
            Err(format!("{step} failed."))
        } else {
            Ok(())
        }
    }
}
impl ForkAssignments for FakeAssignments {
    fn copy_github(&self, _: &str, target: &str) -> Result<(), String> {
        self.check("copy_github")?;
        self.github.lock().unwrap().push(target.into());
        Ok(())
    }
    fn forget_github(&self, target: &str) -> Result<(), String> {
        self.check("forget_github")?;
        self.github.lock().unwrap().retain(|name| name != target);
        Ok(())
    }
    fn copy_secrets(&self, _: &str, target: &str) -> Result<(), String> {
        self.check("copy_secrets")?;
        self.secrets.lock().unwrap().push(target.into());
        Ok(())
    }
    fn forget_secrets(&self, target: &str) -> Result<(), String> {
        self.check("forget_secrets")?;
        self.secrets.lock().unwrap().retain(|name| name != target);
        Ok(())
    }
}

fn fork_fixture(directory: &tempfile::TempDir) -> (RuntimePaths, ForkSource) {
    let paths = restore_fixture(directory, None);
    let mut record = load(&paths, ID).unwrap();
    record.snapshot_group = Some("dev".into());
    save(&paths, ID, &record).unwrap();
    let fork = ForkSource {
        source_id: ID.into(),
        snapshot_group: "dev".into(),
        member: "c000000000000000000000000000000".into(),
        scope: "full".into(),
        desired_policy: serde_json::json!({"default_egress":"deny","default_ingress":"allow","rules":[]}),
    };
    (paths, fork)
}

fn record_ids(paths: &RuntimePaths) -> Vec<String> {
    let mut ids: Vec<String> = fs::read_dir(directory(paths))
        .unwrap()
        .filter_map(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_str()?
                .strip_suffix(".json")
                .map(str::to_owned)
        })
        .collect();
    ids.sort();
    ids
}

#[test]
fn fork_commit_adds_the_stopped_fork_and_its_assignments() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, fork) = fork_fixture(&directory);
    crate::computer_use::set_approval(&paths, ID, crate::computer_use::Approval::Auto).unwrap();
    let assignments = FakeAssignments::new(&[]);
    fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &assignments,
        &fork,
        "branch",
    )
    .unwrap();
    let metadata = read_metadata(&paths.metadata).unwrap();
    let child = metadata
        .computers
        .iter()
        .find(|configuration| configuration.name() == "branch")
        .unwrap();
    assert_eq!(
        crate::computer_use::settings(&paths, child.id()).approval,
        crate::computer_use::Approval::Auto
    );
    let pending = load(&paths, child.id())
        .unwrap()
        .pending_checkpoint_restore
        .unwrap();
    assert_eq!(pending.checkpoint_id, fork.member);
    assert_eq!(pending.source_computer, "dev");
    assert_eq!(*assignments.github.lock().unwrap(), ["branch"]);
    assert_eq!(*assignments.secrets.lock().unwrap(), ["branch"]);
}

#[test]
fn a_failed_approval_copy_does_not_publish_the_fork_or_leave_its_record() {
    let _test_state = crate::test_support::global_state();
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let (paths, fork) = fork_fixture(&directory);
    crate::computer_use::set_approval(&paths, ID, crate::computer_use::Approval::Auto).unwrap();
    let policy_directory = paths.metadata.with_file_name("computer-use");
    fs::set_permissions(&policy_directory, fs::Permissions::from_mode(0o500)).unwrap();
    let assignments = FakeAssignments::new(&[]);
    let result = fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &assignments,
        &fork,
        "branch",
    );
    fs::set_permissions(&policy_directory, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("computer-use setting"));
    assert_eq!(record_ids(&paths), [ID]);
    assert_eq!(read_metadata(&paths.metadata).unwrap().computers.len(), 1);
    assert!(assignments.github.lock().unwrap().is_empty());
    assert!(assignments.secrets.lock().unwrap().is_empty());
    assert_eq!(
        crate::computer_use::settings(&paths, ID).approval,
        crate::computer_use::Approval::Auto
    );
    assert_eq!(fs::read_dir(&policy_directory).unwrap().count(), 1);
}

#[test]
fn a_published_fork_keeps_its_checkpoint_and_assignments_after_a_durability_error() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, fork) = fork_fixture(&directory);
    let assignments = FakeAssignments::new(&[]);
    let failure = fork_commit_with_metadata_writer(
        &journal_runner("Running", ""),
        &paths,
        &assignments,
        &fork,
        "branch",
        &|path, metadata| {
            write_metadata(path, metadata)?;
            Err(RuntimeError::Unavailable("Directory sync failed.".into()))
        },
    )
    .unwrap_err()
    .to_string();
    assert!(failure.starts_with("Directory sync failed."), "{failure}");
    let metadata = read_metadata(&paths.metadata).unwrap();
    let child = metadata
        .computers
        .iter()
        .find(|configuration| configuration.name() == "branch")
        .unwrap();
    let record = load(&paths, child.id()).unwrap();
    assert_eq!(
        record.pending_checkpoint_restore.unwrap().checkpoint_id,
        fork.member
    );
    assert_eq!(record.desired_network_policy, Some(fork.desired_policy));
    assert_eq!(*assignments.github.lock().unwrap(), ["branch"]);
    assert_eq!(*assignments.secrets.lock().unwrap(), ["branch"]);
}

#[test]
fn an_unreadable_inventory_keeps_fork_dependencies_after_a_write_error() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, fork) = fork_fixture(&directory);
    let assignments = FakeAssignments::new(&[]);
    let failure = fork_commit_with_metadata_writer(
        &journal_runner("Running", ""),
        &paths,
        &assignments,
        &fork,
        "branch",
        &|path, _| {
            fs::write(path, b"{broken").unwrap();
            Err(RuntimeError::Unavailable("Inventory write failed.".into()))
        },
    )
    .unwrap_err()
    .to_string();
    assert!(failure.starts_with("Inventory write failed."), "{failure}");
    assert!(failure.contains("could not be checked"), "{failure}");
    let ids = record_ids(&paths);
    assert_eq!(ids.len(), 2);
    let child_id = ids.iter().find(|id| id.as_str() != ID).unwrap();
    assert_eq!(
        load(&paths, child_id)
            .unwrap()
            .pending_checkpoint_restore
            .unwrap()
            .checkpoint_id,
        fork.member
    );
    assert_eq!(*assignments.github.lock().unwrap(), ["branch"]);
    assert_eq!(*assignments.secrets.lock().unwrap(), ["branch"]);
}

#[test]
fn failed_fork_steps_leave_no_record_inventory_or_assignments_and_keep_the_original_error() {
    let _test_state = crate::test_support::global_state();
    for (fail, expected) in [
        (&["copy_github"][..], "copy_github failed."),
        (&["copy_secrets"][..], "copy_secrets failed."),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let (paths, fork) = fork_fixture(&directory);
        let assignments = FakeAssignments::new(fail);
        let failure = fork_commit(
            &journal_runner("Running", ""),
            &paths,
            &assignments,
            &fork,
            "branch",
        )
        .unwrap_err()
        .to_string();
        assert_eq!(failure, expected);
        assert_eq!(record_ids(&paths), [ID]);
        assert_eq!(read_metadata(&paths.metadata).unwrap().computers.len(), 1);
        assert!(assignments.github.lock().unwrap().is_empty());
        assert!(assignments.secrets.lock().unwrap().is_empty());
    }
}

#[test]
fn a_failing_fork_cleanup_is_reported_after_the_original_error() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, fork) = fork_fixture(&directory);
    let assignments = FakeAssignments::new(&["copy_secrets", "forget_github"]);
    let failure = fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &assignments,
        &fork,
        "branch",
    )
    .unwrap_err()
    .to_string();
    assert!(failure.starts_with("copy_secrets failed."), "{failure}");
    assert!(
        failure.contains("Cleanup was incomplete: forget_github failed."),
        "{failure}"
    );
    // The record is still removed although the GitHub cleanup failed.
    assert_eq!(record_ids(&paths), [ID]);
}

#[test]
fn a_failed_inventory_write_removes_the_fork_record_and_assignments() {
    let _test_state = crate::test_support::global_state();
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let (paths, fork) = fork_fixture(&directory);
    crate::computer_use::set_approval(&paths, ID, crate::computer_use::Approval::Auto).unwrap();
    let parent = paths.metadata.parent().unwrap().to_path_buf();
    // The checkpoint and approval directories exist, so only the inventory write fails.
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();
    let assignments = FakeAssignments::new(&[]);
    let result = fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &assignments,
        &fork,
        "branch",
    );
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("computer settings"));
    assert_eq!(record_ids(&paths), [ID]);
    assert_eq!(read_metadata(&paths.metadata).unwrap().computers.len(), 1);
    assert!(assignments.github.lock().unwrap().is_empty());
    assert!(assignments.secrets.lock().unwrap().is_empty());
    assert_eq!(
        fs::read_dir(paths.metadata.with_file_name("computer-use"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn fork_commit_rechecks_the_name_and_the_checkpoint_after_the_capture() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, fork) = fork_fixture(&directory);
    let assignments = FakeAssignments::new(&[]);
    assert!(fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &assignments,
        &fork,
        "dev"
    )
    .is_err());
    let missing = ForkSource {
        member: "c999999999999999999999999999999".into(),
        ..fork
    };
    assert!(fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &assignments,
        &missing,
        "branch"
    )
    .is_err());
    assert_eq!(record_ids(&paths), [ID]);
    assert!(assignments.github.lock().unwrap().is_empty());
}

#[test]
fn current_state_fork_captures_under_the_source_lane_and_writes_under_the_device_lane() {
    let _test_state = crate::test_support::global_state();
    struct LaneRunner {
        gate: &'static super::super::operation_gate::OperationGate,
        created: Mutex<Option<String>>,
        lanes: Mutex<Vec<(String, Option<String>, bool)>>,
    }
    impl RuntimeRunner for LaneRunner {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            let running = self.gate.snapshot().running;
            let lane = running.first().and_then(|entry| entry.computer_id.clone());
            // Another computer can start while this lane is held only if it is not device-wide.
            let other_computer_free = std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        self.gate
                            .try_computer("other-computer", "other", "Starting")
                            .is_ok()
                    })
                    .join()
                    .unwrap()
            });
            let command = if args[0] == "snapshot" {
                format!("snapshot {}", args[1])
            } else {
                args[0].clone()
            };
            self.lanes
                .lock()
                .unwrap()
                .push((command.clone(), lane, other_computer_free));
            let stdout = match command.as_str() {
                    "inspect" => serde_json::json!({"name":"dev","status":"Running","config":{
                        "labels":{"silo.managed":"true","silo.machine-id":ID},
                        "mounts":[{"guest":"/workspace","type":"Owned","storage":{"kind":"disk","capacity_mib":1024}}],
                        "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]}}
                    }}).to_string(),
                    "snapshot create" => {
                        *self.created.lock().unwrap() = Some(args[2].clone());
                        String::new()
                    }
                    "snapshot list" => {
                        let name = self.created.lock().unwrap().clone().unwrap_or_default();
                        serde_json::json!([{"group":"dev","name":name,"scope":"full","availability":"ready"}]).to_string()
                    }
                    _ => String::new(),
                };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let (paths, _) = fork_fixture(&directory);
    let gate: &'static super::super::operation_gate::OperationGate =
        Box::leak(Box::new(super::super::operation_gate::OperationGate::new()));
    let runner = LaneRunner {
        gate,
        created: Mutex::new(None),
        lanes: Mutex::new(Vec::new()),
    };
    let assignments = FakeAssignments::new(&[]);
    fork_in_lanes(
        gate,
        &runner,
        &paths,
        &assignments,
        ID,
        None,
        "branch",
        &Vec::new,
    )
    .unwrap();
    let lanes = runner.lanes.lock().unwrap();
    let create = lanes
        .iter()
        .find(|(command, ..)| command == "snapshot create")
        .unwrap();
    assert_eq!(
        create.1.as_deref(),
        Some(ID),
        "the capture holds only the source's lane"
    );
    assert!(
        create.2,
        "other computers are not queued behind the capture"
    );
    let last = lanes.last().unwrap();
    assert_eq!(last.0, "snapshot list");
    assert_eq!(
        last.1, None,
        "the inventory write holds the device-wide lane"
    );
    assert!(!last.2);
    assert!(read_metadata(&paths.metadata)
        .unwrap()
        .computers
        .iter()
        .any(|configuration| configuration.name() == "branch"));
}

/// A fake native store that enforces MicroSandbox's children and head guards.
struct Store {
    members: Mutex<Vec<(String, String, String, Option<String>)>>,
    heads: Mutex<HashMap<String, String>>,
    computers: Vec<(&'static str, Option<&'static str>)>,
    fail_create_after_publish: bool,
    fail_remove: bool,
    calls: Mutex<Vec<Vec<String>>>,
}
impl Store {
    fn new(computers: Vec<(&'static str, Option<&'static str>)>) -> Self {
        Self {
            members: Mutex::new(Vec::new()),
            heads: Mutex::new(HashMap::new()),
            computers,
            fail_create_after_publish: false,
            fail_remove: false,
            calls: Mutex::new(Vec::new()),
        }
    }
    fn with(self, group: &str, name: &str, id: &str, parent: Option<&str>) -> Self {
        self.members.lock().unwrap().push((
            group.into(),
            name.into(),
            id.into(),
            parent.map(str::to_owned),
        ));
        self.heads.lock().unwrap().insert(group.into(), id.into());
        self
    }
    fn names(&self) -> Vec<String> {
        self.members
            .lock()
            .unwrap()
            .iter()
            .map(|member| member.1.clone())
            .collect()
    }
    fn removals(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call[0] == "snapshot" && call[1] == "remove")
            .map(|call| call[2].clone())
            .collect()
    }
}
impl RuntimeRunner for Store {
    fn run(
        &self,
        paths: &RuntimePaths,
        args: &[String],
        _: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.calls.lock().unwrap().push(args.to_vec());
        let ok = |stdout: String| {
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        };
        match (args[0].as_str(), args.get(1).map(String::as_str)) {
                ("list", _) => ok(serde_json::Value::Array(self.computers.iter().map(|(name, _)| serde_json::json!({"name": name})).collect()).to_string()),
                ("inspect", Some(name)) => {
                    let computer_id = read_metadata(&paths.metadata)?.computers.into_iter()
                        .find(|configuration| configuration.name() == name)
                        .map(|configuration| configuration.id().to_owned())
                        .unwrap_or_else(|| ID.into());
                    let parent = self.computers.iter().find(|(listed, _)| listed == &name).and_then(|(_, parent)| *parent);
                    ok(serde_json::json!({"name": name, "status": "Running", "config": {
                        "labels": {"silo.managed": "true", "silo.machine-id": computer_id},
                        "mounts": [{"guest": "/workspace", "type": "Owned", "storage": {"kind": "disk", "capacity_mib": 1024}}],
                        "network": {"policy": {"default_egress": "deny", "default_ingress": "allow", "rules": []}},
                        "snapshot_parent": parent,
                    }}).to_string())
                }
                ("snapshot", Some("list")) => ok(serde_json::Value::Array(self.members.lock().unwrap().iter().map(|(group, name, id, parent)| serde_json::json!({
                    "snapshot_id": id, "name": name, "group": group, "parent_digest": parent,
                    "scope": "full", "availability": "ready", "created_at": "2026-01-01T00:00:00+00:00",
                })).collect()).to_string()),
                ("snapshot", Some("head")) => {
                    let selector = &args[2];
                    if let Some((group, id)) = selector.split_once(':') {
                        self.heads.lock().unwrap().insert(group.into(), id.into());
                    }
                    let group = selector.split(':').next().unwrap();
                    ok(serde_json::json!({"group": group, "head": self.heads.lock().unwrap().get(group)}).to_string())
                }
                ("snapshot", Some("remove")) => {
                    let (group, name) = args[2].split_once(':').unwrap();
                    let mut members = self.members.lock().unwrap();
                    let index = members.iter().position(|member| member.0 == group && member.1 == name)
                        .ok_or_else(|| RuntimeError::Unavailable("snapshot not found".into()))?;
                    let id = members[index].2.clone();
                    if self.fail_remove {
                        return Err(RuntimeError::Unavailable("permission denied".into()));
                    }
                    if members.iter().any(|member| member.3.as_deref() == Some(id.as_str())) {
                        return Err(RuntimeError::Unavailable("snapshot has indexed children; pass --force".into()));
                    }
                    let mut heads = self.heads.lock().unwrap();
                    if heads.get(group) == Some(&id) {
                        if members.iter().filter(|member| member.0 == group).count() > 1 {
                            return Err(RuntimeError::Unavailable("cannot remove current head".into()));
                        }
                        heads.remove(group);
                    }
                    members.remove(index);
                    ok(String::new())
                }
                ("snapshot", Some("create")) => {
                    let group = args[args.iter().position(|arg| arg == "--group").unwrap() + 1].clone();
                    let id = format!("snap_{:032x}", self.members.lock().unwrap().len() + 100);
                    self.members.lock().unwrap().push((group.clone(), args[2].clone(), id.clone(), None));
                    self.heads.lock().unwrap().entry(group).or_insert(id);
                    if self.fail_create_after_publish {
                        return Err(RuntimeError::Cancelled { operation: "snapshot create".into() });
                    }
                    ok(String::new())
                }
                _ => ok(String::new()),
            }
    }
}

const A: &str = "c000000000000000000000000000000";
const B: &str = "c111111111111111111111111111111";
const C: &str = "c222222222222222222222222222222";
const FORK_ID: &str = "00000000-0000-4000-8000-000000000009";

fn entry(id: &str, name: &str, reason: &str) -> Checkpoint {
    Checkpoint {
        id: id.into(),
        native_id: None,
        name: name.into(),
        created_at: 1,
        scope: "full".into(),
        reason: reason.into(),
    }
}

/// `dev` with checkpoints; `fork` adds a second configured computer and its record.
fn delete_fixture(
    directory: &tempfile::TempDir,
    entries: Vec<Checkpoint>,
    fork: Option<Record>,
) -> RuntimePaths {
    let paths = paths(directory);
    let mut computers = vec![computer_configuration()];
    if let Some(record) = &fork {
        let ComputerConfiguration {
            cpus,
            max_cpus,
            memory_gib,
            max_memory_gib,
            workspace_storage_gib,
            runtime_storage_gib,
            desktop,
            ..
        } = computer_configuration();
        computers.push(ComputerConfiguration {
            id: FORK_ID.into(),
            name: "branch".into(),
            cpus,
            max_cpus,
            memory_gib,
            max_memory_gib,
            workspace_storage_gib,
            runtime_storage_gib,
            desktop,
        });
        save(&paths, FORK_ID, record).unwrap();
    }
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers,
        },
    )
    .unwrap();
    let mut record = Record::default();
    record.snapshot_group = Some("dev".into());
    record.checkpoints = entries;
    save(&paths, ID, &record).unwrap();
    paths
}

fn cursor(paths: &RuntimePaths, computer: &str, snapshot: &str) {
    let directory = paths.home.join("sandboxes").join(computer);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("snapshot-lineage.json"),
        serde_json::json!({"computer_id": 7, "snapshot_id": snapshot}).to_string(),
    )
    .unwrap();
}

#[test]
fn removal_plan_goes_leaves_first_and_keeps_used_positioned_and_parent_members() {
    let _test_state = crate::test_support::global_state();
    let member = |name: &str, id: &str, parent: Option<&str>| native::Member {
        snapshot_id: id.into(),
        name: Some(name.into()),
        group: Some("dev".into()),
        parent_digest: parent.map(str::to_owned),
        ..Default::default()
    };
    let inventory = vec![
        member(A, "snap_a", None),
        member(B, "snap_b", Some("snap_a")),
        member(C, "snap_c", Some("snap_b")),
    ];
    let all: HashSet<native::Key> = inventory.iter().filter_map(native::Member::key).collect();
    let order = |plan: native::Plan| {
        plan.remove
            .into_iter()
            .map(|member| member.snapshot_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        order(native::plan(
            &inventory,
            &all,
            &HashMap::new(),
            &HashMap::new()
        )),
        ["snap_c", "snap_b", "snap_a"]
    );

    let positioned = native::plan(
        &inventory,
        &all,
        &HashMap::new(),
        &HashMap::from([("snap_c".to_owned(), "dev".to_owned())]),
    );
    assert!(positioned.remove.is_empty());
    assert_eq!(positioned.kept.len(), 3);

    let used = HashMap::from([(
        ("dev".to_owned(), B.to_owned()),
        vec![native::Use {
            computer_id: FORK_ID.into(),
            computer: "branch".into(),
            purpose: native::Purpose::PendingStart,
        }],
    )]);
    let plan = native::plan(&inventory, &all, &used, &HashMap::new());
    assert_eq!(order(plan), ["snap_c"]);
}

#[test]
fn deleting_a_recovery_checkpoint_moves_the_head_removes_its_member_and_drops_the_entry() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = delete_fixture(
        &directory,
        vec![
            entry(B, "Before restore", "before-restore"),
            entry(A, "Selected", "manual"),
        ],
        None,
    );
    // dev was restored from A; B was its previous instance's last capture and the group head.
    let store = Store::new(vec![("dev", Some("snap_a"))])
        .with("dev", A, "snap_a", None)
        .with("dev", B, "snap_b", Some("snap_a"));
    delete_checkpoint_with(&store, &paths, ID, B).unwrap();
    assert_eq!(store.names(), [A]);
    let calls = store.calls.lock().unwrap().clone();
    let moved = calls
        .iter()
        .position(|call| call[1] == "head" && call[2] == "dev:snap_a")
        .expect("the head moved first");
    let removed = calls.iter().position(|call| call[1] == "remove").unwrap();
    assert!(moved < removed);
    assert!(!calls
        .iter()
        .any(|call| call.iter().any(|arg| arg == "--force")));
    let record = load(&paths, ID).unwrap();
    assert_eq!(
        record
            .checkpoints
            .iter()
            .map(|checkpoint| checkpoint.id.as_str())
            .collect::<Vec<_>>(),
        [A]
    );

    // The checkpoint dev was restored from is what its next capture builds on.
    let failure = delete_checkpoint_with(&store, &paths, ID, A)
        .unwrap_err()
        .to_string();
    assert!(
        failure.contains("dev’s next checkpoint and export build on this one"),
        "{failure}"
    );
    assert_eq!(store.names(), [A]);
    assert_eq!(load(&paths, ID).unwrap().checkpoints.len(), 1);
}

#[test]
fn checkpoints_that_forks_later_checkpoints_or_pending_starts_depend_on_are_refused() {
    let _test_state = crate::test_support::global_state();
    // A later checkpoint builds on this one.
    let directory = tempfile::tempdir().unwrap();
    let paths = delete_fixture(
        &directory,
        vec![
            entry(B, "After deploy", "manual"),
            entry(A, "Before deploy", "manual"),
        ],
        None,
    );
    let store = Store::new(vec![("dev", None)])
        .with("dev", A, "snap_a", None)
        .with("dev", B, "snap_b", Some("snap_a"));
    cursor(&paths, "dev", "snap_b");
    let failure = delete_checkpoint_with(&store, &paths, ID, A)
        .unwrap_err()
        .to_string();
    assert!(
        failure.contains(
            "“After deploy” was saved after this checkpoint and builds on it. Delete it first."
        ),
        "{failure}"
    );
    // The latest capture is what dev's next capture names as its parent.
    let failure = delete_checkpoint_with(&store, &paths, ID, B)
        .unwrap_err()
        .to_string();
    assert!(
        failure.contains("next checkpoint and export build on this one"),
        "{failure}"
    );
    assert!(store.removals().is_empty());

    // A fork that has not started yet starts from it.
    let directory = tempfile::tempdir().unwrap();
    let mut fork = Record::default();
    fork.snapshot_group = Some("dev".into());
    fork.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: A.into(),
        source_computer: "dev".into(),
        state: "full".into(),
    });
    let paths = delete_fixture(
        &directory,
        vec![entry(A, "Before deploy", "manual")],
        Some(fork),
    );
    let store = Store::new(vec![]).with("dev", A, "snap_a", None);
    let failure = delete_checkpoint_with(&store, &paths, ID, A)
        .unwrap_err()
        .to_string();
    assert_eq!(
        failure,
        "Used by branch. branch starts from it the next time it starts."
    );

    // A started fork still builds on the checkpoint it was restored from.
    let directory = tempfile::tempdir().unwrap();
    let mut started = Record::default();
    started.snapshot_group = Some("dev".into());
    let paths = delete_fixture(
        &directory,
        vec![entry(A, "Before deploy", "manual")],
        Some(started),
    );
    let store = Store::new(vec![("branch", Some("snap_a"))]).with("dev", A, "snap_a", None);
    let failure = delete_checkpoint_with(&store, &paths, ID, A)
        .unwrap_err()
        .to_string();
    assert!(
        failure.starts_with("Used by branch. branch was started from this checkpoint"),
        "{failure}"
    );

    // This computer restarts from it, or a Restore is unfinished.
    let directory = tempfile::tempdir().unwrap();
    let paths = delete_fixture(&directory, vec![entry(A, "Before deploy", "manual")], None);
    let mut record = load(&paths, ID).unwrap();
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: A.into(),
        source_computer: "dev".into(),
        state: "full".into(),
    });
    save(&paths, ID, &record).unwrap();
    let store = Store::new(vec![]).with("dev", A, "snap_a", None);
    assert!(delete_checkpoint_with(&store, &paths, ID, A)
        .unwrap_err()
        .to_string()
        .contains("dev starts from this checkpoint the next time it starts"));
    assert!(store.removals().is_empty());
    assert_eq!(load(&paths, ID).unwrap().checkpoints.len(), 1);
}

#[test]
fn a_refused_native_removal_keeps_the_checkpoint_and_says_why() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = delete_fixture(
        &directory,
        vec![
            entry(B, "Before restore", "before-restore"),
            entry(A, "Selected", "manual"),
        ],
        None,
    );
    let mut store = Store::new(vec![("dev", Some("snap_a"))])
        .with("dev", A, "snap_a", None)
        .with("dev", B, "snap_b", None);
    store.fail_remove = true;
    let failure = delete_checkpoint_with(&store, &paths, ID, B)
        .unwrap_err()
        .to_string();
    assert!(
        failure.starts_with("The checkpoint could not be deleted; it was kept."),
        "{failure}"
    );
    assert_eq!(load(&paths, ID).unwrap().checkpoints.len(), 2);
}

#[test]
fn a_shared_or_missing_member_drops_only_the_entry() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let mut alias = entry(C, "Alias", "manual");
    alias.native_id = Some(A.into());
    let paths = delete_fixture(
        &directory,
        vec![
            alias,
            entry(A, "Before deploy", "manual"),
            entry(B, "Gone", "manual"),
        ],
        None,
    );
    let store = Store::new(vec![("dev", None)]).with("dev", A, "snap_a", None);
    delete_checkpoint_with(&store, &paths, ID, C).unwrap();
    delete_checkpoint_with(&store, &paths, ID, B).unwrap();
    assert!(store.removals().is_empty());
    assert_eq!(store.names(), [A]);
    assert_eq!(
        load(&paths, ID)
            .unwrap()
            .checkpoints
            .iter()
            .map(|checkpoint| checkpoint.id.as_str())
            .collect::<Vec<_>>(),
        [A]
    );
}

#[test]
fn checkpoint_usage_skips_runtime_surveys_without_native_members() {
    let _test_state = crate::test_support::global_state();
    struct Fleet {
        store: Store,
        count: usize,
    }
    impl RuntimeRunner for Fleet {
        fn run(
            &self,
            paths: &RuntimePaths,
            args: &[String],
            timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            if args[0] != "list" {
                return self.store.run(paths, args, timeout);
            }
            self.store.calls.lock().unwrap().push(args.to_vec());
            Ok(CommandOutput {
                stdout: serde_json::to_string(
                    &(0..self.count)
                        .map(|index| serde_json::json!({"name": format!("computer-{index}")}))
                        .collect::<Vec<_>>(),
                )
                .unwrap(),
                stderr: String::new(),
            })
        }
    }
    let mut measured_calls = Vec::new();
    for count in [1, 10, 100] {
        for has_entry in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let entries = if has_entry {
                vec![entry(A, "Missing", "manual")]
            } else {
                Vec::new()
            };
            let paths = delete_fixture(&directory, entries, None);
            let fleet = Fleet {
                store: Store::new(vec![]).with("other-group", A, "snap_other", None),
                count,
            };
            let started = std::time::Instant::now();
            survey(&fleet, &paths).unwrap();
            let baseline = started.elapsed();
            assert_eq!(fleet.store.calls.lock().unwrap().len(), count + 2);
            fleet.store.calls.lock().unwrap().clear();
            let started = std::time::Instant::now();
            let usage = usage_with(&fleet, &paths, ID).unwrap();
            let elapsed = started.elapsed();
            let calls = fleet.store.calls.lock().unwrap();
            eprintln!("usage: computers={count} entry={has_entry} survey_calls={} survey={baseline:?} usage_calls={} usage={elapsed:?}", count + 2, calls.len());
            assert_eq!(usage.total_bytes, Some(0));
            assert_eq!(usage.checkpoints.len(), usize::from(has_entry));
            measured_calls.push((calls.len(), usize::from(has_entry)));
            if has_entry {
                assert_eq!(calls[0], ["snapshot", "list", "--format", "json"]);
                assert!(usage.checkpoints[0].delete_blocker.is_none());
            }
        }
    }
    for (actual, expected) in measured_calls {
        assert_eq!(actual, expected);
    }
    let directory = tempfile::tempdir().unwrap();
    let paths = delete_fixture(&directory, Vec::new(), Some(Record::default()));
    fs::write(path(&paths, FORK_ID), b"invalid history").unwrap();
    let store = Store::new(vec![]);
    let usage = usage_with(&store, &paths, ID).unwrap();
    assert_eq!(usage.total_bytes, Some(0));
    assert!(usage.checkpoints.is_empty());
    assert!(store.calls.lock().unwrap().is_empty());
}

#[test]
fn checkpoint_usage_keeps_saved_dependencies_when_native_member_is_missing() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let mut pending = Record::default();
    pending.pending_checkpoint_restore = Some(PendingRestore {
        source_computer: "dev".into(),
        checkpoint_id: A.into(),
        state: "full".into(),
    });
    let paths = delete_fixture(
        &directory,
        vec![entry(A, "Missing", "manual")],
        Some(pending),
    );
    let store = Store::new(vec![("branch", None)]);
    let usage = usage_with(&store, &paths, ID).unwrap();
    assert_eq!(usage.checkpoints[0].used_by, ["branch"]);
    assert!(usage.checkpoints[0]
        .delete_blocker
        .as_ref()
        .unwrap()
        .starts_with("Used by branch."));
    assert_eq!(store.calls.lock().unwrap().len(), 1);
}

#[test]
fn checkpoint_storage_totals_lists_only_the_selected_lineage_group() {
    let _test_state = crate::test_support::global_state();
    for group in [Some("source"), None] {
        let directory = tempfile::tempdir().unwrap();
        let paths = delete_fixture(&directory, vec![entry(A, "Base", "manual")], None);
        let mut record = load(&paths, ID).unwrap();
        record.snapshot_group = group.map(str::to_owned);
        save(&paths, ID, &record).unwrap();
        let group = group.unwrap_or("dev");
        let store = Store::new(vec![]).with(group, A, "snap_a", None);
        assert_eq!(storage_totals(&store, &paths, ID, "dev"), (None, 1));
        assert_eq!(
            *store.calls.lock().unwrap(),
            [vec![
                "snapshot", "list", "--group", group, "--format", "json"
            ]]
        );
    }
}

#[test]
fn checkpoint_usage_keeps_unconfigured_lineage_and_cross_group_children() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = delete_fixture(&directory, vec![entry(A, "Base", "manual")], None);
    let store = Store::new(vec![("outside-silo", Some("snap_a"))]).with("dev", A, "snap_a", None);
    let usage = usage_with(&store, &paths, ID).unwrap();
    assert_eq!(usage.checkpoints[0].used_by, ["outside-silo"]);
    assert!(usage.checkpoints[0]
        .delete_blocker
        .as_ref()
        .unwrap()
        .starts_with("Used by outside-silo."));

    let store = Store::new(vec![]).with("dev", A, "snap_a", None).with(
        "another-group",
        B,
        "snap_b",
        Some("snap_a"),
    );
    let usage = usage_with(&store, &paths, ID).unwrap();
    assert!(usage.checkpoints[0]
        .delete_blocker
        .as_ref()
        .unwrap()
        .contains("A later saved state"));
}

#[test]
fn checkpoint_usage_reports_sizes_users_and_blockers() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let mut started = Record::default();
    started.snapshot_group = Some("dev".into());
    let paths = delete_fixture(
        &directory,
        vec![
            entry(B, "Before restore", "before-restore"),
            entry(A, "Selected", "manual"),
        ],
        Some(started),
    );
    let store = Store::new(vec![("dev", Some("snap_a")), ("branch", Some("snap_a"))])
        .with("dev", A, "snap_a", None)
        .with("dev", B, "snap_b", None);
    let usage = usage_with(&store, &paths, ID).unwrap();
    let json = serde_json::to_value(&usage).unwrap();
    let find = |id: &str| {
        json["checkpoints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == id)
            .unwrap()
            .clone()
    };
    assert!(find(B).get("deleteBlocker").is_none());
    assert_eq!(find(A)["usedBy"], serde_json::json!(["branch"]));
    assert!(find(A)["deleteBlocker"]
        .as_str()
        .unwrap()
        .starts_with("Used by branch."));
}

#[test]
fn deleting_a_computer_removes_members_only_it_used_and_keeps_what_a_fork_builds_on() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let mut started = Record::default();
    started.snapshot_group = Some("dev".into());
    let paths = delete_fixture(
        &directory,
        vec![entry(B, "Later", "manual"), entry(A, "Base", "manual")],
        Some(started),
    );
    // dev is deleted: the inventory no longer lists it; its record is removed afterwards.
    let metadata = read_metadata(&paths.metadata).unwrap();
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: metadata
                .computers
                .into_iter()
                .filter(|configuration| configuration.id() != ID)
                .collect(),
        },
    )
    .unwrap();
    let store = Store::new(vec![("branch", Some("snap_a"))])
        .with("dev", A, "snap_a", None)
        .with("dev", B, "snap_b", Some("snap_a"))
        .with("dev", "silo-backup-0-1-2", "snap_x", Some("snap_b"))
        .with("elsewhere", C, "snap_c", None);
    remove_deleted_snapshots(&store, &paths, ID, "dev").unwrap();
    assert_eq!(
        store.removals(),
        [
            "dev:silo-backup-0-1-2",
            "dev:c111111111111111111111111111111"
        ]
    );
    assert_eq!(store.names(), [A, C]);
}

#[test]
fn a_failed_capture_removes_its_published_member_unless_the_computer_builds_on_it() {
    let _test_state = crate::test_support::global_state();
    for builds_on_it in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let paths = delete_fixture(&directory, Vec::new(), None);
        let mut store = Store::new(vec![("dev", None)]);
        store.fail_create_after_publish = true;
        if builds_on_it {
            cursor(&paths, "dev", "snap_00000000000000000000000000000064");
        }
        capture_with(&store, &paths, ID, "Interrupted", "manual").unwrap_err();
        let record = load(&paths, ID).unwrap();
        if builds_on_it {
            assert_eq!(store.names().len(), 1, "a member dev builds on is kept");
            assert!(
                record.inflight_checkpoint.is_some(),
                "and stays reachable for reconciliation"
            );
        } else {
            assert!(store.names().is_empty());
            assert!(record.inflight_checkpoint.is_none());
        }
        assert_eq!(record.checkpoint_operation.unwrap().status, "failed");
    }
}

#[test]
fn launch_cleanup_removes_only_the_capture_identified_by_its_journal() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = delete_fixture(&directory, vec![entry(A, "Kept", "manual")], None);
    let mut record = load(&paths, ID).unwrap();
    record.inflight_checkpoint = Some(entry(B, "Interrupted", "manual"));
    save(&paths, ID, &record).unwrap();
    let store = Store::new(vec![("dev", None)])
        .with("dev", A, "snap_a", None)
        .with("dev", B, "snap_b", None)
        .with("gone", C, "snap_c", None)
        .with("dev", "user-made", "snap_u", None);
    recover_interrupted(&store, &paths).unwrap();
    assert_eq!(store.names(), [A, C, "user-made"]);
    assert!(load(&paths, ID).unwrap().inflight_checkpoint.is_none());
    assert_eq!(load(&paths, ID).unwrap().checkpoints.len(), 1);
}

#[test]
fn recovery_reports_unresolved_owner_and_continues_independent_captures() {
    let _test_state = crate::test_support::global_state();
    for damaged_history in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut fork = Record::default();
        fork.inflight_checkpoint = Some(entry(C, "Interrupted branch", "manual"));
        let paths = delete_fixture(&directory, vec![], Some(fork));
        let mut record = load(&paths, ID).unwrap();
        record.inflight_checkpoint = Some(entry(B, "Interrupted source", "manual"));
        save(&paths, ID, &record).unwrap();
        if damaged_history {
            fs::write(path(&paths, ID), b"{broken").unwrap();
        }
        fs::write(cleanup_path(&paths), b"{broken-cleanup").unwrap();
        let before = fs::read(path(&paths, ID)).unwrap();
        let mut store = Store::new(vec![]).with("dev", B, "snap_b", None);
        store.fail_remove = true;
        let recovery = recover_interrupted(&store, &paths).unwrap();
        assert_eq!(recovery.unresolved.len(), 1);
        assert!(recovery.unresolved.contains_key(ID));
        assert!(recovery.cleanup_error.is_some());
        assert_eq!(fs::read(cleanup_path(&paths)).unwrap(), b"{broken-cleanup");
        assert_eq!(fs::read(path(&paths, ID)).unwrap(), before);
        assert_eq!(store.names(), [B]);
        let recovered = load(&paths, FORK_ID).unwrap();
        assert!(recovered.inflight_checkpoint.is_none());
        assert_eq!(recovered.checkpoint_operation.unwrap().status, "failed");
        store.fail_remove = false;
        fs::remove_file(cleanup_path(&paths)).unwrap();
        if damaged_history {
            save(&paths, ID, &record).unwrap();
        }
        assert!(recover_interrupted(&store, &paths)
            .unwrap()
            .unresolved
            .is_empty());
        assert!(store.names().is_empty());
    }
}

#[test]
fn deleting_the_last_dependent_removes_the_deleted_sources_kept_members() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let mut started = Record::default();
    started.snapshot_group = Some("dev".into());
    let paths = delete_fixture(&directory, vec![entry(A, "Base", "manual")], Some(started));
    let metadata = read_metadata(&paths.metadata).unwrap();
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: metadata
                .computers
                .into_iter()
                .filter(|configuration| configuration.id() != ID)
                .collect(),
        },
    )
    .unwrap();
    let mut store = Store::new(vec![("branch", Some("snap_a"))]).with("dev", A, "snap_a", None);
    remove_deleted_snapshots(&store, &paths, ID, "dev").unwrap();
    forget_removed(&paths, ID).unwrap();
    assert_eq!(store.names(), [A]);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![],
        },
    )
    .unwrap();
    store.computers.clear();
    remove_deleted_snapshots(&store, &paths, FORK_ID, "branch").unwrap();
    assert!(store.names().is_empty());
}

#[test]
fn deleting_the_last_dependent_checkpoint_releases_the_deleted_sources_member() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let mut fork = Record::default();
    fork.snapshot_group = Some("dev".into());
    fork.checkpoints = vec![entry(B, "Fork checkpoint", "manual")];
    let paths = delete_fixture(&directory, vec![entry(A, "Base", "manual")], Some(fork));
    let metadata = read_metadata(&paths.metadata).unwrap();
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: metadata
                .computers
                .into_iter()
                .filter(|configuration| configuration.id() != ID)
                .collect(),
        },
    )
    .unwrap();
    let store = Store::new(vec![("branch", None)])
        .with("dev", A, "snap_a", None)
        .with("dev", B, "snap_b", Some("snap_a"));
    remove_deleted_snapshots(&store, &paths, ID, "dev").unwrap();
    forget_removed(&paths, ID).unwrap();
    assert_eq!(store.names(), [A, B]);
    delete_checkpoint_with(&store, &paths, FORK_ID, B).unwrap();
    assert!(store.names().is_empty());
}

#[test]
fn deletion_recovery_does_not_remove_a_new_member_reusing_the_old_name() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = delete_fixture(&directory, vec![entry(A, "Delete", "manual")], None);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![],
        },
    )
    .unwrap();
    let mut store = Store::new(vec![]).with("dev", A, "snap_old", None);
    store.fail_remove = true;
    remove_deleted_snapshots(&store, &paths, ID, "dev").unwrap();
    store.members.lock().unwrap().clear();
    store.heads.lock().unwrap().clear();
    let store = store.with("dev", A, "snap_new", None);
    let attempts = store.removals().len();
    recover_interrupted(&store, &paths).unwrap();
    assert_eq!(store.names(), [A]);
    assert_eq!(store.removals().len(), attempts);
    assert!(load_cleanup(&paths).unwrap().is_empty());
}

#[test]
fn launch_retries_journaled_deletion_without_selecting_other_orphans() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = delete_fixture(&directory, vec![entry(A, "Delete", "manual")], None);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![],
        },
    )
    .unwrap();
    let mut store =
        Store::new(vec![])
            .with("dev", A, "snap_a", None)
            .with("elsewhere", C, "snap_c", None);
    store.fail_remove = true;
    remove_deleted_snapshots(&store, &paths, ID, "dev").unwrap();
    forget_removed(&paths, ID).unwrap();
    assert_eq!(store.names(), [A, C]);
    store.fail_remove = false;
    recover_interrupted(&store, &paths).unwrap();
    assert_eq!(store.names(), [C]);
}

/// Models the process runner's cancel: any command started while the running operation
/// was asked to cancel (and not masked) is killed.
struct CancelRunner {
    gate: &'static super::super::operation_gate::OperationGate,
    kill_create: bool,
    state: Mutex<&'static str>,
    created: Mutex<Option<String>>,
    calls: Mutex<Vec<Vec<String>>>,
}
impl RuntimeRunner for CancelRunner {
    fn run(
        &self,
        _: &RuntimePaths,
        args: &[String],
        _: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.calls.lock().unwrap().push(args.to_vec());
        if super::super::operation_gate::cancel_requested() {
            return Err(RuntimeError::Cancelled {
                operation: args[0].clone(),
            });
        }
        let ok = |stdout: String| {
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        };
        match (args[0].as_str(), args.get(1).map(String::as_str)) {
                ("inspect", _) => ok(serde_json::json!({"name":"dev","status":*self.state.lock().unwrap(),"config":{
                    "labels":{"silo.managed":"true","silo.machine-id":ID},
                    "mounts":[{"guest":"/workspace","type":"Owned","storage":{"kind":"disk","capacity_mib":1024}}]
                }}).to_string()),
                ("snapshot", Some("create")) => {
                    let entry = self.gate.snapshot().running[0].id;
                    self.gate.cancel(entry).unwrap();
                    if self.kill_create {
                        *self.state.lock().unwrap() = "Paused";
                        return Err(RuntimeError::Cancelled { operation: "snapshot create".into() });
                    }
                    *self.created.lock().unwrap() = Some(args[2].clone());
                    ok(String::new())
                }
                ("snapshot", Some("list")) => {
                    let name = self.created.lock().unwrap().clone();
                    ok(serde_json::Value::Array(name.into_iter().map(|name| serde_json::json!({
                        "snapshot_id": "snap_1", "group": "dev", "name": name, "scope": "full", "availability": "ready"
                    })).collect()).to_string())
                }
                ("resume", _) => {
                    *self.state.lock().unwrap() = "Running";
                    ok(String::new())
                }
                ("list", _) => ok(r#"[{"name":"dev"}]"#.into()),
                _ => ok(String::new()),
            }
    }
}

fn cancel_runner(kill_create: bool) -> CancelRunner {
    let gate: &'static super::super::operation_gate::OperationGate =
        Box::leak(Box::new(super::super::operation_gate::OperationGate::new()));
    CancelRunner {
        gate,
        kill_create,
        state: Mutex::new("Running"),
        created: Mutex::new(None),
        calls: Mutex::new(Vec::new()),
    }
}

#[test]
fn a_cancel_after_the_capture_returned_still_records_the_checkpoint() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let runner = cancel_runner(false);
    let guard = runner
        .gate
        .kind(super::super::operation_gate::OperationKind::CheckpointCapture)
        .computer(ID, "dev", "Creating checkpoint")
        .unwrap();
    guard.allow_cancel();
    capture_with(&runner, &paths, ID, "Late cancel", "manual").unwrap();
    drop(guard);
    let record = load(&paths, ID).unwrap();
    assert_eq!(record.checkpoints[0].name, "Late cancel");
    assert!(record.checkpoint_operation.is_none());
    assert!(record.inflight_checkpoint.is_none());
}

#[test]
fn a_cancelled_full_capture_resumes_the_computer_it_left_paused() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let runner = cancel_runner(true);
    let guard = runner
        .gate
        .kind(super::super::operation_gate::OperationKind::CheckpointCapture)
        .computer(ID, "dev", "Creating checkpoint")
        .unwrap();
    guard.allow_cancel();
    let failure = capture_with(&runner, &paths, ID, "Cancelled", "manual").unwrap_err();
    drop(guard);
    assert!(matches!(failure, RuntimeError::Cancelled { .. }));
    assert_eq!(*runner.state.lock().unwrap(), "Running");
    assert!(runner
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|call| call[0] == "resume"));
    let record = load(&paths, ID).unwrap();
    assert_eq!(record.checkpoint_operation.unwrap().status, "failed");
    assert!(
        record.inflight_checkpoint.is_none(),
        "nothing was published, so nothing is left to reconcile"
    );
}

#[test]
fn a_computer_that_cannot_resume_after_a_failed_recovery_capture_is_stopped_instead_of_left_paused()
{
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let runner = journal_runner("Running", "create|resume");
    let failure = restore_with(&runner, &paths, ID, "c000000000000000000000000000000")
        .unwrap_err()
        .to_string();
    assert!(failure.starts_with("create failed on this host."));
    assert!(failure.contains("resume failed on this host."));
    assert!(failure.contains("disks were preserved"));
    assert_eq!(*runner.state.lock().unwrap(), "Stopped");
    let stored = load(&paths, ID).unwrap();
    assert!(stored.restore_journal.is_none());
    assert!(
        !needs_explicit_start(&paths, ID).unwrap(),
        "Start, Stop and Quit work again"
    );
}

#[test]
fn abandoning_an_unfinished_restore_resumes_the_paused_computer_and_keeps_its_state() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, Some(("capturing", true)));
    let mut runner = journal_runner("Paused", "");
    runner.listed = true;
    abandon_restore_with(&runner, &paths, ID).unwrap();
    assert_eq!(*runner.state.lock().unwrap(), "Running");
    let stored = load(&paths, ID).unwrap();
    assert!(stored.restore_journal.is_none());
    assert!(stored.checkpoint_operation.is_none());
    assert!(stored.pending_checkpoint_restore.is_none());
    assert!(!runner
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|call| call[0] == "remove"));

    // A secured Restore whose original computer was already removed cannot be abandoned.
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, Some(("secured", true)));
    let runner = journal_runner("Stopped", "");
    let failure = abandon_restore_with(&runner, &paths, ID)
        .unwrap_err()
        .to_string();
    assert!(failure.contains("already replaced"), "{failure}");
    assert!(load(&paths, ID).unwrap().restore_journal.is_some());
    assert!(abandon_restore_with(
        &journal_runner("Running", ""),
        &restore_fixture(&tempfile::tempdir().unwrap(), None),
        ID
    )
    .is_err());
}

#[test]
fn unfinished_restore_messages_and_view_name_the_checkpoint() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, Some(("capturing", true)));
    assert_eq!(
            explicit_start_message(&paths, ID, "dev"),
            "The Restore of dev to “Selected” is unfinished. Retry it, or abandon it in Checkpoints, first."
        );
    let view =
        serde_json::to_value(view_unfinished_restore(&load(&paths, ID).unwrap()).unwrap()).unwrap();
    assert_eq!(
        view,
        serde_json::json!({"checkpointId": "c000000000000000000000000000000", "checkpointName": "Selected", "phase": "capturing"})
    );
    let failure = start_pending(
        &journal_runner("Running", ""),
        &paths,
        &computer_configuration(),
    )
    .unwrap_err()
    .to_string();
    assert!(failure.contains("to “Selected” is unfinished"), "{failure}");

    // Restoring a different checkpoint names the unfinished one.
    let mut record = load(&paths, ID).unwrap();
    record.checkpoints.push(Checkpoint {
        id: "c333333333333333333333333333333".into(),
        native_id: Some("c000000000000000000000000000000".into()),
        name: "Other".into(),
        created_at: 3,
        scope: "full".into(),
        reason: "manual".into(),
    });
    save(&paths, ID, &record).unwrap();
    let mut runner = journal_runner("Running", "");
    runner.listed = true;
    let failure = restore_with(&runner, &paths, ID, "c333333333333333333333333333333")
        .unwrap_err()
        .to_string();
    assert!(failure.contains("to “Selected” is unfinished"), "{failure}");

    // A fork waiting for its first Start is not described as a Restore.
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let mut record = load(&paths, ID).unwrap();
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: "c000000000000000000000000000000".into(),
        source_computer: "dev".into(),
        state: "full".into(),
    });
    save(&paths, ID, &record).unwrap();
    assert_eq!(
        explicit_start_message(&paths, ID, "dev"),
        "dev starts from a checkpoint first. Use Start on its page."
    );
    assert!(view_unfinished_restore(&record).is_none());
}

#[test]
fn quit_releases_a_computer_an_unfinished_restore_left_paused() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, Some(("capturing", true)));
    let runner = journal_runner("Paused", "");
    release_paused_restore(&runner, &paths, &computer_configuration());
    assert_eq!(*runner.state.lock().unwrap(), "Running");
    // Without an unfinished Restore, Quit's own handling applies.
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let runner = journal_runner("Paused", "");
    release_paused_restore(&runner, &paths, &computer_configuration());
    assert_eq!(*runner.state.lock().unwrap(), "Paused");
}

/// A restore attempt whose computer runs but never verifies, then is stopped (as Quit does).
struct AttemptRunner {
    state: Mutex<&'static str>,
    exists: Mutex<bool>,
    restore_ok: bool,
    attempt: Mutex<Option<String>>,
    calls: Mutex<Vec<Vec<String>>>,
}
impl RuntimeRunner for AttemptRunner {
    fn run(
        &self,
        _: &RuntimePaths,
        args: &[String],
        _: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.calls.lock().unwrap().push(args.to_vec());
        let ok = |stdout: String| {
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        };
        match args[0].as_str() {
                "snapshot" => ok(serde_json::json!([{"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"}]).to_string()),
                "list" => ok(if *self.exists.lock().unwrap() { r#"[{"name":"dev"}]"#.into() } else { "[]".into() }),
                "restore" => {
                    let label = args.iter().find_map(|arg| arg.strip_prefix("silo.restore-attempt=")).unwrap().to_owned();
                    *self.attempt.lock().unwrap() = Some(label);
                    *self.exists.lock().unwrap() = true;
                    *self.state.lock().unwrap() = "Running";
                    if self.restore_ok { ok(String::new()) } else { Err(RuntimeError::TimedOut { operation: "restore".into() }) }
                }
                "inspect" if *self.exists.lock().unwrap() => ok(serde_json::json!({"name":"dev","status":*self.state.lock().unwrap(),"config":{
                    // No SILO_GITHUB secret: this computer never passes restore verification.
                    "labels":{"silo.managed":"true","silo.machine-id":ID,"silo.restore-attempt":self.attempt.lock().unwrap().clone()},
                    "resources":{"max_cpus":1,"max_memory_mib":1024},
                    "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]}}
                }}).to_string()),
                "inspect" => Err(RuntimeError::Unavailable("computer not found: dev".into())),
                "start" => {
                    *self.state.lock().unwrap() = "Running";
                    ok(String::new())
                }
                _ => ok(String::new()),
            }
    }
}

fn pending_fixture(directory: &tempfile::TempDir) -> RuntimePaths {
    let paths = paths(directory);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer_configuration()],
        },
    )
    .unwrap();
    let mut record = Record::default();
    record.snapshot_group = Some("dev".into());
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: "c000000000000000000000000000000".into(),
        source_computer: "dev".into(),
        state: "full".into(),
    });
    record.desired_network_policy =
        Some(serde_json::json!({"default_egress":"deny","default_ingress":"allow","rules":[]}));
    save(&paths, ID, &record).unwrap();
    paths
}

#[test]
fn a_retried_start_keeps_and_starts_an_attempt_that_already_ran() {
    let _test_state = crate::test_support::global_state();
    for restore_ok in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let paths = pending_fixture(&directory);
        let runner = AttemptRunner {
            state: Mutex::new("Stopped"),
            exists: Mutex::new(false),
            restore_ok,
            attempt: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
        };
        start_pending(&runner, &paths, &computer_configuration()).unwrap_err();
        let stored = load(&paths, ID).unwrap();
        assert!(stored.restore_attempt_ran, "restore_ok={restore_ok}");
        // Quit stopped the unverified computer; the user then retries Start.
        *runner.state.lock().unwrap() = "Stopped";
        runner.calls.lock().unwrap().clear();
        start_pending(&runner, &paths, &computer_configuration()).unwrap();
        let calls = runner.calls.lock().unwrap();
        assert!(
            !calls
                .iter()
                .any(|call| call[0] == "remove" || call[0] == "restore"),
            "{calls:?}"
        );
        assert!(calls.iter().any(|call| call[0] == "start"));
        assert_eq!(*runner.state.lock().unwrap(), "Running");
        let stored = load(&paths, ID).unwrap();
        assert!(stored.pending_checkpoint_restore.is_none());
        assert!(!stored.restore_attempted && !stored.restore_attempt_ran);
        assert!(!needs_explicit_start(&paths, ID).unwrap());
    }
}

#[test]
fn an_attempt_that_never_ran_is_recreated_from_the_checkpoint() {
    let _test_state = crate::test_support::global_state();
    struct NeverRan(Mutex<Vec<String>>);
    impl RuntimeRunner for NeverRan {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.0.lock().unwrap().push(args[0].clone());
            match args[0].as_str() {
                    "snapshot" => Ok(CommandOutput { stdout: serde_json::json!([{"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"}]).to_string(), stderr: String::new() }),
                    "list" => Ok(CommandOutput { stdout: "[]".into(), stderr: String::new() }),
                    "restore" => Err(RuntimeError::Failed { operation: "restore".into(), exit_code: Some(1), detail: "incomplete".into() }),
                    _ => Err(RuntimeError::Unavailable("computer not found: dev".into())),
                }
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let paths = pending_fixture(&directory);
    start_pending(
        &NeverRan(Mutex::new(Vec::new())),
        &paths,
        &computer_configuration(),
    )
    .unwrap_err();
    assert!(!load(&paths, ID).unwrap().restore_attempt_ran);
}

/// Real checkpoint Restore and Fork against the bundled runtime: a running computer is
/// captured as a full checkpoint, changed, forked (the fork starts from RAM), then
/// restored in place. Uses only a disposable /tmp home and `e2e-*` computers. Run
/// with `SILO_LIVE_TEST_CONFIRM`, `SILO_TEST_MSB` and `SILO_TEST_LIBKRUNFW`.
#[test]
#[ignore = "requires the packaged runtime and hardware virtualization"]
fn live_checkpoint_restore_and_fork_use_the_runtimes_names() {
    crate::test_support::live::require_confirmation();
    let _test_state = crate::test_support::global_state();
    // The live runtime control socket requires a short root (104 bytes on macOS).
    let directory = tempfile::Builder::new()
        .prefix("silo-ck-")
        .tempdir_in(crate::test_support::live::temp_root())
        .unwrap();
    let paths = RuntimePaths {
        guest_image: crate::test_support::manifest_root().join("runtime/guest-image"),
        executable: std::path::PathBuf::from(std::env::var("SILO_TEST_MSB").unwrap()),
        library: std::path::PathBuf::from(std::env::var("SILO_TEST_LIBKRUNFW").unwrap()),
        home: directory.path().join("runtime"),
        storage_home: None,
        metadata: directory.path().join("computers.json"),
        volumes: directory.path().join("volumes"),
    };
    struct Cleanup<'a>(&'a RuntimePaths);
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            for name in ["e2e-ck-source", "e2e-ck-fork"] {
                let _ = super::super::run_msb(
                    self.0,
                    &["stop".into(), name.into()],
                    Duration::from_secs(60),
                );
            }
        }
    }
    let _cleanup = Cleanup(&paths);
    let run = |arguments: &[&str]| {
        super::super::run_msb(
            &paths,
            &arguments.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            Duration::from_secs(180),
        )
        .unwrap()
    };
    let write = |name: &str, value: &str| {
        run(&[
            "exec",
            name,
            "--",
            "sh",
            "-c",
            &format!(
                "printf {value} > /workspace/e2e-marker; printf {value} > /root/e2e-marker; sync"
            ),
        ]);
    };
    let read = |name: &str| {
        run(&[
            "exec",
            name,
            "--",
            "sh",
            "-c",
            "cat /workspace/e2e-marker; printf :; cat /root/e2e-marker",
        ])
        .stdout
    };

    let configuration =
        super::super::create_disposable_test_computer(&paths, "e2e-ck-source").unwrap();
    run(&["start", "e2e-ck-source"]);
    write("e2e-ck-source", "before");
    let checkpoint = capture_for_test(&paths, configuration.id(), "Milestone").unwrap();
    let record = load(&paths, configuration.id()).unwrap();
    assert_eq!(record.checkpoints[0].scope, "full");
    write("e2e-ck-source", "after");
    assert_eq!(read("e2e-ck-source").trim(), "after:after");

    // Fork the checkpoint into a new, stopped computer and start it from RAM.
    let assignments = FakeAssignments::new(&[]);
    let fork = fork_source(
        &ProcessRunner,
        &paths,
        configuration.id(),
        Some(&checkpoint),
        "e2e-ck-fork",
    )
    .unwrap();
    assert_eq!(fork.scope, "full");
    fork_commit(&ProcessRunner, &paths, &assignments, &fork, "e2e-ck-fork").unwrap();
    super::super::start_disposable_test_import(&paths, "e2e-ck-fork").unwrap();
    assert_eq!(read("e2e-ck-fork").trim(), "before:before");
    // The source kept its later state while the fork ran.
    assert_eq!(read("e2e-ck-source").trim(), "after:after");
    run(&["stop", "e2e-ck-fork"]);

    // Restore the checkpoint in place.
    // Restore leaves the computer stopped with the checkpoint pending; Start resumes it.
    run(&["stop", "e2e-ck-source"]);
    restore_with(&ProcessRunner, &paths, configuration.id(), &checkpoint).unwrap();
    super::super::start_disposable_test_import(&paths, "e2e-ck-source").unwrap();
    assert_eq!(read("e2e-ck-source").trim(), "before:before");
    eprintln!("Verified live checkpoint fork and restore.");
}

/// A built-in computer's whole life against the real runtime: restart, stop and start
/// (the app's own actions), then a checkpoint of the running computer, a fork of it and
/// an in-place restore. Every boot must end with the desktop session running and
/// computer use ready, the ChatGPT folder mounted read-only, and the approval mode
/// kept. Needs the same inputs as the other built-in live tests (see
/// `test_support::computer_use_live`).
#[test]
#[ignore = "requires the v4 guest image, a published ChatGPT app and hardware virtualization"]
fn live_built_in_lifecycle_keeps_the_desktop_and_computer_use() {
    use crate::test_support::computer_use_live::Fixture;
    let _test_state = crate::test_support::global_state();
    let mut fixture = Fixture::new("silo-life-", None, true);
    let (source, fork_name) = ("e2e-life-src", "e2e-life-fork");
    let configuration = fixture.create(source);
    fixture.track(fork_name);
    crate::computer_use::apply_approval_with(
        std::sync::Arc::new(ProcessRunner),
        &fixture.paths,
        &configuration,
        crate::computer_use::Approval::Auto,
        false,
    )
    .unwrap();
    super::super::start_disposable_test_computer(&fixture.paths, source).unwrap();
    let checks = |name: &str, label: &str| {
        let (status, elapsed) = fixture.wait_ready(name, label);
        eprintln!("RESULT {label}: ready {}s after start", elapsed.as_secs());
        assert_eq!(status["computerUse"]["approval"], "auto", "{label}");
        let mounts = fixture.exec_status(name, "grep ' /opt/silo/chatgpt ' /proc/mounts");
        assert!(mounts.contains(" ro,"), "{label}: {mounts}");
        // Not only what Silo reports: LCU lists windows and takes a screenshot.
        fixture.wait_doctor(name, label);
    };
    checks(source, "fresh");
    super::super::disposable_test_action(&fixture.paths, source, "restart").unwrap();
    checks(source, "restart");
    super::super::disposable_test_action(&fixture.paths, source, "stop").unwrap();
    let stopped = fixture.status(source);
    assert_eq!(stopped["sessionState"], "stopped", "{stopped}");
    assert_eq!(stopped["computerUse"]["approval"], "auto", "{stopped}");
    super::super::start_disposable_test_computer(&fixture.paths, source).unwrap();
    checks(source, "stop-start");

    let write = |name: &str, value: &str| {
        fixture
            .exec(
                name,
                "root",
                &format!("printf {value} > /workspace/e2e-marker; sync"),
            )
            .unwrap();
    };
    let read = |name: &str| {
        fixture
            .exec(name, "root", "cat /workspace/e2e-marker")
            .unwrap()
    };
    write(source, "before");
    let checkpoint = capture_for_test(&fixture.paths, configuration.id(), "Milestone").unwrap();
    assert_eq!(
        load(&fixture.paths, configuration.id())
            .unwrap()
            .checkpoints[0]
            .scope,
        "full"
    );
    write(source, "after");

    // Fork the checkpoint into a new computer and boot it from RAM.
    let fork = fork_source(
        &ProcessRunner,
        &fixture.paths,
        configuration.id(),
        Some(&checkpoint),
        fork_name,
    )
    .unwrap();
    fork_commit(
        &ProcessRunner,
        &fixture.paths,
        &FakeAssignments::new(&[]),
        &fork,
        fork_name,
    )
    .unwrap();
    super::super::start_disposable_test_import(&fixture.paths, fork_name).unwrap();
    checks(fork_name, "fork");
    assert_eq!(read(fork_name), "before");
    assert_eq!(read(source), "after");
    fixture.stop(fork_name);

    // Restore the checkpoint in place; Start resumes it.
    fixture.stop(source);
    restore_with(
        &ProcessRunner,
        &fixture.paths,
        configuration.id(),
        &checkpoint,
    )
    .unwrap();
    super::super::start_disposable_test_import(&fixture.paths, source).unwrap();
    checks(source, "restore");
    assert_eq!(read(source), "before");
    fixture.stop(source);
    eprintln!("Verified the built-in desktop through restart, stop, start, fork and restore.");
}

/// A computer created from the previous (v3) guest image is unchanged by the built-in
/// desktop: no ChatGPT mount, no automatic desktop, no computer-use helper, and its
/// ordinary flows (restart, stop, start, checkpoint, fork, restore) still work.
/// `SILO_TEST_V3_GUEST_IMAGE` names a directory with the v3 manifest.json and
/// image.tar.gz (from the guest-ubuntu-24.04-v3 release); the other inputs are those of
/// the built-in live tests.
#[test]
#[ignore = "requires the v3 guest image, hardware virtualization and the packaged runtime"]
fn live_pre_v4_computer_gets_no_mount_no_desktop_and_keeps_its_flows() {
    use crate::test_support::computer_use_live::Fixture;
    let _test_state = crate::test_support::global_state();
    let image = std::path::PathBuf::from(std::env::var("SILO_TEST_V3_GUEST_IMAGE").unwrap());
    let mut fixture = Fixture::new("silo-v3-", Some(image), false);
    let (source, fork_name) = ("e2e-v3-src", "e2e-v3-fork");
    let configuration = fixture.create(source);
    fixture.track(fork_name);
    assert!(
        !crate::computer_use::is_built_in(&configuration),
        "{configuration:?}"
    );
    assert!(computer_desktop_absent(&configuration), "{configuration:?}");
    let inspected = inspect_computer(&ProcessRunner, &fixture.paths, source).unwrap();
    assert!(!has_chatgpt_mount(&inspected.config));
    let checks = |label: &str| {
        let report = fixture.exec_status(
                source,
                "ls /usr/local/bin/silo-desktop /usr/local/libexec/silo-computer-use /opt/silo 2>&1; \
                 ls /var/lib/silo-computer-use 2>&1; pgrep -c Xvfb; grep -c /opt/silo/chatgpt /proc/mounts; \
                 cat /etc/silo-guest-version 2>/dev/null; true",
            );
        eprintln!("{label}: legacy guest state:\n{report}");
        assert!(
            !report.contains("/usr/local/libexec/silo-computer-use\n"),
            "{label}: {report}"
        );
        assert!(report.contains("No such file"), "{label}: {report}");
        let status = fixture.status(source);
        assert!(status["computerUse"].is_null(), "{label}: {status}");
        assert_ne!(status["sessionState"], "running", "{label}: {status}");
    };
    super::super::start_disposable_test_computer(&fixture.paths, source).unwrap();
    std::thread::sleep(Duration::from_secs(45));
    checks("fresh");
    super::super::disposable_test_action(&fixture.paths, source, "restart").unwrap();
    std::thread::sleep(Duration::from_secs(30));
    checks("restart");
    super::super::disposable_test_action(&fixture.paths, source, "stop").unwrap();
    super::super::start_disposable_test_computer(&fixture.paths, source).unwrap();
    std::thread::sleep(Duration::from_secs(30));
    checks("stop-start");

    let write = |name: &str, value: &str| {
        fixture
            .exec(
                name,
                "root",
                &format!("printf {value} > /workspace/e2e-marker; sync"),
            )
            .unwrap();
    };
    let read = |name: &str| {
        fixture
            .exec(name, "root", "cat /workspace/e2e-marker")
            .unwrap()
    };
    write(source, "before");
    let checkpoint = capture_for_test(&fixture.paths, configuration.id(), "Milestone").unwrap();
    write(source, "after");
    let fork = fork_source(
        &ProcessRunner,
        &fixture.paths,
        configuration.id(),
        Some(&checkpoint),
        fork_name,
    )
    .unwrap();
    fork_commit(
        &ProcessRunner,
        &fixture.paths,
        &FakeAssignments::new(&[]),
        &fork,
        fork_name,
    )
    .unwrap();
    super::super::start_disposable_test_import(&fixture.paths, fork_name).unwrap();
    assert_eq!(read(fork_name), "before");
    assert_eq!(read(source), "after");
    let fork_computer = fixture.computer_configuration(fork_name);
    assert!(!crate::computer_use::is_built_in(&fork_computer));
    let inspected = inspect_computer(&ProcessRunner, &fixture.paths, fork_name).unwrap();
    assert!(!has_chatgpt_mount(&inspected.config));
    fixture.stop(fork_name);
    fixture.stop(source);
    restore_with(
        &ProcessRunner,
        &fixture.paths,
        configuration.id(),
        &checkpoint,
    )
    .unwrap();
    super::super::start_disposable_test_import(&fixture.paths, source).unwrap();
    assert_eq!(read(source), "before");
    let inspected = inspect_computer(&ProcessRunner, &fixture.paths, source).unwrap();
    assert!(!has_chatgpt_mount(&inspected.config));
    fixture.stop(source);
    eprintln!(
        "Verified a pre-v4 computer through restart, stop, start, checkpoint, fork and restore."
    );
}

fn has_chatgpt_mount(config: &Value) -> bool {
    config["mounts"].as_array().is_some_and(|mounts| {
        mounts
            .iter()
            .any(|mount| mount["guest"] == "/opt/silo/chatgpt")
    })
}

fn computer_desktop_absent(configuration: &crate::runtime::ComputerConfiguration) -> bool {
    crate::desktop::configuration(configuration).is_none()
}
