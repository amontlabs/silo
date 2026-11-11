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
    /// `repair` fails with this exit code instead of repairing.
    repair_exit: Option<i32>,
    /// `repair` reports success but leaves the status unchanged.
    repair_is_a_no_op: bool,
    /// `stop` succeeds but the computer keeps running.
    stays_running: bool,
}

fn computer(status: &'static str, running: bool) -> Fake {
    Fake {
        status,
        running,
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
        }
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
        match args[0].as_str() {
            "--silo-storage-repair-protocol" => self
                .probe
                .map(|text| output(text.into()))
                .ok_or_else(|| failed(2)),
            "doctor" => {
                let fake = &computers[&args[2]];
                let report = serde_json::json!({"sandboxes":[{"name":args[2],"status":fake.status,"running":fake.running}]})
                    .to_string();
                // The real `doctor` prints the report and exits non-zero unless ready.
                match fake.status {
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
    let record = || {
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
    assert_eq!(fake.calls(), ["doctor --sandbox a --format json"]);
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
fn a_failed_backup_ends_the_upgrade_before_any_repair() {
    let fake = Runtime::new(&[("a", computer("eligible", false))]);
    let error = run(&fake, &["a"], true, Err("no space")).unwrap_err();
    assert_eq!(error, "no space");
    assert!(!fake.calls().iter().any(|call| call.starts_with("self repair")));
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
    clone_backup(directory.path(), &storage).unwrap();
    let backup = directory.path().join(BACKUP);
    assert_eq!(
        fs::read(backup.join("microsandbox/sandboxes/dev/upper.ext4")).unwrap(),
        b"disk"
    );
    // A later run never replaces a finished backup with the storage as it is then.
    fs::write(storage.join("microsandbox/sandboxes/dev/upper.ext4"), b"changed").unwrap();
    clone_backup(directory.path(), &storage).unwrap();
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
    clone_backup(directory.path(), &storage).unwrap();
    assert!(!partial.exists());
    assert!(directory.path().join(BACKUP).join("computers.json").is_file());
    assert!(!directory.path().join(BACKUP).join("half").exists());
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
    // A link the clone refuses to copy.
    #[cfg(unix)]
    std::os::unix::fs::symlink("/nonexistent", dir.path().join(CONVERTED).join("absolute")).unwrap();
    let controller = Controller::new(dir.path().to_path_buf());
    assert!(controller.hold_for_protected_storage());
    let fake = Runtime::new(&[("drv-x", computer("maintenance_required", false))]);
    let error = controller
        .run_protected_storage(&fake, &paths(), &|| {})
        .unwrap_err();
    assert!(error.contains("Relaunch Silo"), "{error}");
    assert!(!fake.calls().iter().any(|call| call.starts_with("self repair")));
    assert!(attention_message(&dir.path().join(CONVERTED).join("computers.json"), "drv-x").unwrap().contains("Relaunch Silo"));
    assert!(!controller.blocks_operations());
    assert!(!dir.path().join(BACKUP).exists());
    assert!(!dir.path().join(CONVERTED).join(MARKER).exists());
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
