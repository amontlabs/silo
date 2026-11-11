//! Moves computers created by MicroSandbox 0.7.6 to the protected storage layout of 0.7.12 and
//! later. The runtime refuses to start an older computer until `msb self repair` has imported its
//! disks, and on macOS that command needs a maintenance confirmation that only the supervising
//! app can give. See `docs/direction/MSB-0.7.15-MIGRATION.md`.
//!
//! The caller holds the migration gate: no other runtime command runs, no previous Silo build
//! can start, and the computers of this storage are Silo's own. Computers that cannot be moved
//! are reported, never removed; the original disk files are always retained.
use super::{copy_runtime_tree_for_backup, runtime};
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

#[derive(Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Report {
    version: u32,
    pub computers: Vec<Computer>,
    /// The computers the backup clone covers consistently: stopped when it was taken, before
    /// they were repaired.
    #[serde(default)]
    pub backed_up: Vec<String>,
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
    /// Clone the storage folder with the named computers stopped in it. Called once, after
    /// they are stopped and before the first repair. `Err` is the reason, in words for the
    /// owner; those computers are then not repaired.
    pub backup: &'a dyn Fn(&[String]) -> Result<(), String>,
    pub progress: &'a dyn Fn(Step),
}

const RECOVERY: &str = "This computer's disks need a manual decision that Silo cannot make for you. Its files were kept untouched. Report this problem before using the computer.";
const UNKNOWN_RUNNING: &str = "Silo could not tell whether this computer is running. Its files were kept untouched. Relaunch Silo to try again.";
const NOT_STOPPED: &str = "This computer did not finish stopping. Its files were kept untouched. Relaunch Silo to try again.";
const ARRIVED: &str = "This computer arrived with the older storage layout. Its files were kept untouched. Relaunch Silo to move it.";
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
    Ok(match item.get("status").and_then(serde_json::Value::as_str) {
        Some("ready") => Diagnosis::Ready,
        Some("eligible" | "maintenance_required" | "in_progress") => {
            // Not knowing whether the computer runs is not the same as it being stopped.
            match item.get("running").and_then(serde_json::Value::as_bool) {
                Some(running) => Diagnosis::Repairable { running },
                None => Diagnosis::Blocked(UNKNOWN_RUNNING.into()),
            }
        }
        Some("recovery_required") => Diagnosis::Blocked(RECOVERY.into()),
        _ => Diagnosis::Blocked(
            "Silo could not establish the state of this computer's disks. Its files were kept untouched. Relaunch Silo to try again.".into(),
        ),
    })
}

/// Where the runtime keeps a computer's folder.
fn sandbox_dir(storage_home: &Path, name: &str) -> PathBuf {
    storage_home.join("sandboxes").join(name)
}

fn runtime_home(paths: &runtime::RuntimePaths) -> &Path {
    paths.storage_home.as_deref().unwrap_or(&paths.home)
}

/// Whether the runtime has made a folder for the computer. A saved computer has none until it
/// first starts (a restore waiting for Start, a checkpoint fork), and there is nothing to
/// inspect or move before then.
pub(super) fn sandbox_exists(storage_home: &Path, name: &str) -> bool {
    match fs::symlink_metadata(sandbox_dir(storage_home, name)) {
        Err(error) => error.kind() != std::io::ErrorKind::NotFound,
        Ok(_) => true,
    }
}

/// The names whose computer has a folder in the storage folder `storage`.
pub(super) fn existing(storage: &Path, names: Vec<String>) -> Vec<String> {
    let storage_home = storage.join("microsandbox");
    names
        .into_iter()
        .filter(|name| sandbox_exists(&storage_home, name))
        .collect()
}

/// Whether the computer's own record says its protected storage is complete. Read from the
/// folder, so it holds while the runtime cannot be asked: on macOS `doctor` rates every computer
/// `recovery_required` while any legacy computer runs.
fn recorded_ready(paths: &runtime::RuntimePaths, name: &str) -> bool {
    fs::read(sandbox_dir(runtime_home(paths), name).join("state-layout.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|record| {
            record.get("schema").is_some_and(serde_json::Value::is_string)
                && record.get("phase").and_then(serde_json::Value::as_str) == Some("ready")
        })
}

/// Whether the runtime says the computer is running, from `inspect` rather than `doctor`: on
/// macOS `doctor` rates every computer `recovery_required` while any legacy computer runs, so its
/// verdict cannot be asked before the running ones are stopped. `None` when it cannot be told.
fn is_running(
    runner: &dyn runtime::RuntimeRunner,
    paths: &runtime::RuntimePaths,
    name: &str,
) -> Option<bool> {
    let args: Vec<String> = ["inspect", name, "--format", "json"].map(String::from).into();
    let output = runner.run(paths, &args, DIAGNOSE_TIMEOUT).ok()?;
    let value: serde_json::Value = serde_json::from_str(&output.stdout).ok()?;
    match value.get("status")?.as_str()? {
        "Created" | "Stopped" | "Crashed" => Some(false),
        "Running" | "Draining" | "Paused" | "Stopping" => Some(true),
        _ => None,
    }
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

fn blocked_message(detail: &str) -> String {
    let status = serde_json::from_str::<serde_json::Value>(detail)
        .ok()
        .and_then(|value| {
            value
                .pointer("/sandbox/status")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
    match status.as_deref() {
        Some("recovery_required") => RECOVERY.into(),
        Some("running" | "stopping") => "This computer did not finish stopping. Its files were kept untouched. Relaunch Silo to try again.".into(),
        _ => "The storage upgrade of this computer was refused. Its original files were kept untouched. Relaunch Silo to try again.".into(),
    }
}

#[cfg(test)]
pub(super) fn blocked_message_for_test(detail: &str) -> String {
    blocked_message(detail)
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
    let output = match runner.run(paths, &args, REPAIR_TIMEOUT) {
        Ok(output) => output,
        // Exit 2 (blocked) and 3 (unavailable) print `{"sandbox": <diagnosis>, "next_step"}`
        // and change nothing; any other failure has no JSON.
        Err(runtime::RuntimeError::Failed {
            exit_code: Some(2 | 3),
            detail,
            ..
        }) => return Err(blocked_message(&detail)),
        Err(_) => return Err("The storage upgrade of this computer did not finish. Its original files were kept.".to_string()),
    };
    let result: serde_json::Value = serde_json::from_str(&output.stdout)
        .map_err(|_| "The storage upgrade returned an unreadable result.".to_string())?;
    if result.get("ready").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err("The storage upgrade of this computer did not report success. Its original files were kept.".into());
    }
    Ok(())
}

fn backup_failed(reason: &str) -> String {
    format!(
        "Silo could not back up your computers before upgrading their storage: {}. Nothing was changed. Free some disk space if it is low, then relaunch Silo.",
        reason.trim_end_matches('.')
    )
}

/// Upgrade every named computer. Per-computer problems, including a backup that could not be
/// made, are in the report; `Err` is reserved for a failure of the step itself.
pub(super) fn upgrade(
    runner: &dyn runtime::RuntimeRunner,
    paths: &runtime::RuntimePaths,
    names: &[String],
    options: &Options,
) -> Result<Report, String> {
    let total = names.len();
    let mut outcomes: Vec<Option<Outcome>> = vec![None; total];
    // Running legacy computers go first: while any legacy computer runs, the macOS `doctor`
    // cannot exclude its writer and rates every other computer `recovery_required`. A computer
    // whose protected storage is recorded as ready has no legacy writer and keeps running. A
    // stop that fails or does not settle leaves that computer untouched and out of the rest of
    // the step.
    for (index, name) in names.iter().enumerate() {
        if recorded_ready(paths, name) || is_running(runner, paths, name) != Some(true) {
            continue;
        }
        (options.progress)(Step::Stopping { name: name.clone() });
        if let Err(message) = stop(runner, paths, name) {
            outcomes[index] = Some(Outcome::NeedsAttention(message));
        } else if is_running(runner, paths, name) != Some(false) {
            outcomes[index] = Some(Outcome::NeedsAttention(NOT_STOPPED.into()));
        }
    }
    let mut pending = Vec::new();
    for (index, name) in names.iter().enumerate() {
        if outcomes[index].is_some() {
            continue;
        }
        (options.progress)(Step::Inspecting { index, total });
        match diagnose(runner, paths, name) {
            Ok(Diagnosis::Ready) => outcomes[index] = Some(Outcome::AlreadyReady),
            Ok(Diagnosis::Repairable { running }) => pending.push((index, running)),
            Ok(Diagnosis::Blocked(message)) | Err(message) => {
                outcomes[index] = Some(Outcome::NeedsAttention(message))
            }
        }
    }
    // A computer that `inspect` did not show as running but `doctor` does is stopped here; the
    // clone and the repair need the disks quiet.
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
                    outcomes[index] = Some(Outcome::NeedsAttention(NOT_STOPPED.into()));
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
    let mut backed_up = Vec::new();
    if !repairable.is_empty() {
        (options.progress)(Step::BackingUp);
        let covered: Vec<String> = repairable.iter().map(|&index| names[index].clone()).collect();
        match (options.backup)(&covered) {
            Ok(()) => backed_up = covered,
            // Only the computers that still need the move wait; the others are not affected.
            Err(reason) => {
                for index in repairable.drain(..) {
                    outcomes[index] = Some(Outcome::NeedsAttention(backup_failed(&reason)));
                }
            }
        }
    }
    for (position, index) in repairable.iter().copied().enumerate() {
        (options.progress)(Step::Repairing {
            index: position,
            total: repairable.len(),
        });
        let name = &names[index];
        // Stopped now, not only when it was cloned: nothing may write the disks past the copy.
        match diagnose(runner, paths, name) {
            Ok(Diagnosis::Repairable { running: false }) => {}
            Ok(Diagnosis::Ready) => {
                outcomes[index] = Some(Outcome::AlreadyReady);
                continue;
            }
            Ok(Diagnosis::Repairable { running: true }) => {
                outcomes[index] = Some(Outcome::NeedsAttention(NOT_STOPPED.into()));
                continue;
            }
            Ok(Diagnosis::Blocked(message)) | Err(message) => {
                outcomes[index] = Some(Outcome::NeedsAttention(message));
                continue;
            }
        }
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
        backed_up,
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

/// Which computers a finished backup covers: the ones that were stopped in it.
const COVERS: &str = "silo-backup-covers.json";

fn covers_all(backup: &Path, covered: &[String]) -> bool {
    fs::read(backup.join(COVERS))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
        .is_some_and(|have| covered.iter().all(|name| have.contains(name)))
}

/// Clone `storage` to `<app_data>/BACKUP`, with `covered` stopped in it, unless an earlier run
/// already made a backup covering all of them. One that does not (those computers were
/// running, blocked or failed then) is replaced, so every computer repaired has a copy taken
/// while it was stopped and before its repair; the computers an earlier launch repaired keep
/// their untouched original files on disk. The copy is built beside its final name and
/// renamed into place, so a crash never leaves a partial backup that a later run would trust.
/// Restoring is a rename of the folders: the clone's files still name the original location,
/// so it only works in place of the original.
pub(super) fn clone_backup(
    app_data: &Path,
    storage: &Path,
    covered: &[String],
) -> Result<(), String> {
    let backup = app_data.join(BACKUP);
    if covers_all(&backup, covered) {
        return Ok(());
    }
    let partial = app_data.join(format!("{BACKUP}.partial"));
    if fs::symlink_metadata(&partial).is_ok() {
        fs::remove_dir_all(&partial)
            .map_err(|_| "an unfinished backup could not be cleared".to_string())?;
    }
    let made = copy_runtime_tree_for_backup(storage, &partial).and_then(|()| {
        let covers = serde_json::to_vec(covered).map_err(|_| "the backup could not be recorded")?;
        fs::write(partial.join(COVERS), covers).map_err(|_| "the backup could not be recorded".to_string())
    });
    if let Err(reason) = made {
        let _ = fs::remove_dir_all(&partial);
        return Err(reason.trim_end_matches('.').trim_start_matches("Runtime copy ").to_owned());
    }
    crate::pre_upgrade_backup::install_clone(app_data, &partial, &backup)
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
    /// Per storage folder, as the runtime names it (`computers.json`'s folder).
    attention: HashMap<PathBuf, BTreeMap<String, String>>,
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

pub(super) fn set_attention(storage: &Path, attention: BTreeMap<String, String>) {
    with_gate(|gate| gate.attention.insert(storage.to_path_buf(), attention));
}

/// A computer that arrived after the step finished (an imported export, for one) is checked
/// at once. Unless its storage is ready it cannot start, says why, and is recorded so the next
/// launch moves it with the step.
pub(super) fn check_arrival(
    runner: &dyn runtime::RuntimeRunner,
    paths: &runtime::RuntimePaths,
    name: &str,
) {
    if !sandbox_exists(runtime_home(paths), name) {
        return;
    }
    let message = match diagnose(runner, paths, name) {
        Ok(Diagnosis::Ready) => return,
        Ok(Diagnosis::Repairable { .. }) => ARRIVED.to_owned(),
        Ok(Diagnosis::Blocked(message)) | Err(message) => message,
    };
    let Some(storage) = paths.metadata.parent() else { return };
    with_gate(|gate| {
        gate.attention
            .entry(storage.to_path_buf())
            .or_default()
            .insert(name.to_owned(), message.clone())
    });
    let mut report = match read_report(storage) {
        Ok(Some(report)) => report,
        Ok(None) => Report { version: VERSION, ..Report::default() },
        Err(_) => return,
    };
    report.computers.retain(|computer| computer.name != name);
    report.computers.push(Computer {
        name: name.to_owned(),
        outcome: Outcome::NeedsAttention(message),
    });
    let _ = write_report(storage, &report);
}

/// Why this computer cannot start or be used until a later launch finishes moving its storage.
/// `metadata` is the runtime's `computers.json` of the storage the computer lives in.
pub fn attention_message(metadata: &Path, name: &str) -> Option<String> {
    gate().as_ref()?.attention.get(metadata.parent()?)?.get(name).cloned()
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
