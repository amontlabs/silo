use super::*;

const WORKSPACE_CHECKSUM: &str = r#"set -eu
stage=$(mktemp -d "${TMPDIR:-/tmp}/silo-checksum.XXXXXXXX")
trap 'rm -rf "$stage"' EXIT
find /workspace -xdev -type f -exec sha256sum {} + > "$stage/files"
LC_ALL=C sort "$stage/files" > "$stage/sorted"
sha256sum < "$stage/sorted"
"#;

fn checksum_fixture(scan_status: u8, sort_status: u8, records: &str) -> std::process::Output {
    let directory = tempfile::tempdir().unwrap();
    crate::test_support::write_shell_script(
        &directory.path().join("find"),
        "printf '%s' \"$CHECKSUM_RECORDS\"; exit \"$SCAN_STATUS\"",
    );
    crate::test_support::write_shell_script(
        &directory.path().join("sort"),
        "[ \"$SORT_STATUS\" -eq 0 ] || exit \"$SORT_STATUS\"; exec /usr/bin/sort \"$@\"",
    );
    crate::test_support::write_shell_script(&directory.path().join("sha256sum"), "exec /bin/cat");
    std::process::Command::new("/bin/sh")
        .args(["-c", WORKSPACE_CHECKSUM])
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", directory.path().display()),
        )
        .env("TMPDIR", directory.path())
        .env("SCAN_STATUS", scan_status.to_string())
        .env("SORT_STATUS", sort_status.to_string())
        .env("CHECKSUM_RECORDS", records)
        .output()
        .unwrap()
}

#[test]
fn workspace_checksum_rejects_failed_scan() {
    let result = checksum_fixture(23, 0, "partial hash  /workspace/file\n");
    assert_eq!(result.status.code(), Some(23));
    assert!(result.stdout.is_empty());
}

#[test]
fn workspace_checksum_rejects_failed_sort() {
    let result = checksum_fixture(0, 17, "hash  /workspace/file\n");
    assert_eq!(result.status.code(), Some(17));
    assert!(result.stdout.is_empty());
}

#[test]
fn workspace_checksum_preserves_sorted_and_empty_streams() {
    for (records, expected) in [
        (
            "b  /workspace/b\na  /workspace/a\n",
            "a  /workspace/a\nb  /workspace/b\n",
        ),
        ("", ""),
    ] {
        let result = checksum_fixture(0, 0, records);
        assert!(result.status.success());
        assert_eq!(result.stdout, expected.as_bytes());
    }
}

fn fixture() -> (
    tempfile::TempDir,
    RuntimePaths,
    ComputerConfiguration,
    InspectedSandbox,
) {
    let directory = tempfile::tempdir().unwrap();
    let paths = super::super::tests::paths(&directory);
    let configuration = ComputerConfiguration {
        id: "00000000-0000-4000-8000-000000000001".into(),
        name: "dev".into(),
        cpus: 1,
        max_cpus: 1,
        memory_gib: 1,
        max_memory_gib: 1,
        workspace_storage_gib: 1,
        runtime_storage_gib: 1,
        desktop: None,
    };
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![configuration.clone()],
        },
    )
    .unwrap();
    fs::create_dir_all(owned_disk(&paths, "dev").parent().unwrap()).unwrap();
    fs::write(owned_disk(&paths, "dev"), vec![7u8; 8192]).unwrap();
    let observed = serde_json::from_value(json!({"name":"dev", "status":"Running", "runtime_instance_id":"run-1", "config": {
        "labels":{"silo.managed":"true","silo.machine-id":configuration.id()},
        "mounts":[{"type":"Owned","guest":"/workspace","storage":{"kind":"disk","capacity_mib":1024}}]
    }})).unwrap();
    verified_starts().lock().unwrap().insert(
        (paths.home.clone(), configuration.id().into()),
        "run-1".into(),
    );
    (directory, paths, configuration, observed)
}
fn owned_disk(paths: &RuntimePaths, name: &str) -> PathBuf {
    paths
        .home
        .join("sandboxes")
        .join(name)
        .join("owned-volumes/workspace_c52ddf65/disk.raw")
}
struct Runner {
    calls: Mutex<Vec<Vec<String>>>,
    fail: bool,
    truncate: bool,
}
impl Runner {
    fn new() -> Self {
        Self {
            calls: Mutex::new(vec![]),
            fail: false,
            truncate: false,
        }
    }
}
impl RuntimeRunner for Runner {
    fn run(
        &self,
        paths: &RuntimePaths,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.calls.lock().unwrap().push(args.to_vec());
        if args[0] == "--silo-storage-protocol" {
            return Ok(CommandOutput {
                stdout: "1\n".into(),
                stderr: String::new(),
            });
        }
        assert_eq!(args[0], "exec");
        assert!(args.iter().any(|arg| arg == "--no-start"));
        assert!(timeout <= TRIM_BUDGET);
        if self.truncate {
            fs::OpenOptions::new()
                .write(true)
                .open(owned_disk(paths, "dev"))
                .unwrap()
                .set_len(4096)
                .unwrap();
        }
        if self.fail {
            return Err(RuntimeError::TimedOut {
                operation: "trim".into(),
            });
        }
        Ok(CommandOutput {
            stdout: "4096 8192\n".into(),
            stderr: String::new(),
        })
    }
}

#[test]
fn exact_intervals_and_clock_rollback() {
    let _test_state = crate::test_support::global_state();
    assert!(due(&Record::default(), 0, WEEK));
    let record = Record {
        last_trim_at: Some(100),
        last_attempt_at: Some(100),
        ..Record::default()
    };
    assert!(!due(&record, 100 + DAY - 1, DAY));
    assert!(due(&record, 100 + DAY, DAY));
    assert!(!due(&record, 100 + WEEK - 1, WEEK));
    assert!(due(&record, 100 + WEEK, WEEK));
    assert!(!due(&record, 50, DAY));
    let failed = Record {
        last_attempt_at: Some(100),
        ..Record::default()
    };
    assert!(!due(&failed, 100 + DAY - 1, WEEK));
    assert!(due(&failed, 100 + DAY, WEEK));
}
#[test]
fn manual_reclaim_bypasses_schedule_and_persists_actual_host_result() {
    let _test_state = crate::test_support::global_state();
    let (_dir, paths, configuration, observed) = fixture();
    let runner = Runner::new();
    trim(
        &runner,
        &paths,
        &configuration,
        &observed,
        TRIM_BUDGET,
        now(),
    )
    .unwrap();
    trim(
        &runner,
        &paths,
        &configuration,
        &observed,
        TRIM_BUDGET,
        now(),
    )
    .unwrap();
    let record = load(&paths, configuration.id()).unwrap();
    assert!(record.last_trim_at.is_some());
    assert_eq!(record.last_reclaimed_bytes, Some(0)); // Never use fstrim's logical byte count.
    assert!(record.last_error.is_none());
    assert_eq!(runner.calls.lock().unwrap().len(), 2);
    assert!(runner.calls.lock().unwrap()[0]
        .iter()
        .any(|s| s.contains("timeout -k 1s")));
}
#[test]
fn failure_is_durable_and_does_not_count_as_success_or_block_stop() {
    let _test_state = crate::test_support::global_state();
    let (_dir, paths, configuration, observed) = fixture();
    let runner = Runner {
        fail: true,
        ..Runner::new()
    };
    before_stop(&runner, &paths, &observed);
    let record = load(&paths, configuration.id()).unwrap();
    assert!(record.last_trim_at.is_none());
    assert!(record.last_error.is_some());
    assert!(!due(&record, now(), DAY));
    before_stop(&runner, &paths, &observed);
    assert_eq!(runner.calls.lock().unwrap().len(), 1);
}
#[test]
fn rejects_stopped_replaced_and_wrong_mount_without_guest_execution() {
    let _test_state = crate::test_support::global_state();
    let (_dir, paths, configuration, mut observed) = fixture();
    let runner = Runner::new();
    observed.status = "Stopped".into();
    assert!(trim(
        &runner,
        &paths,
        &configuration,
        &observed,
        TRIM_BUDGET,
        now()
    )
    .is_err());
    observed.status = "Running".into();
    observed.config["labels"]["silo.machine-id"] = json!("replacement");
    assert!(trim(
        &runner,
        &paths,
        &configuration,
        &observed,
        TRIM_BUDGET,
        now()
    )
    .is_err());
    observed.config["labels"]["silo.machine-id"] = json!(configuration.id());
    observed.active_config = Some(json!({"mounts":[]}));
    assert!(trim(
        &runner,
        &paths,
        &configuration,
        &observed,
        TRIM_BUDGET,
        now()
    )
    .is_err());
    observed.active_config = None;
    observed.config["mounts"][0]["storage"]["kind"] = json!("directory");
    assert!(trim(
        &runner,
        &paths,
        &configuration,
        &observed,
        TRIM_BUDGET,
        now()
    )
    .is_err());
    assert!(runner.calls.lock().unwrap().is_empty());
}
#[test]
fn stopped_usage_reports_allocated_blocks_without_starting_guest() {
    let _test_state = crate::test_support::global_state();
    let (_dir, paths, configuration, mut observed) = fixture();
    observed.status = "Stopped".into();
    let runner = Runner::new();
    let value = state(&runner, &paths, &configuration, &observed).unwrap();
    assert_eq!(
        value.workspace_host_bytes,
        Some(allocated(&owned_disk(&paths, "dev")).unwrap())
    );
    assert_eq!(value.workspace_used_bytes, None);
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[test]
fn failed_pending_restore_reports_present_disks_and_preserves_errors() {
    let _test_state = crate::test_support::global_state();
    struct Restored {
        observed: InspectedSandbox,
        guest: Runner,
    }
    impl RuntimeRunner for Restored {
        fn run(
            &self,
            paths: &RuntimePaths,
            args: &[String],
            timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            let stdout = match args[0].as_str() {
                "list" => json!([{"name":"dev"}]).to_string(),
                "inspect" => json!({"name":self.observed.name,"status":self.observed.status,"config":self.observed.config,"runtime_instance_id":self.observed.runtime_instance_id}).to_string(),
                _ => return self.guest.run(paths, args, timeout),
            };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }
    for status in ["Running", "Stopped"] {
        let (_dir, paths, configuration, mut observed) = fixture();
        observed.status = status.into();
        let root = paths.home.join("sandboxes/dev/rootfs.raw");
        fs::write(&root, vec![3u8; 8192]).unwrap();
        let pending: checkpoints::Record = serde_json::from_value(json!({
            "version":1,
            "checkpoints":[],
            "restoreAttempted":true,
            "pendingCheckpointRestore":{
                "checkpointId":"c000000000000000000000000000000",
                "sourceComputer":"source",
                "state":"disk"
            }
        }))
        .unwrap();
        checkpoints::save(&paths, configuration.id(), &pending).unwrap();
        save(
            &paths,
            configuration.id(),
            &Record {
                last_error: Some("Previous trim failed.".into()),
                ..Record::default()
            },
        )
        .unwrap();
        let runner = Restored {
            observed,
            guest: Runner::new(),
        };
        let value = storage_with(&runner, &paths, configuration.id(), false).unwrap();
        assert_eq!(
            value.workspace_host_bytes,
            Some(allocated(&owned_disk(&paths, "dev")).unwrap())
        );
        assert_eq!(value.runtime_host_bytes, Some(allocated(&root).unwrap()));
        assert_eq!(value.last_error.as_deref(), Some("Previous trim failed."));
        let error = storage_with(&runner, &paths, configuration.id(), true).unwrap_err();
        assert!(error.to_string().contains("restore"), "{error}");
        assert!(runner
            .guest
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| !args.iter().any(|arg| arg == TRIM)));
    }
}

#[test]
fn unstarted_pending_restore_reports_zero_without_starting_a_computer() {
    let _test_state = crate::test_support::global_state();
    struct EmptyRuntime;
    impl RuntimeRunner for EmptyRuntime {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            assert_eq!(args[0], "list", "storage must not start an absent computer");
            Ok(CommandOutput {
                stdout: "[]".into(),
                stderr: String::new(),
            })
        }
    }
    let (_dir, paths, configuration, _) = fixture();
    fs::remove_dir_all(paths.home.join("sandboxes/dev")).unwrap();
    let mut pending = checkpoints::Record::default();
    pending.pending_checkpoint_restore = Some(checkpoints::PendingRestore {
        checkpoint_id: "c000000000000000000000000000000".into(),
        source_computer: "source".into(),
        state: "disk".into(),
    });
    checkpoints::save(&paths, configuration.id(), &pending).unwrap();
    let value = storage_with(&EmptyRuntime, &paths, configuration.id(), false).unwrap();
    assert_eq!(value.workspace_host_bytes, Some(0));
    assert_eq!(value.runtime_host_bytes, Some(0));
    assert!(storage_with(&EmptyRuntime, &paths, configuration.id(), true).is_err());
}
#[test]
fn runtime_tail_truncation_is_repaired_and_reported_even_on_timeout() {
    let _test_state = crate::test_support::global_state();
    for fail in [false, true] {
        let (_dir, paths, configuration, observed) = fixture();
        let runner = Runner {
            fail,
            truncate: true,
            ..Runner::new()
        };
        assert!(trim(
            &runner,
            &paths,
            &configuration,
            &observed,
            TRIM_BUDGET,
            now()
        )
        .unwrap_err()
        .to_string()
        .contains("original length was restored"));
        let content = fs::read(owned_disk(&paths, "dev")).unwrap();
        assert_eq!(content.len(), 8192);
        assert_eq!(&content[..4096], &[7; 4096]);
        let record = load(&paths, configuration.id()).unwrap();
        assert!(record.last_trim_at.is_none());
    }
}
#[test]
fn expired_budget_does_not_enter_guest_or_record_attempt() {
    let _test_state = crate::test_support::global_state();
    let (_dir, paths, configuration, observed) = fixture();
    let runner = Runner::new();
    assert!(trim(
        &runner,
        &paths,
        &configuration,
        &observed,
        Duration::from_secs(3),
        now()
    )
    .is_err());
    assert!(runner.calls.lock().unwrap().is_empty());
    assert!(load(&paths, configuration.id())
        .unwrap()
        .last_attempt_at
        .is_none());
}
#[test]
fn malformed_measurements_and_history_are_not_silent_success() {
    let _test_state = crate::test_support::global_state();
    assert!(stats("999 100").is_err());
    assert!(stats("1 2 3").is_err());
    assert!(stats("-1 2").is_err());
    assert_eq!(stats(" 123 456\n").unwrap(), (123, 456));
    let (_dir, paths, configuration, observed) = fixture();
    fs::create_dir_all(record_path(&paths, configuration.id()).parent().unwrap()).unwrap();
    fs::write(record_path(&paths, configuration.id()), b"broken").unwrap();
    let runner = Runner::new();
    assert!(trim(
        &runner,
        &paths,
        &configuration,
        &observed,
        TRIM_BUDGET,
        now()
    )
    .is_err());
    assert!(runner.calls.lock().unwrap().is_empty());
    assert_eq!(
        fs::read(record_path(&paths, configuration.id())).unwrap(),
        b"broken"
    );
}

#[test]
fn normal_stop_completes_after_a_failed_trim_and_does_not_retry_it() {
    let _test_state = crate::test_support::global_state();
    struct LifecycleRunner {
        running: Mutex<bool>,
        calls: Mutex<Vec<String>>,
        config: Value,
    }
    impl RuntimeRunner for LifecycleRunner {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args[0].clone());
            let stdout = match args[0].as_str() {
                "inspect" => json!({"name":"dev", "runtime_instance_id":"run-1", "status": if *self.running.lock().unwrap() {"Running"} else {"Stopped"}, "config":self.config}).to_string(),
                "exec" => return Err(RuntimeError::TimedOut { operation: "trim".into() }),
                "stop" => { *self.running.lock().unwrap() = false; String::new() }
                other => panic!("Unexpected command: {other}"),
            };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }
    let (_dir, paths, configuration, observed) = fixture();
    let runner = LifecycleRunner {
        running: Mutex::new(true),
        calls: Mutex::new(vec![]),
        config: observed.config,
    };
    let device = DeviceResources {
        logical_cpus: 0,
        physical_memory_bytes: None,
    };
    lifecycle_recovery::perform(&runner, &paths, &device, "stop", "dev").unwrap();
    assert!(!*runner.running.lock().unwrap());
    assert_eq!(
        *runner.calls.lock().unwrap(),
        ["inspect", "exec", "stop", "inspect"]
    );
    assert!(load(&paths, configuration.id())
        .unwrap()
        .last_error
        .is_some());
}

#[test]
fn next_start_trims_once_and_recent_success_skips_automatic_work() {
    let _test_state = crate::test_support::global_state();
    let (_dir, paths, configuration, observed) = fixture();
    let runner = Runner::new();
    verified_starts()
        .lock()
        .unwrap()
        .remove(&(paths.home.clone(), configuration.id().into()));
    after_start(&runner, &paths, &observed);
    before_stop(&runner, &paths, &observed);
    after_start(&runner, &paths, &observed);
    assert_eq!(
        runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|args| args[0] == "exec")
            .count(),
        1
    );
    assert!(load(&paths, configuration.id())
        .unwrap()
        .last_trim_at
        .is_some());
}

#[test]
fn old_or_replaced_worker_requires_restart_before_any_trim() {
    let _test_state = crate::test_support::global_state();
    let (_dir, paths, configuration, mut observed) = fixture();
    let runner = Runner::new();
    observed.runtime_instance_id = Some("different-run".into());
    assert!(trim(
        &runner,
        &paths,
        &configuration,
        &observed,
        TRIM_BUDGET,
        now()
    )
    .unwrap_err()
    .to_string()
    .contains("Restart this computer"));
    observed.runtime_instance_id = None;
    assert!(trim(
        &runner,
        &paths,
        &configuration,
        &observed,
        TRIM_BUDGET,
        now()
    )
    .is_err());
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[test]
fn periodic_reclaims_due_running_computer_once_without_starting_any_computer() {
    let _test_state = crate::test_support::global_state();
    struct PeriodicRunner {
        guest: Runner,
        config: Value,
        inspections: Mutex<usize>,
    }
    impl RuntimeRunner for PeriodicRunner {
        fn run(
            &self,
            paths: &RuntimePaths,
            args: &[String],
            timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            if args[0] == "inspect" {
                *self.inspections.lock().unwrap() += 1;
                return Ok(CommandOutput { stdout: json!({"name":"dev", "status":"Running", "runtime_instance_id":"run-1", "config":self.config}).to_string(), stderr: String::new() });
            }
            self.guest.run(paths, args, timeout)
        }
    }
    let (_dir, paths, configuration, observed) = fixture();
    let runner = PeriodicRunner {
        guest: Runner::new(),
        config: observed.config,
        inspections: Mutex::new(0),
    };
    save(
        &paths,
        configuration.id(),
        &Record {
            last_trim_at: Some(now() - WEEK),
            last_attempt_at: Some(now() - WEEK),
            ..Record::default()
        },
    )
    .unwrap();
    assert!(periodic(&runner, &paths, &HashMap::new()).unwrap());
    assert!(!periodic(&runner, &paths, &HashMap::new()).unwrap());
    assert_eq!(*runner.inspections.lock().unwrap(), 1);
    assert_eq!(runner.guest.calls.lock().unwrap().len(), 1);
}

#[test]
fn maintenance_tick_preserves_damaged_checkpoint_and_trims_healthy_owner() {
    let _test_state = crate::test_support::global_state();
    struct MaintenanceRunner {
        guest: Runner,
        config: Value,
    }
    impl RuntimeRunner for MaintenanceRunner {
        fn run(
            &self,
            paths: &RuntimePaths,
            args: &[String],
            timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            if args[0] == "inspect" {
                assert_eq!(args[1], "dev", "damaged owner must not be trimmed");
                return Ok(CommandOutput {
                    stdout: json!({"name":"dev", "status":"Running", "runtime_instance_id":"run-1", "config":self.config}).to_string(),
                    stderr: String::new(),
                });
            }
            self.guest.run(paths, args, timeout)
        }
    }
    let (_dir, paths, configuration, observed) = fixture();
    let mut damaged = configuration.clone();
    {
        let ComputerConfiguration { id, name, .. } = &mut damaged;
        *id = "00000000-0000-4000-8000-000000000002".into();
        *name = "damaged".into();
    }
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![damaged.clone(), configuration.clone()],
        },
    )
    .unwrap();
    let checkpoint_path = paths
        .metadata
        .with_file_name("checkpoints")
        .join(format!("{}.json", damaged.id()));
    fs::create_dir_all(checkpoint_path.parent().unwrap()).unwrap();
    fs::write(&checkpoint_path, b"{broken").unwrap();
    let runner = MaintenanceRunner {
        guest: Runner::new(),
        config: observed.config,
    };
    assert!(maintenance_tick(&runner, &paths).unwrap());
    assert_eq!(fs::read(&checkpoint_path).unwrap(), b"{broken");
    assert!(load(&paths, configuration.id())
        .unwrap()
        .last_trim_at
        .is_some());
    assert!(load(&paths, damaged.id())
        .unwrap()
        .last_attempt_at
        .is_none());
    assert_eq!(runner.guest.calls.lock().unwrap().len(), 1);
}

#[test]
fn unavailable_guest_stats_do_not_hide_a_maintenance_failure() {
    let _test_state = crate::test_support::global_state();
    let (_dir, paths, configuration, observed) = fixture();
    let runner = Runner {
        fail: true,
        ..Runner::new()
    };
    save(
        &paths,
        configuration.id(),
        &Record {
            last_error: Some("Preserved maintenance failure".into()),
            ..Record::default()
        },
    )
    .unwrap();
    let value = state(&runner, &paths, &configuration, &observed).unwrap();
    assert_eq!(
        value.last_error.as_deref(),
        Some("Preserved maintenance failure")
    );
    assert_eq!(value.workspace_used_bytes, None);
}

#[test]
fn starting_computer_does_not_begin_maintenance_during_quit() {
    let _test_state = crate::test_support::global_state();
    let (_dir, paths, configuration, observed) = fixture();
    let runner = Runner::new();
    shutdown::begin();
    after_start(&runner, &paths, &observed);
    assert!(runner.calls.lock().unwrap().is_empty());
    assert!(load(&paths, configuration.id())
        .unwrap()
        .last_attempt_at
        .is_none());
}

// Explicitly opt-in: boots and trims the named local computer, preserving its files.
// Keep separate from ordinary tests and never distribute the test executable.
#[test]
#[ignore = "requires explicit live computer runtime, home, identity and baseline checksum"]
fn live_reclaim_preserves_capacity_contents_and_reboots() {
    let _test_state = crate::test_support::global_state();
    crate::test_support::live::require_confirmation();
    let executable = PathBuf::from(
        std::env::var("SILO_STORAGE_LIVE_RUNTIME").expect("explicit runtime path required"),
    );
    let home = PathBuf::from(
        std::env::var("SILO_STORAGE_LIVE_HOME").expect("explicit runtime home required"),
    );
    let id = std::env::var("SILO_STORAGE_LIVE_ID").expect("explicit computer ID required");
    let expected_hash = std::env::var("SILO_STORAGE_EXPECTED_SHA256")
        .expect("pre-existing workspace checksum required");
    let storage = fs::canonicalize(&home)
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let library = executable
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Frameworks/libkrunfw.5.dylib");
    assert!(library.is_file());
    let paths = RuntimePaths {
        executable,
        library,
        home,
        storage_home: None,
        metadata: storage.join("computers.json"),
        volumes: storage.join("volumes"),
        guest_image: storage.join("unused-image"),
    };
    let _guard = OPERATIONS.device("Live reclaim test").unwrap();
    let configuration = computer_configuration(&paths, &id).unwrap();
    let runner = ProcessRunner;
    let initial = inspect_computer(&runner, &paths, configuration.name()).unwrap();
    assert!(
        initial.status.eq_ignore_ascii_case("stopped"),
        "Do not interrupt an existing running computer"
    );
    let disk = owned_disk(&paths, configuration.name());
    let length = fs::metadata(&disk).unwrap().len();
    let device = device_resources().unwrap();
    let checksum = WORKSPACE_CHECKSUM;
    let mut previous_instance = None;
    for _ in 0..2 {
        let result = (|| -> Result<(), RuntimeError> {
            lifecycle_recovery::perform(&runner, &paths, &device, "start", configuration.name())?;
            let observed = inspect_computer(&runner, &paths, configuration.name())?;
            if !verified_worker(&paths, &configuration, &observed)
                || observed.runtime_instance_id == previous_instance
            {
                return Err(failure(
                    "The newly started runtime instance was not verified.",
                ));
            }
            previous_instance = observed.runtime_instance_id.clone();
            let before = runner.run(
                &paths,
                &guest_args(configuration.name(), 29, checksum),
                Duration::from_secs(30),
            )?;
            if before.stdout.split_whitespace().next() != Some(expected_hash.as_str()) {
                return Err(failure(
                    "Workspace checksum differs from the pre-existing baseline.",
                ));
            }
            trim(
                &runner,
                &paths,
                &configuration,
                &observed,
                TRIM_BUDGET,
                now(),
            )?;
            let after = runner.run(
                &paths,
                &guest_args(configuration.name(), 29, checksum),
                Duration::from_secs(30),
            )?;
            if after.stdout != before.stdout || fs::metadata(&disk).unwrap().len() != length {
                return Err(failure("Computer contents or logical capacity changed."));
            }
            let measurements = state(&runner, &paths, &configuration, &observed)?;
            if measurements.workspace_used_bytes.is_none() || measurements.last_error.is_some() {
                return Err(failure(
                    "Live storage measurements or trim result were unavailable.",
                ));
            }
            println!(
                "Verified live storage: {}",
                serde_json::to_string(&measurements).unwrap()
            );
            Ok(())
        })();
        let stopped =
            lifecycle_recovery::perform(&runner, &paths, &device, "stop", configuration.name());
        result.unwrap();
        stopped.unwrap();
        assert_eq!(fs::metadata(&disk).unwrap().len(), length);
        assert!(inspect_computer(&runner, &paths, configuration.name())
            .unwrap()
            .status
            .eq_ignore_ascii_case("stopped"));
    }
}

fn workspace_dir(paths: &RuntimePaths, name: &str) -> PathBuf {
    owned_disk(paths, name).parent().unwrap().to_path_buf()
}

#[test]
fn restored_layered_computer_is_measured_and_a_missing_disk_is_unknown() {
    let _test_state = crate::test_support::global_state();
    // A computer restored from a checkpoint keeps its workspace disk as sealed layers plus a
    // writable qcow2 head; there is no disk.raw.
    let (_dir, paths, configuration, mut observed) = fixture();
    let directory = workspace_dir(&paths, "dev");
    fs::remove_file(owned_disk(&paths, "dev")).unwrap();
    fs::write(directory.join("sealed-000.raw"), vec![1u8; 16384]).unwrap();
    fs::write(directory.join("writable.qcow2"), vec![2u8; 8192]).unwrap();
    observed.status = "Stopped".into();
    let runner = Runner::new();
    let expected = allocated(&directory.join("sealed-000.raw")).unwrap()
        + allocated(&directory.join("writable.qcow2")).unwrap();
    let value = state(&runner, &paths, &configuration, &observed).unwrap();
    assert_eq!(value.workspace_host_bytes, Some(expected));
    assert!(expected > 0);
    assert!(workspace_mount(&paths, &configuration, &observed));

    fs::remove_dir_all(&directory).unwrap();
    let value = state(&runner, &paths, &configuration, &observed).unwrap();
    assert_eq!(
        value.workspace_host_bytes, None,
        "a missing workspace disk is unknown, not 0 B"
    );
    assert!(!workspace_mount(&paths, &configuration, &observed));
}

#[test]
fn runtime_usage_counts_flat_roots_and_is_unknown_without_a_computer_directory() {
    let _test_state = crate::test_support::global_state();
    let (_dir, paths, configuration, mut observed) = fixture();
    observed.status = "Stopped".into();
    let computer = paths.home.join("sandboxes/dev");
    fs::write(computer.join("rootfs.raw"), vec![3u8; 8192]).unwrap();
    fs::write(computer.join("upper.ext4"), vec![4u8; 8192]).unwrap();
    let expected = allocated(&computer.join("rootfs.raw")).unwrap()
        + allocated(&computer.join("upper.ext4")).unwrap();
    let runner = Runner::new();
    assert_eq!(
        state(&runner, &paths, &configuration, &observed)
            .unwrap()
            .runtime_host_bytes,
        Some(expected)
    );
    fs::remove_dir_all(&computer).unwrap();
    assert_eq!(
        state(&runner, &paths, &configuration, &observed)
            .unwrap()
            .runtime_host_bytes,
        None
    );
}

#[test]
fn layered_computer_reclaim_measures_the_whole_chain_and_guards_every_layer_length() {
    let _test_state = crate::test_support::global_state();
    struct LayerRunner {
        shrink: bool,
    }
    impl RuntimeRunner for LayerRunner {
        fn run(
            &self,
            paths: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            if args[0] == "--silo-storage-protocol" {
                return Ok(CommandOutput {
                    stdout: "1\n".into(),
                    stderr: String::new(),
                });
            }
            let head = workspace_dir(paths, "dev").join("writable.qcow2");
            let file = fs::OpenOptions::new().write(true).open(&head).unwrap();
            // Discard releases the head's blocks but keeps its logical length (hole punch),
            // unless this runner simulates the old truncating runtime.
            file.set_len(0).unwrap();
            if !self.shrink {
                file.set_len(1024 * 1024).unwrap();
            }
            Ok(CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }
    for shrink in [false, true] {
        let (_dir, paths, configuration, observed) = fixture();
        let head = workspace_dir(&paths, "dev").join("writable.qcow2");
        fs::write(&head, vec![5u8; 1024 * 1024]).unwrap();
        let result = trim(
            &LayerRunner { shrink },
            &paths,
            &configuration,
            &observed,
            TRIM_BUDGET,
            now(),
        );
        let record = load(&paths, configuration.id()).unwrap();
        assert_eq!(
            fs::metadata(&head).unwrap().len(),
            1024 * 1024,
            "every layer keeps its logical length"
        );
        if shrink {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("original length was restored"));
            assert!(record.last_trim_at.is_none());
        } else {
            result.unwrap();
            assert!(
                record.last_reclaimed_bytes.unwrap() >= 1024 * 1024,
                "{:?}",
                record.last_reclaimed_bytes
            );
        }
    }
}

#[test]
fn qcow2_growth_during_reclaim_is_not_a_disk_capacity_change() {
    let _test_state = crate::test_support::global_state();
    struct GrowingDisk(PathBuf);
    impl RuntimeRunner for GrowingDisk {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            assert_eq!(args[0], "exec");
            fs::OpenOptions::new()
                .append(true)
                .open(&self.0)
                .unwrap()
                .write_all(&vec![8u8; 4096])
                .unwrap();
            Ok(CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }
    for qcow2 in [true, false] {
        let (_dir, paths, configuration, observed) = fixture();
        let disk = if qcow2 {
            let head = workspace_dir(&paths, "dev").join("writable.qcow2");
            fs::write(&head, vec![7u8; 8192]).unwrap();
            head
        } else {
            owned_disk(&paths, "dev")
        };
        let result = trim(
            &GrowingDisk(disk.clone()),
            &paths,
            &configuration,
            &observed,
            TRIM_BUDGET,
            now(),
        );
        let record = load(&paths, configuration.id()).unwrap();
        if qcow2 {
            result.unwrap();
            assert!(record.last_trim_at.is_some());
            assert_eq!(record.last_reclaimed_bytes, Some(0));
        } else {
            assert!(result.unwrap_err().to_string().contains("size changed"));
            assert!(record.last_trim_at.is_none());
        }
        let contents = fs::read(&disk).unwrap();
        assert_eq!(&contents[..8192], &vec![7u8; 8192]);
        assert_eq!(&contents[8192..], &vec![8u8; 4096]);
    }
}

#[test]
fn history_retains_latest_fifty_attempts_including_failures() {
    let _test_state = crate::test_support::global_state();
    let (_dir, paths, configuration, observed) = fixture();
    for at in 0..51 {
        trim_triggered(
            &Runner::new(),
            &paths,
            &configuration,
            &observed,
            TRIM_BUDGET,
            at,
            "scheduled",
        )
        .unwrap();
    }
    let failing = Runner {
        fail: true,
        ..Runner::new()
    };
    assert!(trim_triggered(
        &failing,
        &paths,
        &configuration,
        &observed,
        TRIM_BUDGET,
        51,
        "manual"
    )
    .is_err());
    let record = load(&paths, configuration.id()).unwrap();
    assert_eq!(record.history.len(), 50);
    assert_eq!(record.history[0].at, 51);
    assert_eq!(record.history[49].at, 2);
    assert_eq!(record.history[0].trigger, "manual");
    assert!(record.history[0].error.is_some());
    assert_eq!(record.history[0].reclaimed_bytes, None);
    assert_eq!(record.history[1].trigger, "scheduled");
    assert_eq!(record.history[1].reclaimed_bytes, Some(0));
    assert!(record.history[1].error.is_none());
}

#[test]
fn additive_maintenance_fields_survive_save_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    let id = "00000000-0000-4000-8000-000000000001";
    let path = record_path(&paths, id);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let saved = json!({
        "lastTrimAt": 100,
        "lastAttemptAt": 100,
        "lastReclaimedBytes": 2048,
        "lastError": null,
        "futureMaintenance": {"method": "discard", "enabled": true},
        "history": [{
            "at": 100,
            "trigger": "future-trigger",
            "reclaimedBytes": 2048,
            "error": null,
            "futureMeasurement": {"bytes": 4096}
        }]
    });
    fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();

    let mut record = load(&paths, id).unwrap();
    assert_eq!(record.last_trim_at, Some(100));
    assert_eq!(record.history[0].trigger, "future-trigger");
    record.last_attempt_at = Some(200);
    save(&paths, id, &record).unwrap();

    let written: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(written["futureMaintenance"], saved["futureMaintenance"]);
    assert_eq!(written["history"], saved["history"]);
    let reloaded = load(&paths, id).unwrap();
    assert_eq!(reloaded.last_attempt_at, Some(200));
    assert_eq!(reloaded.last_trim_at, Some(100));
    assert_eq!(reloaded.history.len(), 1);
}

#[test]
fn malformed_known_maintenance_fields_still_preserve_the_file() {
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    let id = "00000000-0000-4000-8000-000000000001";
    let path = record_path(&paths, id);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = br#"{"lastTrimAt":"invalid","futureMaintenance":true}"#;
    fs::write(&path, bytes).unwrap();
    assert!(load(&paths, id).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn legacy_record_keeps_last_success_when_history_is_introduced() {
    let _test_state = crate::test_support::global_state();
    let (_dir, paths, configuration, _) = fixture();
    let path = record_path(&paths, configuration.id());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        r#"{"lastTrimAt":100,"lastAttemptAt":100,"lastReclaimedBytes":2048,"lastError":null}"#,
    )
    .unwrap();
    let record = load(&paths, configuration.id()).unwrap();
    assert_eq!(record.history.len(), 1);
    assert_eq!(record.history[0].at, 100);
    assert_eq!(record.history[0].reclaimed_bytes, Some(2048));
    assert_eq!(record.history[0].trigger, "legacy");
    save(&paths, configuration.id(), &record).unwrap();
    assert_eq!(load(&paths, configuration.id()).unwrap().history.len(), 1);
}

fn parsed(status: &str, instance: Option<Value>) -> InspectedSandbox {
    let mut value = json!({"name":"dev", "status":status, "config": {}});
    if let Some(instance) = instance {
        value["runtime_instance_id"] = instance;
    }
    serde_json::from_value(value).unwrap()
}

#[test]
fn a_reported_instance_is_the_running_identity() {
    let directory = tempfile::tempdir().unwrap();
    let paths = super::super::tests::paths(&directory);
    let found = running_instance_id(&paths, &parsed("Running", Some(json!("run-1:t"))));
    assert_eq!(found.unwrap().as_deref(), Some("run-1:t"));
}

#[test]
fn a_stopped_computer_or_an_instance_not_established_yet_is_quietly_none() {
    let directory = tempfile::tempdir().unwrap();
    let paths = super::super::tests::paths(&directory);
    // Stopped: nothing runs, whatever the runtime reports.
    assert_eq!(
        running_instance_id(&paths, &parsed("Stopped", None)).unwrap(),
        None
    );
    // The runtime reports the entry but no active run matches yet (null).
    let unestablished = parsed("Running", Some(Value::Null));
    assert!(unestablished.runtime_instance_reported);
    assert_eq!(running_instance_id(&paths, &unestablished).unwrap(), None);
}

#[test]
fn a_runtime_without_instance_reporting_is_an_explicit_error_not_a_silent_skip() {
    let directory = tempfile::tempdir().unwrap();
    let paths = super::super::tests::paths(&directory);
    // A runtime without the patch never writes the entry at all.
    let unpatched = parsed("Running", None);
    assert!(!unpatched.runtime_instance_reported);
    let error = running_instance_id(&paths, &unpatched).unwrap_err();
    assert!(
        error.to_string().contains("runtime_instance_id"),
        "the diagnostic names the missing capability: {error}"
    );
}
