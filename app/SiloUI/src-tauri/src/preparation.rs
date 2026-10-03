//! Background preparation of what each device needs before computers work well: the
//! bundled VM image imported into the runtime and the pinned LCU archive downloaded and
//! verified. Both start at launch without the device-wide operation gate. Actions that
//! need one call `ensure_image` or `ensure_lcu`, which join the work in flight or run it.
//! The ChatGPT for Linux download has its own worker (`chatgpt_app`) and status.
use crate::{
    chatgpt_app::{self, DebArch, Downloader, HttpDownloader},
    runtime::{guest_image, ProcessRunner, RuntimePaths},
};
use serde::Serialize;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Condvar, Mutex, MutexGuard, OnceLock,
    },
    time::SystemTime,
};
use tauri::{Emitter, Manager};

const STATUS_EVENT: &str = "silo://preparation-status";
const LCU_LOCK: &str = include_str!("../guest/lcu-lock.json");
const DOWNLOAD_DIR: &str = ".download";
const PUBLISH_PREFIX: &str = ".publish-";

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreparationTask {
    pub state: TaskState,
    pub fraction: Option<u8>,
    pub message: Option<String>,
    pub retryable: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    Pending,
    Running,
    Ready,
    Failed,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreparationStatus {
    pub image: PreparationTask,
    pub lcu: PreparationTask,
}

impl PreparationTask {
    const fn new(state: TaskState) -> Self {
        Self {
            state,
            fraction: None,
            message: None,
            retryable: false,
        }
    }
}

/// Why a run failed and whether trying again can help.
struct Failure {
    message: String,
    retryable: bool,
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self {
            message,
            retryable: true,
        }
    }
}

// ------------------------------------------------------------------ job

/// One preparation: at most one run at a time, shared by every caller.
struct Job {
    inner: Mutex<Inner>,
    changed: Condvar,
}

struct Inner {
    task: PreparationTask,
    running: bool,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// Ends the claim on a job even if the run panics.
struct Claim<'a> {
    job: &'a Job,
    finished: bool,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.job.finish(Err(Failure::from(
                "Preparation stopped unexpectedly. Retry.".to_owned(),
            )));
        }
    }
}

impl Job {
    const fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                task: PreparationTask::new(TaskState::Pending),
                running: false,
            }),
            changed: Condvar::new(),
        }
    }

    fn task(&self) -> PreparationTask {
        locked(&self.inner).task.clone()
    }

    fn finish(&self, result: Result<(), Failure>) {
        let mut inner = locked(&self.inner);
        inner.task = match result {
            Ok(()) => PreparationTask::new(TaskState::Ready),
            Err(failure) => PreparationTask {
                state: TaskState::Failed,
                fraction: None,
                message: Some(failure.message),
                retryable: failure.retryable,
            },
        };
        inner.running = false;
        drop(inner);
        self.changed.notify_all();
    }

    /// Returns once the job is ready: joins a run in flight (and fails with its failure),
    /// returns at once when `ready` holds, else runs `run`. `changed` is called after each
    /// change of the task and `report` receives the fraction of the work this caller waits for.
    fn ensure(
        &self,
        ready: &dyn Fn() -> bool,
        run: &dyn Fn(&dyn Fn(Option<u8>)) -> Result<(), Failure>,
        changed: &dyn Fn(),
        report: &dyn Fn(Option<u8>),
    ) -> Result<(), String> {
        let mut inner = locked(&self.inner);
        let mut joined = false;
        while inner.running {
            joined = true;
            report(inner.task.fraction);
            inner = self
                .changed
                .wait(inner)
                .unwrap_or_else(|error| error.into_inner());
        }
        if joined {
            return match inner.task.state {
                TaskState::Ready => Ok(()),
                _ => Err(inner
                    .task
                    .message
                    .clone()
                    .unwrap_or_else(|| "Preparation failed. Retry.".into())),
            };
        }
        inner.running = true;
        let was_ready = inner.task.state == TaskState::Ready;
        drop(inner);
        let mut claim = Claim {
            job: self,
            finished: false,
        };
        if ready() {
            claim.finished = true;
            self.finish(Ok(()));
            if !was_ready {
                changed();
            }
            return Ok(());
        }
        locked(&self.inner).task = PreparationTask::new(TaskState::Running);
        changed();
        report(None);
        let result = run(&|fraction| {
            let mut inner = locked(&self.inner);
            if inner.task.fraction != fraction {
                inner.task.fraction = fraction;
                drop(inner);
                self.changed.notify_all();
                changed();
            }
            report(fraction);
        });
        let outcome = result
            .as_ref()
            .map(|_| ())
            .map_err(|failure| failure.message.clone());
        claim.finished = true;
        self.finish(result);
        changed();
        outcome
    }
}

// -------------------------------------------------------------- globals

static IMAGE: Job = Job::new();
static LCU: Job = Job::new();
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
static PATHS: OnceLock<RuntimePaths> = OnceLock::new();
static LCU_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn snapshot() -> PreparationStatus {
    PreparationStatus {
        image: IMAGE.task(),
        lcu: LCU.task(),
    }
}

fn emit() {
    if let Some(app) = APP.get() {
        let _ = app.emit_to("main", STATUS_EVENT, snapshot());
    }
}

/// Called once from app setup after the runtime paths are known. Returns at once; the
/// image import and the LCU download run on their own threads.
pub fn start(app: &tauri::AppHandle, paths: RuntimePaths) {
    if APP.set(app.clone()).is_err() {
        return;
    }
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = LCU_ROOT.set(dir.join("lcu"));
    }
    let _ = PATHS.set(paths);
    kick();
}

/// Starts any preparation that is not ready; a run already in flight is joined.
fn kick() {
    let Some(paths) = PATHS.get().cloned() else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("preparation-image".into())
        .spawn(move || {
            let _ = ensure_image(&paths, &|_| {});
        });
    let _ = std::thread::Builder::new()
        .name("preparation-lcu".into())
        .spawn(|| {
            let _ = ensure_lcu(&|_| {});
        });
}

/// Returns once the VM image is imported: joins the in-flight background import, or runs it now.
/// `report` receives a 0..=100 fraction when known (None = indeterminate). Never holds the operation gate.
pub fn ensure_image(paths: &RuntimePaths, report: &dyn Fn(Option<u8>)) -> Result<(), String> {
    IMAGE.ensure(
        &|| guest_image::is_imported(paths),
        &|_| {
            guest_image::prepare(&ProcessRunner, paths)
                .map(|_| ())
                .map_err(|error| Failure::from(error.to_string()))
        },
        &emit,
        report,
    )
}

/// The read-only host folder holding the verified pinned LCU archive for this device's guest
/// architecture (file name as in guest/lcu-lock.json's URL), once ready.
pub fn lcu_folder() -> Option<PathBuf> {
    let root = LCU_ROOT.get()?;
    let spec = LcuSpec::bundled().ok()?;
    published(root, &spec)
}

/// Returns the folder once the archive is downloaded and verified (joins or starts the download).
pub fn ensure_lcu(report: &dyn Fn(Option<u8>)) -> Result<PathBuf, String> {
    let root = LCU_ROOT
        .get()
        .ok_or("Silo is still starting. Retry in a moment.")?;
    let spec = LcuSpec::bundled()?;
    LCU.ensure(
        &|| published(root, &spec).is_some(),
        &|_| download_and_publish(root, &spec, &HttpDownloader::default()),
        &emit,
        report,
    )?;
    published(root, &spec).ok_or_else(|| "The LCU download is no longer available. Retry.".into())
}

#[tauri::command]
pub(crate) fn read_preparation_status() -> PreparationStatus {
    snapshot()
}

/// Starts again whatever failed. Resolves at once; progress follows as events.
#[tauri::command]
pub(crate) fn retry_preparation() -> PreparationStatus {
    kick();
    snapshot()
}

// ------------------------------------------------------------------ LCU

struct LcuSpec {
    version: String,
    archive: String,
    url: String,
    sha256: String,
}

fn safe_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('.')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
}

impl LcuSpec {
    fn bundled() -> Result<Self, String> {
        let arch = DebArch::host().map_err(|error| error.message)?;
        Self::parse(LCU_LOCK, arch)
    }

    fn parse(json: &str, arch: DebArch) -> Result<Self, String> {
        let invalid = || "LCU lock is invalid.".to_owned();
        let lock: serde_json::Value = serde_json::from_str(json).map_err(|_| invalid())?;
        let version = lock["version"].as_str().ok_or_else(invalid)?;
        let asset = &lock["assets"][arch.name()];
        let url = asset["url"].as_str().ok_or_else(invalid)?;
        let sha256 = asset["sha256"].as_str().ok_or_else(invalid)?;
        let archive = url.rsplit('/').next().unwrap_or_default();
        if !url.starts_with("https://")
            || !safe_name(version)
            || !safe_name(archive)
            || sha256.len() != 64
            || !sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(invalid());
        }
        Ok(Self {
            version: version.into(),
            archive: archive.into(),
            url: url.into(),
            sha256: sha256.into(),
        })
    }
}

type Stamp = (PathBuf, u64, Option<SystemTime>);

/// The archive a full checksum covered in this process.
static VERIFIED: Mutex<Option<Stamp>> = Mutex::new(None);

/// The folder of the verified archive, `None` when it is missing or does not match the lock.
/// The checksum is read once per process and again when the file changes.
fn published(root: &Path, spec: &LcuSpec) -> Option<PathBuf> {
    let folder = root.join(&spec.version);
    let file = folder.join(&spec.archive);
    let meta = fs::symlink_metadata(&file).ok()?;
    if !meta.is_file() {
        return None;
    }
    let stamp = (file.clone(), meta.len(), meta.modified().ok());
    if locked(&VERIFIED).as_ref() == Some(&stamp) {
        return Some(folder);
    }
    if chatgpt_app::sha256_file(&file).ok()? != spec.sha256 {
        return None;
    }
    *locked(&VERIFIED) = Some(stamp);
    Some(folder)
}

static UNIQUE: AtomicU64 = AtomicU64::new(0);

fn remove_tree(path: &Path) {
    let _ = chatgpt_app::make_tree_deletable(path);
    let _ = fs::remove_dir_all(path);
}

fn failure(error: &chatgpt_app::Error) -> Failure {
    Failure {
        message: if error.retryable {
            "Silo could not download LCU. Check your network connection, then retry.".into()
        } else {
            "LCU is no longer available at its pinned location. Update Silo.".into()
        },
        retryable: error.retryable,
    }
}

fn storage_failure(_: std::io::Error) -> Failure {
    Failure::from("Silo could not save LCU. Check free disk space, then retry.".to_owned())
}

/// Downloads the pinned archive (resuming a partial file), verifies its checksum and publishes
/// it read-only as `<root>/<version>/<archive>`, then removes every other version.
fn download_and_publish(
    root: &Path,
    spec: &LcuSpec,
    downloader: &dyn Downloader,
) -> Result<(), Failure> {
    let downloads = root.join(DOWNLOAD_DIR);
    fs::create_dir_all(&downloads).map_err(storage_failure)?;
    let part = downloads.join(format!("{}.part", spec.archive));
    downloader
        .fetch(&spec.url, &part, 0, &mut |_| {})
        .map_err(|error| failure(&error))?;
    let matches = chatgpt_app::sha256_file(&part).is_ok_and(|hash| hash == spec.sha256);
    if !matches {
        let _ = fs::remove_file(&part);
        return Err(Failure::from(
            "The downloaded LCU file did not match its checksum and was removed. Retry.".to_owned(),
        ));
    }
    let staging = root.join(format!(
        "{PUBLISH_PREFIX}{}-{}",
        std::process::id(),
        UNIQUE.fetch_add(1, Ordering::Relaxed)
    ));
    let target = root.join(&spec.version);
    let moved = std::cell::Cell::new(false);
    let publish = || -> std::io::Result<()> {
        fs::create_dir(&staging)?;
        let file = staging.join(&spec.archive);
        fs::rename(&part, &file)?;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o444))?;
        remove_tree(&target);
        // The folder is moved into place while writable, then made read-only.
        fs::rename(&staging, &target)?;
        moved.set(true);
        fs::set_permissions(&target, fs::Permissions::from_mode(0o555))
    };
    if let Err(error) = publish() {
        remove_tree(&staging);
        if moved.get() {
            remove_tree(&target);
        }
        return Err(storage_failure(error));
    }
    collect_garbage(root, &spec.version);
    Ok(())
}

/// Removes every version but `keep` and any unfinished staging directory.
fn collect_garbage(root: &Path, keep: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name == keep || name == DOWNLOAD_DIR {
            continue;
        }
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            remove_tree(&entry.path());
        }
    }
}

#[cfg(test)]
mod tests;
