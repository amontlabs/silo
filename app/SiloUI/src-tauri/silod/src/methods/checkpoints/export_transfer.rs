//! `exports.transfer`: copies an export file between this device and another over Connections.
//!
//! The controller's half of sharing a computer that lives on another device. The owner writes
//! an export into its transfer area (`exports.create` with `transferArea`) or imports from a
//! file it received there (`exports.import` with `transferName`); this job moves the bytes
//! with the owner's `exports.file` stream (`silo_engine::transfer_area`). The job owns
//! progress, cancel and the time limit, and raw bytes never cross `engine.sock`: a client only
//! starts the job and follows it.
//!
//! A receive writes the file at `path` (which must not exist) and removes it again if any part
//! of the copy fails. A send reads the file at `path` and returns the name the owner gave it.
//! Either way the result is the owner's `TransferFile`. Removing the owner's copy is the
//! client's explicit `exports.discard` after it is done.

use std::{
    fs::{File, OpenOptions},
    io::{BufRead, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use silo_api::{
    jobs::{ErrorCode, JobId, JobProgress},
    methods::{
        exports::{ExportTransferDirection, ExportTransferParams, TransferFile},
        files::{TransferDirection, TransferProgress, TransferState},
        operations::OperationKind,
    },
};
use silo_engine::{
    context::Audience,
    jobs::CancelRoute,
    transfer_area::{self, FileInfo},
};

use crate::{
    context::SilodContext,
    dispatcher::{DispatchError, HandlerOutput},
};

use super::{blocking, output, HandlerContext};

pub(crate) const TOO_OLD: &str =
    "The other machine's Silo is too old to share computers over Connections. Update Silo there.";

/// How often progress is published.
const PROGRESS_EVERY: Duration = Duration::from_millis(500);

/// An open `exports.file` stream. Dropping `guard` closes it.
pub(crate) struct Opened {
    pub input: Box<dyn Write + Send>,
    pub output: Box<dyn BufRead + Send>,
    pub guard: Box<dyn Send>,
}

/// How this engine reaches the owner. Production goes over the Connections SSH bridge; tests
/// supply streams.
pub(crate) trait FileLink: Send + Sync {
    fn capabilities(&self, device: &str) -> Result<Vec<String>, String>;
    fn open(
        &self,
        device: &str,
        params: Value,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Opened, String>;
}

struct SshLink;

impl FileLink for SshLink {
    fn capabilities(&self, device: &str) -> Result<Vec<String>, String> {
        silo_engine::remote::device_capabilities(device).map_err(|error| error.message)
    }

    fn open(
        &self,
        device: &str,
        params: Value,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Opened, String> {
        let stream = silo_engine::remote::open_bridge_stream(
            device,
            transfer_area::FILE_STREAM,
            params,
            cancelled,
        )
        .map_err(|error| error.message)?;
        let silo_engine::remote::BridgeStream {
            child,
            input,
            output,
        } = stream;
        Ok(Opened {
            input: Box::new(input),
            output: Box::new(output),
            guard: Box::new(child),
        })
    }
}

#[cfg(test)]
static TEST_LINK: Mutex<Option<Arc<dyn FileLink>>> = Mutex::new(None);
#[cfg(test)]
static TEST_LINK_IN_USE: Mutex<()> = Mutex::new(());

/// Replaces the link to the owner for one test; tests that do so run one at a time.
#[cfg(test)]
pub(crate) struct TestLink(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

#[cfg(test)]
pub(crate) fn install_test_link(link: Arc<dyn FileLink>) -> TestLink {
    let guard = TEST_LINK_IN_USE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *TEST_LINK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(link);
    TestLink(guard)
}

#[cfg(test)]
impl Drop for TestLink {
    fn drop(&mut self) {
        *TEST_LINK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}

fn link() -> Arc<dyn FileLink> {
    #[cfg(test)]
    if let Some(link) = TEST_LINK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
    {
        return link;
    }
    Arc::new(SshLink)
}

/// Refuses a device whose Silo does not serve `exports.file`.
pub(crate) fn require_file_sharing(device: &str) -> Result<(), DispatchError> {
    let capabilities = link()
        .capabilities(device)
        .map_err(|message| DispatchError::internal(message))?;
    if capabilities
        .iter()
        .any(|name| name == transfer_area::FILE_STREAM)
    {
        Ok(())
    } else {
        Err(DispatchError {
            code: ErrorCode::UnsupportedRemoteOperation,
            message: TOO_OLD.to_owned(),
            details: None,
        })
    }
}

/// A validated request.
#[derive(Debug)]
struct Plan {
    direction: ExportTransferDirection,
    path: PathBuf,
    expected: Option<FileInfo>,
    computer: String,
}

fn invalid(message: &str) -> DispatchError {
    DispatchError::internal(format!("Invalid params: {message}"))
}

fn plan(params: &ExportTransferParams) -> Result<Plan, DispatchError> {
    let path = PathBuf::from(&params.path);
    if !path.is_absolute() {
        return Err(invalid("path must be an absolute path."));
    }
    let computer = params.computer.clone().unwrap_or_default();
    match params.direction {
        ExportTransferDirection::Receive => {
            let (Some(name), Some(bytes), Some(sha256)) =
                (&params.name, params.bytes, &params.sha256)
            else {
                return Err(invalid("receiving needs name, bytes and sha256."));
            };
            if transfer_area::parse_name(name) != Some(transfer_area::Role::Export) {
                return Err(invalid("name is not an export file of the other machine."));
            }
            if bytes == 0 || bytes > transfer_area::MAX_BYTES {
                return Err(invalid("bytes is outside what Silo transfers."));
            }
            if !transfer_area::is_sha256(sha256) {
                return Err(invalid("sha256 is not a lowercase hex checksum."));
            }
            if path.extension().and_then(|value| value.to_str()) != Some("silo-backup") {
                return Err(invalid("the file name must end in .silo-backup."));
            }
            let folder = path
                .parent()
                .ok_or_else(|| invalid("path has no folder."))?;
            if !folder.is_dir() {
                return Err(invalid("the folder for the file does not exist."));
            }
            if path.symlink_metadata().is_ok() {
                return Err(DispatchError::internal(format!(
                    "A file already exists at {}.",
                    path.display()
                )));
            }
            let free = transfer_area::free_space(folder).map_err(DispatchError::internal)?;
            if free < bytes.saturating_add(transfer_area::FREE_SPACE_MARGIN) {
                return Err(DispatchError::internal(
                    "There is not enough free space on this device for that file.",
                ));
            }
            Ok(Plan {
                direction: ExportTransferDirection::Receive,
                path,
                expected: Some(FileInfo {
                    name: name.clone(),
                    bytes,
                    sha256: sha256.clone(),
                }),
                computer,
            })
        }
        ExportTransferDirection::Send => {
            if params.name.is_some() || params.bytes.is_some() || params.sha256.is_some() {
                return Err(invalid("sending takes only a path."));
            }
            let metadata = std::fs::metadata(&path).map_err(|_| {
                DispatchError::internal(format!("{} is not a file.", path.display()))
            })?;
            if !metadata.is_file() {
                return Err(DispatchError::internal(format!(
                    "{} is not a file.",
                    path.display()
                )));
            }
            if metadata.len() == 0 {
                return Err(DispatchError::internal("That file is empty."));
            }
            if metadata.len() > transfer_area::MAX_BYTES {
                return Err(DispatchError::internal(
                    "That file is larger than Silo transfers between machines.",
                ));
            }
            Ok(Plan {
                direction: ExportTransferDirection::Send,
                path,
                expected: None,
                computer,
            })
        }
    }
}

/// `exports.transfer`: starts the copy and returns its job.
pub(super) async fn start(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ExportTransferParams = context.decode_params(params)?;
    let device = super::super::guest_remote::remote_device(context, Some(&params.device_id))
        .ok_or_else(|| invalid("deviceId must name another device."))?;
    let plan = plan(&params)?;
    {
        let device = device.clone();
        blocking(move || require_file_sharing(&device)).await?;
    }
    // The `transfer` space is the one a transfer's cancel route belongs to.
    let started = context.start_job_with_id(
        "exports.transfer",
        JobId::new(format!("transfer:{}", uuid::Uuid::new_v4()))
            .map_err(|error| DispatchError::internal(error.to_string()))?,
    )?;
    let job_id = started.job_id.clone();
    let registry = context.job_registry()?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let cancel = cancel.clone();
        registry.register_cancel(job_id.clone(), CancelRoute::Transfer, move || {
            cancel.store(true, Ordering::Release);
            Ok(())
        });
    }
    registry
        .mark_running(&job_id)
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    let engine = context.silod_context().clone();
    let running = job_id.clone();
    let spawned = std::thread::Builder::new()
        .name("silod-export-transfer".into())
        .spawn(move || {
            let outcome = run(&engine, &running, &device, &plan, &cancel);
            finish(&engine, running, outcome, &cancel);
        });
    if let Err(error) = spawned {
        let _ = silo_engine::jobs::finish_with_error(
            context.silod_context(),
            job_id,
            ErrorCode::Internal,
            format!("Could not start the transfer: {error}"),
            Audience::All,
        );
        return Err(DispatchError::internal(format!(
            "Could not start the transfer: {error}"
        )));
    }
    output(started)
}

fn finish(
    engine: &SilodContext,
    job_id: JobId,
    outcome: Result<TransferFile, String>,
    cancel: &AtomicBool,
) {
    let _ = match outcome {
        Ok(file) => silo_engine::jobs::finish_with_result(
            engine,
            job_id,
            serde_json::to_value(file).unwrap_or(Value::Null),
            Audience::All,
        ),
        Err(message) => silo_engine::jobs::finish_with_error(
            engine,
            job_id,
            if cancel.load(Ordering::Acquire) {
                ErrorCode::Cancelled
            } else {
                ErrorCode::Internal
            },
            message,
            Audience::All,
        ),
    };
}

/// Publishes the copy's progress, at most every `PROGRESS_EVERY` and always at the end.
struct Reporter<'a> {
    engine: &'a SilodContext,
    job_id: JobId,
    plan: &'a Plan,
    name: String,
    total: u64,
    last: Instant,
}

impl<'a> Reporter<'a> {
    fn new(engine: &'a SilodContext, job_id: &JobId, plan: &'a Plan, total: u64) -> Self {
        let name = plan
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        Self {
            engine,
            job_id: job_id.clone(),
            plan,
            name,
            total,
            last: Instant::now() - PROGRESS_EVERY,
        }
    }

    fn report(&mut self, done: u64, state: TransferState) {
        if state == TransferState::Transferring && self.last.elapsed() < PROGRESS_EVERY {
            return;
        }
        self.last = Instant::now();
        let receiving = self.plan.direction == ExportTransferDirection::Receive;
        let _ = silo_engine::jobs::publish_progress(
            self.engine,
            JobProgress::Transfer {
                job_id: self.job_id.clone(),
                kind: Some(if receiving {
                    OperationKind::Export
                } else {
                    OperationKind::Import
                }),
                computer_id: None,
                data: TransferProgress {
                    id: self.job_id.as_str().to_owned(),
                    computer: self.plan.computer.clone(),
                    direction: if receiving {
                        TransferDirection::Download
                    } else {
                        TransferDirection::Upload
                    },
                    state,
                    name: self.name.clone(),
                    file_index: 1,
                    file_count: 1,
                    bytes_done: done,
                    bytes_total: self.total,
                },
            },
            Audience::All,
        );
    }
}

/// Closes the stream when the job is cancelled or its time is up, which ends the blocked read.
struct Watchdog {
    finished: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<&'static str>>>,
}

impl Watchdog {
    fn start(
        guard: Arc<Mutex<Option<Box<dyn Send>>>>,
        cancel: Arc<AtomicBool>,
        limit: Duration,
    ) -> Self {
        let finished = Arc::new(AtomicBool::new(false));
        let reason = Arc::new(Mutex::new(None));
        let (done, why) = (finished.clone(), reason.clone());
        let deadline = Instant::now() + limit;
        let _ = std::thread::Builder::new()
            .name("silod-export-transfer-watchdog".into())
            .spawn(move || {
                while !done.load(Ordering::Acquire) {
                    let stop = if cancel.load(Ordering::Acquire) {
                        Some("The transfer was cancelled.")
                    } else if Instant::now() >= deadline {
                        Some("The transfer took too long and was stopped.")
                    } else {
                        None
                    };
                    if let Some(text) = stop {
                        *why.lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(text);
                        guard
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .take();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
        Self { finished, reason }
    }

    /// Why the watchdog stopped the stream, when it did.
    fn stopped(&self) -> Option<&'static str> {
        *self
            .reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn finish(&self) {
        self.finished.store(true, Ordering::Release);
    }
}

fn run(
    engine: &SilodContext,
    job_id: &JobId,
    device: &str,
    plan: &Plan,
    cancel: &Arc<AtomicBool>,
) -> Result<TransferFile, String> {
    match plan.direction {
        ExportTransferDirection::Receive => receive(engine, job_id, device, plan, cancel),
        ExportTransferDirection::Send => send(engine, job_id, device, plan, cancel),
    }
}

fn cancelled(cancel: &Arc<AtomicBool>) -> impl Fn() -> bool + '_ {
    move || cancel.load(Ordering::Acquire)
}

fn receive(
    engine: &SilodContext,
    job_id: &JobId,
    device: &str,
    plan: &Plan,
    cancel: &Arc<AtomicBool>,
) -> Result<TransferFile, String> {
    let expected = plan.expected.clone().ok_or("Missing file details.")?;
    // The copy goes to a temporary file of its own beside `path` and takes its name only when
    // the hash matched, so a failed or interrupted copy never leaves a file at the path the user
    // chose. An existing file is refused now, not after hours of copying.
    let part = part_path(&plan.path);
    if std::fs::symlink_metadata(&plan.path).is_ok() {
        return Err(format!(
            "Silo could not create {}: a file already exists there.",
            plan.path.display()
        ));
    }
    let mut file = create_part(&part)?;
    let mut reporter = Reporter::new(engine, job_id, plan, expected.bytes);
    let result = (|| {
        let opened = link().open(
            device,
            json!({"mode": "read", "name": expected.name}),
            &cancelled(cancel),
        )?;
        let Opened {
            input,
            mut output,
            guard,
        } = opened;
        // The input stays open until the copy ends; closing it would end the owner's stream.
        let _input = input;
        let guard = Arc::new(Mutex::new(Some(guard)));
        let watchdog = Watchdog::start(
            guard.clone(),
            cancel.clone(),
            transfer_area::time_budget(expected.bytes),
        );
        let copied = transfer_area::pull(
            &mut output,
            &mut file,
            &expected,
            &mut |done| reporter.report(done, TransferState::Transferring),
            &cancelled(cancel),
        );
        watchdog.finish();
        let copied = match (copied, watchdog.stopped()) {
            (Err(_), Some(reason)) => Err(reason.to_owned()),
            (copied, _) => copied,
        };
        guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        copied
    })();
    drop(file);
    // Publishing never replaces: something that appeared at the path meanwhile is kept.
    let result = result.and_then(|()| publish(&part, &plan.path));
    match result {
        Ok(()) => {
            remove_partial(&part);
            reporter.report(expected.bytes, TransferState::Done);
            Ok(TransferFile {
                name: expected.name,
                bytes: expected.bytes,
                sha256: expected.sha256,
            })
        }
        Err(message) => {
            remove_partial(&part);
            reporter.report(
                0,
                if cancel.load(Ordering::Acquire) {
                    TransferState::Cancelled
                } else {
                    TransferState::Failed
                },
            );
            Err(message)
        }
    }
}

/// The temporary file of one copy to `path`: `<path>.<random>.part`, in the same folder so it
/// can take the final name with a rename. Every copy gets its own, so two copies to one path
/// never write to, remove or publish each other's file.
fn part_path(path: &Path) -> PathBuf {
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let mut part = path.as_os_str().to_owned();
    part.push(format!(".{}.part", &unique[..16]));
    PathBuf::from(part)
}

/// Creates `part` for writing. Nothing that already exists there is opened or removed.
fn create_part(part: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(part)
        .map_err(|error| format!("Silo could not create {}: {error}", part.display()))
}

/// Moves the finished copy to the path the user chose without replacing anything there.
fn publish(part: &Path, path: &Path) -> Result<(), String> {
    transfer_area::publish_without_replacing(part, path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            format!(
                "Silo could not create {}: a file already exists there.",
                path.display()
            )
        } else {
            format!("Silo could not create {}: {error}", path.display())
        }
    })
}

fn remove_partial(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn send(
    engine: &SilodContext,
    job_id: &JobId,
    device: &str,
    plan: &Plan,
    cancel: &Arc<AtomicBool>,
) -> Result<TransferFile, String> {
    let (bytes, sha256) = transfer_area::digest(&plan.path, &cancelled(cancel))
        .map_err(|error| format!("Silo could not read {}: {error}", plan.path.display()))?;
    if bytes == 0 || bytes > transfer_area::MAX_BYTES {
        return Err("That file is outside what Silo transfers between machines.".into());
    }
    let mut source = File::open(&plan.path)
        .map_err(|error| format!("Silo could not read {}: {error}", plan.path.display()))?;
    let mut reporter = Reporter::new(engine, job_id, plan, bytes);
    let result = (|| {
        let opened = link().open(
            device,
            json!({"mode": "write", "bytes": bytes, "sha256": sha256}),
            &cancelled(cancel),
        )?;
        let Opened {
            mut input,
            mut output,
            guard,
        } = opened;
        let guard = Arc::new(Mutex::new(Some(guard)));
        let watchdog = Watchdog::start(
            guard.clone(),
            cancel.clone(),
            transfer_area::time_budget(bytes),
        );
        let sent = transfer_area::push(
            &mut input,
            &mut output,
            &mut source,
            bytes,
            &mut |done| reporter.report(done, TransferState::Transferring),
            &cancelled(cancel),
        );
        watchdog.finish();
        let sent = match (sent, watchdog.stopped()) {
            (Err(_), Some(reason)) => Err(reason.to_owned()),
            (sent, _) => sent,
        };
        guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        sent
    })();
    match result {
        Ok(name) => {
            reporter.report(bytes, TransferState::Done);
            Ok(TransferFile {
                name,
                bytes,
                sha256,
            })
        }
        Err(message) => {
            reporter.report(
                0,
                if cancel.load(Ordering::Acquire) {
                    TransferState::Cancelled
                } else {
                    TransferState::Failed
                },
            );
            Err(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_copy_to_a_path_writes_its_own_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.silo-backup");
        let (first, second) = (part_path(&path), part_path(&path));
        assert_ne!(first, second);
        for part in [&first, &second] {
            assert_eq!(part.parent(), path.parent());
            let name = part.file_name().unwrap().to_str().unwrap();
            assert!(name.starts_with("x.silo-backup.") && name.ends_with(".part"));
        }

        // The second copy starts while the first is unfinished, then the first finishes.
        let mut one = create_part(&first).unwrap();
        let mut two = create_part(&second).unwrap();
        one.write_all(b"first, complete").unwrap();
        two.write_all(b"second, unfin").unwrap();
        drop(one);
        publish(&first, &path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first, complete");

        // The second one cannot take the name and removes only its own file.
        drop(two);
        assert!(publish(&second, &path)
            .unwrap_err()
            .contains("already exists"));
        remove_partial(&second);
        assert_eq!(std::fs::read(&path).unwrap(), b"first, complete");
        assert!(!first.exists() && !second.exists());
    }

    #[test]
    fn a_temporary_file_that_exists_is_left_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("x.silo-backup.0123456789abcdef.part");
        std::fs::write(&part, b"another copy's").unwrap();
        let message = create_part(&part).unwrap_err();
        assert!(message.contains("x.silo-backup.0123456789abcdef.part"));
        assert_eq!(std::fs::read(&part).unwrap(), b"another copy's");
    }

    #[test]
    fn publishing_keeps_a_file_that_appeared_at_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("a.part");
        let path = dir.path().join("a");
        std::fs::write(&part, b"copy").unwrap();
        publish(&part, &path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"copy");
        assert!(!part.exists());

        std::fs::write(&part, b"second").unwrap();
        let message = publish(&part, &path).unwrap_err();
        assert!(message.contains("already exists"));
        assert_eq!(std::fs::read(&path).unwrap(), b"copy");
    }
}
