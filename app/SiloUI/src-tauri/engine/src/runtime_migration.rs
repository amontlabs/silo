//! One-time gate for the MicroSandbox storage upgrade. The converter is kept
//! here so the normal runtime never has to read the previous storage format.
use crate::runtime;
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

const VERSION: u32 = 1;
const FILE: &str = "runtime-migration.json";
const GENERATION: &str = "runtime-generation.json";
const CLEAN: &str = "runtime-checkpoints-clean";
const CONVERTED: &str = "runtime-checkpoints-converted";
const MISSING_GENERATION: &str = "Completed migration has no saved computer storage selection. Existing data was preserved. Report this problem before using computers.";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Generation {
    version: u32,
    directory: String,
}

fn generation(app_data: &Path) -> Result<Option<String>, String> {
    let bytes = match fs::read(app_data.join(GENERATION)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Silo could not read which computer storage to use. Existing data was preserved. Relaunch Silo and retry migration.".into()),
    };
    let selected: Generation = serde_json::from_slice(&bytes)
        .map_err(|_| "The saved computer storage selection is damaged. Existing data was preserved. Relaunch Silo; if the problem continues, report it before retrying migration.".to_string())?;
    if selected.version != VERSION || !matches!(selected.directory.as_str(), CLEAN | CONVERTED) {
        return Err("This computer storage selection needs another Silo version. Existing data was preserved. Update Silo and retry migration.".into());
    }
    Ok(Some(selected.directory))
}

fn select_generation(app_data: &Path, directory: &str) -> Result<(), String> {
    if !matches!(directory, CLEAN | CONVERTED) {
        return Err("Silo could not select the computer storage for migration. Relaunch Silo and retry migration.".into());
    }
    if let Some(current) = generation(app_data)? {
        return if current == directory {
            Ok(())
        } else {
            Err("Silo is already using a different computer storage folder. Relaunch Silo to refresh migration status before retrying.".into())
        };
    }
    let mut temporary = tempfile::NamedTempFile::new_in(app_data)
        .map_err(|_| "Silo could not prepare the computer storage selection. Check free space and access to Silo storage, then retry migration.")?;
    serde_json::to_writer(
        &mut temporary,
        &Generation {
            version: VERSION,
            directory: directory.into(),
        },
    )
    .map_err(|_| {
        "Silo could not save the computer storage selection. Relaunch Silo and retry migration."
    })?;
    temporary.write_all(b"\n").and_then(|_| temporary.as_file().sync_all())
        .map_err(|_| "Silo could not finish saving the computer storage selection. Check free space and access to Silo storage, then retry migration.")?;
    temporary.persist_noclobber(app_data.join(GENERATION))
        .map_err(|_| "Silo could not apply the computer storage selection. Relaunch Silo to refresh migration status before retrying.")?;
    fs::File::open(app_data).and_then(|file| file.sync_all())
        .map_err(|_| "Silo could not finish saving the computer storage selection. Check free space and access to Silo storage, then retry migration.")?;
    Ok(())
}

pub(crate) fn selected_runtime_storage(app_data: &Path) -> Result<PathBuf, String> {
    Ok(app_data.join(generation(app_data)?.as_deref().unwrap_or("runtime")))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationState {
    version: u32,
    pub status: String,
    pub stage: String,
    pub logs: Vec<String>,
    pub migrated_count: usize,
    pub failed_count: usize,
    pub total_count: usize,
    pub can_continue: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub struct Controller {
    app_data: PathBuf,
    path: PathBuf,
    state: Mutex<MigrationState>,
    writable: bool,
}

impl Controller {
    pub fn new(app_data: PathBuf) -> Self {
        let path = app_data.join(FILE);
        let (state, writable) = match initial(&path, &app_data) {
            Ok(state) => (state, true),
            Err(error) => {
                let mut state = fresh("failed", 0);
                state.stage = "Migration needs attention".into();
                state.error = Some(error);
                (state, false)
            }
        };
        Self {
            app_data,
            path,
            state: Mutex::new(state),
            writable,
        }
    }

    pub fn snapshot(&self) -> Result<MigrationState, String> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| "Migration state is unavailable.".to_string())
    }

    pub fn ensure_ready(&self) -> Result<(), String> {
        let state = self.snapshot()?;
        self.ensure_ready_for(&state)
    }

    pub fn ensure_ready_for(&self, state: &MigrationState) -> Result<(), String> {
        check_ready(self.writable, &state.status)
    }

    pub fn runtime_storage(&self) -> Result<PathBuf, String> {
        let state = self.snapshot()?;
        usable_storage(&self.app_data, self.writable, &state.status)
    }

    pub fn blocks_operations(&self) -> bool {
        self.ensure_ready().is_err()
    }

    pub fn needs_recovery(&self) -> Result<bool, String> {
        Ok(generation(&self.app_data)?.is_none())
    }

    pub fn staged_storage(&self) -> PathBuf {
        self.app_data.join(CONVERTED)
    }

    fn update(
        &self,
        change: impl FnOnce(&mut MigrationState) -> Result<(), String>,
    ) -> Result<MigrationState, String> {
        if !self.writable {
            return Err("Saved migration data needs manual repair. The file was preserved.".into());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Migration state is unavailable.")?;
        let mut next = state.clone();
        change(&mut next)?;
        write(&self.path, &next)?;
        *state = next.clone();
        Ok(next)
    }

    pub fn record_failure(&self, error: String) -> Result<(), String> {
        record_failure(&self.state, &self.path, error)
    }

    pub fn prepare_retry(&self) -> Result<MigrationState, String> {
        self.update(|state| prepare_retry(state, &self.app_data))
    }

    pub fn continue_after_failure(&self) -> Result<MigrationState, String> {
        if !self.writable {
            return Err("Saved migration data needs manual repair. The file was preserved.".into());
        }
        let current = self.snapshot()?;
        if current.status != "failed" || current.total_count == 0 {
            return Err("Continue is available only after a computer migration failure.".into());
        }
        // This creates an independent runtime namespace. The old database, root
        // disks and external computer images stay at their original paths, even
        // if a previous computer process has them open.
        prepare_clean_generation(&self.app_data)?;
        select_generation(&self.app_data, CLEAN)?;
        quarantine_previous_backup_state(&self.app_data, CLEAN)?;
        self.update(|state| {
            state.status = "running".into();
            state.stage = "Restarting into a fresh runtime".into();
            state.error = None;
            state.can_continue = false;
            state.failed_count = state.total_count;
            state.logs.push(
                "Original computer data was preserved. Silo will restart with an empty computer runtime."
                    .into(),
            );
            Ok(())
        })
    }

    pub fn convert_with_progress(
        &self,
        runner: &dyn runtime::RuntimeRunner,
        paths: &runtime::RuntimePaths,
        progress_changed: &dyn Fn(),
    ) -> Result<(), String> {
        convert_with(runner, &self.app_data, paths, &|step| {
            self.update(|state| {
                apply_step(state, &step);
                Ok(())
            })?;
            progress_changed();
            Ok(())
        })?;
        self.update(|state| {
            state.status = "running".into();
            state.stage = "Restarting with upgraded computer storage".into();
            state.can_continue = false;
            state.error = None;
            Ok(())
        })?;
        progress_changed();
        Ok(())
    }
}

fn fresh(status: &str, total_count: usize) -> MigrationState {
    MigrationState {
        version: VERSION,
        status: status.into(),
        stage: if total_count == 0 {
            "Ready"
        } else {
            "Checking existing computers"
        }
        .into(),
        logs: Vec::new(),
        migrated_count: 0,
        failed_count: 0,
        total_count,
        can_continue: false,
        log_path: None,
        error: None,
    }
}

fn write(path: &Path, state: &MigrationState) -> Result<(), String> {
    let parent = path.parent().ok_or("Migration storage path is invalid.")?;
    fs::create_dir_all(parent).map_err(|_| "Migration storage could not be created.")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "Migration progress could not be saved.")?;
    serde_json::to_writer_pretty(&mut temporary, state)
        .map_err(|_| "Migration progress could not be encoded.")?;
    temporary
        .write_all(b"\n")
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|_| "Migration progress could not be saved.")?;
    temporary
        .persist(path)
        .map_err(|_| "Migration progress could not be committed.")?;
    fs::File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|_| "Migration progress could not be committed.")?;
    Ok(())
}

fn read(path: &Path) -> Result<Option<MigrationState>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(
                "Saved migration progress could not be read. The file was preserved.".into(),
            )
        }
    };
    let state: MigrationState = serde_json::from_slice(&bytes)
        .map_err(|_| "Saved migration progress is invalid. The file was preserved.".to_string())?;
    if state.version != VERSION
        || !matches!(
            state.status.as_str(),
            "scanning" | "running" | "failed" | "complete" | "not-required"
        )
    {
        return Err(
            "Saved migration progress uses an unsupported version. The file was preserved.".into(),
        );
    }
    Ok(Some(state))
}

/// The inventory in `folder`, the previous generation's, in whichever vocabulary it was
/// saved. An empty one when `folder` has none.
fn previous_computers(folder: &Path) -> Result<runtime::ComputerConfigurationRequest, String> {
    match vocabulary::read_previous_computers(folder)? {
        Some(inventory) => runtime::metadata_from_value(inventory)
            .map_err(|_| "Silo's saved computer configuration is invalid.".to_string()),
        None => Ok(runtime::ComputerConfigurationRequest {
            schema_version: 1,
            computers: Vec::new(),
        }),
    }
}

fn initial(path: &Path, app_data: &Path) -> Result<MigrationState, String> {
    if let Some(selected) = generation(app_data)? {
        let metadata = app_data.join(&selected).join("computers.json");
        let computers = runtime::read_metadata(&metadata).map_err(|_| {
            "Silo could not verify the selected computer storage. Relaunch Silo and retry migration."
        })?;
        let mut state = read(path)?
            .ok_or("The selected runtime has no migration record. Existing data was preserved.")?;
        let computer_count = computers.computers.iter().count();
        if state.status != "complete" {
            if selected == CLEAN && computer_count != 0 {
                return Err("The new computer storage folder already contains computers. Existing data was preserved. Report this problem before retrying migration.".into());
            }
            if selected == CONVERTED
                && (state.migrated_count != state.total_count
                    || computer_count != state.total_count)
            {
                return Err(
                    "The selected converted runtime does not match verified migration progress."
                        .into(),
                );
            }
        }
        if state.status != "complete" {
            quarantine_previous_backup_state(app_data, &selected)?;
        }
        state.status = "complete".into();
        state.stage = if selected == CLEAN {
            "Ready with a fresh runtime"
        } else {
            "Migration complete"
        }
        .into();
        state.can_continue = false;
        state.error = None;
        if selected == CLEAN {
            state.failed_count = state.total_count;
        }
        write(path, &state)?;
        return Ok(state);
    }
    if let Some(mut state) = read(path)? {
        if state.status == "complete" {
            return Err(MISSING_GENERATION.into());
        }
        if matches!(state.status.as_str(), "scanning" | "running") {
            state.status = "failed".into();
            state.stage = "Interrupted migration".into();
            state.error = Some("The interrupted migration needs verification before retry.".into());
            state.can_continue = true;
            write(path, &state)?;
        }
        return Ok(state);
    }
    let computers = previous_computers(&app_data.join("runtime")).map_err(|error| {
        format!("Existing computer settings could not be read: {error} No data was changed.")
    })?;
    let total = computers.computers.iter().count();
    let state = if total == 0 {
        fresh("not-required", 0)
    } else {
        fresh("scanning", total)
    };
    write(path, &state)?;
    Ok(state)
}

/// Keep the previous generation's export history and unfinished export journal from being
/// replayed against the generation `selected`, by moving them into the previous one.
///
/// A journal that recorded its result stays, so the export and import page still reports
/// it after the upgrade. So does one that waits for the upgrade to finish its cleanup,
/// but only when the converted generation was selected: recovery removes what the
/// interrupted operation left from that generation, which holds a copy of it. The clean
/// generation holds none of it, and the previous generation, where the computers that
/// could not be converted remain, is never cleaned.
fn quarantine_previous_backup_state(app_data: &Path, selected: &str) -> Result<(), String> {
    let old = app_data.join("runtime");
    for name in ["backup-operation.json", "backup-history.json"] {
        let source = app_data.join(name);
        if !source.exists() {
            continue;
        }
        if name == "backup-operation.json" && journal_stays(&source, selected == CONVERTED) {
            continue;
        }
        let destination = old.join(format!("before-checkpoints-{name}"));
        if destination.exists() {
            return Err(
                "Previous backup progress could not be isolated. No backup operation was resumed."
                    .into(),
            );
        }
        fs::create_dir_all(&old).map_err(|_| "Previous backup storage could not be prepared.")?;
        fs::rename(&source, &destination)
            .map_err(|_| "Previous backup progress could not be isolated.")?;
        fs::File::open(app_data)
            .and_then(|file| file.sync_all())
            .map_err(|_| "Previous backup progress could not be synced.")?;
        fs::File::open(&old)
            .and_then(|file| file.sync_all())
            .map_err(|_| "Previous backup progress could not be synced.")?;
    }
    Ok(())
}

/// Whether the export/import journal at `path` is one nothing resumes against the previous
/// runtime: it recorded a result (`terminal`), or, when `awaiting_upgrade` is allowed, it
/// waits for the upgrade to finish its cleanup.
fn journal_stays(path: &Path, awaiting_upgrade: bool) -> bool {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|journal| {
            journal
                .get("terminal")
                .is_some_and(|result| !result.is_null())
                || (awaiting_upgrade
                    && journal.get("awaitingUpgrade") == Some(&serde_json::Value::Bool(true)))
        })
}

fn prepare_clean_generation(app_data: &Path) -> Result<(), String> {
    let source = previous_computers(&app_data.join("runtime")).map_err(|error| {
        format!("Previous computer settings could not be read: {error} No data was changed.")
    })?;
    let target = app_data.join(CLEAN);
    if target.exists() {
        if fs::symlink_metadata(&target)
            .map_err(|_| "A prior clean runtime attempt could not be inspected.")?
            .file_type()
            .is_symlink()
        {
            return Err("A prior clean runtime attempt is redirected. No data was changed.".into());
        }
        vocabulary::convert_storage_directory(&target)?;
        let entries = fs::read_dir(&target)
            .map_err(|_| "A prior clean runtime attempt could not be inspected.")?;
        for entry in entries {
            if entry
                .map_err(|_| "A prior clean runtime attempt could not be inspected.")?
                .file_name()
                != "computers.json"
            {
                return Err(
                    "A prior clean runtime attempt contains unexpected data. No data was changed."
                        .into(),
                );
            }
        }
        let existing = runtime::read_metadata(&target.join("computers.json"))
            .map_err(|_| "A prior clean runtime attempt could not be verified.")?;
        if !existing.computers.is_empty() {
            return Err(
                "A prior clean runtime attempt contains computers. No data was changed.".into(),
            );
        }
        return Ok(());
    }
    let clean = runtime::ComputerConfigurationRequest {
        schema_version: source.schema_version,
        computers: Vec::new(),
    };
    runtime::write_metadata(&target.join("computers.json"), &clean).map_err(|_| {
        "Clean runtime settings could not be prepared. Previous data was preserved.".to_string()
    })?;
    Ok(())
}

/// Copy `source` to `destination`, leaving out the regular files in `excluded`.
fn copy_runtime_tree(
    source: &Path,
    destination: &Path,
    excluded: &[PathBuf],
) -> Result<(), String> {
    if destination.exists() {
        return Err("The staged runtime already exists.".into());
    }
    let mut stack = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((from, to)) = stack.pop() {
        fs::create_dir(&to).map_err(|_| "Runtime copy could not create a directory.")?;
        let entries =
            fs::read_dir(&from).map_err(|_| "Runtime copy could not read a directory.")?;
        for entry in entries {
            let entry = entry.map_err(|_| "Runtime copy could not inspect an entry.")?;
            let name = entry.file_name();
            // Runtime sockets and locks refer to processes in the old generation.
            if from == source && (name == "run" || name == "update-resume.json") {
                continue;
            }
            let path = entry.path();
            let target = to.join(&name);
            let metadata = fs::symlink_metadata(&path)
                .map_err(|_| "Runtime copy could not inspect an entry.")?;
            if metadata.is_dir() {
                stack.push((path, target));
            } else if metadata.is_file() && excluded.contains(&path) {
                continue;
            } else if metadata.is_file() {
                microsandbox_utils::copy::fast_copy(&path, &target)
                    .map_err(|_| "Runtime copy could not copy a file.")?;
                fs::set_permissions(&target, metadata.permissions())
                    .map_err(|_| "Runtime copy could not preserve file permissions.")?;
            } else if metadata.file_type().is_symlink() {
                let link = fs::read_link(&path)
                    .map_err(|_| "Runtime copy could not inspect a symlink.")?;
                if link.is_absolute() {
                    return Err("Runtime copy found an absolute symlink; migration stopped before changing data.".into());
                }
                #[cfg(unix)]
                std::os::unix::fs::symlink(link, target)
                    .map_err(|_| "Runtime copy could not preserve a symlink.")?;
                #[cfg(not(unix))]
                return Err("Runtime copy cannot preserve this symlink on this platform.".into());
            } else if !metadata.file_type().is_socket() {
                return Err("Runtime copy found an unsupported filesystem entry.".into());
            }
        }
        fs::File::open(&to)
            .and_then(|file| file.sync_all())
            .map_err(|_| "Runtime copy could not sync a directory.")?;
    }
    Ok(())
}

fn inspection_failure(stage: &str, name: &str, error: &runtime::RuntimeError) -> String {
    let computer = if name.len() <= 64
        && !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        format!("computer {name}")
    } else {
        "computer".into()
    };
    let reason = match error {
        runtime::RuntimeError::Failed { detail, .. } => {
            let lower = detail.to_ascii_lowercase();
            let reason = if lower.contains("permission denied") {
                "staged runtime files could not be accessed"
            } else if lower.contains("no space left") {
                "this device is out of storage space"
            } else if lower.contains("no such file") || lower.contains("not found") {
                "a staged runtime file is missing"
            } else if lower.contains("unsupported")
                || lower.contains("unknown option")
                || lower.contains("unexpected argument")
            {
                "the runtime rejected the inspection request"
            } else if lower.contains("database") || lower.contains("sqlite") {
                "the staged runtime database could not be read"
            } else {
                "the runtime inspection command failed"
            };
            reason
        }
        runtime::RuntimeError::Unavailable(message) => {
            let reason = if message.contains("bundled MicroSandbox executable") {
                "the bundled runtime executable is missing"
            } else if message.contains("bundled MicroSandbox library") {
                "the bundled runtime library is missing"
            } else if message.contains("managed runtime path") {
                "the staged runtime home could not be prepared"
            } else {
                "the runtime inspection was unavailable"
            };
            reason
        }
        runtime::RuntimeError::Launch(_) => "the bundled runtime could not start",
        runtime::RuntimeError::Partial(_) => "the runtime inspection command failed",
        runtime::RuntimeError::TimedOut { .. } => "the runtime inspection timed out",
        runtime::RuntimeError::Cancelled { .. } => "the runtime inspection was cancelled",
        runtime::RuntimeError::Admission(_) => "the computer operation could not be admitted",
        runtime::RuntimeError::Busy => "another computer operation is still running",
        runtime::RuntimeError::Malformed(_) => "the runtime returned invalid inspection output",
        runtime::RuntimeError::Invalid(_) => "the runtime rejected the inspection request",
    };
    format!("{stage} {computer} inspection failed: {reason}. Relaunch Silo and retry migration.")
}

fn conversion_failure(name: &str, error: &runtime::RuntimeError) -> String {
    let computer = if name.len() <= 64
        && !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        format!("computer {name}")
    } else {
        "computer".into()
    };
    let reason = match error {
        runtime::RuntimeError::Failed { detail, .. } => {
            let lower = detail.to_ascii_lowercase();
            let reason = if lower.contains("unsupported persisted sandbox configuration")
                && lower.contains("config.network.secrets.secrets[0].substitution.basic_auth")
            {
                "saved field config.network.secrets.secrets[0].substitution.basic_auth cannot be preserved by the upgraded runtime"
            } else if lower.contains("permission denied") {
                "the staged runtime files could not be accessed"
            } else if lower.contains("no space left") {
                "this device is out of storage space"
            } else if lower.contains("no such file") || lower.contains("not found") {
                "a staged runtime file is missing"
            } else if lower.contains("process could not be observed") {
                "the runtime process ended without a result"
            } else if lower.contains("returned too much output") {
                "the runtime returned too much diagnostic output"
            } else if lower.contains("unknown option") || lower.contains("unexpected argument") {
                "the runtime rejected the disk conversion command"
            } else if lower.contains("integrity")
                || lower.contains("differs from")
                || lower.contains("invalid size")
                || lower.contains("raw ext4")
            {
                "the workspace disk failed validation or integrity verification"
            } else if lower.contains("database") || lower.contains("sqlite") {
                "the staged runtime database could not be read"
            } else {
                "the runtime conversion command failed"
            };
            reason
        }
        runtime::RuntimeError::Unavailable(message) => {
            let reason = if message.contains("bundled MicroSandbox executable") {
                "the bundled runtime executable is missing"
            } else if message.contains("bundled MicroSandbox library") {
                "the bundled runtime library is missing"
            } else if message.contains("managed runtime path") {
                "the staged runtime home could not be prepared"
            } else {
                "the runtime conversion was unavailable"
            };
            reason
        }
        runtime::RuntimeError::Launch(_) => "the bundled runtime could not start",
        runtime::RuntimeError::Partial(_) => "the runtime conversion command failed",
        runtime::RuntimeError::TimedOut { .. } => {
            "the disk conversion exceeded its 30-minute limit before the runtime reported a cause"
        }
        runtime::RuntimeError::Cancelled { .. } => "the disk conversion was cancelled",
        runtime::RuntimeError::Admission(_) => "the computer operation could not be admitted",
        runtime::RuntimeError::Busy => "another computer operation is still running",
        runtime::RuntimeError::Malformed(_) => "the runtime returned invalid output",
        runtime::RuntimeError::Invalid(_) => "the runtime rejected the workspace disk",
    };
    format!(
        "Workspace disk migration failed for {computer}: {reason}. Original data was preserved. Resolve the cause, then retry migration."
    )
}

fn record_migration_failure(state: &mut MigrationState, error: String) {
    state.status = "failed".into();
    state.stage = "Migration stopped".into();
    state.failed_count = state.total_count.saturating_sub(state.migrated_count);
    state.can_continue = true;
    state.error = Some(error.clone());
    state.logs.push(error);
}

fn record_failure(state: &Mutex<MigrationState>, path: &Path, error: String) -> Result<(), String> {
    let mut state = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    record_migration_failure(&mut state, error);
    write(path, &state)
}

/// Adoption reads the configured disk in the previous generation, verifies its
/// owned copy, and never writes the source.
fn convert_workspace_disk(
    runner: &dyn runtime::RuntimeRunner,
    paths: &runtime::RuntimePaths,
    name: &str,
) -> Result<(), String> {
    let args = vec!["adopt-disk".into(), name.into()];
    runner
        .run(paths, &args, Duration::from_secs(60 * 30))
        .map(|_| ())
        .map_err(|error| conversion_failure(name, &error))
}

fn verify_staged_computer(
    runner: &dyn runtime::RuntimeRunner,
    paths: &runtime::RuntimePaths,
    name: &str,
    id: &str,
    old_runtime: &Path,
) -> Result<(), String> {
    let args = vec![
        "inspect".into(),
        name.into(),
        "--format".into(),
        "json".into(),
    ];
    let output = runner
        .run(paths, &args, Duration::from_secs(30))
        .map_err(|error| inspection_failure("Staged", name, &error))?;
    let inspected: serde_json::Value = serde_json::from_str(&output.stdout)
        .map_err(|_| "The staged computer returned invalid inspection data.")?;
    let status = inspected.get("status").and_then(serde_json::Value::as_str);
    // Inspection reconciles dead Running processes to Crashed. All three states
    // allow disk-only adoption; never start guest code during migration.
    if !matches!(status, Some("Created" | "Stopped" | "Crashed")) {
        return Err(
            "An existing computer is not stopped. Stop it before retrying migration.".into(),
        );
    }
    if inspected
        .pointer("/config/labels/silo.machine-id")
        .and_then(serde_json::Value::as_str)
        != Some(id)
    {
        return Err("A staged computer's identity does not match Silo's saved identity.".into());
    }
    let mounts = inspected
        .pointer("/config/mounts")
        .and_then(serde_json::Value::as_array)
        .ok_or("A staged computer does not report its mounts.")?;
    let computer: Vec<_> = mounts
        .iter()
        .filter(|mount| {
            mount.get("guest").and_then(serde_json::Value::as_str) == Some("/workspace")
        })
        .collect();
    if computer.len() != 1
        || computer[0].get("type").and_then(serde_json::Value::as_str) != Some("DiskImage")
    {
        return Err("A staged computer does not have the expected external workspace disk.".into());
    }
    if mounts.iter().any(|mount| {
        mount.get("type").and_then(serde_json::Value::as_str) == Some("DiskImage")
            && mount.get("guest").and_then(serde_json::Value::as_str) != Some("/workspace")
    }) {
        return Err("This computer has a linked disk outside /workspace that Silo cannot include in checkpoints. Its files were preserved. Report this problem before retrying migration.".into());
    }
    let host = computer[0]
        .get("host")
        .and_then(serde_json::Value::as_str)
        .ok_or("A staged computer's external disk path is missing.")?;
    let original = Path::new(host);
    if !original.starts_with(old_runtime) {
        return Err("A staged computer's external disk lies outside Silo storage.".into());
    }
    if !original.is_file() {
        return Err("An existing workspace disk is unavailable.".into());
    }
    Ok(())
}

/// A conversion step reported to the migration state.
enum Step {
    Copying,
    Converting { index: usize, total: usize },
    Converted { index: usize, total: usize },
}

fn apply_step(state: &mut MigrationState, step: &Step) {
    match *step {
        Step::Copying => state.stage = "Copying existing computer data".into(),
        Step::Converting { index, total } => {
            state.stage = format!("Converting computer {} of {}", index + 1, total)
        }
        Step::Converted { index, total } => {
            state.migrated_count = index + 1;
            state.logs.push(format!(
                "Computer {} of {} converted and verified.",
                index + 1,
                total
            ));
        }
    }
}

/// Convert the previous runtime generation under `app_data` into a staged copy
/// at `paths` and select it. Every refusal happens before anything is written;
/// the original generation is never modified (D-42).
fn convert_with(
    runner: &dyn runtime::RuntimeRunner,
    app_data: &Path,
    paths: &runtime::RuntimePaths,
    progress: &dyn Fn(Step) -> Result<(), String>,
) -> Result<(), String> {
    // Once selected, this generation is committed storage, never disposable staging.
    if let Some(selected) = generation(app_data)? {
        return quarantine_previous_backup_state(app_data, &selected);
    }
    let old_runtime = app_data.join("runtime");
    let old_metadata = previous_computers(&old_runtime).map_err(|error| {
        format!("Existing computer settings could not be read: {error} No data was changed.")
    })?;
    let computers: Vec<_> = old_metadata.computers.iter().collect();
    if computers.is_empty() {
        return Err("No existing computers need conversion.".into());
    }
    let lifecycle = old_runtime.join("lifecycle-operations");
    let pending_lifecycle = lifecycle.is_dir()
        && fs::read_dir(&lifecycle)
            .map_err(|_| "Previous computer actions could not be inspected.")?
            .next()
            .is_some();
    if old_runtime.join("configuration-operation.json").exists() || pending_lifecycle {
        return Err(
            "A previous computer operation still needs recovery. Migration did not change data."
                .into(),
        );
    }
    // A journal that was read but cannot be used is not waited for, since nothing can
    // settle it. It is set aside after every refusal below, so a refused migration still
    // changes nothing. One that could not be read at all is not known to be unusable, so it
    // is never set aside: the migration waits for a launch that can read it.
    let journal = crate::backup_controller::journal_state(app_data);
    if matches!(journal, crate::backup_controller::JournalState::Pending) {
        return Err(
            "An interrupted backup must finish before computer migration. No data was changed."
                .into(),
        );
    }
    if matches!(journal, crate::backup_controller::JournalState::Unavailable) {
        return Err(
            "The saved export or import record could not be read. Relaunch Silo to try again. No data was changed."
                .into(),
        );
    }
    let staged = app_data.join(CONVERTED);
    if staged.exists() || fs::symlink_metadata(&staged).is_ok() {
        if fs::symlink_metadata(&staged)
            .map_err(|_| "Staged runtime could not be inspected.")?
            .file_type()
            .is_symlink()
        {
            return Err("Staged runtime is redirected; it was preserved.".into());
        }
        let _guard = runtime::configuration_recovery::command_lock(paths, Duration::from_secs(30))
            .map_err(|_| "A previous conversion command is still finishing. Retry later.")?;
        fs::remove_dir_all(&staged).map_err(|_| "Prior staged conversion could not be cleared.")?;
    }
    if matches!(journal, crate::backup_controller::JournalState::Unreadable) {
        let aside = crate::backup_controller::set_aside_unreadable_journal(app_data)?;
        eprintln!(
            "An export or import record could not be read and was set aside as {}.",
            aside.display()
        );
    }
    progress(Step::Copying)?;
    // Each workspace disk is adopted straight from the previous generation, so a
    // staged copy would only remain as an unused duplicate of the computer.
    let mut previous = paths.clone();
    previous.volumes = old_runtime.join("volumes");
    let workspace_disks: Vec<_> = computers
        .iter()
        .map(|configuration| runtime::disk_path(&previous, configuration.name(), "workspace"))
        .collect();
    copy_runtime_tree(&old_runtime, &staged, &workspace_disks)?;
    vocabulary::convert_storage_directory(&staged)?;
    // The copied image descriptors still name the old runtime's files by absolute path.
    // An image already broken in the old runtime stays as it was; it does not stop the rest.
    if let Err(message) = runtime::image_cache::repair(&staged.join("microsandbox/cache")) {
        eprintln!("Image cache repair: {message}");
    }
    runtime::prepare_runtime_home(&paths.home, paths.storage_home.as_deref())
        .map_err(|_| "Staged runtime home could not be prepared.")?;
    let total = computers.len();
    for (index, configuration) in computers.iter().enumerate() {
        progress(Step::Converting { index, total })?;
        verify_staged_computer(
            runner,
            paths,
            configuration.name(),
            configuration.id(),
            &old_runtime,
        )?;
        convert_workspace_disk(runner, paths, configuration.name())?;
        let inspect_args = vec![
            "inspect".into(),
            configuration.name().into(),
            "--format".into(),
            "json".into(),
        ];
        let output = runner
            .run(paths, &inspect_args, Duration::from_secs(30))
            .map_err(|error| inspection_failure("Converted", configuration.name(), &error))?;
        let inspected: serde_json::Value = serde_json::from_str(&output.stdout)
            .map_err(|_| "Converted computer inspection returned invalid data.")?;
        let owned = inspected
            .pointer("/config/mounts")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|mounts| {
                mounts.iter().any(|mount| {
                    mount.get("guest").and_then(serde_json::Value::as_str) == Some("/workspace")
                        && mount.get("type").and_then(serde_json::Value::as_str) == Some("Owned")
                })
            });
        if !owned {
            return Err("Converted workspace disk was not verified as owned storage.".into());
        }
        progress(Step::Converted { index, total })?;
    }
    // The marker is the commit point. Before it, normal Silo paths still use
    // the untouched old generation; after it, all paths resolve to the staged
    // and verified new generation. A crash before the quarantine below is
    // finished by `initial` at the next launch.
    select_generation(app_data, CONVERTED)?;
    quarantine_previous_backup_state(app_data, CONVERTED)?;
    Ok(())
}

const NOT_READY: &str = "Finish the Silo runtime migration before using computers.";

/// Whether the normal runtime may be used in this migration state. Until the
/// migration is `complete` or `not-required`, no runtime command may run and no
/// runtime database may be opened (a newer `msb` upgrades the database it opens in
/// place, and a live one can tear the conversion's copy). After "Continue" the
/// untouched previous generation holds the only copy of unconverted computers.
fn check_ready(writable: bool, status: &str) -> Result<(), String> {
    if !writable {
        return Err("Saved migration data needs manual repair. The file was preserved.".into());
    }
    if matches!(status, "complete" | "not-required") {
        Ok(())
    } else {
        Err(NOT_READY.into())
    }
}

/// The runtime storage the normal runtime may use in this migration state, or why
/// it may not use any. Until a generation is selected this is the previous
/// generation (`runtime/`), which is why a pending, running or failed migration
/// refuses every caller: the folder is a pre-upgrade backup, never a live runtime.
fn usable_storage(app_data: &Path, writable: bool, status: &str) -> Result<PathBuf, String> {
    check_ready(writable, status)?;
    match generation(app_data)? {
        Some(selected) => Ok(app_data.join(selected)),
        None if status == "complete" => Err(MISSING_GENERATION.into()),
        None => Ok(app_data.join("runtime")),
    }
}

/// Record a failed migration in memory even when saving it fails (a full disk
/// is the likely cause of the failed copy), so Retry and Continue stay
/// available for this session. The persistence error is returned.
pub async fn migration_snapshot(
    controller: Arc<Controller>,
) -> Result<MigrationState, String> {
    tokio::task::spawn_blocking(move || {
        controller
            .state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| "Migration state is unavailable.".to_string())
    })
    .await
    .map_err(|_| "Migration state is unavailable.".to_string())?
}

fn prepare_retry(state: &mut MigrationState, app_data: &Path) -> Result<(), String> {
    if !matches!(state.status.as_str(), "scanning" | "failed") {
        return Err("Migration is already running or complete.".into());
    }
    let committed = generation(app_data)?.is_some();
    state.status = "running".into();
    state.stage = if committed {
        "Finishing runtime migration"
    } else {
        "Preparing conversion"
    }
    .into();
    state.error = None;
    state.can_continue = false;
    if !committed {
        state.migrated_count = 0;
        state.failed_count = 0;
        state.logs.clear();
    }
    Ok(())
}

/// True when the migration finished and every computer lives in the converted generation.
/// The previous generation (`<app_data>/runtime`) is then a pre-upgrade backup that nothing
/// reads. Never true after "Continue" (the clean generation), where it holds the only copy
/// of the unconverted computers, or while the migration is unfinished.
pub(crate) fn previous_generation_is_backup(app_data: &Path) -> bool {
    matches!(generation(app_data), Ok(Some(selected)) if selected == CONVERTED)
        && matches!(read(&app_data.join(FILE)), Ok(Some(state)) if state.status == "complete")
}

/// The pre-upgrade backup and the storage Silo reads computers from instead, once
/// [`previous_generation_is_backup`] holds. Paths only: the backup module measures, reveals
/// or deletes the previous generation and never runs the runtime against either path.
pub(crate) struct BackupLocations {
    pub(crate) previous: PathBuf,
    pub(crate) selected: PathBuf,
}

pub(crate) fn backup_locations(app_data: &Path) -> Option<BackupLocations> {
    if !previous_generation_is_backup(app_data) {
        return None;
    }
    let selected = selected_runtime_storage(app_data).ok()?;
    Some(BackupLocations {
        previous: app_data.join("runtime"),
        selected,
    })
}

/// The symlink in the account's state directory through which the runtime reached a
/// generation's `microsandbox` directory, and the directory it points at.
pub(crate) struct RuntimeAlias {
    pub(crate) link: PathBuf,
    pub(crate) target: PathBuf,
}

impl BackupLocations {
    /// The alias the previous generation was reached through, derived exactly as the runtime
    /// derives it. It dangles once the backup is deleted and nothing uses it after the
    /// migration. The converted generation's alias hashes another storage path, so it is a
    /// different entry that this never names.
    pub(crate) fn previous_alias(&self, user_home: &Path) -> RuntimeAlias {
        let target = self.previous.join("microsandbox");
        RuntimeAlias {
            link: runtime::runtime_home_alias(user_home, &target),
            target,
        }
    }
}

#[cfg(test)]
#[cfg_attr(feature = "engine-sources", path = "runtime_migration/guard_tests.rs")]
mod guard_tests;
#[cfg(test)]
#[cfg_attr(feature = "engine-sources", path = "runtime_migration/interrupted_tests.rs")]
mod interrupted_tests;
#[cfg(feature = "engine-sources")]
#[path = "runtime_migration/vocabulary.rs"]
pub mod vocabulary;
#[cfg(not(feature = "engine-sources"))]
pub(crate) mod vocabulary;
#[cfg(test)]
#[cfg_attr(feature = "engine-sources", path = "runtime_migration/vocabulary_tests.rs")]
pub(crate) mod vocabulary_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_state_wait_does_not_block_the_async_executor() {
        let directory = tempfile::tempdir().unwrap();
        let controller = Arc::new(Controller {
            app_data: directory.path().into(),
            path: directory.path().join(FILE),
            state: Mutex::new(fresh("running", 1)),
            writable: true,
        });
        let writing = controller.clone();
        let (held, acquired) = std::sync::mpsc::channel();
        let (heartbeat, observed) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let _guard = writing.state.lock().unwrap();
            held.send(()).unwrap();
            observed.recv_timeout(Duration::from_secs(2)).is_ok()
        });
        acquired.recv().unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (result, ()) = runtime.block_on(async {
            tokio::join!(biased; migration_snapshot(controller), async {
                let _ = heartbeat.send(());
            })
        });
        assert_eq!(result.unwrap().status, "running");
        assert!(
            writer.join().unwrap(),
            "waiting for migration state blocked the executor"
        );
    }
    #[test]
    fn failure_is_recorded_in_memory_when_saving_it_fails() {
        let directory = tempfile::tempdir().unwrap();
        let blocker = directory.path().join("file");
        fs::write(&blocker, "").unwrap();
        let mut running = fresh("running", 2);
        running.migrated_count = 1;
        let state = Mutex::new(running);
        assert!(record_failure(&state, &blocker.join("state.json"), "disk full".into()).is_err());
        let state = state.lock().unwrap();
        assert_eq!(state.status, "failed");
        assert!(state.can_continue);
        assert_eq!(state.error.as_deref(), Some("disk full"));
    }

    fn one_computer(name: &str, id: &str) -> runtime::ComputerConfigurationRequest {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "computers": [{"id":id,"name":name,"cpus":1,"maxCPUs":1,"memoryGiB":1,"maxMemoryGiB":1,"workspaceStorageGiB":1,"runtimeStorageGiB":1}]
        })).unwrap()
    }

    #[test]
    fn failed_staged_inspection_preserves_a_safe_actionable_cause() {
        let dir = tempfile::tempdir().unwrap();
        let paths = runtime::RuntimePaths {
            guest_image: dir.path().join("guest-image"),
            storage_home: None,
            executable: dir.path().join("missing-msb"),
            home: dir.path().join("home"),
            library: dir.path().join("library"),
            metadata: dir.path().join("computers.json"),
            volumes: dir.path().join("volumes"),
        };
        let error = verify_staged_computer(
            &runtime::ProcessRunner,
            &paths,
            "dev",
            "synthetic-id",
            dir.path(),
        )
        .unwrap_err();
        assert!(error.contains("dev"));
        assert!(error.contains("bundled runtime executable is missing"));
        assert!(!error.contains(dir.path().to_str().unwrap()));
    }

    #[test]
    fn inspection_failure_explains_recovery_without_runtime_output() {
        let error = runtime::RuntimeError::Failed {
            operation: "Reading computer state".into(),
            exit_code: Some(73),
            detail: "unknown option token=private /Users/person/data".into(),
        };
        let message = inspection_failure("Staged", "dev", &error);
        assert!(message.contains("dev"));
        assert!(message.contains("runtime rejected the inspection request"));
        assert!(!message.contains("exit code"));
        assert!(message.contains("Relaunch Silo"));
        assert!(!message.contains("private"));
        assert!(!message.contains("/Users"));
        assert!(message.len() < 200);
        assert!(!inspection_failure("Staged", "token=private", &error).contains("private"));
    }

    #[cfg(unix)]
    #[test]
    fn rejected_disk_conversion_reports_safe_cause_in_migration_state() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("msb");
        fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' 'Permission denied token=TOPSECRET /private/tmp/workspace.raw' >&2\nexit 23\n",
        ).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let library = dir.path().join("libkrunfw");
        fs::write(&library, b"fixture").unwrap();
        let paths = runtime::RuntimePaths {
            guest_image: dir.path().join("guest-image"),
            storage_home: None,
            executable,
            home: dir.path().join("home"),
            library,
            metadata: dir.path().join("computers.json"),
            volumes: dir.path().join("volumes"),
        };

        let error = convert_workspace_disk(&runtime::ProcessRunner, &paths, "dev").unwrap_err();
        let mut state = fresh("running", 2);
        record_migration_failure(&mut state, error);

        let visible = state.error.as_deref().unwrap();
        assert!(visible.contains("computer dev"));
        assert!(visible.contains("permission denied") || visible.contains("could not be accessed"));
        assert!(!visible.contains("exit code"));
        assert!(visible.contains("retry migration"));
        assert!(!visible.contains("TOPSECRET"));
        assert!(!visible.contains("/private/tmp"));
        assert_eq!(state.logs.first().map(String::as_str), Some(visible));
        assert_eq!(state.failed_count, 2);
        assert!(state.can_continue);
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_saved_basic_auth_configuration_is_reported_from_runtime_stderr() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("msb");
        fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' 'error: invalid config: unsupported persisted sandbox configuration: field config.network.secrets.secrets[0].substitution.basic_auth cannot be preserved' >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let library = dir.path().join("libkrunfw");
        fs::write(&library, b"fixture").unwrap();
        let paths = runtime::RuntimePaths {
            guest_image: dir.path().join("guest-image"),
            storage_home: None,
            executable,
            home: dir.path().join("home"),
            library,
            metadata: dir.path().join("computers.json"),
            volumes: dir.path().join("volumes"),
        };

        let error = convert_workspace_disk(&runtime::ProcessRunner, &paths, "dev").unwrap_err();
        let mut state = fresh("running", 2);
        record_migration_failure(&mut state, error);

        let visible = state.error.as_deref().unwrap();
        assert!(visible.contains("computer dev"));
        assert!(!visible.contains("exit code"));
        assert!(visible.contains("retry migration"));
        assert!(visible.contains("config.network.secrets.secrets[0].substitution.basic_auth"));
        assert!(visible.contains("cannot be preserved by the upgraded runtime"));
        assert_eq!(state.logs.first().map(String::as_str), Some(visible));
    }

    #[test]
    fn conversion_diagnostics_distinguish_timeout_busy_and_missing_exit_status() {
        let timeout = runtime::RuntimeError::TimedOut {
            operation: "adopt-disk".into(),
        };
        let busy = runtime::RuntimeError::Busy;
        let no_exit = runtime::RuntimeError::Failed {
            operation: "adopt-disk".into(),
            exit_code: None,
            detail: "the process could not be observed: interrupted".into(),
        };

        assert!(conversion_failure("dev", &timeout).contains("30-minute limit"));
        assert!(conversion_failure("dev", &busy).contains("another computer operation"));
        assert!(conversion_failure("dev", &no_exit).contains("ended without a result"));
    }

    #[test]
    fn progress_is_durable_and_interruption_blocks_startup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let metadata = dir.path().join("computers.json");
        let state = initial(&path, &metadata).unwrap();
        assert_eq!(state.status, "not-required");
        assert_eq!(read(&path).unwrap().unwrap().status, "not-required");
        let mut running = fresh("running", 1);
        running.logs.push("Copying workspace disk".into());
        write(&path, &running).unwrap();
        let recovered = initial(&path, &metadata).unwrap();
        assert_eq!(recovered.status, "failed");
        assert_eq!(recovered.total_count, 1);
        assert!(recovered.can_continue);
    }

    #[test]
    fn unsupported_journal_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        fs::write(&path, br#"{"version":2,"status":"complete"}"#).unwrap();
        assert!(read(&path).is_err());
        assert_eq!(
            fs::read(&path).unwrap(),
            br#"{"version":2,"status":"complete"}"#
        );
    }

    #[test]
    fn continue_generation_keeps_old_computer_and_backup_data_out_of_new_runtime() {
        // The runtime alias requires a short root to fit its Unix control socket path.
        let dir = tempfile::Builder::new()
            .prefix("sm")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let app_data = dir.path();
        let old = app_data.join("runtime");
        fs::create_dir(&old).unwrap();
        let previous: runtime::ComputerConfigurationRequest = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "computers": [
                {"id":"fcfbc268-ae3f-40ff-8dfa-8af78911e52f","name":"old", "cpus":1,"maxCPUs":1,"memoryGiB":1,"maxMemoryGiB":1,"workspaceStorageGiB":1,"runtimeStorageGiB":1}
            ]
        })).unwrap();
        runtime::write_metadata(&old.join("computers.json"), &previous).unwrap();
        fs::write(old.join("workspace.raw"), b"original").unwrap();
        fs::write(
            app_data.join("backup-operation.json"),
            b"old backup journal",
        )
        .unwrap();
        let before = fs::read(old.join("computers.json")).unwrap();
        prepare_clean_generation(app_data).unwrap();
        select_generation(app_data, CLEAN).unwrap();
        let failed = fresh("failed", 1);
        write(&app_data.join(FILE), &failed).unwrap();
        quarantine_previous_backup_state(app_data, CLEAN).unwrap();
        assert_eq!(
            selected_runtime_storage(app_data).unwrap(),
            app_data.join(CLEAN)
        );
        let old_alias = runtime::runtime_home_alias(app_data, &old.join("microsandbox"));
        let new_alias =
            runtime::runtime_home_alias(app_data, &app_data.join(CLEAN).join("microsandbox"));
        assert_ne!(old_alias, new_alias);
        runtime::prepare_runtime_home(&new_alias, Some(&app_data.join(CLEAN).join("microsandbox")))
            .unwrap();
        assert_eq!(
            fs::read_link(&new_alias).unwrap(),
            app_data.join(CLEAN).join("microsandbox")
        );
        let clean = runtime::read_metadata(&app_data.join(CLEAN).join("computers.json")).unwrap();
        assert!(clean.computers.is_empty());
        assert_eq!(fs::read(old.join("computers.json")).unwrap(), before);
        assert_eq!(fs::read(old.join("workspace.raw")).unwrap(), b"original");
        assert_eq!(
            fs::read(old.join("before-checkpoints-backup-operation.json")).unwrap(),
            b"old backup journal"
        );
        select_generation(app_data, CLEAN).unwrap();
        let reopened = initial(&app_data.join(FILE), app_data).unwrap();
        assert_eq!(reopened.status, "complete");
        assert_eq!(reopened.failed_count, 1);

        let mut with_new_computer = clean;
        with_new_computer.computers.extend(
            one_computer(
                "created-after-continue",
                "60e2e26f-c248-4f24-9035-c766d8168fa8",
            )
            .computers,
        );
        runtime::write_metadata(
            &app_data.join(CLEAN).join("computers.json"),
            &with_new_computer,
        )
        .unwrap();
        assert_eq!(
            initial(&app_data.join(FILE), app_data).unwrap().status,
            "complete"
        );
    }

    #[test]
    fn completed_migration_never_falls_back_when_its_generation_marker_is_missing() {
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path();
        let path = app_data.join(FILE);
        let metadata = one_computer("source", "fcfbc268-ae3f-40ff-8dfa-8af78911e52f");
        for generation in ["runtime", CONVERTED] {
            runtime::write_metadata(&app_data.join(generation).join("computers.json"), &metadata)
                .unwrap();
        }
        fs::write(app_data.join("runtime/workspace.raw"), b"pre-upgrade").unwrap();
        fs::write(app_data.join(CONVERTED).join("workspace.raw"), b"converted").unwrap();
        let mut completed = fresh("complete", 1);
        completed.migrated_count = 1;
        write(&path, &completed).unwrap();
        let before = fs::read(&path).unwrap();

        assert!(initial(&path, app_data).is_err());
        assert!(usable_storage(app_data, true, "complete").is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(
            fs::read(app_data.join("runtime/workspace.raw")).unwrap(),
            b"pre-upgrade"
        );
        assert_eq!(
            fs::read(app_data.join(CONVERTED).join("workspace.raw")).unwrap(),
            b"converted"
        );

        select_generation(app_data, CONVERTED).unwrap();
        assert_eq!(initial(&path, app_data).unwrap().status, "complete");
        assert_eq!(
            usable_storage(app_data, true, "complete").unwrap(),
            app_data.join(CONVERTED)
        );
        fs::remove_file(app_data.join(GENERATION)).unwrap();
        assert!(usable_storage(app_data, true, "complete").is_err());
    }

    #[test]
    fn an_unmigrated_install_keeps_the_original_generation_without_a_marker() {
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path();
        let path = app_data.join(FILE);
        write(&path, &fresh("not-required", 0)).unwrap();
        assert_eq!(initial(&path, app_data).unwrap().status, "not-required");
        assert_eq!(
            usable_storage(app_data, true, "not-required").unwrap(),
            app_data.join("runtime")
        );
    }

    #[test]
    fn completed_converted_generation_allows_forks_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let app_data = dir.path();
        let converted = app_data.join(CONVERTED);
        fs::create_dir(&converted).unwrap();
        runtime::write_metadata(
            &converted.join("computers.json"),
            &one_computer("source", "fcfbc268-ae3f-40ff-8dfa-8af78911e52f"),
        )
        .unwrap();
        select_generation(app_data, CONVERTED).unwrap();
        let mut migrating = fresh("running", 1);
        migrating.migrated_count = 1;
        write(&app_data.join(FILE), &migrating).unwrap();
        let current_history = app_data.join("backup-history.json");
        fs::write(&current_history, b"previous backup history").unwrap();

        let first_start = initial(&app_data.join(FILE), app_data).unwrap();
        assert_eq!(first_start.status, "complete");
        assert_eq!(
            read(&app_data.join(FILE)).unwrap().unwrap().status,
            "complete"
        );
        assert_eq!(
            fs::read(app_data.join("runtime/before-checkpoints-backup-history.json")).unwrap(),
            b"previous backup history"
        );

        let saved_history = br#"{"schemaVersion":1,"destination":"/tmp/backups","archives":[]}"#;
        fs::write(&current_history, saved_history).unwrap();

        let mut with_fork = one_computer("source", "fcfbc268-ae3f-40ff-8dfa-8af78911e52f");
        with_fork
            .computers
            .extend(one_computer("fork", "60e2e26f-c248-4f24-9035-c766d8168fa8").computers);
        runtime::write_metadata(&converted.join("computers.json"), &with_fork).unwrap();
        fs::create_dir_all(converted.join("checkpoints/fcfbc268-ae3f-40ff-8dfa-8af78911e52f"))
            .unwrap();
        fs::write(
            converted.join("checkpoints/fcfbc268-ae3f-40ff-8dfa-8af78911e52f/lifecycle-v1.json"),
            b"{}",
        )
        .unwrap();

        assert_eq!(
            initial(&app_data.join(FILE), app_data).unwrap().status,
            "complete"
        );
        assert_eq!(fs::read(&current_history).unwrap(), saved_history);
        assert_eq!(
            fs::read(app_data.join("runtime/before-checkpoints-backup-history.json")).unwrap(),
            b"previous backup history"
        );
    }

    const COMPUTER_ID: &str = "fcfbc268-ae3f-40ff-8dfa-8af78911e52f";

    /// A previous runtime generation with one computer and its external workspace disk,
    /// and the staged paths the converter would use.
    fn previous_generation() -> (tempfile::TempDir, runtime::RuntimePaths) {
        // Use /tmp rather than macOS TMPDIR: staged homes must fit Unix socket path limits.
        let dir = tempfile::Builder::new()
            .prefix("sm")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let app_data = dir.path();
        let old = app_data.join("runtime");
        fs::create_dir_all(old.join("volumes/dev")).unwrap();
        runtime::write_metadata(
            &old.join("computers.json"),
            &one_computer("dev", COMPUTER_ID),
        )
        .unwrap();
        fs::write(old.join("volumes/dev/workspace.raw"), b"computer").unwrap();
        let storage = app_data.join(CONVERTED);
        let storage_home = storage.join("microsandbox");
        let paths = runtime::RuntimePaths {
            guest_image: app_data.join("guest-image"),
            executable: app_data.join("msb"),
            home: runtime::runtime_home_alias(app_data, &storage_home),
            storage_home: Some(storage_home),
            library: app_data.join("libkrunfw"),
            metadata: storage.join("computers.json"),
            volumes: storage.join("volumes"),
        };
        (dir, paths)
    }

    /// Every entry under `root` with its bytes (files), target (symlinks) or a marker (directories).
    fn tree(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        let mut entries = std::collections::BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(directory) = stack.pop() {
            for entry in fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                let key = path.strip_prefix(root).unwrap().to_path_buf();
                let value = if metadata.file_type().is_symlink() {
                    fs::read_link(&path)
                        .unwrap()
                        .into_os_string()
                        .into_encoded_bytes()
                } else if metadata.is_dir() {
                    stack.push(path);
                    b"<directory>".to_vec()
                } else {
                    fs::read(&path).unwrap()
                };
                entries.insert(key, value);
            }
        }
        entries
    }

    /// The staged runtime: reports the external disk until `adopt-disk` ran, then owned storage.
    struct StagedRuntime {
        old_runtime: PathBuf,
        status: &'static str,
        calls: Mutex<Vec<String>>,
        adoption: Mutex<Vec<String>>,
    }
    impl StagedRuntime {
        fn new(app_data: &Path, status: &'static str) -> Self {
            Self {
                old_runtime: app_data.join("runtime"),
                status,
                calls: Mutex::new(Vec::new()),
                adoption: Mutex::new(Vec::new()),
            }
        }
    }
    impl runtime::RuntimeRunner for StagedRuntime {
        fn run(
            &self,
            _: &runtime::RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<runtime::CommandOutput, runtime::RuntimeError> {
            let mut calls = self.calls.lock().unwrap();
            calls.push(args[0].clone());
            if args[0] == "adopt-disk" {
                *self.adoption.lock().unwrap() = args.to_vec();
            }
            let mount = if calls.iter().any(|call| call == "adopt-disk") {
                serde_json::json!({"guest":"/workspace","type":"Owned"})
            } else {
                serde_json::json!({"guest":"/workspace","type":"DiskImage","host":self.old_runtime.join("volumes/dev/workspace.raw")})
            };
            Ok(runtime::CommandOutput {
                stdout: serde_json::json!({"name":"dev","status":self.status,"config":{"labels":{"silo.machine-id":COMPUTER_ID},"mounts":[mount]}}).to_string(),
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn every_conversion_refusal_leaves_the_data_directory_untouched() {
        type Arrange = fn(&Path);
        let cases: [(&str, Arrange); 6] = [
            ("No existing computers", |app_data| {
                runtime::write_metadata(
                    &app_data.join("runtime/computers.json"),
                    &runtime::ComputerConfigurationRequest {
                        schema_version: 1,
                        computers: vec![],
                    },
                )
                .unwrap();
            }),
            ("still needs recovery", |app_data| {
                fs::write(app_data.join("runtime/configuration-operation.json"), b"{}").unwrap()
            }),
            ("still needs recovery", |app_data| {
                fs::create_dir(app_data.join("runtime/lifecycle-operations")).unwrap();
                fs::write(
                    app_data.join("runtime/lifecycle-operations/pending.json"),
                    b"{}",
                )
                .unwrap();
            }),
            ("interrupted backup", |app_data| {
                // A journal an interrupted export wrote and no launch has settled yet.
                fs::write(
                    app_data.join("backup-operation.json"),
                    br#"{"version":1,"id":"5b0c8e3e-3b8e-4c4c-9a0b-1f0f5f2d2b77","archive":{"name":"dev.silo-backup","archivePath":"/exports/dev.silo-backup","completedLabel":"In progress","size":"Unknown","destination":"/exports","computers":["dev"]},"request":{"kind":"backup","names":["dev"],"computers":[],"running":[]},"cancelled":false,"terminal":null}"#,
                )
                .unwrap()
            }),
            // An unreadable journal is set aside only once nothing else refuses: it never
            // changes the data directory of a migration that does not start.
            ("redirected", |app_data| {
                fs::write(app_data.join("backup-operation.json"), b"{not json").unwrap();
                std::os::unix::fs::symlink(app_data.join("elsewhere"), app_data.join(CONVERTED))
                    .unwrap()
            }),
            ("redirected", |app_data| {
                std::os::unix::fs::symlink(app_data.join("elsewhere"), app_data.join(CONVERTED))
                    .unwrap()
            }),
        ];
        for (expected, arrange) in cases {
            let (dir, paths) = previous_generation();
            arrange(dir.path());
            let before = tree(dir.path());
            let runner = StagedRuntime::new(dir.path(), "Stopped");
            let error = convert_with(&runner, dir.path(), &paths, &|_| Ok(())).unwrap_err();
            assert!(error.contains(expected), "{expected}: {error}");
            assert_eq!(tree(dir.path()), before, "{expected}");
            assert!(runner.calls.lock().unwrap().is_empty(), "{expected}");
        }
    }

    #[test]
    fn conversion_replaces_a_prior_staged_attempt_and_commits_before_quarantine() {
        let (dir, paths) = previous_generation();
        let app_data = dir.path();
        fs::create_dir_all(app_data.join(CONVERTED).join("leftover")).unwrap();
        fs::write(
            app_data.join("backup-history.json"),
            b"previous backup history",
        )
        .unwrap();
        fs::write(
            app_data.join("runtime/volumes/dev/.silo-configuration-owner"),
            COMPUTER_ID,
        )
        .unwrap();
        let old_before = tree(&app_data.join("runtime"));
        let steps = Mutex::new(Vec::new());
        let runner = StagedRuntime::new(app_data, "Stopped");
        convert_with(&runner, app_data, &paths, &|step| {
            steps.lock().unwrap().push(match step {
                Step::Copying => "copying".to_string(),
                Step::Converting { index, total } => format!("converting {} of {total}", index + 1),
                Step::Converted { index, total } => format!("converted {} of {total}", index + 1),
            });
            Ok(())
        })
        .unwrap();
        assert_eq!(
            *steps.lock().unwrap(),
            ["copying", "converting 1 of 1", "converted 1 of 1"]
        );
        assert_eq!(
            *runner.calls.lock().unwrap(),
            ["inspect", "adopt-disk", "inspect"]
        );
        assert!(
            !app_data.join(CONVERTED).join("leftover").exists(),
            "the prior staged attempt was cleared"
        );
        // Adoption reads the previous generation's disk; no duplicate is staged.
        assert_eq!(*runner.adoption.lock().unwrap(), ["adopt-disk", "dev"]);
        assert!(!app_data
            .join(CONVERTED)
            .join("volumes/dev/workspace.raw")
            .exists());
        assert_eq!(
            fs::read(
                app_data
                    .join(CONVERTED)
                    .join("volumes/dev/.silo-configuration-owner")
            )
            .unwrap(),
            COMPUTER_ID.as_bytes()
        );
        assert_eq!(
            selected_runtime_storage(app_data).unwrap(),
            app_data.join(CONVERTED)
        );
        // The original generation is unchanged apart from the quarantined backup history.
        let mut old_after = tree(&app_data.join("runtime"));
        assert_eq!(
            old_after
                .remove(Path::new("before-checkpoints-backup-history.json"))
                .unwrap(),
            b"previous backup history"
        );
        assert_eq!(old_after, old_before);
    }

    #[test]
    fn conversion_accepts_crashed_computers_without_starting_guest_code() {
        let (dir, paths) = previous_generation();
        let runner = StagedRuntime::new(dir.path(), "Crashed");
        let original = tree(&dir.path().join("runtime"));
        convert_with(&runner, dir.path(), &paths, &|_| Ok(())).unwrap();
        assert_eq!(
            *runner.calls.lock().unwrap(),
            ["inspect", "adopt-disk", "inspect"]
        );
        assert_eq!(tree(&dir.path().join("runtime")), original);
        assert_eq!(
            selected_runtime_storage(dir.path()).unwrap(),
            dir.path().join(CONVERTED)
        );
    }

    #[test]
    fn conversion_rejects_active_and_unknown_computer_states() {
        for status in ["Running", "Starting", "Draining", "Paused", "Unknown"] {
            let (dir, paths) = previous_generation();
            let runner = StagedRuntime::new(dir.path(), status);
            let original = tree(&dir.path().join("runtime"));
            let error = convert_with(&runner, dir.path(), &paths, &|_| Ok(())).unwrap_err();
            assert!(
                error.contains("Stop it before retrying"),
                "{status}: {error}"
            );
            assert_eq!(*runner.calls.lock().unwrap(), ["inspect"], "{status}");
            assert_eq!(tree(&dir.path().join("runtime")), original, "{status}");
        }
    }

    #[test]
    fn conversion_refuses_a_missing_workspace_disk_before_adoption() {
        let (dir, paths) = previous_generation();
        fs::remove_file(dir.path().join("runtime/volumes/dev/workspace.raw")).unwrap();
        let runner = StagedRuntime::new(dir.path(), "Stopped");
        let error = convert_with(&runner, dir.path(), &paths, &|_| Ok(())).unwrap_err();
        assert!(error.contains("workspace disk is unavailable"), "{error}");
        assert_eq!(*runner.calls.lock().unwrap(), ["inspect"]);
        assert_eq!(
            selected_runtime_storage(dir.path()).unwrap(),
            dir.path().join("runtime")
        );
    }

    #[test]
    fn launch_finishes_a_conversion_that_stopped_between_commit_and_quarantine() {
        let (dir, paths) = previous_generation();
        let app_data = dir.path();
        fs::write(
            app_data.join("backup-history.json"),
            b"previous backup history",
        )
        .unwrap();
        // Make the quarantine step fail right after the commit, as a crash would stop it.
        fs::write(
            app_data.join("runtime/before-checkpoints-backup-history.json"),
            b"stale",
        )
        .unwrap();
        let mut progress = fresh("running", 1);
        let recorded = Mutex::new(progress.clone());
        let runner = StagedRuntime::new(app_data, "Stopped");
        assert!(convert_with(&runner, app_data, &paths, &|step| {
            apply_step(&mut recorded.lock().unwrap(), &step);
            Ok(())
        })
        .is_err());
        assert_eq!(
            selected_runtime_storage(app_data).unwrap(),
            app_data.join(CONVERTED),
            "the commit came first"
        );
        assert!(app_data.join("backup-history.json").exists());
        progress = recorded.into_inner().unwrap();
        write(&app_data.join(FILE), &progress).unwrap();
        fs::remove_file(app_data.join("runtime/before-checkpoints-backup-history.json")).unwrap();
        let recovered = initial(&app_data.join(FILE), app_data).unwrap();
        assert_eq!(recovered.status, "complete");
        assert_eq!(recovered.stage, "Migration complete");
        assert!(!app_data.join("backup-history.json").exists());
        assert_eq!(
            fs::read(app_data.join("runtime/before-checkpoints-backup-history.json")).unwrap(),
            b"previous backup history"
        );
    }

    #[test]
    fn retry_after_commit_preserves_verified_generation_until_quarantine_finishes() {
        let (dir, paths) = previous_generation();
        let app_data = dir.path();
        let history = app_data.join("backup-history.json");
        let obstruction = app_data.join("runtime/before-checkpoints-backup-history.json");
        fs::write(&history, b"previous backup history").unwrap();
        fs::write(&obstruction, b"stale").unwrap();
        let recorded = Mutex::new(fresh("running", 1));
        let runner = StagedRuntime::new(app_data, "Stopped");
        let error = convert_with(&runner, app_data, &paths, &|step| {
            apply_step(&mut recorded.lock().unwrap(), &step);
            Ok(())
        })
        .unwrap_err();
        let mut progress = recorded.into_inner().unwrap();
        record_migration_failure(&mut progress, error);
        write(&app_data.join(FILE), &progress).unwrap();
        let committed = tree(&app_data.join(CONVERTED));
        let calls = runner.calls.lock().unwrap().clone();

        // Retry must finish the commit, even while quarantine remains blocked.
        prepare_retry(&mut progress, app_data).unwrap();
        write(&app_data.join(FILE), &progress).unwrap();
        let error = convert_with(&runner, app_data, &paths, &|_| {
            Err("Conversion must not run again after selection.".into())
        })
        .unwrap_err();
        assert!(
            app_data.join(CONVERTED).is_dir(),
            "Retry deleted committed storage"
        );
        assert_eq!(tree(&app_data.join(CONVERTED)), committed);
        assert_eq!(*runner.calls.lock().unwrap(), calls);
        assert!(error.contains("could not be isolated"), "{error}");
        assert_eq!(progress.migrated_count, 1);
        record_migration_failure(&mut progress, error);

        fs::remove_file(&obstruction).unwrap();
        prepare_retry(&mut progress, app_data).unwrap();
        write(&app_data.join(FILE), &progress).unwrap();
        convert_with(&runner, app_data, &paths, &|_| {
            Err("Conversion must not run again after selection.".into())
        })
        .unwrap();
        assert_eq!(tree(&app_data.join(CONVERTED)), committed);
        assert_eq!(*runner.calls.lock().unwrap(), calls);
        let recovered = initial(&app_data.join(FILE), app_data).unwrap();
        assert_eq!(recovered.status, "complete");
        assert_eq!(recovered.migrated_count, 1);
        assert_eq!(fs::read(obstruction).unwrap(), b"previous backup history");
    }

    #[test]
    fn staged_copy_is_independent_and_omits_live_runtime_handles() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("runtime");
        let stage = dir.path().join(CONVERTED);
        fs::create_dir(&source).unwrap();
        fs::create_dir(source.join("run")).unwrap();
        fs::write(source.join("run/old.lock"), b"old").unwrap();
        fs::write(source.join("update-resume.json"), b"old starts").unwrap();
        fs::create_dir(source.join("sandboxes")).unwrap();
        fs::write(source.join("sandboxes/upper.ext4"), b"old root").unwrap();
        fs::create_dir_all(source.join("volumes/dev")).unwrap();
        fs::write(source.join("volumes/dev/workspace.raw"), b"computer").unwrap();
        fs::write(source.join("volumes/dev/other.raw"), b"other").unwrap();
        copy_runtime_tree(&source, &stage, &[source.join("volumes/dev/workspace.raw")]).unwrap();
        assert!(!stage.join("run").exists());
        assert!(!stage.join("update-resume.json").exists());
        assert!(!stage.join("volumes/dev/workspace.raw").exists());
        assert_eq!(
            fs::read(stage.join("volumes/dev/other.raw")).unwrap(),
            b"other"
        );
        fs::write(stage.join("sandboxes/upper.ext4"), b"new root").unwrap();
        assert_eq!(
            fs::read(source.join("sandboxes/upper.ext4")).unwrap(),
            b"old root"
        );
    }
}
