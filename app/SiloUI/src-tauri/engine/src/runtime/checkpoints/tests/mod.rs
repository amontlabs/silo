use super::*;

#[test]
fn a_migrated_checkpoint_record_loads() {
    let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
    let paths = migrated.runtime_paths();
    // The record layout is upgraded once at launch, after the vocabulary conversion.
    upgrade_records(paths.metadata.parent().unwrap()).unwrap();
    let record = load(&paths, crate::runtime_migration::vocabulary_tests::ID).unwrap();
    let pending = record.pending_checkpoint_restore.unwrap();
    assert_eq!(pending.source_computer, "dev");
    assert_eq!(pending.state, "full");
}

mod failed_capture_tests;
mod network_live;

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
        kind: "manual".into(),
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
        kind: "manual".into(),
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
        kind: "manual".into(),
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
        assert_eq!(
            restore.iter().any(|arg| arg == "--allow-missing-resources"),
            desired_mode == "disk",
            "native={native_scope}, desired={desired_mode}: {restore:?}"
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
fn a_fork_after_an_identity_save_copies_no_environment() {
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
    let keys = |id: &str| -> Vec<String> {
        load(&paths, id)
            .unwrap()
            .desired_environment
            .into_iter()
            .map(|entry| entry.key)
            .collect()
    };
    assert_eq!(keys(ID), ["PROJECT_MODE"]);
    assert!(keys(child.id()).is_empty(), "a fork copies no environment");
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
fn restoring_a_built_in_computer_adds_no_host_mount() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    pending_import(&paths);
    let runner = RestoreProbe(Mutex::new(Vec::new()));
    assert!(start_pending(&runner, &paths, &built_in_computer()).is_err());
    let calls = runner.0.lock().unwrap();
    let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
    assert!(!restore
        .iter()
        .any(|arg| arg == "-v" || arg.contains("/opt/silo")));
}

/// A built-in computer from before Cua Driver that still records the read-only ChatGPT folder,
/// through an in-place restore: the original is removed, then the checkpoint is restored, which
/// itself fails.
struct LegacySource {
    host: PathBuf,
    state: Mutex<&'static str>,
    recovery: Mutex<Option<String>>,
    calls: Mutex<Vec<Vec<String>>>,
}
impl RuntimeRunner for LegacySource {
    fn run(
        &self,
        _paths: &RuntimePaths,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.calls.lock().unwrap().push(args.to_vec());
        let state = *self.state.lock().unwrap();
        let stdout = match args[0].as_str() {
            "inspect" if state == "Removed" => return Err(error("computer not found")),
            "inspect" => serde_json::json!({"name":"dev","status":state,"config":{
                "labels":{"silo.managed":"true","silo.machine-id":ID},
                "mounts":[
                    {"guest":"/workspace","type":"Owned","storage":{"kind":"disk","capacity_mib":1024}},
                    {"type":"Bind","host":self.host,"guest":"/opt/silo/chatgpt",
                        "options":{"readonly":true}},
                ],
                "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]}}
            }})
            .to_string(),
            "snapshot" if args[1] == "list" => {
                let mut entries = vec![serde_json::json!({"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"})];
                if let Some(id) = self.recovery.lock().unwrap().as_ref() {
                    entries.push(serde_json::json!({"group":"dev","name":id,"scope":"full","availability":"ready"}));
                }
                serde_json::to_string(&entries).unwrap()
            }
            "snapshot" if args[1] == "create" => {
                *self.recovery.lock().unwrap() = Some(args[2].clone());
                String::new()
            }
            "snapshot" if args[1] == "verify" => String::new(),
            "pause" => {
                *self.state.lock().unwrap() = "Paused";
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
            "list" => "[]".into(),
            "restore" => return Err(error("synthetic restore failure")),
            _ => panic!("unexpected command: {args:?}"),
        };
        Ok(CommandOutput {
            stdout,
            stderr: String::new(),
        })
    }
}

#[test]
fn restoring_from_a_computer_with_legacy_mounts_passes_them_again() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![built_in_computer()],
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
        kind: "manual".into(),
    });
    save(&paths, ID, &record).unwrap();
    let published = directory.path().join("published");
    std::fs::create_dir(&published).unwrap();
    let runner = LegacySource {
        host: published.clone(),
        state: Mutex::new("Running"),
        recovery: Mutex::new(None),
        calls: Mutex::new(Vec::new()),
    };
    restore_with(&runner, &paths, ID, "c000000000000000000000000000000").unwrap();
    assert_eq!(*runner.state.lock().unwrap(), "Removed");
    assert!(start_pending(&runner, &paths, &built_in_computer()).is_err());
    let calls = runner.calls.lock().unwrap();
    let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
    let mount = format!("{}:/opt/silo/chatgpt:ro,uid=0,gid=0", published.display());
    assert!(
        restore
            .windows(2)
            .any(|pair| pair == ["-v", mount.as_str()]),
        "{restore:?}"
    );
}

#[test]
fn restoring_a_computer_without_built_in_computer_use_adds_no_mount() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    pending_import(&paths);
    let runner = RestoreProbe(Mutex::new(Vec::new()));
    assert!(start_pending(&runner, &paths, &computer_configuration()).is_err());
    let calls = runner.0.lock().unwrap();
    let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
    assert!(!restore
        .iter()
        .any(|arg| arg == "-v" || arg.contains("/opt/silo")));
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
        kind: "manual".into(),
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
        runtime: None,
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
fn a_fork_point_of_a_running_computer_saves_disks_only_after_a_guest_flush() {
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
    let runner = Runner {
        mount_type: "Owned",
        calls: Mutex::new(Vec::new()),
    };
    capture_fork_point(&runner, &paths, ID).unwrap();
    let calls = runner.calls.lock().unwrap();
    let create = calls
        .iter()
        .find(|call| call[..2] == ["snapshot", "create"])
        .unwrap();
    assert!(!create.iter().any(|arg| arg == "--full"), "{create:?}");
    assert!(create
        .windows(2)
        .any(|pair| pair == ["--guest-flush", "required"]));
    assert!(!calls
        .iter()
        .any(|call| call.iter().any(|arg| arg.contains("drop_caches"))));
    let record = load(&paths, ID).unwrap();
    assert_eq!(record.checkpoints[0].scope, "disk");
    assert_eq!(record.checkpoints[0].name, "Fork point");
}

#[test]
fn only_a_fork_point_checkpoint_saves_disks_only_and_a_warm_one_is_refused() {
    use silo_api::methods::checkpoints::CheckpointKind;
    for kind in [
        CheckpointKind::Manual,
        CheckpointKind::Automatic,
        CheckpointKind::BeforeKeep,
        CheckpointKind::Step,
    ] {
        assert_eq!(capture_policy(kind).unwrap().1, CaptureDepth::WithMemory);
    }
    assert_eq!(
        capture_policy(CheckpointKind::ForkPoint).unwrap(),
        ("manual", CaptureDepth::DisksOnly)
    );
    assert!(capture_policy(CheckpointKind::Warm).is_err());
}

#[test]
fn a_fork_point_of_a_stopped_computer_asks_for_no_guest_flush() {
    let _test_state = crate::test_support::global_state();
    struct Stopped(Runner);
    impl RuntimeRunner for Stopped {
        fn run(
            &self,
            paths: &RuntimePaths,
            args: &[String],
            timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            let mut output = self.0.run(paths, args, timeout)?;
            if args[0] == "inspect" {
                output.stdout = output.stdout.replace("Running", "Stopped");
            }
            Ok(output)
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
    let runner = Stopped(Runner {
        mount_type: "Owned",
        calls: Mutex::new(Vec::new()),
    });
    capture_fork_point(&runner, &paths, ID).unwrap();
    let calls = runner.0.calls.lock().unwrap();
    let create = calls
        .iter()
        .find(|call| call[..2] == ["snapshot", "create"])
        .unwrap();
    assert!(!create
        .iter()
        .any(|arg| arg == "--full" || arg == "--guest-flush"));
    assert_eq!(load(&paths, ID).unwrap().checkpoints[0].scope, "disk");
}

#[test]
fn a_full_capture_runs_nothing_in_the_guest_before_the_snapshot() {
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
    // A folder an earlier engine left in the runtime share changes nothing.
    std::fs::create_dir_all(paths.home.join("sandboxes/dev/runtime/silo-bridge/req")).unwrap();
    let runner = Runner {
        mount_type: "Owned",
        calls: Mutex::new(Vec::new()),
    };
    capture_with(&runner, &paths, ID, "Before work", "manual").unwrap();
    let calls = runner.calls.lock().unwrap();
    assert!(!calls.iter().any(|call| call[0] == "exec"), "{calls:?}");
    assert!(calls.iter().any(|call| call[..2] == ["snapshot", "create"]));
}

#[test]
fn every_fork_scrubs_the_guest_client_token_once() {
    use silo_api::methods::guest_client::{
        GUEST_ENDPOINT_PATH as token, LEGACY_GUEST_TOKEN_PATH as legacy,
    };
    let mut options = ForkOptions::default();
    options.scrub_guest_client_token();
    options.scrub_guest_client_token();
    assert_eq!(options.scrub, [token, legacy]);
    let mut named = ForkOptions {
        scrub: vec!["/root/.bot.env".into(), token.into()],
        ..ForkOptions::default()
    };
    named.scrub_guest_client_token();
    assert_eq!(named.scrub, ["/root/.bot.env", token, legacy]);
    assert!(named.validate().is_ok());
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
        kind: "manual".into(),
    };
    let second = Checkpoint {
        id: "c111111111111111111111111111111".into(),
        native_id: None,
        name: "Second full state".into(),
        created_at: 2,
        scope: "full".into(),
        reason: "manual".into(),
        kind: "manual".into(),
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
        kind: "manual".into(),
    };
    let recovery = Checkpoint {
        id: "c111111111111111111111111111111".into(),
        native_id: None,
        name: "Before restore".into(),
        created_at: 2,
        scope: "full".into(),
        reason: "before-restore".into(),
        kind: "manual".into(),
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
        kind: "manual".into(),
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
        kind: "manual".into(),
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
                kind: "manual".into(),
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
        kind: "manual".into(),
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
    secret_names: Mutex<Vec<String>>,
}
impl FakeAssignments {
    fn new(fail: &'static [&'static str]) -> Self {
        Self {
            fail,
            github: Mutex::new(Vec::new()),
            secrets: Mutex::new(Vec::new()),
            secret_names: Mutex::new(Vec::new()),
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
    fn copy_secrets(&self, _: &str, target: &str, names: &[String]) -> Result<(), String> {
        self.check("copy_secrets")?;
        self.secret_names
            .lock()
            .unwrap()
            .extend(names.iter().cloned());
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
        // The fixture forks name GitHub and one secret, so the copy and rollback paths run.
        options: ForkOptions {
            secrets: vec!["github".into(), "TOKEN".into()],
            ..ForkOptions::default()
        },
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
    assert_eq!(
        (fork.scope.as_str(), pending.state.as_str()),
        ("full", "disk"),
        "a fork of a full checkpoint boots from its disks and never resumes memory"
    );
    assert_eq!(*assignments.github.lock().unwrap(), ["branch"]);
    assert_eq!(*assignments.secrets.lock().unwrap(), ["branch"]);
}

#[test]
fn a_fork_keeps_its_sources_legacy_mounts_for_its_first_start() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, fork) = fork_fixture(&directory);
    let published = directory.path().join("published");
    std::fs::create_dir(&published).unwrap();
    let runner = LegacySource {
        host: published.clone(),
        state: Mutex::new("Running"),
        recovery: Mutex::new(None),
        calls: Mutex::new(Vec::new()),
    };
    fork_commit(&runner, &paths, &FakeAssignments::new(&[]), &fork, "branch").unwrap();
    let child = read_metadata(&paths.metadata)
        .unwrap()
        .computers
        .into_iter()
        .find(|configuration| configuration.name() == "branch")
        .unwrap();
    // The source is deleted before the fork's first start: the child still has the mounts.
    *runner.state.lock().unwrap() = "Removed";
    let mounts = load(&paths, child.id()).unwrap().legacy_mounts;
    assert_eq!(
        crate::legacy_mounts::restore_args_for(&mounts).unwrap(),
        [
            "-v".to_owned(),
            format!("{}:/opt/silo/chatgpt:ro,uid=0,gid=0", published.display())
        ]
    );
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
        ForkOptions::default(),
        &Vec::new,
        None,
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
    /// Layers (head included) the fake runtime reports per disk for `modify --compact`.
    layers: usize,
    /// Layers of the `/workspace` disk when its chain differs from the root's.
    workspace_layers: Option<usize>,
    fail_compact: bool,
    /// The real compaction ends on Silo's own timeout.
    compact_timed_out: bool,
    /// What a poll of a busy runtime reports instead of the client deadline.
    poll_failure: Option<&'static str>,
    /// A real compaction whose reply the client stops waiting for while the runtime goes on.
    slow_compaction: Mutex<SlowCompaction>,
    calls: Mutex<Vec<Vec<String>>>,
}

#[derive(Clone, Copy)]
enum SlowCompaction {
    Off,
    /// The next real compaction hits the client's reply deadline; the runtime then answers
    /// nothing for this many requests and applies the merge of `.1` layers when done.
    Armed(usize, usize),
    Busy(usize, usize),
    Done(usize),
}

impl Store {
    fn new(computers: Vec<(&'static str, Option<&'static str>)>) -> Self {
        Self {
            members: Mutex::new(Vec::new()),
            heads: Mutex::new(HashMap::new()),
            computers,
            fail_create_after_publish: false,
            fail_remove: false,
            layers: 0,
            workspace_layers: None,
            fail_compact: false,
            compact_timed_out: false,
            poll_failure: None,
            slow_compaction: Mutex::new(SlowCompaction::Off),
            calls: Mutex::new(Vec::new()),
        }
    }
    /// The reply deadline expires; the runtime answers again after `busy` requests, having
    /// merged `merged` layers into one.
    fn slow_compaction(self, busy: usize, merged: usize) -> Self {
        *self.slow_compaction.lock().unwrap() = SlowCompaction::Armed(busy, merged);
        self
    }
    fn with_layers(mut self, layers: usize) -> Self {
        self.layers = layers;
        self
    }
    fn failing_compaction(mut self) -> Self {
        self.fail_compact = true;
        self
    }
    /// The disk and `--layers` of every real (not dry-run) compaction.
    fn compaction_calls(&self) -> Vec<(Option<String>, usize)> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| {
                call[0] == "modify"
                    && call.contains(&"--compact".into())
                    && !call.contains(&"--dry-run".into())
            })
            .map(|call| {
                let value = |flag: &str| {
                    call.iter()
                        .position(|arg| arg == flag)
                        .map(|at| call[at + 1].clone())
                };
                (value("--disk"), value("--layers").unwrap().parse().unwrap())
            })
            .collect()
    }
    /// The `--layers` of every real compaction.
    fn compactions(&self) -> Vec<usize> {
        self.compaction_calls()
            .into_iter()
            .map(|(_, layers)| layers)
            .collect()
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
            .map(|call| {
                call.iter()
                    .skip(2)
                    .find(|arg| !arg.starts_with("--"))
                    .unwrap()
                    .clone()
            })
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
                ("modify", Some(name)) if args.contains(&"--compact".into()) => {
                    let layers = args.iter().position(|arg| arg == "--layers").map(|at| args[at + 1].parse::<usize>().unwrap());
                    if args.contains(&"--dry-run".into()) {
                        let mut slow = self.slow_compaction.lock().unwrap();
                        match *slow {
                            SlowCompaction::Busy(0, merged) => *slow = SlowCompaction::Done(merged),
                            SlowCompaction::Busy(left, merged) => {
                                *slow = SlowCompaction::Busy(left - 1, merged);
                                if let Some(text) = self.poll_failure {
                                    // msb reports a client-side failure as a failed command.
                                    return Err(if text.starts_with("control client error") {
                                        RuntimeError::Failed {
                                            operation: "modify".into(),
                                            exit_code: Some(1),
                                            detail: text.into(),
                                        }
                                    } else {
                                        RuntimeError::Unavailable(text.into())
                                    });
                                }
                                return Err(RuntimeError::Failed {
                                    operation: "modify".into(),
                                    exit_code: Some(1),
                                    detail: "control client error: local deadline expired (delivery: NotSent)".into(),
                                });
                            }
                            _ => {}
                        }
                        let merged_away = match *slow {
                            SlowCompaction::Done(merged) => merged.saturating_sub(1),
                            _ => 0,
                        };
                        drop(slow);
                        let only = args.iter().position(|arg| arg == "--disk").map(|at| args[at + 1].clone());
                        let disk = |guest: &str, total: usize| {
                            let total = total - merged_away;
                            let selected = layers.unwrap_or(total.saturating_sub(1)).min(total.saturating_sub(1));
                            serde_json::json!({"guest_path": guest, "input_layers": total, "selected_layers": selected, "output_layers": total - selected + 1})
                        };
                        let workspace = self.workspace_layers.unwrap_or(self.layers);
                        let disks: Vec<_> = [("/", self.layers), ("/workspace", workspace)]
                            .into_iter()
                            .filter(|(guest, _)| only.as_deref().is_none_or(|only| only == *guest))
                            .map(|(guest, total)| disk(guest, total))
                            .collect();
                        return ok(serde_json::json!({"dry_run": true, "disks": disks}).to_string());
                    }
                    assert_eq!(name, "dev");
                    let mut slow = self.slow_compaction.lock().unwrap();
                    if let SlowCompaction::Armed(busy, merged) = *slow {
                        *slow = SlowCompaction::Busy(busy, merged);
                        return Err(RuntimeError::Failed {
                            operation: "modify".into(),
                            exit_code: Some(1),
                            detail: "control client error: local deadline expired (delivery: Unknown)".into(),
                        });
                    }
                    drop(slow);
                    if self.compact_timed_out {
                        return Err(RuntimeError::TimedOut { operation: "modify".into() });
                    }
                    if self.fail_compact {
                        return Err(RuntimeError::Unavailable("compaction of / failed: injected; VM kept paused; restart follows per-disk journals".into()));
                    }
                    ok("{}".into())
                }
                ("snapshot", Some("remove")) => {
                    let force = args.contains(&"--force".into());
                    let selector = args.iter().skip(2).find(|arg| !arg.starts_with("--")).unwrap();
                    let (group, name) = selector.split_once(':').unwrap();
                    let mut members = self.members.lock().unwrap();
                    let index = members.iter().position(|member| member.0 == group && member.1 == name)
                        .ok_or_else(|| RuntimeError::Unavailable("snapshot not found".into()))?;
                    let id = members[index].2.clone();
                    if self.fail_remove {
                        return Err(RuntimeError::Unavailable("permission denied".into()));
                    }
                    if !force && members.iter().any(|member| member.3.as_deref() == Some(id.as_str())) {
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
        kind: "manual".into(),
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
            runtime: None,
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

const IMPORT_GROUP: &str = "silo-import-0123456789abcdef0123456789abcdef";
const OTHER_IMPORT_GROUP: &str = "silo-import-fedcba9876543210fedcba9876543210";

#[test]
fn deleting_an_imported_computer_removes_its_whole_import_group() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let mut imported = Record::default();
    imported.snapshot_group = Some(IMPORT_GROUP.into());
    let paths = delete_fixture(&directory, Vec::new(), None);
    save(&paths, ID, &imported).unwrap();
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: Vec::new(),
        },
    )
    .unwrap();
    // The archive carried the head and an ancestor named after the exporting computer's
    // fork point; neither is a name Silo gives its own captures.
    let store = Store::new(Vec::new())
        .with(IMPORT_GROUP, "fork-point-7", "snap_p", None)
        .with(IMPORT_GROUP, "silo-backup-0-1-2", "snap_h", Some("snap_p"))
        .with("elsewhere", C, "snap_c", None);

    remove_deleted_snapshots(&store, &paths, ID, "dev").unwrap();

    assert_eq!(store.names(), [C]);
}

#[test]
fn the_sweep_removes_import_groups_no_computer_owns() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let mut owner = Record::default();
    owner.snapshot_group = Some(OTHER_IMPORT_GROUP.into());
    let paths = delete_fixture(&directory, Vec::new(), Some(owner));
    let store = Store::new(Vec::new())
        .with(IMPORT_GROUP, "fork-point-7", "snap_p", None)
        .with(IMPORT_GROUP, "silo-backup-0-1-2", "snap_h", Some("snap_p"))
        .with(OTHER_IMPORT_GROUP, "silo-backup-0-1-2", "snap_o", None)
        .with("elsewhere", C, "snap_c", None);

    recover_interrupted(&store, &paths).unwrap();

    // The group a record names, and groups that are not imports, stay.
    assert_eq!(store.names(), ["silo-backup-0-1-2", C]);
    assert!(store
        .members
        .lock()
        .unwrap()
        .iter()
        .all(|member| member.0 != IMPORT_GROUP));
}

#[test]
fn a_member_released_later_is_removed_through_the_runtime() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = delete_fixture(&directory, Vec::new(), None);
    let store = Store::new(Vec::new()).with(IMPORT_GROUP, "fork-point-7", "snap_p", None);
    // The member was queued when its computer was dropped and kept until now.
    save_cleanup(&paths, &native::inventory(&store, &paths).unwrap()).unwrap();

    retry_deleted_snapshots(&store, &paths).unwrap();

    // The empty group folder MicroSandbox leaves (about 4 KB) is not removed by Silo.
    assert!(store.names().is_empty());
}

#[test]
fn the_launch_upgrade_moves_a_layout_two_history_to_layout_three() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let storage = directory.path();
    fs::create_dir_all(storage.join("checkpoints")).unwrap();
    let file = storage.join("checkpoints").join(format!("{ID}.json"));
    let mut record = Record::default();
    record.snapshot_group = Some("dev".into());
    record.checkpoints = vec![entry(A, "Mine", "manual"), automatic_entry(B, 5)];
    record.version = 2;
    fs::write(&file, serde_json::to_vec(&record).unwrap()).unwrap();

    assert_eq!(upgrade_records(storage).unwrap(), 1);

    let upgraded: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(upgraded["version"], 3);
    assert_eq!(upgraded["checkpoints"][1]["kind"], "automatic");
    assert_eq!(
        upgrade_records(storage).unwrap(),
        0,
        "a layout-three file is left alone"
    );
    let mut with_before_keep = Record::default();
    with_before_keep
        .checkpoints
        .push(before_keep_entry(&native_id(1), 5));
    with_before_keep.version = RECORD_VERSION;
    validate(&with_before_keep).unwrap();
    with_before_keep.version = 2;
    assert!(
        validate(&with_before_keep).is_err(),
        "an old layout is refused"
    );
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
        kind: "manual".into(),
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
/// computer use ready, nothing mounted from the host, and the approval mode
/// kept. Needs the same inputs as the other built-in live tests (see
/// `test_support::computer_use_live`).
#[test]
#[ignore = "requires the v4 guest image, network access and hardware virtualization"]
fn live_built_in_lifecycle_keeps_the_desktop_and_computer_use() {
    use crate::test_support::computer_use_live::Fixture;
    let _test_state = crate::test_support::global_state();
    let mut fixture = Fixture::new("silo-life-", None);
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
        let mounts = fixture.exec_status(name, "grep -c ' /opt/silo/' /proc/mounts");
        assert!(mounts.starts_with('0'), "{label}: {mounts}");
        // Not only what Silo reports: Cua Driver's own readiness check passes.
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
    let mut fixture = Fixture::new("silo-v3-", Some(image));
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

// MARK: Automatic checkpoints and retention

fn automatic_entry(id: &str, created_at: u64) -> Checkpoint {
    Checkpoint {
        created_at,
        kind: "automatic".into(),
        ..entry(
            id,
            &format!("Before run: {created_at}, deleted automatically"),
            "manual",
        )
    }
}

fn native_id(index: u64) -> String {
    format!("c{index:031x}")
}

fn before_keep_entry(id: &str, created_at: u64) -> Checkpoint {
    Checkpoint {
        created_at,
        kind: "before-keep".into(),
        ..entry(id, &format!("Before keep: {created_at}"), "manual")
    }
}

fn trim_before_keep(store: &dyn RuntimeRunner, paths: &RuntimePaths, count: u32) {
    let gate = super::super::operation_gate::OperationGate::new();
    prune_before_keep_with(
        &gate,
        store,
        paths,
        ID,
        &|| Ok(count),
        silo_api::jobs::JobId::new("job:9").unwrap(),
    );
}

fn step_entry(id: &str, created_at: u64) -> Checkpoint {
    Checkpoint {
        created_at,
        kind: "step".into(),
        ..entry(id, &format!("Step {created_at}"), "manual")
    }
}

#[test]
fn step_retention_keeps_the_newest_step_checkpoints_and_leaves_other_kinds() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let mut entries: Vec<Checkpoint> = (1..=4).map(|n| step_entry(&native_id(n), n * 10)).collect();
    entries.push(entry(&native_id(9), "Mine", "manual"));
    entries.push(before_keep_entry(&native_id(8), 5));
    let paths = delete_fixture(&directory, entries, None);
    let store = Store::new(vec![("dev", None)])
        .with("dev", &native_id(1), "snap_1", None)
        .with("dev", &native_id(2), "snap_2", None)
        .with("dev", &native_id(3), "snap_3", None)
        .with("dev", &native_id(4), "snap_4", None)
        .with("dev", &native_id(9), "snap_9", None)
        .with("dev", &native_id(8), "snap_8", None);
    let gate = super::super::operation_gate::OperationGate::new();
    trim_kind_with(
        &gate,
        &store,
        &paths,
        ID,
        "step",
        "Removing an older step checkpoint",
        &|| Ok(2),
        silo_api::jobs::JobId::new("job:9").unwrap(),
    )
    .unwrap();
    assert_eq!(
        store.removals(),
        [
            format!("dev:{}", native_id(1)),
            format!("dev:{}", native_id(2)),
        ]
    );
    let left: Vec<_> = load(&paths, ID)
        .unwrap()
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.clone())
        .collect();
    assert_eq!(
        left,
        [native_id(3), native_id(4), native_id(9), native_id(8)]
    );
    // Zero keeps every step checkpoint.
    let keep_all = delete_fixture(
        &tempfile::tempdir().unwrap(),
        vec![step_entry(&native_id(1), 10), step_entry(&native_id(2), 20)],
        None,
    );
    let untouched = Store::new(vec![("dev", None)])
        .with("dev", &native_id(1), "snap_1", None)
        .with("dev", &native_id(2), "snap_2", None);
    trim_kind_with(
        &gate,
        &untouched,
        &keep_all,
        ID,
        "step",
        "Removing an older step checkpoint",
        &|| Ok(0),
        silo_api::jobs::JobId::new("job:9").unwrap(),
    )
    .unwrap();
    assert!(untouched.removals().is_empty());
}

#[test]
fn before_keep_retention_removes_a_chain_oldest_first_with_force() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let mut entries: Vec<Checkpoint> = (1..=3)
        .map(|n| before_keep_entry(&native_id(n), n * 10))
        .collect();
    // Newer records outside the chain stay; the newest before-keep one is the retained one.
    entries.push(entry(&native_id(9), "Mine", "manual"));
    entries.push(before_keep_entry(&native_id(8), 80));
    let paths = delete_fixture(&directory, entries, None);
    let store = Store::new(vec![("dev", None)])
        .with("dev", &native_id(1), "snap_1", None)
        .with("dev", &native_id(2), "snap_2", Some("snap_1"))
        .with("dev", &native_id(3), "snap_3", Some("snap_2"))
        .with("dev", &native_id(9), "snap_9", None)
        .with("dev", &native_id(8), "snap_8", None);
    trim_before_keep(&store, &paths, 1);

    assert_eq!(
        store.removals(),
        [
            format!("dev:{}", native_id(1)),
            format!("dev:{}", native_id(2)),
            format!("dev:{}", native_id(3)),
        ],
        "oldest first"
    );
    assert!(store
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|call| call[0] == "snapshot" && call[1] == "remove")
        .all(|call| call.iter().any(|arg| arg == "--force")));
    let left: Vec<_> = load(&paths, ID)
        .unwrap()
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.clone())
        .collect();
    assert_eq!(left, [native_id(9), native_id(8)]);
}

#[test]
fn before_keep_retention_removes_the_bases_a_retained_checkpoint_builds_on() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let entries: Vec<Checkpoint> = (1..=3)
        .map(|n| before_keep_entry(&native_id(n), n * 10))
        .collect();
    let paths = delete_fixture(&directory, entries, None);
    // 3 is retained and builds on 2, which builds on 1. Parent links are lineage only: each
    // snapshot holds its own layers, so both older ones go.
    let store = Store::new(vec![("dev", None)])
        .with("dev", &native_id(1), "snap_1", None)
        .with("dev", &native_id(2), "snap_2", Some("snap_1"))
        .with("dev", &native_id(3), "snap_3", Some("snap_2"));

    trim_before_keep(&store, &paths, 1);

    assert_eq!(
        store.removals(),
        [
            format!("dev:{}", native_id(1)),
            format!("dev:{}", native_id(2))
        ]
    );
    assert_eq!(store.names(), [native_id(3)]);
    assert_eq!(load(&paths, ID).unwrap().checkpoints.len(), 1);
}

#[test]
fn before_keep_retention_trims_only_its_own_kind_beyond_the_newest_count() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let mut entries: Vec<Checkpoint> = (1..=7)
        .map(|n| before_keep_entry(&native_id(n), n * 10))
        .collect();
    entries.push(entry(&native_id(90), "Mine", "manual"));
    entries.push(automatic_entry(&native_id(91), 5));
    let paths = delete_fixture(&directory, entries, None);
    let mut store = Store::new(vec![("dev", None)]);
    for n in [90, 91, 1, 2, 3, 4, 5, 6, 7] {
        store = store.with("dev", &native_id(n), &format!("snap_{n}"), None);
    }

    trim_before_keep(&store, &paths, 5);

    let mut removed = store.removals();
    removed.sort();
    assert_eq!(
        removed,
        [
            format!("dev:{}", native_id(1)),
            format!("dev:{}", native_id(2))
        ]
    );
    let record = load(&paths, ID).unwrap();
    let mut left: Vec<_> = record.checkpoints.iter().map(|c| c.id.clone()).collect();
    left.sort();
    assert_eq!(
        left,
        [3, 4, 5, 6, 7, 90, 91].map(native_id),
        "manual and automatic records are never trimmed"
    );
}

#[test]
fn a_before_keep_count_of_zero_keeps_every_checkpoint() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let entries: Vec<Checkpoint> = (1..=3)
        .map(|n| before_keep_entry(&native_id(n), n * 10))
        .collect();
    let paths = delete_fixture(&directory, entries, None);
    let mut store = Store::new(vec![("dev", None)]);
    for n in 1..=3 {
        store = store.with("dev", &native_id(n), &format!("snap_{n}"), None);
    }

    trim_before_keep(&store, &paths, 0);

    assert!(store.removals().is_empty());
    assert_eq!(load(&paths, ID).unwrap().checkpoints.len(), 3);
}

#[test]
fn a_before_keep_capture_persists_its_policy_kind_and_reaches_the_api() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = delete_fixture(&directory, Vec::new(), None);
    let store = Store::new(vec![("dev", None)]);

    let captured = capture_with_kind(
        &store,
        &paths,
        ID,
        "Before keep",
        "manual",
        "before-keep",
        CaptureDepth::WithMemory,
    )
    .unwrap();

    assert_eq!(load(&paths, ID).unwrap().checkpoints[0].kind, "before-keep");
    assert_eq!(
        api_checkpoint(captured).kind,
        silo_api::methods::checkpoints::CheckpointKind::BeforeKeep
    );
    let kind: silo_api::methods::checkpoints::CheckpointKind =
        serde_json::from_str("\"before-keep\"").unwrap();
    assert_eq!(serde_json::to_string(&kind).unwrap(), "\"before-keep\"");
}

#[test]
fn an_automatic_capture_returns_its_record_and_persists_its_policy_kind() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = delete_fixture(&directory, Vec::new(), None);
    let store = Store::new(vec![("dev", None)]);

    let captured = capture_with_kind(
        &store,
        &paths,
        ID,
        "Before run: tests, deleted automatically",
        "manual",
        "automatic",
        CaptureDepth::WithMemory,
    )
    .unwrap();

    assert_eq!(captured.kind, "automatic");
    let record = load(&paths, ID).unwrap();
    assert_eq!(record.checkpoints.len(), 1);
    assert_eq!(record.checkpoints[0].id, captured.id);
    assert_eq!(record.checkpoints[0].kind, "automatic");
    let api = api_checkpoint(captured);
    assert_eq!(
        api.kind,
        silo_api::methods::checkpoints::CheckpointKind::Automatic
    );
    assert_eq!(
        api.reason,
        silo_api::methods::checkpoints::CheckpointReason::Manual
    );
}

#[test]
fn the_canonical_computer_list_carries_checkpoints_in_the_api_shape() {
    let mut computer = json!({"checkpoints": [{
        "id": "c1", "name": "Before run: x, deleted automatically",
        "createdAt": 1791986953387u64, "scope": "full", "reason": "manual", "kind": "automatic"
    }]});
    super::super::canonical_checkpoint_times(&mut computer);
    let back: Vec<silo_api::methods::checkpoints::Checkpoint> =
        serde_json::from_value(computer["checkpoints"].clone()).unwrap();
    assert_eq!(back[0].created_at, "2026-10-14T14:09:13.387Z");
}

#[test]
fn retention_removes_only_automatic_records_beyond_the_newest_count() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let mut entries: Vec<Checkpoint> = (1..=5)
        .map(|n| automatic_entry(&native_id(n), n * 10))
        .collect();
    // Manual and before-restore records are older than every automatic one and never touched.
    entries.push(entry(&native_id(90), "Mine", "manual"));
    entries.push(entry(&native_id(91), "Before restore", "before-restore"));
    let paths = delete_fixture(&directory, entries, None);
    let mut store = Store::new(vec![("dev", None)]);
    for n in [90, 91, 1, 2, 3, 4, 5] {
        store = store.with("dev", &native_id(n), &format!("snap_{n}"), None);
    }
    // The head is the newest member; the others can be removed one by one.
    store
        .heads
        .lock()
        .unwrap()
        .insert("dev".into(), "snap_5".into());
    let gate = super::super::operation_gate::OperationGate::new();

    prune_automatic_with(
        &gate,
        &store,
        &paths,
        ID,
        &|| Ok(3),
        silo_api::jobs::JobId::new("job:1").unwrap(),
    );

    let record = load(&paths, ID).unwrap();
    let mut left: Vec<_> = record.checkpoints.iter().map(|c| c.id.clone()).collect();
    left.sort();
    assert_eq!(
        left,
        [
            native_id(3),
            native_id(4),
            native_id(5),
            native_id(90),
            native_id(91)
        ]
    );
    assert_eq!(store.removals().len(), 2);
}

#[test]
fn retention_stops_when_the_setting_is_turned_off_before_the_work_is_admitted() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let entries: Vec<Checkpoint> = (1..=4)
        .map(|n| automatic_entry(&native_id(n), n * 10))
        .collect();
    let paths = delete_fixture(&directory, entries, None);
    let mut store = Store::new(vec![("dev", None)]);
    for n in 1..=4 {
        store = store.with("dev", &native_id(n), &format!("snap_{n}"), None);
    }
    let gate = super::super::operation_gate::OperationGate::new();
    let reads = std::sync::atomic::AtomicUsize::new(0);
    // The first read (the plan) sees 1; the read once the work is admitted sees "off".
    let setting = || {
        Ok(
            if reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 1 {
                1
            } else {
                0
            },
        )
    };

    prune_automatic_with(
        &gate,
        &store,
        &paths,
        ID,
        &setting,
        silo_api::jobs::JobId::new("job:2").unwrap(),
    );

    assert!(store.removals().is_empty());
    assert_eq!(load(&paths, ID).unwrap().checkpoints.len(), 4);
}

/// Runs retention with the device lane held, waits (without sleeping) until its first
/// deletion is queued, changes the setting to `after`, then releases the lane.
fn prune_while_queued(initial: u32, after: u32, automatic: usize) -> (Store, usize) {
    let directory = tempfile::tempdir().unwrap();
    let entries: Vec<Checkpoint> = (1..=automatic as u64)
        .map(|n| automatic_entry(&native_id(n), n * 10))
        .collect();
    let paths = delete_fixture(&directory, entries, None);
    let mut store = Store::new(vec![("dev", None)]);
    for n in 1..=automatic as u64 {
        store = store.with("dev", &native_id(n), &format!("snap_{n}"), None);
    }
    let gate = super::super::operation_gate::OperationGate::new();
    let setting = std::sync::atomic::AtomicU32::new(initial);
    let hold = gate.device("Holding the lane").unwrap();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            prune_automatic_with(
                &gate,
                &store,
                &paths,
                ID,
                &|| Ok(setting.load(std::sync::atomic::Ordering::SeqCst)),
                silo_api::jobs::JobId::new("job:7").unwrap(),
            );
        });
        while gate.snapshot().waiting.is_empty() && !worker.is_finished() {
            std::thread::yield_now();
        }
        assert!(!worker.is_finished(), "the deletion never queued");
        setting.store(after, std::sync::atomic::Ordering::SeqCst);
        drop(hold);
        worker.join().unwrap();
    });
    let left = load(&paths, ID).unwrap().checkpoints.len();
    (store, left)
}

#[test]
fn retention_turned_off_while_the_deletion_queues_deletes_nothing() {
    let _test_state = crate::test_support::global_state();
    let (store, left) = prune_while_queued(2, 0, 5);
    assert!(store.removals().is_empty());
    assert_eq!(left, 5);
}

#[test]
fn retention_raised_while_the_deletion_queues_keeps_what_the_new_count_retains() {
    let _test_state = crate::test_support::global_state();
    let (store, left) = prune_while_queued(2, 5, 5);
    assert!(store.removals().is_empty());
    assert_eq!(left, 5);
    // Lowered while queued, the plan is made again with the new count.
    let (store, left) = prune_while_queued(4, 2, 5);
    assert_eq!(store.removals().len(), 3);
    assert_eq!(left, 2);
}

#[test]
fn retention_removes_a_chained_old_record_and_keeps_the_newest() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let entries: Vec<Checkpoint> = (1..=4)
        .map(|n| automatic_entry(&native_id(n), n * 10))
        .collect();
    let paths = delete_fixture(&directory, entries, None);
    // Checkpoint 2 builds on checkpoint 1.
    let store = Store::new(vec![("dev", None)])
        .with("dev", &native_id(1), "snap_1", None)
        .with("dev", &native_id(2), "snap_2", Some("snap_1"))
        .with("dev", &native_id(3), "snap_3", None)
        .with("dev", &native_id(4), "snap_4", None);
    let gate = super::super::operation_gate::OperationGate::new();

    prune_automatic_with(
        &gate,
        &store,
        &paths,
        ID,
        &|| Ok(2),
        silo_api::jobs::JobId::new("job:3").unwrap(),
    );

    let mut left: Vec<_> = load(&paths, ID)
        .unwrap()
        .checkpoints
        .iter()
        .map(|c| c.id.clone())
        .collect();
    left.sort();
    // 1 and 2 go oldest first; 3 and 4 are the kept newest pair.
    assert_eq!(left, [native_id(3), native_id(4)]);
    assert_eq!(
        store.removals(),
        [
            format!("dev:{}", native_id(1)),
            format!("dev:{}", native_id(2))
        ]
    );
}

// MARK: Retention that returns disk

/// A linear chain `snap_1 <- ... <- snap_n`, one checkpoint per entry of `kinds`, in the
/// group `dev`, the computer positioned on `snap_n` with `n + 1` layers (the head last).
fn chain_fixture(
    directory: &tempfile::TempDir,
    kinds: &[&str],
    fork: Option<Record>,
) -> (RuntimePaths, Store) {
    let n = kinds.len() as u64;
    let entries: Vec<Checkpoint> = kinds
        .iter()
        .enumerate()
        .map(|(index, kind)| Checkpoint {
            created_at: (index as u64 + 1) * 10,
            kind: (*kind).into(),
            ..entry(
                &native_id(index as u64 + 1),
                &format!("Checkpoint {}", index + 1),
                "manual",
            )
        })
        .collect();
    let paths = delete_fixture(directory, entries, fork);
    // Created from an image: no `snapshot_parent`; the capture cursor names the latest capture.
    cursor(&paths, "dev", &format!("snap_{n}"));
    let mut store = Store::new(vec![("dev", None)]).with_layers(n as usize + 1);
    for index in 1..=n {
        let parent = (index > 1).then(|| format!("snap_{}", index - 1));
        store = store.with(
            "dev",
            &native_id(index),
            &format!("snap_{index}"),
            parent.as_deref(),
        );
    }
    (paths, store)
}

fn trim_automatic(store: &dyn RuntimeRunner, paths: &RuntimePaths, count: u32) {
    let gate = super::super::operation_gate::OperationGate::new();
    prune_automatic_with(
        &gate,
        store,
        paths,
        ID,
        &|| Ok(count),
        silo_api::jobs::JobId::new("job:20").unwrap(),
    );
}

fn left_ids(paths: &RuntimePaths) -> Vec<String> {
    load(paths, ID)
        .unwrap()
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.clone())
        .collect()
}

#[test]
fn retention_merges_the_layers_older_than_the_oldest_kept_checkpoint_before_removing() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, store) = chain_fixture(&directory, &["automatic"; 6], None);

    trim_automatic(&store, &paths, 2);

    // 6 captures plus the head are 7 layers; snap_5 (two newest kept) sealed layer 4, so
    // layers 0..=3 (captures 1-4) are merged.
    assert_eq!(store.compactions(), [4]);
    assert_eq!(
        store.removals(),
        (1..=4)
            .map(|n| format!("dev:{}", native_id(n)))
            .collect::<Vec<_>>(),
        "oldest first"
    );
    let calls = store.calls.lock().unwrap().clone();
    let compact_at = calls
        .iter()
        .position(|call| call[0] == "modify" && !call.contains(&"--dry-run".into()))
        .unwrap();
    let first_removal = calls
        .iter()
        .position(|call| call.get(1).is_some_and(|word| word == "remove"))
        .unwrap();
    assert!(
        compact_at < first_removal,
        "the merge runs before any removal"
    );
    assert!(calls
        .iter()
        .filter(|call| call.get(1).is_some_and(|word| word == "remove"))
        .all(|call| call.contains(&"--force".into())));
    assert_eq!(left_ids(&paths), [native_id(5), native_id(6)]);
}

#[test]
fn retention_skips_the_merge_when_fewer_than_two_layers_would_be_merged() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    // Keeping the newest of 2: snap_2 sealed layer 1, so only one layer is older.
    let (paths, store) = chain_fixture(&directory, &["automatic"; 2], None);

    trim_automatic(&store, &paths, 1);

    assert!(store.compactions().is_empty());
    assert_eq!(store.removals(), [format!("dev:{}", native_id(1))]);
    assert_eq!(left_ids(&paths).len(), 1);
}

#[test]
fn a_failed_merge_removes_nothing_and_is_recorded_without_a_retry() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, store) = chain_fixture(&directory, &["automatic"; 6], None);
    let store = store.failing_compaction();

    trim_automatic(&store, &paths, 2);

    assert!(store.removals().is_empty());
    let attempts = store
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|call| call[0] == "modify" && !call.contains(&"--dry-run".into()))
        .count();
    assert_eq!(attempts, 1);
    let record = load(&paths, ID).unwrap();
    assert_eq!(record.checkpoints.len(), 6);
    let operation = record.checkpoint_operation.unwrap();
    assert_eq!(operation.status, "failed");
    assert_eq!(operation.stage, reclaim::RECLAIM_STAGE);
    let message = operation.error.unwrap();
    assert!(message.contains("kept"), "{message}");
    assert!(message.contains("Restart dev"), "{message}");
}

#[test]
fn a_merge_the_client_stopped_waiting_for_is_awaited_and_retention_goes_on() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, store) = chain_fixture(&directory, &["automatic"; 6], None);
    let store = store.slow_compaction(3, 4);

    trim_automatic(&store, &paths, 2);

    assert_eq!(store.compactions(), [4], "the merge is not issued twice");
    assert_eq!(
        store.removals(),
        (1..=4)
            .map(|n| format!("dev:{}", native_id(n)))
            .collect::<Vec<_>>()
    );
    let record = load(&paths, ID).unwrap();
    assert_eq!(left_ids(&paths), [native_id(5), native_id(6)]);
    assert!(
        record.checkpoint_operation.is_none(),
        "{:?}",
        record.checkpoint_operation
    );
}

#[test]
fn a_merge_that_never_ran_after_the_wait_keeps_every_checkpoint_and_is_recorded() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, store) = chain_fixture(&directory, &["automatic"; 6], None);
    // The runtime comes back with the same layers: nothing was merged.
    let store = store.slow_compaction(1, 1);

    trim_automatic(&store, &paths, 2);

    assert!(store.removals().is_empty());
    assert_eq!(load(&paths, ID).unwrap().checkpoints.len(), 6);
    let operation = load(&paths, ID).unwrap().checkpoint_operation.unwrap();
    assert_eq!(operation.status, "failed");
    assert_eq!(operation.stage, reclaim::RECLAIM_STAGE);
    assert!(operation.error.unwrap().contains("did not merge"));
}

fn dry_runs(store: &Store) -> usize {
    store
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|call| call[0] == "modify" && call.contains(&"--dry-run".into()))
        .count()
}

#[test]
fn silos_own_merge_timeout_does_not_start_another_wait() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, mut store) = chain_fixture(&directory, &["automatic"; 6], None);
    store.compact_timed_out = true;
    let planning = {
        trim_automatic(&store, &paths, 2);
        dry_runs(&store)
    };

    assert!(store.removals().is_empty());
    // Only the planning and confirmation dry runs ran; nothing polled afterwards.
    assert_eq!(planning, dry_runs(&store));
    let calls = store.calls.lock().unwrap();
    let merge = calls
        .iter()
        .position(|call| call[0] == "modify" && !call.contains(&"--dry-run".into()))
        .unwrap();
    assert!(calls[merge + 1..]
        .iter()
        .all(|call| !(call[0] == "modify" && call.contains(&"--dry-run".into()))));
    drop(calls);
    assert_eq!(
        load(&paths, ID)
            .unwrap()
            .checkpoint_operation
            .unwrap()
            .status,
        "failed"
    );
}

#[test]
fn a_poll_that_reports_a_paused_computer_ends_the_wait_with_the_restart_hint() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, mut store) = chain_fixture(&directory, &["automatic"; 6], None);
    store.poll_failure = Some("the sandbox is paused");
    let store = store.slow_compaction(5, 4);

    trim_automatic(&store, &paths, 2);

    assert!(store.removals().is_empty());
    let message = load(&paths, ID)
        .unwrap()
        .checkpoint_operation
        .unwrap()
        .error
        .unwrap();
    assert!(message.contains("Restart dev"), "{message}");
}

#[test]
fn a_missing_control_endpoint_while_the_computer_runs_is_a_merge_still_running() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, mut store) = chain_fixture(&directory, &["automatic"; 6], None);
    // Observed with msb 0.7.6: after the client's 30 s deadline, polls of a runtime that is
    // still merging fail with the missing endpoint, not with the deadline.
    store.poll_failure =
        Some("control client error: transport I/O failed (NotFound) (delivery: NotSent)");
    let store = store.slow_compaction(5, 4);

    trim_automatic(&store, &paths, 2);

    assert!(!store.removals().is_empty());
    assert!(load(&paths, ID).unwrap().checkpoint_operation.is_none());
}

#[test]
fn a_failed_pass_leaves_the_next_pass_free_to_merge_and_remove() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, store) = chain_fixture(&directory, &["automatic"; 6], None);
    let store = store.slow_compaction(1, 1);
    trim_automatic(&store, &paths, 2);
    assert_eq!(
        load(&paths, ID)
            .unwrap()
            .checkpoint_operation
            .unwrap()
            .status,
        "failed"
    );

    // The next pass starts clean: the runtime answers and this time the merge applies.
    let again = Store {
        slow_compaction: Mutex::new(SlowCompaction::Off),
        ..store
    };
    trim_automatic(&again, &paths, 2);

    assert_eq!(left_ids(&paths), [native_id(5), native_id(6)]);
    assert!(load(&paths, ID).unwrap().checkpoint_operation.is_none());
}

#[test]
fn retention_never_removes_the_newest_checkpoint_or_another_kind() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let kinds = [
        "automatic",
        "before-keep",
        "automatic",
        "before-keep",
        "automatic",
        "before-keep",
    ];
    let (paths, store) = chain_fixture(&directory, &kinds, None);

    trim_automatic(&store, &paths, 1);

    assert_eq!(
        store.removals(),
        [native_id(1), native_id(3)].map(|id| format!("dev:{id}"))
    );
    assert_eq!(
        left_ids(&paths),
        [2, 4, 5, 6].map(native_id),
        "the newest automatic record and every before-keep one remain"
    );
    // snap_2 sits at layer 1 and cannot bound a merge; snap_4 (layer 3) does.
    assert_eq!(store.compactions(), [3]);
}

#[test]
fn retention_keeps_a_checkpoint_a_named_checkpoint_builds_on() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let kinds = ["automatic", "manual", "automatic", "automatic", "automatic"];
    let (paths, store) = chain_fixture(&directory, &kinds, None);

    trim_automatic(&store, &paths, 2);

    // The named checkpoint 2 builds on 1, so 1 stays; nothing named builds on 3.
    assert_eq!(store.removals(), [format!("dev:{}", native_id(3))]);
    assert_eq!(left_ids(&paths), [1, 2, 4, 5].map(native_id));
}

#[test]
fn retention_keeps_a_checkpoint_a_fork_computer_or_an_export_depends_on() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let fork = Record {
        snapshot_group: Some("dev".into()),
        pending_checkpoint_restore: Some(PendingRestore {
            checkpoint_id: native_id(1),
            source_computer: "dev".into(),
            state: "full".into(),
        }),
        ..Record::default()
    };
    let (paths, store) = chain_fixture(&directory, &["automatic"; 4], Some(fork));
    let store = Store {
        computers: vec![("dev", Some("snap_4")), ("branch", None)],
        ..store
    };
    store.members.lock().unwrap().push((
        "dev".into(),
        "silo-backup-1-2-3".into(),
        "snap_export".into(),
        Some("snap_2".into()),
    ));

    trim_automatic(&store, &paths, 1);

    // 1 starts a fork and 2 has an export child, so both stay; 4 is the newest.
    assert_eq!(store.removals(), [format!("dev:{}", native_id(3))]);
    assert_eq!(left_ids(&paths), [1, 2, 4].map(native_id));
}

#[test]
fn retention_keeps_the_checkpoint_a_computer_is_positioned_on() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, store) = chain_fixture(&directory, &["automatic"; 4], None);
    // The computer was restored from checkpoint 2 and its next capture builds on it.
    let store = Store {
        computers: vec![("dev", Some("snap_2"))],
        ..store
    };

    trim_automatic(&store, &paths, 1);

    assert!(!store.removals().contains(&format!("dev:{}", native_id(2))));
    assert!(left_ids(&paths).contains(&native_id(2)));
    assert!(left_ids(&paths).contains(&native_id(4)));
}

#[test]
fn the_capture_cursor_not_the_restore_origin_positions_the_merge() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    // Restored from checkpoint 2, then captured four more times: the configuration still
    // names snap_2, the cursor names snap_6.
    let (paths, store) = chain_fixture(&directory, &["automatic"; 6], None);
    let store = Store {
        computers: vec![("dev", Some("snap_2"))],
        ..store
    };

    trim_automatic(&store, &paths, 2);

    // snap_2 is the computer's restore origin and stays; snap_5 bounds the merge.
    assert_eq!(store.compactions(), [4]);
    assert_eq!(
        store.removals(),
        [1, 3, 4].map(|n| format!("dev:{}", native_id(n)))
    );
    assert_eq!(left_ids(&paths), [2, 5, 6].map(native_id));
}

#[test]
fn a_named_checkpoint_in_the_middle_bounds_the_merge_without_stopping_later_ones() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    // 9 layers for 8 captures. Checkpoints 1-3 stay behind the named 4; 5 and 6 go.
    let kinds = [
        "automatic",
        "automatic",
        "automatic",
        "manual",
        "automatic",
        "automatic",
        "automatic",
        "automatic",
    ];
    let (paths, store) = chain_fixture(&directory, &kinds, None);

    trim_automatic(&store, &paths, 2);

    assert_eq!(
        store.compactions(),
        [2],
        "only the layers older than snap_3"
    );
    assert_eq!(
        store.removals(),
        [5, 6].map(|n| format!("dev:{}", native_id(n)))
    );
}

#[test]
fn a_checkpoint_older_than_the_merged_base_does_not_stop_the_merge() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    // An earlier merge folded the oldest captures: 8 captures sit on 5 layers. The named
    // checkpoint 1 lies inside the base and must not bound the merge.
    let kinds = [
        "manual",
        "automatic",
        "automatic",
        "automatic",
        "automatic",
        "automatic",
        "automatic",
        "automatic",
    ];
    let (paths, store) = chain_fixture(&directory, &kinds, None);
    let store = store.with_layers(5);

    trim_automatic(&store, &paths, 2);

    assert_eq!(store.compactions(), [2]);
    assert_eq!(
        store.removals(),
        [2, 3, 4, 5, 6].map(|n| format!("dev:{}", native_id(n)))
    );
}

#[test]
fn disks_with_different_chains_are_merged_and_confirmed_one_by_one() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, mut store) = chain_fixture(&directory, &["automatic"; 6], None);
    store.workspace_layers = Some(6);

    trim_automatic(&store, &paths, 2);

    assert_eq!(
        store.compaction_calls(),
        [
            (Some("/".to_owned()), 4),
            (Some("/workspace".to_owned()), 3)
        ]
    );
    let confirmations = store
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|call| {
            call[0] == "modify"
                && call.contains(&"--dry-run".into())
                && call.contains(&"--disk".into())
        })
        .count();
    assert_eq!(confirmations, 2);
    assert_eq!(left_ids(&paths).len(), 2);
}

#[test]
fn a_checkpoint_that_could_not_be_removed_is_reported_in_the_operation_status() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, mut store) = chain_fixture(&directory, &["automatic"; 6], None);
    store.fail_remove = true;

    trim_automatic(&store, &paths, 2);

    let record = load(&paths, ID).unwrap();
    assert_eq!(record.checkpoints.len(), 6);
    let operation = record.checkpoint_operation.unwrap();
    assert_eq!(operation.status, "failed");
    assert_eq!(operation.stage, reclaim::RECLAIM_STAGE);
    assert!(operation.error.unwrap().contains("could not be removed"));
}

#[test]
fn an_unreadable_capture_cursor_is_reported_and_removes_nothing() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, store) = chain_fixture(&directory, &["automatic"; 6], None);
    let cursor_path = paths
        .home
        .join("sandboxes")
        .join("dev")
        .join("snapshot-lineage.json");
    fs::write(cursor_path, "not json").unwrap();

    trim_automatic(&store, &paths, 2);

    assert!(store.removals().is_empty());
    let operation = load(&paths, ID).unwrap().checkpoint_operation.unwrap();
    assert_eq!(operation.status, "failed");
    assert!(operation.error.unwrap().contains("lineage"));
}

// MARK: Launch upgrade of the record layout

fn layout_one_record() -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "checkpoints": [{
            "id": A, "nativeId": "native-a", "name": "Legacy", "createdAt": 5,
            "scope": "full", "reason": "before-restore"
        }],
        "inflightCheckpoint": {
            "id": B, "name": "Capturing", "createdAt": 6, "scope": "disk", "reason": "manual"
        },
        "restoreJournal": {
            "targetCheckpointId": A,
            "recoveryCheckpoint": {
                "id": C, "name": "Before restore", "createdAt": 7, "scope": "disk",
                "reason": "before-restore"
            },
            "priorRunning": false,
            "phase": "capturing"
        },
        "pendingCheckpointRestore": null,
        "checkpointOperation": null
    })
}

#[test]
fn the_launch_upgrade_makes_every_old_record_manual_and_keeps_the_rest() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let storage = directory.path();
    fs::create_dir_all(storage.join("checkpoints")).unwrap();
    let file = storage.join("checkpoints").join(format!("{ID}.json"));
    fs::write(&file, layout_one_record().to_string()).unwrap();
    let damaged = storage.join("checkpoints").join(format!("{FORK_ID}.json"));
    fs::write(&damaged, b"{not json").unwrap();

    // Before the upgrade the old layout is refused, not silently accepted.
    let paths = crate::test_support::paths(storage);
    assert!(load(&paths, ID).is_err());

    assert_eq!(upgrade_records(storage), Ok(1));

    let record = load(&paths, ID).unwrap();
    assert_eq!(record.version, RECORD_VERSION);
    let first = &record.checkpoints[0];
    assert_eq!(first.kind, "manual");
    assert_eq!(first.reason, "before-restore", "the reason is kept");
    assert_eq!(first.native_id.as_deref(), Some("native-a"));
    assert_eq!(first.created_at, 5);
    assert_eq!(record.inflight_checkpoint.unwrap().kind, "manual");
    assert_eq!(
        record.restore_journal.unwrap().recovery_checkpoint.kind,
        "manual"
    );
    assert_eq!(
        fs::read(&damaged).unwrap(),
        b"{not json",
        "a damaged history is preserved"
    );

    // Idempotent: a converted file is not rewritten.
    let converted = fs::read(&file).unwrap();
    assert_eq!(upgrade_records(storage), Ok(0));
    assert_eq!(fs::read(&file).unwrap(), converted);
}

#[test]
fn the_launch_upgrade_leaves_structurally_invalid_histories_untouched() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let storage = directory.path();
    fs::create_dir_all(storage.join("checkpoints")).unwrap();
    let write = |id: &str, change: &dyn Fn(&mut Value)| {
        let mut value = layout_one_record();
        change(&mut value);
        let file = storage.join("checkpoints").join(format!("{id}.json"));
        fs::write(&file, value.to_string()).unwrap();
        (file, value.to_string())
    };
    let cases = [
        write(ID, &|v| v["checkpoints"][0]["reason"] = "bogus".into()),
        write(FORK_ID, &|v| {
            v["checkpoints"][0].as_object_mut().unwrap().remove("name");
        }),
        write(&uuid::Uuid::new_v4().to_string(), &|v| {
            v["checkpoints"][0]["scope"] = "nope".into();
        }),
    ];
    assert_eq!(upgrade_records(storage), Ok(0));
    for (file, original) in cases {
        assert_eq!(fs::read_to_string(file).unwrap(), original);
    }
}

#[test]
fn a_computer_without_a_history_folder_has_nothing_to_upgrade() {
    let directory = tempfile::tempdir().unwrap();
    assert_eq!(upgrade_records(directory.path()), Ok(0));
}

fn fork_child(paths: &RuntimePaths) -> (String, Record) {
    let child = read_metadata(&paths.metadata)
        .unwrap()
        .computers
        .into_iter()
        .find(|configuration| configuration.name() == "branch")
        .unwrap();
    let record = load(paths, child.id()).unwrap();
    (child.id().to_owned(), record)
}

#[test]
fn a_fork_copies_nothing_it_was_not_told_to() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, mut fork) = fork_fixture(&directory);
    fork.options = ForkOptions::default();
    let assignments = FakeAssignments::new(&[]);
    fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &assignments,
        &fork,
        "branch",
    )
    .unwrap();
    assert!(assignments.github.lock().unwrap().is_empty());
    assert!(assignments.secrets.lock().unwrap().is_empty());
    let (_, record) = fork_child(&paths);
    let pending = record.pending_checkpoint_restore.unwrap();
    assert_eq!(pending.state, "disk", "a fork never resumes memory");
    assert_eq!(
        record.quiet,
        Some(QuietRecord {
            scheduled_jobs_disabled: true,
            new_identity: true,
            ..QuietRecord::default()
        })
    );
}

#[test]
fn a_fork_copies_only_the_named_secrets_and_github_on_request() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, mut fork) = fork_fixture(&directory);
    fork.options = ForkOptions {
        secrets: vec!["API_KEY".into()],
        ..ForkOptions::default()
    };
    let assignments = FakeAssignments::new(&[]);
    fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &assignments,
        &fork,
        "branch",
    )
    .unwrap();
    assert!(assignments.github.lock().unwrap().is_empty());
    assert_eq!(*assignments.secret_names.lock().unwrap(), ["API_KEY"]);

    let directory = tempfile::tempdir().unwrap();
    let (paths, mut fork) = fork_fixture(&directory);
    fork.options = ForkOptions {
        secrets: vec!["github".into()],
        ..ForkOptions::default()
    };
    let assignments = FakeAssignments::new(&[]);
    fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &assignments,
        &fork,
        "branch",
    )
    .unwrap();
    assert_eq!(*assignments.github.lock().unwrap(), ["branch"]);
    assert!(assignments.secrets.lock().unwrap().is_empty());
    let (_, record) = fork_child(&paths);
    assert_eq!(record.quiet.unwrap().secrets, ["github"]);
}

#[test]
fn a_fork_starts_on_a_closed_network_only_when_asked_to() {
    let _test_state = crate::test_support::global_state();
    let deny_all = serde_json::json!({
        "default_egress": "deny",
        "default_ingress": "deny",
        "rules": []
    });
    let directory = tempfile::tempdir().unwrap();
    let (paths, mut fork) = fork_fixture(&directory);
    let parent_policy = fork.desired_policy.clone();
    assert_ne!(parent_policy, deny_all);
    fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &FakeAssignments::new(&[]),
        &fork,
        "branch",
    )
    .unwrap();
    let (_, record) = fork_child(&paths);
    assert_eq!(record.desired_network_policy, Some(parent_policy));

    let directory = tempfile::tempdir().unwrap();
    let (paths, closed) = fork_fixture(&directory);
    fork = closed;
    fork.options = ForkOptions {
        closed_network: true,
        ..ForkOptions::default()
    };
    fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &FakeAssignments::new(&[]),
        &fork,
        "branch",
    )
    .unwrap();
    let (_, record) = fork_child(&paths);
    assert_eq!(record.desired_network_policy, Some(deny_all));
}

#[test]
fn a_fork_has_its_own_channel_port_and_the_closed_copy_of_an_export_has_none() {
    use crate::network_policy::{extra_rules, plan_for, ExtraRule, Rule};
    let _test_state = crate::test_support::global_state();
    let store_directory = tempfile::tempdir().unwrap();
    crate::network_policy::store::use_test_path(Some(
        store_directory.path().join("network-policy.json"),
    ));
    crate::guest_client::tokens::use_test_directory(Some(
        store_directory.path().join("guest-client"),
    ));
    let tokens = crate::guest_client::TokenStore::current().unwrap();
    crate::guest_client::turn_on(ID).unwrap();
    let source_port = tokens.port_of(ID).unwrap();

    let directory = tempfile::tempdir().unwrap();
    let (paths, fork) = fork_fixture(&directory);
    fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &FakeAssignments::new(&[]),
        &fork,
        "branch",
    )
    .unwrap();
    let (child, _) = fork_child(&paths);
    let child_port = tokens.port_of(&child).unwrap();
    assert_ne!(child_port, source_port);
    assert_eq!(extra_rules(&child), vec![ExtraRule::GatewayPort(child_port)]);
    let plan = plan_for(&child, "branch", None).unwrap();
    assert!(plan.rules.contains(&Rule::GatewayPort(child_port)));
    assert!(!plan.rules.contains(&Rule::GatewayPort(source_port)));

    let directory = tempfile::tempdir().unwrap();
    let (paths, mut closed) = fork_fixture(&directory);
    closed.options = ForkOptions {
        closed_network: true,
        ..ForkOptions::default()
    };
    fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &FakeAssignments::new(&[]),
        &closed,
        "branch",
    )
    .unwrap();
    let (copy, _) = fork_child(&paths);
    assert!(tokens.port_of(&copy).is_none(), "the closed copy has no channel");
    assert!(extra_rules(&copy).is_empty());
    let plan = plan_for(&copy, "branch", None).unwrap();
    assert!(plan.rules.is_empty());
    assert!(!plan.ingress_allowed);
    crate::guest_client::tokens::use_test_directory(None);
    crate::network_policy::store::use_test_path(None);
}

struct InspectRunner(Value);
impl RuntimeRunner for InspectRunner {
    fn run(
        &self,
        _paths: &RuntimePaths,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        assert_eq!(args[0], "inspect", "{args:?}");
        Ok(CommandOutput {
            stdout: serde_json::json!({
                "name": "dev",
                "status": "Stopped",
                "config": self.0,
            })
            .to_string(),
            stderr: String::new(),
        })
    }
}

fn inspected_with_policy(policy: Option<Value>) -> InspectRunner {
    let mut config = serde_json::json!({
        "labels": {"silo.managed": "true", "silo.machine-id": ID},
        "network": {"strict": true},
    });
    if let Some(policy) = policy {
        config["network"]["policy"] = policy;
    }
    InspectRunner(config)
}

#[test]
fn a_computer_with_no_saved_policy_gets_its_channel_rule_added_to_the_rules_it_was_made_with() {
    use crate::network_policy::{plan, Grants, NetworkPolicy, Rule};
    let _test_state = crate::test_support::global_state();
    let store_directory = tempfile::tempdir().unwrap();
    let file = store_directory.path().join("network-policy.json");
    crate::network_policy::store::use_test_path(Some(file.clone()));
    crate::guest_client::tokens::use_test_directory(Some(
        store_directory.path().join("guest-client"),
    ));
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let configuration = computer_configuration();
    let open = plan(&NetworkPolicy::default(), &Grants::default(), &[]);
    let runner = inspected_with_policy(Some(open.policy_value()));

    // No saved policy and no channel: the rules it was made with stay.
    assert!(!network_policy_changed(&runner, &paths, &configuration).unwrap());
    ensure_network_rules_current(&runner, &paths, &configuration).unwrap();

    // With a channel the rule is added to the recorded rules, not to an assumed open policy.
    crate::guest_client::turn_on(ID).unwrap();
    let port = crate::guest_client::TokenStore::current().unwrap().port_of(ID).unwrap();
    let (planned, _) = changed_network_plan(&runner, &paths, &configuration)
        .unwrap()
        .unwrap();
    assert_eq!(planned.rules, vec![Rule::Dns, Rule::GatewayPort(port), Rule::Public]);
    let refused = ensure_network_rules_current(&runner, &paths, &configuration).unwrap_err();
    assert!(refused.to_string().contains("was not started"), "{refused}");
    let closed = inspected_with_policy(Some(serde_json::json!({
        "default_egress": "deny",
        "default_ingress": "deny",
        "rules": []
    })));
    let (planned, _) = changed_network_plan(&closed, &paths, &configuration)
        .unwrap()
        .unwrap();
    assert_eq!(planned.rules, vec![Rule::GatewayPort(port)]);
    assert!(!planned.ingress_allowed);

    // Once the runtime holds the channel rule there is nothing to change.
    let current = inspected_with_policy(Some(planned_value(&open, port)));
    ensure_network_rules_current(&current, &paths, &configuration).unwrap();

    // When the recorded rules cannot be read, nothing is guessed.
    let mut record = load(&paths, ID).unwrap();
    record.desired_network_policy = None;
    save(&paths, ID, &record).unwrap();
    let unknown = inspected_with_policy(None);
    let refused = changed_network_plan(&unknown, &paths, &configuration).unwrap_err();
    assert!(refused.to_string().contains("cannot read"), "{refused}");

    // A saved policy that cannot be read is an error whatever else is known.
    std::fs::write(&file, "{ not json").unwrap();
    assert!(ensure_network_rules_current(&current, &paths, &configuration).is_err());
    crate::guest_client::tokens::use_test_directory(None);
    crate::network_policy::store::use_test_path(None);
}

fn planned_value(open: &crate::network_policy::Plan, port: u16) -> Value {
    let mut plan = open.clone();
    plan.rules.insert(1, crate::network_policy::Rule::GatewayPort(port));
    plan.policy_value()
}

#[test]
fn a_fork_records_what_it_was_quieted_with() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, mut fork) = fork_fixture(&directory);
    fork.options = ForkOptions {
        secrets: vec!["github".into(), "TOKEN".into()],
        stop: vec!["poller".into()],
        scrub: vec!["/root/.bot.env".into()],
        closed_network: false,
        network_policy: None,
    };
    fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &FakeAssignments::new(&[]),
        &fork,
        "branch",
    )
    .unwrap();
    let quiet = fork_child(&paths).1.quiet.unwrap();
    assert_eq!(quiet.stop, ["poller"]);
    assert_eq!(quiet.scrub, ["/root/.bot.env"]);
    assert_eq!(quiet.secrets, ["github", "TOKEN"]);
    assert!(!quiet.applied);
}

#[test]
fn fork_options_refuse_odd_names_and_paths() {
    let options = |stop: &[&str], scrub: &[&str]| ForkOptions {
        secrets: Vec::new(),
        stop: stop.iter().map(|s| s.to_string()).collect(),
        scrub: scrub.iter().map(|s| s.to_string()).collect(),
        closed_network: false,
        network_policy: None,
    };
    assert!(options(
        &["poller", "my-app.service", "a@b"],
        &["/root/.env", "/home/*/.cache/x*"]
    )
    .validate()
    .is_ok());
    for stop in ["", "-rf", "a b", "a;b", "$(x)", "a\nb"] {
        assert!(options(&[stop], &[]).validate().is_err(), "{stop:?}");
    }
    for scrub in [
        "",
        "relative/x",
        "/",
        "/*",
        "/etc",
        "/etc/../x",
        "/a\nb/c",
        "/*/x",
    ] {
        assert!(options(&[], &[scrub]).validate().is_err(), "{scrub:?}");
    }
}

#[test]
fn a_fork_hostname_follows_its_name() {
    assert_eq!(fork_hostname("My Fork_2"), "my-fork-2");
    assert_eq!(fork_hostname("---"), "silo-fork");
    assert_eq!(fork_hostname(&"a".repeat(80)).len(), 63);
}

#[test]
fn the_first_boot_script_covers_every_quiet_rule() {
    for needle in [
        "/etc/machine-id",
        "/var/lib/dbus/machine-id",
        "/etc/hostname",
        "/var/spool/cron/crontabs",
        "/etc/cron.d",
        "/etc/anacrontab",
        "/etc/crontab",
        "atjobs",
        "*.timer",
        "stop=*",
        "scrub=*",
        "/dev/null",
    ] {
        assert!(QUIET_SCRIPT.contains(needle), "{needle}");
    }
    let status = std::process::Command::new("sh")
        .args(["-n", "-c", QUIET_SCRIPT])
        .status()
        .unwrap();
    assert!(status.success());
}

fn quiet_record(token: &str) -> QuietRecord {
    QuietRecord {
        scheduled_jobs_disabled: true,
        new_identity: true,
        token: token.into(),
        ..QuietRecord::default()
    }
}

const TOKEN: &str = "0f0f0f0f-0000-4000-8000-000000000001";

#[test]
fn quiet_arguments_are_positional_and_run_as_root() {
    let quiet = QuietRecord {
        stop: vec!["poller".into()],
        scrub: vec!["/root/xy*".into()],
        ..quiet_record(TOKEN)
    };
    let home = tempfile::tempdir().unwrap();
    let args = quiet_exec_args(&paths(&home), "My Fork", &quiet);
    assert_eq!(&args[..2], ["exec", "My Fork"]);
    assert!(args.windows(2).any(|pair| pair == ["--user", "root"]));
    assert_eq!(
        args[args.len() - 4..],
        [TOKEN, "my-fork", "stop=poller", "scrub=/root/xy*"]
    );
    assert_eq!(args[args.len() - 5], "silo-quiet");
    assert_eq!(
        quiet_labels("My Fork", &quiet),
        [
            format!("silo.quiet-token={TOKEN}"),
            "silo.quiet-host=my-fork".to_string(),
            "silo.quiet-args=stop=poller scrub=/root/xy*".to_string(),
        ]
    );
}

/// A guest that records whether the quiet step ran, behind the production runner's refusal to
/// exec a computer with a pending restore.
struct QuietGuest {
    marker: Mutex<bool>,
    script_fails: bool,
    stop_fails: bool,
    calls: Mutex<Vec<Vec<String>>>,
}
impl QuietGuest {
    fn new(marker: bool) -> Self {
        QuietGuest {
            marker: Mutex::new(marker),
            script_fails: false,
            stop_fails: false,
            calls: Mutex::new(Vec::new()),
        }
    }
    fn count(&self, word: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.iter().any(|arg| arg == word))
            .count()
    }
}
impl RuntimeRunner for QuietGuest {
    fn run(
        &self,
        paths: &RuntimePaths,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.calls.lock().unwrap().push(args.to_vec());
        super::super::refuse_pending_start(paths, args)?;
        let ok = || {
            Ok(CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
            })
        };
        match args[0].as_str() {
            "exec" if args.iter().any(|arg| arg == "test") => {
                if *self.marker.lock().unwrap() {
                    ok()
                } else {
                    Err(error("exit status 1"))
                }
            }
            "exec" if args.iter().any(|arg| arg == "silo-quiet") => {
                if self.script_fails {
                    Err(error("the quiet step failed"))
                } else {
                    *self.marker.lock().unwrap() = true;
                    ok()
                }
            }
            "stop" if self.stop_fails => Err(error("stop failed")),
            _ => ok(),
        }
    }
}

fn unquieted_computer(directory: &tempfile::TempDir, pending: bool) -> RuntimePaths {
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
    record.quiet = Some(quiet_record(TOKEN));
    if pending {
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: "c000000000000000000000000000000".into(),
            source_computer: "dev".into(),
            state: "disk".into(),
        });
    }
    save(&paths, ID, &record).unwrap();
    paths
}

#[test]
fn the_production_guard_refuses_exec_while_a_restore_is_pending() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = unquieted_computer(&directory, true);
    let guest = QuietGuest::new(true);
    let mut record = load(&paths, ID).unwrap();
    assert!(apply_quiet(&guest, &paths, "dev", &mut record).is_err());
    assert!(!record.quiet.unwrap().applied);
}

#[test]
fn quieting_confirms_the_marker_and_runs_nothing_when_it_is_there() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = unquieted_computer(&directory, false);
    let guest = QuietGuest::new(true);
    ensure_quiet(&guest, &paths, "dev").unwrap();
    assert!(load(&paths, ID).unwrap().quiet.unwrap().applied);
    assert_eq!(guest.count("silo-quiet"), 0, "the boot hook already ran it");
    // Once applied, nothing is asked of the guest again.
    let again = QuietGuest::new(false);
    ensure_quiet(&again, &paths, "dev").unwrap();
    assert!(again.calls.lock().unwrap().is_empty());
}

#[test]
fn a_fork_noted_for_a_base_gets_one_attempted_once_it_is_quiet() {
    use crate::changes::store::{Slot, Store};
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = unquieted_computer(&directory, false);
    let context = crate::guest::test_support::RecordingContext::new(directory.path());
    let store = Store::for_engine(context.as_ref());
    crate::changes::fork_base::install(context.clone());
    store.put_pending("dev", "cp1").unwrap();
    ensure_quiet(&QuietGuest::new(true), &paths, "dev").unwrap();
    // Quiet was confirmed, so the capture ran: it either stored the base (and used the note up)
    // or recorded why not (and kept the note for the next start).
    assert!(
        store.get(ID, Slot::Base).unwrap().is_some() || store.get_failure(ID).is_some(),
        "the start neither stored a base nor recorded a failure"
    );
}

#[test]
fn a_fork_that_is_not_quiet_yet_keeps_its_note() {
    use crate::changes::store::Store;
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = unquieted_computer(&directory, true);
    let context = crate::guest::test_support::RecordingContext::new(directory.path());
    let store = Store::for_engine(context.as_ref());
    crate::changes::fork_base::install(context.clone());
    store.put_pending("dev", "cp1").unwrap();
    assert!(ensure_quiet(&QuietGuest::new(true), &paths, "dev").is_err());
    assert_eq!(store.take_pending("dev").as_deref(), Some("cp1"));
}

#[test]
fn quieting_runs_the_step_itself_when_the_boot_hook_did_not() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = unquieted_computer(&directory, false);
    let guest = QuietGuest::new(false);
    ensure_quiet(&guest, &paths, "dev").unwrap();
    assert_eq!(guest.count("silo-quiet"), 1);
    assert_eq!(guest.count("silo-desktop"), 0);
    assert!(guest
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|call| call.iter().any(|arg| arg.contains("silo-desktop boot"))));
    assert!(load(&paths, ID).unwrap().quiet.unwrap().applied);
}

#[test]
fn a_failed_quiet_step_stops_the_fork_and_stays_unapplied() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = unquieted_computer(&directory, false);
    let mut guest = QuietGuest::new(false);
    guest.script_fails = true;
    let failure = ensure_quiet(&guest, &paths, "dev").unwrap_err();
    assert!(
        failure.to_string().contains("so it was stopped"),
        "{failure}"
    );
    assert_eq!(guest.count("stop"), 1);
    assert!(!load(&paths, ID).unwrap().quiet.unwrap().applied);

    let mut guest = QuietGuest::new(false);
    guest.script_fails = true;
    guest.stop_fails = true;
    let failure = ensure_quiet(&guest, &paths, "dev").unwrap_err();
    assert!(
        failure.to_string().contains("could not be stopped"),
        "{failure}"
    );
    assert!(!load(&paths, ID).unwrap().quiet.unwrap().applied);
}

#[test]
fn a_computer_that_is_not_a_fork_is_left_alone() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = unquieted_computer(&directory, false);
    let mut record = load(&paths, ID).unwrap();
    record.quiet = None;
    save(&paths, ID, &record).unwrap();
    let guest = QuietGuest::new(false);
    ensure_quiet(&guest, &paths, "dev").unwrap();
    assert!(guest.calls.lock().unwrap().is_empty());
}

/// A fork whose restore succeeds; the guest runs the quiet step only through the runner.
struct ForkRestoreGuest {
    guest: QuietGuest,
    attempt: Mutex<Option<String>>,
    restore_labels: Mutex<Vec<String>>,
}
impl RuntimeRunner for ForkRestoreGuest {
    fn run(
        &self,
        paths: &RuntimePaths,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        let ok = |stdout: String| {
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        };
        match args[0].as_str() {
            "snapshot" => {
                self.guest.calls.lock().unwrap().push(args.to_vec());
                ok(serde_json::json!([{"group":"dev","name":"c000000000000000000000000000000","scope":"disk","availability":"ready"}]).to_string())
            }
            "list" => {
                self.guest.calls.lock().unwrap().push(args.to_vec());
                ok(if self.attempt.lock().unwrap().is_some() {
                    r#"[{"name":"dev"}]"#.into()
                } else {
                    "[]".into()
                })
            }
            "restore" => {
                self.guest.calls.lock().unwrap().push(args.to_vec());
                let labels: Vec<String> = args
                    .windows(2)
                    .filter(|pair| pair[0] == "--label")
                    .map(|pair| pair[1].clone())
                    .collect();
                *self.attempt.lock().unwrap() = labels.iter().find_map(|label| {
                    label
                        .strip_prefix("silo.restore-attempt=")
                        .map(str::to_owned)
                });
                *self.restore_labels.lock().unwrap() = labels;
                ok(String::new())
            }
            "inspect" => {
                self.guest.calls.lock().unwrap().push(args.to_vec());
                ok(serde_json::json!({"name":"dev","status":"Running","config":{
                    "labels":{"silo.managed":"true","silo.machine-id":ID,"silo.restore-attempt":self.attempt.lock().unwrap().clone()},
                    "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]},
                        "secrets":{"secrets":[{"env_var":"SILO_GITHUB","value":"","source":{"kind":"env","var":"SILO_GITHUB"}}]}}
                }}).to_string())
            }
            _ => self.guest.run(paths, args, timeout),
        }
    }
}

fn fork_restore_guest(marker: bool) -> ForkRestoreGuest {
    ForkRestoreGuest {
        guest: QuietGuest::new(marker),
        attempt: Mutex::new(None),
        restore_labels: Mutex::new(Vec::new()),
    }
}

fn fork_pending_fixture(directory: &tempfile::TempDir) -> RuntimePaths {
    let paths = unquieted_computer(directory, true);
    let mut record = load(&paths, ID).unwrap();
    record.snapshot_group = Some("dev".into());
    record.quiet = Some(QuietRecord {
        stop: vec!["poller".into()],
        scrub: vec!["/root/.env".into()],
        ..quiet_record("")
    });
    record.desired_network_policy =
        Some(serde_json::json!({"default_egress":"deny","default_ingress":"allow","rules":[]}));
    save(&paths, ID, &record).unwrap();
    paths
}

#[test]
fn a_fork_start_goes_through_the_production_guard_and_is_quiet_before_it_is_reported() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = fork_pending_fixture(&directory);
    let runner = fork_restore_guest(true);
    start_pending(&runner, &paths, &computer_configuration()).unwrap();
    let labels = runner.restore_labels.lock().unwrap().clone();
    let attempt = runner.attempt.lock().unwrap().clone().unwrap();
    assert!(
        labels.contains(&format!("silo.quiet-token={attempt}")),
        "{labels:?}"
    );
    assert!(labels.contains(&"silo.quiet-host=dev".to_string()));
    assert!(labels.contains(&"silo.quiet-args=stop=poller scrub=/root/.env".to_string()));
    let stored = load(&paths, ID).unwrap();
    assert!(stored.pending_checkpoint_restore.is_none());
    let quiet = stored.quiet.unwrap();
    assert!(quiet.applied);
    assert_eq!(quiet.token, attempt);
}

#[test]
fn a_first_start_of_a_noted_fork_takes_its_base_when_quiet_is_confirmed() {
    use crate::changes::store::{Slot, Store};
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = fork_pending_fixture(&directory);
    let context = crate::guest::test_support::RecordingContext::new(directory.path());
    let store = Store::for_engine(context.as_ref());
    crate::changes::fork_base::install(context.clone());
    store.put_pending("dev", "cp1").unwrap();
    let runner = fork_restore_guest(true);
    start_pending(&runner, &paths, &computer_configuration()).unwrap();
    // The capture could not reach a real guest here, so it recorded why and kept the note
    // for the next start; either way the start path attempted it.
    assert!(
        store.get(ID, Slot::Base).unwrap().is_some() || store.get_failure(ID).is_some(),
        "the start neither stored a base nor recorded a failure"
    );
}

#[test]
fn a_fork_whose_quiet_step_fails_is_stopped_and_every_later_start_quiets_it_first() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = fork_pending_fixture(&directory);
    let mut runner = fork_restore_guest(false);
    runner.guest.script_fails = true;
    let failure = start_pending(&runner, &paths, &computer_configuration()).unwrap_err();
    assert!(
        failure.to_string().contains("so it was stopped"),
        "{failure}"
    );
    let stored = load(&paths, ID).unwrap();
    assert!(
        stored.pending_checkpoint_restore.is_none(),
        "the retry no longer clears a pending restore before it quiets"
    );
    assert!(!stored.quiet.as_ref().unwrap().applied);

    // The next start, from any caller, confirms the step before it reports success.
    let guest = QuietGuest::new(false);
    ensure_quiet(&guest, &paths, "dev").unwrap();
    assert_eq!(guest.count("silo-quiet"), 1);
    assert!(load(&paths, ID).unwrap().quiet.unwrap().applied);
}

#[test]
fn a_running_fork_that_already_matches_is_still_quieted() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = fork_pending_fixture(&directory);
    let runner = fork_restore_guest(false);
    let mut first = fork_restore_guest(false);
    first.guest.script_fails = true;
    start_pending(&first, &paths, &computer_configuration()).unwrap_err();
    // Put the pending restore back as an interrupted attempt that booted and matches.
    let mut record = load(&paths, ID).unwrap();
    let attempt = first.attempt.lock().unwrap().clone().unwrap();
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: "c000000000000000000000000000000".into(),
        source_computer: "dev".into(),
        state: "disk".into(),
    });
    record.restore_attempted = true;
    record.restore_attempt_id = Some(attempt.clone());
    save(&paths, ID, &record).unwrap();
    *runner.attempt.lock().unwrap() = Some(attempt);
    start_pending(&runner, &paths, &computer_configuration()).unwrap();
    assert_eq!(runner.guest.count("silo-quiet"), 1);
    assert!(load(&paths, ID).unwrap().quiet.unwrap().applied);
}

#[test]
fn a_fork_does_not_copy_the_sources_environment_variables() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let (paths, fork) = fork_fixture(&directory);
    let mut source = load(&paths, &fork.source_id).unwrap();
    source.set_environment_for_test(&[("API_TOKEN", "secret")]);
    save(&paths, &fork.source_id, &source).unwrap();
    fork_commit(
        &journal_runner("Running", ""),
        &paths,
        &FakeAssignments::new(&[]),
        &fork,
        "branch",
    )
    .unwrap();
    assert!(fork_child(&paths).1.desired_environment.is_empty());
}

#[test]
fn stop_names_refuse_dots() {
    let options = |stop: &str| ForkOptions {
        stop: vec![stop.into()],
        ..ForkOptions::default()
    };
    for stop in [".", "..", "...", "a b", "a/b"] {
        assert!(options(stop).validate().is_err(), "{stop:?}");
    }
    assert!(options("poller.py").validate().is_ok());
    let scrub = |path: &str| ForkOptions {
        scrub: vec![path.into()],
        ..ForkOptions::default()
    };
    for path in ["/root/./x", "/root/a b", "/root/a\tb"] {
        assert!(scrub(path).validate().is_err(), "{path:?}");
    }
    assert!(scrub("/home/*/.cache").validate().is_ok());
    assert!(scrub("/home/*").validate().is_ok());
}

fn script_function(name: &str, until: &str) -> String {
    let start = QUIET_SCRIPT
        .find(&format!("{name}() {{"))
        .unwrap_or_else(|| panic!("{name} missing"));
    let end = start + QUIET_SCRIPT[start..].find(until).unwrap();
    QUIET_SCRIPT[start..end].to_string()
}

fn run_sh(program: &str) -> std::process::Output {
    std::process::Command::new("sh")
        .args(["-c", program])
        .output()
        .unwrap()
}

#[test]
fn the_boot_patch_carries_the_script_the_engine_runs() {
    let patch = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../patches/microsandbox-create-stopped-0.7.6.patch"),
    )
    .unwrap();
    let lines: Vec<&str> = patch.lines().collect();
    let first = lines
        .iter()
        .position(|line| line.ends_with("r####\"set -u"))
        .unwrap();
    let last = first
        + 1
        + lines[first + 1..]
            .iter()
            .position(|line| *line == "+\"####;")
            .unwrap();
    let mut embedded = String::from("set -u\n");
    for line in &lines[first + 1..last] {
        embedded.push_str(line.strip_prefix('+').unwrap());
        embedded.push('\n');
    }
    assert_eq!(embedded, QUIET_SCRIPT);
    assert!(patch.contains("silo.quiet-token"));
}

#[test]
fn argv_matching_uses_the_program_or_script_name_exactly() {
    let functions = script_function("argv_matches", "pids_of() {");
    let check = |want: &str, argv: &[&str]| {
        let lines = argv.join("\n");
        let program = format!("{functions}\nprintf '%s\\n' \"$LINES_IN\" | argv_matches \"$WANT\"");
        std::process::Command::new("sh")
            .args(["-c", &program])
            .env("LINES_IN", lines)
            .env("WANT", want)
            .status()
            .unwrap()
            .success()
    };
    assert!(check("poller", &["/usr/bin/poller", "--flag"]));
    assert!(check("poller.py", &["python3", "-u", "/opt/app/poller.py"]));
    assert!(check("server.js", &["node", "/srv/server.js"]));
    assert!(check(
        "a-very-long-service-name",
        &["/bin/a-very-long-service-name"]
    ));
    assert!(!check("poller", &["python3", "/opt/pollerx.py"]));
    assert!(!check("poller", &["sh", "-c", "echo poller"]));
    assert!(!check("po.*", &["/usr/bin/poller"]));
    assert!(!check("poller", &["python3", "/opt/a/poller/run.py"]));
}

#[cfg(target_os = "linux")]
#[test]
fn stopping_by_name_ends_a_long_named_script_and_fails_on_survivors() {
    let directory = tempfile::tempdir().unwrap();
    let script = directory.path().join("a-long-named-poller-script.sh");
    std::fs::write(&script, "#!/bin/sh\nwhile :; do sleep 1; done\n").unwrap();
    let mut child = std::process::Command::new("sh")
        .arg(&script)
        .spawn()
        .unwrap();
    let functions = script_function("argv_matches", "for daemon in cron");
    let program = format!("{functions}\nstop_processes a-long-named-poller-script.sh");
    let outcome = run_sh(&program);
    assert!(outcome.status.success(), "{outcome:?}");
    assert!(child.wait().is_ok());
    let none = run_sh(&format!("{functions}\nstop_processes no-such-thing"));
    assert!(none.status.success());
}

#[test]
fn scrub_stays_inside_the_pattern_and_removes_links_without_following_them() {
    let functions = script_function("scrub_one", "for a; do");
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let path = |relative: &str| root.join(relative);
    let make = |relative: &str| {
        let file = path(relative);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "x").unwrap();
    };
    let scrub = |pattern: &str| {
        run_sh(&format!("{functions}\nscrub_one '{pattern}'"))
            .status
            .success()
    };

    make("home/ann/.cache/x");
    make("home/bob/.cache/y");
    make("home/ann/keep");
    assert!(scrub(&format!("{}/home/*/.cache", root.display())));
    assert!(!path("home/ann/.cache").exists() && !path("home/bob/.cache").exists());
    assert!(path("home/ann/keep").exists());

    // `.[.]` is `..`: the match leaves the literal directory and the step fails.
    make("etc/a");
    make("outside/secret");
    assert!(!scrub(&format!("{}/etc/.[.]/*", root.display())));
    assert!(path("outside/secret").exists() && path("etc/a").exists());

    // A symlinked directory in the path redirects nothing.
    make("elsewhere/secret");
    std::fs::create_dir_all(path("root")).unwrap();
    std::os::unix::fs::symlink(path("elsewhere"), path("root/link")).unwrap();
    assert!(!scrub(&format!("{}/root/link/secret", root.display())));
    assert!(path("elsewhere/secret").exists());

    // A link named by the pattern is removed itself.
    make("kept/file");
    std::os::unix::fs::symlink(path("kept"), path("root/pointer")).unwrap();
    assert!(scrub(&format!("{}/root/pointer", root.display())));
    assert!(std::fs::symlink_metadata(path("root/pointer")).is_err());
    assert!(path("kept/file").exists());

    // The top of the file system is never a target.
    assert!(!scrub("/*"));
}

#[test]
fn autostart_entries_match_any_word_of_their_exec_line() {
    let functions = script_function("desktop_runs", "pids_of() {");
    let dir = tempfile::tempdir().unwrap();
    let check = |exec: &str, want: &str| {
        let file = dir.path().join("entry.desktop");
        std::fs::write(&file, format!("[Desktop Entry]\nName=x\nExec={exec}\n")).unwrap();
        std::process::Command::new("sh")
            .args([
                "-c",
                &format!("{functions}\ndesktop_runs \"$FILE\" \"$WANT\""),
            ])
            .env("FILE", &file)
            .env("WANT", want)
            .status()
            .unwrap()
            .success()
    };
    assert!(check(
        "/usr/local/bin/silo-check-poller.sh",
        "silo-check-poller.sh"
    ));
    assert!(check(
        "sh /opt/silo-check-poller.sh --loop",
        "silo-check-poller.sh"
    ));
    assert!(check("env A=1 \"/opt/poller\"", "poller"));
    assert!(!check("/usr/bin/pollerx", "poller"));
    assert!(!check("/usr/bin/poller.old", "poller"));
}

#[test]
fn a_running_container_is_captured_with_disks_only() {
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
    crate::containers::capability::make_container(&paths.home, "dev");
    let runner = Runner {
        mount_type: "Owned",
        calls: Mutex::new(Vec::new()),
    };
    capture_with(&runner, &paths, ID, "Before work", "manual").unwrap();
    let calls = runner.calls.lock().unwrap();
    let create = calls
        .iter()
        .find(|call| call[..2] == ["snapshot", "create"])
        .unwrap();
    assert!(!create.iter().any(|arg| arg == "--full"), "{create:?}");
    assert!(!calls
        .iter()
        .any(|call| call.iter().any(|arg| arg.contains("drop_caches"))));
    assert_eq!(load(&paths, ID).unwrap().checkpoints[0].scope, "disk");
}

struct ContainerRunner {
    state: Mutex<&'static str>,
    calls: Mutex<Vec<Vec<String>>>,
}
impl RuntimeRunner for ContainerRunner {
    fn run(
        &self,
        _paths: &RuntimePaths,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.calls.lock().unwrap().push(args.to_vec());
        let stdout = match args[0].as_str() {
            "inspect" => serde_json::json!({"name":"dev","status":*self.state.lock().unwrap(),"config":{
                "labels":{"silo.managed":"true","silo.machine-id":ID},
                "mounts":[{"guest":"/workspace","type":"Owned","storage":{"kind":"disk","capacity_mib":1024}}],
                "network":{"policy":{"default_egress":"allow","default_ingress":"deny","rules":[]}}
            }}).to_string(),
            "snapshot" if args[1] == "list" => {
                let entry = |name: &str| serde_json::json!({"group":"dev","name":name,"scope":"disk","availability":"ready"});
                let mut names = vec!["c000000000000000000000000000000".to_owned()];
                names.extend(
                    self.calls.lock().unwrap().iter().filter(|call| call[..2] == ["snapshot", "create"]).map(|call| call[2].clone()),
                );
                serde_json::to_string(&names.iter().map(|name| entry(name)).collect::<Vec<_>>()).unwrap()
            }
            "snapshot" => String::new(),
            "stop" => {
                assert!(args.iter().any(|arg| arg == "--force"));
                *self.state.lock().unwrap() = "Stopped";
                String::new()
            }
            "remove" => String::new(),
            // The computer is listed until it has been removed.
            "list" if self.calls.lock().unwrap().iter().any(|call| call[0] == "remove") => {
                "[]".into()
            }
            "list" => serde_json::json!([{"name":"dev","status":*self.state.lock().unwrap()}])
                .to_string(),
            _ => panic!("unexpected runtime command: {args:?}"),
        };
        Ok(CommandOutput {
            stdout,
            stderr: String::new(),
        })
    }
}

#[test]
fn restoring_a_running_container_saves_its_disks_and_stops_it_without_pausing() {
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
    crate::containers::capability::make_container(&paths.home, "dev");
    let mut record = Record::default();
    record.checkpoints.push(Checkpoint {
        id: "c000000000000000000000000000000".into(),
        native_id: None,
        name: "Selected".into(),
        created_at: 1,
        scope: "disk".into(),
        reason: "manual".into(),
        kind: "manual".into(),
    });
    save(&paths, ID, &record).unwrap();
    let runner = ContainerRunner {
        state: Mutex::new("Running"),
        calls: Mutex::new(Vec::new()),
    };
    restore_with(&runner, &paths, ID, "c000000000000000000000000000000").unwrap();
    let calls = runner.calls.lock().unwrap();
    let position = |command: &str| calls.iter().position(|args| args[0] == command).unwrap();
    assert!(!calls.iter().any(|args| args[0] == "pause"));
    let create = calls.iter().find(|call| call[..2] == ["snapshot", "create"]).unwrap();
    assert!(!create.iter().any(|arg| arg == "--full"), "{create:?}");
    assert!(position("snapshot") < position("stop"));
    assert!(position("stop") < position("remove"));
    let stored = load(&paths, ID).unwrap();
    assert_eq!(stored.checkpoints[0].reason, "before-restore");
    assert_eq!(stored.checkpoints[0].scope, "disk");
    assert_eq!(stored.pending_checkpoint_restore.unwrap().state, "disk");
}

#[test]
fn a_retried_restore_does_not_stop_a_container_that_is_running_again() {
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
    crate::containers::capability::make_container(&paths.home, "dev");
    let selected = "c000000000000000000000000000000";
    let mut record = Record::default();
    record.checkpoints.push(Checkpoint {
        id: selected.into(),
        native_id: None,
        name: "Selected".into(),
        created_at: 1,
        scope: "disk".into(),
        reason: "manual".into(),
        kind: "manual".into(),
    });
    // An earlier run secured the computer; the engine then crashed and took it back running.
    record.restore_journal = Some(RestoreJournal {
        target_checkpoint_id: selected.into(),
        recovery_checkpoint: record.checkpoints[0].clone(),
        prior_running: true,
        phase: "secured".into(),
    });
    save(&paths, ID, &record).unwrap();
    let runner = ContainerRunner {
        state: Mutex::new("Running"),
        calls: Mutex::new(Vec::new()),
    };
    let refused = restore_with(&runner, &paths, ID, selected).unwrap_err();
    assert!(
        refused.to_string().contains("resumed after its recovery checkpoint"),
        "{refused}"
    );
    assert!(!runner.calls.lock().unwrap().iter().any(|args| args[0] == "stop"));
}

#[test]
fn an_imported_container_starts_from_the_deny_all_policy_like_a_virtual_machine() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let (key, value) = crate::containers::RUNTIME_LABEL.split_once('=').unwrap();
    for config in [
        serde_json::json!({}),
        serde_json::json!({"labels": {key: value}}),
    ] {
        import_pending_restore_with_environment(
            &paths,
            ID,
            "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
            "silo-backup-0-330418-1790360984903",
            &config,
        )
        .unwrap();
        assert_eq!(
            load(&paths, ID).unwrap().desired_network_policy,
            Some(serde_json::json!({
                "default_egress": "deny",
                "default_ingress": "deny",
                "rules": []
            })),
            "{config}"
        );
        // The first start asks the runtime for that policy, whichever runtime answers.
        let runner = RestoreProbe(Mutex::new(Vec::new()));
        assert!(start_pending(&runner, &paths, &computer_configuration()).is_err());
        let calls = runner.0.lock().unwrap();
        let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
        for flag in ["--net-default-egress", "--net-default-ingress"] {
            assert!(
                restore.windows(2).any(|pair| pair == [flag, "deny"]),
                "{restore:?}"
            );
        }
        assert!(!restore.iter().any(|arg| arg == "--net-rule"), "{restore:?}");
    }
}

#[test]
fn an_import_saves_a_closed_policy_and_a_start_does_not_run_on_an_unreadable_one() {
    let _test_state = crate::test_support::global_state();
    struct Probe(Mutex<Vec<Vec<String>>>);
    impl RuntimeRunner for Probe {
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
    let file = directory.path().join("network-policy.json");
    crate::network_policy::store::use_test_path(Some(file.clone()));
    // A policy left over from a computer of this id is replaced by an explicit closed one.
    crate::network_policy::store::set(
        ID,
        &crate::network_policy::NetworkPolicy::default(),
    )
    .unwrap();
    crate::guest_client::tokens::use_test_directory(Some(directory.path().join("guest-client")));
    // Silo's tools left over from a computer of this id are turned off by the import.
    crate::guest_client::turn_on(ID).unwrap();
    import_pending_restore(
        &paths,
        ID,
        "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
        "silo-backup-0-330418-1790360984903",
    )
    .unwrap();
    let stored = crate::network_policy::store::get_stored(ID).unwrap().unwrap();
    assert_eq!(stored.mode, crate::network_policy::NetworkPolicyMode::Closed);
    assert!(stored.incoming_denied);
    assert!(crate::network_policy::extra_rules(ID).is_empty());
    assert!(crate::guest_client::TokenStore::current()
        .unwrap()
        .port_of(ID)
        .is_none());
    // With the saved policies unreadable the start refuses before it asks the runtime for
    // anything that changes it.
    std::fs::write(&file, "{ not json").unwrap();
    let runner = Probe(Mutex::new(Vec::new()));
    let refused = start_pending(&runner, &paths, &computer_configuration()).unwrap_err();
    assert!(refused.to_string().contains("unreadable"), "{refused}");
    assert!(runner
        .0
        .lock()
        .unwrap()
        .iter()
        .all(|args| args[0] != "restore" && args[0] != "remove"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json");
    crate::guest_client::tokens::use_test_directory(None);
    crate::network_policy::store::use_test_path(None);
}
