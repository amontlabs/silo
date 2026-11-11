use super::protected_storage::*;
use crate::runtime::{self, RuntimeError};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

/// One computer of the fake runtime: what `doctor` reports and what `repair` does.
#[derive(Clone)]
struct Fake {
    status: &'static str,
    running: bool,
    /// `doctor` leaves out `running`, or prints it as a string.
    running_field: RunningField,
    /// After this many `doctor` calls the computer is running again (something started it).
    starts_after_doctors: Option<usize>,
    /// `repair` fails with this exit code instead of repairing.
    repair_exit: Option<i32>,
    /// `repair` reports success but leaves the status unchanged.
    repair_is_a_no_op: bool,
    /// `stop` succeeds but the computer keeps running.
    stays_running: bool,
}

#[derive(Clone, Copy)]
enum RunningField {
    Bool,
    Missing,
    NotBoolean,
}

fn computer(status: &'static str, running: bool) -> Fake {
    Fake {
        status,
        running,
        running_field: RunningField::Bool,
        starts_after_doctors: None,
        repair_exit: None,
        repair_is_a_no_op: false,
        stays_running: false,
    }
}

struct Runtime {
    computers: RefCell<HashMap<String, Fake>>,
    calls: RefCell<Vec<String>>,
    /// What `--silo-storage-repair-protocol` prints; `None` fails as an unknown option.
    probe: Option<&'static str>,
    /// The macOS `doctor`: while any computer runs, every stopped one is `recovery_required`.
    running_blocks_others: bool,
}

impl Runtime {
    fn new(computers: &[(&str, Fake)]) -> Self {
        Self {
            computers: RefCell::new(
                computers
                    .iter()
                    .map(|(name, fake)| (name.to_string(), fake.clone()))
                    .collect(),
            ),
            calls: RefCell::default(),
            probe: Some("1\n"),
            running_blocks_others: false,
        }
    }

    fn macos_doctor(mut self) -> Self {
        self.running_blocks_others = true;
        self
    }

    fn without_repair_protocol(mut self, probe: Option<&'static str>) -> Self {
        self.probe = probe;
        self
    }

    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

fn failed_with(exit_code: i32, detail: String) -> RuntimeError {
    RuntimeError::Failed {
        operation: "msb".into(),
        exit_code: Some(exit_code),
        detail,
    }
}

fn failed(exit_code: i32) -> RuntimeError {
    failed_with(exit_code, String::new())
}

impl runtime::RuntimeRunner for Runtime {
    fn run(
        &self,
        _: &runtime::RuntimePaths,
        args: &[String],
        _: Duration,
    ) -> Result<runtime::CommandOutput, RuntimeError> {
        self.calls.borrow_mut().push(args.join(" "));
        let output = |stdout: String| runtime::CommandOutput {
            stdout,
            stderr: String::new(),
        };
        let mut computers = self.computers.borrow_mut();
        // A running computer with protected storage is no legacy writer.
        let any_running = computers.values().any(|fake| fake.running && fake.status != "ready");
        match args[0].as_str() {
            "inspect" => {
                let fake = computers.get(&args[1]).unwrap();
                let status = if fake.running { "Running" } else { "Stopped" };
                Ok(output(serde_json::json!({"name":args[1],"status":status}).to_string()))
            }
            "--silo-storage-repair-protocol" => self
                .probe
                .map(|text| output(text.into()))
                .ok_or_else(|| failed(2)),
            "doctor" => {
                let fake = computers.get_mut(&args[2]).unwrap();
                if let Some(left) = fake.starts_after_doctors.as_mut() {
                    if *left == 0 {
                        fake.running = true;
                    } else {
                        *left -= 1;
                    }
                }
                let status = if self.running_blocks_others && any_running && !fake.running {
                    "recovery_required"
                } else {
                    fake.status
                };
                let mut item = serde_json::json!({"name":args[2],"status":status,"running":fake.running});
                match fake.running_field {
                    RunningField::Bool => {}
                    RunningField::Missing => {
                        item.as_object_mut().unwrap().remove("running");
                    }
                    RunningField::NotBoolean => item["running"] = "false".into(),
                }
                let report = serde_json::json!({"sandboxes":[item]}).to_string();
                // The real `doctor` prints the report and exits non-zero unless ready.
                match status {
                    "ready" => Ok(output(report)),
                    "unavailable" => Err(failed_with(3, report)),
                    _ => Err(failed_with(2, report)),
                }
            }
            "stop" => {
                let fake = computers.get_mut(&args[1]).unwrap();
                fake.running = fake.stays_running;
                Ok(output(String::new()))
            }
            "self" => {
                let name = &args[3];
                let fake = computers.get_mut(name).unwrap();
                if let Some(code) = fake.repair_exit {
                    return Err(failed(code));
                }
                if !fake.repair_is_a_no_op {
                    fake.status = "ready";
                }
                Ok(output(
                    serde_json::json!({"name":name,"ready":true,"originals_retained":true})
                        .to_string(),
                ))
            }
            other => panic!("unexpected runtime command {other}"),
        }
    }
}

fn paths() -> runtime::RuntimePaths {
    runtime::RuntimePaths {
        guest_image: PathBuf::new(),
        executable: PathBuf::new(),
        home: PathBuf::new(),
        storage_home: None,
        library: PathBuf::new(),
        metadata: PathBuf::new(),
        volumes: PathBuf::new(),
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| name.to_string()).collect()
}

/// Runs the upgrade, recording the backup into the same call log, and returns the report.
fn run(
    fake: &Runtime,
    list: &[&str],
    confirm_maintenance: bool,
    backup: Result<(), &'static str>,
) -> Result<Report, String> {
    let record = |_: &[String]| {
        fake.calls.borrow_mut().push("<backup>".into());
        backup.map_err(String::from)
    };
    upgrade(
        fake,
        &paths(),
        &names(list),
        &Options {
            confirm_maintenance,
            backup: &record,
            progress: &|_| {},
        },
    )
}

/// Paths whose runtime home has one sandbox folder per listed computer; `true` marks the
/// protected layout as ready in it, as the runtime writes it.
fn paths_with_sandboxes(directory: &Path, sandboxes: &[(&str, bool)]) -> runtime::RuntimePaths {
    let storage_home = directory.join("microsandbox");
    for (name, ready) in sandboxes {
        let sandbox = storage_home.join("sandboxes").join(name);
        fs::create_dir_all(&sandbox).unwrap();
        if *ready {
            fs::write(
                sandbox.join("state-layout.json"),
                br#"{"schema":"silo.sandbox-layout.v1","phase":"ready","binding":null,"root":null}"#,
            )
            .unwrap();
        }
    }
    runtime::RuntimePaths {
        home: storage_home.clone(),
        storage_home: Some(storage_home),
        metadata: directory.join("computers.json"),
        ..paths()
    }
}

fn run_in(
    fake: &Runtime,
    paths: &runtime::RuntimePaths,
    list: &[&str],
) -> Result<Report, String> {
    upgrade(
        fake,
        paths,
        &names(list),
        &Options {
            confirm_maintenance: true,
            backup: &|_| Ok(()),
            progress: &|_| {},
        },
    )
}

fn outcome<'a>(report: &'a Report, name: &str) -> &'a Outcome {
    &report
        .computers
        .iter()
        .find(|computer| computer.name == name)
        .unwrap()
        .outcome
}

#[test]
fn stopped_legacy_computers_are_backed_up_once_then_repaired_and_verified() {
    let fake = Runtime::new(&[
        ("a", computer("maintenance_required", false)),
        ("b", computer("eligible", false)),
    ]);
    let report = run(&fake, &["a", "b"], true, Ok(())).unwrap();
    assert!(report.complete());
    assert_eq!(outcome(&report, "a"), &Outcome::Repaired);
    assert_eq!(outcome(&report, "b"), &Outcome::Repaired);
    let calls = fake.calls();
    let backup = calls.iter().position(|call| call == "<backup>").unwrap();
    assert_eq!(calls.iter().filter(|call| *call == "<backup>").count(), 1);
    let first_repair = calls
        .iter()
        .position(|call| call.starts_with("self repair"))
        .unwrap();
    assert!(backup < first_repair, "the backup precedes every repair");
    assert!(calls.contains(
        &"self repair --sandbox a --format json --legacy-writers-stopped".to_string()
    ));
    assert_eq!(
        calls.last().unwrap(),
        "doctor --sandbox b --format json",
        "each repair is followed by a check that the storage is ready"
    );
}

#[test]
fn the_maintenance_confirmation_is_sent_only_where_the_platform_asks_for_it() {
    let fake = Runtime::new(&[("a", computer("eligible", false))]);
    run(&fake, &["a"], false, Ok(())).unwrap();
    assert!(fake.calls().contains(&"self repair --sandbox a --format json".to_string()));
}

#[test]
fn ready_computers_are_left_alone_and_cost_no_backup() {
    let fake = Runtime::new(&[("a", computer("ready", false))]);
    let report = run(&fake, &["a"], true, Ok(())).unwrap();
    assert_eq!(outcome(&report, "a"), &Outcome::AlreadyReady);
    assert!(report.complete());
    assert_eq!(
        fake.calls(),
        ["inspect a --format json", "doctor --sandbox a --format json"]
    );
}

#[test]
fn a_running_computer_is_stopped_before_the_backup_and_the_repair() {
    let fake = Runtime::new(&[("a", computer("maintenance_required", true))]);
    let report = run(&fake, &["a"], true, Ok(())).unwrap();
    assert_eq!(outcome(&report, "a"), &Outcome::Repaired);
    let calls = fake.calls();
    let position = |prefix: &str| calls.iter().position(|call| call.starts_with(prefix)).unwrap();
    assert!(position("stop a") < position("<backup>"));
    assert!(position("<backup>") < position("self repair"));
}

#[test]
fn one_launch_finishes_when_a_running_computer_makes_doctor_rate_the_others_recovery_required() {
    let fake = Runtime::new(&[
        ("run", computer("maintenance_required", true)),
        ("other", computer("eligible", false)),
        ("ready", computer("ready", false)),
    ])
    .macos_doctor();
    let report = run(&fake, &["other", "run", "ready"], true, Ok(())).unwrap();
    assert!(report.complete(), "{:?}", report.computers);
    assert_eq!(outcome(&report, "run"), &Outcome::Repaired);
    assert_eq!(outcome(&report, "other"), &Outcome::Repaired);
    assert_eq!(outcome(&report, "ready"), &Outcome::AlreadyReady);
    let calls = fake.calls();
    let stop = calls.iter().position(|call| call == "stop run").unwrap();
    let first_doctor = calls.iter().position(|call| call.starts_with("doctor")).unwrap();
    assert!(stop < first_doctor, "everything running is stopped before any doctor call");
}

#[test]
fn a_computer_that_will_not_stop_is_kept_untouched_and_the_others_continue() {
    let mut stuck = computer("maintenance_required", true);
    stuck.stays_running = true;
    let fake = Runtime::new(&[("stuck", stuck), ("b", computer("eligible", false))]);
    let report = run(&fake, &["stuck", "b"], true, Ok(())).unwrap();
    assert!(matches!(outcome(&report, "stuck"), Outcome::NeedsAttention(_)));
    assert_eq!(outcome(&report, "b"), &Outcome::Repaired);
    assert!(!report.complete());
    assert!(!fake.calls().iter().any(|call| call.contains("--sandbox stuck --format json --")));
}

#[test]
fn disks_that_need_a_manual_decision_are_reported_and_never_repaired() {
    for status in ["recovery_required", "unavailable"] {
        let fake = Runtime::new(&[
            ("odd", computer(status, false)),
            ("b", computer("eligible", false)),
        ]);
        let report = run(&fake, &["odd", "b"], true, Ok(())).unwrap();
        assert!(matches!(outcome(&report, "odd"), Outcome::NeedsAttention(_)), "{status}");
        assert_eq!(outcome(&report, "b"), &Outcome::Repaired);
        assert!(!fake.calls().iter().any(|call| call.contains("repair --sandbox odd")));
    }
}

#[test]
fn a_refused_repair_is_reported_for_that_computer_only() {
    let mut refused = computer("maintenance_required", false);
    refused.repair_exit = Some(2);
    let fake = Runtime::new(&[("refused", refused), ("b", computer("eligible", false))]);
    let report = run(&fake, &["refused", "b"], true, Ok(())).unwrap();
    assert!(matches!(outcome(&report, "refused"), Outcome::NeedsAttention(_)));
    assert_eq!(outcome(&report, "b"), &Outcome::Repaired);
    assert!(!report.complete());
}

#[test]
fn a_repair_whose_result_cannot_be_verified_does_not_count() {
    let mut liar = computer("eligible", false);
    liar.repair_is_a_no_op = true;
    let fake = Runtime::new(&[("liar", liar)]);
    let report = run(&fake, &["liar"], true, Ok(())).unwrap();
    assert!(matches!(outcome(&report, "liar"), Outcome::NeedsAttention(_)));
}

#[test]
fn a_failed_backup_blocks_only_the_computers_that_needed_repair_and_says_why() {
    let fake = Runtime::new(&[
        ("a", computer("eligible", false)),
        ("ok", computer("ready", false)),
    ]);
    let report = run(&fake, &["a", "ok"], true, Err("there is not enough free disk space")).unwrap();
    assert_eq!(outcome(&report, "ok"), &Outcome::AlreadyReady);
    let Outcome::NeedsAttention(message) = outcome(&report, "a") else { panic!() };
    assert!(message.contains("not enough free disk space"), "{message}");
    assert!(!message.starts_with("Relaunch"), "{message}");
    assert!(report.backed_up.is_empty());
    assert!(!fake.calls().iter().any(|call| call.starts_with("self repair")));
}

#[test]
fn a_missing_or_unreadable_running_flag_is_never_taken_for_stopped() {
    for field in [RunningField::Missing, RunningField::NotBoolean] {
        let mut unsure = computer("maintenance_required", false);
        unsure.running_field = field;
        let fake = Runtime::new(&[("unsure", unsure), ("b", computer("eligible", false))]);
        let report = run(&fake, &["unsure", "b"], true, Ok(())).unwrap();
        let Outcome::NeedsAttention(message) = outcome(&report, "unsure") else { panic!() };
        assert!(message.contains("whether this computer is running"), "{message}");
        assert_eq!(outcome(&report, "b"), &Outcome::Repaired);
        assert!(!fake.calls().iter().any(|call| call.contains("repair --sandbox unsure")));
    }
}

#[test]
fn a_computer_that_runs_again_before_its_repair_is_not_repaired() {
    let mut restarted = computer("eligible", false);
    // Inspect, then the check right before the repair.
    restarted.starts_after_doctors = Some(1);
    let fake = Runtime::new(&[("late", restarted), ("b", computer("eligible", false))]);
    let report = run(&fake, &["late", "b"], true, Ok(())).unwrap();
    assert!(matches!(outcome(&report, "late"), Outcome::NeedsAttention(_)));
    assert_eq!(outcome(&report, "b"), &Outcome::Repaired);
    assert!(!fake.calls().iter().any(|call| call.contains("repair --sandbox late")));
}

#[test]
fn the_report_records_which_computers_the_backup_covers() {
    let fake = Runtime::new(&[
        ("a", computer("eligible", false)),
        ("stuck", computer("recovery_required", false)),
        ("ok", computer("ready", false)),
    ]);
    let seen = RefCell::new(Vec::new());
    let backup = |covered: &[String]| {
        seen.borrow_mut().extend(covered.iter().cloned());
        Ok(())
    };
    let report = upgrade(
        &fake,
        &paths(),
        &names(&["a", "stuck", "ok"]),
        &Options { confirm_maintenance: true, backup: &backup, progress: &|_| {} },
    )
    .unwrap();
    assert_eq!(report.backed_up, ["a"]);
    assert_eq!(*seen.borrow(), ["a"]);
}

fn storage_with_a_disk(directory: &Path) -> PathBuf {
    let storage = directory.join("runtime");
    fs::create_dir_all(storage.join("microsandbox/sandboxes/dev")).unwrap();
    fs::write(storage.join("microsandbox/sandboxes/dev/upper.ext4"), b"disk").unwrap();
    fs::write(storage.join("computers.json"), b"{}").unwrap();
    storage
}

#[test]
fn the_backup_is_a_complete_clone_made_once() {
    let directory = tempfile::tempdir().unwrap();
    let storage = storage_with_a_disk(directory.path());
    let covered = names(&["dev"]);
    clone_backup(directory.path(), &storage, &covered).unwrap();
    let backup = directory.path().join(BACKUP);
    assert_eq!(
        fs::read(backup.join("microsandbox/sandboxes/dev/upper.ext4")).unwrap(),
        b"disk"
    );
    // A later run never replaces a finished backup that covers the same computers.
    fs::write(storage.join("microsandbox/sandboxes/dev/upper.ext4"), b"changed").unwrap();
    clone_backup(directory.path(), &storage, &covered).unwrap();
    assert_eq!(
        fs::read(backup.join("microsandbox/sandboxes/dev/upper.ext4")).unwrap(),
        b"disk"
    );
}

#[test]
fn an_unfinished_backup_is_discarded_not_trusted() {
    let directory = tempfile::tempdir().unwrap();
    let storage = storage_with_a_disk(directory.path());
    let partial = directory.path().join(format!("{BACKUP}.partial"));
    fs::create_dir_all(&partial).unwrap();
    fs::write(partial.join("half"), b"x").unwrap();
    clone_backup(directory.path(), &storage, &names(&["dev"])).unwrap();
    assert!(!partial.exists());
    assert!(directory.path().join(BACKUP).join("computers.json").is_file());
    assert!(!directory.path().join(BACKUP).join("half").exists());
}

#[test]
fn a_backup_that_misses_a_computer_is_replaced_by_one_taken_while_it_is_stopped() {
    let directory = tempfile::tempdir().unwrap();
    let storage = storage_with_a_disk(directory.path());
    let disk = "microsandbox/sandboxes/dev/upper.ext4";
    clone_backup(directory.path(), &storage, &names(&["dev"])).unwrap();
    fs::write(storage.join(disk), b"stopped later").unwrap();
    // "late" was running when the first backup was taken, so that backup cannot serve it.
    clone_backup(directory.path(), &storage, &names(&["dev", "late"])).unwrap();
    let backup = directory.path().join(BACKUP);
    assert_eq!(fs::read(backup.join(disk)).unwrap(), b"stopped later");
    clone_backup(directory.path(), &storage, &names(&["late"])).unwrap();
    assert_eq!(fs::read(backup.join(disk)).unwrap(), b"stopped later");
    assert!(!directory.path().join(format!("{BACKUP}.partial")).exists());
}

#[cfg(unix)]
#[test]
fn the_backup_keeps_absolute_links_as_they_are_and_names_a_failure() {
    let directory = tempfile::tempdir().unwrap();
    let storage = storage_with_a_disk(directory.path());
    std::os::unix::fs::symlink("/nonexistent", storage.join("absolute")).unwrap();
    clone_backup(directory.path(), &storage, &names(&["dev"])).unwrap();
    let link = directory.path().join(BACKUP).join("absolute");
    assert_eq!(fs::read_link(link).unwrap(), Path::new("/nonexistent"));

    let other = tempfile::tempdir().unwrap();
    let storage = storage_with_a_disk(other.path());
    assert!(std::process::Command::new("mkfifo").arg(storage.join("pipe")).status().unwrap().success());
    let error = clone_backup(other.path(), &storage, &names(&["dev"])).unwrap_err();
    assert!(error.contains("unsupported filesystem entry"), "{error}");
    assert!(!other.path().join(BACKUP).exists());
    assert!(!other.path().join(format!("{BACKUP}.partial")).exists());
}

#[test]
fn a_fresh_backup_is_not_deleted_by_a_check_that_came_due_before_it() {
    let directory = tempfile::tempdir().unwrap();
    let storage = storage_with_a_disk(directory.path());
    clone_backup(directory.path(), &storage, &names(&["dev"])).unwrap();
    let backup = directory.path().join(BACKUP);
    // Hold the deletion lock as a due check does: the swap waits for it instead of racing it.
    let swapped = std::thread::scope(|scope| {
        let guard = crate::pre_upgrade_backup::hold_deletion_for_test();
        let handle = scope.spawn(|| clone_backup(directory.path(), &storage, &names(&["dev", "x"])));
        std::thread::sleep(Duration::from_millis(200));
        assert!(!handle.is_finished(), "the swap waits for the deletion lock");
        assert_eq!(fs::read(backup.join("computers.json")).unwrap(), b"{}");
        drop(guard);
        handle.join().unwrap()
    });
    swapped.unwrap();
    assert!(backup.join("computers.json").is_file());
}

#[test]
fn the_report_round_trips_and_a_damaged_one_is_not_replaced() {
    let directory = tempfile::tempdir().unwrap();
    assert_eq!(read_report(directory.path()).unwrap(), None);
    let fake = Runtime::new(&[
        ("a", computer("eligible", false)),
        ("odd", computer("recovery_required", false)),
    ]);
    let report = run(&fake, &["a", "odd"], true, Ok(())).unwrap();
    write_report(directory.path(), &report).unwrap();
    assert_eq!(read_report(directory.path()).unwrap(), Some(report));
    fs::write(directory.path().join(MARKER), b"{").unwrap();
    assert!(read_report(directory.path()).is_err());
}

#[test]
fn a_runtime_without_the_repair_protocol_moves_nothing_and_takes_no_backup() {
    for probe in [None, Some("0\n"), Some("2\n")] {
        let fake = Runtime::new(&[
            ("a", computer("maintenance_required", false)),
            ("ok", computer("ready", false)),
        ])
        .without_repair_protocol(probe);
        let report = run(&fake, &["a", "ok"], true, Ok(())).unwrap();
        assert!(matches!(outcome(&report, "a"), Outcome::NeedsAttention(_)));
        assert_eq!(outcome(&report, "ok"), &Outcome::AlreadyReady);
        let calls = fake.calls();
        assert!(!calls.iter().any(|call| call.starts_with("self repair")));
        assert!(!calls.contains(&"<backup>".to_string()));
    }
}

#[test]
fn the_repair_protocol_is_not_probed_when_nothing_needs_repair() {
    let fake = Runtime::new(&[("ok", computer("ready", false))]);
    run(&fake, &["ok"], true, Ok(())).unwrap();
    assert!(!fake
        .calls()
        .iter()
        .any(|call| call.starts_with("--silo-")));
}

// MARK: Launch driver

use super::{
    fresh, protected_computers, select_generation, write, Controller, CONVERTED, FILE,
};

/// An app-data folder after a finished migration whose converted storage holds `names`.
fn launched(names: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let converted = dir.path().join(CONVERTED);
    fs::create_dir_all(converted.join("microsandbox")).unwrap();
    fs::write(converted.join("microsandbox/db"), b"catalogue").unwrap();
    for name in names {
        fs::create_dir_all(converted.join("microsandbox/sandboxes").join(name)).unwrap();
    }
    let computers: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            serde_json::json!({"id":format!("00000000-0000-4000-8000-00000000000{index}"),"name":name,"cpus":1,"maxCPUs":1,"memoryGiB":1,"maxMemoryGiB":1,"workspaceStorageGiB":1,"runtimeStorageGiB":1})
        })
        .collect();
    runtime::write_metadata(
        &converted.join("computers.json"),
        &serde_json::from_value(serde_json::json!({"schemaVersion":1,"computers":computers}))
            .unwrap(),
    )
    .unwrap();
    select_generation(dir.path(), CONVERTED).unwrap();
    write(&dir.path().join(FILE), &fresh("complete", names.len())).unwrap();
    dir
}

#[test]
fn the_launch_driver_holds_the_computers_moves_them_and_opens_them_again() {
    let dir = launched(&["drv-a", "drv-b"]);
    let controller = Controller::new(dir.path().to_path_buf());
    assert!(controller.hold_for_protected_storage());
    assert_eq!(controller.snapshot().unwrap().status, "running");
    assert!(controller.blocks_operations());

    let fake = Runtime::new(&[
        ("drv-a", computer("maintenance_required", false)),
        ("drv-b", computer("ready", false)),
    ]);
    let stages = RefCell::new(Vec::new());
    controller
        .run_protected_storage(&fake, &paths(), &|| {
            stages.borrow_mut().push(controller.snapshot().unwrap().stage)
        })
        .unwrap();

    let stages = stages.into_inner();
    assert!(stages.contains(&"Backing up computer storage".to_string()), "{stages:?}");
    assert!(stages.contains(&"Upgrading computer storage (1 of 1)".to_string()), "{stages:?}");
    assert_eq!(controller.snapshot().unwrap().status, "complete");
    assert!(!controller.blocks_operations());
    assert!(dir.path().join(BACKUP).join("microsandbox/db").is_file());
    let report = read_report(&dir.path().join(CONVERTED)).unwrap().unwrap();
    assert!(report.complete());
    assert_eq!(outcome(&report, "drv-a"), &Outcome::Repaired);
    assert_eq!(outcome(&report, "drv-b"), &Outcome::AlreadyReady);
    assert_eq!(attention_message(&dir.path().join(CONVERTED).join("computers.json"), "drv-a"), None);
    // Nothing left to do at the next launch.
    assert!(!Controller::new(dir.path().to_path_buf()).hold_for_protected_storage());
}

#[test]
fn a_computer_that_cannot_be_moved_is_refused_with_its_reason_and_retried_at_the_next_launch() {
    let dir = launched(&["drv-odd", "drv-ok"]);
    let controller = Controller::new(dir.path().to_path_buf());
    assert!(controller.hold_for_protected_storage());
    let fake = Runtime::new(&[
        ("drv-odd", computer("recovery_required", false)),
        ("drv-ok", computer("eligible", false)),
    ]);
    controller.run_protected_storage(&fake, &paths(), &|| {}).unwrap();

    assert!(!controller.blocks_operations(), "the other computers work");
    let message = attention_message(&dir.path().join(CONVERTED).join("computers.json"), "drv-odd").unwrap();
    assert!(message.contains("kept untouched"), "{message}");
    assert_eq!(attention_message(&dir.path().join(CONVERTED).join("computers.json"), "drv-ok"), None);
    let report = read_report(&dir.path().join(CONVERTED)).unwrap().unwrap();
    assert!(!report.complete());
    assert!(Controller::new(dir.path().to_path_buf()).hold_for_protected_storage());
}

#[test]
fn a_failed_backup_repairs_nothing_and_every_pending_computer_says_so() {
    let dir = launched(&["drv-x"]);
    // An entry the clone refuses to copy.
    #[cfg(unix)]
    assert!(std::process::Command::new("mkfifo").arg(dir.path().join(CONVERTED).join("pipe")).status().unwrap().success());
    let controller = Controller::new(dir.path().to_path_buf());
    assert!(controller.hold_for_protected_storage());
    let fake = Runtime::new(&[("drv-x", computer("maintenance_required", false))]);
    controller.run_protected_storage(&fake, &paths(), &|| {}).unwrap();
    assert!(!fake.calls().iter().any(|call| call.starts_with("self repair")));
    let message = attention_message(&dir.path().join(CONVERTED).join("computers.json"), "drv-x").unwrap();
    assert!(message.contains("could not back up") && message.contains("unsupported filesystem entry"), "{message}");
    assert!(!controller.blocks_operations());
    assert!(!dir.path().join(BACKUP).exists());
    let report = read_report(&dir.path().join(CONVERTED)).unwrap().unwrap();
    assert!(!report.complete(), "retried at the next launch");
}

#[test]
fn a_computer_that_arrives_after_the_move_is_checked_and_flagged_until_it_is_ready() {
    let dir = launched(&["drv-a"]);
    let metadata = dir.path().join(CONVERTED).join("computers.json");
    let controller = Controller::new(dir.path().to_path_buf());
    assert!(controller.hold_for_protected_storage());
    let fake = Runtime::new(&[("drv-a", computer("ready", false))]);
    controller.run_protected_storage(&fake, &paths(), &|| {}).unwrap();
    assert!(read_report(&dir.path().join(CONVERTED)).unwrap().unwrap().complete());

    let storage_home = dir.path().join(CONVERTED).join("microsandbox");
    for name in ["new-ready", "new-old", "new-odd"] {
        fs::create_dir_all(storage_home.join("sandboxes").join(name)).unwrap();
    }
    let arrival_paths = runtime::RuntimePaths {
        metadata: metadata.clone(),
        storage_home: Some(storage_home),
        ..paths()
    };
    let fake = Runtime::new(&[
        ("new-ready", computer("ready", false)),
        ("new-old", computer("maintenance_required", false)),
        ("new-odd", computer("recovery_required", false)),
    ]);
    check_arrival(&fake, &arrival_paths, "new-ready");
    assert_eq!(attention_message(&metadata, "new-ready"), None);
    assert!(read_report(&dir.path().join(CONVERTED)).unwrap().unwrap().complete());

    check_arrival(&fake, &arrival_paths, "new-old");
    check_arrival(&fake, &arrival_paths, "new-odd");
    assert!(attention_message(&metadata, "new-old").unwrap().contains("Relaunch Silo"));
    assert!(attention_message(&metadata, "new-odd").unwrap().contains("manual decision"));
    assert_eq!(attention_message(&metadata, "drv-a"), None);
    assert!(!fake.calls().iter().any(|call| call.starts_with("self repair")));
    let report = read_report(&dir.path().join(CONVERTED)).unwrap().unwrap();
    assert!(!report.complete(), "the next launch runs the step again");
    assert_eq!(outcome(&report, "drv-a"), &Outcome::AlreadyReady);
}

#[test]
fn a_conversion_that_finishes_closes_the_computers_until_the_move_has_run() {
    use crate::runtime_migration::{drive_pending, LaunchDrive};
    let dir = launched(&["drv-a"]);
    let controller = Controller {
        app_data: dir.path().to_path_buf(),
        path: dir.path().join(FILE),
        state: std::sync::Mutex::new(fresh("scanning", 1)),
        writable: true,
    };
    let context = crate::context::test_support::RecordingContext::default();
    let result = drive_pending(&context, &controller, |_| {
        controller.state.lock().unwrap().status = "complete".into();
        Ok(())
    });
    assert_eq!(result, Ok(LaunchDrive::Converted));
    assert_eq!(controller.snapshot().unwrap().status, "running");
    assert!(controller.blocks_operations());
    controller.release_protected_storage();
}

#[test]
fn nothing_is_held_without_a_finished_migration_computers_or_work() {
    // No computers.
    let empty = launched(&[]);
    assert!(!Controller::new(empty.path().to_path_buf()).hold_for_protected_storage());
    // A conversion that has not finished already holds everything back.
    let scanning = launched(&["drv-s"]);
    write(&scanning.path().join(FILE), &fresh("scanning", 1)).unwrap();
    let controller = Controller::new(scanning.path().to_path_buf());
    assert!(!controller.hold_for_protected_storage());
    // Running without a hold does nothing.
    let fake = Runtime::new(&[]);
    controller.run_protected_storage(&fake, &paths(), &|| {}).unwrap();
    assert!(fake.calls().is_empty());
}

#[test]
fn containers_are_not_part_of_the_move() {
    let inventory = serde_json::from_value(serde_json::json!({"schemaVersion":1,"computers":[
        {"id":"00000000-0000-4000-8000-000000000001","name":"vm","cpus":1,"maxCPUs":1,"memoryGiB":1,"maxMemoryGiB":1,"workspaceStorageGiB":1,"runtimeStorageGiB":1},
        {"id":"00000000-0000-4000-8000-000000000002","name":"box","runtime":"container","cpus":1,"maxCPUs":1,"memoryGiB":1,"maxMemoryGiB":1,"workspaceStorageGiB":1,"runtimeStorageGiB":1}
    ]})).unwrap();
    assert_eq!(protected_computers(&inventory), ["vm"]);
}

#[test]
fn a_blocked_repair_reads_its_diagnosis_from_stdout() {
    assert!(blocked_message_for_test(&blocked_detail("recovery_required")).contains("manual decision"));
    assert!(blocked_message_for_test(&blocked_detail("running")).contains("stopping"));
    assert!(blocked_message_for_test("not json").contains("refused"));
}

fn blocked_detail(status: &str) -> String {
    format!(r#"{{"sandbox":{{"status":"{status}"}},"next_step":"x"}}"#)
}

#[test]
fn a_running_computer_with_protected_storage_is_never_stopped_beside_one_that_needs_attention() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths_with_sandboxes(directory.path(), &[("healthy", true), ("odd", false)]);
    let fake = Runtime::new(&[
        ("healthy", computer("ready", true)),
        ("odd", computer("recovery_required", false)),
    ])
    .macos_doctor();
    for launch in 0..2 {
        let report = run_in(&fake, &paths, &["healthy", "odd"]).unwrap();
        assert!(!report.complete(), "launch {launch}");
        assert_eq!(outcome(&report, "healthy"), &Outcome::AlreadyReady);
        assert!(matches!(outcome(&report, "odd"), Outcome::NeedsAttention(_)));
        assert!(
            !fake.calls().iter().any(|call| call.starts_with("stop healthy")),
            "launch {launch}: {:?}",
            fake.calls()
        );
        assert!(fake.computers.borrow()["healthy"].running, "launch {launch}");
    }
}

#[test]
fn only_a_running_computer_with_legacy_storage_is_stopped() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths_with_sandboxes(directory.path(), &[("healthy", true), ("old", false)]);
    let fake = Runtime::new(&[
        ("healthy", computer("ready", true)),
        ("old", computer("maintenance_required", true)),
    ])
    .macos_doctor();
    let report = run_in(&fake, &paths, &["healthy", "old"]).unwrap();
    assert!(report.complete(), "{:?}", report.computers);
    assert_eq!(outcome(&report, "old"), &Outcome::Repaired);
    assert!(fake.calls().iter().any(|call| call == "stop old"));
    assert!(!fake.calls().iter().any(|call| call == "stop healthy"));
}

#[test]
fn a_ready_record_that_doctor_contradicts_does_not_keep_a_running_computer_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths_with_sandboxes(directory.path(), &[("odd", true)]);
    let fake = Runtime::new(&[("odd", computer("maintenance_required", true))]);
    let report = run_in(&fake, &paths, &["odd"]).unwrap();
    assert_eq!(outcome(&report, "odd"), &Outcome::Repaired);
    assert!(fake.calls().iter().any(|call| call == "stop odd"));
}

/// Adds computers to the saved inventory of a `launched` folder, without runtime folders.
fn add_saved_computers(dir: &Path, names: &[&str]) {
    let metadata = dir.join(CONVERTED).join("computers.json");
    let mut inventory = serde_json::to_value(runtime::read_metadata(&metadata).unwrap()).unwrap();
    for (index, name) in names.iter().enumerate() {
        inventory["computers"].as_array_mut().unwrap().push(serde_json::json!({
            "id": format!("00000000-0000-4000-8000-0000000001{index:02}"), "name": name,
            "cpus": 1, "maxCPUs": 1, "memoryGiB": 1, "maxMemoryGiB": 1,
            "workspaceStorageGiB": 1, "runtimeStorageGiB": 1
        }));
    }
    runtime::write_metadata(&metadata, &serde_json::from_value(inventory).unwrap()).unwrap();
}

#[test]
fn computers_that_were_never_started_are_neither_inspected_nor_flagged_across_launches() {
    let dir = launched(&["drv-a"]);
    // A restore waiting for its first Start and an unstarted checkpoint fork: saved, no folder.
    add_saved_computers(dir.path(), &["pending-import", "unstarted-fork"]);
    let metadata = dir.path().join(CONVERTED).join("computers.json");
    for launch in 0..2 {
        let controller = Controller::new(dir.path().to_path_buf());
        // Only the existing computer is known to the fake runtime: asking about another panics.
        let fake = Runtime::new(&[("drv-a", computer("maintenance_required", false))]);
        let held = controller.hold_for_protected_storage();
        assert_eq!(held, launch == 0, "launch {launch}");
        if held {
            controller.run_protected_storage(&fake, &paths(), &|| {}).unwrap();
        }
        controller.release_protected_storage();
        for name in ["pending-import", "unstarted-fork"] {
            assert_eq!(attention_message(&metadata, name), None, "{name}, launch {launch}");
        }
        let report = read_report(&dir.path().join(CONVERTED)).unwrap().unwrap();
        assert!(report.complete(), "launch {launch}: {:?}", report.computers);
        assert!(report.computers.iter().all(|computer| computer.name == "drv-a"));
    }
}

#[test]
fn a_saved_computer_without_a_runtime_folder_alone_leaves_nothing_to_hold() {
    let dir = launched(&[]);
    add_saved_computers(dir.path(), &["fork"]);
    assert!(!Controller::new(dir.path().to_path_buf()).hold_for_protected_storage());
}

#[test]
fn a_computer_without_a_runtime_folder_is_not_checked_when_it_arrives() {
    let dir = launched(&["drv-a"]);
    let metadata = dir.path().join(CONVERTED).join("computers.json");
    let arrival_paths = runtime::RuntimePaths {
        metadata: metadata.clone(),
        storage_home: Some(dir.path().join(CONVERTED).join("microsandbox")),
        ..paths()
    };
    let fake = Runtime::new(&[]);
    check_arrival(&fake, &arrival_paths, "restore-waiting-for-start");
    assert!(fake.calls().is_empty());
    assert_eq!(attention_message(&metadata, "restore-waiting-for-start"), None);
    assert!(read_report(&dir.path().join(CONVERTED)).unwrap().is_none());
}
