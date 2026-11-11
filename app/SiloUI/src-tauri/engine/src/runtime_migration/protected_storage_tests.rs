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
        }
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
