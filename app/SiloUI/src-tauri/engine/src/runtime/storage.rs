//! Computer discard maintenance. Never deletes guest files or starts a computer.
use super::*;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::sync::atomic::{AtomicUsize, Ordering};

const DAY: u64 = 24 * 60 * 60;
const WEEK: u64 = 7 * DAY;
pub(super) const TRIM_BUDGET: Duration = Duration::from_secs(15);
const STATS: &str = "set -eu; test \"$(findmnt -rn -T /workspace -o TARGET,FSTYPE)\" = '/workspace ext4'; LC_ALL=C df -B1 --output=used,size /workspace | tail -n 1";
// The inner timeout survives a lost host client. No shell expansion of user input.
const TRIM: &str = "set -eu; test \"$(findmnt -rn -T /workspace -o TARGET,FSTYPE)\" = '/workspace ext4'; exec timeout -k 1s \"$1\" fstrim /workspace";
type VerifiedStarts = HashMap<(PathBuf, String), String>;
static VERIFIED_STARTS: OnceLock<Mutex<VerifiedStarts>> = OnceLock::new();

fn verified_starts() -> &'static Mutex<VerifiedStarts> {
    VERIFIED_STARTS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn verified_worker(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
    observed: &InspectedSandbox,
) -> bool {
    observed
        .runtime_instance_id
        .as_ref()
        .is_some_and(|instance| {
            verified_starts().lock().is_ok_and(|starts| {
                starts.get(&(paths.home.clone(), configuration.id().into())) == Some(instance)
            })
        })
}

const HISTORY_LIMIT: usize = 50;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReclaimEntry {
    at: u64,
    trigger: String,
    reclaimed_bytes: Option<u64>,
    error: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    #[serde(default)]
    history: Vec<ReclaimEntry>,
    last_trim_at: Option<u64>,
    last_attempt_at: Option<u64>,
    last_reclaimed_bytes: Option<u64>,
    last_error: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageState {
    history: Vec<ReclaimEntry>,
    /// Host allocation of the workspace disk and its layers; `None` when it could not be found.
    workspace_host_bytes: Option<u64>,
    /// Host allocation of the runtime root disks; `None` when the computer directory is missing.
    runtime_host_bytes: Option<u64>,
    /// Host allocation of the computer's checkpoints; `None` when it could not be measured.
    checkpoint_host_bytes: Option<u64>,
    checkpoint_count: usize,
    workspace_used_bytes: Option<u64>,
    workspace_capacity_bytes: Option<u64>,
    last_reclaimed_bytes: Option<u64>,
    last_trim_at: Option<u64>,
    last_error: Option<String>,
}

fn failure(message: &str) -> RuntimeError {
    RuntimeError::Unavailable(message.into())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn record_path(paths: &RuntimePaths, id: &str) -> PathBuf {
    paths
        .metadata
        .with_file_name("storage-maintenance")
        .join(format!("{id}.json"))
}
fn load(paths: &RuntimePaths, id: &str) -> Result<Record, RuntimeError> {
    let file = match File::open(record_path(paths, id)) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Record::default()),
        Err(_) => return Err(failure("Storage maintenance history could not be read.")),
    };
    let mut record: Record = serde_json::from_reader(file.take(65536))
        .map_err(|_| failure("Storage maintenance history is invalid; it was preserved."))?;
    if record.history.is_empty() {
        if let Some(at) = record.last_trim_at {
            record.history.push(ReclaimEntry {
                at,
                trigger: "legacy".into(),
                reclaimed_bytes: record.last_reclaimed_bytes,
                error: None,
                extra: serde_json::Map::new(),
            });
        }
    }
    record.history.truncate(HISTORY_LIMIT);
    Ok(record)
}
fn save(paths: &RuntimePaths, id: &str, record: &Record) -> Result<(), RuntimeError> {
    let target = record_path(paths, id);
    let directory = target.parent().unwrap();
    fs::create_dir_all(directory)
        .map_err(|_| failure("Storage maintenance history could not be saved."))?;
    let mut file = tempfile::NamedTempFile::new_in(directory)
        .map_err(|_| failure("Storage maintenance history could not be saved."))?;
    serde_json::to_writer(&mut file, record)
        .map_err(|_| failure("Storage maintenance history could not be saved."))?;
    file.as_file()
        .sync_all()
        .map_err(|_| failure("Storage maintenance history could not be synced."))?;
    file.persist(&target)
        .map_err(|_| failure("Storage maintenance history could not be saved."))?;
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|_| failure("Storage maintenance history could not be synced."))?;
    Ok(())
}

pub(super) fn forget_removed(paths: &RuntimePaths, id: &str) {
    if uuid::Uuid::parse_str(id).is_ok() {
        let _ = fs::remove_file(record_path(paths, id));
    }
    if let Ok(mut starts) = verified_starts().lock() {
        starts.remove(&(paths.home.clone(), id.into()));
    }
}
fn due(record: &Record, at: u64, interval: u64) -> bool {
    // Also throttle failures and interrupted attempts: unsupported disks must not
    // be retried every monitor tick or every Quit. Manual requests bypass this.
    record
        .last_attempt_at
        .is_none_or(|last| at.saturating_sub(last) >= DAY)
        && record
            .last_trim_at
            .is_none_or(|last| at.saturating_sub(last) >= interval)
}
fn computer_configuration(
    paths: &RuntimePaths,
    id: &str,
) -> Result<ComputerConfiguration, RuntimeError> {
    uuid::Uuid::parse_str(id).map_err(|_| failure("Invalid computer identity."))?;
    read_metadata(&paths.metadata)?
        .computers
        .into_iter()
        .find(|m| m.id() == id)
        .ok_or_else(|| {
            failure("Storage maintenance is available only for a configured local computer.")
        })
}
fn verify(
    configuration: &ComputerConfiguration,
    observed: &InspectedSandbox,
) -> Result<(), RuntimeError> {
    validate_name(configuration.name())?;
    ensure_managed(observed)?;
    if observed.name != configuration.name()
        || observed
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            != Some(configuration.id())
    {
        return Err(failure(
            "The computer identity changed. No storage operation was performed.",
        ));
    }
    Ok(())
}
/// The owned `/workspace` volume's directory. A fresh computer keeps one `disk.raw` there; a full
/// checkpoint rolls it onto qcow2 layers, and a computer restored from a checkpoint has only sealed
/// layers and a writable qcow2 head (MicroSandbox `restore/owned.rs`).
fn workspace_disk_dir(paths: &RuntimePaths, configuration: &ComputerConfiguration) -> PathBuf {
    let mut mount_id = String::from("workspace_");
    for byte in Sha256::digest(WORKSPACE_MOUNT.as_bytes()).iter().take(4) {
        use std::fmt::Write as _;
        let _ = write!(mount_id, "{byte:02x}");
    }
    paths
        .home
        .join("sandboxes")
        .join(configuration.name())
        .join("owned-volumes")
        .join(mount_id)
}
const MAX_DISK_FILES: usize = 256;
/// Regular (never symlinked) disk files of the workspace volume; `None` when it is missing.
fn workspace_disk_files(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<Option<Vec<PathBuf>>, RuntimeError> {
    let directory = workspace_disk_dir(paths, configuration);
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(failure("The workspace disk location is not a directory.")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(failure("The workspace disk usage could not be read.")),
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(&directory)
        .map_err(|_| failure("The workspace disk usage could not be read."))?
    {
        let entry = entry.map_err(|_| failure("The workspace disk usage could not be read."))?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| failure("The workspace disk usage could not be read."))?;
        if metadata.is_file() {
            files.push(entry.path());
            if files.len() > MAX_DISK_FILES {
                return Err(failure(
                    "The workspace disk has too many layers to measure.",
                ));
            }
        }
    }
    files.sort();
    Ok(Some(files))
}
fn workspace_host_bytes(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<Option<u64>, RuntimeError> {
    let Some(files) = workspace_disk_files(paths, configuration)? else {
        return Ok(None);
    };
    if files.is_empty() {
        return Ok(None);
    }
    files
        .iter()
        .try_fold(0u64, |total, file| {
            Ok(total.saturating_add(allocated(file)?))
        })
        .map(Some)
}
fn workspace_mount(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
    observed: &InspectedSandbox,
) -> bool {
    let backing_is_file = workspace_disk_files(paths, configuration)
        .ok()
        .flatten()
        .is_some_and(|files| {
            files.iter().any(|file| {
                file.file_name().is_some_and(|name| name == "disk.raw")
                    || file
                        .extension()
                        .is_some_and(|extension| extension == "qcow2")
            })
        });
    let matches = |config: &Value| {
        config
            .get("mounts")
            .and_then(Value::as_array)
            .is_some_and(|mounts| {
                mounts
                    .iter()
                    .filter(|m| m["guest"] == WORKSPACE_MOUNT)
                    .count()
                    == 1
                    && mounts.iter().any(|m| {
                        m["guest"] == WORKSPACE_MOUNT
                            && m["type"] == "Owned"
                            && m.pointer("/storage/kind").and_then(Value::as_str) == Some("disk")
                    })
            })
    };
    backing_is_file
        && matches(&observed.config)
        && observed.active_config.as_ref().is_none_or(matches)
}
fn allocated(path: &Path) -> Result<u64, RuntimeError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(metadata.blocks().saturating_mul(512)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        _ => Err(failure("The computer disk is not a readable regular file.")),
    }
}
/// Root disks live beside the computer record: `upper.ext4`, a flat `rootfs.raw`, and
/// the qcow2 layers a checkpoint or restore adds.
fn runtime_allocated(paths: &RuntimePaths, name: &str) -> Result<Option<u64>, RuntimeError> {
    let dir = paths.home.join("sandboxes").join(name);
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(failure("The runtime disk usage could not be read.")),
    };
    let mut total = 0u64;
    for entry in entries {
        let entry = entry.map_err(|_| failure("The runtime disk usage could not be read."))?;
        if entry.path().extension().is_some_and(|extension| {
            extension == "ext4" || extension == "qcow2" || extension == "raw"
        }) {
            total = total.saturating_add(allocated(&entry.path())?);
        }
    }
    Ok(Some(total))
}
fn guest_args(name: &str, seconds: u64, script: &str) -> Vec<String> {
    vec![
        "exec".into(),
        name.into(),
        "--no-start".into(),
        "--no-tty".into(),
        "--user".into(),
        "root".into(),
        "--timeout".into(),
        format!("{seconds}s"),
        "--".into(),
        "sh".into(),
        "-c".into(),
        script.into(),
        "silo-storage".into(),
    ]
}
fn stats(output: &str) -> Result<(u64, u64), RuntimeError> {
    let values: Vec<_> = output.split_whitespace().collect();
    if values.len() != 2 {
        return Err(failure("The guest returned invalid storage measurements."));
    }
    let used = values[0]
        .parse::<u64>()
        .map_err(|_| failure("Invalid guest storage usage."))?;
    let capacity = values[1]
        .parse::<u64>()
        .map_err(|_| failure("Invalid guest storage capacity."))?;
    if used > capacity || capacity == 0 {
        return Err(failure("Invalid guest storage capacity."));
    }
    Ok((used, capacity))
}
fn state(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
    observed: &InspectedSandbox,
) -> Result<StorageState, RuntimeError> {
    verify(configuration, observed)?;
    let record = load(paths, configuration.id())?;
    let (checkpoint_host_bytes, checkpoint_count) =
        checkpoints::storage_totals(runner, paths, configuration.id(), configuration.name());
    let mut state = StorageState {
        checkpoint_host_bytes,
        checkpoint_count,
        history: record.history,
        workspace_host_bytes: workspace_host_bytes(paths, configuration)?,
        runtime_host_bytes: runtime_allocated(paths, configuration.name())?,
        workspace_used_bytes: None,
        workspace_capacity_bytes: None,
        last_reclaimed_bytes: record.last_reclaimed_bytes,
        last_trim_at: record.last_trim_at,
        last_error: record.last_error,
    };
    if observed.status.eq_ignore_ascii_case("running")
        && !verified_worker(paths, configuration, observed)
    {
        let restart = "Restart this computer in Silo before reclaiming space so it uses the corrected storage runtime.";
        state.last_error = Some(
            state
                .last_error
                .take()
                .map_or_else(|| restart.into(), |error| format!("{error} {restart}")),
        );
    }
    if observed.status.eq_ignore_ascii_case("running")
        && workspace_mount(paths, configuration, observed)
    {
        match runner
            .run(
                paths,
                &guest_args(configuration.name(), 4, STATS),
                Duration::from_secs(5),
            )
            .and_then(|out| stats(&out.stdout))
        {
            Ok((used, capacity)) => {
                state.workspace_used_bytes = Some(used);
                state.workspace_capacity_bytes = Some(capacity);
            }
            Err(_) => {
                state.last_error.get_or_insert_with(|| "Storage usage inside the computer is unavailable. Refresh after the computer finishes starting.".into());
            }
        }
    }
    Ok(state)
}
fn trim(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
    observed: &InspectedSandbox,
    budget: Duration,
    at: u64,
) -> Result<(), RuntimeError> {
    trim_triggered(runner, paths, configuration, observed, budget, at, "manual")
}
fn trim_triggered(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
    observed: &InspectedSandbox,
    budget: Duration,
    at: u64,
    trigger: &str,
) -> Result<(), RuntimeError> {
    verify(configuration, observed)?;
    if !observed.status.eq_ignore_ascii_case("running") {
        return Err(failure(
            "Start the computer before reclaiming unused computer space.",
        ));
    }
    if !workspace_mount(paths, configuration, observed) {
        return Err(failure(
            "The computer does not have the expected workspace disk mounted. No space was reclaimed.",
        ));
    }
    if !verified_worker(paths, configuration, observed) {
        return Err(failure("Restart this computer in Silo before reclaiming space so it uses the corrected storage runtime."));
    }
    let seconds = budget.as_secs();
    if seconds < 4 {
        return Err(failure("No time remains for computer reclamation."));
    }
    let _command_guard = configuration_recovery::command_lock(paths, Duration::ZERO)?;
    // Hold every layer open so a runtime that shortens any of them is caught and repaired.
    let mut disks = Vec::new();
    for disk_path in workspace_disk_files(paths, configuration)?.unwrap_or_default() {
        let disk = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&disk_path)
            .map_err(|_| failure("The owned workspace disk could not be opened safely."))?;
        let original_length = disk
            .metadata()
            .map_err(|_| failure("The owned workspace disk length could not be verified."))?
            .len();
        let qcow2 = disk_path
            .extension()
            .is_some_and(|extension| extension == "qcow2");
        disks.push((disk, original_length, qcow2));
    }
    if disks.is_empty() {
        return Err(failure(
            "The computer does not have the expected workspace disk mounted. No space was reclaimed.",
        ));
    }
    let before = workspace_host_bytes(paths, configuration)?
        .ok_or_else(|| failure("The owned workspace disk could not be measured."))?;
    let mut record = load(paths, configuration.id())?;
    record.last_attempt_at = Some(at);
    record.last_error = Some("The previous computer reclamation did not complete.".into());
    record.history.insert(
        0,
        ReclaimEntry {
            at,
            trigger: trigger.into(),
            reclaimed_bytes: None,
            error: record.last_error.clone(),
            extra: serde_json::Map::new(),
        },
    );
    record.history.truncate(HISTORY_LIMIT);
    save(paths, configuration.id(), &record)?;
    let mut args = guest_args(configuration.name(), seconds - 1, TRIM);
    args.push(format!("{}s", seconds - 3));
    let result = runner.run(paths, &args, budget).map(|_| ());
    // Check every layer even after the first failure, so each shortened one is repaired.
    let length_result = disks
        .iter()
        .map(|(disk, original_length, qcow2)| preserve_length(disk, *original_length, *qcow2))
        .fold(Ok(()), |first: Result<(), RuntimeError>, next| {
            first.and(next)
        });
    let length_error = length_result.as_ref().err().map(ToString::to_string);
    let result = length_result.and(result);
    match result {
        Ok(_) => {
            let after = workspace_host_bytes(paths, configuration)?
                .ok_or_else(|| failure("The reclaimed disk could not be measured."))?;
            record.last_trim_at = Some(now());
            record.last_reclaimed_bytes = Some(before.saturating_sub(after));
            record.last_error = None;
        }
        Err(_) => {
            record.last_error = Some(length_error.unwrap_or_else(|| "Computer reclamation failed or exceeded its time limit. Your files were preserved; the disk may not support reclamation.".into()));
        }
    }
    record.history[0].error = record.last_error.clone();
    record.history[0].reclaimed_bytes = if result.is_ok() {
        record.last_reclaimed_bytes
    } else {
        None
    };
    save(paths, configuration.id(), &record)?;
    result.map_err(|_| failure(record.last_error.as_deref().unwrap()))
}

fn preserve_length(disk: &File, original_length: u64, qcow2: bool) -> Result<(), RuntimeError> {
    let length = disk
        .metadata()
        .map_err(|_| failure("The workspace disk length could not be verified."))?
        .len();
    if length < original_length {
        disk.set_len(original_length).and_then(|_| disk.sync_all())
            .map_err(|_| failure("The runtime shortened the workspace disk and its length could not be restored. Keep the computer stopped and repair its disk."))?;
        return Err(failure("The runtime shortened the workspace disk; its original length was restored. Update the runtime before reclaiming again."));
    }
    // A qcow2 file grows as the running guest allocates host clusters; its file
    // length is independent of the virtual disk capacity.
    if length != original_length && !qcow2 {
        return Err(failure(
            "The workspace disk size changed during reclamation.",
        ));
    }
    Ok(())
}

pub(super) fn before_stop(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    observed: &InspectedSandbox,
) {
    automatic(
        runner,
        paths,
        observed,
        DAY,
        shutdown::maintenance_budget().min(TRIM_BUDGET),
    );
}

pub(super) fn after_start(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    observed: &InspectedSandbox,
) {
    // Quit may have begun while the bounded start command held the mutation lock.
    // Do not start new maintenance after its shared deadline has expired.
    if shutdown::ensure_accepting_operations().is_err() {
        return;
    }
    let Some(id) = observed
        .config
        .pointer("/labels/silo.machine-id")
        .and_then(Value::as_str)
    else {
        return;
    };
    // A runtime that lacks the capability is reported by name, not skipped silently.
    let Ok(Some(instance)) = running_instance_id(paths, observed) else {
        return;
    };
    if !observed.status.eq_ignore_ascii_case("running") {
        return;
    }
    let Ok(configuration) = computer_configuration(paths, id) else {
        return;
    };
    if verify(&configuration, observed).is_err()
        || !workspace_mount(paths, &configuration, observed)
    {
        return;
    }
    if !runner
        .run(
            paths,
            &["--silo-storage-protocol".into()],
            Duration::from_secs(1),
        )
        .is_ok_and(|out| out.stdout.trim() == "1")
    {
        return;
    }
    if let Ok(mut starts) = verified_starts().lock() {
        starts.insert((paths.home.clone(), id.into()), instance);
    }
    automatic(
        runner,
        paths,
        observed,
        WEEK,
        shutdown::maintenance_budget().min(TRIM_BUDGET),
    );
}

fn automatic(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    observed: &InspectedSandbox,
    interval: u64,
    budget: Duration,
) {
    let Some(id) = observed
        .config
        .pointer("/labels/silo.machine-id")
        .and_then(Value::as_str)
    else {
        return;
    };
    let Ok(configuration) = computer_configuration(paths, id) else {
        return;
    };
    let Ok(record) = load(paths, id) else {
        return;
    };
    if !due(&record, now(), interval) || !workspace_mount(paths, &configuration, observed) {
        return;
    }
    // Maintenance failure must never veto stopping a computer.
    let _ = trim_triggered(
        runner,
        paths,
        &configuration,
        observed,
        budget,
        now(),
        if interval == DAY {
            "beforeStop"
        } else {
            "afterStart"
        },
    );
}

fn periodic(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    unresolved: &HashMap<String, RuntimeError>,
) -> Result<bool, RuntimeError> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let mut computers = read_metadata(&paths.metadata)?.computers;
    if computers.is_empty() {
        return Ok(false);
    }
    let offset = NEXT.fetch_add(1, Ordering::Relaxed) % computers.len();
    computers.rotate_left(offset);
    let deadline = Instant::now() + TRIM_BUDGET;
    for configuration in computers {
        if unresolved.contains_key(configuration.id()) {
            continue;
        }
        if shutdown::ensure_accepting_operations().is_err() {
            return Ok(false);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.as_secs() < 4 {
            return Ok(false);
        }
        let Ok(record) = load(paths, configuration.id()) else {
            continue;
        };
        if !due(&record, now(), WEEK) {
            continue;
        }
        let Ok(output) = runner.run(
            paths,
            &[
                "inspect".into(),
                configuration.name().into(),
                "--format".into(),
                "json".into(),
            ],
            remaining.min(READ_TIMEOUT),
        ) else {
            continue;
        };
        let Ok(observed) = serde_json::from_str::<InspectedSandbox>(&output.stdout) else {
            continue;
        };
        if verify(&configuration, &observed).is_err()
            || !workspace_mount(paths, &configuration, &observed)
        {
            continue;
        }
        if !observed.status.eq_ignore_ascii_case("running") {
            continue;
        }
        let budget = deadline
            .saturating_duration_since(Instant::now())
            .min(shutdown::maintenance_budget());
        // One computer per tick keeps the global mutation lock available for user actions.
        trim_triggered(
            runner,
            paths,
            &configuration,
            &observed,
            budget,
            now(),
            "scheduled",
        )?;
        return Ok(true);
    }
    Ok(false)
}
fn maintenance_tick(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
) -> Result<bool, RuntimeError> {
    let recovery = checkpoints::recover_interrupted(runner, paths)?;
    if let Some(failure) = recovery.cleanup_error {
        eprintln!("Deleted checkpoint data was kept: {failure}");
    }
    for (id, failure) in &recovery.unresolved {
        eprintln!("Interrupted checkpoint data for computer {id} was kept: {failure}");
    }
    periodic(runner, paths, &recovery.unresolved)
}

pub fn start_monitor(host: Arc<dyn RuntimeHost>) {
    thread::spawn(move || {
        loop {
            // Periodic background work skips whenever any operation is active or waiting.
            if let Ok(_guard) = OPERATIONS.try_device_hidden("Trimming computer storage") {
                if shutdown::ensure_accepting_operations().is_ok() {
                    // No paths exist while the storage migration is unfinished, so
                    // neither step below can reach the previous generation.
                    if let Ok(paths) = runtime_paths(host.as_ref()) {
                        if let Err(failure) = maintenance_tick(&ProcessRunner, &paths) {
                            eprintln!("Storage maintenance could not finish: {failure}");
                        }
                    }
                }
            }
            thread::sleep(Duration::from_secs(60));
        }
    });
}

pub fn workspace_storage(
    host: &dyn RuntimeHost,
    id: &str,
    reclaim: bool,
) -> Result<StorageState, String> {
    // Reclaim rewrites shared disk state and waits its turn; a plain read does
    // not take the gate so status stays available during other operations.
    let _guard = if reclaim {
        Some(
            OPERATIONS
                .kind(super::operation_gate::OperationKind::StorageReclaim)
                .device("Reclaiming computer storage")
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    shutdown::ensure_accepting_operations()?;
    let paths = runtime_paths(host)?;
    storage_with(&ProcessRunner, &paths, id, reclaim).map_err(|error| safe_activity_error(&error))
}

fn storage_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    id: &str,
    reclaim: bool,
) -> Result<StorageState, RuntimeError> {
    let configuration = computer_configuration(paths, id)?;
    let absent = if checkpoints::pending_view(paths, configuration.id(), false)? {
        let exists = list_managed(runner, paths)?
            .iter()
            .any(|entry| entry.name == configuration.name());
        checkpoints::pending_view(paths, configuration.id(), exists)?
    } else {
        false
    };
    if absent {
        if reclaim {
            return Err(failure(&format!("Start {} first.", configuration.name())));
        }
        let record = load(paths, configuration.id())?;
        let (checkpoint_host_bytes, checkpoint_count) =
            checkpoints::storage_totals(runner, paths, configuration.id(), configuration.name());
        return Ok(StorageState {
            checkpoint_host_bytes,
            checkpoint_count,
            history: record.history,
            workspace_host_bytes: Some(0),
            runtime_host_bytes: Some(0),
            workspace_used_bytes: None,
            workspace_capacity_bytes: None,
            last_reclaimed_bytes: record.last_reclaimed_bytes,
            last_trim_at: record.last_trim_at,
            last_error: record.last_error,
        });
    }
    let observed = inspect_computer(runner, paths, configuration.name())?;
    verify(&configuration, &observed)?;
    if reclaim {
        if checkpoints::needs_explicit_start(paths, configuration.id())? {
            return Err(failure("The computer has an unfinished restore. Retry Start before reclaiming computer space."));
        }
        trim(runner, paths, &configuration, &observed, TRIM_BUDGET, now())?;
    }
    state(runner, paths, &configuration, &observed)
}

#[cfg(test)]
mod tests;
