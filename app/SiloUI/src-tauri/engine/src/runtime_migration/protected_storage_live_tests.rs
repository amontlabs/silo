//! Opt-in live check of the protected storage step against a home made by the 0.7.6 runtime.
//! `docs/direction/checks/msb_0714_migration_old_home.sh` builds the home (`SILO_M14_APP`, the
//! app-data folder holding `runtime-checkpoints-converted/`); this test runs the engine's own
//! launch step on it with the bundled runtime (`SILO_TEST_MSB`, `SILO_TEST_LIBKRUNFW`).
use super::protected_storage::*;
use super::{fresh, select_generation, write, Controller, CONVERTED, FILE};
use crate::runtime::{self, ProcessRunner, RuntimePaths, RuntimeRunner};
use std::{fs, path::PathBuf, time::Duration};

const GOOD: [&str; 4] = ["e2e-mig-main", "e2e-mig-fork", "e2e-mig-disk", "e2e-mig-run"];
const T: Duration = Duration::from_secs(300);

fn inventory(names: &[&str]) -> runtime::ComputerConfigurationRequest {
    let computers: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            serde_json::json!({"id":format!("00000000-0000-4000-8000-00000000000{index}"),"name":name,"cpus":1,"maxCPUs":1,"memoryGiB":1,"maxMemoryGiB":1,"workspaceStorageGiB":1,"runtimeStorageGiB":2})
        })
        .collect();
    serde_json::from_value(serde_json::json!({"schemaVersion":1,"computers":computers})).unwrap()
}

/// The runtime called directly: the computers restored and forked by the 0.7.6 runtime carry no
/// Silo ownership label, which `run_msb` insists on for start and exec.
fn msb(paths: &RuntimePaths, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new(&paths.executable)
        .args(args)
        .env("MSB_HOME", &paths.home)
        .env("MSB_PATH", &paths.executable)
        .env("MSB_LIBKRUNFW_PATH", &paths.library)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("{args:?}: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!("{args:?}: {}", String::from_utf8_lossy(&output.stderr)))
    }
}

fn read(paths: &RuntimePaths, name: &str, file: &str) -> String {
    msb(paths, &["exec", name, "--", "sh", "-c", &format!("cat {file} 2>/dev/null || printf MISSING")])
        .unwrap_or_else(|error| panic!("{error}"))
}

fn stopped(paths: &RuntimePaths, name: &str) {
    let _ = msb(paths, &["stop", name]);
}

fn state_of(paths: &RuntimePaths, name: &str) -> String {
    msb(paths, &["doctor", "--sandbox", name, "--format", "json"]).unwrap_or_else(|e| e)
}

#[test]
#[ignore = "requires SILO_M14_APP from msb_0714_migration_old_home.sh, the packaged runtime and hardware virtualization"]
fn live_old_home_moves_to_protected_storage() {
    crate::test_support::live::require_confirmation();
    let _test_state = crate::test_support::global_state();
    let app = PathBuf::from(std::env::var("SILO_M14_APP").expect("SILO_M14_APP"));
    assert!(app.starts_with("/tmp/m14"), "private home only");
    assert!(!app.ends_with(".silo") && !app.ends_with(".silo-dev"));
    let storage = app.join(CONVERTED);
    let paths = RuntimePaths {
        guest_image: crate::test_support::manifest_root().join("runtime/guest-image"),
        executable: PathBuf::from(std::env::var("SILO_TEST_MSB").unwrap()),
        library: PathBuf::from(std::env::var("SILO_TEST_LIBKRUNFW").unwrap()),
        home: storage.join("microsandbox"),
        storage_home: None,
        metadata: storage.join("computers.json"),
        volumes: storage.join("volumes"),
    };
    runtime::write_metadata(&paths.metadata, &inventory(&GOOD)).unwrap();
    select_generation(&app, CONVERTED).unwrap();
    write(&app.join(FILE), &fresh("complete", GOOD.len())).unwrap();

    // Launch 1: the 0.7.6 computer left running is still running when the new engine starts.
    let launch = |label: &str| -> (Vec<String>, Report) {
        let controller = Controller::new(app.clone());
        assert!(controller.hold_for_protected_storage(), "{label}: the step has work");
        assert!(controller.blocks_operations());
        let stages = std::cell::RefCell::new(Vec::<String>::new());
        controller
            .run_protected_storage(&ProcessRunner, &paths, &|| {
                let stage = controller.snapshot().unwrap().stage;
                if stages.borrow().last() != Some(&stage) {
                    stages.borrow_mut().push(stage);
                }
            })
            .unwrap();
        assert!(!controller.blocks_operations());
        eprintln!("STAGES {label} {:?}", stages.borrow());
        eprintln!("REPORT {label} {}", fs::read_to_string(storage.join(MARKER)).unwrap());
        (stages.into_inner(), read_report(&storage).unwrap().unwrap())
    };
    let protected = |home: &std::path::Path, name: &str| {
        home.join("microsandbox/sandboxes").join(name).join("state-layout.json").exists()
    };
    let (stages1, report1) = launch("launch 1");
    assert!(stages1.iter().any(|s| s.starts_with("Stopping")), "the running computer was stopped");
    assert!(stages1.iter().any(|s| s.starts_with("Backing up")));
    let backup = app.join(BACKUP);
    assert!(backup.join("computers.json").is_file());
    assert!(!protected(&backup, "e2e-mig-run"), "the backup was taken before the repair");
    assert!(protected(&storage, "e2e-mig-run"));
    for name in GOOD {
        let outcome = &report1.computers.iter().find(|c| c.name == name).unwrap().outcome;
        eprintln!("LAUNCH1 {name}: {outcome:?}");
    }
    if !report1.complete() {
        // Finding: while a legacy VM runs, `doctor` rates every other computer recovery_required.
        eprintln!("FINDING launch 1 left computers needing attention; relaunching as the step intends");
        let (_, report2) = launch("launch 2");
        assert!(report2.complete(), "launch 2 finishes what launch 1 flagged");
    }
    let report = read_report(&storage).unwrap().unwrap();
    for name in GOOD {
        let outcome = &report.computers.iter().find(|c| c.name == name).unwrap().outcome;
        assert!(matches!(outcome, Outcome::Repaired | Outcome::AlreadyReady), "{name}: {outcome:?}");
        assert!(protected(&storage, name), "{name} repaired in place");
    }
    let covers: Vec<String> =
        serde_json::from_slice(&fs::read(backup.join("silo-backup-covers.json")).unwrap()).unwrap();
    for name in ["e2e-mig-main", "e2e-mig-fork", "e2e-mig-disk"] {
        assert!(covers.iter().any(|c| c == name), "{name} covered by the backup");
        assert!(!protected(&backup, name), "{name}: the backup was taken before its repair");
    }
    let marker_before = fs::read(storage.join(MARKER)).unwrap();

    // Next launch: nothing to do, nothing rewritten.
    let again = Controller::new(app.clone());
    assert!(!again.hold_for_protected_storage(), "a complete report ends the step");
    assert_eq!(fs::read(storage.join(MARKER)).unwrap(), marker_before);

    // Every computer starts with its data.
    let expect: [(&str, &str, &str); 9] = [
        ("e2e-mig-main", "/workspace/a", "main-1"),
        ("e2e-mig-main", "/workspace/b", "main-2"),
        ("e2e-mig-main", "/workspace/c", "main-3"),
        ("e2e-mig-main", "/root/a", "main-root-1"),
        ("e2e-mig-fork", "/workspace/r", "run-1"),
        ("e2e-mig-fork", "/workspace/f", "fork-1"),
        ("e2e-mig-disk", "/workspace/a", "main-1"),
        ("e2e-mig-disk", "/workspace/d", "disk-1"),
        ("e2e-mig-run", "/workspace/r", "run-1"),
    ];
    for name in GOOD {
        msb(&paths, &["start", name]).unwrap_or_else(|e| panic!("start {name}: {e}"));
    }
    for (name, file, content) in expect {
        assert_eq!(read(&paths, name, file), content, "{name}:{file}");
    }
    assert_eq!(read(&paths, "e2e-mig-disk", "/workspace/b"), "MISSING", "the restore predates b");
    for name in GOOD {
        stopped(&paths, name);
    }

    // Checkpoints made by 0.7.6 restore: the disk one cold, the full one with its memory.
    msb(&paths, &["restore", "e2e-mig-main:diskck", "--name", "e2e-mig-rdisk"]).unwrap();
    msb(&paths, &["start", "e2e-mig-rdisk"]).ok();
    assert_eq!(read(&paths, "e2e-mig-rdisk", "/workspace/a"), "main-1");
    assert_eq!(read(&paths, "e2e-mig-rdisk", "/workspace/b"), "MISSING");
    stopped(&paths, "e2e-mig-rdisk");
    msb(&paths, &["restore", "e2e-mig-main:fullck", "--name", "e2e-mig-rfull"]).unwrap();
    msb(&paths, &["start", "e2e-mig-rfull"]).ok();
    assert_eq!(read(&paths, "e2e-mig-rfull", "/workspace/b"), "main-2");
    assert_eq!(read(&paths, "e2e-mig-rfull", "/workspace/c"), "MISSING");
    stopped(&paths, "e2e-mig-rfull");
    msb(&paths, &["restore", "e2e-mig-main:fullck", "--name", "e2e-mig-rcold", "--disk-only"]).unwrap();
    msb(&paths, &["start", "e2e-mig-rcold"]).ok();
    assert_eq!(read(&paths, "e2e-mig-rcold", "/workspace/b"), "main-2");
    assert_eq!(read(&paths, "e2e-mig-rcold", "/workspace/c"), "MISSING");
    stopped(&paths, "e2e-mig-rcold");

    // A computer that arrives with the old layout is flagged, not touched.
    let late = "e2e-mig-late";
    check_late(&paths, &app, &storage, late);

    // A computer the runtime cannot establish (damaged legacy records) is refused with its
    // files kept, the others are not disturbed, and the step stays pending.
    let bad = "e2e-mig-bad";
    let mut all = GOOD.to_vec();
    all.push(bad);
    runtime::write_metadata(&paths.metadata, &inventory(&all)).unwrap();
    fs::remove_file(storage.join(MARKER)).unwrap();
    eprintln!("BAD doctor before: {}", state_of(&paths, bad));
    let third = Controller::new(app.clone());
    assert!(third.hold_for_protected_storage());
    third.run_protected_storage(&ProcessRunner, &paths, &|| {}).unwrap();
    let report = read_report(&storage).unwrap().unwrap();
    eprintln!("REPORT2 {}", fs::read_to_string(storage.join(MARKER)).unwrap());
    assert!(!report.complete());
    let message = attention_message(&paths.metadata, bad).expect("flagged");
    assert!(message.contains("kept untouched") || message.contains("kept"), "{message}");
    for name in GOOD {
        let outcome = &report.computers.iter().find(|c| c.name == name).unwrap().outcome;
        assert_eq!(outcome, &Outcome::AlreadyReady, "{name}");
    }
    assert!(!protected(&storage, bad));
    assert!(Controller::new(app.clone()).hold_for_protected_storage(), "retried next launch");
    eprintln!("Verified the protected storage step on a 0.7.6 home.");
}

fn check_late(paths: &RuntimePaths, app: &std::path::Path, storage: &std::path::Path, late: &str) {
    super::check_protected_storage_of_arrival(&ProcessRunner, paths, late);
    let message = attention_message(&paths.metadata, late).expect("flagged on arrival");
    eprintln!("LATE {message}");
    let report = read_report(storage).unwrap().unwrap();
    assert!(!report.complete());
    assert!(report.computers.iter().any(|c| c.name == late));
    // Back to the clean report for the next part.
    let mut report = report;
    report.computers.retain(|c| c.name != late);
    write_report(storage, &report).unwrap();
    let _ = app;
}
