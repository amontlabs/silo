//! Moves computers created by MicroSandbox 0.7.6 to the protected storage layout of 0.7.12 and
//! later. The runtime refuses to start an older computer until `msb self repair` has imported its
//! disks, and on macOS that command needs a maintenance confirmation that only the supervising
//! app can give. See `docs/direction/MSB-0.7.15-MIGRATION.md`.
//!
//! The caller holds the migration gate: no other runtime command runs, no previous Silo build
//! can start, and the computers of this storage are Silo's own. Computers that cannot be moved
//! are reported, never removed; the original disk files are always retained.
use super::{copy_runtime_tree, runtime};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};

pub(super) const MARKER: &str = "protected-storage.json";
/// A clone of the storage folder taken after the computers stopped and before the first repair.
pub(crate) const BACKUP: &str = "runtime-backup-before-protected-storage";
/// Hidden option of the bundled `msb self repair`: the caller asserts that every legacy writer is
/// stopped, as the macOS maintenance prompt asks. It is `StorageRecoveryOptions::legacy_writers_stopped`.
const MAINTENANCE_FLAG: &str = "--legacy-writers-stopped";
/// Answers `1` when the bundled `msb self repair` accepts [`MAINTENANCE_FLAG`] with
/// `--format json`. Probed like the other `--silo-*-protocol` options before it is relied on.
const REPAIR_PROBE: &str = "--silo-storage-repair-protocol";
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const VERSION: u32 = 1;
const DIAGNOSE_TIMEOUT: Duration = Duration::from_secs(60);
const STOP_TIMEOUT: Duration = Duration::from_secs(120);
const REPAIR_TIMEOUT: Duration = Duration::from_secs(60 * 30);

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "state", content = "message", rename_all = "kebab-case")]
pub(super) enum Outcome {
    /// Already in the protected layout; nothing was changed.
    AlreadyReady,
    /// Imported and verified; the original disk files are retained.
    Repaired,
    /// Left exactly as it was. The text is shown to the owner.
    NeedsAttention(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(super) struct Computer {
    pub name: String,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Report {
    version: u32,
    pub computers: Vec<Computer>,
}

impl Report {
    /// Every computer can start with the new runtime.
    pub fn complete(&self) -> bool {
        !self
            .computers
            .iter()
            .any(|computer| matches!(computer.outcome, Outcome::NeedsAttention(_)))
    }
}

/// What the owner-facing progress shows.
pub(super) enum Step {
    Inspecting { index: usize, total: usize },
    Stopping { name: String },
    BackingUp,
    Repairing { index: usize, total: usize },
}

pub(super) struct Options<'a> {
    /// Pass the maintenance confirmation (macOS). The bundled runtime refuses to repair
    /// without it there.
    pub confirm_maintenance: bool,
    /// Clone the storage folder. Called once, after every computer is stopped and before the
    /// first repair; a failure ends the upgrade with nothing repaired.
    pub backup: &'a dyn Fn() -> Result<(), String>,
    pub progress: &'a dyn Fn(Step),
}

const UNSUPPORTED: &str = "This version of Silo cannot move this computer to the new storage. Its files were kept untouched. Update Silo, then relaunch it.";

fn repair_supported(runner: &dyn runtime::RuntimeRunner, paths: &runtime::RuntimePaths) -> bool {
    runner
        .run(paths, &[REPAIR_PROBE.into()], PROBE_TIMEOUT)
        .is_ok_and(|output| output.stdout.trim() == "1")
}

enum Diagnosis {
    Ready,
    Repairable { running: bool },
    Blocked(String),
}

fn diagnose(
    runner: &dyn runtime::RuntimeRunner,
    paths: &runtime::RuntimePaths,
    name: &str,
) -> Result<Diagnosis, String> {
    let args: Vec<String> = ["doctor", "--sandbox", name, "--format", "json"]
        .map(String::from)
        .into();
    // `doctor` exits 2 or 3 when the storage is not ready and still prints its report; the
    // runner hands that stdout back as the failure detail.
    let report = match runner.run(paths, &args, DIAGNOSE_TIMEOUT) {
        Ok(output) => output.stdout,
        Err(runtime::RuntimeError::Failed {
            exit_code: Some(2 | 3),
            detail,
            ..
        }) => detail,
        Err(_) => return Err("Silo could not inspect this computer's storage.".into()),
    };
    let value: serde_json::Value = serde_json::from_str(&report)
        .map_err(|_| "Silo could not read this computer's storage report.".to_string())?;
    let item = value
        .pointer("/sandboxes/0")
        .ok_or("The storage report has no entry for this computer.")?;
    let running = item
        .get("running")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    Ok(match item.get("status").and_then(serde_json::Value::as_str) {
        Some("ready") => Diagnosis::Ready,
        Some("eligible" | "maintenance_required" | "in_progress") => {
            Diagnosis::Repairable { running }
        }
        Some("recovery_required") => Diagnosis::Blocked(
            "This computer's disks need a manual decision that Silo cannot make for you. Its files were kept untouched. Report this problem before using the computer.".into(),
        ),
        _ => Diagnosis::Blocked(
            "Silo could not establish the state of this computer's disks. Its files were kept untouched. Relaunch Silo to try again.".into(),
        ),
    })
}

fn stop(
    runner: &dyn runtime::RuntimeRunner,
    paths: &runtime::RuntimePaths,
    name: &str,
) -> Result<(), String> {
    let args: Vec<String> = ["stop", name].map(String::from).into();
    runner
        .run(paths, &args, STOP_TIMEOUT)
        .map(|_| ())
        .map_err(|_| "Silo could not stop this computer for the storage upgrade.".to_string())
}

fn repair(
    runner: &dyn runtime::RuntimeRunner,
    paths: &runtime::RuntimePaths,
    name: &str,
    confirm_maintenance: bool,
) -> Result<(), String> {
    let mut args: Vec<String> = ["self", "repair", "--sandbox", name, "--format", "json"]
        .map(String::from)
        .into();
    if confirm_maintenance {
        args.push(MAINTENANCE_FLAG.into());
    }
    let output = runner
        .run(paths, &args, REPAIR_TIMEOUT)
        .map_err(|_| "The storage upgrade of this computer did not finish. Its original files were kept.".to_string())?;
    let result: serde_json::Value = serde_json::from_str(&output.stdout)
        .map_err(|_| "The storage upgrade returned an unreadable result.".to_string())?;
    if result.get("ready").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err("The storage upgrade of this computer did not report success. Its original files were kept.".into());
    }
    Ok(())
}

/// Upgrade every named computer. `Err` means nothing was repaired and the owner should retry;
/// per-computer problems are in the report.
pub(super) fn upgrade(
    runner: &dyn runtime::RuntimeRunner,
    paths: &runtime::RuntimePaths,
    names: &[String],
    options: &Options,
) -> Result<Report, String> {
    let total = names.len();
    let mut outcomes: Vec<Option<Outcome>> = vec![None; total];
    let mut pending = Vec::new();
    for (index, name) in names.iter().enumerate() {
        (options.progress)(Step::Inspecting { index, total });
        match diagnose(runner, paths, name) {
            Ok(Diagnosis::Ready) => outcomes[index] = Some(Outcome::AlreadyReady),
            Ok(Diagnosis::Repairable { running }) => pending.push((index, running)),
            Ok(Diagnosis::Blocked(message)) | Err(message) => {
                outcomes[index] = Some(Outcome::NeedsAttention(message))
            }
        }
    }
    // A running computer holds its disks; the clone and the repair need them quiet.
    let mut repairable = Vec::new();
    for (index, running) in pending {
        let name = &names[index];
        if running {
            (options.progress)(Step::Stopping { name: name.clone() });
            if let Err(message) = stop(runner, paths, name) {
                outcomes[index] = Some(Outcome::NeedsAttention(message));
                continue;
            }
            // The stop must have settled before the disks are cloned.
            match diagnose(runner, paths, name) {
                Ok(Diagnosis::Repairable { running: false }) => {}
                Ok(_) | Err(_) => {
                    outcomes[index] = Some(Outcome::NeedsAttention(
                        "This computer did not finish stopping. Its files were kept untouched. Relaunch Silo to try again.".into(),
                    ));
                    continue;
                }
            }
        }
        repairable.push(index);
    }
    if !repairable.is_empty() && !repair_supported(runner, paths) {
        for index in repairable.drain(..) {
            outcomes[index] = Some(Outcome::NeedsAttention(UNSUPPORTED.into()));
        }
    }
    if !repairable.is_empty() {
        (options.progress)(Step::BackingUp);
        (options.backup)()?;
    }
    for (position, index) in repairable.iter().copied().enumerate() {
        (options.progress)(Step::Repairing {
            index: position,
            total: repairable.len(),
        });
        let name = &names[index];
        let outcome = repair(runner, paths, name, options.confirm_maintenance)
            .and_then(|()| match diagnose(runner, paths, name) {
                Ok(Diagnosis::Ready) => Ok(()),
                _ => Err("The upgraded storage of this computer could not be verified. Its original files were kept.".to_string()),
            })
            .map_or_else(Outcome::NeedsAttention, |()| Outcome::Repaired);
        outcomes[index] = Some(outcome);
    }
    Ok(Report {
        version: VERSION,
        computers: names
            .iter()
            .zip(outcomes)
            .map(|(name, outcome)| Computer {
                name: name.clone(),
                outcome: outcome.expect("every computer has an outcome"),
            })
            .collect(),
    })
}

/// Clone `storage` to `<app_data>/BACKUP` unless an earlier run already finished it. The copy is
/// built beside its final name and renamed into place, so a crash never leaves a partial backup
/// that a later run would trust. Restoring is a rename of the folders: the clone's files still
/// name the original location, so it only works in place of the original.
pub(super) fn clone_backup(app_data: &Path, storage: &Path) -> Result<(), String> {
    let backup = app_data.join(BACKUP);
    if fs::symlink_metadata(&backup).is_ok() {
        return Ok(());
    }
    let partial = app_data.join(format!("{BACKUP}.partial"));
    if fs::symlink_metadata(&partial).is_ok() {
        fs::remove_dir_all(&partial)
            .map_err(|_| "An unfinished backup could not be cleared.".to_string())?;
    }
    copy_runtime_tree(storage, &partial, &[])?;
    fs::rename(&partial, &backup)
        .and_then(|()| fs::File::open(app_data).and_then(|directory| directory.sync_all()))
        .map_err(|_| "The backup could not be completed.".to_string())?;
    crate::pre_upgrade_backup::restart_window(app_data);
    Ok(())
}

pub(super) fn write_report(storage: &Path, report: &Report) -> Result<(), String> {
    let mut temporary = tempfile::NamedTempFile::new_in(storage)
        .map_err(|_| "The storage upgrade result could not be saved.")?;
    serde_json::to_writer_pretty(&mut temporary, report)
        .map_err(|_| "The storage upgrade result could not be encoded.")?;
    temporary
        .write_all(b"\n")
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|_| "The storage upgrade result could not be saved.")?;
    temporary
        .persist(storage.join(MARKER))
        .map_err(|_| "The storage upgrade result could not be committed.")?;
    fs::File::open(storage)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "The storage upgrade result could not be committed.".to_string())
}

/// The saved result, if the upgrade ran. Only a complete report ends the upgrade; a report with
/// computers that need attention is retried at the next launch.
pub(super) fn read_report(storage: &Path) -> Result<Option<Report>, String> {
    match fs::read(storage.join(MARKER)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "The saved storage upgrade result is damaged. Existing data was preserved.".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("The saved storage upgrade result could not be read.".into()),
    }
}

// MARK: Gate

#[derive(Default)]
struct Gate {
    /// The step is running (or about to): the stage the owner sees.
    running: HashMap<PathBuf, String>,
    /// Computers the step could not move, with the text shown to the owner.
    attention: BTreeMap<String, String>,
}

static GATE: Mutex<Option<Gate>> = Mutex::new(None);

fn gate() -> MutexGuard<'static, Option<Gate>> {
    GATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn with_gate<T>(change: impl FnOnce(&mut Gate) -> T) -> T {
    change(gate().get_or_insert_with(Gate::default))
}

/// The stage shown while the step holds the computers of `app_data` back.
pub(super) fn running_stage(app_data: &Path) -> Option<String> {
    gate().as_ref()?.running.get(app_data).cloned()
}

pub(super) fn hold(app_data: &Path, stage: &str) {
    with_gate(|gate| gate.running.insert(app_data.to_path_buf(), stage.into()));
}

pub(super) fn release(app_data: &Path) {
    with_gate(|gate| gate.running.remove(app_data));
}

pub(super) fn set_attention(attention: BTreeMap<String, String>) {
    with_gate(|gate| gate.attention = attention);
}

/// Why this computer cannot start or be used until a later launch finishes moving its storage.
pub fn attention_message(name: &str) -> Option<String> {
    gate().as_ref()?.attention.get(name).cloned()
}

/// What the owner sees as the stage while the step runs.
pub(super) fn stage_text(step: &Step) -> String {
    match step {
        Step::Inspecting { index, total } => {
            format!("Checking computer storage ({} of {total})", index + 1)
        }
        Step::Stopping { .. } => "Stopping computers for the storage upgrade".into(),
        Step::BackingUp => "Backing up computer storage".into(),
        Step::Repairing { index, total } => {
            format!("Upgrading computer storage ({} of {total})", index + 1)
        }
    }
}

/// Every name in the report that needs attention, with its text.
pub(super) fn attention_of(report: &Report) -> BTreeMap<String, String> {
    report
        .computers
        .iter()
        .filter_map(|computer| match &computer.outcome {
            Outcome::NeedsAttention(message) => Some((computer.name.clone(), message.clone())),
            _ => None,
        })
        .collect()
}

/// Holds the gate for the step's whole run and releases it however the run ends.
pub(super) struct Held<'a>(pub &'a Path);

impl Drop for Held<'_> {
    fn drop(&mut self) {
        release(self.0);
    }
}

/// True when the step still has work for this storage: it has no report, or the report left
/// computers that need attention.
pub(super) fn pending(storage: &Path) -> bool {
    !matches!(read_report(storage), Ok(Some(report)) if report.complete())
}
