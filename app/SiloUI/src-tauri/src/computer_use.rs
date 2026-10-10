//! Built-in computer use for computers created from a v4 or later guest image.
//!
//! Every such computer mounts the device's published ChatGPT app folder read-only at
//! `/opt/silo/chatgpt` (see `chatgpt_app`). After each boot, and whenever the app
//! becomes ready, Silo pushes `guest/silo-computer-use.py` and the pinned
//! app/LCU pair into the guest and runs its `apply`, which installs LCU against the
//! mounted app and runs `lcu setup` with the computer's approval mode. The host drives the
//! guest toward the mode the user chose and remembers how each attempt ended; the guest
//! is a plain executor. See
//! `docs/SiloUI-CHATGPT-APP.md` and `docs/SiloUI-COMPUTER-USE-PLAN.md`.
use crate::{
    chatgpt_app::{self, DebArch, Status},
    desktop,
    runtime::{self, ComputerConfiguration, RuntimeError, RuntimePaths, RuntimeRunner},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter};

/// Where the guest sees the device's published ChatGPT app folder.
pub(crate) const GUEST_MOUNT: &str = "/opt/silo/chatgpt";
/// Where the guest finds the host's verified LCU archive, read-only.
pub(crate) const LCU_GUEST_MOUNT: &str = "/opt/silo/lcu";
const HELPER: &str = include_str!("../guest/silo-computer-use.py");
const LCU_LOCK: &str = include_str!("../guest/lcu-lock.json");
const GUEST_HELPER: &str = "/usr/local/libexec/silo-computer-use";
/// The most one run of the guest helper may take (install, setup and the readiness check
/// with its bounded wait for the desktop session), and the host's allowance on top of it
/// for starting the command and collecting its output.
const APPLY_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const APPLY_GRACE: Duration = Duration::from_secs(60);

// ------------------------------------------------------------- approval

/// Whether an agent's computer-use actions in a computer ask for approval first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Approval {
    #[default]
    Ask,
    Auto,
}

impl Approval {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Auto => "auto",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "ask" => Some(Self::Ask),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// What the guest last reported while the computer ran, shown while it is stopped.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Known {
    state: String,
    #[serde(default)]
    compatibility: Option<String>,
    #[serde(default)]
    warning: Option<String>,
    #[serde(default)]
    app_version: Option<String>,
    #[serde(default)]
    runtime_version: Option<String>,
    #[serde(default)]
    lcu_version: Option<String>,
    #[serde(default)]
    agents: Option<Vec<String>>,
}

/// How one run of the guest helper ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Outcome {
    Applied,
    Failed,
    /// `lcu setup` configured some agents and failed for others.
    Partial,
}

impl Outcome {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "applied" => Some(Self::Applied),
            "failed" => Some(Self::Failed),
            "partial" => Some(Self::Partial),
            _ => None,
        }
    }
}

/// The last attempt to apply an approval mode in the guest: what was tried and how it
/// ended. Kept until a later attempt replaces it; nothing is assumed rolled back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Attempt {
    pub(crate) mode: Approval,
    #[serde(deserialize_with = "saved_outcome")]
    pub(crate) outcome: Outcome,
    /// Seconds since the Unix epoch.
    pub(crate) at: u64,
    /// A stable code (see `reason_text`); only for a failure or a partial application.
    #[serde(default)]
    pub(crate) reason: Option<String>,
}

fn saved_outcome<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Outcome, D::Error> {
    let value = String::deserialize(deserializer)?;
    // A newer outcome cannot prove success, but must not discard the user's choice.
    Ok(Outcome::parse(&value).unwrap_or(Outcome::Failed))
}

/// The computer's approval policy, kept in `<storage>/computer-use/<id>.json`: the mode the user
/// chose (`approval`, the desired one), the last mode that was applied completely
/// (`applied`) and the last attempt. Only a user change, a fork or an apply writes it, all
/// under one lock, so a status read never changes it. Files of older versions carry a
/// revision and a generation; they are ignored.
///
/// The switch is a convenience, not a security boundary: agents in the computer have root and
/// can edit their own harness settings. The host therefore only drives the guest toward
/// the chosen mode and remembers how that went.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Policy {
    #[serde(default)]
    pub(crate) approval: Approval,
    #[serde(default)]
    pub(crate) applied: Option<Approval>,
    #[serde(default)]
    pub(crate) last: Option<Attempt>,
    /// The mode of an attempt that started and has no result yet. Written before the
    /// helper runs and replaced by the result, so an attempt cut short by a crash or a
    /// power loss is still known afterwards: whatever `last` says, the guest may have
    /// been left half changed.
    #[serde(default)]
    pub(crate) unfinished: Option<Approval>,
}

impl Policy {
    /// Whether the guest must be driven toward the chosen mode: an attempt never ended, no
    /// attempt yet, the last one was for another mode, or it did not apply completely.
    pub(crate) fn needs_apply(&self) -> bool {
        self.unfinished.is_some()
            || self
                .last
                .as_ref()
                .is_none_or(|last| last.mode != self.approval || last.outcome != Outcome::Applied)
    }
}

/// Per-computer computer-use settings as read: the policy plus the last observation.
/// Removed with the computer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Settings {
    pub(crate) approval: Approval,
    pub(crate) applied: Option<Approval>,
    pub(crate) last: Option<Attempt>,
    /// An attempt started and never recorded a result (see `Policy::unfinished`).
    pub(crate) unfinished: bool,
    pub(crate) known: Option<Known>,
    /// The policy file exists but cannot be read, so `approval` is only the fail-closed
    /// default and the user's choice is unknown.
    pub(crate) unreadable: bool,
}

/// Serializes every read-modify-write of a policy file.
static POLICY_LOCK: Mutex<()> = Mutex::new(());
const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;

/// The approval mode a computer created or imported on this device starts with: the
/// `computerUseAutoApproval` app setting, kept here so creation and import need no app handle.
static NEW_COMPUTER_AUTO: AtomicBool = AtomicBool::new(false);

/// The initial mode named by the app settings; anything but `true` means ask.
pub(crate) fn initial_approval_from(settings: &serde_json::Map<String, Value>) -> Approval {
    match settings.get("computerUseAutoApproval") {
        Some(Value::Bool(true)) => Approval::Auto,
        _ => Approval::Ask,
    }
}

/// Makes `settings` the source of the initial mode of computers created or imported from now on.
pub(crate) fn sync_initial_approval(settings: &serde_json::Map<String, Value>) {
    NEW_COMPUTER_AUTO.store(
        initial_approval_from(settings) == Approval::Auto,
        Ordering::SeqCst,
    );
}

#[cfg(test)]
thread_local! {
    static TEST_INITIAL: std::cell::Cell<Option<Approval>> = const { std::cell::Cell::new(None) };
}

/// Runs `body` with this thread seeing `mode` as the initial mode, whatever other tests do.
#[cfg(test)]
pub(crate) fn with_initial_approval<T>(mode: Approval, body: impl FnOnce() -> T) -> T {
    TEST_INITIAL.with(|cell| cell.set(Some(mode)));
    let result = body();
    TEST_INITIAL.with(|cell| cell.set(None));
    result
}

pub(crate) fn initial_approval() -> Approval {
    #[cfg(test)]
    if let Some(mode) = TEST_INITIAL.with(std::cell::Cell::get) {
        return mode;
    }
    if NEW_COMPUTER_AUTO.load(Ordering::SeqCst) {
        Approval::Auto
    } else {
        Approval::Ask
    }
}

/// Gives a new or imported computer its starting mode and nothing else, so its first boot
/// applies it. Only the local setting decides it: an archive never carries a mode.
pub(crate) fn start_with(paths: &RuntimePaths, id: &str, approval: Approval) {
    if approval == Approval::Ask {
        return;
    }
    let _lock = lock_policies();
    let _ = write_atomic(
        paths,
        policy_path(paths, id),
        &Policy {
            approval,
            ..Policy::default()
        },
    );
}

fn directory(paths: &RuntimePaths) -> PathBuf {
    paths.metadata.with_file_name("computer-use")
}

fn policy_path(paths: &RuntimePaths, id: &str) -> Option<PathBuf> {
    // Ids are UUIDs; anything else never names a file.
    uuid::Uuid::parse_str(id)
        .ok()
        .map(|id| directory(paths).join(format!("{id}.json")))
}

fn observed_path(paths: &RuntimePaths, id: &str) -> Option<PathBuf> {
    policy_path(paths, id).map(|path| path.with_extension("observed.json"))
}

fn read_settings_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Computer-use settings must be a regular file.",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_SETTINGS_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SETTINGS_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Computer-use settings exceed the 1 MiB safety limit.",
        ));
    }
    Ok(bytes)
}

fn read_json<T: serde::de::DeserializeOwned>(path: Option<PathBuf>) -> Option<T> {
    path.and_then(|path| read_settings_bytes(&path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

fn read_policy(paths: &RuntimePaths, id: &str) -> Policy {
    read_policy_checked(paths, id).unwrap_or_default()
}

/// The stored policy: the default for a computer that has none yet, `None` when a file exists
/// but cannot be read or parsed (the user's choice is then unknown, not `ask`).
fn read_policy_checked(paths: &RuntimePaths, id: &str) -> Option<Policy> {
    let Some(path) = policy_path(paths, id) else {
        return Some(Policy::default());
    };
    match read_settings_bytes(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Policy::default()),
        Err(_) => None,
    }
}

/// The settings of computer `id`; a missing or unreadable file means the defaults (ask).
pub(crate) fn settings(paths: &RuntimePaths, id: &str) -> Settings {
    let checked = read_policy_checked(paths, id);
    let unreadable = checked.is_none();
    let policy = checked.unwrap_or_default();
    Settings {
        approval: policy.approval,
        applied: policy.applied,
        last: policy.last,
        unfinished: policy.unfinished.is_some(),
        known: read_json(observed_path(paths, id)),
        unreadable,
    }
}

fn write_atomic<T: Serialize>(
    paths: &RuntimePaths,
    path: Option<PathBuf>,
    value: &T,
) -> Result<(), RuntimeError> {
    let fail = || RuntimeError::Unavailable("Silo could not save the computer-use setting.".into());
    let path =
        path.ok_or_else(|| RuntimeError::Invalid("Silo could not identify this computer.".into()))?;
    let directory = directory(paths);
    runtime::prepare_private_directory(&directory).map_err(|_| fail())?;
    let bytes = serde_json::to_vec(value).map_err(|_| fail())?;
    let mut temporary = tempfile::NamedTempFile::new_in(&directory).map_err(|_| fail())?;
    std::io::Write::write_all(&mut temporary, &bytes).map_err(|_| fail())?;
    temporary.as_file().sync_all().map_err(|_| fail())?;
    temporary.persist(path).map_err(|_| fail())?;
    Ok(())
}

fn lock_policies() -> std::sync::MutexGuard<'static, ()> {
    POLICY_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Stores a new approval choice and returns the stored policy. The last attempt is kept:
/// it is compared with the choice, never rewritten by it.
pub(crate) fn set_approval(
    paths: &RuntimePaths,
    id: &str,
    approval: Approval,
) -> Result<Policy, RuntimeError> {
    let _lock = lock_policies();
    // An unreadable file is replaced: the user's explicit choice repairs it.
    let mut policy = read_policy(paths, id);
    policy.approval = approval;
    write_atomic(paths, policy_path(paths, id), &policy)?;
    Ok(policy)
}

/// Remembers how an attempt to apply a mode ended. Touches only the attempt (and the
/// last applied mode), never the user's choice, and writes nothing when the policy file
/// cannot be read (the choice would be lost).
fn record_attempt(paths: &RuntimePaths, id: &str, attempt: Attempt) {
    let _lock = lock_policies();
    let Some(mut policy) = read_policy_checked(paths, id) else {
        return;
    };
    if attempt.outcome == Outcome::Applied {
        policy.applied = Some(attempt.mode);
    }
    policy.last = Some(attempt);
    policy.unfinished = None;
    let _ = write_atomic(paths, policy_path(paths, id), &policy);
}

/// Marks an attempt for `mode` as started and returns the marker it replaced, so a run
/// that turns out not to be an attempt can put it back (`restore_unfinished`). The marker is
/// what makes an interrupted attempt visible after a crash, so an attempt that cannot save it
/// (full disk, permissions, an unreadable policy) does not start.
fn begin_attempt(
    paths: &RuntimePaths,
    id: &str,
    mode: Approval,
) -> Result<Option<Approval>, RuntimeError> {
    let _lock = lock_policies();
    let mut policy = read_policy_checked(paths, id).ok_or_else(|| {
        RuntimeError::Unavailable("Silo could not read the computer-use setting.".into())
    })?;
    let previous = policy.unfinished.replace(mode);
    write_atomic(paths, policy_path(paths, id), &policy)?;
    Ok(previous)
}

fn restore_unfinished(paths: &RuntimePaths, id: &str, previous: Option<Approval>) {
    let _lock = lock_policies();
    let Some(mut policy) = read_policy_checked(paths, id) else {
        return;
    };
    policy.unfinished = previous;
    let _ = write_atomic(paths, policy_path(paths, id), &policy);
}

/// The policy an apply works from. A policy file that cannot be read is replaced by the
/// default (ask) first: the user's choice is already lost, and failing closed keeps the
/// guest from running with a mode nobody chose.
fn policy_for_apply(paths: &RuntimePaths, id: &str) -> Policy {
    let _lock = lock_policies();
    match read_policy_checked(paths, id) {
        Some(policy) => policy,
        None => {
            let policy = Policy::default();
            let _ = write_atomic(paths, policy_path(paths, id), &policy);
            policy
        }
    }
}

/// A fork starts with its source's approval mode and nothing else: its guest disk
/// carries the source's configuration, so no attempt is known and its first boot applies.
pub(crate) fn inherit_settings(
    paths: &RuntimePaths,
    from: &str,
    to: &str,
) -> Result<(), RuntimeError> {
    let _lock = lock_policies();
    let approval = read_policy(paths, from).approval;
    write_atomic(
        paths,
        policy_path(paths, to),
        &Policy {
            approval,
            ..Policy::default()
        },
    )
}

/// Removes the settings of a deleted computer, or of an imported one: an import or transfer
/// starts from the destination's initial mode (`start_with`) with no attempt known, so its
/// first boot applies it over whatever configuration the imported disk carries.
pub(crate) fn forget(paths: &RuntimePaths, id: &str) -> Result<(), RuntimeError> {
    cancel_retry(id);
    let _lock = lock_policies();
    let mut failure = None;
    for path in [policy_path(paths, id), observed_path(paths, id)]
        .into_iter()
        .flatten()
    {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                failure = Some(RuntimeError::Unavailable(
                    "Silo could not remove the computer-use settings.".into(),
                ));
            }
        }
    }
    failure.map_or(Ok(()), Err)
}

/// Records what the guest last reported. Touches only the observation file.
fn remember(paths: &RuntimePaths, id: &str, known: Known) {
    if read_json::<Known>(observed_path(paths, id)).as_ref() != Some(&known) {
        let _ = write_atomic(paths, observed_path(paths, id), &known);
    }
}

/// Computer id -> the number of applies scheduled or running for it, so the state says
/// `pending` while one is on its way.
static PENDING: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

/// Held by an apply from its scheduling until it ends, whatever the outcome.
struct Pending(String);

impl Pending {
    fn begin(id: &str) -> Self {
        *PENDING
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(id.to_owned())
            .or_default() += 1;
        Self(id.to_owned())
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        let mut pending = PENDING.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(count) = pending.get_mut(&self.0) {
            *count -= 1;
            if *count == 0 {
                pending.remove(&self.0);
            }
        }
    }
}

fn is_pending(id: &str) -> bool {
    PENDING
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains_key(id)
}

/// The helper's failure code for a download that did not work out; the one failure the
/// host retries by itself.
const RETRYABLE_REASON: &str = "lcu-archive-unavailable";

/// Waits before the automatic retries of a sync that failed because the computer's network
/// was unavailable (the helper has already retried the download for about two minutes).
/// After the last one the failure stays until the next boot or a manual setup.
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(15 * 60),
];

/// Computer id -> the cancel token of the retry waiting for it, so the state says `preparing`
/// while one is scheduled.
static RETRIES: Mutex<BTreeMap<String, Arc<std::sync::atomic::AtomicBool>>> =
    Mutex::new(BTreeMap::new());

/// A scheduled retry; dropping it unschedules it (unless another already replaced it).
struct RetryGuard {
    id: String,
    token: Arc<std::sync::atomic::AtomicBool>,
}

impl RetryGuard {
    fn begin(id: &str) -> Self {
        let token = Arc::new(std::sync::atomic::AtomicBool::new(false));
        RETRIES
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.to_owned(), token.clone());
        Self {
            id: id.to_owned(),
            token,
        }
    }
}

impl Drop for RetryGuard {
    fn drop(&mut self) {
        let mut retries = RETRIES.lock().unwrap_or_else(|p| p.into_inner());
        if retries
            .get(&self.id)
            .is_some_and(|token| Arc::ptr_eq(token, &self.token))
        {
            retries.remove(&self.id);
        }
    }
}

/// Cancels the computer's scheduled retry, if any: a new apply, a manual setup or a deletion
/// takes over.
fn cancel_retry(id: &str) {
    if let Some(token) = RETRIES.lock().unwrap_or_else(|p| p.into_inner()).remove(id) {
        token.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn retry_scheduled(id: &str) -> bool {
    RETRIES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains_key(id)
}

/// Sleeps for `delay` in short steps; false when `token` was set meanwhile.
fn wait_unless_cancelled(delay: Duration, token: &std::sync::atomic::AtomicBool) -> bool {
    let end = std::time::Instant::now() + delay;
    while std::time::Instant::now() < end {
        if token.load(std::sync::atomic::Ordering::SeqCst) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20).min(delay));
    }
    !token.load(std::sync::atomic::Ordering::SeqCst)
}

// ---------------------------------------------------------------- mount

static PUBLISHED: Mutex<Option<PathBuf>> = Mutex::new(None);
/// The ChatGPT storage root, remembered so the published folder can be prepared again
/// whenever it is missing (see `register_published`).
static STORAGE_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);
#[cfg(test)]
thread_local! {
    static TEST_PUBLISHED: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Records the canonical published folder computers mount (see `install`).
fn set_published_dir(dir: PathBuf) {
    *PUBLISHED.lock().unwrap_or_else(|p| p.into_inner()) = Some(dir);
}

/// Prepares the published folder under `root` and registers its canonical path for
/// `mount_args`. Safe to repeat: the app start does it, every preparation attempt does it
/// again, and `mount_args` does it when no folder is registered, so a start-up that
/// failed (disk space, permissions) never leaves new computers without the mount for the rest of
/// the session once the cause is gone.
pub(crate) fn register_published(root: &Path) -> Result<PathBuf, chatgpt_app::Error> {
    *STORAGE_ROOT.lock().unwrap_or_else(|p| p.into_inner()) = Some(root.to_path_buf());
    let dir = chatgpt_app::ensure_published_dir(root)?;
    set_published_dir(dir.clone());
    Ok(dir)
}

/// Registers the folder again from the remembered root without waiting for a download
/// that holds the storage lock (this runs while a computer is being created).
fn register_published_now() -> Option<PathBuf> {
    let root = STORAGE_ROOT
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()?;
    let dir = chatgpt_app::ensure_published_dir_nowait(&root).ok()?;
    set_published_dir(dir.clone());
    Some(dir)
}

#[cfg(test)]
fn reset_published_for_test() {
    *PUBLISHED.lock().unwrap_or_else(|p| p.into_inner()) = None;
    *STORAGE_ROOT.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

/// The canonical `<app data>/chatgpt/published` folder, once Silo prepared it.
pub(crate) fn published_dir() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(dir) = TEST_PUBLISHED.with(|dir| dir.borrow().clone()) {
        return Some(dir);
    }
    PUBLISHED.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

#[cfg(test)]
pub(crate) fn set_test_published_dir(dir: Option<PathBuf>) {
    TEST_PUBLISHED.with(|slot| *slot.borrow_mut() = dir);
}

/// Whether the computer was created with the built-in desktop (v4 image) and so with the mount.
pub(crate) fn is_built_in(configuration: &ComputerConfiguration) -> bool {
    desktop::configuration(configuration).is_some_and(|configuration| configuration.built_in)
}

/// `uid=0,gid=0` pins the guest owner of every file in the folder. Without it MicroSandbox
/// maps the host owner to the guest's default user only after the guest's agent
/// reports it, which a RAM restore (a checkpoint fork or restore) never repeats: the
/// restored guest then saw the host's own uid, and LCU refuses an app folder that root
/// does not own ("not in a location only root and this account can change").
fn mount_spec(dir: &Path) -> String {
    format!("{}:{GUEST_MOUNT}:ro,uid=0,gid=0", dir.display())
}

/// The `msb create` / `msb restore` arguments that mount the published folder: empty
/// for a computer without built-in computer use, an error when it needs the folder and
/// Silo has none (the computer would never get computer use). MicroSandbox refuses a
/// symlinked mount root, so the path is canonical.
pub(crate) fn mount_args(
    configuration: &ComputerConfiguration,
) -> Result<Vec<String>, RuntimeError> {
    mount_args_with(configuration, crate::preparation::lcu_folder())
}

/// `mount_args` with the host folder of the verified LCU archive, when Silo has one. It
/// is lent read-only beside the app folder to new computers so the guest installs LCU without
/// downloading it; a computer without it falls back to the download.
fn mount_args_with(
    configuration: &ComputerConfiguration,
    lcu: Option<PathBuf>,
) -> Result<Vec<String>, RuntimeError> {
    if !is_built_in(configuration) {
        return Ok(Vec::new());
    }
    // Only a missing or unusable folder blocks. An existing folder is mounted whatever it
    // holds: the ChatGPT app downloads in the background and a computer must not wait for it
    // (the guest reports the app as not ready until it appears).
    let unavailable = || {
        RuntimeError::Unavailable(
            "Silo could not prepare the shared ChatGPT folder for computer use. Restart Silo and try again.".into(),
        )
    };
    let dir = published_dir()
        .filter(|dir| dir.is_dir())
        .or_else(register_published_now)
        .ok_or_else(unavailable)?;
    let mut args = vec!["-v".into(), mount_spec(&dir)];
    // MicroSandbox refuses a symlinked mount root, so the folder is canonical; one that
    // vanished is not worth failing a creation for.
    if let Some(lcu) = lcu
        .and_then(|dir| dir.canonicalize().ok())
        .filter(|dir| dir.is_dir())
    {
        args.push("-v".into());
        args.push(format!(
            "{}:{LCU_GUEST_MOUNT}:ro,uid=0,gid=0",
            lcu.display()
        ));
    }
    Ok(args)
}

/// Whether a computer's inspected configuration has the read-only computer-use mount
/// that `computer` needs (always true for a computer without built-in computer use).
pub(crate) fn mount_present(config: &Value, configuration: &ComputerConfiguration) -> bool {
    if !is_built_in(configuration) {
        return true;
    }
    config
        .get("mounts")
        .and_then(Value::as_array)
        .is_some_and(|mounts| {
            mounts
                .iter()
                .any(|mount| is_computer_use_mount(mount) && read_only(mount))
        })
}

fn is_computer_use_mount(mount: &Value) -> bool {
    mount.get("type").and_then(Value::as_str) == Some("Bind")
        && mount.get("guest").and_then(Value::as_str) == Some(GUEST_MOUNT)
}

fn read_only(mount: &Value) -> bool {
    mount.pointer("/options/readonly").and_then(Value::as_bool) == Some(true)
        || mount.get("readonly").and_then(Value::as_bool) == Some(true)
}

/// Removes the computer-use mount from a configuration about to be exported. Its host
/// path means nothing on another device; the importing Silo mounts its own folder
/// because the computer settings say `builtIn`. A writable mount at that path is refused.
pub(crate) fn strip_mount_for_export(config: &mut Value) -> Result<(), String> {
    let Some(mounts) = config.get_mut("mounts").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    if mounts
        .iter()
        .any(|mount| is_computer_use_mount(mount) && !read_only(mount))
    {
        return Err(
            "The computer mounts the shared ChatGPT folder writable, so it cannot be exported."
                .into(),
        );
    }
    mounts.retain(|mount| !is_computer_use_mount(mount));
    Ok(())
}

// ----------------------------------------------------------- pinned pair

/// The tested app/LCU pair this build installs, per guest architecture.
pub(crate) fn pinned(arch: DebArch) -> Result<Value, String> {
    let app = chatgpt_app::Lock::bundled().map_err(|e| e.message)?;
    let lcu: Value = serde_json::from_str(LCU_LOCK).map_err(|_| "LCU lock is invalid.")?;
    let version = lcu["version"].as_str().ok_or("LCU lock is invalid.")?;
    if app.lcu_version.as_deref() != Some(version) {
        return Err("The ChatGPT app and LCU locks name different LCU versions.".into());
    }
    let asset = &lcu["assets"][arch.name()];
    let url = asset["url"].as_str().ok_or("LCU lock is invalid.")?;
    let sha256 = asset["sha256"].as_str().ok_or("LCU lock is invalid.")?;
    let archive = url.rsplit('/').next().unwrap_or_default();
    Ok(json!({
        "schemaVersion": 1,
        "app": {
            "dir": app.directory_name(arch),
            "version": app.version,
            "runtime": app.cua_runtime_version,
        },
        "lcu": {"version": version, "archive": archive, "url": url, "sha256": sha256},
    }))
}

/// A shell script that installs the helper and the pinned pair in the guest, then
/// runs `command` (a helper invocation).
fn guest_script(pinned: &Value, command: &str) -> String {
    format!(
        "set -eu\ncu_stage=$(mktemp -d /tmp/silo-cu.XXXXXXXX)\ntrap 'rm -rf \"$cu_stage\"' EXIT\n\
cat > \"$cu_stage/helper\" <<'SILO_CU_HELPER_EOF'\n{HELPER}\nSILO_CU_HELPER_EOF\n\
cat > \"$cu_stage/pinned.json\" <<'SILO_CU_PINNED_EOF'\n{pinned}\nSILO_CU_PINNED_EOF\n\
install -d -m 0755 /usr/local/libexec /var/lib/silo-computer-use\n\
install -m 0755 -o root -g root \"$cu_stage/helper\" {GUEST_HELPER}\n\
install -m 0644 -o root -g root \"$cu_stage/pinned.json\" /var/lib/silo-computer-use/pinned.json\n\
{command}\n"
    )
}

/// The guest command that reads computer-use status (empty object when no helper yet).
pub(crate) const STATUS_COMMAND: &str =
    "if [ -x /usr/local/libexec/silo-computer-use ]; then /usr/local/libexec/silo-computer-use status; else printf '%s\\n' '{}'; fi";

fn apply_command(mode: Approval, force: bool, boot: bool) -> String {
    let mut command = format!("{GUEST_HELPER} apply --approval {}", mode.as_str());
    if force {
        command.push_str(" --force");
    }
    if boot {
        command.push_str(" --boot");
    }
    command
}

/// What one helper run reported about the approval it was asked to apply.
#[derive(Debug, PartialEq, Eq)]
enum Report {
    /// The run reached `lcu setup` (or found it done) and ended this way.
    Done(Outcome, Option<String>),
    /// The guest cannot apply yet because the ChatGPT app is not there. Not a result:
    /// the next boot or the app becoming ready applies.
    NotReady,
}

type Run = Result<(Value, Report), RuntimeError>;

/// The pinned runtime ends an `exec` that outlives its `--timeout` with a plain failure,
/// `exec timed out after <n>s` (crates/cli/lib/commands/exec.rs, `drive_stream`), after
/// killing the guest command. That is a timeout, not an unreachable computer.
fn timeout_as_timed_out(error: RuntimeError) -> RuntimeError {
    match error {
        RuntimeError::Failed {
            operation, detail, ..
        } if detail.lines().any(|line| {
            line.trim()
                .trim_start_matches("Error:")
                .trim()
                .strip_prefix("exec timed out after ")
                .and_then(|rest| rest.strip_suffix('s'))
                .is_some_and(|secs| !secs.is_empty() && secs.bytes().all(|b| b.is_ascii_digit()))
        }) =>
        {
            RuntimeError::TimedOut { operation }
        }
        error => error,
    }
}

/// Runs the guest helper's `apply` to completion (never detached) within `APPLY_TIMEOUT`
/// and returns its status and the report of the run.
fn run_helper(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
    mode: Approval,
    force: bool,
    boot: bool,
) -> Run {
    run_helper_with(runner, paths, name, mode, force, boot, false)
}

/// `run_helper`; `allow_boot` lets the run boot a stopped computer for the call and stop it again
/// (creation, which has no running guest to apply in).
fn run_helper_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
    mode: Approval,
    force: bool,
    boot: bool,
    allow_boot: bool,
) -> Run {
    let pinned = pinned(DebArch::host().map_err(|e| RuntimeError::Unavailable(e.message))?)
        .map_err(RuntimeError::Unavailable)?;
    let output = desktop::guest_within(
        runner,
        paths,
        name,
        &guest_script(&pinned, &apply_command(mode, force, boot)),
        APPLY_TIMEOUT,
        APPLY_GRACE,
        allow_boot,
    )
    .map_err(timeout_as_timed_out)?;
    let malformed = || RuntimeError::Malformed("Computer use returned an invalid status.".into());
    let status: Value = serde_json::from_str(output.lines().last().unwrap_or("").trim())
        .map_err(|_| malformed())?;
    let report = status.get("apply").ok_or_else(malformed)?;
    let outcome = report
        .get("outcome")
        .and_then(Value::as_str)
        .and_then(Outcome::parse)
        .ok_or_else(malformed)?;
    let reason = report
        .get("reason")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let report = match (outcome, reason.as_deref()) {
        (Outcome::Failed, Some("app-missing" | "not-configured")) => Report::NotReady,
        _ => Report::Done(outcome, reason),
    };
    Ok((status, report))
}

/// The attempt to remember for a run, `None` when it was not an attempt at all.
fn attempt_of(mode: Approval, run: &Run) -> Option<Attempt> {
    let (outcome, reason) = match run {
        Ok((_, Report::NotReady)) => return None,
        Ok((_, Report::Done(outcome, reason))) => (*outcome, reason.clone()),
        Err(RuntimeError::Cancelled { .. }) => (Outcome::Failed, Some("cancelled".into())),
        Err(RuntimeError::TimedOut { .. }) => (Outcome::Failed, Some("timed-out".into())),
        Err(RuntimeError::Malformed(_)) => (Outcome::Failed, Some("invalid-report".into())),
        Err(_) => (Outcome::Failed, Some("unreachable".into())),
    };
    Some(Attempt {
        mode,
        outcome,
        at: unix_seconds(),
        reason,
    })
}

/// One attempt to apply `mode`: marks it unfinished on disk, runs the helper and replaces
/// the marker with the result. A run that was not an attempt (the app is not there yet)
/// leaves the marker as it was.
fn run_attempt(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    id: &str,
    name: &str,
    mode: Approval,
    force: bool,
    boot: bool,
) -> Run {
    run_attempt_with(runner, paths, id, name, mode, force, boot, false)
}

#[allow(clippy::too_many_arguments)]
fn run_attempt_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    id: &str,
    name: &str,
    mode: Approval,
    force: bool,
    boot: bool,
    allow_boot: bool,
) -> Run {
    let previous = match begin_attempt(paths, id, mode) {
        Ok(previous) => previous,
        Err(error) => {
            // Never run the helper without the marker: a crash would leave no trace of the
            // attempt. The failure is kept when the setting can be written at all.
            record_attempt(
                paths,
                id,
                Attempt {
                    mode,
                    outcome: Outcome::Failed,
                    at: unix_seconds(),
                    reason: Some("state-not-saved".into()),
                },
            );
            return Err(error);
        }
    };
    let run = run_helper_with(runner, paths, name, mode, force, boot, allow_boot);
    match attempt_of(mode, &run) {
        Some(attempt) => record_attempt(paths, id, attempt),
        None => restore_unfinished(paths, id, previous),
    }
    run
}

/// Sets computer use up as the last step of creating a built-in computer: one deliberate boot with
/// the desktop session up, the guest helper's apply in the computer's approval mode, and the
/// stop that ends every temporary boot. The attempt is recorded like any other, so a
/// failure is retried by the first start. `Err` is a short reason for the caller to show.
pub(crate) fn finish_in_creation(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    id: &str,
    name: &str,
) -> Result<(), String> {
    let mode = policy_for_apply(paths, id).approval;
    match run_attempt_with(runner, paths, id, name, mode, false, true, true) {
        Ok((_, Report::Done(Outcome::Applied, _))) => Ok(()),
        Ok((_, Report::Done(_, reason))) => Err(reason.unwrap_or_else(|| "not-applied".into())),
        Ok((_, Report::NotReady)) => Err("app-missing".into()),
        Err(error) => Err(error.to_string()),
    }
}

// ---------------------------------------------------------------- hooks

fn built_in_computer(paths: &RuntimePaths, name: &str) -> Option<ComputerConfiguration> {
    runtime::read_metadata(&paths.metadata)
        .ok()?
        .computers
        .into_iter()
        .find(|configuration| configuration.name() == name && is_built_in(configuration))
}

/// A runner the background apply can own.
pub(crate) type SharedRunner = Arc<dyn RuntimeRunner + Send + Sync>;

/// A running computer's identity: the runtime instance that is running now and the Silo computer id
/// its runtime computer carries. `None` unless the computer runs, is labelled with `id` and
/// the runtime names its instance: an identity that cannot be established is never trusted.
fn running_identity(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
    id: &str,
) -> Option<String> {
    // Asks the runtime directly: a computer restored from a checkpoint is still recorded as
    // pending while the restore that boots it runs `prepare_booted`, and `observe_computer`
    // would call it absent.
    let inspected = runtime::inspect_computer(runner, paths, name).ok()?;
    let labelled = inspected
        .config
        .pointer("/labels/silo.machine-id")
        .and_then(Value::as_str)
        == Some(id);
    if inspected.status != "Running" || !labelled {
        return None;
    }
    // A runtime that lacks the capability is reported by name, not skipped silently.
    runtime::running_instance_id(paths, &inspected)
        .ok()
        .flatten()
}

/// How long a queued apply waits for its turn before it gives up (the next boot or app
/// start applies again).
const GATE_WAIT: Duration = Duration::from_secs(10 * 60);

/// Label of the apply's queue entry; other work is never preempted by an identical one.
const SYNC_LABEL: &str = "Setting up computer use in";

/// Whether a queued lifecycle operation (its dedup key, `computer:<id>:<action>`) must end the helper's
/// turn. Only work that takes the computer away does: a stop or restart. A start of a computer that is
/// already running (the helper only runs in one), a dismissed error and any other action can
/// wait for the helper to finish. An operation without a key says nothing about its action, so it
/// takes the safe side and preempts.
fn lifecycle_key_preempts(key: Option<&str>, id: &str) -> bool {
    let Some(key) = key else {
        return true;
    };
    key.strip_prefix("computer:")
        .and_then(|rest| rest.strip_prefix(id))
        .and_then(|rest| rest.strip_prefix(':'))
        .is_some_and(|action| matches!(action, "stop" | "restart"))
}

/// Ends the helper's turn quickly when work that must not wait queues for it: a stop or
/// delete of the same computer (a delete is device-wide and names its targets, see
/// `OperationGate::removing`), or a device-wide shutdown (Quit, update). Sets the running
/// operation's cancel flag, which the runtime's polling loops observe by killing the
/// child. A start of the already-running computer, a dismissed error or any other queued
/// operation waits instead (`lifecycle_key_preempts`). The cut-short apply is recorded as such and tried again at the next boot or
/// app start; a stopped computer has nothing to apply.
struct Preempt {
    done: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Preempt {
    fn watch(
        gate: &'static runtime::operation_gate::OperationGate,
        id: &str,
        token: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        use runtime::operation_gate::OperationKind;
        use std::sync::atomic::Ordering;
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (flag, id) = (done.clone(), id.to_owned());
        let thread = std::thread::Builder::new()
            .name("computer-use-preempt".into())
            .spawn(move || {
                while !flag.load(Ordering::SeqCst) {
                    // A queued deletion of this computer names it only in its targets.
                    let blocked = gate.removal_queued(&id)
                        // Quit or update: device-wide, whatever computer it names.
                        || gate
                            .snapshot()
                            .waiting
                            .iter()
                            .any(|entry| entry.kind == OperationKind::Shutdown)
                        || gate
                            .waiting_lifecycle_keys(&id)
                            .iter()
                            .any(|key| lifecycle_key_preempts(key.as_deref(), &id));
                    if blocked {
                        token.store(true, Ordering::SeqCst);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            })
            .ok();
        Self { done, thread }
    }
}

impl Drop for Preempt {
    fn drop(&mut self) {
        self.done.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Why an apply runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trigger {
    /// A boot or the ChatGPT app becoming ready: always runs the helper, which installs
    /// and sets up whatever is missing and is cheap when nothing is.
    Boot,
    /// The user changed the switch, or the app started and found the last attempt
    /// incomplete: runs only while the computer's policy still needs it when its turn comes, so
    /// a queued apply that an earlier one made redundant does nothing.
    Change,
}

/// After a computer boots (start or restore): installs and configures computer use in the
/// background. Returns at once, never fails the boot, and never waits for the guest:
/// running the helper happens on a host thread (returned for tests).
pub(crate) fn after_boot(
    runner: SharedRunner,
    paths: &RuntimePaths,
    name: &str,
) -> Option<std::thread::JoinHandle<()>> {
    apply_with(&runtime::OPERATIONS, runner, paths, name, Trigger::Boot)
}

/// Drives a running built-in computer's guest toward the computer's chosen approval mode on a host
/// thread, and returns the thread (for tests); `None` unless the computer is the running,
/// labelled built-in one.
///
/// The thread takes the computer's operation turn, which is the per-computer lock that serializes
/// applies (and any other work on the computer) and keeps a stop, delete or recreate from
/// replacing the computer between the identity check and the helper. Inside the turn it
/// confirms the computer is the same recorded computer and the same running instance, then reads
/// the *current* choice and runs the helper synchronously within `APPLY_TIMEOUT`, so a
/// queued apply never writes an older choice over a newer one. The turn is cancellable:
/// it yields to a queued stop or delete of this computer and to Quit (`Preempt`). The outcome
/// is recorded in every case, and the boot that scheduled the thread never waits for it.
fn apply_with(
    gate: &'static runtime::operation_gate::OperationGate,
    runner: SharedRunner,
    paths: &RuntimePaths,
    name: &str,
    trigger: Trigger,
) -> Option<std::thread::JoinHandle<()>> {
    apply_with_delays(gate, runner, paths, name, trigger, &RETRY_DELAYS)
}

/// `apply_with` with the waits before each automatic retry (see `RETRY_DELAYS`).
fn apply_with_delays(
    gate: &'static runtime::operation_gate::OperationGate,
    runner: SharedRunner,
    paths: &RuntimePaths,
    name: &str,
    trigger: Trigger,
    delays: &'static [Duration],
) -> Option<std::thread::JoinHandle<()>> {
    let configuration = built_in_computer(paths, name)?;
    let id = configuration.id().to_owned();
    let instance = running_identity(runner.as_ref(), paths, name, &id)?;
    // A new apply (a boot, a change of the switch) takes over from a retry still waiting.
    cancel_retry(&id);
    // Registered before the thread starts, so a state read right after a change says
    // `pending` (and a boot that has nothing to change is not shown as applying).
    let pending = read_policy(paths, &id)
        .needs_apply()
        .then(|| Pending::begin(&id));
    let (paths, name) = (paths.clone(), name.to_owned());
    std::thread::Builder::new()
        .name("computer-use-apply".into())
        .spawn(move || {
            let _pending = pending;
            let mut trigger = trigger;
            let mut waited = 0;
            // Held while a retry waits, so the state says `preparing`.
            let mut _retry: Option<RetryGuard> = None;
            while apply_once(gate, &runner, &paths, &name, &id, &instance, trigger) {
                // The network was unavailable: try again after a bounded, growing wait,
                // outside the computer's turn so other work is never held up by it.
                let Some(delay) = delays.get(waited).copied() else {
                    return;
                };
                waited += 1;
                let guard = RetryGuard::begin(&id);
                let token = guard.token.clone();
                _retry = Some(guard);
                if !wait_unless_cancelled(delay, &token) {
                    return;
                }
                let same_computer = built_in_computer(&paths, &name).is_some_and(|m| m.id() == id)
                    && running_identity(runner.as_ref(), &paths, &name, &id).as_deref()
                        == Some(instance.as_str());
                if !same_computer || token.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                trigger = Trigger::Change;
            }
        })
        .ok()
}

/// One turn of an apply: waits for the computer's operation turn, then runs the helper until the
/// chosen mode is the one applied. Returns whether it ended because the guest could not
/// download LCU (a failure worth retrying later) and nothing cancelled it.
fn apply_once(
    gate: &'static runtime::operation_gate::OperationGate,
    runner: &SharedRunner,
    paths: &RuntimePaths,
    name: &str,
    id: &str,
    instance: &str,
    trigger: Trigger,
) -> bool {
    let deadline = std::time::Instant::now() + GATE_WAIT;
    // The apply is routine background work after every boot: it holds the computer's turn for
    // correctness but stays out of the queue UI. A failure surfaces in the computer panel.
    let Ok(turn) = gate
        .kind(runtime::operation_gate::OperationKind::Other)
        .hidden()
        .acquire_while(
            runtime::operation_gate::Scope::Computer { id: id.to_owned() },
            Some(name.to_owned()),
            &format!("{SYNC_LABEL} {name}"),
            &|| std::time::Instant::now() < deadline,
        )
    else {
        return false;
    };
    turn.allow_cancel();
    let _preempt = Preempt::watch(gate, id, turn.cancel_token());
    let same_computer = built_in_computer(paths, name).is_some_and(|m| m.id() == id)
        && running_identity(runner.as_ref(), paths, name, id).as_deref() == Some(instance);
    if !same_computer {
        return false;
    }
    let mut trigger = trigger;
    loop {
        let policy = policy_for_apply(paths, id);
        let mode = policy.approval;
        if trigger == Trigger::Change && !policy.needs_apply() {
            return false;
        }
        let run = run_attempt(
            runner.as_ref(),
            paths,
            id,
            name,
            mode,
            false,
            trigger == Trigger::Boot,
        );
        match &run {
            Err(RuntimeError::Cancelled { .. }) | Ok(_) => {}
            Err(error) => {
                eprintln!("Computer use could not be applied in {name}: {error}")
            }
        }
        let retryable = matches!(
            &run,
            Ok((_, Report::Done(Outcome::Failed, Some(reason)))) if reason == RETRYABLE_REASON
        );
        // A cut-short or not-yet-possible apply ends here (the next boot or the
        // app becoming ready tries again). Otherwise the user may have chosen
        // another mode while the helper ran, and the change that did it found an
        // earlier result for that mode and scheduled nothing: converge now, inside
        // the turn, on whatever is chosen at this moment.
        let cancelled = turn
            .cancel_token()
            .load(std::sync::atomic::Ordering::SeqCst);
        if matches!(
            run,
            Err(RuntimeError::Cancelled { .. }) | Ok((_, Report::NotReady))
        ) || cancelled
            || read_policy(paths, id).approval == mode
        {
            return retryable && !cancelled;
        }
        trigger = Trigger::Change;
    }
}

/// At app start, after the runtime is ready: finishes approval changes whose apply never
/// ran, failed or was cut short (the app quit, the guest failed, the computer stopped), so a
/// running guest does not keep an old mode, and brings running guests whose last reported
/// LCU is not the pinned one up to date. The host never reads the guest to decide:
/// its own record of the last attempt is enough. Returns the threads started, one per
/// Computer that needs it (for tests).
fn reconcile_in(
    gate: &'static runtime::operation_gate::OperationGate,
    runner: &SharedRunner,
    paths: &RuntimePaths,
    running: &[String],
) -> Vec<std::thread::JoinHandle<()>> {
    running
        .iter()
        .filter_map(|name| {
            let configuration = built_in_computer(paths, name)?;
            let id = configuration.id();
            // A computer that kept running through an update still has the previous LCU
            // pin and helper; a boot-style apply installs the current ones.
            if pin_is_stale(read_json::<Known>(observed_path(paths, id)).as_ref()) {
                return apply_with(gate, runner.clone(), paths, name, Trigger::Boot);
            }
            read_policy(paths, id).needs_apply().then_some(())?;
            apply_with(gate, runner.clone(), paths, name, Trigger::Change)
        })
        .collect()
}

/// Whether the guest last reported an LCU other than the one this build pins. A computer
/// that never reported one counts as stale.
fn pin_is_stale(known: Option<&Known>) -> bool {
    let pinned = serde_json::from_str::<Value>(LCU_LOCK)
        .ok()
        .and_then(|lock| lock["version"].as_str().map(str::to_owned));
    pinned.is_some() && known.and_then(|known| known.lcu_version.as_deref()) != pinned.as_deref()
}

/// `reconcile_in` for every running computer of this device. Returns at once.
pub(crate) fn reconcile(app: &AppHandle) {
    let Ok(paths) = runtime::runtime_paths(app) else {
        return;
    };
    let Ok(running) = runtime::update_recovery::running_names(app) else {
        return;
    };
    let runner: SharedRunner = Arc::new(runtime::ProcessRunner);
    let handles = reconcile_in(&runtime::OPERATIONS, &runner, &paths, &running);
    if handles.is_empty() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        for handle in handles {
            let _ = handle.join();
        }
        let _ = app.emit("silo://application-state-changed", ());
    });
}

/// The ChatGPT app became ready: running built-in computers set up computer use now instead
/// of at their next boot. Stopped computers do it when they start.
pub(crate) fn app_ready(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Ok(paths) = runtime::runtime_paths(&app) else {
            return;
        };
        let Ok(running) = runtime::update_recovery::running_names(&app) else {
            return;
        };
        for name in running {
            let _ = after_boot(Arc::new(runtime::ProcessRunner), &paths, &name);
        }
        let _ = app.emit("silo://application-state-changed", ());
    });
}

// ------------------------------------------------------- reported state

fn reason_text(code: &str) -> &'static str {
    match code {
        "interrupted" => "Setup was interrupted. Try again.",
        "doctor-failed" => "LCU's readiness check failed. Details are in /var/log/silo-computer-use.log in the computer.",
        "cross-turn-failed" => "LCU could not turn on Computer Use across turns. Silo retries at the next start.",
        "desktop-session-not-running" => "The Linux desktop was not running. Start it, then try again.",
        "timed-out" => "Setup timed out. Try again.",
        "lcu-archive-unavailable" => "Could not download LCU (network). Silo retries at the next start; check this computer's network.",
        "lcu-archive-mismatch" | "lcu-archive-invalid" => "The LCU package did not pass verification.",
        "mount-missing" => "This computer has no shared ChatGPT folder. Create a new computer to use computer use.",
        "mount-writable" => "The shared ChatGPT folder is mounted writable; Silo refuses to use it.",
        _ => "Setup failed. Details are in /var/log/silo-computer-use.log in the computer.",
    }
}

/// Why applying the approval mode failed or only partly worked, for the panel.
fn approval_reason_text(code: &str) -> &'static str {
    match code {
        "cancelled" => "Applying was interrupted. Silo tries again when the computer starts.",
        "timed-out" => "Applying took too long. Silo tries again when the computer starts.",
        "unreachable" => "Silo could not reach the computer to apply it. Silo tries again when the computer starts.",
        "state-not-saved" => "Silo could not save the computer-use setting, so it did not apply it. Free some disk space or check permissions, then try again.",
        "invalid-report" => "The computer returned an unreadable answer. Silo tries again when the computer starts.",
        "setup-partial" => "Some agents could not be configured. Details are in /var/log/silo-computer-use.log in the computer.",
        "mount-missing" | "mount-writable" => reason_text(code),
        _ => "Silo could not configure the agents' approval settings. Details are in /var/log/silo-computer-use.log in the computer.",
    }
}

fn compat(value: Option<&str>) -> &'static str {
    match value {
        Some("tested") => "tested",
        Some("untested") => "untested",
        _ => "unknown",
    }
}

/// Inputs to `computer_use_state` that do not need a guest.
pub(crate) struct Inputs<'a> {
    /// The ChatGPT app status on this device; `None` until it was first checked.
    pub(crate) app: Option<&'a Status>,
    pub(crate) computer_running: bool,
    /// The helper's `status` output, when the computer runs and reported one.
    pub(crate) guest: Option<&'a Value>,
    pub(crate) settings: &'a Settings,
    /// An apply of the chosen mode is scheduled or running.
    pub(crate) pending: bool,
    /// A retry of a failed network download is scheduled for this computer.
    pub(crate) retrying: bool,
}

/// How applying the chosen mode stands: `applied`, `pending` (scheduled, running, or
/// waiting for the computer to start), `failed` or `partial`. Judged against the *last
/// attempt*, never the guest: a failed or partial attempt stays visible until a later one
/// applies completely, and an attempt for another mode says nothing about this one.
fn approval_apply(settings: &Settings, pending: bool) -> &'static str {
    if pending {
        return "pending";
    }
    if settings.unfinished {
        return "pending";
    }
    match &settings.last {
        Some(last) if last.mode == settings.approval => match last.outcome {
            Outcome::Applied => "applied",
            Outcome::Failed => "failed",
            Outcome::Partial => "partial",
        },
        _ => "pending",
    }
}

fn state_object(
    state: &str,
    reason: Option<&str>,
    inputs: &Inputs,
    details: Option<&Known>,
) -> Value {
    let settings = inputs.settings;
    let known = details.cloned().unwrap_or_default();
    let apply = approval_apply(settings, inputs.pending);
    let apply_reason = settings
        .last
        .as_ref()
        .filter(|_| matches!(apply, "failed" | "partial"))
        .and_then(|last| last.reason.as_deref())
        .map(approval_reason_text);
    json!({
        "state": state,
        "reason": reason,
        "compatibility": compat(known.compatibility.as_deref()),
        "warning": known.warning,
        // `approval` is what the user chose (unknown when the saved choice cannot be
        // read). `appliedApproval` is the last mode that was applied completely, `unknown`
        // before any was, whatever the state of the app download or the guest. `approvalApply`
        // is how applying the chosen mode stands.
        "approval": if settings.unreadable { "unknown" } else { settings.approval.as_str() },
        "appliedApproval": settings.applied.map_or("unknown", Approval::as_str),
        "approvalApply": apply,
        "approvalApplyReason": apply_reason,
        "appVersion": known.app_version,
        "runtimeVersion": known.runtime_version,
        "lcuVersion": known.lcu_version,
        "agents": known.agents,
    })
}

/// The `computerUse` object of the desktop state, plus what to remember for later.
pub(crate) fn computer_use_state(inputs: &Inputs) -> (Value, Option<Known>) {
    let known = inputs.settings.known.as_ref();
    match inputs.app {
        None => {
            return (
                state_object(
                    "preparing",
                    Some("Checking the ChatGPT app."),
                    inputs,
                    known,
                ),
                None,
            )
        }
        Some(Status::Idle) => {
            return (
                state_object(
                    "preparing",
                    Some("Waiting to download ChatGPT for Linux."),
                    inputs,
                    known,
                ),
                None,
            )
        }
        Some(Status::Downloading { .. } | Status::Verifying | Status::Extracting) => {
            return (
                state_object(
                    "preparing",
                    Some("Preparing ChatGPT for Linux."),
                    inputs,
                    known,
                ),
                None,
            )
        }
        // Silo retries a retryable failure by itself, so the computer is still preparing.
        Some(Status::Failed {
            reason,
            retryable: true,
        }) => {
            let reason = format!("{reason} Silo tries again automatically.");
            return (
                state_object("preparing", Some(&reason), inputs, known),
                None,
            );
        }
        // The host download failed for good: setting up the guest cannot fix it.
        Some(Status::Failed { reason, .. }) => {
            let mut state = state_object("failed", Some(reason), inputs, known);
            state["cause"] = json!("app-download");
            return (state, None);
        }
        Some(Status::Ready { .. }) => {}
    }
    if !inputs.computer_running {
        return match known.filter(|known| matches!(known.state.as_str(), "ready" | "failed")) {
            Some(known) => {
                let reason = (known.state == "failed").then(|| reason_text("setup-failed"));
                (
                    state_object(&known.state, reason, inputs, Some(known)),
                    None,
                )
            }
            None => (
                state_object(
                    "unavailable",
                    Some("Start the computer to set up computer use."),
                    inputs,
                    known,
                ),
                None,
            ),
        };
    }
    let Some(guest) = inputs.guest.filter(|guest| guest.get("state").is_some()) else {
        return (
            state_object(
                "unavailable",
                Some("Computer use is not set up yet. Silo sets it up at each start."),
                inputs,
                known,
            ),
            None,
        );
    };
    let text = |field: &str| guest.get(field).and_then(Value::as_str).map(str::to_owned);
    let details = Known {
        state: text("state").unwrap_or_default(),
        compatibility: text("compatibility"),
        warning: text("warning"),
        app_version: text("appVersion"),
        runtime_version: text("runtimeVersion"),
        lcu_version: text("lcuVersion"),
        agents: guest.get("agents").and_then(Value::as_array).map(|agents| {
            agents
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        }),
    };
    let reason = text("reason");
    let (state, reason): (&str, Option<String>) = match details.state.as_str() {
        "ready" => ("ready", None),
        "installing" => ("installing", None),
        // A network failure Silo is about to retry by itself is not a failure yet.
        "failed" if inputs.retrying && reason.as_deref() == Some(RETRYABLE_REASON) => (
            "preparing",
            Some("Could not download LCU (network). Silo tries again automatically.".into()),
        ),
        "failed" => (
            "failed",
            Some(reason_text(reason.as_deref().unwrap_or("setup-failed")).into()),
        ),
        "needs-app" => (
            "preparing",
            Some("Waiting for the ChatGPT folder inside the computer.".into()),
        ),
        _ => (
            "unavailable",
            Some("Computer use is not set up yet. Silo sets it up at each start.".into()),
        ),
    };
    let remembered = matches!(state, "ready" | "failed").then(|| Known {
        state: state.into(),
        ..details.clone()
    });
    (
        state_object(state, reason.as_deref(), inputs, Some(&details)),
        remembered,
    )
}

/// The `computerUse` object for `computer`, or `None` for a computer without built-in
/// computer use. Reads only cached state; `guest` is the helper's status output.
pub(crate) fn desktop_state(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
    computer_running: bool,
    guest: Option<&Value>,
) -> Option<Value> {
    if !is_built_in(configuration) {
        return None;
    }
    let current = settings(paths, configuration.id());
    let app = chatgpt_app::cached_status();
    let (state, remembered) = computer_use_state(&Inputs {
        app: app.as_ref(),
        computer_running,
        guest,
        settings: &current,
        pending: is_pending(configuration.id()),
        retrying: retry_scheduled(configuration.id()),
    });
    if let Some(known) = remembered {
        remember(paths, configuration.id(), known);
    }
    Some(state)
}

// -------------------------------------------------------- commands

/// Runs setup (or re-runs it for agents installed later) in a running computer and returns
/// the helper's status. The caller holds the computer's operation turn (`cancel` is its token), so this cannot overlap
/// a background apply; the run applies the chosen mode and records how that went.
pub(crate) fn setup_with(
    gate: &'static runtime::operation_gate::OperationGate,
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
    force: bool,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> Result<Value, RuntimeError> {
    // Like an apply, the run yields to a queued stop or delete of this computer and to Quit.
    // A manual setup takes over from a scheduled retry.
    cancel_retry(configuration.id());
    let _preempt = Preempt::watch(gate, configuration.id(), cancel.clone());
    let policy = policy_for_apply(paths, configuration.id());
    let mut mode = policy.approval;
    let mut force = force;
    let _pending = policy
        .needs_apply()
        .then(|| Pending::begin(configuration.id()));
    loop {
        let run = run_attempt(
            runner,
            paths,
            configuration.id(),
            configuration.name(),
            mode,
            force,
            false,
        );
        // A queued follow-up can expire while manual setup holds the turn, so
        // finish applying the current choice here just as a background apply does.
        if matches!(
            &run,
            Err(RuntimeError::Cancelled { .. }) | Ok((_, Report::NotReady))
        ) || cancel.load(std::sync::atomic::Ordering::SeqCst)
            || read_policy(paths, configuration.id()).approval == mode
        {
            return run.map(|(status, _)| status);
        }
        mode = policy_for_apply(paths, configuration.id()).approval;
        force = false;
    }
}

/// Stores the computer's approval mode and, when it runs, applies it on a background thread
/// (returned for tests): the change returns at once and the state says `pending` until
/// the apply ends. A stopped computer picks it up at its next boot.
pub(crate) fn apply_approval_with(
    runner: SharedRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
    approval: Approval,
    running: bool,
) -> Result<Option<std::thread::JoinHandle<()>>, RuntimeError> {
    apply_approval_in(
        &runtime::OPERATIONS,
        runner,
        paths,
        configuration,
        approval,
        running,
    )
}

fn apply_approval_in(
    gate: &'static runtime::operation_gate::OperationGate,
    runner: SharedRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
    approval: Approval,
    running: bool,
) -> Result<Option<std::thread::JoinHandle<()>>, RuntimeError> {
    let policy = set_approval(paths, configuration.id(), approval)?;
    if !running || !policy.needs_apply() {
        return Ok(None);
    }
    Ok(apply_with(
        gate,
        runner,
        paths,
        configuration.name(),
        Trigger::Change,
    ))
}

/// Prepares the shared folder and the status cache at app start, then downloads the
/// pinned ChatGPT app in the background when it is not published yet.
pub(crate) fn install(app: &AppHandle) {
    let Ok(root) = chatgpt_app::storage_root(app) else {
        return;
    };
    // Cheap (two directories), and a computer created right after launch needs it. A failure
    // here is retried before every preparation attempt and when a computer needs the folder.
    if let Err(error) = register_published(&root) {
        eprintln!("ChatGPT app folder unavailable: {}", error.message);
    }
    // Verifying the app tree the first time reads every byte (seconds): off the main thread.
    let app = app.clone();
    std::thread::spawn(move || chatgpt_app::start_automatic(&app));
}

#[cfg(test)]
mod tests;
