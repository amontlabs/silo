use super::*;
use silo_engine::context::{Audience, Event};
use std::os::fd::AsRawFd;

const OWNER: &str = ".silo-configuration-owner";
/// Set once startup recovery has run (successfully or not) in this process.
static STARTUP_SETTLED: AtomicBool = AtomicBool::new(false);
/// Configuration attempts currently running in this process.
static LIVE_ATTEMPTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Marks a configuration attempt as live for as long as it is held. A saved
/// journal blocks snapshots only while an attempt is live (or before startup
/// recovery has run), so an attempt that fails in-session no longer freezes
/// the computer list until relaunch.
#[must_use]
pub(super) struct Attempt(());
impl Drop for Attempt {
    fn drop(&mut self) {
        LIVE_ATTEMPTS.fetch_sub(1, Ordering::SeqCst);
    }
}
pub(super) fn attempt() -> Attempt {
    LIVE_ATTEMPTS.fetch_add(1, Ordering::SeqCst);
    Attempt(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    version: u32,
    pub(super) previous: ComputerConfigurationRequest,
    pub(super) request: ComputerConfigurationRequest,
}

fn path(paths: &RuntimePaths) -> PathBuf {
    paths
        .metadata
        .with_file_name("configuration-operation.json")
}
fn failure(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::Unavailable(format!("Could not recover computer configuration: {error}"))
}

fn publish_recovery_progress(
    context: &dyn silo_engine::EngineContext,
    event: ComputerConfigurationProgress,
) {
    if let Ok(payload) = serde_json::to_value(event) {
        silo_engine::context::publish_to_main_window(
            context,
            Event::new("silo://computer-configuration-progress", payload),
        );
    }
}

pub(super) fn load(paths: &RuntimePaths) -> Result<Option<Journal>, RuntimeError> {
    let file = match File::open(path(paths)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failure(error)),
    };
    let journal: Journal = serde_json::from_reader(file.take(MAX_OUTPUT_BYTES)).map_err(failure)?;
    if journal.version != 1 {
        return Err(failure("Unsupported saved operation; it was preserved."));
    }
    // First-run metadata is legitimately empty; submitted configurations are not.
    if !journal.previous.computers.is_empty() || journal.previous.schema_version != 1 {
        validate_request(&journal.previous)?;
    }
    validate_request(&journal.request)?;
    Ok(Some(journal))
}

/// Include computers created before their metadata commit without replaying setup on Quit.
pub(super) fn shutdown_computers(
    paths: &RuntimePaths,
) -> Result<Vec<ComputerConfiguration>, RuntimeError> {
    let Some(journal) = load(paths)? else {
        return Ok(Vec::new());
    };
    let mut computers = journal.previous.computers;
    for configuration in journal.request.computers {
        if !computers.iter().any(|old| old.id() == configuration.id()) {
            computers.push(configuration);
        }
    }
    Ok(computers)
}

/// The target configuration of an interrupted attempt, if one is pending. Used by the
/// retry command to resume that attempt against current state without the UI resending
/// the whole list.
pub(super) fn pending_request(
    paths: &RuntimePaths,
) -> Result<Option<ComputerConfigurationRequest>, RuntimeError> {
    Ok(load(paths)?.map(|journal| journal.request))
}

/// Whether `name`, a runtime computer no saved computer has, is one the pending configuration
/// is creating: it is new in the request and carries the label of the id requested for it.
pub(super) fn is_being_created(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    journal: &Journal,
    name: &str,
) -> Result<bool, RuntimeError> {
    let Some(requested) = journal.request.computers.iter().find(|computer| {
        computer.name() == name
            && !journal
                .previous
                .computers
                .iter()
                .any(|old| old.id() == computer.id())
    }) else {
        return Ok(false);
    };
    let inspected = inspect_computer(runner, paths, name)?;
    Ok(inspected
        .config
        .pointer("/labels/silo.machine-id")
        .and_then(Value::as_str)
        == Some(requested.id()))
}

pub(super) fn begin(
    paths: &RuntimePaths,
    request: &ComputerConfigurationRequest,
) -> Result<(), RuntimeError> {
    if let Some(saved) = load(paths)? {
        if saved.request == *request {
            return Ok(());
        }
        return Err(failure("An interrupted configuration is pending. Relaunch Silo to resume it before making another change."));
    }
    let journal = Journal {
        version: 1,
        previous: read_metadata(&paths.metadata)?,
        request: request.clone(),
    };
    write(paths, &journal)
}

fn write(paths: &RuntimePaths, journal: &Journal) -> Result<(), RuntimeError> {
    let parent = paths
        .metadata
        .parent()
        .ok_or_else(|| failure("Missing storage directory."))?;
    fs::create_dir_all(parent).map_err(failure)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(failure)?;
    serde_json::to_writer(&mut file, &journal).map_err(failure)?;
    file.as_file().sync_all().map_err(failure)?;
    file.persist(path(paths)).map_err(failure)?;
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(failure)
}

// Reserve new storage before formatting. The marker permits cleanup only of
// files created for this exact, not-yet-committed computer ID.
pub(super) fn claim(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    use std::os::unix::fs::PermissionsExt;
    let Some(journal) = load(paths)? else {
        return Ok(());
    };
    if journal
        .previous
        .computers
        .iter()
        .any(|old| old.id() == configuration.id())
    {
        return Ok(());
    }
    let folder = paths.volumes.join(configuration.name());
    fs::create_dir_all(&paths.volumes).map_err(failure)?;
    if folder.exists() {
        // Reuse only an empty directory, including leftovers from older builds.
        fs::remove_dir(&folder).map_err(|_| {
            failure("New computer storage is already occupied. No existing files were changed.")
        })?;
    }
    let stage = tempfile::Builder::new()
        .prefix(".configuration-claim-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(&paths.volumes)
        .map_err(failure)?;
    let mut marker = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(stage.path().join(OWNER))
        .map_err(failure)?;
    marker
        .write_all(configuration.id().as_bytes())
        .and_then(|_| marker.sync_all())
        .map_err(failure)?;
    File::open(stage.path())
        .and_then(|file| file.sync_all())
        .map_err(failure)?;
    fs::rename(stage.path(), &folder).map_err(failure)?;
    File::open(&paths.volumes)
        .and_then(|file| file.sync_all())
        .map_err(failure)
}

pub(super) fn finish(paths: &RuntimePaths) -> Result<(), RuntimeError> {
    if let Some(journal) = load(paths)? {
        for configuration in &journal.request.computers {
            let marker = paths.volumes.join(configuration.name()).join(OWNER);
            if fs::read_to_string(&marker).ok().as_deref() == Some(configuration.id()) {
                fs::remove_file(&marker).map_err(failure)?;
                File::open(marker.parent().unwrap())
                    .and_then(|file| file.sync_all())
                    .map_err(failure)?;
            }
        }
        fs::remove_file(path(paths)).map_err(failure)?;
        File::open(paths.metadata.parent().unwrap())
            .and_then(|file| file.sync_all())
            .map_err(failure)?;
    }
    Ok(())
}

const CHILD_RECORD_PREFIX: &str = ".silo-configuration-worker.child-";
/// Written before a runtime command is started and replaced by the command's own record once
/// its pid is known, so there is no moment where a started command has no record at all.
const PENDING_RECORD_PREFIX: &str = ".silo-configuration-worker.child-pending-";
/// How long a new holder waits for a command whose engine died between starting it and
/// recording it. Nothing names that command, so this is a fixed bound, not a wait for an exit.
const PENDING_WAIT: Duration = Duration::from_secs(3);
/// Added to a record's command timeout before the record is called stale.
const RECORD_SLACK: Duration = Duration::from_secs(10);
static RECORD_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Makes the next `record_child` fail, to exercise the caller's recovery.
#[cfg(test)]
pub(crate) static FAIL_CHILD_RECORD: AtomicBool = AtomicBool::new(false);

/// The lock lives only in the engine's descriptor: the runtime commands it starts do not
/// inherit it, so the computers those commands leave running never hold it, and it ends
/// with the engine. A parent-held command lock releases the shared flock even when a
/// concurrent fork temporarily inherited its descriptor before exec closed it.
pub(crate) struct CommandLock {
    file: File,
    home: PathBuf,
    child_record: Option<PathBuf>,
    record_is_pending: bool,
    release_on_drop: bool,
}

impl CommandLock {
    /// A shutdown worker shares the parent's flock. Only the parent unlocks after
    /// all workers have joined.
    pub(crate) fn duplicate_for_shutdown(&self) -> Result<Self, RuntimeError> {
        Ok(Self {
            file: self.file.try_clone().map_err(failure)?,
            home: self.home.clone(),
            child_record: None,
            record_is_pending: false,
            release_on_drop: false,
        })
    }

    /// Note that this engine is about to start the runtime command this lock covers. The
    /// record names the engine, so a new holder can tell it from a command that is running.
    pub(crate) fn begin_command(&mut self, timeout: Duration) -> Result<(), RuntimeError> {
        let record = self.home.join(format!(
            "{PENDING_RECORD_PREFIX}{}-{}",
            std::process::id(),
            RECORD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        write_record(&self.home, &record, std::process::id(), timeout).map_err(failure)?;
        self.child_record = Some(record);
        self.record_is_pending = true;
        Ok(())
    }

    /// Note the runtime command this lock covers, so that a new engine started after this one
    /// died waits for that command to exit before it takes the lock itself. The record names
    /// the `msb` process only: a helper that command detaches (`create` and `restore` detach
    /// the computer's VM by design) is not covered and never holds the lock.
    pub(crate) fn record_child(&mut self, pid: u32, timeout: Duration) -> Result<(), RuntimeError> {
        let record = self.home.join(format!("{CHILD_RECORD_PREFIX}{pid}"));
        #[cfg(test)]
        if FAIL_CHILD_RECORD.swap(false, Ordering::SeqCst) {
            return Err(failure("the record could not be written"));
        }
        write_record(&self.home, &record, pid, timeout).map_err(failure)?;
        if let Some(pending) = self.child_record.replace(record) {
            if self.record_is_pending {
                let _ = fs::remove_file(pending);
            }
        }
        self.record_is_pending = false;
        Ok(())
    }

    /// The command exited, so nothing outlives this lock on its behalf.
    pub(crate) fn mark_child_exited(&mut self) {
        if let Some(record) = self.child_record.take() {
            let _ = fs::remove_file(record);
        }
        self.record_is_pending = false;
    }
}

fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Writes a record through a temporary file and a rename, so a reader never finds a partly
/// written one. `subject` is the process whose start time is stored; when that is not
/// available the record still carries when it was written and how long its command may run.
fn write_record(home: &Path, record: &Path, subject: u32, timeout: Duration) -> std::io::Result<()> {
    use std::io::Write;
    let mut text = String::new();
    if let Some(start) = process_start(subject) {
        text.push_str(&format!("start={start}\n"));
    }
    text.push_str(&format!(
        "written={}\ntimeout={}\n",
        unix_seconds(SystemTime::now()),
        timeout.as_secs()
    ));
    let temporary = home.join(format!(
        ".silo-configuration-worker.record-{}-{}.tmp",
        std::process::id(),
        RECORD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = File::create(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, record)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

impl AsRawFd for CommandLock {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.file.as_raw_fd()
    }
}

impl Drop for CommandLock {
    fn drop(&mut self) {
        if self.release_on_drop {
            // A pending record that was never replaced belongs to a command that did not start.
            if self.record_is_pending {
                if let Some(record) = self.child_record.take() {
                    let _ = fs::remove_file(record);
                }
            }
            // SAFETY: this descriptor remains owned by self until after Drop.
            // LOCK_UN also releases copies inherited by unrelated forks.
            unsafe {
                libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}

/// When the process started, in a unit that is only compared with itself. It tells a pid
/// that was reused by another program from the process a record was written for.
fn process_start(pid: u32) -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: info is a writable buffer of exactly `size` bytes.
        let written = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        (written == size).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
    }
    #[cfg(target_os = "linux")]
    {
        proc_stat_start(&fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = pid;
        None
    }
}

/// The start time (field 22, in clock ticks after boot) from the text of `/proc/<pid>/stat`.
/// The command name in field 2 may hold spaces and parentheses, so fields are counted from
/// the last closing parenthesis.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn proc_stat_start(stat: &str) -> Option<u64> {
    let after_name = stat.get(stat.rfind(')')? + 1..)?;
    after_name.split_whitespace().nth(19)?.parse().ok()
}

fn process_exists(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only checks that the process exists.
    let status = unsafe { libc::kill(pid, 0) };
    status == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Which process a record is about: a runtime command, or the engine that was about to start one.
#[derive(Clone, Copy)]
enum RecordKind {
    Command(u32),
    Pending(u32),
}

fn record_kind(name: &str) -> Option<RecordKind> {
    if let Some(rest) = name.strip_prefix(PENDING_RECORD_PREFIX) {
        let pid = rest.split('-').next()?.parse().ok()?;
        return Some(RecordKind::Pending(pid));
    }
    let pid = name.strip_prefix(CHILD_RECORD_PREFIX)?.parse().ok()?;
    Some(RecordKind::Command(pid))
}

fn record_field(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('=')?.trim().parse().ok())
}

/// Whether the command (or the starting engine) a record names must still be waited for.
/// A record that cannot be read falls back to the file's modification time and the longest
/// ordinary command timeout, and every record expires once its command's timeout has passed,
/// so a damaged record never holds the lock on behalf of an unrelated long-lived process.
fn record_running(
    kind: RecordKind,
    text: &str,
    modified: Option<SystemTime>,
    now: SystemTime,
) -> bool {
    let now = unix_seconds(now);
    let written = record_field(text, "written")
        .or_else(|| modified.map(unix_seconds))
        .unwrap_or(0)
        .min(now);
    let timeout = record_field(text, "timeout").unwrap_or(MUTATION_TIMEOUT.as_secs());
    if now > written + timeout + RECORD_SLACK.as_secs() {
        return false;
    }
    let (RecordKind::Command(pid) | RecordKind::Pending(pid)) = kind;
    let same_process = process_exists(pid)
        && match (record_field(text, "start"), process_start(pid)) {
            (Some(recorded), Some(current)) => recorded == current,
            _ => true,
        };
    match kind {
        RecordKind::Command(_) => same_process,
        RecordKind::Pending(_) => same_process || now - written < PENDING_WAIT.as_secs(),
    }
}

/// Whether a runtime command that a dead engine started is still running. Records of
/// commands that have exited, or whose pid now belongs to another program, are removed.
fn child_command_running(home: &Path) -> bool {
    let Ok(entries) = fs::read_dir(home) else {
        return false;
    };
    let mut running = false;
    for entry in entries.flatten() {
        let Some(kind) = entry.file_name().to_str().and_then(record_kind) else {
            continue;
        };
        let text = fs::read_to_string(entry.path()).unwrap_or_default();
        let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
        if record_running(kind, &text, modified, SystemTime::now()) {
            running = true;
        } else {
            let _ = fs::remove_file(entry.path());
        }
    }
    running
}

pub fn command_lock(paths: &RuntimePaths, timeout: Duration) -> Result<CommandLock, RuntimeError> {
    prepare_runtime_home(&paths.home, paths.storage_home.as_deref())?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(paths.home.join(".silo-configuration-worker.lock"))
        .map_err(failure)?;
    let started = Instant::now();
    loop {
        // SAFETY: the open file owns this descriptor until the lock is dropped.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            if !child_command_running(&paths.home) {
                return Ok(CommandLock {
                    file,
                    home: paths.home.clone(),
                    child_record: None,
                    record_is_pending: false,
                    release_on_drop: true,
                });
            }
            // SAFETY: as above.
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        } else {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::WouldBlock {
                return Err(failure(error));
            }
        }
        if started.elapsed() >= timeout {
            return Err(failure(
                "The previous computer command is still finishing. Retry after it exits.",
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Removes the storage folder reserved for a computer that was never saved. A folder Silo did
/// not reserve for this computer is only removed when it is empty.
fn discard_unsaved_storage(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    let folder = paths.volumes.join(configuration.name());
    if !folder.exists() {
        return Ok(());
    }
    if fs::read_to_string(folder.join(OWNER)).ok().as_deref() != Some(configuration.id()) {
        if fs::remove_dir(&folder).is_ok() {
            return Ok(());
        }
        return Err(failure("Incomplete storage ownership could not be verified. No files were removed."));
    }
    fs::remove_dir_all(folder).map_err(failure)
}

/// Whether the tools check ran in the computer and exited with a failure, as opposed to a
/// check that could not finish (a timeout, a runtime that did not answer or was killed).
fn tools_check_failed(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::Failed {
            exit_code: Some(code),
            ..
        } if *code != -1
    )
}

fn reconcile(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    journal: &Journal,
) -> Result<(), RuntimeError> {
    let mut current = read_metadata(&paths.metadata)?;
    for configuration in &current.computers {
        if !journal.previous.computers.contains(configuration)
            && !journal.request.computers.contains(configuration)
        {
            return Err(failure(
                "Computer settings changed since the interruption. Saved data was preserved.",
            ));
        }
    }
    let listed = list_managed(runner, paths)?;
    for configuration in journal.request.computers.iter().filter(|configuration| {
        !journal
            .previous
            .computers
            .iter()
            .any(|old| old.id() == configuration.id())
    }) {
        if listed
            .iter()
            .any(|entry| entry.name == configuration.name())
        {
            let inspected = inspect_computer(runner, paths, configuration.name())?;
            ensure_managed(&inspected)?;
            if inspected
                .config
                .pointer("/labels/silo.machine-id")
                .and_then(Value::as_str)
                != Some(configuration.id())
            {
                return Err(failure(
                    "A different computer now owns the requested name. Its data was preserved.",
                ));
            }
            let unsaved = !current
                .computers
                .iter()
                .any(|entry| entry.id() == configuration.id());
            // A computer that was never saved is not worth keeping when it cannot be used: its
            // leftovers are removed so that the replay below creates it again. A check that did
            // not finish (a timeout, a runtime that does not answer) says nothing about it, so
            // the computer stays for the next launch.
            let mismatched = configuration_attention(paths, configuration, &inspected).is_some();
            if !(unsaved && mismatched) {
                verify_computer_configuration(runner, paths, configuration)?;
            }
            let unusable = if !unsaved {
                false
            } else if mismatched {
                true
            } else {
                settle_unsaved_computer(runner, paths, &inspected, configuration.name())?;
                match verify_guest_tools(runner, paths, configuration.name()) {
                    Ok(()) => false,
                    Err(error) if tools_check_failed(&error) => true,
                    Err(error) => return Err(error),
                }
            };
            if unusable {
                cleanup_failed_create(runner, paths, configuration.name(), configuration.id())
                    .and_then(|()| discard_unsaved_storage(paths, configuration))?;
                continue;
            }
            if unsaved {
                if let Some(desktop) = crate::desktop::configuration(configuration) {
                    crate::desktop::configure_with(
                        runner,
                        paths,
                        configuration.name(),
                        None,
                        desktop,
                    )?;
                    let restored = inspect_computer(runner, paths, configuration.name())?;
                    ensure_managed(&restored)?;
                    if !matches!(restored.status.as_str(), "Created" | "Stopped") {
                        return Err(failure(
                            "Desktop installation did not restore the new computer's stopped state.",
                        ));
                    }
                }
                current.computers.push(configuration.clone());
            }
        } else {
            if current
                .computers
                .iter()
                .any(|entry| entry.id() == configuration.id())
            {
                return Err(failure(
                    "A saved computer is missing from the runtime. Its disk files were preserved.",
                ));
            }
            discard_unsaved_storage(paths, configuration)?;
        }
    }
    for configuration in journal.previous.computers.iter().filter(|configuration| {
        !journal
            .request
            .computers
            .iter()
            .any(|next| next.id() == configuration.id())
    }) {
        if !listed
            .iter()
            .any(|entry| entry.name == configuration.name())
        {
            crate::network::computer_removed(paths, configuration.name()).map_err(failure)?;
            crate::secrets::computer_removed(configuration.name()).map_err(failure)?;
            remove_computer_volumes(paths, configuration)?;
            lifecycle_recovery::forget_removed(paths, configuration)?;
            current
                .computers
                .retain(|entry| entry.id() != configuration.id());
        } else {
            let inspected = inspect_computer(runner, paths, configuration.name())?;
            if inspected
                .config
                .pointer("/labels/silo.machine-id")
                .and_then(Value::as_str)
                != Some(configuration.id())
            {
                return Err(failure(
                    "The computer selected for deletion changed ownership. It was preserved.",
                ));
            }
        }
    }
    if !current.computers.is_empty() || paths.metadata.exists() {
        write_metadata(&paths.metadata, &current)?;
    }
    // An interrupted removal keeps its checkpoint history until exact native members
    // have entered the cleanup journal. The updated inventory releases its own pins.
    for configuration in journal.previous.computers.iter().filter(|configuration| {
        !journal
            .request
            .computers
            .iter()
            .any(|next| next.id() == configuration.id())
            && !current
                .computers
                .iter()
                .any(|next| next.id() == configuration.id())
    }) {
        checkpoints::remove_deleted_snapshots(
            runner,
            paths,
            configuration.id(),
            configuration.name(),
        )?;
        checkpoints::forget_removed(paths, configuration.id())?;
    }
    Ok(())
}

fn verify_committed_edits(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    journal: &Journal,
    requested: &ComputerConfigurationRequest,
) -> Result<(), RuntimeError> {
    // Metadata is committed before the final verification. An existing computer whose
    // edit reached that checkpoint still needs verification after relaunch.
    let current = read_metadata(&paths.metadata)?;
    for configuration in journal.request.computers.iter().filter(|configuration| {
        journal
            .previous
            .computers
            .iter()
            .any(|old| old.id() == configuration.id() && old != *configuration)
            && current.computers.contains(configuration)
            && requested.computers.contains(configuration)
    }) {
        let inspected = inspect_computer(runner, paths, configuration.name())?;
        ensure_managed(&inspected)?;
        if inspected
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            != Some(configuration.id())
        {
            return Err(failure(
                "The updated computer changed ownership. Its data was preserved.",
            ));
        }
        verify_computer_configuration(runner, paths, configuration)?;
    }
    Ok(())
}

pub(super) fn recover_at_paths(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    resources: &DeviceResources,
    progress: &dyn Fn(&str, &str, u8),
) -> Result<(), RuntimeError> {
    debug_assert!(
        operation_gate::held(),
        "configuration recovery requires the device operation gate"
    );
    let Some(journal) = load(paths)? else {
        return Ok(());
    };
    // Drain a surviving child before inspecting state. The caller holds the
    // operation gate (device scope), which serializes the application-level
    // recovery transaction against all other computer-changing work. This comes first: the
    // normalization below asks the runtime which computers already exist, and a creation
    // that is still running would make that answer wrong.
    drop(command_lock(paths, MUTATION_TIMEOUT)?);
    let journal = normalize_desktop_intent(runner, paths, journal)?;
    reconcile(runner, paths, &journal)?;
    verify_committed_edits(runner, paths, &journal, &journal.request)?;
    apply_whole_configuration_with_progress(
        runner,
        paths,
        resources,
        journal.request,
        None,
        progress,
    )?;
    finish(paths)
}

/// A journal written by an older Silo has no desktop defaults. Apply the ones an explicit
/// retry would, and re-save the intent atomically, so replay (which defaults the same way
/// before `begin` compares with the journal) matches what is saved.
///
/// The defaults describe a computer that is still to be created from the current image. A
/// creation that got as far as the runtime before the interruption made its computer from
/// the image of its time (a 0.x journal means the v3 image, with no built-in desktop and no
/// computer-use mount), and recovery adopts that computer as it is: its journaled settings
/// are kept, so it is never promoted to built-in. The runtime is asked only when a default
/// would change something.
pub(super) fn normalize_desktop_intent(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    mut journal: Journal,
) -> Result<Journal, RuntimeError> {
    // Computers already committed count as existing: their desktop settings are kept.
    let mut known = journal.previous.clone();
    for configuration in read_metadata(&paths.metadata)?.computers {
        if !known
            .computers
            .iter()
            .any(|old| old.id() == configuration.id())
        {
            known.computers.push(configuration);
        }
    }
    let mut request = journal.request.clone();
    apply_desktop_defaults(&known, &mut request);
    if request == journal.request {
        return Ok(journal);
    }
    let created: Vec<String> = list_managed(runner, paths)?
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    for (defaulted, original) in request.computers.iter_mut().zip(&journal.request.computers) {
        if defaulted != original && created.iter().any(|name| name == original.name()) {
            *defaulted = original.clone();
        }
    }
    if request != journal.request {
        journal.request = request;
        write(paths, &journal)?;
    }
    Ok(journal)
}

pub(super) fn prepare_retry(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    request: Option<&ComputerConfigurationRequest>,
) -> Result<(), RuntimeError> {
    let _attempt = attempt();
    if let Some(journal) = load(paths)? {
        drop(command_lock(paths, MUTATION_TIMEOUT)?);
        reconcile(runner, paths, &journal)?;
        verify_committed_edits(runner, paths, &journal, request.unwrap_or(&journal.request))?;
        if let Some(request) = request.filter(|request| **request != journal.request) {
            validate_request(request)?;
            let previous = read_metadata(&paths.metadata)?;
            for configuration in &request.computers {
                if let Some(old) = previous
                    .computers
                    .iter()
                    .find(|old| old.id() == configuration.id())
                {
                    validate_computer_update(old, configuration)?;
                }
            }
            for configuration in &previous.computers {
                let marker = paths.volumes.join(configuration.name()).join(OWNER);
                if fs::read_to_string(&marker).ok().as_deref() == Some(configuration.id()) {
                    fs::remove_file(&marker).map_err(failure)?;
                    File::open(marker.parent().unwrap())
                        .and_then(|file| file.sync_all())
                        .map_err(failure)?;
                }
            }
            // Reconciliation has either adopted completed additions or removed
            // owned partial files. Atomically replace intent without losing the
            // committed computers that the revised request may now edit/remove.
            write(
                paths,
                &Journal {
                    version: 1,
                    previous,
                    request: request.clone(),
                },
            )?;
        }
    }
    Ok(())
}

pub(super) fn pending(paths: &RuntimePaths) -> Result<bool, String> {
    let idle = STARTUP_SETTLED.load(Ordering::SeqCst) && LIVE_ATTEMPTS.load(Ordering::SeqCst) == 0;
    blocks_snapshot(paths, idle)
}

fn blocks_snapshot(paths: &RuntimePaths, no_live_attempt: bool) -> Result<bool, String> {
    // Once no attempt is running (startup recovery or an in-session change
    // stopped with an error), let the normal snapshot verifier show the actual
    // committed state so the user can correct the request. The saved intent,
    // activity failure and error remain; this is not completion.
    if no_live_attempt {
        return Ok(false);
    }
    load(paths)
        .map(|journal| journal.is_some())
        .map_err(|e| e.to_string())
}

fn publish_application_state_changed(context: &dyn silo_engine::context::EngineContext) {
    context.publish(
        Event::new("silo://application-state-changed", Value::Null),
        Audience::All,
    );
}

pub fn recover(host: &dyn RuntimeHost) -> Result<(), String> {
    let context = host.engine_context();
    let result = {
        let _attempt = attempt();
        STARTUP_SETTLED.store(true, Ordering::SeqCst);
        recover_inner(host, context.as_ref())
    };
    publish_application_state_changed(context.as_ref());
    result
}

fn recover_inner(
    host: &dyn RuntimeHost,
    context: &dyn silo_engine::context::EngineContext,
) -> Result<(), String> {
    let paths = runtime_paths(host)?;
    if load(&paths).map_err(|e| e.to_string())?.is_none() {
        return Ok(());
    }
    let _guard = OPERATIONS
        .device("Recovering computer configuration")
        .map_err(|_| "Computer configuration lock is unavailable.")?;
    // Waiting for the gate let other work replace or finish the journal, so the copy to
    // reserve from is read now. Replaying must not add a name a macOS computer took
    // since the interruption.
    let journal = load(&paths).map_err(|e| e.to_string())?;
    let Some(_reservation) = reserve_replay(journal.as_ref(), &|| host.macos_computer_names())?
    else {
        return Ok(());
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    let activity = Mutex::new(ActivityJournal::start(&paths, &request_id)?);
    let progress = |step: &str, name: &str, fraction: u8| {
        let event = activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append(computer_progress(&request_id, step, name, fraction));
        publish_recovery_progress(context, event);
    };
    progress("setup-started", "", 0);
    let result = device_resources()
        .and_then(|resources| recover_at_paths(&ProcessRunner, &paths, &resources, &progress));
    progress(
        if result.is_ok() {
            "setup-completed"
        } else {
            "setup-interrupted"
        },
        "",
        0,
    );
    publish_application_state_changed(context);
    result.map_err(|e| e.to_string())
}

/// Reserves the names replaying `journal` would add. `None` when no journal is pending.
fn reserve_replay(
    journal: Option<&Journal>,
    macos_names: &dyn Fn() -> Vec<String>,
) -> Result<Option<crate::computer_names::Reservation>, String> {
    let Some(journal) = journal else {
        return Ok(None);
    };
    crate::computer_names::reserve(
        &super::new_names(&journal.previous.computers, &journal.request.computers),
        macos_names,
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn computer(id: &str, name: &str) -> ComputerConfiguration {
        ComputerConfiguration {
            id: id.into(),
            name: name.into(),
            cpus: 2,
            max_cpus: 4,
            memory_gib: 4,
            max_memory_gib: 8,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            runtime: None,
            desktop: None,
        }
    }

    fn journal(previous: &[ComputerConfiguration], request: &[ComputerConfiguration]) -> Journal {
        let request_of = |computers: &[ComputerConfiguration]| ComputerConfigurationRequest {
            schema_version: 1,
            computers: computers.to_vec(),
        };
        Journal {
            version: 1,
            previous: request_of(previous),
            request: request_of(request),
        }
    }

    #[test]
    fn recovered_configuration_progress_targets_only_the_main_client() {
        use silo_engine::context::{test_support::RecordingContext, Audience, ClientId};

        let context = RecordingContext::default();
        publish_recovery_progress(
            &context,
            super::super::computer_progress("request", "setup-started", "", 0),
        );

        let published = context.published();
        assert_eq!(published.len(), 1);
        assert_eq!(
            published[0].0.name,
            "silo://computer-configuration-progress"
        );
        assert_eq!(published[0].1, Audience::Client(ClientId::new("main")));
    }

    #[test]
    fn replay_reserves_from_the_journal_read_after_the_gate() {
        let macos = || vec!["rr-foo".to_string()];
        // The journal read before the gate added rr-foo; a change since then replaced it.
        let stale = journal(&[], &[computer("1", "rr-foo")]);
        let fresh = journal(&[], &[computer("2", "rr-bar")]);
        assert!(reserve_replay(Some(&stale), &macos).is_err());
        assert!(reserve_replay(Some(&fresh), &macos).unwrap().is_some());
        // A journal that finished meanwhile leaves nothing to replay or reserve.
        assert!(reserve_replay(None, &macos).unwrap().is_none());
    }

    #[test]
    fn a_migrated_configuration_operation_loads() {
        let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
        let journal = load(&migrated.runtime_paths()).unwrap().unwrap();
        let names = |request: &ComputerConfigurationRequest| {
            request
                .computers
                .iter()
                .map(|c| c.name().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&journal.previous), ["dev"]);
        assert_eq!(names(&journal.request), ["dev"]);
        assert!(journal.request.computers[0].desktop.is_some());
    }

    #[test]
    fn network_mappings_are_removed_when_recovering_an_interrupted_deletion() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let computer = ComputerConfiguration {
            id: "00000000-0000-4000-8000-000000000001".into(),
            name: "dev".into(),
            cpus: 1,
            max_cpus: 1,
            memory_gib: 2,
            max_memory_gib: 2,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            runtime: None,
            desktop: None,
        };
        let previous = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer],
        };
        write_metadata(&paths.metadata, &previous).unwrap();
        let request = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![],
        };
        begin(&paths, &request).unwrap();
        let network = paths.metadata.with_file_name("network.json");
        fs::write(
            &network,
            json!({"mappings":[
                {"computer":"dev","port":3000,"hostPort":null,"scheme":"http","enabled":true}
            ]})
            .to_string(),
        )
        .unwrap();
        let runner = crate::test_support::runner::ScriptedRunner::new([
            crate::test_support::runner::ExpectedCommand::ok(
                ["list", "--label", MANAGED_LABEL, "--format", "json"],
                "[]",
            ),
        ]);
        prepare_retry(&runner, &paths, None).unwrap();
        runner.assert_finished();
        assert!(read_metadata(&paths.metadata).unwrap().computers.is_empty());
        let saved: Value = serde_json::from_slice(&fs::read(network).unwrap()).unwrap();
        assert!(saved["mappings"].as_array().unwrap().is_empty());
    }

    #[test]
    fn claimed_computer_directory_is_private() {
        let _test_state = crate::test_support::global_state();
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let configuration = super::super::tests::computer();
        let request = super::super::tests::request(vec![configuration.clone()]);
        begin(&paths, &request).unwrap();
        claim(&paths, &configuration).unwrap();
        let folder = paths.volumes.join(configuration.name());
        assert_eq!(
            fs::metadata(&folder).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::read_to_string(folder.join(OWNER)).unwrap(),
            configuration.id()
        );
    }

    #[cfg(unix)]
    #[test]
    fn ordinary_command_lock_release_survives_an_unrelated_fork() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let lock = command_lock(&paths, Duration::ZERO).unwrap();
        let mut ready = [0; 2];
        let mut release = [0; 2];
        // SAFETY: only async-signal-safe libc calls run in the child. The pipes
        // keep it alive until the parent has tested the inherited descriptor.
        unsafe {
            assert_eq!(libc::pipe(ready.as_mut_ptr()), 0);
            assert_eq!(libc::pipe(release.as_mut_ptr()), 0);
            let child = libc::fork();
            assert!(child >= 0);
            if child == 0 {
                libc::close(ready[0]);
                libc::close(release[1]);
                let marker = [1u8];
                libc::write(ready[1], marker.as_ptr().cast(), 1);
                let mut finish = [0u8];
                libc::read(release[0], finish.as_mut_ptr().cast(), 1);
                libc::_exit(0);
            }
            libc::close(ready[1]);
            libc::close(release[0]);
            let mut marker = [0u8];
            assert_eq!(libc::read(ready[0], marker.as_mut_ptr().cast(), 1), 1);
            drop(lock);
            let reacquired = command_lock(&paths, Duration::ZERO).is_ok();
            libc::write(release[1], marker.as_ptr().cast(), 1);
            assert_eq!(libc::waitpid(child, std::ptr::null_mut(), 0), child);
            libc::close(ready[0]);
            libc::close(release[1]);
            assert!(
                reacquired,
                "an unrelated fork kept the released command lock"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn command_lock_waits_for_a_recorded_command_that_is_still_running() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let mut child = std::process::Command::new("sleep").arg("60").spawn().unwrap();
        let mut lock = command_lock(&paths, Duration::ZERO).unwrap();
        lock.record_child(child.id(), Duration::from_secs(60)).unwrap();
        drop(lock);
        assert!(command_lock(&paths, Duration::ZERO).is_err());
        child.kill().unwrap();
        child.wait().unwrap();
        drop(command_lock(&paths, Duration::ZERO).unwrap());
        assert!(!paths
            .home
            .join(format!("{CHILD_RECORD_PREFIX}{}", child.id()))
            .exists());
    }

    #[cfg(unix)]
    #[test]
    fn command_lock_ignores_a_record_whose_pid_belongs_to_another_program() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let mut lock = command_lock(&paths, Duration::ZERO).unwrap();
        lock.record_child(std::process::id(), Duration::from_secs(60)).unwrap();
        let record = paths
            .home
            .join(format!("{CHILD_RECORD_PREFIX}{}", std::process::id()));
        drop(lock);
        assert!(command_lock(&paths, Duration::ZERO).is_err());
        if process_start(std::process::id()).is_some() {
            fs::write(
                &record,
                format!(
                    "start=1\nwritten={}\ntimeout=60\n",
                    unix_seconds(SystemTime::now())
                ),
            )
            .unwrap();
            drop(command_lock(&paths, Duration::ZERO).unwrap());
            assert!(!record.exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn exiting_a_recorded_command_removes_its_record() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let mut lock = command_lock(&paths, Duration::ZERO).unwrap();
        lock.record_child(std::process::id(), Duration::from_secs(60)).unwrap();
        lock.mark_child_exited();
        drop(lock);
        drop(command_lock(&paths, Duration::ZERO).unwrap());
    }

    #[cfg(unix)]
    fn record_in(paths: &RuntimePaths, name: &str, text: &str) -> PathBuf {
        fs::create_dir_all(&paths.home).unwrap();
        let record = paths.home.join(name);
        fs::write(&record, text).unwrap();
        record
    }

    #[cfg(unix)]
    fn written_ago(seconds: u64) -> u64 {
        unix_seconds(SystemTime::now()) - seconds
    }

    #[cfg(unix)]
    #[test]
    fn waiting_on_a_recorded_command_ends_with_a_plain_timeout_message() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let mut child = std::process::Command::new("sleep").arg("60").spawn().unwrap();
        let mut lock = command_lock(&paths, Duration::ZERO).unwrap();
        lock.record_child(child.id(), Duration::from_secs(60)).unwrap();
        drop(lock);
        let started = Instant::now();
        let error = match command_lock(&paths, Duration::from_millis(350)) {
            Ok(_) => panic!("a recorded command that is running did not hold the lock"),
            Err(error) => error,
        };
        assert!(started.elapsed() >= Duration::from_millis(350));
        assert!(
            error
                .to_string()
                .contains("The previous computer command is still finishing. Retry after it exits."),
            "{error}"
        );
        child.kill().unwrap();
        child.wait().unwrap();
        drop(command_lock(&paths, Duration::ZERO).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_record_replaces_the_pending_record_and_leaves_no_temporary_file() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let names = || -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&paths.home)
                .unwrap()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with(".silo-configuration-worker.") && name != ".silo-configuration-worker.lock")
                .collect();
            names.sort();
            names
        };
        let mut lock = command_lock(&paths, Duration::ZERO).unwrap();
        lock.begin_command(Duration::from_secs(60)).unwrap();
        let pending = names();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].starts_with(PENDING_RECORD_PREFIX), "{pending:?}");
        lock.record_child(std::process::id(), Duration::from_secs(60)).unwrap();
        assert_eq!(
            names(),
            vec![format!("{CHILD_RECORD_PREFIX}{}", std::process::id())]
        );
        lock.mark_child_exited();
        assert!(names().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_pending_record_is_removed_when_its_lock_is_dropped_without_a_command() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let mut lock = command_lock(&paths, Duration::ZERO).unwrap();
        lock.begin_command(Duration::from_secs(60)).unwrap();
        drop(lock);
        drop(command_lock(&paths, Duration::ZERO).unwrap());
        assert!(!fs::read_dir(&paths.home)
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().contains("pending")));
    }

    #[cfg(unix)]
    #[test]
    fn a_pending_record_of_a_dead_engine_is_waited_for_briefly_then_removed() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let name = format!("{PENDING_RECORD_PREFIX}2000000000-0");
        let record = record_in(
            &paths,
            &name,
            &format!("start=1\nwritten={}\ntimeout=180\n", written_ago(0)),
        );
        assert!(command_lock(&paths, Duration::ZERO).is_err());
        assert!(record.exists());
        fs::write(
            &record,
            format!("start=1\nwritten={}\ntimeout=180\n", written_ago(10)),
        )
        .unwrap();
        drop(command_lock(&paths, Duration::ZERO).unwrap());
        assert!(!record.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_record_without_a_start_time_expires_with_its_command_timeout() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let name = format!("{CHILD_RECORD_PREFIX}{}", std::process::id());
        let record = record_in(
            &paths,
            &name,
            &format!("written={}\ntimeout=60\n", written_ago(5)),
        );
        assert!(command_lock(&paths, Duration::ZERO).is_err());
        fs::write(
            &record,
            format!("written={}\ntimeout=60\n", written_ago(600)),
        )
        .unwrap();
        drop(command_lock(&paths, Duration::ZERO).unwrap());
        assert!(!record.exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_record_does_not_hold_the_lock_for_an_unrelated_long_lived_process() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let name = format!("{CHILD_RECORD_PREFIX}{}", std::process::id());
        for garbage in ["", "not a record", "start=abc\nwritten=\n"] {
            let record = record_in(&paths, &name, garbage);
            // Fresh, it is treated like any record whose process is still running.
            assert!(command_lock(&paths, Duration::ZERO).is_err());
            let old = SystemTime::now() - Duration::from_secs(3600);
            File::options()
                .write(true)
                .open(&record)
                .unwrap()
                .set_modified(old)
                .unwrap();
            drop(command_lock(&paths, Duration::ZERO).unwrap());
            assert!(!record.exists());
        }
    }

    #[test]
    fn the_start_time_is_read_from_a_linux_process_status_line() {
        // Field 22 follows a command name that may hold spaces and parentheses.
        let stat = "4242 (my (odd) name) S 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 3 0 987654 \
                    1000 50 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 17 2 0 0 0 0 0";
        assert_eq!(proc_stat_start(stat), Some(987654));
        assert_eq!(proc_stat_start("4242 (short) S 1"), None);
        assert_eq!(proc_stat_start("no parenthesis"), None);
        assert_eq!(proc_stat_start(""), None);
    }

    #[test]
    fn recovery_checks_the_tools_of_an_interrupted_creation_once() {
        let _test_state = crate::test_support::global_state();
        struct InterruptedRuntime {
            inspected: Value,
            calls: Mutex<Vec<Vec<String>>>,
        }
        impl RuntimeRunner for InterruptedRuntime {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.calls.lock().unwrap().push(args.to_vec());
                let value = match args[0].as_str() {
                    "list" => json!([{"name":"dev"}]),
                    "inspect" => self.inspected.clone(),
                    "exec" => Value::Null,
                    other => panic!("Unexpected recovery operation: {other}"),
                };
                Ok(CommandOutput {
                    stdout: value.to_string(),
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let paths = RuntimePaths {
            executable: root.join("msb"),
            library: root.join("library"),
            home: root.join("home"),
            storage_home: None,
            guest_image: root.join("image"),
            metadata: root.join("computers.json"),
            volumes: root.join("volumes"),
        };
        let configuration = ComputerConfiguration {
            id: uuid::Uuid::new_v4().to_string(),
            name: "dev".into(),
            cpus: 1,
            max_cpus: 2,
            memory_gib: 4,
            max_memory_gib: 8,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            runtime: None,
            desktop: None,
        };
        let request = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![configuration.clone()],
        };
        begin(&paths, &request).unwrap();
        claim(&paths, &configuration).unwrap();
        let inspected = json!({"name":"dev","status":"Stopped","config":{
            "image":{"Oci":{"root_disk":{"kind":"managed","size_mib":10240}}},
            "resources":{"cpus":1,"max_cpus":2,"memory_mib":4096,"max_memory_mib":8192},
            "labels":{"silo.managed":"true","silo.machine-id":configuration.id()},
            "mounts":[{"type":"Owned","guest":"/workspace","storage":{"kind":"disk","capacity_mib":10240}}]
        }});
        let runner = InterruptedRuntime {
            inspected,
            calls: Mutex::new(Vec::new()),
        };
        prepare_retry(&runner, &paths, None).unwrap();
        assert_eq!(read_metadata(&paths.metadata).unwrap(), request);
        let calls = runner.calls.lock().unwrap();
        let commands: Vec<_> = calls.iter().filter(|args| args[0] == "exec").collect();
        assert_eq!(commands.len(), 1);
        assert!(commands[0]
            .windows(2)
            .any(|pair| pair == ["--user", "root"]));
        let script = commands[0].last().unwrap();
        // The account is set up by the boot of this exec, not by the script.
        assert_eq!(script, include_str!("../../../guest/verify-tools.sh"));
        drop(calls);
        // Metadata adoption makes a subsequent retry read-only.
        runner.calls.lock().unwrap().clear();
        prepare_retry(&runner, &paths, None).unwrap();
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args[0] == "exec"));
    }

    #[test]
    fn failed_recovery_unblocks_verified_current_state_without_discarding_intent() {
        let _test_state = crate::test_support::global_state();
        struct EmptyRuntime;
        impl RuntimeRunner for EmptyRuntime {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                assert_eq!(args[0], "list");
                Ok(CommandOutput {
                    stdout: "[]".into(),
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let paths = RuntimePaths {
            executable: root.join("msb"),
            library: root.join("library"),
            home: root.join("home"),
            storage_home: None,
            guest_image: root.join("image"),
            metadata: root.join("computers.json"),
            volumes: root.join("volumes"),
        };
        let current = ComputerConfigurationRequest {
            schema_version: 1,
            computers: Vec::new(),
        };
        write_metadata(&paths.metadata, &current).unwrap();
        let new = ComputerConfiguration {
            id: uuid::Uuid::new_v4().to_string(),
            name: "dev".into(),
            cpus: 1,
            max_cpus: 2,
            memory_gib: 4,
            max_memory_gib: 8,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            runtime: None,
            desktop: None,
        };
        let request = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![new],
        };
        begin(&paths, &request).unwrap();
        assert!(blocks_snapshot(&paths, false).unwrap());
        assert!(!blocks_snapshot(&paths, true).unwrap());
        // The existing verifier still checks real runtime state, rather than
        // presenting the pending requested computer as successfully created.
        let source = read_application_state_with(&EmptyRuntime, &paths).unwrap();
        assert_eq!(source.computers.len(), 0);
        assert_eq!(read_metadata(&paths.metadata).unwrap(), current);
        assert!(path(&paths).is_file());
        let mut revised = request;
        revised.computers[0].memory_gib = 2;
        prepare_retry(&EmptyRuntime, &paths, Some(&revised)).unwrap();
        assert!(load(&paths)
            .unwrap()
            .is_some_and(|journal| journal.request == revised));
    }

    #[test]
    fn in_session_failure_stops_blocking_snapshots_but_keeps_intent() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let new = ComputerConfiguration {
            id: uuid::Uuid::new_v4().to_string(),
            name: "dev".into(),
            cpus: 1,
            max_cpus: 2,
            memory_gib: 4,
            max_memory_gib: 8,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            runtime: None,
            desktop: None,
        };
        let request = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![new],
        };
        let settled = STARTUP_SETTLED.swap(true, Ordering::SeqCst);
        let blocked_while_live = {
            let _attempt = attempt();
            begin(&paths, &request).unwrap();
            pending(&paths).unwrap()
            // The attempt fails here without `finish`.
        };
        let blocked_after_failure = pending(&paths).unwrap();
        STARTUP_SETTLED.store(settled, Ordering::SeqCst);
        assert!(blocked_while_live);
        assert!(!blocked_after_failure);
        assert!(
            path(&paths).is_file(),
            "the interrupted intent stays available for Retry"
        );
    }

    #[test]
    fn retry_resumes_the_recorded_request_and_rejects_settings_changed_since() {
        let _test_state = crate::test_support::global_state();
        struct EmptyRuntime;
        impl RuntimeRunner for EmptyRuntime {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                assert_eq!(args[0], "list");
                Ok(CommandOutput {
                    stdout: "[]".into(),
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let paths = RuntimePaths {
            executable: root.join("msb"),
            library: root.join("library"),
            home: root.join("home"),
            storage_home: None,
            guest_image: root.join("image"),
            metadata: root.join("computers.json"),
            volumes: root.join("volumes"),
        };
        let entry = |name: &str| ComputerConfiguration {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            cpus: 1,
            max_cpus: 2,
            memory_gib: 4,
            max_memory_gib: 8,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            runtime: None,
            desktop: None,
        };
        let (a, b, c) = (entry("a"), entry("b"), entry("c"));
        // The interrupted attempt was adding "b" to an inventory that held "a".
        let previous = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![a.clone()],
        };
        write_metadata(&paths.metadata, &previous).unwrap();
        let request = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![a.clone(), b],
        };
        begin(&paths, &request).unwrap();
        // The retry command reads exactly this recorded target to resume.
        assert_eq!(pending_request(&paths).unwrap(), Some(request.clone()));
        // Someone changed the inventory since the attempt: "c" is foreign to both the
        // pre-attempt state and the recorded target, so resuming is rejected.
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![a, c],
            },
        )
        .unwrap();
        let error = prepare_retry(&EmptyRuntime, &paths, Some(&request))
            .unwrap_err()
            .to_string();
        assert!(error.contains("changed since"), "{error}");
    }

    /// A runtime holding the computer an interrupted creation made: it is `status` and keeps
    /// it until it is stopped. An `exec` leaves the status as it is, as the real one does.
    struct InterruptedCreation {
        id: String,
        status: Mutex<&'static str>,
        exec_error: Option<fn() -> RuntimeError>,
        stop_error: Option<fn() -> RuntimeError>,
        cpus: u64,
        removed: AtomicBool,
        calls: Mutex<Vec<String>>,
    }

    impl InterruptedCreation {
        fn new(id: &str, status: &'static str, exec_error: Option<fn() -> RuntimeError>) -> Self {
            Self {
                id: id.into(),
                status: Mutex::new(status),
                exec_error,
                stop_error: None,
                cpus: 1,
                removed: AtomicBool::new(false),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn ran(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl RuntimeRunner for InterruptedCreation {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args[0].clone());
            let value = match args[0].as_str() {
                "list" if self.removed.load(Ordering::SeqCst) => json!([]),
                "list" => json!([{"name":"dev"}]),
                "inspect" => json!({"name":"dev","status":*self.status.lock().unwrap(),"config":{
                    "image":{"Oci":{"root_disk":{"kind":"managed","size_mib":10240}}},
                    "resources":{"cpus":self.cpus,"max_cpus":2,"memory_mib":4096,"max_memory_mib":8192},
                    "labels":{"silo.managed":"true","silo.machine-id":self.id},
                    "mounts":[{"type":"Owned","guest":"/workspace","storage":{"kind":"disk","capacity_mib":10240}}]
                }}),
                "stop" if self.stop_error.is_some() => return Err((self.stop_error.unwrap())()),
                "stop" => {
                    *self.status.lock().unwrap() = "Stopped";
                    Value::Null
                }
                "exec" if self.exec_error.is_some() => return Err((self.exec_error.unwrap())()),
                "exec" => Value::Null,
                "remove" => {
                    self.removed.store(true, Ordering::SeqCst);
                    Value::Null
                }
                other => panic!("Unexpected recovery operation: {other}"),
            };
            Ok(CommandOutput {
                stdout: value.to_string(),
                stderr: String::new(),
            })
        }
    }

    fn interrupted_creation(
        directory: &tempfile::TempDir,
    ) -> (RuntimePaths, ComputerConfiguration, ComputerConfigurationRequest) {
        let root = directory.path();
        let paths = RuntimePaths {
            executable: root.join("msb"),
            library: root.join("library"),
            home: root.join("home"),
            storage_home: None,
            guest_image: root.join("image"),
            metadata: root.join("computers.json"),
            volumes: root.join("volumes"),
        };
        let configuration = computer(&uuid::Uuid::new_v4().to_string(), "dev");
        let configuration = ComputerConfiguration {
            cpus: 1,
            max_cpus: 2,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            ..configuration
        };
        let request = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![configuration.clone()],
        };
        begin(&paths, &request).unwrap();
        claim(&paths, &configuration).unwrap();
        (paths, configuration, request)
    }

    #[test]
    fn recovery_stops_the_running_computer_an_interrupted_creation_left() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let (paths, configuration, request) = interrupted_creation(&directory);
        let runtime = InterruptedCreation::new(configuration.id(), "Running", None);
        prepare_retry(&runtime, &paths, None).unwrap();
        assert_eq!(read_metadata(&paths.metadata).unwrap(), request);
        let calls = runtime.ran();
        let position = |command: &str| calls.iter().position(|call| call == command).unwrap();
        assert!(position("stop") < position("exec"), "{calls:?}");
        assert!(!calls.iter().any(|call| call == "remove"), "{calls:?}");
    }

    fn tools_failed() -> RuntimeError {
        RuntimeError::Failed {
            operation: "Running a command".into(),
            exit_code: Some(1),
            detail: "tools missing".into(),
        }
    }

    fn timed_out() -> RuntimeError {
        RuntimeError::TimedOut {
            operation: "Running a command".into(),
        }
    }

    fn unavailable() -> RuntimeError {
        RuntimeError::Unavailable("The runtime is not answering.".into())
    }

    #[test]
    fn recovery_removes_an_interrupted_creation_whose_tools_check_failed() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let (paths, configuration, request) = interrupted_creation(&directory);
        let runtime = InterruptedCreation::new(configuration.id(), "Running", Some(tools_failed));
        prepare_retry(&runtime, &paths, None).unwrap();
        // The computer was never saved, so nothing of it stays; the replay creates it again.
        assert!(runtime.removed.load(Ordering::SeqCst));
        assert!(read_metadata(&paths.metadata).unwrap().computers.is_empty());
        assert!(!paths.volumes.join("dev").exists());
        assert_eq!(pending_request(&paths).unwrap(), Some(request));
    }

    #[test]
    fn recovery_keeps_an_interrupted_creation_when_the_tools_check_did_not_finish() {
        for exec_error in [timed_out as fn() -> RuntimeError, unavailable] {
            let _test_state = crate::test_support::global_state();
            let directory = tempfile::tempdir().unwrap();
            let (paths, configuration, request) = interrupted_creation(&directory);
            let runtime = InterruptedCreation::new(configuration.id(), "Running", Some(exec_error));
            assert!(prepare_retry(&runtime, &paths, None).is_err());
            // Nothing was removed: the next launch checks the same computer again.
            assert!(!runtime.removed.load(Ordering::SeqCst));
            assert!(paths.volumes.join("dev").exists());
            assert_eq!(pending_request(&paths).unwrap(), Some(request));
            assert!(read_metadata(&paths.metadata).unwrap().computers.is_empty());
        }
    }

    #[test]
    fn recovery_keeps_an_interrupted_creation_that_would_not_stop() {
        for stop_error in [timed_out as fn() -> RuntimeError, unavailable] {
            let _test_state = crate::test_support::global_state();
            let directory = tempfile::tempdir().unwrap();
            let (paths, configuration, request) = interrupted_creation(&directory);
            let mut runtime = InterruptedCreation::new(configuration.id(), "Running", None);
            runtime.stop_error = Some(stop_error);
            assert!(prepare_retry(&runtime, &paths, None).is_err());
            assert!(!runtime.ran().iter().any(|call| call == "remove"));
            assert!(paths.volumes.join("dev").exists());
            assert_eq!(pending_request(&paths).unwrap(), Some(request));
        }
    }

    #[test]
    fn recovery_removes_an_interrupted_creation_whose_resources_do_not_match() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let (paths, configuration, request) = interrupted_creation(&directory);
        let mut runtime = InterruptedCreation::new(configuration.id(), "Running", None);
        runtime.cpus = 2;
        prepare_retry(&runtime, &paths, None).unwrap();
        assert!(runtime.removed.load(Ordering::SeqCst));
        assert!(!paths.volumes.join("dev").exists());
        assert!(read_metadata(&paths.metadata).unwrap().computers.is_empty());
        assert_eq!(pending_request(&paths).unwrap(), Some(request));
    }

    #[test]
    fn recovery_never_removes_a_saved_computer_whose_tools_cannot_be_checked() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let (paths, configuration, request) = interrupted_creation(&directory);
        write_metadata(&paths.metadata, &request).unwrap();
        let runtime = InterruptedCreation::new(configuration.id(), "Running", Some(unavailable));
        prepare_retry(&runtime, &paths, None).unwrap();
        let calls = runtime.ran();
        assert!(!calls.iter().any(|call| matches!(call.as_str(), "stop" | "exec" | "remove")), "{calls:?}");
        assert_eq!(read_metadata(&paths.metadata).unwrap(), request);
    }
}
