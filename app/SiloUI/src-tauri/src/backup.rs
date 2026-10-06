use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAGIC: &[u8; 16] = b"SILO-BACKUP\0\0\0\0\0";
const FORMAT_VERSION: u32 = 4;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_COMMAND_OUTPUT: usize = 32 * 1024;
const MAX_STRUCTURED_OUTPUT: usize = 1024 * 1024;
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(25);
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const WORKER_LOCK_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const CLEANUP_COMMAND_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const TERMINATE_GRACE: Duration = Duration::from_secs(10);
/// Slow external or network storage; below this a transfer is stuck.
const MIN_TRANSFER_BYTES_PER_SECOND: u64 = 16 * 1024 * 1024;
/// A snapshot archive holds a handful of descriptors per snapshot, its disk
/// layers, image blobs and 32 MiB memory packs; a quarter million entries is
/// far beyond any real computer chain.
const MAX_SNAPSHOT_ENTRIES: u64 = 256 * 1024;
/// Captures advance the source lineage and cannot be safely deleted automatically.
/// Bound hidden state-export members while allowing reuse of an existing checkpoint.
const MAX_STATE_EXPORT_CAPTURES: usize = 128;
/// Free space left untouched on a volume an export or import writes to.
const FREE_SPACE_RESERVE: u64 = 1024 * 1024 * 1024;
const DEFAULT_MAX_ARCHIVE_BYTES: u64 = 8 * 1024 * 1024 * 1024 * 1024;

#[derive(Clone, Default)]
pub(crate) struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// The shared cancel flag, so the operation gate and this controller can point at the
    /// same bit and agree on cancellation regardless of which path the user takes.
    pub(crate) fn flag(&self) -> Arc<AtomicBool> {
        self.0.clone()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MsbCommand {
    pub(crate) executable: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) storage_home: Option<PathBuf>,
    pub(crate) library: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct CommandOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

pub(crate) trait MsbRunner: Send + Sync {
    fn run(
        &self,
        command: &MsbCommand,
        arguments: &[String],
        timeout: Duration,
        cancellation: &Cancellation,
    ) -> Result<CommandOutput, BackupError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SystemMsbRunner;

impl MsbRunner for SystemMsbRunner {
    fn run(
        &self,
        command: &MsbCommand,
        arguments: &[String],
        timeout: Duration,
        cancellation: &Cancellation,
    ) -> Result<CommandOutput, BackupError> {
        run_msb_process(command, arguments, timeout, cancellation, TERMINATE_GRACE)
    }
}

fn stop_child(child: &mut std::process::Child, grace: Duration) {
    let _ = crate::child_process::terminate_child(child, grace);
}

fn run_msb_process(
    command: &MsbCommand,
    arguments: &[String],
    timeout: Duration,
    cancellation: &Cancellation,
    grace: Duration,
) -> Result<CommandOutput, BackupError> {
    if cancellation.cancelled() {
        return Err(BackupError::Cancelled);
    }
    crate::runtime::prepare_runtime_home(&command.home, command.storage_home.as_deref())
        .map_err(|error| BackupError::InvalidRequest(error.to_string()))?;
    // Export and import only run `snapshot` commands that write native
    // snapshot data. Such a command can outlive Silo; keep the lock in the
    // child until it exits, even if Silo dies.
    let worker_lock = if arguments.first().is_some_and(|arg| arg == "snapshot") {
        Some(wait_for_worker_lock(
            &command.home,
            worker_lock_timeout(timeout),
            cancellation,
        )?)
    } else {
        None
    };
    let mut process = Command::new(&command.executable);
    if let Some(lock) = &worker_lock {
        inherit_worker_lock(&mut process, lock);
    }
    let mut child = process
        .args(arguments)
        .env("MSB_HOME", &command.home)
        .env("MSB_PATH", &command.executable)
        .env("MSB_LIBKRUNFW_PATH", &command.library)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(BackupError::Io)?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    // JSON is parsed, so keep it whole from the start (like the runtime
    // runner) and fail on truncation. Other output is only diagnostics,
    // where the tail matters most.
    let structured = arguments
        .windows(2)
        .any(|pair| pair[0] == "--format" && pair[1] == "json");
    let stdout_reader = thread::spawn(move || {
        read_output(
            stdout,
            if structured {
                MAX_STRUCTURED_OUTPUT
            } else {
                MAX_COMMAND_OUTPUT
            },
            !structured,
        )
    });
    let stderr_reader = thread::spawn(move || read_output(stderr, MAX_COMMAND_OUTPUT, true));
    let started = Instant::now();
    loop {
        if cancellation.cancelled() {
            stop_child(&mut child, grace);
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(BackupError::Cancelled);
        }
        if started.elapsed() >= timeout {
            stop_child(&mut child, grace);
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(BackupError::CommandTimeout);
        }
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                stop_child(&mut child, grace);
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(BackupError::Io(error));
            }
        };
        if let Some(status) = status {
            let (stdout, stdout_truncated) = stdout_reader
                .join()
                .map_err(|_| BackupError::Io(io::Error::other("stdout reader failed")))??;
            let (stderr, _) = stderr_reader
                .join()
                .map_err(|_| BackupError::Io(io::Error::other("stderr reader failed")))??;
            if structured && stdout_truncated {
                let what = if arguments.first().is_some_and(|arg| arg == "snapshot") {
                    "checkpoint index"
                } else {
                    "computer list"
                };
                return Err(BackupError::InvalidRequest(format!(
                    "The runtime {what} exceeds Silo's 1 MiB size safety limit."
                )));
            }
            return Ok(CommandOutput {
                status,
                stdout: if structured {
                    String::from_utf8_lossy(&stdout).trim().to_owned()
                } else {
                    bounded_output(&stdout)
                },
                stderr: bounded_output(&stderr),
            });
        }
        thread::sleep(COMMAND_POLL_INTERVAL);
    }
}

fn inherit_worker_lock(command: &mut Command, lock: &File) {
    use std::os::unix::process::CommandExt;
    let fd = lock.as_raw_fd();
    // SAFETY: pre_exec only calls async-signal-safe fcntl on this owned FD.
    unsafe {
        command.pre_exec(move || {
            if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

const WORKER_LOCK_FILE: &str = ".silo-backup-worker.lock";

/// Also held by a surviving snapshot/create child after the app exits. A bounded
/// wait prevents recovery from racing that child's writes or hanging forever.
pub(crate) fn wait_for_interrupted_command(home: &Path, timeout: Duration) -> io::Result<File> {
    match wait_for_worker_lock(home, timeout, &Cancellation::default()) {
        Ok(file) => Ok(file),
        Err(BackupError::Io(error)) => Err(error),
        Err(error) => Err(io::Error::other(error.to_string())),
    }
}

/// Only a surviving child of an earlier Silo process holds the lock (this
/// process serializes its own export and import work), so waiting for it is
/// bounded well below the command timeout and stops as soon as the user cancels.
fn worker_lock_timeout(command_timeout: Duration) -> Duration {
    command_timeout.min(WORKER_LOCK_TIMEOUT)
}

fn wait_for_worker_lock(
    home: &Path,
    timeout: Duration,
    cancellation: &Cancellation,
) -> Result<File, BackupError> {
    fs::create_dir_all(home)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.join(WORKER_LOCK_FILE))?;
    lock_exclusively(file, timeout, cancellation)
}

/// [`wait_for_interrupted_command`] for a home that must not be written, such as
/// the previous runtime generation before the storage migration: it opens an
/// existing lock file read-only and creates neither the file nor the folder.
/// `None` means no lock file exists, so no earlier child can hold it.
pub(crate) fn wait_for_interrupted_command_in_place(
    home: &Path,
    timeout: Duration,
) -> io::Result<Option<File>> {
    let file = match OpenOptions::new()
        .read(true)
        .open(home.join(WORKER_LOCK_FILE))
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    match lock_exclusively(file, timeout, &Cancellation::default()) {
        Ok(file) => Ok(Some(file)),
        Err(BackupError::Io(error)) => Err(error),
        Err(error) => Err(io::Error::other(error.to_string())),
    }
}

fn lock_exclusively(
    file: File,
    timeout: Duration,
    cancellation: &Cancellation,
) -> Result<File, BackupError> {
    let started = Instant::now();
    loop {
        // SAFETY: file owns this valid descriptor for the duration of the lock.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(file);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::WouldBlock {
            return Err(error.into());
        }
        check_cancelled(cancellation)?;
        if started.elapsed() >= timeout {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "The previous export or import command is still finishing. Wait a moment and relaunch Silo to resume.",
            )
            .into());
        }
        thread::sleep(COMMAND_POLL_INTERVAL);
    }
}

fn read_output(mut input: impl Read, limit: usize, keep_tail: bool) -> io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; 8 * 1024];
    let mut truncated = false;
    loop {
        let count = input.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if keep_tail {
            output.extend_from_slice(&chunk[..count]);
            if output.len() > limit {
                output.drain(..output.len() - limit);
                truncated = true;
            }
        } else {
            let remaining = limit.saturating_sub(output.len());
            output.extend_from_slice(&chunk[..count.min(remaining)]);
            truncated |= count > remaining;
        }
    }
    Ok((output, truncated))
}

fn bounded_output(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(MAX_COMMAND_OUTPUT);
    String::from_utf8_lossy(&bytes[start..]).trim().to_owned()
}

#[derive(Debug)]
pub(crate) enum BackupError {
    Busy,
    Cancelled,
    CommandTimeout,
    CommandFailed {
        operation: String,
        detail: String,
    },
    /// A computer name is already taken.
    Conflict(String),
    /// A file already exists at the given path.
    FileConflict(String),
    /// A stored import group with the same identity already exists.
    ImportGroupConflict(String),
    InvalidArchive(String),
    /// A volume lacks the space an export or import needs; names both sizes.
    InsufficientSpace(String),
    /// A well-formed export from another Silo or runtime version; the
    /// message says which version can import it.
    UnsupportedArchive(String),
    UnsupportedStorage(String),
    InvalidRequest(String),
    Io(io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => write!(formatter, "Another export or import is running."),
            Self::Cancelled => write!(formatter, "The operation was cancelled."),
            Self::CommandTimeout => write!(formatter, "The bundled runtime operation timed out."),
            Self::CommandFailed { operation, detail } => {
                write!(formatter, "{operation} failed: {detail}")
            }
            Self::Conflict(name) => write!(formatter, "A computer named {name} already exists."),
            Self::FileConflict(path) => write!(formatter, "A file already exists at {path}."),
            Self::ImportGroupConflict(group) => write!(
                formatter,
                "An earlier import is still stored as {group}. Try the import again."
            ),
            Self::InvalidArchive(detail) => write!(formatter, "Invalid Silo export: {detail}"),
            Self::InsufficientSpace(detail) | Self::UnsupportedArchive(detail) => {
                write!(formatter, "{detail}")
            }
            Self::UnsupportedStorage(detail) => write!(formatter, "{detail}"),
            Self::InvalidRequest(detail) => write!(formatter, "{detail}"),
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for BackupError {}

impl From<io::Error> for BackupError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for BackupError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BackupSource {
    pub(crate) name: String,
    pub(crate) snapshot_group: String,
    pub(crate) was_running: bool,
    pub(crate) runtime_config: Value,
    pub(crate) computer_configuration: Value,
    /// When set, export an already-captured checkpoint member from
    /// `snapshot_group` instead of capturing the computer's current state.
    /// `was_running` is irrelevant on this path (no new capture is taken).
    pub(crate) existing_member: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct BackupRequest {
    pub(crate) destination: PathBuf,
    pub(crate) sources: Vec<BackupSource>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BackupResult {
    pub(crate) created_at_ms: u64,
    pub(crate) destination: PathBuf,
    pub(crate) size_bytes: u64,
    pub(crate) computers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ArchiveInspection {
    pub(crate) created_at_ms: u64,
    pub(crate) size_bytes: u64,
    pub(crate) computers: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct RestoreRequest {
    pub(crate) archive: PathBuf,
    pub(crate) source_name: Option<String>,
    pub(crate) new_name: String,
}

/// A verified, extracted snapshot. The private staging directory is deleted on drop.
pub(crate) struct PreparedRestore {
    pub(crate) source_name: String,
    pub(crate) new_name: String,
    pub(crate) runtime_config: Value,
    pub(crate) computer_configuration: Value,
    pub(crate) snapshot_group: String,
    pub(crate) snapshot_member: String,
    _stage: tempfile::TempDir,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PackageManifest {
    schema_version: u32,
    created_at_ms: u64,
    runtime: RuntimeManifest,
    computers: Vec<PackageComputer>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuntimeManifest {
    name: String,
    version: String,
    snapshot_format: String,
    guest_architecture: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PackageComputer {
    name: String,
    runtime_config: Value,
    computer_configuration: Value,
    payload_size: u64,
    payload_sha256: String,
    /// Format 3 reserves this list for separate disk payloads. The computer
    /// disk travels inside the MicroSandbox snapshot, so it must stay empty.
    volumes: Vec<Value>,
}

/// See `BackupService::discard_import_on_failure`.
#[must_use = "dropping the guard immediately discards the import"]
pub(crate) struct ImportGroupGuard<'a, R: MsbRunner> {
    service: &'a BackupService<R>,
    group: String,
    keep: bool,
}

impl<R: MsbRunner> ImportGroupGuard<'_, R> {
    /// The new computer now owns the group.
    pub(crate) fn keep(mut self) {
        self.keep = true;
    }
}

impl<R: MsbRunner> Drop for ImportGroupGuard<'_, R> {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        if let Err(error) = self.service.discard_import_group(&self.group) {
            eprintln!(
                "Silo could not remove the incomplete import {}: {error}",
                self.group
            );
        }
    }
}

struct OperationGuard<'a>(&'a AtomicBool);

impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(crate) struct BackupService<R = SystemMsbRunner> {
    command: MsbCommand,
    scratch_root: PathBuf,
    runner: R,
    busy: AtomicBool,
    command_timeout: Duration,
    max_archive_bytes: u64,
    /// Bytes available to this account on the volume holding a path.
    free_space: fn(&Path) -> io::Result<u64>,
}

impl BackupService<SystemMsbRunner> {
    pub(crate) fn new(command: MsbCommand, scratch_root: PathBuf) -> Self {
        Self::with_runner(command, scratch_root, SystemMsbRunner)
    }
}

impl<R: MsbRunner> BackupService<R> {
    pub(crate) fn with_runner(command: MsbCommand, scratch_root: PathBuf, runner: R) -> Self {
        Self {
            command,
            scratch_root,
            runner,
            busy: AtomicBool::new(false),
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
            max_archive_bytes: DEFAULT_MAX_ARCHIVE_BYTES,
            free_space: available_bytes,
        }
    }

    /// A command that reads or writes `bytes` of disk data gets the base
    /// timeout plus time for that data at a slow-disk rate, so a
    /// multi-hundred-GB computer on slow storage is not killed at one hour.
    fn data_timeout(&self, bytes: u64) -> Duration {
        self.command_timeout
            .saturating_add(Duration::from_secs(bytes / MIN_TRANSFER_BYTES_PER_SECOND))
    }

    /// Where MicroSandbox unpacks and keeps snapshots (`cache/tmp` and
    /// `snapshots` under the runtime home, which may alias external storage).
    fn native_store_root(&self) -> &Path {
        self.command
            .storage_home
            .as_deref()
            .unwrap_or(&self.command.home)
    }

    fn begin(&self) -> Result<OperationGuard<'_>, BackupError> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| BackupError::Busy)?;
        Ok(OperationGuard(&self.busy))
    }

    fn staging_directory(&self, prefix: &str) -> io::Result<tempfile::TempDir> {
        use std::os::unix::fs::PermissionsExt;
        tempfile::Builder::new()
            .prefix(prefix)
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(&self.scratch_root)
    }

    pub(crate) fn cleanup_interrupted_staging(&self) -> io::Result<()> {
        let entries = match fs::read_dir(&self.scratch_root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if (name.starts_with("backup-") || name.starts_with("restore-"))
                && entry.file_type()?.is_dir()
            {
                fs::remove_dir_all(entry.path())?;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn create_backup(
        &self,
        request: BackupRequest,
        cancellation: &Cancellation,
    ) -> Result<BackupResult, BackupError> {
        self.create_backup_with_token(request, cancellation, None, &|_, _| Ok(()))
    }

    pub(crate) fn create_backup_with_token(
        &self,
        request: BackupRequest,
        cancellation: &Cancellation,
        token: Option<&str>,
        capture_intent: &dyn Fn(&BackupSource, Option<&str>) -> Result<(), BackupError>,
    ) -> Result<BackupResult, BackupError> {
        if let Some(token) = token {
            uuid::Uuid::parse_str(token).map_err(|_| {
                BackupError::InvalidRequest("Invalid export operation identity.".into())
            })?;
        }
        let _guard = self.begin()?;
        validate_backup_request(&request)?;
        fs::create_dir_all(&self.scratch_root)?;
        self.check_export_space(&request, cancellation)?;
        let stage = self.staging_directory("backup-")?;
        let mut payloads = Vec::with_capacity(request.sources.len());
        let mut total_payload_bytes = 0_u64;

        for (index, source) in request.sources.iter().enumerate() {
            check_cancelled(cancellation)?;
            validate_computer_name(&source.name)?;
            let mut runtime_config = source.runtime_config.clone();
            let mut computer_configuration = source.computer_configuration.clone();
            portable_export_network(&source.name, &mut runtime_config)?;
            validate_export_configs(&source.name, &runtime_config, &computer_configuration)?;
            let snapshot_group = source.snapshot_group.clone();
            // Capture, verification and saving read the computer's disks.
            let data_timeout =
                self.data_timeout(declared_storage_bytes(&source.computer_configuration));
            let flush = if source.was_running {
                "required"
            } else {
                "auto"
            };
            let capture_result = (|| {
                // A checkpoint export reuses an already-captured, immutable member;
                // a state export captures the computer's current disk first.
                let snapshot = if let Some(member) = &source.existing_member {
                    let snapshot =
                        self.captured_snapshot_path(&snapshot_group, member, cancellation)?;
                    // Describe the checkpoint as it was captured, not the
                    // computer as it is now (E-29).
                    apply_captured_layout(
                        &source.name,
                        &read_snapshot_descriptor(&snapshot)?,
                        &mut runtime_config,
                        &mut computer_configuration,
                    )?;
                    validate_export_configs(
                        &source.name,
                        &runtime_config,
                        &computer_configuration,
                    )?;
                    snapshot
                } else {
                    let entries =
                        self.snapshot_index("Checking export capture limit", cancellation)?;
                    let captures = entries
                        .iter()
                        .filter(|entry| {
                            entry["group"] == snapshot_group
                                && entry["name"]
                                    .as_str()
                                    .is_some_and(|name| name.starts_with("silo-backup-"))
                        })
                        .count();
                    if captures >= MAX_STATE_EXPORT_CAPTURES {
                        return Err(BackupError::InvalidRequest(format!(
                            "{} already has {} state-export captures. Export an existing checkpoint instead. Captures are kept until the computer is deleted because later checkpoints depend on them.",
                            source.name, MAX_STATE_EXPORT_CAPTURES
                        )));
                    }
                    let snapshot_name = format!("silo-backup-{index}-{}", unique_suffix());
                    capture_intent(source, Some(&snapshot_name))?;
                    self.require_success_with(
                        "Capturing computer disk",
                        &[
                            "snapshot".into(),
                            "create".into(),
                            snapshot_name.clone(),
                            "--from-sandbox".into(),
                            source.name.clone(),
                            "--group".into(),
                            snapshot_group.clone(),
                            "--guest-flush".into(),
                            flush.into(),
                            "--integrity".into(),
                            "--quiet".into(),
                        ],
                        data_timeout,
                        cancellation,
                    )?;
                    // Capture advances MicroSandbox's source lineage. Its ancestor must
                    // remain in the native snapshot store even if archive writing fails.
                    self.captured_snapshot_path(&snapshot_group, &snapshot_name, cancellation)?
                };
                self.require_success_with(
                    "Verifying captured computer disk",
                    &[
                        "snapshot".into(),
                        "verify".into(),
                        snapshot.to_string_lossy().into_owned(),
                    ],
                    data_timeout,
                    cancellation,
                )?;
                // Once verified, this complete capture becomes the computer's lineage
                // parent. Keep it until its last dependent is deleted.
                if source.existing_member.is_none() {
                    capture_intent(source, None)?;
                }
                let payload_path = stage.path().join(format!("{index}.msb"));
                self.require_success_with(
                    "Writing computer disks to the export file",
                    &[
                        "snapshot".into(),
                        "save".into(),
                        snapshot.to_string_lossy().into_owned(),
                        payload_path.to_string_lossy().into_owned(),
                        "--with-parents".into(),
                        "--with-image".into(),
                    ],
                    data_timeout,
                    cancellation,
                )?;
                Ok::<_, BackupError>(payload_path)
            })();
            let payload_path = capture_result?;
            let (payload, payload_size) = open_payload(&payload_path, self.max_archive_bytes)?;
            total_payload_bytes = total_payload_bytes
                .checked_add(payload_size)
                .filter(|size| *size <= self.max_archive_bytes)
                .ok_or_else(|| {
                    BackupError::InvalidRequest(
                        "The selected computer checkpoints exceed the export size safety limit."
                            .into(),
                    )
                })?;
            payloads.push((
                source,
                runtime_config,
                computer_configuration,
                payload,
                payload_size,
            ));
        }

        let mut manifest = PackageManifest {
            schema_version: FORMAT_VERSION,
            created_at_ms: now_ms(),
            runtime: RuntimeManifest {
                name: "microsandbox".into(),
                version: bundled_runtime_version().into(),
                snapshot_format: snapshot_format_for(bundled_runtime_version()),
                guest_architecture: std::env::consts::ARCH.into(),
            },
            computers: payloads
                .iter()
                .map(
                    |(source, runtime_config, computer_configuration, _, payload_size)| {
                        PackageComputer {
                            name: source.name.clone(),
                            runtime_config: runtime_config.clone(),
                            computer_configuration: computer_configuration.clone(),
                            payload_size: *payload_size,
                            // Filled in while the payload is copied into the archive.
                            payload_sha256: pending_digest(),
                            // Format 3 keeps this field; MicroSandbox's snapshot carries the disks.
                            volumes: Vec::new(),
                        }
                    },
                )
                .collect(),
        };
        let archive_payloads = payloads
            .into_iter()
            .map(|(_, _, _, payload, _)| payload)
            .collect::<Vec<_>>();
        let size_bytes = write_immutable_package(
            &request.destination,
            &mut manifest,
            archive_payloads,
            cancellation,
            token,
            self.free_space,
        )?;
        Ok(BackupResult {
            created_at_ms: manifest.created_at_ms,
            destination: request.destination,
            size_bytes,
            computers: request
                .sources
                .into_iter()
                .map(|source| source.name)
                .collect(),
        })
    }

    /// Estimate the self-contained archive before capture/save writes anything.
    /// Include the native snapshot store (parents may be in another group) and
    /// image cache, plus source disks for new captures. Counting unrelated native
    /// data is conservative; the final destination check uses the actual archive.
    fn check_export_space(
        &self,
        request: &BackupRequest,
        cancellation: &Cancellation,
    ) -> Result<(), BackupError> {
        let store = self.native_store_root();
        let mut capture_bytes = 0_u64;
        for source in &request.sources {
            validate_computer_name(&source.name)?;
            if source.existing_member.is_none() {
                capture_bytes = capture_bytes.saturating_add(estimated_tree_bytes(
                    &store.join("sandboxes").join(&source.name),
                    cancellation,
                )?);
            }
        }
        let existing = estimated_tree_bytes(&store.join("snapshots"), cancellation)?
            .saturating_add(estimated_tree_bytes(&store.join("cache"), cancellation)?);
        // Multiple selected computers can include the same ancestors/image.
        let estimate = existing
            .saturating_mul(request.sources.len() as u64)
            .saturating_add(capture_bytes)
            .saturating_add(MAX_MANIFEST_BYTES);
        let estimate = estimate.saturating_add(estimate / 100); // tar/zstd overhead
        let destination = request.destination.parent().ok_or_else(|| {
            BackupError::InvalidRequest("The export destination has no parent directory.".into())
        })?;
        let writes = [
            (
                self.scratch_root.as_path(),
                estimate,
                "Silo's export working copy",
            ),
            (destination, estimate, "the export destination"),
            (store, capture_bytes, "Silo's checkpoint storage"),
        ];
        for (index, (path, _, label)) in writes.iter().enumerate() {
            let needed = writes
                .iter()
                .enumerate()
                .filter(|(other, (other_path, _, _))| {
                    *other == index || same_volume(path, other_path)
                })
                .fold(FREE_SPACE_RESERVE, |total, (_, (_, bytes, _))| {
                    total.saturating_add(*bytes)
                });
            let available = (self.free_space)(path)?;
            if available < needed {
                return Err(BackupError::InsufficientSpace(format!(
                    "This export needs an estimated {} of free space for {label} and any other export data on the same volume; {} is available.",
                    format_bytes(needed), format_bytes(available)
                )));
            }
        }
        Ok(())
    }

    fn captured_snapshot_path(
        &self,
        group: &str,
        name: &str,
        cancellation: &Cancellation,
    ) -> Result<PathBuf, BackupError> {
        let entries = self.snapshot_index("Locating captured computer disk", cancellation)?;
        let mut matches = entries.iter().filter(|entry| {
            entry["group"] == group && entry["name"] == name && entry["availability"] == "ready"
        });
        let entry = matches
            .next()
            .filter(|_| matches.next().is_none())
            .ok_or_else(|| {
                BackupError::InvalidRequest(
                    "The runtime did not publish exactly one ready captured checkpoint.".into(),
                )
            })?;
        self.member_artifact(entry, group)
    }

    /// The canonical artifact directory of an indexed member, confined to
    /// `<native store>/snapshots/<group>/<snapshot id>`.
    fn member_artifact(&self, entry: &Value, group: &str) -> Result<PathBuf, BackupError> {
        let id = entry["snapshot_id"]
            .as_str()
            .filter(|id| valid_snapshot_id(id))
            .ok_or_else(|| {
                BackupError::InvalidRequest(
                    "The runtime returned an invalid checkpoint identity.".into(),
                )
            })?;
        let path = Path::new(entry["artifact_path"].as_str().ok_or_else(|| {
            BackupError::InvalidRequest("The runtime omitted the captured checkpoint path.".into())
        })?);
        let native_store = self
            .command
            .storage_home
            .as_deref()
            .unwrap_or(&self.command.home)
            .join("snapshots");
        let native_store = fs::canonicalize(native_store)?;
        let path = fs::canonicalize(path)?;
        if !path.is_dir()
            || path.file_name().is_none_or(|part| part != id)
            || path
                .parent()
                .and_then(Path::file_name)
                .is_none_or(|part| part != group)
            || path.parent().and_then(Path::parent) != Some(native_store.as_path())
        {
            return Err(BackupError::InvalidRequest(
                "The captured checkpoint is outside the checkpoint storage.".into(),
            ));
        }
        Ok(path)
    }

    /// Check the whole archive, hashing every payload (the review before an import).
    pub(crate) fn inspect_archive(
        &self,
        archive: &Path,
        cancellation: &Cancellation,
    ) -> Result<ArchiveInspection, BackupError> {
        self.read_inspection(archive, cancellation, PayloadMode::VerifyAll)
    }

    /// Read the archive's header and manifest without reading any payload.
    /// The import itself verifies the selected payload while extracting it,
    /// so it needs no second full inspection (E-26).
    pub(crate) fn describe_archive(
        &self,
        archive: &Path,
        cancellation: &Cancellation,
    ) -> Result<ArchiveInspection, BackupError> {
        self.read_inspection(archive, cancellation, PayloadMode::Describe)
    }

    fn read_inspection(
        &self,
        archive: &Path,
        cancellation: &Cancellation,
        mode: PayloadMode<'_>,
    ) -> Result<ArchiveInspection, BackupError> {
        let package = read_and_verify_package(archive, self.max_archive_bytes, cancellation, mode)?;
        Ok(ArchiveInspection {
            created_at_ms: package.manifest.created_at_ms,
            size_bytes: package.size_bytes,
            computers: package
                .manifest
                .computers
                .iter()
                .map(|computer| computer.name.clone())
                .collect(),
        })
    }

    #[cfg(test)]
    pub(crate) fn prepare_restore(
        &self,
        request: RestoreRequest,
        cancellation: &Cancellation,
    ) -> Result<PreparedRestore, BackupError> {
        self.prepare_restore_in_group(request, &new_import_group(), cancellation, &|| Ok(()))
    }

    /// Like `prepare_restore`, but loads into a group the caller named first,
    /// so a journal can record the group before any native data exists and
    /// relaunch recovery can discard it after a crash.
    pub(crate) fn prepare_restore_in_group(
        &self,
        request: RestoreRequest,
        import_group: &str,
        cancellation: &Cancellation,
        before_load: &dyn Fn() -> Result<(), BackupError>,
    ) -> Result<PreparedRestore, BackupError> {
        let _guard = self.begin()?;
        if !valid_import_group(import_group) {
            return Err(BackupError::InvalidRequest(
                "The import checkpoint group name is invalid.".into(),
            ));
        }
        validate_computer_name(&request.new_name)?;
        let names = self.list_computer_names(cancellation)?;
        if names.contains(&request.new_name) {
            return Err(BackupError::Conflict(request.new_name));
        }
        fs::create_dir_all(&self.scratch_root)?;
        let stage = self.staging_directory("restore-")?;
        let store = self.native_store_root();
        let space = SpaceBudget {
            stage_free: (self.free_space)(stage.path())?,
            store_free: (self.free_space)(store)?,
            shared_volume: same_volume(stage.path(), store),
        };
        let package = read_and_verify_package(
            &request.archive,
            self.max_archive_bytes,
            cancellation,
            PayloadMode::Extract {
                dir: stage.path(),
                source: request.source_name.as_deref(),
                space,
            },
        )?;
        let extracted = package.extracted.as_ref().ok_or_else(|| {
            BackupError::InvalidArchive("checkpoint payload was not extracted".into())
        })?;
        let source = &package.manifest.computers[extracted.index];
        let payload_path = &extracted.path;
        // The pre-scan measured exactly what the runtime will unpack; check
        // again now that the private copy is written.
        let needed = extracted
            .scan
            .unpacked_bytes
            .saturating_add(FREE_SPACE_RESERVE);
        let available = (self.free_space)(store)?;
        if available < needed {
            return Err(BackupError::InsufficientSpace(format!(
                "Importing {} needs {} of free space in Silo's runtime storage; {} is available.",
                source.name,
                format_bytes(needed),
                format_bytes(available)
            )));
        }
        let before = self.snapshot_index("Checking imported checkpoint identity", cancellation)?;
        if before.iter().any(|entry| entry["group"] == import_group) {
            // Never load into, or clean up, a group this attempt did not create.
            return Err(BackupError::ImportGroupConflict(import_group.into()));
        }
        let stage_paths = native_import_stage_paths(self.native_store_root(), import_group)?;
        for path in &stage_paths {
            match fs::symlink_metadata(path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
                Ok(_) => return Err(BackupError::ImportGroupConflict(import_group.into())),
            }
        }
        // Ownership is established only after refusing any existing group, and
        // is journaled before load creates native members (including partials).
        before_load()?;
        // From here on the runtime may hold native data for this group. Every
        // failure, including Cancel and a timed-out or killed load, removes the
        // group and the runtime's leftover import staging before returning.
        match self.load_import_group(
            payload_path,
            import_group,
            &source.runtime_config,
            extracted.scan.unpacked_bytes,
            cancellation,
        ) {
            Ok(snapshot_member) => Ok(PreparedRestore {
                source_name: source.name.clone(),
                new_name: request.new_name,
                runtime_config: source.runtime_config.clone(),
                computer_configuration: source.computer_configuration.clone(),
                snapshot_group: import_group.into(),
                snapshot_member,
                _stage: stage,
            }),
            Err(error) => {
                if let Err(cleanup) = self.remove_import_group(import_group) {
                    eprintln!(
                        "Silo could not remove the incomplete import {import_group}: {cleanup}"
                    );
                }
                Err(error)
            }
        }
    }

    /// Load the verified payload into `import_group` and return its verified head member.
    fn load_import_group(
        &self,
        payload_path: &Path,
        import_group: &str,
        runtime_config: &Value,
        unpacked_bytes: u64,
        cancellation: &Cancellation,
    ) -> Result<String, BackupError> {
        let data_timeout = self.data_timeout(unpacked_bytes);
        self.require_success_with(
            "Loading computer disks from the export file",
            &[
                "snapshot".into(),
                "load".into(),
                payload_path.to_string_lossy().into_owned(),
                "--group".into(),
                import_group.into(),
                "--stage-id".into(),
                import_group["silo-import-".len()..].into(),
            ],
            data_timeout,
            cancellation,
        )?;
        // The CLI's printed reference is useful for diagnostics only. Resolve
        // the loaded member from the runtime's indexed JSON before using it.
        let entries = self.snapshot_index("Checking imported checkpoint", cancellation)?;
        let imported: Vec<_> = entries
            .iter()
            .filter(|entry| entry["group"] == import_group)
            .collect();
        if imported.is_empty()
            || imported
                .iter()
                .any(|entry| entry["availability"] != "ready")
        {
            return Err(BackupError::InvalidArchive(
                "the runtime did not publish a complete ready imported checkpoint group".into(),
            ));
        }
        let head = self.require_success(
            "Checking imported checkpoint head",
            &[
                "snapshot".into(),
                "head".into(),
                import_group.into(),
                "--format".into(),
                "json".into(),
            ],
            cancellation,
        )?;
        let head: Value = serde_json::from_str(&head.stdout).map_err(|_| {
            BackupError::InvalidArchive(
                "the runtime returned an invalid imported checkpoint head".into(),
            )
        })?;
        if head["group"] != import_group {
            return Err(BackupError::InvalidArchive(
                "the imported checkpoint head belongs to another group".into(),
            ));
        }
        let head_id = head["head"]
            .as_str()
            .filter(|id| valid_snapshot_id(id))
            .ok_or_else(|| {
                BackupError::InvalidArchive(
                    "the imported checkpoint head is missing or invalid".into(),
                )
            })?;
        let mut matches = imported
            .iter()
            .filter(|entry| entry["snapshot_id"] == head_id);
        let head_member = matches
            .next()
            .filter(|_| matches.next().is_none())
            .ok_or_else(|| {
                BackupError::InvalidArchive(
                    "the imported checkpoint head is not uniquely indexed".into(),
                )
            })?;
        let snapshot_member = head_member["name"]
            .as_str()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                BackupError::InvalidArchive(
                    "the runtime omitted the imported checkpoint member".into(),
                )
            })?
            .to_owned();
        self.require_success_with(
            "Verifying imported computer disk",
            &[
                "snapshot".into(),
                "verify".into(),
                format!("{import_group}:{snapshot_member}"),
            ],
            data_timeout,
            cancellation,
        )?;
        // `msb restore` rebuilds the computer from this descriptor, not from the
        // export manifest Silo validated, so the two must agree (E-19).
        let descriptor =
            read_snapshot_descriptor(&self.member_artifact(head_member, import_group)?)?;
        compare_loaded_descriptor(&descriptor, head_id, runtime_config).map_err(|detail| {
            BackupError::InvalidArchive(format!(
                "the loaded checkpoint does not match the export's settings: {detail}"
            ))
        })?;
        Ok(snapshot_member)
    }

    /// A crash can interrupt Silo after the runtime finished capture but before
    /// its journal was cleared. Verify that complete member before keeping it as
    /// a live computer's lineage parent; incomplete members must be removed.
    pub(crate) fn export_capture_ready(
        &self,
        group: &str,
        member: &str,
    ) -> Result<bool, BackupError> {
        let cleanup = Cancellation::default();
        let entries = self.snapshot_index("Checking interrupted export capture", &cleanup)?;
        if !entries.iter().any(|entry| {
            entry["group"] == group && entry["name"] == member && entry["availability"] == "ready"
        }) {
            return Ok(false);
        }
        let path = self.captured_snapshot_path(group, member, &cleanup)?;
        self.require_success_with(
            "Verifying interrupted export capture",
            &[
                "snapshot".into(),
                "verify".into(),
                path.to_string_lossy().into_owned(),
            ],
            self.command_timeout.min(CLEANUP_COMMAND_TIMEOUT),
            &cleanup,
        )?;
        Ok(true)
    }

    /// Remove a Silo import group that no computer uses: after a failed import,
    /// or during relaunch recovery of an interrupted one. Idempotent.
    pub(crate) fn discard_import_group(&self, group: &str) -> Result<(), BackupError> {
        let _guard = self.begin()?;
        self.remove_import_group(group)
    }

    /// Covers the steps between a successful `prepare_restore` and the saved
    /// computer: unless `keep` is called, dropping the guard discards the group.
    pub(crate) fn discard_import_on_failure(&self, group: &str) -> ImportGroupGuard<'_, R> {
        ImportGroupGuard {
            service: self,
            group: group.to_owned(),
            keep: false,
        }
    }

    /// Remove every member through `msb snapshot remove`, never `--force`:
    /// children before parents, with a root selected as head and removed
    /// last, because MicroSandbox refuses to remove a snapshot with indexed
    /// children or the head of a group that still has other members. Runs
    /// with its own cancellation so a cancelled import still cleans up.
    fn remove_import_group(&self, group: &str) -> Result<(), BackupError> {
        if !valid_import_group(group) {
            return Err(BackupError::InvalidRequest(
                "Only Silo import checkpoint groups can be discarded.".into(),
            ));
        }
        let cleanup = Cancellation::default();
        let timeout = self.command_timeout.min(CLEANUP_COMMAND_TIMEOUT);
        // A snapshot child can outlive Silo. Use the existing inherited lock
        // before direct filesystem cleanup, then release it for msb commands.
        let worker =
            wait_for_worker_lock(&self.command.home, worker_lock_timeout(timeout), &cleanup)?;
        cleanup_owned_native_import_stages(self.native_store_root(), group)?;
        drop(worker);
        let entries = self.snapshot_index_with("Checking incomplete import", timeout, &cleanup)?;
        let members: Vec<&Value> = entries
            .iter()
            .filter(|entry| entry["group"] == group)
            .collect();
        if members.is_empty() {
            return Ok(());
        }
        use crate::runtime::checkpoints::{native_removal_plan, NativeMember};
        // The runtime's historical parent_digest column holds a snapshot ID,
        // not a content digest. Reuse checkpoint cleanup's graph planner.
        let inventory: Vec<NativeMember> = members
            .iter()
            .map(|entry| {
                let mut member: NativeMember = serde_json::from_value((*entry).clone())?;
                if member.name.as_deref().is_none_or(str::is_empty)
                    && valid_snapshot_id(&member.snapshot_id)
                {
                    member.name = Some(member.snapshot_id.clone());
                }
                Ok::<_, BackupError>(member)
            })
            .collect::<Result<_, _>>()?;
        let candidates = inventory.iter().filter_map(NativeMember::key).collect();
        let plan = native_removal_plan(
            &inventory,
            &candidates,
            &Default::default(),
            &Default::default(),
        );
        if !plan.kept.is_empty() || plan.remove.len() != members.len() {
            return Err(BackupError::InvalidArchive(
                "the incomplete import has a cyclic or invalid checkpoint chain".into(),
            ));
        }
        let selector = |member: &NativeMember| -> Result<String, BackupError> {
            let (group, name) = member.key().ok_or_else(|| {
                BackupError::InvalidArchive("an imported checkpoint member has no identity".into())
            })?;
            Ok(format!("{group}:{name}"))
        };
        let mut ordered = plan.remove;
        let last = ordered.pop().expect("members is not empty");
        if !ordered.is_empty() {
            // Keep one root as the group's head so every other member can go.
            self.require_success_with(
                "Selecting the incomplete import to remove last",
                &["snapshot".into(), "head".into(), selector(&last)?],
                timeout,
                &cleanup,
            )?;
        }
        for entry in ordered {
            self.remove_snapshot_member(&selector(&entry)?, timeout, &cleanup)?;
        }
        self.remove_snapshot_member(&selector(&last)?, timeout, &cleanup)
    }

    fn remove_snapshot_member(
        &self,
        selector: &str,
        timeout: Duration,
        cancellation: &Cancellation,
    ) -> Result<(), BackupError> {
        self.require_success_with(
            "Removing incomplete import",
            &[
                "snapshot".into(),
                "remove".into(),
                "--quiet".into(),
                selector.into(),
            ],
            timeout,
            cancellation,
        )
        .map(|_| ())
    }

    fn snapshot_index(
        &self,
        operation: &str,
        cancellation: &Cancellation,
    ) -> Result<Vec<Value>, BackupError> {
        self.snapshot_index_with(operation, self.command_timeout, cancellation)
    }

    fn snapshot_index_with(
        &self,
        operation: &str,
        timeout: Duration,
        cancellation: &Cancellation,
    ) -> Result<Vec<Value>, BackupError> {
        let output = self.require_success_with(
            operation,
            &[
                "snapshot".into(),
                "list".into(),
                "--format".into(),
                "json".into(),
            ],
            timeout,
            cancellation,
        )?;
        serde_json::from_str(&output.stdout).map_err(|_| {
            BackupError::InvalidRequest("The runtime returned an invalid checkpoint index.".into())
        })
    }

    fn list_computer_names(
        &self,
        cancellation: &Cancellation,
    ) -> Result<HashSet<String>, BackupError> {
        let output = self.require_success(
            "Checking computer name",
            &["list".into(), "--format".into(), "json".into()],
            cancellation,
        )?;
        let value: Value =
            serde_json::from_str(&output.stdout).map_err(|_| BackupError::CommandFailed {
                operation: "Checking computer name".into(),
                detail: "the bundled runtime returned malformed computer data".into(),
            })?;
        let rows = value.as_array().ok_or_else(|| BackupError::CommandFailed {
            operation: "Checking computer name".into(),
            detail: "the bundled runtime returned an unexpected computer list".into(),
        })?;
        rows.iter()
            .map(|row| {
                row.get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| BackupError::CommandFailed {
                        operation: "Checking computer name".into(),
                        detail: "the bundled runtime omitted a computer name".into(),
                    })
            })
            .collect()
    }

    fn require_success(
        &self,
        operation: &str,
        arguments: &[String],
        cancellation: &Cancellation,
    ) -> Result<CommandOutput, BackupError> {
        self.require_success_with(operation, arguments, self.command_timeout, cancellation)
    }

    fn require_success_with(
        &self,
        operation: &str,
        arguments: &[String],
        timeout: Duration,
        cancellation: &Cancellation,
    ) -> Result<CommandOutput, BackupError> {
        let output = self
            .runner
            .run(&self.command, arguments, timeout, cancellation)?;
        if output.status.success() {
            return Ok(output);
        }
        let detail = if output.stderr.is_empty() {
            if output.stdout.is_empty() {
                "the bundled runtime returned no error detail".into()
            } else {
                output.stdout.clone()
            }
        } else {
            output.stderr.clone()
        };
        Err(BackupError::CommandFailed {
            operation: operation.into(),
            detail,
        })
    }
}

/// MicroSandbox 0.7.4 (`packages/microsandbox-types/rust/lib/snapshot`):
/// every installed member has one `snapshot.json` descriptor of at most 1 MiB.
const SNAPSHOT_DESCRIPTOR: &str = "snapshot.json";
const MAX_SNAPSHOT_DESCRIPTOR_BYTES: u64 = 1024 * 1024;
const OWNED_VOLUMES_EXTENSION: &str = "microsandbox.owned-volumes";
const RESTORE_DEFAULTS_EXTENSION: &str = "microsandbox.restore-defaults";

fn read_snapshot_descriptor(artifact: &Path) -> Result<Value, BackupError> {
    let invalid =
        || BackupError::InvalidArchive("the loaded checkpoint descriptor is unreadable".into());
    let (file, metadata) =
        open_regular_file(&artifact.join(SNAPSHOT_DESCRIPTOR)).map_err(|error| match error {
            OpenRegularError::NotRegular => invalid(),
            OpenRegularError::Io(error) => BackupError::Io(error),
        })?;
    if metadata.len() > MAX_SNAPSHOT_DESCRIPTOR_BYTES {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(MAX_SNAPSHOT_DESCRIPTOR_BYTES)
        .read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

fn descriptor_scope_supported(scope: Option<&str>, state_kind: Option<&str>) -> bool {
    matches!(
        (scope, state_kind),
        (Some("file" | "disk"), Some("file")) | (Some("checkpoint" | "full"), Some("checkpoint"))
    )
}

/// The restorable configuration a MicroSandbox snapshot carries is its
/// descriptor: image, root layout, owned volumes, the default user and, for
/// a full checkpoint, the computer geometry. Env, patches, init, rlimits and
/// host-bound mounts have no descriptor field (it is closed with
/// `deny_unknown_fields`). Environment defaults are reapplied from the export
/// manifest on Start; host-bound resources are not portable. The descriptor must match the export
/// manifest's validated configuration exactly, and anything else in it is
/// refused.
fn compare_loaded_descriptor(
    descriptor: &Value,
    head_id: &str,
    runtime_config: &Value,
) -> Result<(), String> {
    let object = descriptor
        .as_object()
        .ok_or("its descriptor is not an object")?;
    const FIELDS: &[&str] = &[
        "schema",
        "snapshot_id",
        "scope",
        "state",
        "capture",
        "image",
        "root_disk",
        "parent",
        "requires",
        "extensions",
    ];
    if let Some(extra) = object.keys().find(|key| !FIELDS.contains(&key.as_str())) {
        return Err(format!("it adds an undeclared setting ({extra})"));
    }
    if object.get("schema").and_then(Value::as_str) != Some("microsandbox.snapshot/1") {
        return Err("its descriptor schema is not supported".into());
    }
    if object.get("snapshot_id").and_then(Value::as_str) != Some(head_id) {
        return Err("its descriptor names another checkpoint".into());
    }
    let state_kind = descriptor.pointer("/state/kind").and_then(Value::as_str);
    // MicroSandbox's descriptor names its scope `file` or `checkpoint`, the same
    // word as `state.kind` (0.7.2 through 0.7.6: `SnapshotScope` serializes with
    // `rename = "file"` / `"checkpoint"`). The runtime's index and `snapshot list`
    // call the same scopes `disk` and `full`; accept those spellings here too, so
    // a descriptor written with the index names is never refused.
    if !descriptor_scope_supported(object.get("scope").and_then(Value::as_str), state_kind) {
        return Err("its capture scope is not supported".into());
    }

    // Image: same reference and, when the export recorded it, the same digest.
    let image = object
        .get("image")
        .and_then(Value::as_object)
        .ok_or("it has no image")?;
    if image
        .keys()
        .any(|key| key != "reference" && key != "manifest_digest")
        || image.get("reference").and_then(Value::as_str).is_none()
        || image.get("reference") != runtime_config.pointer("/image/Oci/reference")
    {
        return Err("it uses a different image".into());
    }
    if let Some(expected) = runtime_config
        .get("manifest_digest")
        .filter(|value| !value.is_null())
    {
        if image.get("manifest_digest") != Some(expected) {
            return Err("it uses a different image digest".into());
        }
    }
    // Root layout: the export requires a managed OCI root (absent means managed).
    if object
        .get("root_disk")
        .is_some_and(|root| root != &serde_json::json!({"layout": "managed"}))
    {
        return Err("it uses a different root disk layout".into());
    }

    let requires = object
        .get("requires")
        .and_then(Value::as_array)
        .ok_or("its required extensions are malformed")?;
    let extensions = object
        .get("extensions")
        .and_then(Value::as_object)
        .ok_or("its extensions are malformed")?;
    let known = [OWNED_VOLUMES_EXTENSION, RESTORE_DEFAULTS_EXTENSION];
    if requires
        .iter()
        .any(|key| !key.as_str().is_some_and(|key| known.contains(&key)))
    {
        return Err("it requires an unsupported runtime extension".into());
    }
    if let Some(extra) = extensions.keys().find(|key| !known.contains(&key.as_str())) {
        return Err(format!("it adds an undeclared extension ({extra})"));
    }

    // Default user for new commands: the validated export has none.
    let user = extensions
        .get(RESTORE_DEFAULTS_EXTENSION)
        .map(|defaults| defaults.get("user").cloned().unwrap_or(Value::Null))
        .unwrap_or(Value::Null);
    if &user
        != runtime_config
            .pointer("/runtime/user")
            .unwrap_or(&Value::Null)
    {
        return Err("it sets a different default user".into());
    }

    // Owned volumes: exactly the owned mounts the export declares.
    let expected: Vec<&Value> = runtime_config
        .get("mounts")
        .and_then(Value::as_array)
        .map(|mounts| {
            mounts
                .iter()
                .filter(|mount| mount["type"] == "Owned")
                .collect()
        })
        .unwrap_or_default();
    let captured: Vec<&Value> = match extensions.get(OWNED_VOLUMES_EXTENSION) {
        None => Vec::new(),
        Some(volumes) => volumes
            .as_array()
            .ok_or("its owned volumes are malformed")?
            .iter()
            .collect(),
    };
    if captured.len() != expected.len() {
        return Err("it adds or omits a mounted volume".into());
    }
    for declared in &expected {
        let found = captured.iter().find(|volume| {
            volume.pointer("/mount/guest").is_some()
                && volume.pointer("/mount/guest") == declared.get("guest")
        });
        let Some(volume) = found else {
            return Err("it mounts a volume at an undeclared path".into());
        };
        // The CLI omits default directory policies in some config output.
        // OwnedMountSnapshot always records them; compare the actual upstream
        // defaults as well as fields explicitly present in the manifest.
        for (field, default) in [
            ("storage", Value::Null),
            (
                "options",
                serde_json::json!({"readonly":false,"noexec":false,"nosuid":false,"nodev":false}),
            ),
            ("stat_virtualization", serde_json::json!("strict")),
            ("host_permissions", serde_json::json!("private")),
        ] {
            let expected = declared.get(field).unwrap_or(&default);
            if volume.pointer(&format!("/mount/{field}")) != Some(expected) {
                return Err(format!(
                    "its {} volume has different {field}",
                    declared["guest"].as_str().unwrap_or("owned")
                ));
            }
        }
    }

    // A full checkpoint restores with its captured geometry.
    if state_kind == Some("checkpoint") {
        for (captured, declared) in [
            ("vcpus", "cpus"),
            ("max_vcpus", "max_cpus"),
            ("memory_mib", "memory_mib"),
            ("max_memory_mib", "max_memory_mib"),
        ] {
            let captured = descriptor
                .pointer(&format!("/state/requirements_summary/{captured}"))
                .and_then(Value::as_u64);
            if captured.is_none()
                || captured
                    != runtime_config
                        .pointer(&format!("/resources/{declared}"))
                        .and_then(Value::as_u64)
            {
                return Err("its CPU or memory layout differs from the export's settings".into());
            }
        }
    }
    Ok(())
}

/// The journaled import group contains the caller-supplied stage ID. Never
/// discover cleanup candidates through a directory census, name prefix, or age.
fn native_import_stage_paths(home: &Path, group: &str) -> Result<[PathBuf; 2], BackupError> {
    if !valid_import_group(group) {
        return Err(BackupError::InvalidRequest(
            "The import stage identity is invalid.".into(),
        ));
    }
    // `home` is the configured storage root (not the runtime's short-path alias).
    // Refuse redirected stage parents before checking or removing any stage.
    for relative in ["snapshots", "cache", "cache/tmp"] {
        match fs::symlink_metadata(home.join(relative)) {
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err(BackupError::InvalidRequest(
                    "Import staging has a redirected or invalid parent; data was preserved.".into(),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
    }
    let id = &group["silo-import-".len()..];
    Ok([
        home.join("snapshots")
            .join(format!(".msb-snapshot-load-{id}")),
        home.join("cache/tmp").join(format!("snapshot-load-{id}")),
    ])
}

fn cleanup_owned_native_import_stages(home: &Path, group: &str) -> Result<(), BackupError> {
    let paths = native_import_stage_paths(home, group)?;
    // Validate both leaves before mutating either. A symlink is never followed.
    for path in &paths {
        match fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err(BackupError::InvalidRequest(
                    "Import staging has been replaced; data was preserved.".into(),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
    }
    for path in paths {
        match fs::remove_dir_all(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(()) => {}
        }
    }
    Ok(())
}

struct VerifiedPackage {
    manifest: PackageManifest,
    /// The one payload extracted by `PayloadMode::Extract`.
    extracted: Option<ExtractedPayload>,
    size_bytes: u64,
}

struct ExtractedPayload {
    index: usize,
    path: PathBuf,
    scan: PayloadScan,
}

/// Free space where an import stages its payload and where the runtime
/// unpacks it (E-25).
#[derive(Clone, Copy, Debug)]
struct SpaceBudget {
    stage_free: u64,
    store_free: u64,
    /// Both on one volume: the staged copy also uses the runtime's space.
    shared_volume: bool,
}

enum PayloadMode<'a> {
    /// Validate the header, manifest and length only; read no payload.
    Describe,
    /// Hash every payload: the review inspection and the export's final check.
    VerifyAll,
    /// Verify, pre-scan and extract only the selected computer's payload; skip
    /// the others without reading them.
    Extract {
        dir: &'a Path,
        source: Option<&'a str>,
        space: SpaceBudget,
    },
}

fn read_and_verify_package(
    path: &Path,
    max_archive_bytes: u64,
    cancellation: &Cancellation,
    mode: PayloadMode<'_>,
) -> Result<VerifiedPackage, BackupError> {
    check_cancelled(cancellation)?;
    let (mut file, metadata) = open_regular_file(path).map_err(|error| match error {
        OpenRegularError::NotRegular => {
            BackupError::InvalidArchive("the selected item is not a regular file".into())
        }
        OpenRegularError::Io(error) => BackupError::Io(error),
    })?;
    if metadata.len() > max_archive_bytes {
        return Err(BackupError::InvalidArchive(format!(
            "the export file exceeds the {} byte safety limit",
            max_archive_bytes
        )));
    }
    let mut magic = [0_u8; MAGIC.len()];
    file.read_exact(&mut magic)
        .map_err(|_| BackupError::InvalidArchive("the file header is incomplete".into()))?;
    if &magic != MAGIC {
        return Err(BackupError::InvalidArchive(
            "the file header is not recognized".into(),
        ));
    }
    let version = read_u32(&mut file)?;
    check_format_version(version)?;
    let manifest_len = read_u64(&mut file)?;
    if manifest_len == 0 || manifest_len > MAX_MANIFEST_BYTES {
        return Err(BackupError::InvalidArchive(
            "the manifest size is invalid".into(),
        ));
    }
    let mut manifest_bytes = vec![0; manifest_len as usize];
    file.read_exact(&mut manifest_bytes)
        .map_err(|_| BackupError::InvalidArchive("the manifest is incomplete".into()))?;
    let manifest: PackageManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| BackupError::InvalidArchive("the manifest is malformed".into()))?;
    validate_manifest(&manifest)?;

    let header_len = MAGIC.len() as u64 + 4 + 8 + manifest_len;
    // `validate_manifest` rejects separate volume payloads, so the snapshot
    // payloads are the whole body.
    let payload_total = manifest
        .computers
        .iter()
        .try_fold(0_u64, |sum, computer| {
            sum.checked_add(computer.payload_size)
        })
        .ok_or_else(|| BackupError::InvalidArchive("payload sizes overflow".into()))?;
    let expected_len = header_len
        .checked_add(payload_total)
        .ok_or_else(|| BackupError::InvalidArchive("export file size overflows".into()))?;
    if expected_len != metadata.len() {
        return Err(BackupError::InvalidArchive(
            "the export file length does not match its manifest".into(),
        ));
    }

    let extract = match mode {
        PayloadMode::Describe => {
            return Ok(VerifiedPackage {
                manifest,
                extracted: None,
                size_bytes: metadata.len(),
            });
        }
        PayloadMode::VerifyAll => None,
        PayloadMode::Extract { dir, source, space } => {
            Some((dir, select_restore_source(&manifest, source)?, space))
        }
    };
    let mut extracted = None;
    for (index, computer) in manifest.computers.iter().enumerate() {
        let label = format!("checkpoint payload for {}", computer.name);
        match extract {
            None => {
                extract_verified_payload(
                    &mut file,
                    computer.payload_size,
                    &computer.payload_sha256,
                    None,
                    &label,
                    cancellation,
                )?;
            }
            Some((dir, selected, space)) if selected == index => {
                // The private copy of the payload must fit where it is staged,
                // and when that is the runtime's volume it also shrinks what
                // the runtime can unpack.
                let staged = computer.payload_size.saturating_add(FREE_SPACE_RESERVE);
                if space.stage_free < staged {
                    return Err(BackupError::InsufficientSpace(format!(
                        "Importing {} needs {} of free space for its private working copy; {} is available.",
                        computer.name,
                        format_bytes(staged),
                        format_bytes(space.stage_free)
                    )));
                }
                let unpack_budget = space
                    .store_free
                    .saturating_sub(FREE_SPACE_RESERVE)
                    .saturating_sub(if space.shared_volume {
                        computer.payload_size
                    } else {
                        0
                    })
                    .min(DEFAULT_MAX_ARCHIVE_BYTES);
                let (path, scan) = extract_scanned_payload(
                    &mut file,
                    computer.payload_size,
                    &computer.payload_sha256,
                    dir.join(format!("snapshot-{index}.tar.zst")),
                    &label,
                    ScanLimits {
                        max_unpacked_bytes: unpack_budget,
                        max_entries: MAX_SNAPSHOT_ENTRIES,
                        max_sparse_bytes: largest_declared_disk(&computer.computer_configuration),
                    },
                    cancellation,
                )
                .map_err(|error| match error {
                    BackupError::InsufficientSpace(_) => BackupError::InsufficientSpace(format!(
                        "Importing {} needs more than the {} of free space available to Silo's runtime storage (keeping {} free).",
                        computer.name,
                        format_bytes(unpack_budget),
                        format_bytes(FREE_SPACE_RESERVE)
                    )),
                    error => error,
                })?;
                extracted = Some(ExtractedPayload { index, path, scan });
            }
            Some(_) => {
                let skip = i64::try_from(computer.payload_size)
                    .map_err(|_| BackupError::InvalidArchive("payload sizes overflow".into()))?;
                file.seek(SeekFrom::Current(skip))?;
            }
        }
    }
    Ok(VerifiedPackage {
        manifest,
        extracted,
        size_bytes: metadata.len(),
    })
}

/// Both disks of a computer: an upper bound for the data a capture, check or
/// save of it reads before compression.
fn declared_storage_bytes(computer_configuration: &Value) -> u64 {
    ["workspaceStorageGiB", "runtimeStorageGiB"]
        .iter()
        .filter_map(|field| computer_configuration.get(*field).and_then(Value::as_u64))
        .fold(0_u64, u64::saturating_add)
        .saturating_mul(1024 * 1024 * 1024)
}

/// Size of the larger of the computer's two disks; `validate_computer_configuration`
/// has already bounded both.
fn largest_declared_disk(computer_configuration: &Value) -> u64 {
    ["workspaceStorageGiB", "runtimeStorageGiB"]
        .iter()
        .filter_map(|field| computer_configuration.get(*field).and_then(Value::as_u64))
        .max()
        .unwrap_or(0)
        .saturating_mul(1024 * 1024 * 1024)
}

/// The bundled MicroSandbox version, read from the checked-in runtime inputs
/// that also pin the runtime build, so an upgrade cannot leave exports
/// claiming the old version.
fn bundled_runtime_version() -> &'static str {
    static VERSION: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        let inputs: Value = serde_json::from_str(include_str!("../../runtime-inputs.json"))
            .expect("checked-in runtime inputs must be valid");
        inputs["microsandboxVersion"]
            .as_str()
            .expect("runtime inputs name the MicroSandbox version")
            .to_owned()
    });
    &VERSION
}

/// Earlier MicroSandbox versions whose snapshot archives the bundled runtime
/// still loads. Add a version here only after loading one of its exports
/// with the new runtime; archives from versions not listed are refused.
const EARLIER_IMPORTABLE_RUNTIME_VERSIONS: &[&str] = &["0.7.2", "0.7.4"];

/// MicroSandbox names its archive format per minor release.
fn snapshot_format_for(version: &str) -> String {
    let minor = version.splitn(3, '.').take(2).collect::<Vec<_>>().join(".");
    format!("msb-snapshot-tar-zstd-v{minor}")
}

fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|part| part.parse::<u64>().ok());
    let parsed = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(parsed)
}

fn check_format_version(version: u32) -> Result<(), BackupError> {
    match version.cmp(&FORMAT_VERSION) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Less => Err(BackupError::UnsupportedArchive(format!(
            "This export uses an earlier Silo export format (version {version}) that this Silo cannot import. Import it with the Silo version that created it, then export it again."
        ))),
        std::cmp::Ordering::Greater => Err(BackupError::UnsupportedArchive(format!(
            "This export was created by a newer version of Silo (export format {version}). Update Silo to import it."
        ))),
    }
}

fn check_runtime_compatibility(runtime: &RuntimeManifest) -> Result<(), BackupError> {
    let current = bundled_runtime_version();
    if runtime.name != "microsandbox" {
        return Err(BackupError::InvalidArchive(
            "it was not created with MicroSandbox".into(),
        ));
    }
    let importable = runtime.version == current
        || EARLIER_IMPORTABLE_RUNTIME_VERSIONS.contains(&runtime.version.as_str());
    if importable {
        if runtime.snapshot_format != snapshot_format_for(&runtime.version) {
            return Err(BackupError::InvalidArchive(
                "its checkpoint format does not match its runtime version".into(),
            ));
        }
        return Ok(());
    }
    let newer = matches!(
        (parse_version(&runtime.version), parse_version(current)),
        (Some(archive), Some(bundled)) if archive > bundled
    );
    Err(BackupError::UnsupportedArchive(if newer {
        format!(
            "This export was created with MicroSandbox {}, newer than the {current} bundled with this Silo. Update Silo to import it.",
            runtime.version
        )
    } else {
        format!(
            "This export was created with MicroSandbox {}, which the {current} bundled with this Silo cannot import. Import it with the Silo version that created it, then export it again.",
            runtime.version
        )
    }))
}

fn validate_manifest(manifest: &PackageManifest) -> Result<(), BackupError> {
    let architecture = manifest.runtime.guest_architecture.as_str();
    if !matches!(architecture, "aarch64" | "x86_64") || architecture != std::env::consts::ARCH {
        return Err(BackupError::InvalidArchive(format!(
            "This export file requires {architecture}; Silo on this device supports {}. Import it on a device with the required architecture.",
            std::env::consts::ARCH
        )));
    }
    check_format_version(manifest.schema_version)?;
    check_runtime_compatibility(&manifest.runtime)?;
    if manifest.computers.is_empty() || manifest.computers.len() > 64 {
        return Err(BackupError::InvalidArchive(
            "the computer count is outside supported limits".into(),
        ));
    }
    let mut names = HashSet::new();
    for computer in &manifest.computers {
        validate_computer_name(&computer.name)
            .map_err(|_| BackupError::InvalidArchive("a computer name is invalid".into()))?;
        if !names.insert(&computer.name) {
            return Err(BackupError::InvalidArchive(
                "computer names must be unique".into(),
            ));
        }
        let config_size = serde_json::to_vec(&serde_json::json!({
            "runtimeConfig": computer.runtime_config,
            "computerConfiguration": computer.computer_configuration,
        }))
        .map_err(|_| BackupError::InvalidArchive("checkpoint metadata is malformed".into()))?
        .len() as u64;
        if computer.payload_size == 0
            || !is_sha256(&computer.payload_sha256)
            || config_size > MAX_MANIFEST_BYTES
        {
            return Err(BackupError::InvalidArchive(
                "checkpoint metadata is invalid".into(),
            ));
        }
        validate_snapshottable_config(&computer.name, &computer.runtime_config)
            .map_err(|error| BackupError::InvalidArchive(error.to_string()))?;
        validate_computer_configuration(&computer.name, &computer.computer_configuration)
            .map_err(|error| BackupError::InvalidArchive(error.to_string()))?;
        validate_package_volumes(&computer.volumes)?;
        validate_volume_contract(&computer.runtime_config, &computer.computer_configuration)?;
    }
    Ok(())
}

fn validate_export_configs(
    name: &str,
    runtime_config: &Value,
    computer_configuration: &Value,
) -> Result<(), BackupError> {
    validate_snapshottable_config(name, runtime_config)?;
    validate_computer_configuration(name, computer_configuration)?;
    validate_volume_sources(name, runtime_config, computer_configuration)
}

/// A checkpoint export packs the disk (and, for a full checkpoint, the computer
/// geometry) as captured, while the computer may have been changed since.
/// Take everything the checkpoint's descriptor records from it: the owned
/// workspace capacity and, for a full checkpoint, CPUs and memory (with the
/// default /tmp size that follows memory). The root disk size cannot change
/// after creation, so it is kept.
fn apply_captured_layout(
    name: &str,
    descriptor: &Value,
    runtime_config: &mut Value,
    computer_configuration: &mut Value,
) -> Result<(), BackupError> {
    let unsupported = |detail: &str| {
        BackupError::UnsupportedStorage(format!(
            "The checkpoint of {name} cannot be exported: {detail}."
        ))
    };
    let computer = descriptor
        .pointer(&format!("/extensions/{OWNED_VOLUMES_EXTENSION}"))
        .and_then(Value::as_array)
        .and_then(|volumes| {
            volumes.iter().find(|volume| {
                volume.pointer("/mount/guest").and_then(Value::as_str) == Some("/workspace")
            })
        })
        .ok_or_else(|| unsupported("it does not include the workspace disk"))?;
    let capacity_mib = computer
        .pointer("/mount/storage/capacity_mib")
        .and_then(Value::as_u64)
        .filter(|mib| *mib > 0 && mib % 1024 == 0)
        .ok_or_else(|| unsupported("its workspace disk size is not a whole number of GiB"))?;
    if let Some(mount) = runtime_config
        .get_mut("mounts")
        .and_then(Value::as_array_mut)
        .and_then(|mounts| {
            mounts
                .iter_mut()
                .find(|mount| mount["type"] == "Owned" && mount["guest"] == "/workspace")
        })
    {
        mount["storage"]["capacity_mib"] = capacity_mib.into();
    }
    computer_configuration["workspaceStorageGiB"] = (capacity_mib / 1024).into();

    if descriptor.pointer("/state/kind").and_then(Value::as_str) == Some("checkpoint") {
        let captured = |field: &str| {
            descriptor
                .pointer(&format!("/state/requirements_summary/{field}"))
                .and_then(Value::as_u64)
                .ok_or_else(|| unsupported("its CPU and memory layout is not recorded"))
        };
        let (cpus, max_cpus) = (captured("vcpus")?, captured("max_vcpus")?);
        let (memory_mib, max_memory_mib) = (captured("memory_mib")?, captured("max_memory_mib")?);
        if memory_mib % 1024 != 0 || max_memory_mib % 1024 != 0 {
            return Err(unsupported("its memory size is not a whole number of GiB"));
        }
        runtime_config["resources"]["cpus"] = cpus.into();
        runtime_config["resources"]["max_cpus"] = max_cpus.into();
        runtime_config["resources"]["memory_mib"] = memory_mib.into();
        runtime_config["resources"]["max_memory_mib"] = max_memory_mib.into();
        computer_configuration["cpus"] = cpus.into();
        computer_configuration["maxCPUs"] = max_cpus.into();
        computer_configuration["memoryGiB"] = (memory_mib / 1024).into();
        computer_configuration["maxMemoryGiB"] = (max_memory_mib / 1024).into();
        // Silo sizes the default /tmp from memory; keep that relation.
        if let Some(tmpfs) = runtime_config
            .get_mut("mounts")
            .and_then(Value::as_array_mut)
            .and_then(|mounts| mounts.iter_mut().find(|mount| mount["type"] == "Tmpfs"))
        {
            tmpfs["size_mib"] = (memory_mib / 4).clamp(1, 512).into();
        }
    }
    Ok(())
}

fn validate_package_volumes(volumes: &[Value]) -> Result<(), BackupError> {
    if !volumes.is_empty() {
        return Err(BackupError::InvalidArchive(
            "workspace disks must be carried by the MicroSandbox checkpoint".into(),
        ));
    }
    Ok(())
}

fn validate_volume_sources(
    name: &str,
    runtime_config: &Value,
    computer_configuration: &Value,
) -> Result<(), BackupError> {
    validate_volume_contract(runtime_config, computer_configuration).map_err(|_| {
        BackupError::UnsupportedStorage(format!(
            "{name} disk metadata does not match its Silo computer configuration."
        ))
    })
}

fn validate_volume_contract(
    runtime_config: &Value,
    computer_configuration: &Value,
) -> Result<(), BackupError> {
    for (runtime_field, computer_field, multiplier) in [
        ("cpus", "cpus", 1),
        ("max_cpus", "maxCPUs", 1),
        ("memory_mib", "memoryGiB", 1024),
        ("max_memory_mib", "maxMemoryGiB", 1024),
    ] {
        let actual = runtime_config
            .get("resources")
            .and_then(|resources| resources.get(runtime_field))
            .and_then(Value::as_u64);
        let expected = computer_configuration
            .get(computer_field)
            .and_then(Value::as_u64)
            .and_then(|value| value.checked_mul(multiplier));
        if actual != expected || actual.is_none() {
            return Err(BackupError::InvalidArchive(
                "computer resources do not match the saved computer settings".into(),
            ));
        }
    }
    let workspace_mib = computer_configuration
        .get("workspaceStorageGiB")
        .and_then(Value::as_u64)
        .and_then(|size| size.checked_mul(1024));
    let owned_workspace = runtime_config
        .get("mounts")
        .and_then(Value::as_array)
        .is_some_and(|mounts| {
            mounts
                .iter()
                .filter(|mount| mount.get("guest").and_then(Value::as_str) == Some("/workspace"))
                .collect::<Vec<_>>()
                .as_slice()
                .iter()
                .any(|mount| {
                    mount["type"] == "Owned"
                        && mount.pointer("/storage/kind").and_then(Value::as_str) == Some("disk")
                        && mount
                            .pointer("/storage/capacity_mib")
                            .and_then(Value::as_u64)
                            == workspace_mib
                })
        });
    if !owned_workspace
        || runtime_config
            .pointer("/image/Oci/root_disk/size_mib")
            .and_then(Value::as_u64)
            != computer_configuration
                .get("runtimeStorageGiB")
                .and_then(Value::as_u64)
                .and_then(|size| size.checked_mul(1024))
    {
        return Err(BackupError::InvalidArchive(
            "computer disk capacities do not match the computer settings".into(),
        ));
    }
    let workspace_mounts = runtime_config
        .get("mounts")
        .and_then(Value::as_array)
        .ok_or_else(|| BackupError::InvalidArchive("computer mounts are missing".into()))?
        .iter()
        .filter(|mount| mount.get("guest").and_then(Value::as_str) == Some("/workspace"))
        .collect::<Vec<_>>();
    if workspace_mounts.len() != 1 {
        return Err(BackupError::InvalidArchive(
            "computer disk mounts do not match the computer settings".into(),
        ));
    }
    Ok(())
}

fn select_restore_source(
    manifest: &PackageManifest,
    selected: Option<&str>,
) -> Result<usize, BackupError> {
    match selected {
        Some(name) => manifest
            .computers
            .iter()
            .position(|computer| computer.name == name)
            .ok_or_else(|| {
                BackupError::InvalidRequest(format!("{name} is not in this export file."))
            }),
        None if manifest.computers.len() == 1 => Ok(0),
        None => Err(BackupError::InvalidRequest(
            "Choose which computer to import from this export file.".into(),
        )),
    }
}

fn validate_backup_request(request: &BackupRequest) -> Result<(), BackupError> {
    if request.sources.is_empty() || request.sources.len() > 64 {
        return Err(BackupError::InvalidRequest(
            "Choose between 1 and 64 computers to export.".into(),
        ));
    }
    if request
        .destination
        .extension()
        .and_then(|value| value.to_str())
        != Some("silo-backup")
    {
        return Err(BackupError::InvalidRequest(
            "The export filename must end in .silo-backup. Choose a filename with that extension."
                .into(),
        ));
    }
    if request.destination.exists() {
        return Err(BackupError::FileConflict(
            request.destination.display().to_string(),
        ));
    }
    let mut names = HashSet::new();
    if request
        .sources
        .iter()
        .any(|source| !names.insert(&source.name))
    {
        return Err(BackupError::InvalidRequest(
            "Each computer may appear only once in an export file.".into(),
        ));
    }
    Ok(())
}

fn validate_computer_name(name: &str) -> Result<(), BackupError> {
    let valid = !name.is_empty()
        && name.len() <= 32
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if !valid {
        return Err(BackupError::InvalidRequest(
            "Computer names must start with a letter and contain only lowercase letters, numbers, and hyphens.".into(),
        ));
    }
    Ok(())
}

fn validate_computer_configuration(name: &str, config: &Value) -> Result<(), BackupError> {
    let object = config.as_object().ok_or_else(|| {
        BackupError::InvalidRequest(format!("{name} has invalid Silo computer metadata."))
    })?;
    const FIELDS: &[&str] = &[
        "id",
        "name",
        "cpus",
        "maxCPUs",
        "memoryGiB",
        "maxMemoryGiB",
        "workspaceStorageGiB",
        "runtimeStorageGiB",
    ];
    if object.len() != FIELDS.len() + usize::from(object.contains_key("desktop"))
        || object
            .keys()
            .any(|key| key != "desktop" && !FIELDS.contains(&key.as_str()))
        || object.get("desktop").is_some_and(|desktop| {
            serde_json::from_value::<crate::desktop::DesktopConfiguration>(desktop.clone()).is_err()
        })
        || object.get("name").and_then(Value::as_str) != Some(name)
        || object
            .get("id")
            .and_then(Value::as_str)
            .is_none_or(|id| uuid::Uuid::try_parse(id).is_err())
    {
        return Err(BackupError::InvalidRequest(format!(
            "{name} has invalid Silo computer metadata."
        )));
    }
    let number = |field: &str| object.get(field).and_then(Value::as_u64);
    let (Some(cpus), Some(max_cpus), Some(memory), Some(max_memory), Some(computer), Some(runtime)) = (
        number("cpus"),
        number("maxCPUs"),
        number("memoryGiB"),
        number("maxMemoryGiB"),
        number("workspaceStorageGiB"),
        number("runtimeStorageGiB"),
    ) else {
        return Err(BackupError::InvalidRequest(format!(
            "{name} has invalid Silo computer metadata."
        )));
    };
    if cpus == 0
        || cpus > max_cpus
        || max_cpus > u8::MAX as u64
        || memory == 0
        || memory > max_memory
        || max_memory > u32::MAX as u64
        || computer == 0
        || runtime == 0
        || computer
            .checked_add(runtime)
            .and_then(|gib| gib.checked_mul(1024))
            .is_none_or(|mib| mib > u32::MAX as u64)
    {
        return Err(BackupError::InvalidRequest(format!(
            "{name} has invalid Silo computer metadata."
        )));
    }
    Ok(())
}

pub(crate) fn validate_snapshottable_config(name: &str, config: &Value) -> Result<(), BackupError> {
    let object = config.as_object().ok_or_else(|| {
        BackupError::UnsupportedStorage(format!(
            "{name} has no verified MicroSandbox configuration, so a complete export cannot be created."
        ))
    })?;
    const ALLOWED_CONFIG_FIELDS: &[&str] = &[
        "name",
        "image",
        "resources",
        "runtime",
        "env",
        "labels",
        "rlimits",
        "mounts",
        "patches",
        "network",
        "vsock",
        "init",
        "pull_policy",
        "security_profile",
        "deployment_profile",
        "lifecycle",
        "manifest_digest",
    ];
    if object
        .keys()
        .any(|key| !ALLOWED_CONFIG_FIELDS.contains(&key.as_str()))
        || object.get("name").and_then(Value::as_str) != Some(name)
    {
        return Err(BackupError::UnsupportedStorage(format!(
            "{name} has a configuration this Silo build cannot import safely."
        )));
    }
    if object.get("labels").is_some_and(|labels| {
        !labels.as_object().is_some_and(|labels| {
            labels.iter().all(|(key, value)| {
                !key.is_empty()
                    && !key.contains(['=', '\0'])
                    && value.as_str().is_some_and(|value| !value.contains('\0'))
            })
        })
    }) {
        return Err(BackupError::UnsupportedStorage(format!(
            "{name} has labels this Silo build cannot import."
        )));
    }
    if object.get("env").is_some_and(|env| {
        !env.as_array().is_some_and(|env| {
            env.iter().all(|entry| {
                let key = entry.get("key").and_then(Value::as_str);
                let value = entry.get("value").and_then(Value::as_str);
                entry.as_object().is_some_and(|entry| entry.len() == 2)
                    && key.is_some_and(|key| !key.is_empty() && !key.contains(['=', '\0']))
                    && value.is_some_and(|value| !value.contains('\0'))
            })
        })
    }) {
        return Err(BackupError::UnsupportedStorage(format!(
            "{name} has environment variables that Silo cannot carry in an export."
        )));
    }
    if let Some(setting) = unsupported_runtime_setting(object) {
        return Err(BackupError::UnsupportedStorage(format!(
            "{name} has custom {setting} settings that Silo cannot carry in an export."
        )));
    }
    let mounts = object
        .get("mounts")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            BackupError::UnsupportedStorage(format!(
            "{name} does not report its mounted storage, so a complete export cannot be created."
        ))
        })?;
    if !silo_disk_mounts_with_optional_tmpfs(object, mounts) {
        return Err(BackupError::UnsupportedStorage(format!(
            "{name} does not use the Silo-owned workspace disk-image mount required for a complete export."
        )));
    }
    if object
        .get("patches")
        .is_some_and(|patches| !patches.as_array().is_some_and(Vec::is_empty))
        || object
            .get("vsock")
            .is_some_and(|vsock| !vsock.as_object().is_some_and(serde_json::Map::is_empty))
        || object.get("network").is_some_and(network_uses_host_files)
    {
        return Err(BackupError::UnsupportedStorage(format!(
            "{name} uses host-linked configuration that this Silo build cannot import safely."
        )));
    }
    let image = object
        .get("image")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            BackupError::UnsupportedStorage(format!(
                "{name} is not an OCI-rooted computer with a managed disk."
            ))
        })?;
    let oci = image.get("Oci").and_then(Value::as_object).ok_or_else(|| {
        BackupError::UnsupportedStorage(format!(
            "{name} is not an OCI-rooted computer with a managed disk."
        ))
    })?;
    match oci.get("root_disk") {
        None | Some(Value::Null) => Ok(()),
        Some(root) if root.get("kind").and_then(Value::as_str) == Some("managed") => Ok(()),
        Some(_) => Err(BackupError::UnsupportedStorage(format!(
            "{name} does not use the managed OCI root disk required by MicroSandbox 0.7.4 checkpoints."
        ))),
    }
}

// Silo currently creates Ubuntu computers with these runtime defaults. Reject overrides
// that the restore command does not reproduce instead of silently changing them.
// Only the exact credential-free profile installed during Silo computer creation is
// restorable here. Custom policies, host secret references and values stay blocked.
pub(crate) fn default_github_network(network: &Value) -> bool {
    with_profile_defaults(network) == github_network_defaults()
}

fn github_network_defaults() -> Value {
    serde_json::from_str(include_str!("../guest/github-network-default.json"))
        .expect("checked-in GitHub network defaults")
}

/// MicroSandbox's `strict` option only changes connections that a hostname-based
/// allow rule (`domain` or `domain_suffix`) admits: they need an inspectable
/// request authority. Silo's profile has no such rule, so either value behaves
/// the same there.
fn strict_is_inert(network: &Value) -> bool {
    network
        .get("policy")
        .and_then(|policy| policy.get("rules"))
        .is_none_or(|rules| {
            rules.as_array().is_some_and(|rules| {
                rules.iter().all(|rule| {
                    rule.get("destination").is_some_and(|destination| {
                        destination.get("domain").is_none()
                            && destination.get("domain_suffix").is_none()
                    })
                })
            })
        })
}

/// Silo creates computers with `strict` on, the profile's value, and the runtime
/// gives restores and forks its current default (also on). The saved value
/// still differs by origin: MicroSandbox 0.7.2 saved its then-default (off), and
/// a configuration saved by 0.6.17 holds none until a newer runtime rewrites it.
/// Archives exported before the profile changed carry the 0.7.2 value as well.
/// Where it cannot change anything (see `strict_is_inert`), and imports create
/// every computer with the profile's value anyway, such a network counts as
/// having the profile's value. Anything else, such as a value that is not a
/// boolean, stays as it is and fails the comparison.
fn with_profile_defaults(network: &Value) -> Value {
    let mut network = network.clone();
    // MicroSandbox 0.7.5 and later save two more fields, both at their runtime
    // defaults for a computer Silo created or restored: readable denial responses
    // (opt-in, off) and the NAT64 prefix. Earlier runtimes save neither, so the
    // profile omits them and a default value counts as absent. A changed value
    // stays and fails the comparison as a custom network setting.
    if let Some(object) = network.as_object_mut() {
        if object.get("http") == Some(&serde_json::json!({"deny_response": false})) {
            object.remove("http");
        }
        if object.get("nat64_prefixes") == Some(&serde_json::json!(["64:ff9b::/96"])) {
            object.remove("nat64_prefixes");
        }
    }
    let boolean_or_missing = network.get("strict").is_none_or(Value::is_boolean);
    if boolean_or_missing && strict_is_inert(&network) {
        if let (Some(object), Some(strict)) = (
            network.as_object_mut(),
            github_network_defaults().get("strict"),
        ) {
            object.insert("strict".into(), strict.clone());
        }
    }
    network
}

/// The network an import starts from: Silo's profile with a deny-all policy.
/// An import applies its own policy and current secret assignments anyway.
fn imported_deny_network_value() -> Value {
    let mut expected = github_network_defaults();
    expected["policy"] = serde_json::json!({
        "default_egress": "deny",
        "default_ingress": "deny",
        "rules": []
    });
    expected
}

fn imported_deny_network(network: &Value) -> bool {
    with_profile_defaults(network) == imported_deny_network_value()
}

/// An export carries no network authority: the importing Silo applies a
/// deny-all policy and its own secret assignments (see
/// `checkpoints::import_pending_restore`). So a Silo network profile with a
/// custom policy, assigned secret references or published ports is exported
/// with those parts reset (E-30). Anything else that differs from the
/// profile is a real blocker and is named.
fn portable_export_network(name: &str, runtime_config: &mut Value) -> Result<(), BackupError> {
    let Some(network) = runtime_config.get_mut("network") else {
        return Ok(());
    };
    // The archive carries the profile's `strict`, whichever value the source had.
    *network = with_profile_defaults(network);
    if default_github_network(network)
        || imported_deny_network(network)
        || network.get("policy").is_some_and(Value::is_null)
    {
        return Ok(());
    }
    let expected = imported_deny_network_value();
    let mut candidate = network.clone();
    for field in ["policy", "secrets", "ports"] {
        candidate[field] = expected[field].clone();
    }
    if let (Some(candidate), Some(expected)) = (candidate.as_object(), expected.as_object()) {
        let differing = expected
            .keys()
            .chain(candidate.keys())
            .find(|key| candidate.get(*key) != expected.get(*key));
        if let Some(field) = differing {
            return Err(BackupError::UnsupportedStorage(format!(
                "{name} has custom network {field} settings that Silo cannot carry in an export."
            )));
        }
    } else {
        return Err(BackupError::UnsupportedStorage(format!(
            "{name} has network settings that Silo cannot carry in an export."
        )));
    }
    *network = candidate;
    Ok(())
}

/// Names the first runtime setting an export cannot carry, if any.
fn unsupported_runtime_setting(config: &serde_json::Map<String, Value>) -> Option<String> {
    let expected = [
        (
            "runtime",
            serde_json::json!({"workdir":null,"shell":"/bin/sh","scripts":{},"entrypoint":null,"cmd":["/bin/bash"],"hostname":null,"user":null,"log_level":null,"metrics_sample_interval_ms":1000,"disable_metrics_sample":false}),
        ),
        (
            "network",
            serde_json::json!({"enabled":true,"ports":[],"interface":null,"policy":null,"dns":null,"tls":null,"secrets":null,"max_connections":null,"rate_limiter":null,"trust_host_cas":false,"outbound_proxy":null}),
        ),
        (
            "lifecycle",
            serde_json::json!({"ephemeral":false,"max_duration_secs":null,"idle_timeout_secs":null}),
        ),
    ];
    for (field, defaults) in expected {
        if let Some(value) = config.get(field) {
            if field == "network" && (default_github_network(value) || imported_deny_network(value))
            {
                continue;
            }
            let Some(fields) = value.as_object() else {
                return Some(field.into());
            };
            if let Some((key, _)) = fields.iter().find(|(key, value)| {
                if field == "runtime" && (*key == "cmd" || *key == "shell") && value.is_null() {
                    return false;
                }
                defaults.get(*key) != Some(*value)
            }) {
                return Some(format!("{field} {key}"));
            }
        }
    }
    for (field, default) in [
        ("security_profile", "default"),
        ("deployment_profile", "single_tenant"),
        ("pull_policy", "IfMissing"),
    ] {
        if config.get(field).is_some_and(|value| {
            value.as_str() != Some(default)
                && !(field == "pull_policy" && value.as_str() == Some("Never"))
        }) {
            return Some(field.replace('_', " "));
        }
    }
    if !config.get("init").is_none_or(Value::is_null) {
        return Some("init".into());
    }
    if !config
        .get("rlimits")
        .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty))
    {
        return Some("resource limit".into());
    }
    let plain_resources = config
        .get("resources")
        .and_then(Value::as_object)
        .is_some_and(|resources| {
            resources.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "cpus" | "max_cpus" | "memory_mib" | "max_memory_mib"
                )
            })
        });
    (!plain_resources).then(|| "CPU and memory".into())
}

fn silo_disk_mounts_with_optional_tmpfs(
    config: &serde_json::Map<String, Value>,
    mounts: &[Value],
) -> bool {
    let Some(memory_mib) = config
        .get("resources")
        .and_then(|resources| resources.get("memory_mib"))
        .and_then(Value::as_u64)
    else {
        return false;
    };
    let expected_size = (memory_mib / 4).clamp(1, 512);
    let disks = mounts
        .iter()
        .filter(|mount| mount.get("type").and_then(Value::as_str) == Some("Owned"))
        .collect::<Vec<_>>();
    if disks.len() != 1 || !disks.iter().all(|mount| valid_owned_workspace_mount(mount)) {
        return false;
    }
    let tmpfs = mounts
        .iter()
        .filter(|mount| mount.get("type").and_then(Value::as_str) == Some("Tmpfs"))
        .collect::<Vec<_>>();
    if mounts.len() != disks.len() + tmpfs.len() || tmpfs.len() > 1 {
        return false;
    }
    let Some(mount) = tmpfs.first() else {
        return true;
    };
    let Some(mount) = mount.as_object() else {
        return false;
    };
    const ALLOWED_TMPFS_FIELDS: &[&str] = &["type", "guest", "size_mib", "options"];
    if mount
        .keys()
        .any(|key| !ALLOWED_TMPFS_FIELDS.contains(&key.as_str()))
        || mount.get("type").and_then(Value::as_str) != Some("Tmpfs")
        || mount.get("guest").and_then(Value::as_str) != Some("/tmp")
        || mount.get("size_mib").and_then(Value::as_u64) != Some(expected_size)
    {
        return false;
    }
    let Some(options) = mount.get("options").and_then(Value::as_object) else {
        return false;
    };
    const ALLOWED_OPTION_FIELDS: &[&str] = &[
        "readonly",
        "noexec",
        "nosuid",
        "nodev",
        "override_uid",
        "override_gid",
    ];
    !options
        .keys()
        .any(|key| !ALLOWED_OPTION_FIELDS.contains(&key.as_str()))
        && ["readonly", "noexec", "nosuid", "nodev"]
            .iter()
            .all(|key| options.get(*key).and_then(Value::as_bool) == Some(false))
        && ["override_uid", "override_gid"]
            .iter()
            .all(|key| options.get(*key).is_none_or(Value::is_null))
}

fn valid_owned_workspace_mount(mount: &Value) -> bool {
    mount.get("type").and_then(Value::as_str) == Some("Owned")
        && mount.get("guest").and_then(Value::as_str) == Some("/workspace")
        && mount.pointer("/storage/kind").and_then(Value::as_str) == Some("disk")
        && mount
            .pointer("/storage/capacity_mib")
            .and_then(Value::as_u64)
            .is_some_and(|size| size > 0)
}

fn network_uses_host_files(network: &Value) -> bool {
    let Some(tls) = network.get("tls") else {
        return false;
    };
    tls.get("upstream_ca_cert")
        .is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty))
        || tls
            .get("scoped_upstream_ca_cert")
            .is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty))
        || tls
            .get("intercept_ca")
            .and_then(Value::as_object)
            .is_some_and(|ca| {
                ["cert_path", "key_path"]
                    .iter()
                    .any(|key| ca.get(*key).is_some_and(|value| !value.is_null()))
            })
}

/// Caps for the MicroSandbox snapshot archive (`.tar.zst`) inside an export,
/// checked before `msb snapshot load` sees it (E-20). MicroSandbox 0.7.4
/// rejects absolute and `..` paths and non-regular entry types itself, but
/// has no aggregate size or entry-count limit.
#[derive(Clone, Copy, Debug)]
struct ScanLimits {
    /// Decompressed tar stream bytes: everything `load` can write densely.
    max_unpacked_bytes: u64,
    /// Every tar entry, including directories.
    max_entries: u64,
    /// Logical size of a sparse entry (a disk layer), which may exceed its data.
    max_sparse_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PayloadScan {
    pub(crate) entries: u64,
    pub(crate) unpacked_bytes: u64,
}

enum ScanFailure {
    Cancelled,
    TooLarge,
    Unsafe(String),
}

/// Reads the archive's payload region once: hashes it, writes it to the
/// private stage, and hands the same bytes to the pre-scan.
struct HashingTee<'a> {
    input: io::Take<&'a mut File>,
    hasher: Sha256,
    output: &'a mut File,
    cancellation: &'a Cancellation,
}

impl Read for HashingTee<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancellation.cancelled() {
            return Err(io::Error::other("cancelled"));
        }
        let count = self.input.read(buffer)?;
        self.hasher.update(&buffer[..count]);
        self.output.write_all(&buffer[..count])?;
        Ok(count)
    }
}

/// Stops decompression as soon as the stream passes its byte budget, so a
/// small highly compressed payload cannot expand without bound.
struct CountingReader<R> {
    inner: R,
    count: u64,
    limit: u64,
    exceeded: bool,
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.count += count as u64;
        if self.count > self.limit {
            self.exceeded = true;
            return Err(io::Error::other("unpacked size limit"));
        }
        Ok(count)
    }
}

/// Stream a zstd-compressed tar through the caps: only regular, sparse and
/// directory entries with relative, normal paths; no PAX or long link
/// headers; bounded entry count, decompressed size and sparse logical size.
fn scan_snapshot_archive(
    reader: impl Read,
    limits: ScanLimits,
    cancellation: &Cancellation,
) -> Result<PayloadScan, ScanFailure> {
    let decoder = zstd::stream::read::Decoder::new(reader)
        .map_err(|error| ScanFailure::Unsafe(format!("it is not a zstd stream ({error})")))?;
    let counting = CountingReader {
        inner: decoder,
        count: 0,
        limit: limits.max_unpacked_bytes,
        exceeded: false,
    };
    let mut archive = tar::Archive::new(counting);
    let mut entries_seen = 0_u64;
    let outcome = (|| -> Result<(), ScanFailure> {
        let entries = archive
            .entries()
            .map_err(|error| ScanFailure::Unsafe(error.to_string()))?;
        for entry in entries {
            if cancellation.cancelled() {
                return Err(ScanFailure::Cancelled);
            }
            let mut entry = entry.map_err(|error| ScanFailure::Unsafe(error.to_string()))?;
            entries_seen += 1;
            if entries_seen > limits.max_entries {
                return Err(ScanFailure::Unsafe(format!(
                    "it has more than {} entries",
                    limits.max_entries
                )));
            }
            let kind = entry.header().entry_type();
            if !(kind.is_file() || kind.is_contiguous() || kind.is_gnu_sparse() || kind.is_dir()) {
                return Err(ScanFailure::Unsafe(
                    "it contains a link, device or other special entry".into(),
                ));
            }
            if entry
                .pax_extensions()
                .map_err(|error| ScanFailure::Unsafe(error.to_string()))?
                .is_some()
                || entry.link_name_bytes().is_some_and(|name| !name.is_empty())
            {
                return Err(ScanFailure::Unsafe(
                    "it contains extended or link headers".into(),
                ));
            }
            let path = entry
                .path()
                .map_err(|error| ScanFailure::Unsafe(error.to_string()))?
                .into_owned();
            if path.as_os_str().is_empty()
                || path.to_str().is_none()
                || !path
                    .components()
                    .all(|component| matches!(component, std::path::Component::Normal(_)))
            {
                return Err(ScanFailure::Unsafe(format!(
                    "it contains the unsafe path {}",
                    path.display()
                )));
            }
            if kind.is_gnu_sparse() && entry.size() > limits.max_sparse_bytes {
                return Err(ScanFailure::Unsafe(format!(
                    "{} is larger than any disk this computer declares",
                    path.display()
                )));
            }
            if kind.is_dir() && entry.header().entry_size().unwrap_or(1) != 0 {
                return Err(ScanFailure::Unsafe("a directory entry carries data".into()));
            }
            // The iterator skips the entry's stored bytes by reading them
            // from the counted stream. Reading through `entry` would also
            // synthesize a sparse file's holes, which the budget must not pay for.
        }
        Ok(())
    })();
    let counting = archive.into_inner();
    if cancellation.cancelled() {
        return Err(ScanFailure::Cancelled);
    }
    if counting.exceeded {
        return Err(ScanFailure::TooLarge);
    }
    outcome?;
    Ok(PayloadScan {
        entries: entries_seen,
        unpacked_bytes: counting.count,
    })
}

/// Hash, extract and pre-scan the selected payload in one read.
fn extract_scanned_payload(
    archive: &mut File,
    size: u64,
    expected_sha256: &str,
    destination: PathBuf,
    label: &str,
    limits: ScanLimits,
    cancellation: &Cancellation,
) -> Result<(PathBuf, PayloadScan), BackupError> {
    check_cancelled(cancellation)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)?;
    let mut tee = HashingTee {
        input: archive.take(size),
        hasher: Sha256::new(),
        output: &mut output,
        cancellation,
    };
    let scanned = scan_snapshot_archive(&mut tee, limits, cancellation);
    let scan = match scanned {
        Ok(scan) => scan,
        Err(ScanFailure::Cancelled) => return Err(BackupError::Cancelled),
        Err(ScanFailure::TooLarge) => {
            // The caller words this with the free space it measured.
            return Err(BackupError::InsufficientSpace(format!(
                "the {label} unpacks to more than {}",
                format_bytes(limits.max_unpacked_bytes)
            )));
        }
        Err(ScanFailure::Unsafe(detail)) => {
            return Err(BackupError::InvalidArchive(format!(
                "the {label} is not safe checkpoint data: {detail}"
            )));
        }
    };
    // Hash and keep whatever follows the tar end marker too.
    io::copy(&mut tee, &mut io::sink()).map_err(|error| {
        if cancellation.cancelled() {
            BackupError::Cancelled
        } else {
            BackupError::Io(error)
        }
    })?;
    let remaining = tee.input.limit();
    let digest = format!("sha256:{:x}", tee.hasher.finalize());
    if remaining != 0 {
        return Err(BackupError::InvalidArchive(format!(
            "the {label} is incomplete"
        )));
    }
    if digest != expected_sha256 {
        return Err(BackupError::InvalidArchive(format!(
            "the {label} failed its integrity check"
        )));
    }
    output.sync_all()?;
    Ok((destination, scan))
}

/// Estimate data a sparse-aware archive copies without reading file contents.
/// Ignore symlinks rather than walking outside the runtime store; MicroSandbox
/// owns the disk and image paths under these roots. Check Cancel between entries.
fn estimated_tree_bytes(root: &Path, cancellation: &Cancellation) -> Result<u64, BackupError> {
    use std::os::unix::fs::MetadataExt;
    let mut pending = vec![root.to_owned()];
    let mut total = 0_u64;
    while let Some(path) = pending.pop() {
        check_cancelled(cancellation)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                check_cancelled(cancellation)?;
                pending.push(entry?.path());
            }
        } else if metadata.is_file() {
            total = total
                .saturating_add(metadata.len().min(metadata.blocks().saturating_mul(512)))
                .saturating_add(1024); // entry headers and alignment
        }
    }
    Ok(total)
}

/// Whether two paths (or their nearest existing ancestors) share a volume.
/// Unknown counts as shared, which only makes the space check stricter.
fn same_volume(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let device = |path: &Path| {
        path.ancestors()
            .find_map(|candidate| fs::metadata(candidate).ok())
            .map(|metadata| metadata.dev())
    };
    match (device(left), device(right)) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    }
}

/// `statvfs` of the nearest existing ancestor: space available to this
/// account (f_bavail), so a path that is not created yet still resolves.
fn available_bytes(path: &Path) -> io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let existing = path
        .ancestors()
        .find(|candidate| candidate.exists())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no existing parent folder"))?;
    let encoded = std::ffi::CString::new(existing.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid path"))?;
    // SAFETY: statvfs is plain data; zeroed is a valid initial value.
    let mut statistics: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: encoded is NUL terminated and statistics is a valid exclusive output pointer.
    if unsafe { libc::statvfs(encoded.as_ptr(), &mut statistics) } != 0 {
        return Err(io::Error::last_os_error());
    }
    #[allow(clippy::unnecessary_cast)] // The field widths differ between platforms.
    (statistics.f_bavail as u64)
        .checked_mul(statistics.f_frsize as u64)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid volume capacity"))
}

/// Binary units, labelled as such.
fn format_bytes(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn extract_verified_payload(
    archive: &mut File,
    size: u64,
    expected_sha256: &str,
    destination: Option<PathBuf>,
    label: &str,
    cancellation: &Cancellation,
) -> Result<Option<PathBuf>, BackupError> {
    check_cancelled(cancellation)?;
    let mut remaining = size;
    let mut hasher = Sha256::new();
    let mut output = match destination {
        Some(path) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            Some((path, file))
        }
        None => None,
    };
    let mut buffer = [0_u8; 128 * 1024];
    while remaining > 0 {
        check_cancelled(cancellation)?;
        let wanted = remaining.min(buffer.len() as u64) as usize;
        archive
            .read_exact(&mut buffer[..wanted])
            .map_err(|_| BackupError::InvalidArchive(format!("the {label} is incomplete")))?;
        hasher.update(&buffer[..wanted]);
        if let Some((_, file)) = output.as_mut() {
            file.write_all(&buffer[..wanted])?;
        }
        remaining -= wanted as u64;
    }
    let digest = format!("sha256:{:x}", hasher.finalize());
    if digest != expected_sha256 {
        return Err(BackupError::InvalidArchive(format!(
            "the {label} failed its integrity check"
        )));
    }
    if let Some((path, file)) = output {
        file.sync_all()?;
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

/// Publish `source` at `destination` without ever replacing a file there.
fn rename_without_replacing(source: &Path, destination: &Path) -> io::Result<()> {
    rename_without_replacing_with(
        source,
        destination,
        exclusive_rename,
        |source, destination| fs::hard_link(source, destination),
    )
}

/// Some volumes (NFS, SMB, exFAT and other FUSE or network file systems)
/// reject the exclusive-rename flag with EINVAL or ENOTSUP. Fall back to a
/// hard link, which also fails if the name is taken. Refuse publication when
/// neither primitive is available: renaming over a placeholder can replace
/// a different file that another writer published in the meantime.
fn rename_without_replacing_with(
    source: &Path,
    destination: &Path,
    exclusive: impl Fn(&Path, &Path) -> io::Result<()>,
    link: impl Fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let unsupported = |error: &io::Error| {
        matches!(
            error.raw_os_error(),
            Some(libc::EINVAL | libc::ENOTSUP | libc::EOPNOTSUPP | libc::ENOSYS)
        )
    };
    match exclusive(source, destination) {
        Err(error) if unsupported(&error) => {}
        result => return result,
    }
    match link(source, destination) {
        Ok(()) => {
            // The archive is published; a leftover temporary name is only clutter.
            let _ = fs::remove_file(source);
            Ok(())
        }
        Err(error)
            if unsupported(&error)
                || matches!(error.raw_os_error(), Some(libc::EPERM | libc::EMLINK)) =>
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "This filesystem cannot safely publish an export without replacing another file. Choose an export folder on a different filesystem.",
            ))
        }
        Err(error) => Err(error),
    }
}

/// The kernel's exclusive rename: atomic, and never replaces an existing file.
fn exclusive_rename(source: &Path, destination: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid source path"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid destination path"))?;
    // SAFETY: both paths are valid NUL-terminated strings; the flags prohibit replacement.
    #[cfg(target_os = "macos")]
    let result = unsafe {
        libc::renameatx_np(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// A placeholder with the exact length of a real digest, so the manifest can
/// be written before the payloads and rewritten in place once they are hashed.
fn pending_digest() -> String {
    format!("sha256:{}", "0".repeat(64))
}

/// Writes the archive, hashing each payload while it is copied (one read per
/// payload), then verifies the whole written file once before publishing it.
/// `payloads[i]` is the open, already-measured payload of `manifest.computers[i]`.
fn write_immutable_package(
    destination: &Path,
    manifest: &mut PackageManifest,
    payloads: Vec<File>,
    cancellation: &Cancellation,
    token: Option<&str>,
    free_space: fn(&Path) -> io::Result<u64>,
) -> Result<u64, BackupError> {
    let parent = destination.parent().ok_or_else(|| {
        BackupError::InvalidRequest("The export destination has no parent directory.".into())
    })?;
    if payloads.len() != manifest.computers.len() {
        return Err(BackupError::InvalidRequest(
            "The export payloads do not match its manifest.".into(),
        ));
    }
    fs::create_dir_all(parent)?;
    for computer in &mut manifest.computers {
        computer.payload_sha256 = pending_digest();
    }
    let manifest_bytes = serde_json::to_vec(manifest)?;
    if manifest_bytes.is_empty() || manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(BackupError::InvalidRequest(
            "Export metadata exceeds the supported size.".into(),
        ));
    }
    // Check the destination before copying possibly hundreds of gigabytes;
    // an external or nearly full volume would otherwise fail at the end.
    let archive_size = manifest
        .computers
        .iter()
        .try_fold(
            (MAGIC.len() + 4 + 8 + manifest_bytes.len()) as u64,
            |sum, computer| sum.checked_add(computer.payload_size),
        )
        .ok_or_else(|| {
            BackupError::InvalidRequest("The export size cannot be represented.".into())
        })?;
    let needed = archive_size.saturating_add(FREE_SPACE_RESERVE);
    let available = free_space(parent)?;
    if available < needed {
        return Err(BackupError::InsufficientSpace(format!(
            "This export needs {} of free space in {}; {} is available.",
            format_bytes(needed),
            parent.display(),
            format_bytes(available)
        )));
    }
    let prefix = token
        .map(|token| format!(".silo-backup-{token}-"))
        .unwrap_or_else(|| ".silo-backup-".into());
    let mut temporary = tempfile::Builder::new()
        .prefix(&prefix)
        .tempfile_in(parent)?;
    temporary.write_all(MAGIC)?;
    temporary.write_all(&FORMAT_VERSION.to_be_bytes())?;
    temporary.write_all(&(manifest_bytes.len() as u64).to_be_bytes())?;
    let manifest_offset = (MAGIC.len() + 4 + 8) as u64;
    temporary.write_all(&manifest_bytes)?;
    let mut buffer = [0_u8; 128 * 1024];
    for (computer, mut input) in manifest.computers.iter_mut().zip(payloads) {
        let mut hasher = Sha256::new();
        let mut copied = 0_u64;
        loop {
            check_cancelled(cancellation)?;
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
            temporary.write_all(&buffer[..count])?;
            copied += count as u64;
        }
        if copied != computer.payload_size {
            return Err(BackupError::InvalidRequest(format!(
                "The checkpoint export file for {} changed while it was being exported.",
                computer.name
            )));
        }
        computer.payload_sha256 = format!("sha256:{:x}", hasher.finalize());
    }
    let final_manifest = serde_json::to_vec(manifest)?;
    if final_manifest.len() != manifest_bytes.len() {
        return Err(BackupError::InvalidRequest(
            "Export metadata changed size while it was being written.".into(),
        ));
    }
    temporary
        .as_file_mut()
        .seek(SeekFrom::Start(manifest_offset))?;
    temporary.write_all(&final_manifest)?;
    temporary.as_file().sync_all()?;
    // Verify all written bytes before the atomic commit. Cancellation cannot turn
    // an already-published, verified archive into a reported cancellation.
    let verified = read_and_verify_package(
        temporary.path(),
        DEFAULT_MAX_ARCHIVE_BYTES,
        cancellation,
        PayloadMode::VerifyAll,
    )?;
    let size_bytes = verified.size_bytes;
    check_cancelled(cancellation)?;
    publish_package(temporary.path(), destination, parent, |parent| {
        File::open(parent).and_then(|directory| directory.sync_all())
    })?;
    Ok(size_bytes)
}

fn publish_package(
    source: &Path,
    destination: &Path,
    parent: &Path,
    sync_directory: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), BackupError> {
    rename_without_replacing(source, destination).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            BackupError::FileConflict(destination.display().to_string())
        } else {
            BackupError::Io(error)
        }
    })?;
    // Publication has committed. Keep the verified archive when durability cannot
    // be confirmed; the destination might already belong to another writer.
    sync_directory(parent)?;
    Ok(())
}

pub(crate) enum OpenRegularError {
    NotRegular,
    Io(io::Error),
}

/// Open without following a final symlink and without blocking on a FIFO,
/// then check the opened handle itself, so a path swapped after an earlier
/// check can neither redirect the read nor hang it. The returned metadata
/// (and its length) belongs to the handle that will be read.
pub(crate) fn open_regular_file(path: &Path) -> Result<(File, fs::Metadata), OpenRegularError> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| {
            if error.raw_os_error() == Some(libc::ELOOP) {
                OpenRegularError::NotRegular
            } else {
                OpenRegularError::Io(error)
            }
        })?;
    let metadata = file.metadata().map_err(OpenRegularError::Io)?;
    if !metadata.is_file() {
        return Err(OpenRegularError::NotRegular);
    }
    Ok((file, metadata))
}

/// Open a payload the runtime wrote and return it with its size; it is
/// hashed later while being copied into the archive.
fn open_payload(path: &Path, max_bytes: u64) -> Result<(File, u64), BackupError> {
    let not_regular = || {
        BackupError::InvalidArchive(
            "the runtime did not create a regular checkpoint export file".into(),
        )
    };
    let (file, metadata) = open_regular_file(path).map_err(|error| match error {
        OpenRegularError::NotRegular => not_regular(),
        OpenRegularError::Io(error) => BackupError::Io(error),
    })?;
    if metadata.len() == 0 {
        return Err(not_regular());
    }
    if metadata.len() > max_bytes {
        return Err(BackupError::InvalidArchive(
            "the checkpoint export file exceeds the configured safety limit".into(),
        ));
    }
    Ok((file, metadata.len()))
}

fn read_u32(reader: &mut impl Read) -> Result<u32, BackupError> {
    let mut bytes = [0_u8; 4];
    reader
        .read_exact(&mut bytes)
        .map_err(|_| BackupError::InvalidArchive("the file header is incomplete".into()))?;
    Ok(u32::from_be_bytes(bytes))
}

fn read_u64(reader: &mut impl Read) -> Result<u64, BackupError> {
    let mut bytes = [0_u8; 8];
    reader
        .read_exact(&mut bytes)
        .map_err(|_| BackupError::InvalidArchive("the file header is incomplete".into()))?;
    Ok(u64::from_be_bytes(bytes))
}

fn check_cancelled(cancellation: &Cancellation) -> Result<(), BackupError> {
    if cancellation.cancelled() {
        Err(BackupError::Cancelled)
    } else {
        Ok(())
    }
}

/// MicroSandbox snapshot identity: `snap_` and 32 hex digits.
fn valid_snapshot_id(id: &str) -> bool {
    id.len() == 37
        && id.starts_with("snap_")
        && id[5..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The only native groups export/import may create or discard: `silo-import-`
/// and 32 lowercase hex digits.
pub(crate) fn valid_import_group(group: &str) -> bool {
    group.strip_prefix("silo-import-").is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

pub(crate) fn new_import_group() -> String {
    format!("silo-import-{}", uuid::Uuid::new_v4().simple())
}

fn is_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn unique_suffix() -> String {
    format!("{}-{}", std::process::id(), now_ms())
}

#[cfg(test)]
pub(crate) use tests::write_finished_export;

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::sync::{atomic::AtomicU64, Mutex};

    fn tar_header(path: &str, kind: tar::EntryType, size: u64) -> tar::Header {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_entry_type(kind);
        header.set_size(size);
        header.set_mode(0o644);
        header.set_cksum();
        header
    }

    fn zstd_tar(build: impl FnOnce(&mut tar::Builder<Vec<u8>>)) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        build(&mut builder);
        zstd::encode_all(builder.into_inner().unwrap().as_slice(), 3).unwrap()
    }

    /// The shape of a real `msb snapshot save` archive, in miniature.
    fn fake_snapshot_archive() -> Vec<u8> {
        zstd_tar(|builder| {
            builder
                .append(
                    &tar_header("snapshots", tar::EntryType::Directory, 0),
                    io::empty(),
                )
                .unwrap();
            builder
                .append(
                    &tar_header(
                        "snapshots/snap_11111111111111111111111111111111/snapshot.json",
                        tar::EntryType::Regular,
                        2,
                    ),
                    &b"{}"[..],
                )
                .unwrap();
        })
    }

    /// What `msb snapshot save` of a disk capture of `config` describes.
    fn loaded_descriptor_for(config: &Value) -> Value {
        let volumes = config["mounts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|mount| mount["type"] == "Owned")
            .map(|mount| {
                serde_json::json!({
                    "mount_id": "workspace",
                    "mount": {
                        "guest": mount["guest"],
                        "storage": mount["storage"],
                        "options": mount["options"],
                        "stat_virtualization": "strict",
                        "host_permissions": "private"
                    },
                    "data": {"kind": "disk", "generation": {}}
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "schema": "microsandbox.snapshot/1",
            "snapshot_id": "snap_11111111111111111111111111111111",
            "scope": "file",
            "state": {"kind": "file", "disk_format": "qcow2", "filesystem": "ext4", "virtual_size": 1, "head": "layer_1", "layers": []},
            "capture": {"created_at": "2026-09-30T00:00:00Z", "source_lineage": "dev", "source_checkpoint": null, "consistency": "crash_consistent"},
            "image": {"reference": config["image"]["Oci"]["reference"], "manifest_digest": format!("sha256:{}", "c".repeat(64))},
            "root_disk": {"layout": "managed"},
            "parent": "snap_00000000000000000000000000000000",
            "requires": [OWNED_VOLUMES_EXTENSION],
            "extensions": {OWNED_VOLUMES_EXTENSION: volumes}
        })
    }

    #[derive(Default)]
    struct FakeRunner {
        calls: Mutex<Vec<Vec<String>>>,
        existing: Mutex<Vec<String>>,
        /// Pre-captured `(group, member)` snapshots published by `snapshot list`,
        /// so a checkpoint export can locate a member without a fresh capture.
        existing_members: Mutex<Vec<(String, String)>>,
        /// `group:member` selectors removed through `snapshot remove`.
        removed: Mutex<Vec<String>>,
        /// Crafted `.tar.zst` written by `snapshot save` instead of a valid one.
        saved_payload: Mutex<Option<Vec<u8>>>,
        /// Descriptor of the loaded head instead of one matching `managed_config("dev")`.
        loaded_descriptor: Mutex<Option<Value>>,
        /// Descriptor of the pre-captured `existing_members`, same default.
        member_descriptor: Mutex<Option<Value>>,
        timeouts: Mutex<Vec<(Vec<String>, Duration)>>,
        fail_load: AtomicBool,
        fail_save: AtomicBool,
        fail_import_verify: AtomicBool,
        cancel_during_load: AtomicBool,
        invalid_import_head: AtomicBool,
        cancel_after_snapshot: AtomicBool,
    }

    impl MsbRunner for FakeRunner {
        fn run(
            &self,
            command: &MsbCommand,
            arguments: &[String],
            timeout: Duration,
            cancellation: &Cancellation,
        ) -> Result<CommandOutput, BackupError> {
            check_cancelled(cancellation)?;
            self.calls.lock().unwrap().push(arguments.to_vec());
            self.timeouts
                .lock()
                .unwrap()
                .push((arguments.to_vec(), timeout));
            let success = || CommandOutput {
                status: ExitStatus::from_raw(0),
                stdout: String::new(),
                stderr: String::new(),
            };
            match arguments
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice()
            {
                ["snapshot", "create", _, "--from-sandbox", _, "--group", group, "--guest-flush", _, "--integrity", "--quiet"] =>
                {
                    let snapshot = command
                        .home
                        .join("snapshots")
                        .join(group)
                        .join("snap_00000000000000000000000000000000");
                    fs::create_dir_all(&snapshot)?;
                    fs::write(snapshot.join("snapshot.json"), b"{}")?;
                    if self.cancel_after_snapshot.load(Ordering::Acquire) {
                        cancellation.cancel();
                    }
                    Ok(success())
                }
                ["snapshot", "verify", target]
                    if target.starts_with("silo-import-")
                        && self.fail_import_verify.load(Ordering::Acquire) =>
                {
                    Ok(CommandOutput {
                        status: ExitStatus::from_raw(1 << 8),
                        stderr: "simulated verification failure".into(),
                        ..success()
                    })
                }
                ["snapshot", "verify", _] => Ok(success()),
                ["snapshot", "head", _selector] => Ok(success()),
                ["snapshot", "remove", "--quiet", selector] => {
                    self.removed.lock().unwrap().push((*selector).to_owned());
                    Ok(success())
                }
                ["snapshot", "save", _, output, "--with-parents", "--with-image"] => {
                    if self.fail_save.load(Ordering::Acquire) {
                        return Ok(CommandOutput {
                            status: ExitStatus::from_raw(1 << 8),
                            stderr: "simulated archive write failure".into(),
                            ..success()
                        });
                    }
                    let payload = self
                        .saved_payload
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or_else(fake_snapshot_archive);
                    fs::write(output, payload)?;
                    Ok(success())
                }
                ["snapshot", "load", _, "--group", group, "--stage-id", id] => {
                    assert_eq!(*id, &group["silo-import-".len()..]);
                    if self.cancel_during_load.load(Ordering::Acquire) {
                        // The runner killed msb after it had installed members.
                        for path in native_import_stage_paths(&command.home, group)? {
                            fs::create_dir_all(path.join("partial"))?;
                        }
                        cancellation.cancel();
                        return Err(BackupError::Cancelled);
                    }
                    if self.fail_load.load(Ordering::Acquire) {
                        for path in native_import_stage_paths(&command.home, group)? {
                            fs::create_dir_all(path.join("partial"))?;
                        }
                        return Ok(CommandOutput {
                            status: ExitStatus::from_raw(1 << 8),
                            stderr: "unsafe archive member".into(),
                            ..success()
                        });
                    }
                    Ok(CommandOutput {
                        stdout: format!("sha256:fake\n{group}:imported-member\n"),
                        ..success()
                    })
                }
                ["snapshot", "list", "--format", "json"] => {
                    let calls = self.calls.lock().unwrap();
                    let mut entries = Vec::new();
                    for (group, member) in self.existing_members.lock().unwrap().iter() {
                        let snapshot = command
                            .home
                            .join("snapshots")
                            .join(group)
                            .join("snap_00000000000000000000000000000000");
                        fs::create_dir_all(&snapshot)?;
                        let descriptor = self
                            .member_descriptor
                            .lock()
                            .unwrap()
                            .clone()
                            .unwrap_or_else(|| loaded_descriptor_for(&managed_config("dev")));
                        fs::write(snapshot.join("snapshot.json"), descriptor.to_string())?;
                        entries.push(serde_json::json!({
                            "group": group,
                            "name": member,
                            "availability": "ready",
                            "snapshot_id": "snap_00000000000000000000000000000000",
                            "artifact_path": snapshot
                        }));
                    }
                    if let Some(create) = calls
                        .iter()
                        .rev()
                        .find(|call| call.get(1).is_some_and(|part| part == "create"))
                    {
                        let group = &create[6];
                        entries.push(serde_json::json!({
                            "group": group,
                            "name": create[2],
                            "availability": "ready",
                            "snapshot_id": "snap_00000000000000000000000000000000",
                            "artifact_path": command.home.join("snapshots").join(group).join("snap_00000000000000000000000000000000")
                        }));
                    }
                    if let Some(load) = calls
                        .iter()
                        .rev()
                        .find(|call| call.get(1).is_some_and(|part| part == "load"))
                    {
                        let group = &load[4];
                        let artifact = |id: &str, descriptor: &Value| -> io::Result<PathBuf> {
                            let path = command.home.join("snapshots").join(group).join(id);
                            fs::create_dir_all(&path)?;
                            fs::write(path.join("snapshot.json"), descriptor.to_string())?;
                            Ok(path)
                        };
                        let head = self
                            .loaded_descriptor
                            .lock()
                            .unwrap()
                            .clone()
                            .unwrap_or_else(|| loaded_descriptor_for(&managed_config("dev")));
                        let parent = format!("sha256:{}", "a".repeat(64));
                        entries.push(serde_json::json!({"group":group,"name":"imported-parent","availability":"ready","snapshot_id":"snap_00000000000000000000000000000000","digest":parent,"parent_digest":null,
                            "artifact_path": artifact("snap_00000000000000000000000000000000", &serde_json::json!({}))?}));
                        entries.push(serde_json::json!({"group":group,"name":"imported-member","availability":"ready","snapshot_id":"snap_11111111111111111111111111111111","digest":format!("sha256:{}", "b".repeat(64)),"parent_digest":"snap_00000000000000000000000000000000",
                            "artifact_path": artifact("snap_11111111111111111111111111111111", &head)?}));
                    }
                    let removed = self.removed.lock().unwrap();
                    entries.retain(|entry| {
                        let selector = format!(
                            "{}:{}",
                            entry["group"].as_str().unwrap_or_default(),
                            entry["name"].as_str().unwrap_or_default()
                        );
                        !removed.contains(&selector)
                    });
                    Ok(CommandOutput {
                        stdout: serde_json::to_string(&entries)?,
                        ..success()
                    })
                }
                ["snapshot", "head", group, "--format", "json"] => Ok(CommandOutput {
                    stdout: serde_json::json!({
                        "group": group,
                        "head": if self.invalid_import_head.load(Ordering::Acquire) {
                            "snap_22222222222222222222222222222222"
                        } else {
                            "snap_11111111111111111111111111111111"
                        }
                    })
                    .to_string(),
                    ..success()
                }),
                ["list", "--format", "json"] => {
                    let rows = self
                        .existing
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|name| serde_json::json!({"name": name}))
                        .collect::<Vec<_>>();
                    Ok(CommandOutput {
                        stdout: serde_json::to_string(&rows)?,
                        ..success()
                    })
                }
                _ => Ok(success()),
            }
        }
    }

    fn managed_config(name: &str) -> Value {
        serde_json::json!({
            "name": name,
            "labels": {"silo.managed":"true"},
            "image": {"Oci": {"reference": "alpine:3.20", "root_disk": {"kind": "managed", "size_mib": 81920}}},
            "mounts": [
                {
                    "type": "Owned",
                    "guest": "/workspace",
                    "storage": {"kind":"disk", "capacity_mib":61440},
                    "options": {"readonly": false, "noexec": false, "nosuid": false, "nodev": false}
                }
            ],
            "resources": {"cpus": 4, "max_cpus":6, "memory_mib": 16384, "max_memory_mib":32768}
        })
    }

    fn managed_config_with_default_tmpfs(name: &str) -> Value {
        let mut config = managed_config(name);
        config["mounts"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "type": "Tmpfs",
                "guest": "/tmp",
                "size_mib": 512,
                "options": {
                    "readonly": false,
                    "noexec": false,
                    "nosuid": false,
                    "nodev": false
                }
            }));
        config
    }

    #[test]
    fn backup_preserves_optional_desktop_preferences_and_rejects_unknown_settings() {
        let mut config = computer_configuration("dev");
        assert!(validate_computer_configuration("dev", &config).is_ok());
        config["desktop"] = serde_json::json!({"startWithComputer":false});
        validate_computer_configuration("dev", &config).unwrap();
        let decoded: crate::runtime::ComputerConfiguration =
            serde_json::from_value(config.clone()).unwrap();
        assert_eq!(
            crate::desktop::configuration(&decoded)
                .unwrap()
                .start_with_computer,
            false
        );
        assert_eq!(serde_json::to_value(decoded).unwrap(), config);
        config["desktop"]["password"] = serde_json::json!("unexpected");
        assert!(validate_computer_configuration("dev", &config).is_err());
        config["desktop"] = serde_json::json!({"startWithComputer":"false"});
        assert!(validate_computer_configuration("dev", &config).is_err());
    }

    fn computer_configuration(name: &str) -> Value {
        serde_json::json!({
            "id": "2f6b739d-ff7a-4be8-aa5e-f6694e4ab0d8",
            "name": name,
            "cpus": 4,
            "maxCPUs": 6,
            "memoryGiB": 16,
            "maxMemoryGiB": 32,
            "workspaceStorageGiB": 60,
            "runtimeStorageGiB": 80
        })
    }

    fn service(temp: &tempfile::TempDir, runner: FakeRunner) -> BackupService<FakeRunner> {
        BackupService::with_runner(
            MsbCommand {
                executable: temp.path().join("msb"),
                home: temp.path().join("home"),
                storage_home: None,
                library: temp.path().join("libkrunfw"),
            },
            temp.path().join("scratch"),
            runner,
        )
    }

    #[test]
    fn backup_staging_is_private_with_permissive_umask() {
        const CHILD: &str = "SILO_PRIVATE_BACKUP_STAGE_TEST";
        if std::env::var_os(CHILD).is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "backup::tests::backup_staging_is_private_with_permissive_umask",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        // The child runs only this test; its umask cannot affect other test workers.
        unsafe { libc::umask(0) };
        let temp = tempfile::tempdir().unwrap();
        let service = service(&temp, FakeRunner::default());
        fs::create_dir_all(&service.scratch_root).unwrap();
        for prefix in ["backup-", "restore-"] {
            let stage = service.staging_directory(prefix).unwrap();
            assert_eq!(
                fs::metadata(stage.path()).unwrap().permissions().mode() & 0o777,
                0o700
            );
            fs::write(stage.path().join("private-payload"), b"fixture").unwrap();
            assert_eq!(
                fs::read(stage.path().join("private-payload")).unwrap(),
                b"fixture"
            );
        }
    }

    fn create_one(
        service: &BackupService<FakeRunner>,
        destination: PathBuf,
        running: bool,
    ) -> Result<BackupResult, BackupError> {
        create_one_in_group(service, destination, running, "dev")
    }

    fn create_one_in_group(
        service: &BackupService<FakeRunner>,
        destination: PathBuf,
        running: bool,
        snapshot_group: &str,
    ) -> Result<BackupResult, BackupError> {
        let runtime_config = managed_config("dev");
        service.create_backup(
            BackupRequest {
                destination,
                sources: vec![BackupSource {
                    name: "dev".into(),
                    snapshot_group: snapshot_group.into(),
                    was_running: running,
                    runtime_config,
                    computer_configuration: computer_configuration("dev"),
                    existing_member: None,
                }],
            },
            &Cancellation::default(),
        )
    }

    /// A finished export of the computer `dev` at `destination`, for tests of code
    /// that finds an export file after the process that wrote it was interrupted.
    pub(crate) fn write_finished_export(destination: &Path) {
        let temp = tempfile::tempdir().unwrap();
        create_one(
            &service(&temp, FakeRunner::default()),
            destination.to_path_buf(),
            false,
        )
        .unwrap();
    }

    fn script_command(directory: &Path, script: &str) -> MsbCommand {
        use std::os::unix::fs::PermissionsExt;
        let executable = directory.join("msb");
        let home = directory.join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(&executable, script).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        MsbCommand {
            executable,
            home,
            storage_home: None,
            library: directory.join("unused-library"),
        }
    }

    fn wait_for_file(path: &Path) -> bool {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !path.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        path.exists()
    }

    #[test]
    fn snapshot_command_keeps_recovery_out_until_it_finishes() {
        let directory = tempfile::tempdir().unwrap();
        let command = script_command(
            directory.path(),
            "#!/bin/sh\nprintf ready > \"$MSB_HOME/ready\"\nwhile [ ! -e \"$MSB_HOME/release\" ]; do sleep 0.02; done\n",
        );
        let home = command.home.clone();
        let worker = thread::spawn(move || {
            SystemMsbRunner.run(
                &command,
                &["snapshot".into(), "load".into(), "/tmp/unused.msb".into()],
                Duration::from_secs(120),
                &Cancellation::default(),
            )
        });
        let ready = wait_for_file(&home.join("ready"));
        let lock_result = wait_for_interrupted_command(&home, Duration::ZERO);
        fs::write(home.join("release"), b"release").unwrap();
        let result = worker.join().unwrap();
        assert!(ready, "snapshot command never reached its side effect");
        assert_eq!(lock_result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(result.unwrap().status.success());
    }

    #[test]
    fn non_snapshot_commands_do_not_take_the_worker_lock() {
        // Whole-process forks in other tests would keep this test's lock busy; see the file descriptor isolation notes in docs/SiloUI-RUST-TEST-SUPPORT.md.
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let command = script_command(
            directory.path(),
            "#!/bin/sh\nprintf ready > \"$MSB_HOME/ready\"\nwhile [ ! -e \"$MSB_HOME/release\" ]; do sleep 0.02; done\nprintf '[]'\n",
        );
        let home = command.home.clone();
        let worker = thread::spawn(move || {
            SystemMsbRunner.run(
                &command,
                &["list".into(), "--format".into(), "json".into()],
                Duration::from_secs(120),
                &Cancellation::default(),
            )
        });
        let ready = wait_for_file(&home.join("ready"));
        let lock_result = wait_for_interrupted_command(&home, Duration::ZERO);
        fs::write(home.join("release"), b"release").unwrap();
        assert!(ready);
        assert!(
            lock_result.is_ok(),
            "a read-only list must not hold the worker lock"
        );
        assert_eq!(worker.join().unwrap().unwrap().stdout, "[]");
    }

    const TRAPS_TERM: &str = "#!/bin/sh\ntrap 'printf term > \"$MSB_HOME/term\"; exit 0' TERM\nprintf ready > \"$MSB_HOME/ready\"\nwhile :; do sleep 0.02; done\n";

    #[test]
    fn cancel_asks_the_runtime_to_stop_before_killing_it() {
        // Whole-process forks in other tests would keep this test's lock busy; see the file descriptor isolation notes in docs/SiloUI-RUST-TEST-SUPPORT.md.
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let command = script_command(directory.path(), TRAPS_TERM);
        let home = command.home.clone();
        let cancellation = Cancellation::default();
        let worker_cancellation = cancellation.clone();
        let worker = thread::spawn(move || {
            run_msb_process(
                &command,
                &["snapshot".into(), "load".into(), "/tmp/unused.msb".into()],
                Duration::from_secs(300),
                &worker_cancellation,
                Duration::from_secs(120),
            )
        });
        assert!(wait_for_file(&home.join("ready")));
        let started = Instant::now();
        cancellation.cancel();
        assert!(matches!(
            worker.join().unwrap(),
            Err(BackupError::Cancelled)
        ));
        assert!(home.join("term").exists(), "msb must receive SIGTERM first");
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "graceful exit must not wait the grace out"
        );
        // The child released the worker lock when it exited.
        assert!(wait_for_interrupted_command(&home, Duration::ZERO).is_ok());
    }

    #[test]
    fn timeout_also_stops_gracefully_and_ignored_term_is_killed_after_the_grace() {
        let directory = tempfile::tempdir().unwrap();
        let command = script_command(directory.path(), TRAPS_TERM);
        let result = run_msb_process(
            &command,
            &["snapshot".into(), "save".into(), "x".into(), "y".into()],
            Duration::from_secs(3),
            &Cancellation::default(),
            Duration::from_secs(10),
        );
        assert!(matches!(result, Err(BackupError::CommandTimeout)));
        assert!(command.home.join("term").exists());

        let stubborn = tempfile::tempdir().unwrap();
        let command = script_command(
            stubborn.path(),
            "#!/bin/sh\ntrap '' TERM\nwhile :; do sleep 0.02; done\n",
        );
        let started = Instant::now();
        let result = run_msb_process(
            &command,
            &["snapshot".into(), "save".into(), "x".into(), "y".into()],
            Duration::from_millis(200),
            &Cancellation::default(),
            Duration::from_millis(300),
        );
        assert!(matches!(result, Err(BackupError::CommandTimeout)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn cancel_stops_waiting_for_an_inherited_worker_lock() {
        // Whole-process forks in other tests would keep this test's lock busy; see the file descriptor isolation notes in docs/SiloUI-RUST-TEST-SUPPORT.md.
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let command = script_command(directory.path(), "#!/bin/sh\nexit 0\n");
        // A surviving child of an earlier Silo still holds the lock.
        let held = wait_for_interrupted_command(&command.home, Duration::ZERO).unwrap();
        let cancellation = Cancellation::default();
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker_cancellation = cancellation.clone();
        thread::spawn(move || {
            let result = SystemMsbRunner.run(
                &command,
                &["snapshot".into(), "load".into(), "/tmp/unused.msb".into()],
                DEFAULT_COMMAND_TIMEOUT,
                &worker_cancellation,
            );
            let _ = sender.send(result);
        });
        thread::sleep(Duration::from_millis(100));
        cancellation.cancel();
        let result = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("Cancel must stop the worker-lock wait");
        assert!(matches!(result, Err(BackupError::Cancelled)));
        drop(held);
    }

    #[test]
    fn worker_lock_wait_is_shorter_than_the_command_timeout() {
        assert_eq!(
            worker_lock_timeout(DEFAULT_COMMAND_TIMEOUT),
            WORKER_LOCK_TIMEOUT
        );
        assert!(WORKER_LOCK_TIMEOUT < DEFAULT_COMMAND_TIMEOUT);
        assert_eq!(
            worker_lock_timeout(Duration::from_secs(5)),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn surviving_child_holds_worker_lock_until_it_exits() {
        // Whole-process forks in other tests would keep this test's lock busy; see the file descriptor isolation notes in docs/SiloUI-RUST-TEST-SUPPORT.md.
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let lock = wait_for_interrupted_command(directory.path(), Duration::ZERO).unwrap();
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "read line"]).stdin(Stdio::piped());
        inherit_worker_lock(&mut command, &lock);
        let mut child = command.spawn().unwrap();
        drop(lock); // The original Silo process no longer owns a descriptor.
        let blocked = wait_for_interrupted_command(directory.path(), Duration::ZERO);
        drop(child.stdin.take());
        child.wait().unwrap();
        assert_eq!(blocked.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(wait_for_interrupted_command(directory.path(), Duration::from_secs(2)).is_ok());
    }

    #[test]
    fn checking_for_a_surviving_child_writes_nothing_and_still_waits_for_it() {
        // Whole-process forks in other tests would keep this test's lock busy; see the file descriptor isolation notes in docs/SiloUI-RUST-TEST-SUPPORT.md.
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        // No lock file: no child can hold it, and neither it nor its folder is created.
        assert!(wait_for_interrupted_command_in_place(&home, Duration::ZERO)
            .unwrap()
            .is_none());
        assert!(!home.exists());
        fs::create_dir(&home).unwrap();
        assert!(wait_for_interrupted_command_in_place(&home, Duration::ZERO)
            .unwrap()
            .is_none());
        assert_eq!(fs::read_dir(&home).unwrap().count(), 0);

        // A surviving child that inherited the lock keeps recovery out until it exits.
        let lock = wait_for_interrupted_command(&home, Duration::ZERO).unwrap();
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "read line"]).stdin(Stdio::piped());
        inherit_worker_lock(&mut command, &lock);
        let mut child = command.spawn().unwrap();
        drop(lock);
        let blocked = wait_for_interrupted_command_in_place(&home, Duration::ZERO);
        drop(child.stdin.take());
        child.wait().unwrap();
        assert_eq!(blocked.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(
            wait_for_interrupted_command_in_place(&home, Duration::from_secs(2))
                .unwrap()
                .is_some()
        );
        // Waiting for it left the one file the previous runtime wrote there.
        assert_eq!(fs::read_dir(&home).unwrap().count(), 1);
    }

    #[test]
    fn backup_contract_requires_owned_workspace_snapshot_and_real_root_capacity() {
        let config = managed_config("dev");
        let configuration = computer_configuration("dev");
        assert!(validate_volume_contract(&config, &configuration).is_ok());
        let mut wrong_root = config.clone();
        wrong_root["image"]["Oci"]["root_disk"]["size_mib"] = Value::from(8192);
        assert!(validate_volume_contract(&wrong_root, &configuration).is_err());
        let mut extra_disk = config;
        extra_disk["mounts"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"type":"DiskImage", "guest":"/extra"}));
        assert!(validate_snapshottable_config("dev", &extra_disk).is_err());
        assert!(validate_package_volumes(&[]).is_ok());
        assert!(validate_package_volumes(&[serde_json::json!({
            "role": "workspace",
            "mountPath": "/workspace",
            "capacityBytes": 1,
            "logicalSizeBytes": 1,
            "payloadSize": 1,
            "payloadSha256": format!("sha256:{}", "0".repeat(64)),
        })])
        .is_err());
    }

    #[test]
    fn incompatible_guest_architecture_is_rejected_before_restore() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("architecture.silo-backup");
        create_one(
            &service(&temp, FakeRunner::default()),
            destination.clone(),
            false,
        )
        .unwrap();
        let mut package = read_and_verify_package(
            &destination,
            DEFAULT_MAX_ARCHIVE_BYTES,
            &Cancellation::default(),
            PayloadMode::VerifyAll,
        )
        .unwrap();
        package.manifest.runtime.guest_architecture = if std::env::consts::ARCH == "aarch64" {
            "x86_64"
        } else {
            "aarch64"
        }
        .into();
        let error = validate_manifest(&package.manifest).unwrap_err();
        assert!(error.to_string().contains("This export file requires"));
    }

    #[test]
    fn backup_is_immutable_verified_and_self_contained() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let service = service(&temp, FakeRunner::default());
        let result = create_one(&service, destination.clone(), false).unwrap();
        let inspection = service
            .inspect_archive(&destination, &Cancellation::default())
            .unwrap();
        assert_eq!(inspection.computers, ["dev"]);
        assert_eq!(inspection.size_bytes, result.size_bytes);
        let first = fs::read(&destination).unwrap();
        let conflict = create_one(&service, destination.clone(), false);
        assert!(matches!(conflict, Err(BackupError::FileConflict(_))));
        let message = conflict.err().unwrap().to_string();
        assert!(
            message.starts_with("A file already exists at "),
            "{message}"
        );
        assert!(!message.contains("Computer named"), "{message}");
        assert_eq!(fs::read(destination).unwrap(), first);
        let calls = service.runner.calls.lock().unwrap();
        assert!(calls
            .iter()
            .any(|args| args.ends_with(&["--with-parents".into(), "--with-image".into()])));
    }

    #[test]
    fn checkpoint_export_reuses_the_existing_member_without_capturing() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev-checkpoint.silo-backup");
        let member = "c0123456789abcdef0123456789abcde";
        let runner = FakeRunner::default();
        runner
            .existing_members
            .lock()
            .unwrap()
            .push(("dev".into(), member.into()));
        let service = service(&temp, runner);
        service
            .create_backup(
                BackupRequest {
                    destination: destination.clone(),
                    sources: vec![BackupSource {
                        name: "dev".into(),
                        snapshot_group: "dev".into(),
                        // Irrelevant on the checkpoint path: no fresh capture runs.
                        was_running: true,
                        runtime_config: managed_config("dev"),
                        computer_configuration: computer_configuration("dev"),
                        existing_member: Some(member.into()),
                    }],
                },
                &Cancellation::default(),
            )
            .unwrap();
        assert!(destination.is_file());
        let calls = service.runner.calls.lock().unwrap();
        // No fresh snapshot capture was taken.
        assert!(calls
            .iter()
            .all(|args| args.get(1).map(String::as_str) != Some("create")));
        // The existing member was verified and saved self-contained.
        assert!(calls
            .iter()
            .any(|args| matches!(args.as_slice(), [head, verb, ..] if head == "snapshot" && verb == "verify")));
        assert!(calls
            .iter()
            .any(|args| args.ends_with(&["--with-parents".into(), "--with-image".into()])));
    }

    fn export_checkpoint(
        runner: FakeRunner,
    ) -> (tempfile::TempDir, Result<PackageManifest, BackupError>) {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("checkpoint.silo-backup");
        let member = "c0123456789abcdef0123456789abcde";
        runner
            .existing_members
            .lock()
            .unwrap()
            .push(("dev".into(), member.into()));
        let service = service(&temp, runner);
        let result = service
            .create_backup(
                BackupRequest {
                    destination: destination.clone(),
                    sources: vec![BackupSource {
                        name: "dev".into(),
                        snapshot_group: "dev".into(),
                        was_running: false,
                        runtime_config: managed_config_with_default_tmpfs("dev"),
                        computer_configuration: computer_configuration("dev"),
                        existing_member: Some(member.into()),
                    }],
                },
                &Cancellation::default(),
            )
            .map(|_| {
                read_and_verify_package(
                    &destination,
                    DEFAULT_MAX_ARCHIVE_BYTES,
                    &Cancellation::default(),
                    PayloadMode::VerifyAll,
                )
                .unwrap()
                .manifest
            });
        (temp, result)
    }

    #[test]
    fn checkpoint_export_describes_the_checkpoint_as_captured() {
        // Captured with 2 of 6 CPUs, 1 of 32 GiB memory and a 30 GiB
        // computer; the computer now has 4 CPUs, 16 GiB and 60 GiB.
        let mut descriptor = loaded_descriptor_for(&managed_config("dev"));
        descriptor["scope"] = "checkpoint".into();
        descriptor["state"] = serde_json::json!({
            "kind": "checkpoint",
            "checkpoint_id": "ckpt",
            "checkpoint_root": format!("sha256:{}", "d".repeat(64)),
            "restore_intents": ["clone", "resume"],
            "requirements_summary": {"vcpus": 2, "max_vcpus": 6, "memory_mib": 1024, "max_memory_mib": 32768}
        });
        descriptor["extensions"][OWNED_VOLUMES_EXTENSION][0]["mount"]["storage"]["capacity_mib"] =
            30720.into();
        let runner = FakeRunner::default();
        *runner.member_descriptor.lock().unwrap() = Some(descriptor.clone());
        let (_temp, manifest) = export_checkpoint(runner);
        let manifest = manifest.unwrap();
        let computer = &manifest.computers[0];
        assert_eq!(computer.computer_configuration["cpus"], 2);
        assert_eq!(computer.computer_configuration["maxCPUs"], 6);
        assert_eq!(computer.computer_configuration["memoryGiB"], 1);
        assert_eq!(computer.computer_configuration["workspaceStorageGiB"], 30);
        assert_eq!(computer.computer_configuration["runtimeStorageGiB"], 80);
        assert_eq!(computer.runtime_config["resources"]["memory_mib"], 1024);
        let tmpfs = computer.runtime_config["mounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|mount| mount["type"] == "Tmpfs")
            .unwrap();
        assert_eq!(tmpfs["size_mib"], 256);
        // The archive now passes the import-side comparison with that snapshot.
        descriptor["extensions"][OWNED_VOLUMES_EXTENSION][0]["mount"]["storage"]["capacity_mib"] =
            30720.into();
        assert!(compare_loaded_descriptor(
            &descriptor,
            "snap_11111111111111111111111111111111",
            &computer.runtime_config
        )
        .is_ok());

        let runner = FakeRunner::default();
        let mut without_computer = loaded_descriptor_for(&managed_config("dev"));
        without_computer["extensions"] = serde_json::json!({});
        without_computer["requires"] = serde_json::json!([]);
        *runner.member_descriptor.lock().unwrap() = Some(without_computer);
        let (_temp, manifest) = export_checkpoint(runner);
        assert!(
            matches!(manifest, Err(BackupError::UnsupportedStorage(message)) if message.contains("workspace disk"))
        );
    }

    #[test]
    fn running_computer_is_snapshotted_live_with_required_guest_flush() {
        let temp = tempfile::tempdir().unwrap();
        let runner = FakeRunner::default();
        let service = service(&temp, runner);
        let result = create_one(&service, temp.path().join("dev.silo-backup"), true).unwrap();
        assert!(result.destination.is_file());
        let calls = service.runner.calls.lock().unwrap();
        let capture = calls
            .iter()
            .position(|args| args.get(1).is_some_and(|arg| arg == "create"))
            .unwrap();
        let archive = calls
            .iter()
            .position(|args| args.get(1).is_some_and(|arg| arg == "save"))
            .unwrap();
        assert!(capture < archive);
        assert!(calls[capture]
            .windows(2)
            .any(|pair| pair[0] == "--guest-flush" && pair[1] == "required"));
        assert!(!calls.iter().any(|args| {
            args.first()
                .is_some_and(|arg| arg == "stop" || arg == "start")
        }));
    }

    #[test]
    fn export_journals_capture_before_create_and_clears_only_after_verification() {
        for cancelled in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let runner = FakeRunner::default();
            runner
                .cancel_after_snapshot
                .store(cancelled, Ordering::Release);
            let service = service(&temp, runner);
            let intents = Mutex::new(Vec::new());
            let result = service.create_backup_with_token(
                BackupRequest {
                    destination: temp.path().join("dev.silo-backup"),
                    sources: vec![BackupSource {
                        name: "dev".into(),
                        snapshot_group: "dev".into(),
                        was_running: false,
                        runtime_config: managed_config("dev"),
                        computer_configuration: computer_configuration("dev"),
                        existing_member: None,
                    }],
                },
                &Cancellation::default(),
                None,
                &|_, member| {
                    let calls = service.runner.calls.lock().unwrap();
                    if member.is_some() {
                        assert!(!calls
                            .iter()
                            .any(|args| args.get(1).is_some_and(|arg| arg == "create")));
                    } else {
                        assert!(calls
                            .iter()
                            .any(|args| args.get(1).is_some_and(|arg| arg == "verify")));
                    }
                    intents.lock().unwrap().push(member.map(str::to_owned));
                    Ok(())
                },
            );
            let intents = intents.into_inner().unwrap();
            assert!(intents[0].as_ref().unwrap().starts_with("silo-backup-"));
            if cancelled {
                assert!(matches!(result, Err(BackupError::Cancelled)));
                assert_eq!(
                    intents.len(),
                    1,
                    "the partial member stays journaled for cleanup"
                );
            } else {
                result.unwrap();
                assert_eq!(intents.len(), 2);
                assert!(intents[1].is_none());
            }
        }
    }

    #[test]
    fn backup_capture_uses_the_saved_lineage_group() {
        let temp = tempfile::tempdir().unwrap();
        let service = service(&temp, FakeRunner::default());
        let group = "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9";
        create_one_in_group(&service, temp.path().join("dev.silo-backup"), false, group).unwrap();
        let calls = service.runner.calls.lock().unwrap();
        let capture = calls
            .iter()
            .find(|args| args.get(1).is_some_and(|arg| arg == "create"))
            .unwrap();
        assert!(capture.windows(2).any(|pair| pair == ["--group", group]));
        assert!(!capture[6].starts_with("silo-export-"));
    }

    #[test]
    fn default_github_network_is_restorable_but_credentials_and_policy_changes_are_not() {
        let mut config = managed_config("dev");
        config["network"] =
            serde_json::from_str(include_str!("../guest/github-network-default.json")).unwrap();
        assert!(validate_snapshottable_config("dev", &config).is_ok());
        let original = config.clone();
        config["network"]["secrets"]["secrets"][0]["value"] =
            serde_json::json!("must-not-be-archived");
        assert!(validate_snapshottable_config("dev", &config).is_err());
        config = original.clone();
        config["network"]["tls"]["verify_upstream"] = serde_json::json!(false);
        assert!(validate_snapshottable_config("dev", &config).is_err());
        config = original;
        config["network"]["secrets"]["secrets"][0]["source"] =
            serde_json::json!({"kind":"file","path":"/private/secret"});
        assert!(validate_snapshottable_config("dev", &config).is_err());
    }

    #[test]
    fn imported_deny_network_is_restorable_but_custom_rules_are_not() {
        let mut config = managed_config("dev");
        config["network"] =
            serde_json::from_str(include_str!("../guest/github-network-default.json")).unwrap();
        config["network"]["policy"] = serde_json::json!({
            "default_egress": "deny",
            "default_ingress": "deny",
            "rules": []
        });

        assert!(validate_snapshottable_config("dev", &config).is_ok());

        config["network"]["policy"]["rules"] = serde_json::json!([{
            "action": "allow",
            "destination": {"group": "public"},
            "direction": "egress",
            "ports": [],
            "protocols": []
        }]);
        assert!(validate_snapshottable_config("dev", &config).is_err());

        config["network"]["policy"] = serde_json::json!({
            "default_egress": "allow",
            "default_ingress": "deny",
            "rules": []
        });
        assert!(validate_snapshottable_config("dev", &config).is_err());
    }

    fn export_with_runtime(runtime_config: Value) -> Result<PackageManifest, BackupError> {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let service = service(&temp, FakeRunner::default());
        service
            .create_backup(
                BackupRequest {
                    destination: destination.clone(),
                    sources: vec![BackupSource {
                        name: "dev".into(),
                        snapshot_group: "dev".into(),
                        was_running: false,
                        runtime_config,
                        computer_configuration: computer_configuration("dev"),
                        existing_member: None,
                    }],
                },
                &Cancellation::default(),
            )
            .map(|_| {
                read_and_verify_package(
                    &destination,
                    DEFAULT_MAX_ARCHIVE_BYTES,
                    &Cancellation::default(),
                    PayloadMode::VerifyAll,
                )
                .unwrap()
                .manifest
            })
    }

    #[test]
    fn export_strips_network_policy_and_secret_references() {
        let mut config = managed_config("dev");
        config["network"] =
            serde_json::from_str(include_str!("../guest/github-network-default.json")).unwrap();
        let mut assigned = config["network"]["secrets"]["secrets"][0].clone();
        assigned["env_var"] = "OPENAI_API_KEY".into();
        assigned["placeholder"] = "$MSB_OPENAI_API_KEY".into();
        assigned["source"] = serde_json::json!({"kind": "env", "var": "OPENAI_API_KEY"});
        config["network"]["secrets"]["secrets"]
            .as_array_mut()
            .unwrap()
            .push(assigned);
        config["network"]["policy"]["rules"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "action": "allow",
                "destination": {"cidr": "10.0.0.0/8"},
                "direction": "egress",
                "ports": [],
                "protocols": []
            }));
        config["network"]["ports"] = serde_json::json!([{"host": 8080, "guest": 80}]);
        assert!(validate_snapshottable_config("dev", &config).is_err());
        let manifest = export_with_runtime(config).unwrap();
        let network = &manifest.computers[0].runtime_config["network"];
        assert!(imported_deny_network(network), "{network}");
        assert!(!network.to_string().contains("OPENAI_API_KEY"));
    }

    #[test]
    fn export_names_the_setting_it_cannot_carry() {
        let mut config = managed_config("dev");
        config["network"] =
            serde_json::from_str(include_str!("../guest/github-network-default.json")).unwrap();
        config["network"]["tls"]["verify_upstream"] = false.into();
        let error = export_with_runtime(config).err().unwrap().to_string();
        assert!(error.contains("custom network tls settings"), "{error}");

        let mut config = managed_config("dev");
        config["runtime"] = serde_json::json!({"workdir": "/custom"});
        let error = export_with_runtime(config).err().unwrap().to_string();
        assert!(error.contains("custom runtime workdir settings"), "{error}");

        let mut config = managed_config("dev");
        config["init"] = serde_json::json!({"cmd": ["/sbin/init"]});
        let error = export_with_runtime(config).err().unwrap().to_string();
        assert!(error.contains("custom init settings"), "{error}");
    }

    /// The `network` section of real `msb inspect --format json` output
    /// (`test_support/msb-inspect`). `created`, `restored`, `forked` and
    /// `imported` come from the bundled MicroSandbox (0.7.4 and 0.7.6): a
    /// create with Silo's arguments, a restore, a forked full restore and an
    /// import-style deny-all restore, the restores with Silo's policy
    /// arguments. 0.7.6 also saves `http` and `nat64_prefixes`, at their
    /// defaults. `created-0.7.2` comes from 0.7.2 and
    /// `migrated-from-*` from an old-layout computer after the 0.7.4 `adopt-disk`
    /// that migration runs.
    fn captured_network(origin: &str) -> Value {
        let text = match origin {
            "created-0.7.6" => include_str!("test_support/msb-inspect/created-0.7.6.json"),
            "restored-0.7.6" => include_str!("test_support/msb-inspect/restored-0.7.6.json"),
            "forked-0.7.6" => include_str!("test_support/msb-inspect/forked-0.7.6.json"),
            "imported-0.7.6" => include_str!("test_support/msb-inspect/imported-0.7.6.json"),
            "created-0.7.4" => include_str!("test_support/msb-inspect/created-0.7.4.json"),
            "restored-0.7.4" => include_str!("test_support/msb-inspect/restored-0.7.4.json"),
            "forked-0.7.4" => include_str!("test_support/msb-inspect/forked-0.7.4.json"),
            "imported-0.7.4" => include_str!("test_support/msb-inspect/imported-0.7.4.json"),
            "created-0.7.2" => include_str!("test_support/msb-inspect/created-0.7.2.json"),
            "migrated-from-0.7.2" => {
                include_str!("test_support/msb-inspect/migrated-from-0.7.2.json")
            }
            "migrated-from-0.6.17" => {
                include_str!("test_support/msb-inspect/migrated-from-0.6.17.json")
            }
            "migrated-from-0.6.17-before-adopt-disk" => {
                include_str!("test_support/msb-inspect/migrated-from-0.6.17-before-adopt-disk.json")
            }
            other => panic!("no captured network for {other}"),
        };
        serde_json::from_str(text).unwrap()
    }

    const CAPTURED_NETWORKS: [(&str, Option<bool>); 12] = [
        // Created by Silo's own `msb create`: the runtime's current default.
        ("created-0.7.6", Some(true)),
        ("restored-0.7.6", Some(true)),
        ("forked-0.7.6", Some(true)),
        ("imported-0.7.6", Some(true)),
        ("created-0.7.4", Some(true)),
        ("restored-0.7.4", Some(true)),
        ("forked-0.7.4", Some(true)),
        ("imported-0.7.4", Some(true)),
        // Before MicroSandbox 0.7.4 the default was off.
        ("created-0.7.2", Some(false)),
        ("migrated-from-0.7.2", Some(false)),
        // MicroSandbox 0.6.17 had no such option; `adopt-disk` saves the current default.
        ("migrated-from-0.6.17", Some(true)),
        ("migrated-from-0.6.17-before-adopt-disk", None),
    ];

    #[test]
    fn captured_networks_carry_the_strict_value_of_their_origin() {
        for (origin, strict) in CAPTURED_NETWORKS {
            assert_eq!(
                captured_network(origin)
                    .get("strict")
                    .and_then(Value::as_bool),
                strict,
                "{origin}"
            );
        }
    }

    #[test]
    fn export_accepts_the_network_of_computers_from_every_origin() {
        let profile_strict = github_network_defaults()["strict"].clone();
        for (origin, _) in CAPTURED_NETWORKS {
            let mut config = managed_config("dev");
            config["network"] = captured_network(origin);
            // A fork keeps the addresses it was captured with; the controller clears
            // them before an export (`canonicalize_backup_runtime`).
            config["network"]["interface"] = serde_json::json!({});
            let manifest = export_with_runtime(config)
                .unwrap_or_else(|error| panic!("{origin} cannot be exported: {error}"));
            let network = &manifest.computers[0].runtime_config["network"];
            assert!(
                default_github_network(network) || imported_deny_network(network),
                "{origin}: {network}"
            );
            // Whatever the source carried, the archive carries the profile's value.
            assert_eq!(network["strict"], profile_strict, "{origin}");
        }
    }

    #[test]
    fn the_profile_is_what_silo_creates_computers_with() {
        // `msb create` in runtime.rs passes `--net-strict=true`; the runtime
        // saves that value, and an export of the computer must accept it as is.
        let mut config = managed_config("dev");
        config["network"] = captured_network("created-0.7.4");
        assert_eq!(config["network"]["strict"], true);
        assert!(default_github_network(&config["network"]));
        assert_eq!(github_network_defaults()["strict"], true);
        assert_eq!(imported_deny_network_value()["strict"], true);
    }

    #[test]
    fn archives_made_while_the_runtime_default_was_off_still_import() {
        // An export made before this change recorded the profile with `strict` off
        // (the 0.7.2 default); a configuration saved by 0.6.17 records no value.
        // Those archives carry the profile's network, or its deny-all variant.
        let profile = github_network_defaults();
        for strict in [Some(false), Some(true), None] {
            for policy in [
                profile["policy"].clone(),
                imported_deny_network_value()["policy"].clone(),
            ] {
                let mut config = managed_config("dev");
                config["network"] = profile.clone();
                config["network"]["policy"] = policy;
                match strict {
                    Some(value) => config["network"]["strict"] = value.into(),
                    None => {
                        config["network"].as_object_mut().unwrap().remove("strict");
                    }
                }
                validate_snapshottable_config("dev", &config)
                    .unwrap_or_else(|error| panic!("{strict:?}: {error}"));
            }
        }
    }

    #[test]
    fn strict_only_counts_as_a_setting_where_a_hostname_rule_depends_on_it() {
        let export_with_rule = |origin: &str, strict: Value, destination: Value| {
            let mut config = managed_config("dev");
            config["network"] = captured_network(origin);
            config["network"]["strict"] = strict;
            config["network"]["policy"]["rules"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "action": "allow",
                    "destination": destination,
                    "direction": "egress",
                    "ports": [],
                    "protocols": []
                }));
            export_with_runtime(config)
        };
        for origin in ["created-0.7.4", "created-0.7.2"] {
            for hostname in [
                serde_json::json!({"domain": "example.com"}),
                serde_json::json!({"domain_suffix": "example.com"}),
            ] {
                // With `strict` off the runtime treats a hostname allow rule differently,
                // so that combination is a real setting, and the export names it.
                let error = export_with_rule(origin, false.into(), hostname.clone())
                    .err()
                    .unwrap()
                    .to_string();
                assert!(error.contains("custom network strict settings"), "{error}");
                // With it on (the profile's value) the rule is only a custom policy,
                // which an export resets like any other.
                let manifest = export_with_rule(origin, true.into(), hostname).unwrap();
                assert!(imported_deny_network(
                    &manifest.computers[0].runtime_config["network"]
                ));
            }
            // Without a hostname rule the value changes nothing.
            let manifest = export_with_rule(
                origin,
                false.into(),
                serde_json::json!({"cidr": "10.0.0.0/8"}),
            )
            .unwrap();
            assert!(imported_deny_network(
                &manifest.computers[0].runtime_config["network"]
            ));
        }
        // A value that is not a boolean is never accepted.
        let mut config = managed_config("dev");
        config["network"] = captured_network("created-0.7.4");
        config["network"]["strict"] = "yes".into();
        let error = export_with_runtime(config).err().unwrap().to_string();
        assert!(error.contains("custom network strict settings"), "{error}");
    }

    #[test]
    fn unsupported_runtime_overrides_cannot_be_backed_up_as_restorable() {
        for (field, value) in [
            ("network", serde_json::json!({"enabled":false})),
            ("runtime", serde_json::json!({"workdir":"/custom"})),
            ("security_profile", serde_json::json!("restricted")),
            ("lifecycle", serde_json::json!({"ephemeral":true})),
        ] {
            let mut config = managed_config("dev");
            config[field] = value;
            assert!(
                validate_snapshottable_config("dev", &config).is_err(),
                "{field}"
            );
        }
        let mut config = managed_config("dev");
        config["env"] = serde_json::json!([{"key":"GIT_AUTHOR_NAME", "value":"Silo Test"}]);
        assert!(validate_snapshottable_config("dev", &config).is_ok());
    }

    #[test]
    fn mounts_and_non_managed_roots_block_complete_backup() {
        assert!(
            validate_snapshottable_config("dev", &managed_config_with_default_tmpfs("dev")).is_ok()
        );
        let mut mounted = managed_config("dev");
        mounted["mounts"] = serde_json::json!([{"type": "Named", "name": "data"}]);
        assert!(matches!(
            validate_snapshottable_config("dev", &mounted),
            Err(BackupError::UnsupportedStorage(_))
        ));
        let mut flat = managed_config("dev");
        flat["image"]["Oci"]["root_disk"]["kind"] = Value::String("flat".into());
        assert!(matches!(
            validate_snapshottable_config("dev", &flat),
            Err(BackupError::UnsupportedStorage(_))
        ));
    }

    #[test]
    fn cancellation_leaves_no_final_or_partial_file() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let service = service(&temp, FakeRunner::default());
        let cancellation = Cancellation::default();
        cancellation.cancel();
        assert!(matches!(
            service.create_backup(
                BackupRequest {
                    destination: destination.clone(),
                    sources: vec![BackupSource {
                        name: "dev".into(),
                        snapshot_group: "dev".into(),
                        was_running: false,
                        runtime_config: managed_config("dev"),
                        computer_configuration: computer_configuration("dev"),
                        existing_member: None,
                    }],
                },
                &cancellation,
            ),
            Err(BackupError::Cancelled)
        ));
        assert!(!destination.exists());
        assert!(fs::read_dir(temp.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".silo-backup-")
        }));
    }

    #[test]
    fn cancelled_capture_does_not_publish_an_archive_or_stop_the_source() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let runner = FakeRunner::default();
        runner.cancel_after_snapshot.store(true, Ordering::Release);
        let service = service(&temp, runner);
        assert!(matches!(
            create_one(&service, destination.clone(), true),
            Err(BackupError::Cancelled)
        ));
        assert!(!destination.exists());
        let captures = fs::read_dir(service.command.home.join("snapshots")).unwrap();
        assert_eq!(
            captures.count(),
            1,
            "captured ancestry must survive cancellation"
        );
        let calls = service.runner.calls.lock().unwrap();
        assert!(!calls.iter().any(|arguments| {
            arguments
                .first()
                .is_some_and(|arg| arg == "stop" || arg == "start")
        }));
    }

    #[test]
    fn state_export_refuses_to_grow_a_full_backup_lineage() {
        let temp = tempfile::tempdir().unwrap();
        let runner = FakeRunner::default();
        for index in 0..128 {
            runner
                .existing_members
                .lock()
                .unwrap()
                .push(("dev".into(), format!("silo-backup-0-{index}-1")));
        }
        // Other groups do not count toward this computer's cap.
        runner
            .existing_members
            .lock()
            .unwrap()
            .push(("other".into(), "silo-backup-0-999-1".into()));
        let service = service(&temp, runner);
        let error = create_one(&service, temp.path().join("full.silo-backup"), false).unwrap_err();
        assert!(
            error.to_string().contains("128 state-export captures"),
            "{error}"
        );
        assert!(error.to_string().contains("existing checkpoint"), "{error}");
        let calls = service.runner.calls.lock().unwrap();
        assert!(!calls
            .iter()
            .any(|call| call.get(1).is_some_and(|verb| verb == "create")));
        assert!(!calls
            .iter()
            .any(|call| call.get(1).is_some_and(|verb| verb == "remove")));
        drop(calls);
        // Reusing an existing capture remains possible at the limit.
        service
            .create_backup(
                BackupRequest {
                    destination: temp.path().join("reuse.silo-backup"),
                    sources: vec![BackupSource {
                        name: "dev".into(),
                        snapshot_group: "dev".into(),
                        was_running: false,
                        runtime_config: managed_config("dev"),
                        computer_configuration: computer_configuration("dev"),
                        existing_member: Some("silo-backup-0-0-1".into()),
                    }],
                },
                &Cancellation::default(),
            )
            .unwrap();
    }

    #[test]
    fn failed_archive_keeps_native_capture_for_the_next_export() {
        let temp = tempfile::tempdir().unwrap();
        let runner = FakeRunner::default();
        runner.fail_save.store(true, Ordering::Release);
        let service = service(&temp, runner);
        let first = temp.path().join("failed.silo-backup");
        assert!(matches!(
            create_one(&service, first.clone(), false),
            Err(BackupError::CommandFailed { .. })
        ));
        assert!(!first.exists());
        assert!(
            fs::read_dir(service.command.home.join("snapshots"))
                .unwrap()
                .count()
                > 0
        );
        service.runner.fail_save.store(false, Ordering::Release);
        create_one(&service, temp.path().join("retry.silo-backup"), false).unwrap();
        let calls = service.runner.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|args| args.get(1).is_some_and(|arg| arg == "create"))
                .count(),
            2
        );
        assert!(!calls
            .iter()
            .any(|args| args.get(1).is_some_and(|arg| arg == "remove")));
        assert!(calls
            .iter()
            .all(|args| !args.iter().any(|arg| arg == "--dest-dir")));
    }

    #[test]
    fn large_computer_list_is_read_whole_and_oversized_json_fails_explicitly() {
        // 2000 computers produce ~60 KiB of JSON, well past the 32 KiB log tail.
        let directory = tempfile::tempdir().unwrap();
        let rows = (0..2000)
            .map(|index| format!("{{\"name\":\"computer-{index:05}\",\"status\":\"Stopped\"}}"))
            .collect::<Vec<_>>()
            .join(",");
        let listing = directory.path().join("listing.json");
        fs::write(&listing, format!("[{rows}]")).unwrap();
        let command = script_command(
            directory.path(),
            &format!("#!/bin/sh\ncat '{}'\n", listing.display()),
        );
        let output = SystemMsbRunner
            .run(
                &command,
                &["list".into(), "--format".into(), "json".into()],
                Duration::from_secs(5),
                &Cancellation::default(),
            )
            .unwrap();
        assert!(output.stdout.len() > MAX_COMMAND_OUTPUT);
        let parsed: Vec<Value> = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(parsed.len(), 2000);
        assert_eq!(parsed[0]["name"], "computer-00000");

        fs::write(&listing, vec![b' '; MAX_STRUCTURED_OUTPUT + 1]).unwrap();
        let error = SystemMsbRunner
            .run(
                &command,
                &["list".into(), "--format".into(), "json".into()],
                Duration::from_secs(5),
                &Cancellation::default(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("safety limit"), "{error}");
    }

    #[test]
    fn growing_native_snapshot_index_is_complete_or_explicitly_rejected() {
        let rows: Vec<_> = (0..600)
            .map(|index| serde_json::json!({
                "group": "linux-legacy-source",
                "name": format!("checkpoint-{index}"),
                "availability": "ready",
                "artifact_path": format!("/runtime/snapshots/linux-legacy-source/snap_{index:032x}"),
            }))
            .collect();
        let index = serde_json::to_vec(&rows).unwrap();
        assert!(index.len() > MAX_COMMAND_OUTPUT);
        let (complete, truncated) =
            read_output(index.as_slice(), MAX_STRUCTURED_OUTPUT, false).unwrap();
        assert!(!truncated);
        assert_eq!(
            serde_json::from_slice::<Vec<Value>>(&complete)
                .unwrap()
                .len(),
            rows.len()
        );
        let oversized = vec![b'x'; MAX_STRUCTURED_OUTPUT + 1];
        let (_, truncated) =
            read_output(oversized.as_slice(), MAX_STRUCTURED_OUTPUT, false).unwrap();
        assert!(
            truncated,
            "oversized structured output must fail before JSON parsing"
        );
        let (tail, truncated) = read_output(index.as_slice(), MAX_COMMAND_OUTPUT, true).unwrap();
        assert!(truncated);
        assert_eq!(tail.len(), MAX_COMMAND_OUTPUT);
    }

    #[test]
    fn corrupt_and_truncated_archives_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let service = service(&temp, FakeRunner::default());
        create_one(&service, destination.clone(), false).unwrap();
        let mut bytes = fs::read(&destination).unwrap();
        *bytes.last_mut().unwrap() ^= 0xff;
        let corrupt = temp.path().join("corrupt.silo-backup");
        fs::write(&corrupt, &bytes).unwrap();
        assert!(matches!(
            service.inspect_archive(&corrupt, &Cancellation::default()),
            Err(BackupError::InvalidArchive(_))
        ));
        bytes.truncate(bytes.len() - 4);
        let truncated = temp.path().join("truncated.silo-backup");
        fs::write(&truncated, bytes).unwrap();
        assert!(matches!(
            service.inspect_archive(&truncated, &Cancellation::default()),
            Err(BackupError::InvalidArchive(_))
        ));
    }

    #[test]
    fn previous_archive_generation_is_rejected_without_legacy_reader() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let service = service(&temp, FakeRunner::default());
        create_one(&service, destination.clone(), false).unwrap();
        let bytes = fs::read(&destination).unwrap();
        let with_format = |format: u32| {
            let mut bytes = bytes.clone();
            bytes[MAGIC.len()..MAGIC.len() + 4].copy_from_slice(&format.to_be_bytes());
            let path = temp.path().join(format!("format-{format}.silo-backup"));
            fs::write(&path, bytes).unwrap();
            service
                .inspect_archive(&path, &Cancellation::default())
                .unwrap_err()
                .to_string()
        };
        for old in [1, 2, 3] {
            let message = with_format(old);
            assert!(
                message.contains(&format!("earlier Silo export format (version {old})"))
                    && message.contains("Import it with the Silo version that created it"),
                "{message}"
            );
        }
        let message = with_format(5);
        assert!(
            message.contains("newer version of Silo") && message.contains("Update Silo"),
            "{message}"
        );
    }

    #[test]
    fn exports_record_the_bundled_runtime_and_explain_other_versions() {
        let inputs: Value =
            serde_json::from_str(include_str!("../../runtime-inputs.json")).unwrap();
        assert_eq!(bundled_runtime_version(), inputs["microsandboxVersion"]);
        assert_eq!(snapshot_format_for("0.7.2"), "msb-snapshot-tar-zstd-v0.7");
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        create_one(
            &service(&temp, FakeRunner::default()),
            destination.clone(),
            false,
        )
        .unwrap();
        let mut manifest = read_and_verify_package(
            &destination,
            DEFAULT_MAX_ARCHIVE_BYTES,
            &Cancellation::default(),
            PayloadMode::VerifyAll,
        )
        .unwrap()
        .manifest;
        assert_eq!(manifest.runtime.version, bundled_runtime_version());
        let (major, minor, patch) = parse_version(bundled_runtime_version()).unwrap();
        manifest.runtime.version = format!("{major}.{}.0", minor + 1);
        manifest.runtime.snapshot_format = snapshot_format_for(&manifest.runtime.version);
        let message = validate_manifest(&manifest).err().unwrap().to_string();
        assert!(
            message.contains("newer than the") && message.contains("Update Silo"),
            "{message}"
        );
        manifest.runtime.version =
            format!("{major}.{minor}.{}", patch.saturating_sub(1).min(patch));
        if manifest.runtime.version == bundled_runtime_version() {
            manifest.runtime.version = format!("{major}.0.0");
        }
        let message = validate_manifest(&manifest).err().unwrap().to_string();
        assert!(
            message.contains("cannot import") && message.contains("export it again"),
            "{message}"
        );

        // The bundled runtime loads and verifies archives exported by Silo's 0.7.2 and
        // 0.7.4 flows (checked with the real binaries when 0.7.6 was bundled).
        for earlier in ["0.7.2", "0.7.4"] {
            manifest.runtime.version = earlier.into();
            manifest.runtime.snapshot_format = snapshot_format_for(earlier);
            validate_manifest(&manifest).unwrap();
        }
        manifest.runtime.snapshot_format = "msb-snapshot-tar-zstd-v0.6".into();
        assert!(validate_manifest(&manifest).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_archive_is_rejected_without_following_it() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let service = service(&temp, FakeRunner::default());
        create_one(&service, destination.clone(), false).unwrap();
        let link = temp.path().join("linked.silo-backup");
        std::os::unix::fs::symlink(&destination, &link).unwrap();
        assert!(matches!(
            service.inspect_archive(&link, &Cancellation::default()),
            Err(BackupError::InvalidArchive(_))
        ));
    }

    #[test]
    fn archive_is_checked_on_the_opened_handle_without_blocking_on_a_fifo() {
        let temp = tempfile::tempdir().unwrap();
        let fifo = temp.path().join("swapped.silo-backup");
        let encoded = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: encoded is a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(encoded.as_ptr(), 0o600) }, 0);
        let (sender, receiver) = std::sync::mpsc::channel();
        let path = fifo.clone();
        thread::spawn(move || {
            let _ = sender.send(matches!(
                open_regular_file(&path),
                Err(OpenRegularError::NotRegular)
            ));
        });
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("opening a FIFO must not block"),
            "a FIFO is not a regular archive"
        );
        let regular = temp.path().join("regular.silo-backup");
        fs::write(&regular, b"12345").unwrap();
        let link = temp.path().join("link.silo-backup");
        std::os::unix::fs::symlink(&regular, &link).unwrap();
        assert!(matches!(
            open_regular_file(&link),
            Err(OpenRegularError::NotRegular)
        ));
        let (_, metadata) = open_regular_file(&regular).ok().unwrap();
        assert_eq!(metadata.len(), 5);
        let service = service(&temp, FakeRunner::default());
        assert!(matches!(
            service.inspect_archive(&fifo, &Cancellation::default()),
            Err(BackupError::InvalidArchive(_))
        ));
    }

    #[test]
    fn restore_rejects_conflict_and_imports_a_verified_pending_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let runner = FakeRunner::default();
        runner.existing.lock().unwrap().push("taken".into());
        let service = service(&temp, runner);
        create_one(&service, destination.clone(), false).unwrap();
        assert!(matches!(
            service.prepare_restore(
                RestoreRequest {
                    archive: destination.clone(),
                    source_name: None,
                    new_name: "taken".into(),
                },
                &Cancellation::default(),
            ),
            Err(BackupError::Conflict(name)) if name == "taken"
        ));
        let restored = service
            .prepare_restore(
                RestoreRequest {
                    archive: destination,
                    source_name: None,
                    new_name: "dev-restored".into(),
                },
                &Cancellation::default(),
            )
            .unwrap();
        assert_eq!(restored.source_name, "dev");
        assert_eq!(restored.new_name, "dev-restored");
        assert_eq!(restored.runtime_config["name"], "dev");
        assert_eq!(restored.computer_configuration["workspaceStorageGiB"], 60);
        assert_eq!(restored.computer_configuration["runtimeStorageGiB"], 80);
        assert!(restored.snapshot_group.starts_with("silo-import-"));
        assert_eq!(restored.snapshot_member, "imported-member");
        let calls = service.runner.calls.lock().unwrap();
        assert!(!calls.iter().any(|arguments| {
            arguments
                .first()
                .is_some_and(|arg| arg == "restore" || arg == "start")
        }));
        drop(calls);
        let stage_root = restored._stage.path().to_path_buf();
        drop(restored);
        assert!(!stage_root.exists());
    }

    #[test]
    fn restore_rejects_an_import_head_absent_from_the_parent_chain() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let service = service(&temp, FakeRunner::default());
        create_one(&service, destination.clone(), false).unwrap();
        service
            .runner
            .invalid_import_head
            .store(true, Ordering::Release);
        let result = service.prepare_restore(
            RestoreRequest {
                archive: destination,
                source_name: None,
                new_name: "dev-restored".into(),
            },
            &Cancellation::default(),
        );
        assert!(
            matches!(result, Err(BackupError::InvalidArchive(message)) if message.contains("head is not uniquely indexed"))
        );
        let calls = service.runner.calls.lock().unwrap();
        assert!(!calls
            .iter()
            .any(|args| args.get(1).is_some_and(|arg| arg == "verify")
                && args.iter().any(|arg| arg.starts_with("silo-import-"))));
    }

    #[test]
    fn failed_runtime_load_rolls_back_private_restore_stage() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let runner = FakeRunner::default();
        let service = service(&temp, runner);
        create_one(&service, destination.clone(), false).unwrap();
        let existing_cache_stage = temp
            .path()
            .join("home/cache/tmp/snapshot-import-preexisting");
        fs::create_dir_all(&existing_cache_stage).unwrap();
        let existing_snapshot_stage = temp
            .path()
            .join("home/snapshots/.msb-snapshot-import-preexisting");
        fs::create_dir_all(&existing_snapshot_stage).unwrap();
        service.runner.fail_load.store(true, Ordering::Release);
        let result = service.prepare_restore(
            RestoreRequest {
                archive: destination,
                source_name: None,
                new_name: "dev-restored".into(),
            },
            &Cancellation::default(),
        );
        assert!(matches!(result, Err(BackupError::CommandFailed { .. })));
        let scratch = temp.path().join("scratch");
        assert!(fs::read_dir(scratch).unwrap().next().is_none());
        assert!(existing_cache_stage.is_dir());
        assert!(existing_snapshot_stage.is_dir());
        let calls = import_group_calls(&service, "load");
        for path in native_import_stage_paths(&temp.path().join("home"), &calls[0][4]).unwrap() {
            assert!(!path.exists());
        }
    }

    fn restore_request(archive: PathBuf) -> RestoreRequest {
        RestoreRequest {
            archive,
            source_name: None,
            new_name: "dev-restored".into(),
        }
    }

    fn import_group_calls(service: &BackupService<FakeRunner>, verb: &str) -> Vec<Vec<String>> {
        service
            .runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|args| {
                args.get(1).is_some_and(|arg| arg == verb)
                    && (if verb == "load" {
                        args.get(4)
                    } else {
                        args.last()
                    })
                    .is_some_and(|arg| arg.starts_with("silo-import-"))
            })
            .cloned()
            .collect()
    }

    #[test]
    fn existing_operation_stages_are_refused_before_the_journal_claims_them() {
        for relative in ["snapshots/.msb-snapshot-load-", "cache/tmp/snapshot-load-"] {
            let temp = tempfile::tempdir().unwrap();
            let destination = temp.path().join("dev.silo-backup");
            let service = service(&temp, FakeRunner::default());
            create_one(&service, destination.clone(), false).unwrap();
            let group = new_import_group();
            let stage = temp
                .path()
                .join("home")
                .join(format!("{relative}{}", &group[12..]));
            fs::create_dir_all(&stage).unwrap();
            fs::write(stage.join("keep"), b"preexisting").unwrap();
            let claimed = AtomicBool::new(false);
            let result = service.prepare_restore_in_group(
                restore_request(destination),
                &group,
                &Cancellation::default(),
                &|| {
                    claimed.store(true, Ordering::Release);
                    Ok(())
                },
            );
            assert!(matches!(result, Err(BackupError::ImportGroupConflict(_))));
            assert!(!claimed.load(Ordering::Acquire));
            assert!(import_group_calls(&service, "load").is_empty());
            assert!(service.runner.removed.lock().unwrap().is_empty());
            assert_eq!(fs::read(stage.join("keep")).unwrap(), b"preexisting");
        }
    }

    #[test]
    fn import_stage_cleanup_waits_for_the_existing_worker_lock() {
        let temp = tempfile::tempdir().unwrap();
        let mut service = service(&temp, FakeRunner::default());
        let group = new_import_group();
        let stages = native_import_stage_paths(&service.command.home, &group).unwrap();
        for stage in &stages {
            fs::create_dir_all(stage).unwrap();
            fs::write(stage.join("partial"), b"still writing").unwrap();
        }
        let worker = wait_for_worker_lock(
            &service.command.home,
            Duration::ZERO,
            &Cancellation::default(),
        )
        .unwrap();
        service.command_timeout = Duration::ZERO;
        assert!(
            matches!(service.discard_import_group(&group), Err(BackupError::Io(error)) if error.kind() == io::ErrorKind::TimedOut)
        );
        for stage in &stages {
            assert_eq!(fs::read(stage.join("partial")).unwrap(), b"still writing");
        }
        assert!(service.runner.calls.lock().unwrap().is_empty());
        drop(worker);
        service.command_timeout = Duration::from_secs(1);
        service.discard_import_group(&group).unwrap();
        for stage in stages {
            assert!(!stage.exists());
        }
    }

    #[test]
    fn failed_import_verification_removes_the_loaded_group_children_first() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let service = service(&temp, FakeRunner::default());
        create_one(&service, destination.clone(), false).unwrap();
        service
            .runner
            .fail_import_verify
            .store(true, Ordering::Release);
        let group = new_import_group();
        let result = service.prepare_restore_in_group(
            restore_request(destination),
            &group,
            &Cancellation::default(),
            &|| Ok(()),
        );
        assert!(matches!(result, Err(BackupError::CommandFailed { .. })));
        // The root becomes head, the child goes first, then the root: no --force.
        assert_eq!(
            import_group_calls(&service, "head"),
            [vec![
                "snapshot".to_string(),
                "head".into(),
                format!("{group}:imported-parent")
            ]]
        );
        assert_eq!(
            import_group_calls(&service, "remove"),
            [
                vec![
                    "snapshot".to_string(),
                    "remove".into(),
                    "--quiet".into(),
                    format!("{group}:imported-member")
                ],
                vec![
                    "snapshot".to_string(),
                    "remove".into(),
                    "--quiet".into(),
                    format!("{group}:imported-parent")
                ],
            ]
        );
        assert!(!service
            .runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args.iter().any(|arg| arg == "--force" || arg == "-f")));
    }

    #[test]
    fn import_persists_owned_group_before_load_and_does_not_load_if_the_journal_fails() {
        for journal_fails in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let destination = temp.path().join("dev.silo-backup");
            let service = service(&temp, FakeRunner::default());
            create_one(&service, destination.clone(), false).unwrap();
            let group = new_import_group();
            let recorded = AtomicBool::new(false);
            let result = service.prepare_restore_in_group(
                restore_request(destination),
                &group,
                &Cancellation::default(),
                &|| {
                    assert!(import_group_calls(&service, "load").is_empty());
                    recorded.store(true, Ordering::Release);
                    if journal_fails {
                        Err(BackupError::InvalidRequest(
                            "test journal write failed".into(),
                        ))
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(recorded.load(Ordering::Acquire));
            if journal_fails {
                assert!(result.is_err());
                assert!(import_group_calls(&service, "load").is_empty());
                assert!(service.runner.removed.lock().unwrap().is_empty());
            } else {
                assert_eq!(result.unwrap().snapshot_group, group);
                assert_eq!(import_group_calls(&service, "load").len(), 1);
            }
        }
    }

    #[test]
    fn cancelled_load_removes_the_partial_import_and_runtime_staging() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let service = service(&temp, FakeRunner::default());
        create_one(&service, destination.clone(), false).unwrap();
        service
            .runner
            .cancel_during_load
            .store(true, Ordering::Release);
        let result =
            service.prepare_restore(restore_request(destination), &Cancellation::default());
        assert!(matches!(result, Err(BackupError::Cancelled)));
        assert_eq!(import_group_calls(&service, "remove").len(), 2);
        let calls = import_group_calls(&service, "load");
        for path in native_import_stage_paths(&temp.path().join("home"), &calls[0][4]).unwrap() {
            assert!(!path.exists());
        }
        assert!(fs::read_dir(temp.path().join("scratch"))
            .unwrap()
            .next()
            .is_none());
    }

    #[test]
    fn an_existing_import_group_is_refused_and_never_cleaned_up() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let group = new_import_group();
        let runner = FakeRunner::default();
        runner
            .existing_members
            .lock()
            .unwrap()
            .push((group.clone(), "someone-else".into()));
        let service = service(&temp, runner);
        create_one(&service, destination.clone(), false).unwrap();
        let result = service.prepare_restore_in_group(
            restore_request(destination),
            &group,
            &Cancellation::default(),
            &|| panic!("an existing group is never journaled as owned"),
        );
        assert!(matches!(result, Err(BackupError::ImportGroupConflict(_))));
        assert!(import_group_calls(&service, "load").is_empty());
        assert!(service.runner.removed.lock().unwrap().is_empty());
    }

    #[test]
    fn discarding_is_limited_to_import_groups_and_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let service = service(&temp, FakeRunner::default());
        for foreign in [
            "dev",
            "silo-import-short",
            "silo-import-6B79CF8F70B34F2D93D13EEB3798A8B9",
        ] {
            assert!(matches!(
                service.discard_import_group(foreign),
                Err(BackupError::InvalidRequest(_))
            ));
        }
        assert!(service.runner.calls.lock().unwrap().is_empty());
        service.discard_import_group(&new_import_group()).unwrap();
        let calls = service.runner.calls.lock().unwrap();
        assert!(calls
            .iter()
            .all(|args| args.get(1).is_some_and(|arg| arg == "list")));
    }

    fn create_two(service: &BackupService<FakeRunner>, destination: PathBuf) {
        let source = |name: &str| BackupSource {
            name: name.into(),
            snapshot_group: name.into(),
            was_running: false,
            runtime_config: managed_config(name),
            computer_configuration: computer_configuration(name),
            existing_member: None,
        };
        service
            .create_backup(
                BackupRequest {
                    destination,
                    sources: vec![source("dev"), source("second")],
                },
                &Cancellation::default(),
            )
            .unwrap();
    }

    /// Byte offset of the first payload in a written archive.
    fn first_payload_offset(bytes: &[u8]) -> usize {
        let manifest_len = u64::from_be_bytes(bytes[20..28].try_into().unwrap()) as usize;
        MAGIC.len() + 4 + 8 + manifest_len
    }

    #[test]
    fn import_extracts_and_verifies_only_the_selected_computer() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("two.silo-backup");
        let service = service(&temp, FakeRunner::default());
        create_two(&service, archive.clone());
        // Damage the first computer's payload; the review inspection sees it,
        // but importing the second computer never reads that payload.
        let mut bytes = fs::read(&archive).unwrap();
        let offset = first_payload_offset(&bytes);
        bytes[offset + 5] ^= 0xff;
        fs::write(&archive, &bytes).unwrap();
        assert!(matches!(
            service.inspect_archive(&archive, &Cancellation::default()),
            Err(BackupError::InvalidArchive(_))
        ));
        let restored = service
            .prepare_restore(
                RestoreRequest {
                    archive,
                    source_name: Some("second".into()),
                    new_name: "second-copy".into(),
                },
                &Cancellation::default(),
            )
            .unwrap();
        assert_eq!(restored.source_name, "second");
        let staged = fs::read_dir(restored._stage.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(staged, ["snapshot-1.tar.zst"]);
    }

    #[test]
    fn describing_an_archive_reads_its_manifest_but_no_payload() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("two.silo-backup");
        let service = service(&temp, FakeRunner::default());
        create_two(&service, archive.clone());
        let mut bytes = fs::read(&archive).unwrap();
        let offset = first_payload_offset(&bytes);
        bytes[offset + 5] ^= 0xff;
        fs::write(&archive, &bytes).unwrap();
        // A damaged payload is only found by hashing it.
        assert!(service
            .inspect_archive(&archive, &Cancellation::default())
            .is_err());
        let described = service
            .describe_archive(&archive, &Cancellation::default())
            .unwrap();
        assert_eq!(described.computers, ["dev", "second"]);
        assert_eq!(described.size_bytes, bytes.len() as u64);
        // The header, manifest and length are still checked.
        bytes.truncate(bytes.len() - 1);
        fs::write(&archive, &bytes).unwrap();
        assert!(matches!(
            service.describe_archive(&archive, &Cancellation::default()),
            Err(BackupError::InvalidArchive(_))
        ));
    }

    #[test]
    fn export_hashes_while_copying_and_refuses_a_payload_that_changed_size() {
        let temp = tempfile::tempdir().unwrap();
        let payload_path = temp.path().join("payload.msb");
        fs::write(&payload_path, b"\x28\xb5\x2f\xfdpayload").unwrap();
        let computer = |size: u64| PackageComputer {
            name: "dev".into(),
            runtime_config: managed_config("dev"),
            computer_configuration: computer_configuration("dev"),
            payload_size: size,
            payload_sha256: pending_digest(),
            volumes: Vec::new(),
        };
        let manifest = |size: u64| PackageManifest {
            schema_version: FORMAT_VERSION,
            created_at_ms: 1,
            runtime: RuntimeManifest {
                name: "microsandbox".into(),
                version: bundled_runtime_version().into(),
                snapshot_format: snapshot_format_for(bundled_runtime_version()),
                guest_architecture: std::env::consts::ARCH.into(),
            },
            computers: vec![computer(size)],
        };
        let destination = temp.path().join("out.silo-backup");
        let mut stale = manifest(4);
        let error = write_immutable_package(
            &destination,
            &mut stale,
            vec![File::open(&payload_path).unwrap()],
            &Cancellation::default(),
            None,
            available_bytes,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("changed while it was being exported"),
            "{error}"
        );
        assert!(!destination.exists());
        assert!(fs::read_dir(temp.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".silo-backup-")
        }));

        let length = fs::metadata(&payload_path).unwrap().len();
        let mut current = manifest(length);
        write_immutable_package(
            &destination,
            &mut current,
            vec![File::open(&payload_path).unwrap()],
            &Cancellation::default(),
            None,
            available_bytes,
        )
        .unwrap();
        let expected = format!(
            "sha256:{:x}",
            Sha256::digest(fs::read(&payload_path).unwrap())
        );
        assert_eq!(current.computers[0].payload_sha256, expected);
        let written = read_and_verify_package(
            &destination,
            DEFAULT_MAX_ARCHIVE_BYTES,
            &Cancellation::default(),
            PayloadMode::VerifyAll,
        )
        .unwrap();
        assert_eq!(written.manifest.computers[0].payload_sha256, expected);
    }

    /// Export an archive whose snapshot payload is `payload`, then try to import it.
    fn import_crafted(
        payload: Vec<u8>,
        free_space: Option<fn(&Path) -> io::Result<u64>>,
    ) -> (Result<PreparedRestore, BackupError>, Vec<Vec<String>>) {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("crafted.silo-backup");
        let runner = FakeRunner::default();
        *runner.saved_payload.lock().unwrap() = Some(payload);
        let mut service = service(&temp, runner);
        if let Some(free_space) = free_space {
            service.free_space = free_space;
        }
        create_one(&service, destination.clone(), false).unwrap();
        let result =
            service.prepare_restore(restore_request(destination), &Cancellation::default());
        let calls = service.runner.calls.lock().unwrap().clone();
        (result, calls)
    }

    fn loaded(calls: &[Vec<String>]) -> bool {
        calls
            .iter()
            .any(|args| args.get(1).is_some_and(|arg| arg == "load"))
    }

    fn scan_error(result: Result<PreparedRestore, BackupError>) -> String {
        match result {
            Err(BackupError::InvalidArchive(detail)) => detail,
            Err(other) => panic!("expected an invalid archive, got {other}"),
            Ok(_) => panic!("the crafted archive was accepted"),
        }
    }

    #[test]
    fn prescan_refuses_links_before_the_runtime_loads_anything() {
        let payload = zstd_tar(|builder| {
            let mut header = tar_header("snapshots/escape", tar::EntryType::Symlink, 0);
            header.set_link_name("/etc/passwd").unwrap();
            header.set_cksum();
            builder.append(&header, io::empty()).unwrap();
        });
        let (result, calls) = import_crafted(payload, None);
        assert!(scan_error(result).contains("link, device or other special entry"));
        assert!(!loaded(&calls));
    }

    #[test]
    fn prescan_refuses_parent_directory_paths() {
        let payload = zstd_tar(|builder| {
            let mut header = tar_header("snapshots/placeholder", tar::EntryType::Regular, 4);
            // tar::Header::set_path refuses `..`, so write the raw name field.
            let name = b"../escape";
            let old = header.as_old_mut();
            old.name = [0; 100];
            old.name[..name.len()].copy_from_slice(name);
            header.set_cksum();
            builder.append(&header, &b"evil"[..]).unwrap();
        });
        let (result, calls) = import_crafted(payload, None);
        assert!(scan_error(result).contains("unsafe path"));
        assert!(!loaded(&calls));
    }

    #[test]
    fn prescan_refuses_pax_headers() {
        let payload = zstd_tar(|builder| {
            let record = b"20 path=snapshots/a\n";
            builder
                .append(
                    &tar_header("PaxHeader/a", tar::EntryType::XHeader, record.len() as u64),
                    &record[..],
                )
                .unwrap();
            builder
                .append(
                    &tar_header("snapshots/b", tar::EntryType::Regular, 2),
                    &b"{}"[..],
                )
                .unwrap();
        });
        let (result, calls) = import_crafted(payload, None);
        assert!(scan_error(result).contains("extended or link headers"));
        assert!(!loaded(&calls));
    }

    #[test]
    fn prescan_stops_a_compression_bomb_at_the_free_space_budget() {
        // 64 MiB of zeros compresses to a few KiB; only 8 MiB may be unpacked.
        let zeros = vec![0_u8; 64 * 1024 * 1024];
        let payload = zstd_tar(|builder| {
            builder
                .append(
                    &tar_header(
                        "layers/zeros.raw",
                        tar::EntryType::Regular,
                        zeros.len() as u64,
                    ),
                    zeros.as_slice(),
                )
                .unwrap();
        });
        assert!(payload.len() < 1024 * 1024);
        let (result, calls) =
            import_crafted(payload, Some(|_| Ok(FREE_SPACE_RESERVE + 8 * 1024 * 1024)));
        let Err(BackupError::InsufficientSpace(detail)) = result else {
            panic!("the bomb was not stopped at the space budget");
        };
        assert!(detail.contains("needs more than the 8.0 MiB"), "{detail}");
        assert!(!loaded(&calls));
    }

    #[test]
    fn prescan_caps_entry_count_and_sparse_logical_size() {
        let limits = ScanLimits {
            max_unpacked_bytes: 1024 * 1024,
            max_entries: 2,
            max_sparse_bytes: 1024 * 1024,
        };
        let three = zstd_tar(|builder| {
            for name in ["a", "b", "c"] {
                builder
                    .append(
                        &tar_header(&format!("snapshots/{name}"), tar::EntryType::Regular, 1),
                        &b"x"[..],
                    )
                    .unwrap();
            }
        });
        assert!(matches!(
            scan_snapshot_archive(three.as_slice(), limits, &Cancellation::default()),
            Err(ScanFailure::Unsafe(detail)) if detail.contains("more than 2 entries")
        ));

        // A GNU sparse entry: 512 stored bytes describing a 2 GiB disk layer.
        let sparse = zstd_tar(|builder| {
            let mut header = tar_header("layers/layer_1.raw", tar::EntryType::GNUSparse, 512);
            let gnu = header.as_gnu_mut().unwrap();
            let logical = 2_u64 << 30;
            gnu.sparse[0]
                .offset
                .copy_from_slice(format!("{:011o}\0", 0).as_bytes());
            gnu.sparse[0]
                .numbytes
                .copy_from_slice(format!("{:011o}\0", 512).as_bytes());
            gnu.sparse[1]
                .offset
                .copy_from_slice(format!("{logical:011o}\0").as_bytes());
            gnu.sparse[1]
                .numbytes
                .copy_from_slice(format!("{:011o}\0", 0).as_bytes());
            gnu.realsize
                .copy_from_slice(format!("{logical:011o}\0").as_bytes());
            header.set_cksum();
            builder.append(&header, &[7_u8; 512][..]).unwrap();
        });
        assert!(matches!(
            scan_snapshot_archive(sparse.as_slice(), limits, &Cancellation::default()),
            Err(ScanFailure::Unsafe(detail)) if detail.contains("larger than any disk")
        ));
        let roomy = ScanLimits {
            max_sparse_bytes: 4 << 30,
            ..limits
        };
        let scan = scan_snapshot_archive(sparse.as_slice(), roomy, &Cancellation::default())
            .ok()
            .unwrap();
        // Only the stored bytes count toward the budget, not the holes.
        assert_eq!(scan.entries, 1);
        assert!(scan.unpacked_bytes < 4096);
    }

    #[test]
    fn prescan_accepts_a_runtime_archive_and_counts_it() {
        let scan = scan_snapshot_archive(
            fake_snapshot_archive().as_slice(),
            ScanLimits {
                max_unpacked_bytes: 1024 * 1024,
                max_entries: 16,
                max_sparse_bytes: 0,
            },
            &Cancellation::default(),
        )
        .ok()
        .unwrap();
        assert_eq!(scan.entries, 2);
        assert!(scan.unpacked_bytes >= 3 * 512);
        let (result, calls) = import_crafted(b"\x28\xb5\x2f\xfdnot really zstd".to_vec(), None);
        assert!(scan_error(result).contains("not safe checkpoint data"));
        assert!(!loaded(&calls));
    }

    /// Import an honest export whose loaded snapshot descriptor was altered.
    fn import_with_descriptor(
        alter: impl FnOnce(&mut Value),
    ) -> (Result<PreparedRestore, BackupError>, Vec<String>) {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let runner = FakeRunner::default();
        let mut descriptor = loaded_descriptor_for(&managed_config("dev"));
        alter(&mut descriptor);
        *runner.loaded_descriptor.lock().unwrap() = Some(descriptor);
        let service = service(&temp, runner);
        create_one(&service, destination.clone(), false).unwrap();
        let result =
            service.prepare_restore(restore_request(destination), &Cancellation::default());
        let removed = service.runner.removed.lock().unwrap().clone();
        (result, removed)
    }

    fn refused_because(result: Result<PreparedRestore, BackupError>) -> String {
        match result {
            Err(BackupError::InvalidArchive(detail)) => {
                assert!(
                    detail.starts_with("the loaded checkpoint does not match"),
                    "{detail}"
                );
                detail
            }
            Err(other) => panic!("expected a descriptor mismatch, got {other}"),
            Ok(_) => panic!("the altered snapshot was accepted"),
        }
    }

    #[test]
    fn loaded_snapshot_may_not_add_mounts_env_image_or_init() {
        let extra_mount = |descriptor: &mut Value| {
            let mut volume = descriptor["extensions"][OWNED_VOLUMES_EXTENSION][0].clone();
            volume["mount_id"] = "host".into();
            volume["mount"]["guest"] = "/host".into();
            descriptor["extensions"][OWNED_VOLUMES_EXTENSION]
                .as_array_mut()
                .unwrap()
                .push(volume);
        };
        let cases: Vec<(&str, Box<dyn FnOnce(&mut Value)>)> = vec![
            ("adds or omits a mounted volume", Box::new(extra_mount)),
            (
                "undeclared setting (env)",
                Box::new(|descriptor: &mut Value| {
                    descriptor["env"] =
                        serde_json::json!([{"key": "LD_PRELOAD", "value": "/tmp/x.so"}]);
                }),
            ),
            (
                "undeclared setting (patches)",
                Box::new(|descriptor: &mut Value| {
                    descriptor["patches"] = serde_json::json!([{"path": "/etc/profile"}]);
                }),
            ),
            (
                "undeclared setting (init)",
                Box::new(|descriptor: &mut Value| {
                    descriptor["init"] = serde_json::json!({"cmd": ["/bin/evil"]});
                }),
            ),
            (
                "different image",
                Box::new(|descriptor: &mut Value| {
                    descriptor["image"]["reference"] = "attacker.example/evil:latest".into();
                }),
            ),
            (
                "different default user",
                Box::new(|descriptor: &mut Value| {
                    descriptor["requires"] =
                        serde_json::json!([OWNED_VOLUMES_EXTENSION, RESTORE_DEFAULTS_EXTENSION]);
                    descriptor["extensions"][RESTORE_DEFAULTS_EXTENSION] =
                        serde_json::json!({"user": "root"});
                }),
            ),
            (
                "unsupported runtime extension",
                Box::new(|descriptor: &mut Value| {
                    descriptor["requires"] =
                        serde_json::json!([OWNED_VOLUMES_EXTENSION, "vendor.host-mounts"]);
                }),
            ),
            (
                "different storage",
                Box::new(|descriptor: &mut Value| {
                    descriptor["extensions"][OWNED_VOLUMES_EXTENSION][0]["mount"]["storage"]
                        ["capacity_mib"] = 4_194_304.into();
                }),
            ),
            (
                "different root disk layout",
                Box::new(|descriptor: &mut Value| {
                    descriptor["root_disk"] = serde_json::json!({"layout": "flat"});
                }),
            ),
            (
                "names another checkpoint",
                Box::new(|descriptor: &mut Value| {
                    descriptor["snapshot_id"] = "snap_22222222222222222222222222222222".into();
                }),
            ),
        ];
        for (expected, alter) in cases {
            let (result, removed) = import_with_descriptor(alter);
            let detail = refused_because(result);
            assert!(detail.contains(expected), "{expected}: {detail}");
            assert_eq!(
                removed.len(),
                2,
                "{expected}: the loaded group must be removed"
            );
        }
    }

    #[test]
    fn loaded_snapshot_must_preserve_default_owned_mount_policies() {
        for (field, replacement) in [
            ("stat_virtualization", "relaxed"),
            ("host_permissions", "mirror"),
        ] {
            let (result, removed) = import_with_descriptor(|descriptor| {
                descriptor["extensions"][OWNED_VOLUMES_EXTENSION][0]["mount"][field] =
                    replacement.into();
            });
            let error = refused_because(result);
            assert!(error.contains(field), "{error}");
            assert_eq!(removed.len(), 2);
        }
    }

    #[test]
    fn descriptor_scope_names_are_the_runtimes_and_the_index_spellings() {
        // Real descriptors written by `msb snapshot create` (see test_support/msb-descriptor).
        for fixture in [
            include_str!("test_support/msb-descriptor/checkpoint-0.7.4.json"),
            include_str!("test_support/msb-descriptor/checkpoint-0.7.6.json"),
        ] {
            let descriptor: Value = serde_json::from_str(fixture).unwrap();
            assert!(descriptor_scope_supported(
                descriptor["scope"].as_str(),
                descriptor.pointer("/state/kind").and_then(Value::as_str)
            ));
        }
        for (scope, kind, supported) in [
            ("file", "file", true),
            ("checkpoint", "checkpoint", true),
            ("disk", "file", true),
            ("full", "checkpoint", true),
            ("file", "checkpoint", false),
            ("checkpoint", "file", false),
            ("disk", "checkpoint", false),
            ("full", "file", false),
            ("resumable", "checkpoint", false),
        ] {
            assert_eq!(
                descriptor_scope_supported(Some(scope), Some(kind)),
                supported,
                "{scope}/{kind}"
            );
        }
        assert!(!descriptor_scope_supported(None, Some("file")));
        // Both spellings of a disk capture import; a disk scope on a checkpoint state does not.
        for scope in ["file", "disk"] {
            let (result, _) = import_with_descriptor(|descriptor| {
                descriptor["scope"] = scope.into();
            });
            assert!(result.is_ok(), "{scope}");
        }
        let (result, removed) = import_with_descriptor(|descriptor| {
            descriptor["scope"] = "checkpoint".into();
        });
        assert!(refused_because(result).contains("capture scope"));
        assert_eq!(removed.len(), 2);
    }

    #[test]
    fn full_checkpoint_import_requires_its_captured_geometry() {
        let full = |vcpus: u64| {
            move |descriptor: &mut Value| {
                descriptor["scope"] = "checkpoint".into();
                descriptor["state"] = serde_json::json!({
                    "kind": "checkpoint",
                    "checkpoint_id": "ckpt",
                    "checkpoint_root": format!("sha256:{}", "d".repeat(64)),
                    "restore_intents": ["clone", "resume"],
                    "requirements_summary": {
                        "architecture": std::env::consts::ARCH,
                        "vcpus": vcpus, "max_vcpus": 6, "memory_mib": 16384, "max_memory_mib": 32768
                    }
                });
            }
        };
        let (result, _) = import_with_descriptor(full(2));
        assert!(refused_because(result).contains("CPU or memory layout"));
        let (result, removed) = import_with_descriptor(full(4));
        assert!(result.is_ok());
        assert!(removed.is_empty());
    }

    #[test]
    fn disk_heavy_commands_get_time_for_the_data_they_move() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let service = service(&temp, FakeRunner::default());
        create_one(&service, destination.clone(), true).unwrap();
        service
            .prepare_restore(restore_request(destination), &Cancellation::default())
            .unwrap();
        let timeouts = service.runner.timeouts.lock().unwrap();
        let timeout = |verb: &str| {
            timeouts
                .iter()
                .filter(|(args, _)| {
                    args.first().is_some_and(|arg| arg == "snapshot") && args[1] == verb
                })
                .map(|(_, timeout)| *timeout)
                .collect::<Vec<_>>()
        };
        // 60 GiB computer + 80 GiB runtime at 16 MiB/s is 8960 s on top of the hour.
        let export = DEFAULT_COMMAND_TIMEOUT + Duration::from_secs(140 * 1024 / 16);
        assert_eq!(timeout("create"), [export]);
        assert_eq!(timeout("save"), [export]);
        assert_eq!(timeout("verify")[0], export);
        // The import knows exactly what it unpacks: a few KiB here.
        assert_eq!(timeout("load"), [DEFAULT_COMMAND_TIMEOUT]);
        assert_eq!(timeout("list")[0], DEFAULT_COMMAND_TIMEOUT);
        drop(timeouts);
        let service = BackupService {
            command_timeout: Duration::from_secs(60),
            ..service
        };
        assert_eq!(
            service.data_timeout(500 * 1024 * 1024 * 1024),
            Duration::from_secs(60 + 500 * 1024 / 16)
        );
    }

    #[test]
    fn directory_sync_failure_preserves_published_files() {
        let temp = tempfile::tempdir().unwrap();
        for replace in [true, false] {
            let source = temp.path().join(".export.tmp");
            let destination = temp.path().join(format!("export-{replace}.silo-backup"));
            fs::write(&source, b"verified export").unwrap();
            let result = publish_package(&source, &destination, temp.path(), |_| {
                if replace {
                    let replacement = temp.path().join("replacement");
                    fs::write(&replacement, b"another writer's file")?;
                    fs::rename(&replacement, &destination)?;
                }
                Err(io::Error::from_raw_os_error(libc::EIO))
            });
            assert!(
                matches!(result, Err(BackupError::Io(error)) if error.raw_os_error() == Some(libc::EIO))
            );
            assert_eq!(
                fs::read(&destination).unwrap(),
                if replace {
                    b"another writer's file".as_slice()
                } else {
                    b"verified export".as_slice()
                }
            );
            assert!(!source.exists());
        }
    }

    #[test]
    fn publishing_refuses_volumes_without_safe_publication() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join(".export.tmp");
        let destination = temp.path().join("export.silo-backup");
        for link_error in [libc::EPERM, libc::ENOTSUP, libc::EMLINK] {
            fs::write(&source, b"verified export").unwrap();
            let result = rename_without_replacing_with(
                &source,
                &destination,
                |_, _| Err(io::Error::from_raw_os_error(libc::EINVAL)),
                |_, _| Err(io::Error::from_raw_os_error(link_error)),
            );
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Unsupported);
            assert_eq!(fs::read(&source).unwrap(), b"verified export");
            assert!(!destination.exists());
        }
        let result = rename_without_replacing_with(
            &source,
            &destination,
            |_, _| Err(io::Error::from_raw_os_error(libc::EINVAL)),
            |_, destination| {
                fs::write(destination, b"another writer's file")?;
                Err(io::Error::from_raw_os_error(libc::EPERM))
            },
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Unsupported);
        assert_eq!(fs::read(&source).unwrap(), b"verified export");
        assert_eq!(fs::read(&destination).unwrap(), b"another writer's file");
    }

    #[test]
    fn publishing_falls_back_when_the_volume_rejects_exclusive_rename() {
        let rejects = |_: &Path, _: &Path| Err(io::Error::from_raw_os_error(libc::EINVAL));
        let no_links = |_: &Path, _: &Path| Err(io::Error::from_raw_os_error(libc::EPERM));
        let temp = tempfile::tempdir().unwrap();
        let fresh = |name: &str| {
            let source = temp.path().join(format!(".{name}.tmp"));
            fs::write(&source, name).unwrap();
            (source, temp.path().join(format!("{name}.silo-backup")))
        };
        let name = "linked";
        let (source, destination) = fresh(name);
        rename_without_replacing_with(&source, &destination, rejects, |s, d| fs::hard_link(s, d))
            .unwrap();
        assert_eq!(fs::read_to_string(&destination).unwrap(), name);
        assert!(!source.exists(), "{name}");

        // A taken name is never replaced by the fallback.
        let (source, _) = fresh(&format!("{name}-again"));
        let result = rename_without_replacing_with(&source, &destination, rejects, |s, d| {
            fs::hard_link(s, d)
        });
        assert_eq!(
            result.unwrap_err().kind(),
            io::ErrorKind::AlreadyExists,
            "{name}"
        );
        assert_eq!(fs::read_to_string(&destination).unwrap(), name);
        assert!(source.exists());
        // Other errors from the exclusive rename are not retried differently.
        let (source, destination) = fresh("denied");
        let denied = |_: &Path, _: &Path| Err(io::Error::from_raw_os_error(libc::EACCES));
        assert!(rename_without_replacing_with(&source, &destination, denied, no_links).is_err());
        assert!(!destination.exists());
    }

    #[test]
    fn export_checks_working_space_before_runtime_capture_or_save() {
        let temp = tempfile::tempdir().unwrap();
        let mut service = service(&temp, FakeRunner::default());
        service.free_space = |path| {
            Ok(if path.ends_with("scratch") {
                FREE_SPACE_RESERVE
            } else {
                u64::MAX / 2
            })
        };
        let error = create_one(&service, temp.path().join("dev.silo-backup"), false).unwrap_err();
        assert!(matches!(error, BackupError::InsufficientSpace(_)));
        assert!(error.to_string().contains("working copy"), "{error}");
        assert!(service.runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn export_checks_the_destination_space_before_copying() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let mut service = service(&temp, FakeRunner::default());
        service.free_space = |_| Ok(FREE_SPACE_RESERVE + 16);
        let error = create_one(&service, destination.clone(), false).unwrap_err();
        assert!(matches!(error, BackupError::InsufficientSpace(_)));
        let message = error.to_string();
        assert!(message.starts_with("This export needs "), "{message}");
        assert!(message.contains("is available"), "{message}");
        assert!(!destination.exists());
        assert!(fs::read_dir(temp.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".silo-backup-")
        }));
    }

    #[test]
    fn import_checks_working_and_runtime_space_before_loading() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let mut service = service(&temp, FakeRunner::default());
        create_one(&service, destination.clone(), false).unwrap();

        // No room for the private working copy.
        service.free_space = |_| Ok(FREE_SPACE_RESERVE);
        let error = service
            .prepare_restore(
                restore_request(destination.clone()),
                &Cancellation::default(),
            )
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("for its private working copy"),
            "{error}"
        );

        // Room to stage, but the runtime store cannot take the unpacked data.
        service.free_space = |path| {
            Ok(if path.ends_with("home") {
                FREE_SPACE_RESERVE + 100
            } else {
                u64::MAX / 2
            })
        };
        let error = service
            .prepare_restore(
                restore_request(destination.clone()),
                &Cancellation::default(),
            )
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("available to Silo's runtime storage"),
            "{error}"
        );

        // The runtime store filled up while the payload was being unpacked.
        static CALLS: AtomicU64 = AtomicU64::new(0);
        CALLS.store(0, Ordering::SeqCst);
        service.free_space = |_| {
            Ok(if CALLS.fetch_add(1, Ordering::SeqCst) < 2 {
                u64::MAX / 2
            } else {
                FREE_SPACE_RESERVE
            })
        };
        let error = service
            .prepare_restore(restore_request(destination), &Cancellation::default())
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("free space in Silo's runtime storage"),
            "{error}"
        );
        let calls = service.runner.calls.lock().unwrap();
        assert!(!calls
            .iter()
            .any(|args| args.get(1).is_some_and(|arg| arg == "load")));
    }

    #[test]
    fn import_guard_discards_unless_the_computer_was_saved() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("dev.silo-backup");
        let service = service(&temp, FakeRunner::default());
        create_one(&service, destination.clone(), false).unwrap();
        let kept = service
            .prepare_restore(
                restore_request(destination.clone()),
                &Cancellation::default(),
            )
            .unwrap();
        service
            .discard_import_on_failure(&kept.snapshot_group)
            .keep();
        assert!(service.runner.removed.lock().unwrap().is_empty());

        let failed = service
            .prepare_restore(restore_request(destination), &Cancellation::default())
            .unwrap();
        {
            let _guard = service.discard_import_on_failure(&failed.snapshot_group);
            // A later step (metadata write, identity check) fails here.
        }
        let removed = service.runner.removed.lock().unwrap();
        assert_eq!(removed.len(), 2);
        assert!(removed
            .iter()
            .all(|selector| selector.starts_with(&failed.snapshot_group)));
    }

    #[test]
    fn manifest_does_not_allow_path_like_computer_names() {
        assert!(validate_computer_name("../victim").is_err());
        assert!(validate_computer_name("/absolute").is_err());
        assert!(validate_computer_name("valid-name").is_ok());
    }
}
