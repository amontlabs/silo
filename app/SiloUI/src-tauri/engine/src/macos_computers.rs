//! macOS computers: macOS guests on Apple silicon through Virtualization.framework,
//! shown in a native display window. They live in the Silo process, apart from the
//! Linux computers MicroSandbox runs.
//!
//! This module owns the state the UI sees and the workflows around it (creating,
//! setting up, starting, stopping, deleting, Quit). `store`, `restore_image` and
//! `templates` are plain Rust; `engine` holds every Virtualization.framework call.
//! `provision` prepares an installed computer for computer use, with `offline_setup`,
//! `guest_access`, `recovery`, `guest_computer_use` and `guest_clipboard` behind it.
//! A finished computer becomes a template (`templates`); later computers are copied
//! from it and made their own by `personalize`. `checkpoints` saves and restores a
//! computer's disk and memory and forks new computers from them.
#[path = "macos_computers/checkpoints.rs"]
mod checkpoints;
#[path = "macos_computers/developer_tools.rs"]
mod developer_tools;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "macos_computers/engine.rs"]
mod engine;
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
#[path = "macos_computers/unsupported.rs"]
mod engine;
#[path = "macos_computers/files.rs"]
pub(crate) mod files;
#[path = "macos_computers/guest_access.rs"]
mod guest_access;
#[path = "macos_computers/guest_clipboard.rs"]
mod guest_clipboard;
#[path = "macos_computers/guest_computer_use.rs"]
mod guest_computer_use;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "macos_computers/guest_screen.rs"]
mod guest_screen;
#[path = "macos_computers/input.rs"]
mod input;
#[path = "macos_computers/offline_setup.rs"]
mod offline_setup;
#[path = "macos_computers/personalize.rs"]
mod personalize;
#[path = "macos_computers/ports.rs"]
mod ports;
#[path = "macos_computers/provision.rs"]
mod provision;
#[path = "macos_computers/proxy/mod.rs"]
mod proxy;
#[path = "macos_computers/proxy_address.rs"]
mod proxy_address;
#[path = "macos_computers/proxy_ca.rs"]
mod proxy_ca;
#[path = "macos_computers/recovery.rs"]
mod recovery;
#[path = "macos_computers/remote_display.rs"]
mod remote_display;
#[path = "macos_computers/restore_image.rs"]
mod restore_image;
#[path = "macos_computers/run_marker.rs"]
mod run_marker;
#[path = "macos_computers/setup_log.rs"]
mod setup_log;
#[path = "macos_computers/ssh_alias.rs"]
mod ssh_alias;
#[path = "macos_computers/ssh_stream.rs"]
mod ssh_stream;
#[path = "macos_computers/store.rs"]
mod store;
#[path = "macos_computers/templates.rs"]
mod templates;

use crate::runtime;
pub use guest_clipboard::Direction as ClipboardDirection;
pub(crate) use remote_display::DisplaySession;
use serde::Serialize;
use sha2::{Digest, Sha256};
use silo_engine::{Audience, EngineContext, Event};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex, Once, OnceLock,
    },
    time::{Duration, Instant},
};
pub use store::{Action, CreateRequest};
use store::{DeleteMode, Layout, Record, State};

const CHANGED_EVENT: &str = "silo://macos-computers-changed";
const NOT_SUPPORTED: &str = "macOS computers need a Mac with Apple silicon.";
const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);
const WATCH_INTERVAL: Duration = Duration::from_secs(2);
static MACOS_JOBS: OnceLock<Mutex<HashMap<String, silo_api::jobs::JobId>>> = OnceLock::new();
static MACOS_CHECKPOINT_JOBS: OnceLock<Mutex<HashMap<String, silo_api::jobs::JobId>>> =
    OnceLock::new();
static MACOS_PENDING_CHECKPOINT_JOBS: OnceLock<
    Mutex<HashMap<silo_api::jobs::JobId, (String, silo_api::jobs::JobFinished)>>,
> = OnceLock::new();
static MACOS_PENDING_FORK_WORKFLOWS: OnceLock<
    Mutex<HashMap<silo_api::jobs::JobId, ForkContinuation>>,
> = OnceLock::new();
static MACOS_CHECKPOINT_RETRY_WORKER: Once = Once::new();
static MACOS_JOB_EVENT_PUBLISH_LOCK: Mutex<()> = Mutex::new(());
/// How long a guest may ignore a stop request before the computer counts as running again.
const STOP_IGNORED_AFTER: Duration = Duration::from_secs(20);
const GRACEFUL_QUIT: Duration = Duration::from_secs(60);
/// Quit waits less for a guest's SSH shutdown than a user-initiated Stop does.
const QUIT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);
/// Less than this is not worth an SSH attempt.
const MIN_QUIT_SHUTDOWN: Duration = Duration::from_secs(2);
const FORCED_QUIT: Duration = Duration::from_secs(8);

/// Host services that remain outside the macOS computer engine.
pub trait MacosHost: Send + Sync {
    fn show_display(
        &self,
        context: &MacosContext,
        id: &str,
        title: &str,
        toolbar: bool,
    ) -> Result<(), String>;
    fn close_display(&self, id: &str);
    fn linux_computer_names(&self) -> Vec<String>;
}

/// Context for macOS computer work: engine capabilities plus its host's display-window adapter.
#[derive(Clone)]
pub struct MacosContext {
    engine: Arc<dyn EngineContext>,
    host: Arc<dyn MacosHost>,
}

impl MacosContext {
    pub fn new(engine: Arc<dyn EngineContext>, host: Arc<dyn MacosHost>) -> Self {
        Self { engine, host }
    }

    fn paths(&self) -> &silo_engine::EnginePaths {
        self.engine.paths()
    }

    fn publish(&self, event: Event, audience: Audience) {
        self.engine.publish(event, audience);
    }

    fn main_thread(&self) -> Option<&dyn silo_engine::MainThread> {
        self.engine.main_thread()
    }

    fn show_display(&self, id: &str, title: &str, toolbar: bool) -> Result<(), String> {
        self.host.show_display(self, id, title, toolbar)
    }

    fn close_display(&self, id: &str) {
        self.host.close_display(id);
    }

    fn linux_computer_names(&self) -> Vec<String> {
        self.host.linux_computer_names()
    }
}

/// Why a creation workflow was asked to end.
const RUN: u8 = 0;
const CANCEL_AND_REMOVE: u8 = 1;
const ABORT_AND_KEEP: u8 = 2;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacosComputer {
    id: String,
    name: String,
    cpus: u64,
    #[serde(rename = "memoryGiB")]
    memory_gib: u64,
    #[serde(rename = "diskGiB")]
    disk_gib: u64,
    os_version: Option<String>,
    state: State,
    progress: Option<f64>,
    detail: Option<String>,
    display_open: bool,
    installed: bool,
    setup_complete: bool,
    /// A copy of a template that still has the template's credentials; it cannot be started.
    needs_personalizing: bool,
    /// Newest first.
    checkpoints: Vec<checkpoints::Summary>,
    checkpoint_operation: Option<checkpoints::Operation>,
    /// A Restore that the next Start continues.
    pending_restore: Option<checkpoints::PendingRestore>,
}

/// The template new computers are copied from.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TemplateSummary {
    macos_version: String,
    build: String,
    /// Made by this version of the setup, so new computers are copied from it.
    current: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacosComputersState {
    supported: bool,
    unsupported_reason: Option<String>,
    computers: Vec<MacosComputer>,
    template: Option<TemplateSummary>,
    /// The smallest disk a new computer may have: a copy keeps its template's size.
    #[serde(rename = "minDiskGiB")]
    min_disk_gib: u64,
}

struct Entry {
    record: Record,
    state: State,
    progress: Option<f64>,
    detail: Option<String>,
    cancel: Arc<AtomicU8>,
    display_open: bool,
    /// Its files are being removed; nothing else may use the computer.
    deleting: bool,
    /// The saved checkpoints, newest first.
    checkpoints: Vec<checkpoints::Meta>,
    /// Which Start owns the computer's `starting` state: counted up each time one is admitted.
    /// What a Start does afterwards (running, a failed start's cleanup) only counts while it
    /// still is the current one.
    attempt: u64,
    /// A checkpoint operation owns the computer: it can't be started, stopped or deleted.
    operation: Option<checkpoints::Operation>,
    /// A Retry setup request has been accepted and is waiting for its queue turn or running.
    setup_pending: bool,
    since: Instant,
    progress_emitted: Option<Instant>,
}

impl Entry {
    fn new(record: Record, state: State, detail: Option<String>) -> Self {
        Self {
            record,
            state,
            progress: None,
            detail,
            cancel: Arc::new(AtomicU8::new(RUN)),
            display_open: false,
            deleting: false,
            checkpoints: Vec::new(),
            attempt: 0,
            operation: None,
            setup_pending: false,
            since: Instant::now(),
            progress_emitted: None,
        }
    }

    fn row(&self) -> MacosComputer {
        MacosComputer {
            id: self.record.id.clone(),
            name: self.record.name.clone(),
            cpus: self.record.cpus,
            memory_gib: self.record.memory_gib,
            disk_gib: self.record.disk_gib,
            os_version: self.record.os_version(),
            state: self.state,
            progress: self.progress,
            detail: self.detail.clone(),
            display_open: self.display_open,
            installed: self.record.installed,
            setup_complete: self.record.setup_complete(),
            needs_personalizing: self.record.setup.needs_personalizing || self.record.imported,
            checkpoints: self
                .checkpoints
                .iter()
                .map(checkpoints::Meta::summary)
                .collect(),
            checkpoint_operation: self.operation.clone(),
            pending_restore: self.record.pending_restore.clone(),
        }
    }
}

/// What `Registry::finish_workflow` decided for the computer's files.
enum Finish {
    Remove,
    Kept,
}

/// Every macOS computer, with the state the UI shows and the rules that keep its operations
/// from colliding. All transitions happen under this registry's lock (`update`, `set_state`,
/// and the `begin_*` / `settle_*` methods); the framework's callbacks and the watcher only
/// ever move a computer forward from a state they have just re-read.
///
/// States and events (`-` = refused or no change):
///
/// | State | Start | Stop | Force stop | Delete | Checkpoint op | Machine ends |
/// | --- | --- | --- | --- | --- | --- | --- |
/// | stopped / failed | starting | - | - | removed | capture, restore, fork, delete | - |
/// | starting | - | - | stopping | - | fork, delete only | stopped, or failed on error |
/// | running | - | stopping (ask) | stopping | - | capture, restore, fork, delete | stopped, or failed on error |
/// | stopping | - | - | stays stopping | - | fork, delete only | stopped |
/// | preparing .. setting-up | - | - | - | cancel, then removed | - | (creation ends) |
///
/// A checkpoint operation (`Entry::operation`) is not a state: the computer keeps its state
/// and additionally refuses Start, Stop, Force stop and Delete, and a second operation, until
/// the operation's guard drops. Quit and updates count it as busy; Quit asks it to end first.
///
/// Reverts: a refused stop request returns `stopping` to `running`; a failed start whose
/// machine can't be released becomes `stopping` with Force stop open, never `stopped`/`failed`.
///
/// The watcher (every `WATCH_INTERVAL`, on the framework's states): a stopped or failed
/// machine finishes `running`/`stopping`; a machine still running after `STOP_IGNORED_AFTER`
/// turns `stopping` back to `running`; a paused machine nobody operates on is resumed, unless
/// a pending Restore is unconsumed, when it is force-stopped instead; a `stopping` computer
/// whose machine is gone (checked again against a fresh sample) becomes stopped.
struct Registry {
    loaded: bool,
    entries: Vec<Entry>,
    template: Option<TemplateSummary>,
    min_disk_gib: u64,
    /// The newest macOS build the framework offered the last time it was asked.
    latest_build: Option<String>,
}

impl Registry {
    fn entry(&mut self, id: &str) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|entry| entry.record.id == id)
    }

    /// Marks a failed start as settled, unless something else already moved the computer on
    /// (a late stop callback that released the machine and set it stopped).
    fn settle_failed_start(&mut self, id: &str, attempt: u64, released: Result<(), String>) {
        if let Some(entry) = self.entry(id) {
            if entry.state == State::Starting && entry.attempt == attempt {
                let (state, detail) = state_after_failed_start(released);
                entry.state = state;
                entry.detail = detail;
                entry.progress = None;
                entry.since = Instant::now();
            }
        }
    }

    /// Marks the pending Restore that Start number `attempt` consumed as used. Nothing changes
    /// unless that Start still owns the computer and the record still names exactly that
    /// Restore: a newer Restore's reference is never cleared by an older Start. `save` makes the
    /// new record durable before the registry takes it.
    fn consume_pending_restore(
        &mut self,
        id: &str,
        attempt: u64,
        consumed: &checkpoints::PendingRestore,
        save: impl FnOnce(&Record) -> Result<(), String>,
    ) -> Result<(), String> {
        let entry = self.entry(id).ok_or("This computer no longer exists.")?;
        if entry.state != State::Starting
            || entry.attempt != attempt
            || entry.record.pending_restore.as_ref() != Some(consumed)
        {
            return Err(engine::SUPERSEDED.into());
        }
        let mut record = entry.record.clone();
        record.pending_restore = None;
        save(&record)?;
        entry.record = record;
        Ok(())
    }

    /// Whether Start number `attempt` still owns the computer's `starting` state.
    fn start_is_current(&self, id: &str, attempt: u64) -> bool {
        self.entries.iter().any(|entry| {
            entry.record.id == id && entry.state == State::Starting && entry.attempt == attempt
        })
    }

    /// Whether Start number `attempt` still owns a running computer that nothing else
    /// operates on: the only time its computer use may be updated.
    fn update_may_run(&self, id: &str, attempt: u64) -> bool {
        self.entries.iter().any(|entry| {
            entry.record.id == id
                && entry.state == State::Running
                && entry.attempt == attempt
                && !entry.deleting
                && entry.operation.is_none()
        })
    }

    /// Records that Start number `attempt` brought the guest's computer use to `version`,
    /// durably through `save`, unless the computer moved on meanwhile (stopped, restored,
    /// forked from, deleted or started again). Then nothing is recorded and the update
    /// runs again at a later Start. Returns whether it was recorded.
    fn finish_computer_use_update(
        &mut self,
        id: &str,
        attempt: u64,
        version: &str,
        approval: crate::computer_use::Approval,
        save: impl FnOnce(&Record) -> Result<(), String>,
    ) -> Result<bool, String> {
        let Some(entry) = self.entries.iter_mut().find(|entry| {
            entry.record.id == id
                && entry.attempt == attempt
                && matches!(entry.state, State::Running | State::Stopping)
                && !entry.deleting
                && entry.operation.is_none()
        }) else {
            return Ok(false);
        };
        let mut record = entry.record.clone();
        record.computer_use_version = Some(version.to_string());
        record.computer_use_approval = Some(approval);
        save(&record)?;
        entry.record = record;
        Ok(true)
    }

    /// Computers stopping whose machine the framework no longer holds: the stop callback was
    /// missed or came before the state was set, so nothing else will ever finish them.
    fn stopping_without_machine(&self, held: &[String]) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| entry.state == State::Stopping && entry.operation.is_none())
            .filter(|entry| !held.contains(&entry.record.id))
            .map(|entry| entry.record.id.clone())
            .collect()
    }

    /// Whether any computer is changing state or owned by a checkpoint operation.
    fn any_busy(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| is_busy(entry.state) || entry.operation.is_some())
    }

    /// Whether the creation of `id` may go on: it exists and no Delete cancelled it.
    fn creation_continues(&mut self, id: &str) -> bool {
        self.entry(id)
            .is_some_and(|entry| entry.cancel.load(Ordering::SeqCst) != CANCEL_AND_REMOVE)
    }

    /// Makes a finished installation visible in state `next`, unless a Delete cancelled it meanwhile.
    fn publish_installed(&mut self, id: &str, record: &Record, next: State) -> bool {
        if !self.creation_continues(id) {
            return false;
        }
        let Some(entry) = self.entry(id) else {
            return false;
        };
        entry.record = record.clone();
        entry.state = next;
        entry.detail = None;
        entry.progress = None;
        entry.since = Instant::now();
        true
    }

    /// Ends a cancelled creation: the computer disappears once its files are gone;
    /// if they cannot be removed it stays, failed, so Delete can be retried.
    fn finish_cancelled(&mut self, id: &str, removal: Result<(), String>) {
        match removal {
            Ok(()) => self.entries.retain(|entry| entry.record.id != id),
            Err(message) => {
                if let Some(entry) = self.entry(id) {
                    entry.cancel.store(RUN, Ordering::SeqCst);
                    entry.state = State::Failed;
                    entry.detail = Some(message);
                    entry.progress = None;
                    entry.since = Instant::now();
                }
            }
        }
    }

    /// Publishes how a creation or setup ended. A Delete accepted before this lock was
    /// taken wins over every outcome, success included.
    fn finish_workflow(&mut self, id: &str, result: Result<(), Stop>) -> Finish {
        let Some(entry) = self.entry(id) else {
            return Finish::Kept;
        };
        let flag = entry.cancel.load(Ordering::SeqCst);
        if flag == CANCEL_AND_REMOVE {
            entry.setup_pending = false;
            return Finish::Remove;
        }
        entry.setup_pending = false;
        // A queued Retry Setup has not taken ownership until it publishes SettingUp.
        // Keep a later state transition such as Start's Running state when this waiter
        // is cancelled or revalidation rejects it.
        if entry.record.installed && entry.state != State::SettingUp {
            if let Err(Stop::Failed(message)) = result {
                entry.detail = Some(message);
            }
            return Finish::Kept;
        }
        let (state, detail) = match classify(result, flag) {
            Ok(()) => (State::Stopped, None),
            Err(Stop::Failed(message)) => (State::Failed, Some(message)),
            // Quit ended the work; what finished is kept and the rest can be retried.
            Err(Stop::Cancelled) if entry.record.installed => (State::Stopped, None),
            Err(Stop::Cancelled) => (State::Failed, Some(store::INTERRUPTED_INSTALL.into())),
        };
        entry.state = state;
        entry.detail = detail;
        entry.progress = None;
        entry.since = Instant::now();
        Finish::Kept
    }

    /// Marks a force stop as under way. Returns the state to restore if it cannot be issued.
    fn begin_force_stop(&mut self, id: &str) -> Result<State, String> {
        let entry = self.entry(id).ok_or("This computer no longer exists.")?;
        if !matches!(
            entry.state,
            State::Running | State::Starting | State::Stopping
        ) {
            return Err("This computer isn't running.".into());
        }
        if entry.operation.is_some() {
            return Err(checkpoints::BUSY.into());
        }
        let before = entry.state;
        if entry.state == State::Running {
            entry.state = State::Stopping;
            entry.since = Instant::now();
        }
        entry.detail = None;
        Ok(before)
    }

    fn undo_force_stop(&mut self, id: &str, before: State) {
        if let Some(entry) = self.entry(id) {
            if entry.state == State::Stopping && before == State::Running {
                entry.state = before;
                entry.since = Instant::now();
            }
        }
    }

    /// A force stop failed while the machine is still alive.
    fn force_stop_failed(&mut self, id: &str, message: String) {
        if let Some(entry) = self.entry(id) {
            if matches!(entry.state, State::Stopping | State::Running) {
                entry.state = State::Running;
                entry.detail = Some(message);
                entry.since = Instant::now();
            }
        }
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    loaded: false,
    entries: Vec::new(),
    template: None,
    min_disk_gib: store::MIN_DISK_GIB,
    latest_build: None,
});

/// Why new computers are refused. Quit and an update each own one flag, so neither
/// can reopen what the other closed.
#[derive(Default)]
struct Closed {
    quit: bool,
    update: bool,
}

impl Closed {
    fn check(&self) -> Result<(), String> {
        if self.quit {
            Err("Silo is quitting and stopping its computers. Wait for shutdown to finish.".into())
        } else if self.update {
            Err("Silo is installing an update. Wait for it to finish.".into())
        } else {
            Ok(())
        }
    }

    /// Closes for an update unless `busy`; a refusal changes nothing.
    fn close_for_update(&mut self, busy: bool) -> Result<(), String> {
        if busy {
            return Err("Stop your macOS computers before installing the update.".into());
        }
        self.update = true;
        Ok(())
    }
}

/// Creating and starting check this under the same lock that `stop_all` and
/// `close_for_update` set it under, so nothing starts after Quit has taken its
/// snapshot or after an update has checked that nothing runs.
static CLOSED: Mutex<Closed> = Mutex::new(Closed {
    quit: false,
    update: false,
});

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
static PROXY_BOOTSTRAP_MIGRATION_READY: AtomicBool = AtomicBool::new(true);

/// Closes macOS Start and computer-changing work until the launch migration has
/// staged the legacy guest bootstrap. Called synchronously during app setup.
pub fn begin_proxy_bootstrap_migration() {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    PROXY_BOOTSTRAP_MIGRATION_READY.store(false, Ordering::SeqCst);
}

fn ensure_proxy_bootstrap_migration_ready() -> Result<(), String> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    if !PROXY_BOOTSTRAP_MIGRATION_READY.load(Ordering::SeqCst) {
        return Err(
            "Silo is preparing macOS network access. Wait for startup to finish, then retry."
                .into(),
        );
    }
    Ok(())
}

fn closed() -> std::sync::MutexGuard<'static, Closed> {
    CLOSED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn admission() -> Result<std::sync::MutexGuard<'static, Closed>, String> {
    let closed = closed();
    closed.check()?;
    ensure_proxy_bootstrap_migration_ready()?;
    Ok(closed)
}

/// Admits macOS computers again after a Quit was cancelled.
pub fn reopen_after_quit() {
    closed().quit = false;
}

/// Admits macOS computers again after an update did not go ahead.
pub fn reopen_after_update() {
    closed().update = false;
}

/// Closes admission for an update unless a macOS computer is busy.
pub fn close_for_update() -> Result<(), String> {
    let mut closed = closed();
    let busy = registry().any_busy();
    closed.close_for_update(busy)
}

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn app_data(app: &MacosContext) -> Result<PathBuf, String> {
    Ok(app.paths().app_data.clone())
}

/// Settles a Restore that Silo was interrupted in, so the disk, the auxiliary storage and the
/// pending Restore in the record agree before anything can start the computer. A Restore that
/// can't be settled leaves its journal, which keeps the computer from starting.
fn settle_restore(layout: &Layout, mut record: Record) -> Record {
    match checkpoints::recover(layout) {
        Ok(Some(pending)) => {
            let metadata = checkpoints::find(layout, &pending.checkpoint_id).ok();
            restore_checkpoint_metadata(&mut record, metadata.as_ref());
            record.pending_restore = Some(pending);
            record.pristine = false;
            if store::save(layout, &record).is_ok() {
                checkpoints::finish_restore(layout);
            }
        }
        Ok(None) => {}
        Err(message) => eprintln!("A macOS computer's Restore could not be settled: {message}"),
    }
    record
}

fn restore_checkpoint_metadata(record: &mut Record, metadata: Option<&checkpoints::Meta>) {
    // Missing metadata means the restored disk's computer use is unknown and must be updated.
    record.computer_use_version = metadata.and_then(|meta| meta.computer_use_version.clone());
    record.setup.developer_tools = metadata.is_some_and(|meta| meta.developer_tools);
    record.developer_tools_version = metadata.and_then(|meta| meta.developer_tools_version.clone());
}

fn ensure_loaded(app: &MacosContext) -> Result<(), String> {
    let mut registry = registry();
    if registry.loaded {
        return Ok(());
    }
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        let data = app_data(app)?;
        registry.entries = store::load_all(&data)
            .into_iter()
            .map(|record| {
                let (state, detail) = store::initial_state(&record);
                let layout = Layout::new(&data, &record.id);
                // A marker from an earlier run of this app is not a running computer.
                run_marker::clear(&layout);
                checkpoints::sweep(&layout);
                let record = settle_restore(&layout, record);
                let mut entry = Entry::new(record, state, detail);
                entry.checkpoints = checkpoints::list(&layout);
                entry
            })
            .collect();
        let data = app_data(app)?;
        registry.refresh_template(&data);
    }
    registry.loaded = true;
    Ok(())
}

impl Registry {
    /// Reads the templates on disk: the newest is the one shown. Only the template a new
    /// computer would be copied from (the current setup version, and the newest build when
    /// that is known) counts as current and sets the smallest disk.
    fn refresh_template(&mut self, data: &std::path::Path) {
        let version = templates::setup_version();
        let computer_use = templates::computer_use_version();
        let all = templates::list(data);
        let used = templates::choose(&all, self.latest_build.as_deref(), &version, &computer_use)
            .map(|template| template.name.clone());
        self.min_disk_gib = used
            .as_ref()
            .and_then(|name| all.iter().find(|template| &template.name == name))
            .map_or(store::MIN_DISK_GIB, |template| {
                template.meta.disk_gib.max(store::MIN_DISK_GIB)
            });
        self.template = all.first().map(|template| TemplateSummary {
            macos_version: template.meta.macos_version.clone(),
            build: template.meta.build.clone(),
            current: used.as_ref() == Some(&template.name),
        });
    }
}

/// The templates that computers still being personalized depend on.
fn protected_templates() -> Vec<String> {
    registry()
        .entries
        .iter()
        .filter(|entry| entry.record.setup.needs_personalizing)
        .filter_map(|entry| entry.record.template.clone())
        .collect()
}

fn refresh_template(app: &MacosContext) {
    if let Ok(data) = app_data(app) {
        registry().refresh_template(&data);
    }
}

/// Removes the templates that nothing needs any more.
fn prune_templates(app: &MacosContext) {
    if let Ok(data) = app_data(app) {
        if let Err(message) = templates::prune_stale(&data, &protected_templates) {
            eprintln!("macOS templates could not be pruned: {message}");
        }
    }
    refresh_template(app);
}

/// Asks the framework for the newest macOS in the background, at most every few minutes, so
/// the form knows which template a new computer would use.
fn refresh_latest_build(app: &MacosContext) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    const EVERY: Duration = Duration::from_secs(600);
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return;
    }
    {
        let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if last.is_some_and(|at| at.elapsed() < EVERY) {
            return;
        }
        *last = Some(Instant::now());
    }
    let app = app.clone();
    std::thread::spawn(move || {
        if let Ok(latest) = engine::fetch_latest() {
            note_latest_build(&app, &latest.build);
        }
    });
}

fn note_latest_build(app: &MacosContext, build: &str) {
    registry().latest_build = Some(build.to_string());
    refresh_template(app);
    emit(app);
}

fn snapshot() -> MacosComputersState {
    let reason = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        engine::unsupported_reason()
    } else {
        Some(NOT_SUPPORTED.to_string())
    };
    let registry = registry();
    MacosComputersState {
        supported: reason.is_none(),
        unsupported_reason: reason,
        computers: registry.entries.iter().map(Entry::row).collect(),
        template: registry.template.clone(),
        min_disk_gib: registry.min_disk_gib,
    }
}

/// The running generation this process last published per computer (`None`: cleared).
static PUBLISHED_RUNS: Mutex<Option<HashMap<String, Option<u64>>>> = Mutex::new(None);

/// Publishes which computers are Running, and in which Start generation, for processes that
/// cannot read this registry (the engine socket's guest streams). Clears markers of every
/// other state. Only changes touch the disk.
fn sync_run_markers(app: &MacosContext) {
    let Ok(data) = app_data(app) else {
        return;
    };
    let desired: Vec<(String, Option<u64>)> = registry()
        .entries
        .iter()
        .map(|entry| {
            (
                entry.record.id.clone(),
                (entry.state == State::Running).then_some(entry.attempt),
            )
        })
        .collect();
    let mut published = PUBLISHED_RUNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let published = published.get_or_insert_with(HashMap::new);
    for (id, generation) in desired {
        if published.get(&id) == Some(&generation) {
            continue;
        }
        let layout = Layout::new(&data, &id);
        let done = match generation {
            Some(generation) => run_marker::publish(&layout, generation).is_ok(),
            None => {
                run_marker::clear(&layout);
                true
            }
        };
        if done {
            published.insert(id, generation);
        }
    }
}

fn emit(app: &MacosContext) {
    sync_run_markers(app);
    let state = snapshot();
    publish_job_activity(app, &state);
    if let Ok(payload) = serde_json::to_value(&state) {
        app.publish(
            Event::new("computers.changed", serde_json::json!({"macos": payload})),
            Audience::All,
        );
        app.publish(Event::new(CHANGED_EVENT, payload), Audience::All);
    }
}

fn publish_job_activity(app: &MacosContext, state: &MacosComputersState) {
    retry_pending_checkpoint_completions(app);
    let jobs = MACOS_JOBS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut progress = Vec::new();
    {
        let active = jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for computer in &state.computers {
            if is_macos_job_state(computer.state) {
                if let Some(job_id) = active.get(&computer.id) {
                    progress.push((job_id.clone(), computer));
                }
            }

            if computer.checkpoint_operation.is_some() {
                let checkpoint_job = MACOS_CHECKPOINT_JOBS
                    .get_or_init(Default::default)
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&computer.id)
                    .cloned();
                if let Some(job_id) = checkpoint_job {
                    progress.push((job_id, computer));
                }
            }
        }
    }
    for (job_id, computer) in progress {
        let progress = serde_json::json!({
            "computerId": computer.id,
            "state": computer.state,
            "progress": computer.progress,
            "detail": computer.detail,
            "checkpointOperation": computer.checkpoint_operation,
        });
        let data = match serde_json::from_value::<silo_api::jobs::MacosJobProgress>(progress) {
            Ok(data) => data,
            Err(error) => {
                eprintln!("silo-engine: could not encode macOS job progress: {error}");
                continue;
            }
        };
        let event = silo_api::jobs::JobProgress::Macos {
            job_id: job_id.clone(),
            kind: None,
            computer_id: Some(data.computer_id.clone()),
            data,
        };
        publish_macos_job_progress(app.engine.as_ref(), &job_id, event);
    }
}

fn publish_macos_job_progress(
    context: &dyn EngineContext,
    job_id: &silo_api::jobs::JobId,
    progress: silo_api::jobs::JobProgress,
) {
    publish_macos_job_progress_with_hook(context, job_id, progress, || {});
}

fn publish_macos_job_progress_with_hook(
    context: &dyn EngineContext,
    job_id: &silo_api::jobs::JobId,
    progress: silo_api::jobs::JobProgress,
    before_lock: impl FnOnce(),
) {
    before_lock();
    let _publication = MACOS_JOB_EVENT_PUBLISH_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !macos_job_progress_is_active(job_id) {
        return;
    }
    if let Err(error) = crate::jobs::publish_progress(context, progress, Audience::All) {
        eprintln!("silo-engine: could not persist macOS job progress: {error}");
    }
}

fn macos_job_progress_is_active(job_id: &silo_api::jobs::JobId) -> bool {
    let pending_finish = MACOS_PENDING_CHECKPOINT_JOBS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains_key(job_id);
    if pending_finish {
        return false;
    }
    let lifecycle_active = MACOS_JOBS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
        .any(|active| active == job_id);
    let checkpoint_active = MACOS_CHECKPOINT_JOBS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
        .any(|active| active == job_id);
    lifecycle_active || checkpoint_active
}

#[derive(Clone)]
struct AcceptedMacosJob {
    registry: Arc<crate::jobs::JobRegistry>,
    job_id: silo_api::jobs::JobId,
    cancellation: Arc<AtomicBool>,
    gate: &'static runtime::operation_gate::OperationGate,
}

/// Accepts a macOS job durably before the caller queues work or changes state.
/// Admitted Virtualization.framework work has no safe generic cancellation seam;
/// creation remains cancellable through the existing `computers.delete` path.
fn accept_macos_job(
    context: &dyn EngineContext,
    method: &str,
    gate: &'static runtime::operation_gate::OperationGate,
) -> Result<AcceptedMacosJob, String> {
    let registry = context.job_registry()?;
    let accepted = crate::jobs::allocate(context, method).map_err(|error| error.to_string())?;
    let job_id = accepted.job_id;
    let cancellation = Arc::new(AtomicBool::new(false));
    register_macos_job_cancel(&registry, job_id.clone(), cancellation.clone(), gate);
    Ok(AcceptedMacosJob {
        registry,
        job_id,
        cancellation,
        gate,
    })
}

/// Do not start a worker unless durable job acceptance succeeded.
fn launch_macos_job<T>(
    context: &dyn EngineContext,
    method: &str,
    gate: &'static runtime::operation_gate::OperationGate,
    launch: impl FnOnce(AcceptedMacosJob) -> Result<T, String>,
) -> Result<T, String> {
    let job = accept_macos_job(context, method, gate)?;
    launch(job)
}

fn register_macos_job_cancel(
    registry: &crate::jobs::JobRegistry,
    job_id: silo_api::jobs::JobId,
    cancellation: Arc<AtomicBool>,
    gate: &'static runtime::operation_gate::OperationGate,
) {
    let cancel_job_id = job_id.clone();
    registry.register_cancel(job_id, crate::jobs::CancelRoute::MacosGate, move || {
        // The durable job is accepted just before it is submitted to the gate.
        // Preserve cancellation in that short gap, then let the gate decide whether
        // the now queued or running operation can be cancelled.
        if !gate.contains_job(&cancel_job_id) {
            cancellation.store(true, Ordering::SeqCst);
        }
        gate.cancel_job(&cancel_job_id)
            .map_err(|error| error.to_string())
    });
}

fn admit_macos_job(
    context: &dyn EngineContext,
    job: &AcceptedMacosJob,
    kind: runtime::operation_gate::OperationKind,
    scope: runtime::operation_gate::Scope,
    computer_name: Option<String>,
    label: &str,
) -> Result<runtime::operation_gate::OperationGuard<'static>, String> {
    admit_macos_job_with_completion(context, job, kind, scope, computer_name, label, true)
}

/// Checkpoint command wrappers own their command result, so their admission helper leaves
/// terminal persistence to that wrapper instead of publishing a second finish here.
fn admit_checkpoint_job(
    context: &dyn EngineContext,
    job: &AcceptedMacosJob,
    kind: runtime::operation_gate::OperationKind,
    scope: runtime::operation_gate::Scope,
    computer_name: Option<String>,
    label: &str,
) -> Result<runtime::operation_gate::OperationGuard<'static>, String> {
    admit_macos_job_with_completion(context, job, kind, scope, computer_name, label, false)
}

fn admit_macos_job_with_completion(
    context: &dyn EngineContext,
    job: &AcceptedMacosJob,
    kind: runtime::operation_gate::OperationKind,
    scope: runtime::operation_gate::Scope,
    computer_name: Option<String>,
    label: &str,
    finish_on_error: bool,
) -> Result<runtime::operation_gate::OperationGuard<'static>, String> {
    let guard = match job.gate.kind(kind).acquire_for_job(
        scope,
        computer_name,
        label,
        Some(job.job_id.as_str().to_owned()),
        job.job_id.clone(),
        Some(job.cancellation.clone()),
    ) {
        Ok(guard) => guard,
        Err(error) => {
            let message = error.to_string();
            let code = match error {
                runtime::operation_gate::GateError::Cancelled => {
                    job.cancellation.store(true, Ordering::SeqCst);
                    silo_api::jobs::ErrorCode::Cancelled
                }
                runtime::operation_gate::GateError::AlreadyQueued => {
                    silo_api::jobs::ErrorCode::AlreadyQueued
                }
                runtime::operation_gate::GateError::Busy => silo_api::jobs::ErrorCode::Busy,
                _ => silo_api::jobs::ErrorCode::Internal,
            };
            if finish_on_error {
                publish_macos_job_finished(
                    context,
                    macos_job_finished(job.job_id.clone(), Err((code, message.clone()))),
                );
            }
            return Err(message);
        }
    };
    if let Err(error) = job.registry.mark_running(&job.job_id) {
        let message = error.to_string();
        if finish_on_error {
            fail_macos_job(context, job, message.clone());
        }
        return Err(message);
    }
    job.registry
        .associate_gate(guard.request_id(), job.job_id.clone());
    Ok(guard)
}

fn fail_macos_job(context: &dyn EngineContext, job: &AcceptedMacosJob, message: String) {
    publish_macos_job_finished(
        context,
        macos_job_finished(
            job.job_id.clone(),
            Err((silo_api::jobs::ErrorCode::Internal, message)),
        ),
    );
}

fn admit_and_track_computer_workflow(
    context: &dyn EngineContext,
    job: &AcceptedMacosJob,
    computer_id: &str,
    computer_name: &str,
    label: &str,
) -> Result<runtime::operation_gate::OperationGuard<'static>, String> {
    let operation = admit_macos_job(
        context,
        job,
        runtime::operation_gate::OperationKind::ComputerConfiguration,
        runtime::operation_gate::Scope::Computer {
            id: computer_id.to_owned(),
        },
        Some(computer_name.to_owned()),
        label,
    )?;
    track_macos_job(computer_id, job.job_id.clone());
    Ok(operation)
}

fn lifecycle_job_outcome(
    result: Result<(), String>,
) -> Result<(), (silo_api::jobs::ErrorCode, String)> {
    result.map_err(|message| (silo_api::jobs::ErrorCode::Internal, message))
}

fn macos_job_finished(
    job_id: silo_api::jobs::JobId,
    outcome: Result<(), (silo_api::jobs::ErrorCode, String)>,
) -> silo_api::jobs::JobFinished {
    match outcome {
        Ok(()) => silo_api::jobs::JobFinished {
            job_id,
            result: Some(serde_json::Value::Null),
            error: None,
            outcome_chunked: false,
        },
        Err((code, message)) => silo_api::jobs::JobFinished {
            job_id,
            result: None,
            error: Some(silo_api::transport::ErrorObject {
                code,
                message,
                diagnostics: None,
            }),
            outcome_chunked: false,
        },
    }
}

fn publish_macos_job_finished(context: &dyn EngineContext, finished: silo_api::jobs::JobFinished) {
    let _publication = MACOS_JOB_EVENT_PUBLISH_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Err(error) = crate::jobs::publish_finished(context, finished, Audience::All) {
        eprintln!("silo-engine: could not persist macOS job outcome: {error}");
    }
}

fn track_macos_job(computer_id: &str, job_id: silo_api::jobs::JobId) {
    MACOS_JOBS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(computer_id.to_owned(), job_id);
}

fn untrack_macos_job(computer_id: &str, job_id: &silo_api::jobs::JobId) {
    let mut jobs = MACOS_JOBS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if jobs.get(computer_id) == Some(job_id) {
        jobs.remove(computer_id);
    }
}

fn finish_macos_job(
    context: &dyn EngineContext,
    computer_id: Option<&str>,
    job_id: silo_api::jobs::JobId,
    outcome: Result<(), (silo_api::jobs::ErrorCode, String)>,
) {
    if let Some(computer_id) = computer_id {
        untrack_macos_job(computer_id, &job_id);
    }
    publish_macos_job_finished(context, macos_job_finished(job_id, outcome));
}

fn finish_workflow_job(
    app: &MacosContext,
    computer_id: &str,
    deleted: bool,
    result: Result<(), Stop>,
) {
    let Some(job_id) = MACOS_JOBS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(computer_id)
    else {
        return;
    };
    let outcome = if deleted {
        Err((
            silo_api::jobs::ErrorCode::Cancelled,
            "Computer creation was cancelled by delete.".to_owned(),
        ))
    } else {
        match result {
            Ok(()) => Ok(()),
            Err(Stop::Cancelled) => Err((
                silo_api::jobs::ErrorCode::Cancelled,
                "The macOS operation was cancelled.".to_owned(),
            )),
            Err(Stop::Failed(message)) => Err((silo_api::jobs::ErrorCode::Internal, message)),
        }
    };
    publish_macos_job_finished(app.engine.as_ref(), macos_job_finished(job_id, outcome));
}

fn publish_checkpoint_completion<T: serde::Serialize>(
    app: &MacosContext,
    job_id: silo_api::jobs::JobId,
    computer_id: &str,
    result: &Result<T, String>,
    cancelled: bool,
) {
    let finished = match result {
        Ok(value) => match serde_json::to_value(value) {
            Ok(result) => silo_api::jobs::JobFinished {
                job_id: job_id.clone(),
                result: Some(result),
                error: None,
                outcome_chunked: false,
            },
            Err(error) => silo_api::jobs::JobFinished {
                job_id: job_id.clone(),
                result: None,
                error: Some(silo_api::transport::ErrorObject {
                    code: silo_api::jobs::ErrorCode::Internal,
                    message: format!("Could not encode the macOS checkpoint result: {error}"),
                    diagnostics: None,
                }),
                outcome_chunked: false,
            },
        },
        Err(message) => silo_api::jobs::JobFinished {
            job_id: job_id.clone(),
            result: None,
            error: Some(silo_api::transport::ErrorObject {
                code: if cancelled {
                    silo_api::jobs::ErrorCode::Cancelled
                } else {
                    silo_api::jobs::ErrorCode::Internal
                },
                message: message.clone(),
                diagnostics: None,
            }),
            outcome_chunked: false,
        },
    };
    MACOS_PENDING_CHECKPOINT_JOBS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(job_id, (computer_id.to_owned(), finished));
    retry_pending_checkpoint_completions(app);
    let pending = !MACOS_PENDING_CHECKPOINT_JOBS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_empty();
    if pending {
        ensure_checkpoint_retry_worker(app.clone());
    }
}

fn finish_fork_command(
    app: &MacosContext,
    job_id: silo_api::jobs::JobId,
    computer_id: &str,
    result: &Result<(), String>,
    cancelled: bool,
    continuation: Option<ForkContinuation>,
) {
    if result.is_ok() {
        if let Some(continuation) = continuation {
            MACOS_PENDING_FORK_WORKFLOWS
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(job_id.clone(), continuation);
        }
    }
    publish_checkpoint_completion(app, job_id, computer_id, result, cancelled);
}

fn ensure_checkpoint_retry_worker(app: MacosContext) {
    MACOS_CHECKPOINT_RETRY_WORKER.call_once(move || {
        let result = std::thread::Builder::new()
            .name("silo-checkpoint-job-retry".to_owned())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_secs(2));
                retry_pending_checkpoint_completions(&app);
            });
        if let Err(error) = result {
            eprintln!("silo-engine: could not start checkpoint outcome retry: {error}");
        }
    });
}

fn retry_pending_checkpoint_completions(app: &MacosContext) {
    retry_pending_checkpoint_completions_with_hook(app, || {});
}

fn retry_pending_checkpoint_completions_with_hook(
    app: &MacosContext,
    mut after_pending_removed: impl FnMut(),
) {
    let registry = match app.engine.job_registry() {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("silo-engine: could not open job registry for retry: {error}");
            return;
        }
    };
    if let Err(error) = crate::jobs::retry_pending_outcomes(app.engine.as_ref(), registry.as_ref())
    {
        eprintln!("silo-engine: could not retry pending job outcomes: {error}");
        return;
    }
    let pending = MACOS_PENDING_CHECKPOINT_JOBS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|(job_id, (computer_id, finished))| {
            (job_id.clone(), computer_id.clone(), finished.clone())
        })
        .collect::<Vec<_>>();
    for (job_id, computer_id, finished) in pending {
        let publication = {
            let _publication = MACOS_JOB_EVENT_PUBLISH_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match crate::jobs::publish_finished(app.engine.as_ref(), finished, Audience::All) {
                Ok(()) => {
                    MACOS_PENDING_CHECKPOINT_JOBS
                        .get_or_init(Default::default)
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .remove(&job_id);
                    after_pending_removed();
                    let mut checkpoint_jobs = MACOS_CHECKPOINT_JOBS
                        .get_or_init(Default::default)
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if checkpoint_jobs.get(&computer_id) == Some(&job_id) {
                        checkpoint_jobs.remove(&computer_id);
                    }
                    Ok(())
                }
                Err(error) => Err(error),
            }
        };
        match publication {
            Ok(()) => {
                let continuation = MACOS_PENDING_FORK_WORKFLOWS
                    .get_or_init(Default::default)
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&job_id);
                if let Some(continuation) = continuation {
                    start_fork_continuation(app, continuation);
                }
            }
            Err(error) => {
                eprintln!("silo-engine: could not persist macOS checkpoint outcome: {error}");
            }
        }
    }
}

fn is_macos_job_state(state: State) -> bool {
    matches!(
        state,
        State::Preparing
            | State::Copying
            | State::Downloading
            | State::Installing
            | State::SettingUp
            | State::Starting
            | State::Stopping
    )
}

/// Applies `change` to one computer and tells the UI. Returns `None` for an unknown id.
fn update<R>(app: &MacosContext, id: &str, change: impl FnOnce(&mut Entry) -> R) -> Option<R> {
    let result = {
        let mut registry = registry();
        registry
            .entries
            .iter_mut()
            .find(|entry| entry.record.id == id)
            .map(change)
    };
    if result.is_some() {
        emit(app);
    }
    result
}

fn set_detail(app: &MacosContext, id: &str, detail: &str) {
    update(app, id, |entry| entry.detail = Some(detail.to_string()));
}

/// The files and latest record of a computer.
fn layout_and_record(app: &MacosContext, id: &str) -> Result<(Layout, Record), String> {
    let (record, _) = computer(id)?;
    Ok((Layout::new(&app_data(app)?, id), record))
}

fn set_state(app: &MacosContext, id: &str, state: State, detail: Option<String>) {
    update(app, id, |entry| {
        entry.state = state;
        entry.detail = detail;
        entry.progress = None;
        entry.since = Instant::now();
    });
}

/// Records progress and tells the UI at most once per `PROGRESS_INTERVAL`.
fn set_progress(app: &MacosContext, id: &str, fraction: f64) {
    let due = {
        let mut registry = registry();
        let Some(entry) = registry.entries.iter_mut().find(|e| e.record.id == id) else {
            return;
        };
        entry.progress = Some(fraction);
        let due = entry
            .progress_emitted
            .is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL);
        if due {
            entry.progress_emitted = Some(Instant::now());
        }
        due
    };
    if due {
        emit(app);
    }
}

/// Whether a checkpoint operation owns the computer.
fn has_operation(id: &str) -> bool {
    registry()
        .entries
        .iter()
        .any(|entry| entry.record.id == id && entry.operation.is_some())
}

/// Whether the computer is changing state or owned by an operation: Quit waits for it.
fn is_active(id: &str) -> bool {
    registry()
        .entries
        .iter()
        .any(|entry| entry.record.id == id && (is_busy(entry.state) || entry.operation.is_some()))
}

fn state_of(id: &str) -> Option<State> {
    registry()
        .entries
        .iter()
        .find(|entry| entry.record.id == id)
        .map(|entry| entry.state)
}

pub fn display_label(id: &str) -> String {
    format!("macos-display-{id}")
}

fn require_supported() -> Result<(), String> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        engine::unsupported_reason().map_or(Ok(()), Err)
    } else {
        Err(NOT_SUPPORTED.into())
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| "The macOS computer operation failed unexpectedly.".to_string())?
}

pub async fn read_macos_computers(app: MacosContext) -> Result<MacosComputersState, String> {
    blocking(move || {
        ensure_loaded(&app)?;
        refresh_template(&app);
        refresh_latest_build(&app);
        Ok(snapshot())
    })
    .await
}

/// Removes the template new computers are copied from. Computers made from it keep working.
pub async fn delete_macos_template(app: MacosContext) -> Result<(), String> {
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || {
        require_supported()?;
        ensure_loaded(&app)?;
        let _guard = runtime::OPERATIONS
            .kind(runtime::operation_gate::OperationKind::ComputerConfiguration)
            .device("Deleting the macOS template")
            .map_err(|error| error.to_string())?;
        templates::remove_all(&app_data(&app)?, &protected_templates)?;
        refresh_template(&app);
        emit(&app);
        Ok(())
    })
    .await
}

fn accept_checkpoint_job(app: &MacosContext, method: &str) -> Result<AcceptedMacosJob, String> {
    accept_macos_job(app.engine.as_ref(), method, &runtime::OPERATIONS)
}

fn accept_loaded_lifecycle_job(
    app: &MacosContext,
    id: &str,
    method: &str,
) -> Result<(AcceptedMacosJob, PathBuf, String), String> {
    let job = accept_macos_job(app.engine.as_ref(), method, &runtime::OPERATIONS)?;
    let prepared: Result<(PathBuf, String), String> = (|| {
        ensure_loaded(app)?;
        let data = app_data(app)?;
        let name = computer(id)?.0.name;
        Ok((data, name))
    })();
    match prepared {
        Ok((data, name)) => Ok((job, data, name)),
        Err(message) => {
            fail_macos_job(app.engine.as_ref(), &job, message.clone());
            Err(message)
        }
    }
}

pub async fn create_macos_computer(
    app: MacosContext,
    request: CreateRequest,
) -> Result<MacosComputer, String> {
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || create(&app, request)).await
}

pub async fn macos_computer_action(
    app: MacosContext,
    id: String,
    action: Action,
) -> Result<(), String> {
    if matches!(
        action,
        Action::Start | Action::Setup | Action::ChangeResources { .. }
    ) {
        runtime::shutdown::ensure_accepting_operations()?;
    }
    blocking(move || perform(&app, &id, action)).await
}

pub async fn open_macos_display(app: MacosContext, id: String) -> Result<(), String> {
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || open_display(&app, &id)).await
}

/// Closes this computer's engine-owned screen window without stopping its VM.
pub async fn close_macos_display(app: MacosContext, id: String) -> Result<(), String> {
    blocking(move || {
        ensure_loaded(&app)?;
        let (_, state) = computer(&id)?;
        if state == State::SettingUp {
            return Err("The display cannot close while computer setup is using it.".into());
        }
        app.close_display(&id);
        Ok(())
    })
    .await
}

/// One macOS computer's row as `computers.list` shows it, or `None` when `id` is not one. It
/// reads the inventory only: unlike [`read_macos_computers`] it never looks for new builds.
pub async fn read_macos_computer(
    app: MacosContext,
    id: String,
) -> Result<Option<serde_json::Value>, String> {
    blocking(move || {
        ensure_loaded(&app)?;
        let registry = registry();
        registry
            .entries
            .iter()
            .find(|entry| entry.record.id == id)
            .map(|entry| serde_json::to_value(entry.row()).map_err(|error| error.to_string()))
            .transpose()
    })
    .await
}

/// Whether `id` names a macOS computer of this engine.
pub async fn has_macos_computer(app: MacosContext, id: String) -> Result<bool, String> {
    blocking(move || {
        ensure_loaded(&app)?;
        Ok(computer(&id).is_ok())
    })
    .await
}

/// The computer-use state a record alone can answer: the desired and applied approval and
/// whether the guest still has to be updated. The receipt fields stay null until the guest
/// receipt is read.
fn computer_use_projection(record: &Record) -> serde_json::Value {
    let desired = record
        .computer_use_desired_approval
        .or(record.computer_use_approval)
        .unwrap_or_else(crate::computer_use::initial_approval);
    let applied = record
        .computer_use_approval
        .filter(|_| !record.computer_use_stale());
    let updating = guest_computer_use::update_running(&record.id);
    serde_json::json!({
        "schemaVersion": null,
        "state": null,
        "reason": null,
        "cause": null,
        "compatibility": null,
        "warning": null,
        "approval": desired.as_str(),
        "appliedApproval": applied.map_or("unknown", |approval| approval.as_str()),
        "approvalApply": if applied.is_some() && !updating { "applied" } else { "pending" },
        "approvalApplyReason": null,
        "appVersion": null,
        "chatgptVersion": null,
        "runtimeVersion": null,
        "lcuVersion": null,
        "agents": null,
        "at": null,
    })
}

/// macOS computer-use state that can be projected without entering the guest.
pub async fn macos_computer_use_state(
    app: MacosContext,
    id: String,
) -> Result<serde_json::Value, String> {
    blocking(move || {
        ensure_loaded(&app)?;
        let (record, _) = computer(&id)?;
        Ok(computer_use_projection(&record))
    })
    .await
}

/// Stores desired computer-use approval and schedules the existing guest updater when running.
/// It returns at once; the guest update runs in the background and a stopped computer applies
/// the change at its next Start.
pub async fn set_macos_computer_use_approval(
    app: MacosContext,
    id: String,
    approval: crate::computer_use::Approval,
) -> Result<serde_json::Value, String> {
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || {
        ensure_loaded(&app)?;
        let data = app_data(&app)?;
        let (mut record, state) = computer(&id)?;
        record.computer_use_desired_approval = Some(approval);
        store::save(&Layout::new(&data, &id), &record)?;
        let (attempt, projection) = {
            let mut registry = registry();
            let entry = registry
                .entry(&id)
                .ok_or("This computer no longer exists.")?;
            entry.record = record;
            (entry.attempt, computer_use_projection(&entry.record))
        };
        emit(&app);
        if state == State::Running {
            guest_computer_use::update_in_background(&app, &id, attempt);
        }
        Ok(projection)
    })
    .await
}

/// The state of a computer added by [`install_test_computer`].
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestComputerState {
    Stopped,
    Running,
    SettingUp,
}

/// Adds a finished computer to the shared inventory, for tests of the layers above the engine.
/// Its record is saved under `app_data`, and the inventory counts as loaded.
#[cfg(any(test, feature = "test-support"))]
pub fn install_test_computer(
    app_data: &std::path::Path,
    name: &str,
    state: TestComputerState,
    approval: crate::computer_use::Approval,
) -> String {
    let mut record = store::new_record(
        &CreateRequest {
            name: name.into(),
            cpus: 4,
            memory_gib: 8,
            disk_gib: 64,
        },
        engine::random_mac(),
    );
    record.installed = true;
    record.setup = store::SetupProgress {
        account: true,
        sip: true,
        developer_tools: true,
        computer_use: true,
        clipboard: true,
        needs_personalizing: false,
    };
    record.computer_use_approval = Some(approval);
    record.computer_use_desired_approval = Some(approval);
    record.computer_use_version = Some(templates::computer_use_version_for(approval));
    store::save(&Layout::new(app_data, &record.id), &record).unwrap();
    let id = record.id.clone();
    let state = match state {
        TestComputerState::Stopped => State::Stopped,
        TestComputerState::Running => State::Running,
        TestComputerState::SettingUp => State::SettingUp,
    };
    let mut computers = registry();
    let mut entry = Entry::new(record, state, None);
    entry.attempt = 1;
    computers.entries.push(entry);
    computers.loaded = true;
    id
}

/// Saves a checkpoint of a computer: its memory too when it runs.
pub async fn create_macos_checkpoint(
    app: MacosContext,
    id: String,
    name: String,
) -> Result<(), String> {
    runtime::shutdown::ensure_accepting_operations()?;

    let job = accept_checkpoint_job(&app, "checkpoints.create")?;
    let job_id = job.job_id.clone();
    let cancelled = job.cancellation.clone();
    let worker_app = app.clone();
    let worker_id = id.clone();
    let result = blocking(move || create_checkpoint(&worker_app, &worker_id, &name, job)).await;
    publish_checkpoint_create_completion(
        &app,
        job_id,
        &id,
        &result,
        cancelled.load(Ordering::SeqCst),
    );
    result.map(|_| ())
}

fn publish_checkpoint_create_completion(
    app: &MacosContext,
    job_id: silo_api::jobs::JobId,
    computer_id: &str,
    result: &Result<checkpoints::Meta, String>,
    cancelled: bool,
) {
    let outcome = result.clone().map(checkpoint_created_job_result);
    publish_checkpoint_completion(app, job_id, computer_id, &outcome, cancelled);
}

fn checkpoint_created_job_result(
    meta: checkpoints::Meta,
) -> silo_api::methods::checkpoints::CheckpointCreatedResult {
    use silo_api::methods::checkpoints::{
        Checkpoint, CheckpointCreatedResult, CheckpointKind, CheckpointReason, CheckpointScope,
    };

    CheckpointCreatedResult {
        checkpoint: Checkpoint {
            id: meta.id,
            name: meta.name,
            created_at: meta.created_at,
            scope: match meta.kind {
                checkpoints::Kind::Memory => CheckpointScope::Full,
                checkpoints::Kind::Disk => CheckpointScope::Disk,
            },
            reason: match meta.reason {
                checkpoints::Reason::Manual => CheckpointReason::Manual,
                checkpoints::Reason::BeforeRestore => CheckpointReason::BeforeRestore,
            },
            kind: CheckpointKind::Manual,
            size_bytes: meta.size_bytes,
        },
    }
}

/// Rewinds a computer to a checkpoint. The computer stays stopped.
pub async fn restore_macos_checkpoint(
    app: MacosContext,
    id: String,
    checkpoint_id: String,
) -> Result<(), String> {
    runtime::shutdown::ensure_accepting_operations()?;

    let job = accept_checkpoint_job(&app, "checkpoints.restore")?;
    let job_id = job.job_id.clone();
    let cancelled = job.cancellation.clone();
    let worker_app = app.clone();
    let worker_id = id.clone();
    let result =
        blocking(move || restore_checkpoint(&worker_app, &worker_id, &checkpoint_id, job)).await;
    publish_checkpoint_completion(&app, job_id, &id, &result, cancelled.load(Ordering::SeqCst));
    result
}

/// Creates a new stopped computer from a checkpoint's disk. It is set up in the background.
pub async fn fork_macos_checkpoint(
    app: MacosContext,
    id: String,
    checkpoint_id: String,
    new_name: String,
) -> Result<(), String> {
    runtime::shutdown::ensure_accepting_operations()?;

    let job = accept_checkpoint_job(&app, "checkpoints.fork")?;
    let job_id = job.job_id.clone();
    let cancelled = job.cancellation.clone();
    let worker_app = app.clone();
    let worker_id = id.clone();
    let fork =
        blocking(move || fork_checkpoint(&worker_app, &worker_id, &checkpoint_id, &new_name, job))
            .await;
    let (result, continuation) = match fork {
        Ok(continuation) => (Ok(()), Some(continuation)),
        Err(message) => (Err(message), None),
    };
    finish_fork_command(
        &app,
        job_id,
        &id,
        &result,
        cancelled.load(Ordering::SeqCst),
        continuation,
    );
    result
}

pub async fn delete_macos_checkpoint(
    app: MacosContext,
    id: String,
    checkpoint_id: String,
) -> Result<(), String> {
    runtime::shutdown::ensure_accepting_operations()?;

    let job = accept_checkpoint_job(&app, "checkpoints.delete")?;
    let job_id = job.job_id.clone();
    let cancelled = job.cancellation.clone();
    let worker_app = app.clone();
    let worker_id = id.clone();
    let result =
        blocking(move || delete_checkpoint(&worker_app, &worker_id, &checkpoint_id, job)).await;
    publish_checkpoint_completion(&app, job_id, &id, &result, cancelled.load(Ordering::SeqCst));
    result
}

/// Pastes this Mac's clipboard into a running computer, or copies the computer's
/// clipboard to this Mac. Only the main window can start it.
pub async fn macos_computer_clipboard(
    app: MacosContext,
    id: String,
    direction: guest_clipboard::Direction,
) -> Result<crate::viewer_clipboard::Report, String> {
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || {
        with_macos_computer_operation(
            &app,
            &id,
            runtime::operation_gate::OperationKind::Other,
            "Transferring clipboard",
            || guest_clipboard::run(&app, &id, direction),
        )
    })
    .await
}

/// Reads explicit clipboard content from a running macOS computer for a remote API client.
pub async fn macos_clipboard_read(
    app: MacosContext,
    id: String,
) -> Result<silo_api::methods::clipboard::ClipboardContent, String> {
    blocking(move || guest_clipboard::read_explicit(&app, &id)).await
}

/// Writes explicit clipboard content to a running macOS computer.
pub async fn macos_clipboard_write(
    app: MacosContext,
    id: String,
    content: silo_api::methods::clipboard::ClipboardWritableContent,
) -> Result<(), String> {
    blocking(move || guest_clipboard::write_explicit(&app, &id, content)).await
}

// MARK: For other devices

/// The state of this device's macOS computers as another device reads it. A device that cannot
/// host them reports `supported: false` with the reason.
pub(crate) fn state_for_remote(app: &MacosContext) -> Result<MacosComputersState, String> {
    ensure_loaded(app)?;
    refresh_template(app);
    refresh_latest_build(app);
    Ok(snapshot())
}

/// Creates a computer on another device's request.
pub(crate) fn create_for_remote(
    app: &MacosContext,
    request: CreateRequest,
) -> Result<MacosComputer, String> {
    runtime::shutdown::ensure_accepting_operations()?;
    create(app, request)
}

/// Runs a lifecycle action on another device's request. Start and Setup are refused once Quit
/// has begun, as they are for the local command. Nothing here opens a window.
pub(crate) fn action_for_remote(
    app: &MacosContext,
    id: &str,
    action: Action,
) -> Result<(), String> {
    if matches!(
        action,
        Action::Start | Action::Setup | Action::ChangeResources { .. }
    ) {
        runtime::shutdown::ensure_accepting_operations()?;
    }
    perform(app, id, action)
}

/// The sign-in for the screen of a running computer, for another device.
pub(crate) fn display_session(app: &MacosContext, id: &str) -> Result<DisplaySession, String> {
    runtime::shutdown::ensure_accepting_operations()?;
    ensure_loaded(app)?;
    remote_display::session(app, id)
}

/// A connection to the Screen Sharing port of a running computer, for the owner to relay.
pub(crate) fn display_stream(app: &MacosContext, id: &str) -> Result<std::net::TcpStream, String> {
    runtime::shutdown::ensure_accepting_operations()?;
    ensure_loaded(app)?;
    remote_display::connect(app, id)
}

/// Opens the SSH transport to a running computer for the local engine socket's one-shot
/// `guest.ssh` stream method. `port` is the published-port label of a forward, used only to
/// revoke the stream; the stream itself always reaches the guest's SSH port.
pub fn open_guest_ssh_stream(
    app_data: &std::path::Path,
    computer_id: &str,
    port: Option<u16>,
) -> Result<GuestSshStream, String> {
    ssh_stream::open(app_data, computer_id, port)
}
pub use ssh_stream::{GuestSshGuard, GuestSshStream};

/// Resizes a running computer's display to a viewer's window size (pixels, clamped).
pub(crate) fn resize_display(
    app: &MacosContext,
    id: &str,
    width: u64,
    height: u64,
) -> Result<(u32, u32), String> {
    runtime::shutdown::ensure_accepting_operations()?;
    ensure_loaded(app)?;
    remote_display::resize(app, id, width, height)
}

// MARK: Create

/// The lowercased names of the macOS computers on this device, loaded or not.
pub fn names(app: &MacosContext) -> Vec<String> {
    app_data(app)
        .map(|data| store::names(&data))
        .unwrap_or_default()
}

/// Whether a selected transfer name belongs to the Apple Virtualization runtime.
pub(crate) fn owns_name(app: &MacosContext, name: &str) -> bool {
    names(app)
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn proxy_record(app: &MacosContext, name: &str) -> Result<Option<Record>, String> {
    ensure_loaded(app)?;
    Ok(registry()
        .entries
        .iter()
        .find(|entry| entry.record.name.eq_ignore_ascii_case(name))
        .map(|entry| entry.record.clone()))
}

pub fn proxy_contains_name(app: &MacosContext, name: &str) -> bool {
    names(app)
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

/// Resolve a checkpoint title for the export review phase.
pub(crate) fn checkpoint_name(
    app: &MacosContext,
    computer: &str,
    checkpoint_id: &str,
) -> Result<String, String> {
    require_supported()?;
    ensure_loaded(app)?;
    let record = registry()
        .entries
        .iter()
        .find(|entry| entry.record.name.eq_ignore_ascii_case(computer))
        .map(|entry| entry.record.clone())
        .ok_or_else(|| format!("Computer '{computer}' is not managed by Silo."))?;
    checkpoints::find(&Layout::new(&app_data(app)?, &record.id), checkpoint_id)
        .map(|meta| meta.name)
}

/// Capture one or more Apple Virtualization computers and write their format-4
/// component archive. Running sources are paused only while immutable copies are
/// made; the computer operation slot is released before compression starts.
pub(crate) fn create_archive<R: crate::backup::MsbRunner>(
    app: &MacosContext,
    service: &crate::backup::BackupService<R>,
    destination: &std::path::Path,
    names: &[String],
    checkpoint_id: Option<&str>,
    cancellation: &crate::backup::Cancellation,
    token: Option<&str>,
) -> Result<crate::backup::BackupResult, String> {
    require_supported()?;
    ensure_loaded(app)?;
    if names.is_empty() || (checkpoint_id.is_some() && names.len() != 1) {
        return Err(if checkpoint_id.is_some() {
            "Exporting from a checkpoint supports one computer at a time.".into()
        } else {
            "Choose at least one computer to export.".into()
        });
    }
    let data = app_data(app)?;
    std::fs::create_dir_all(store::root(&data))
        .map_err(|error| store::io_error("prepare the macOS export", &error))?;
    let mut sources = Vec::with_capacity(names.len());
    let mut captures = Vec::with_capacity(names.len());
    for name in names {
        if cancellation.cancelled() {
            return Err(crate::backup::BackupError::Cancelled.to_string());
        }
        let record = registry()
            .entries
            .iter()
            .find(|entry| entry.record.name.eq_ignore_ascii_case(name))
            .map(|entry| entry.record.clone())
            .ok_or_else(|| format!("Computer '{name}' is not managed by Silo."))?;
        let held = begin_operation_for(app, &record.id, checkpoints::OperationKind::Capture, None)?;
        let record = held.record.clone();
        let layout = Layout::new(&data, &record.id);
        let capture = export_capture_dir(&data, token)
            .map_err(|error| store::io_error("prepare the macOS export", &error))?;
        let mut source_kind = crate::backup::MacosArchiveSourceKind::WholeComputer;
        let mut saved_memory = false;
        let mut source_cpus = held.record.cpus;
        let mut source_memory_gib = held.record.memory_gib;
        let mut source_disk_gib = held.record.disk_gib;
        let mut source_setup_fingerprint = held.record.setup_version.clone();
        let mut source_computer_use_fingerprint = held.record.computer_use_version.clone();
        let mut source_guest = held.record.restore_image.clone();
        let mut source_host_build = checkpoints::host_build();

        if let Some(checkpoint_id) = checkpoint_id {
            let meta = checkpoints::find(&layout, checkpoint_id)?;
            source_cpus = meta.cpus.ok_or_else(|| {
                "This checkpoint predates saved VM resources and cannot be exported. Create a new checkpoint and export it again.".to_string()
            })?;
            source_memory_gib = meta.memory_gib.ok_or_else(|| {
                "This checkpoint predates saved VM resources and cannot be exported. Create a new checkpoint and export it again.".to_string()
            })?;
            source_disk_gib = meta.disk_gib.ok_or_else(|| {
                "This checkpoint predates saved VM resources and cannot be exported. Create a new checkpoint and export it again.".to_string()
            })?;
            source_setup_fingerprint = meta.setup_fingerprint.clone();
            source_computer_use_fingerprint = meta.computer_use_version.clone();
            source_host_build = meta.host_build.clone();
            source_guest = meta.macos_version.as_deref().and_then(parse_macos_version);
            if source_guest.is_none() {
                return Err(
                    "This checkpoint predates saved macOS version details and cannot be exported."
                        .into(),
                );
            }
            saved_memory = meta.kind == checkpoints::Kind::Memory;
            source_kind = crate::backup::MacosArchiveSourceKind::Checkpoint {
                id: meta.id.clone(),
                name: meta.name,
                kind: match meta.kind {
                    checkpoints::Kind::Disk => crate::backup::MacosCheckpointKind::Disk,
                    checkpoints::Kind::Memory => crate::backup::MacosCheckpointKind::Memory,
                },
            };
            clone_archive_component(
                &checkpoints::dir(&layout, checkpoint_id).join("disk.img"),
                &capture.path().join("disk.img"),
            )?;
            clone_archive_component(
                &checkpoints::dir(&layout, checkpoint_id).join("auxiliary-storage.img"),
                &capture.path().join("auxiliary-storage.img"),
            )?;
            clone_archive_component(
                &layout.hardware_model(),
                &capture.path().join("hardware-model.bin"),
            )?;
            clone_archive_component(
                &layout.machine_identifier(),
                &capture.path().join("machine-identifier.bin"),
            )?;
            if saved_memory {
                let expected = meta.size_bytes.ok_or_else(|| {
                    "This memory checkpoint has no saved size and cannot be exported.".to_string()
                })?;
                let state = checkpoints::state_file(&layout, checkpoint_id);
                if std::fs::metadata(&state)
                    .map(|metadata| metadata.len())
                    .ok()
                    != Some(expected)
                {
                    return Err(
                        "This checkpoint's saved memory is incomplete and cannot be exported."
                            .into(),
                    );
                }
                clone_archive_component(&state, &capture.path().join("state.vzvmsave"))?;
            }
        } else {
            let copy = || -> Result<(), String> {
                if cancellation.cancelled() {
                    return Err("The operation was cancelled.".into());
                }
                clone_archive_component(&layout.disk(), &capture.path().join("disk.img"))?;
                clone_archive_component(
                    &layout.auxiliary_storage(),
                    &capture.path().join("auxiliary-storage.img"),
                )?;
                clone_archive_component(
                    &layout.hardware_model(),
                    &capture.path().join("hardware-model.bin"),
                )?;
                clone_archive_component(
                    &layout.machine_identifier(),
                    &capture.path().join("machine-identifier.bin"),
                )
            };
            if held.state == State::Running {
                engine::memory_support(app, &record.id)?;
                let _space = reserve_for_memory(&data, &held.record, true)?;
                let machine = LiveMachine {
                    app,
                    id: &record.id,
                    cancel: &held.cancel,
                };
                checkpoints::Machine::save_running(
                    &machine,
                    &capture.path().join("state.vzvmsave"),
                    &mut || copy(),
                )?;
                saved_memory = true;
            } else {
                copy()?;
            }
        }

        let guest = source_guest.take().ok_or_else(|| {
            "This computer has no saved macOS restore image details and cannot be exported."
                .to_string()
        })?;
        let setup_fingerprint = source_setup_fingerprint.ok_or_else(|| {
            "This computer has no setup fingerprint. Use Retry setup before exporting it."
                .to_string()
        })?;
        let computer_use_fingerprint = source_computer_use_fingerprint.ok_or_else(|| {
            "This computer has no Computer Use fingerprint. Use Retry setup before exporting it.".to_string()
        })?;
        let hardware_model_digest = digest_file(&capture.path().join("hardware-model.bin"))?;
        let memory = if saved_memory {
            if source_host_build.is_empty() {
                return Err(
                    "This memory checkpoint has no host build and cannot be exported.".into(),
                );
            }
            let state = capture.path().join("state.vzvmsave");
            let size = std::fs::metadata(&state)
                .map_err(|error| store::io_error("inspect saved memory", &error))?
                .len();
            crate::backup::MacosArchiveMemory::Saved {
                host_build: source_host_build.clone(),
                configuration_digest: crate::backup::memory_configuration_digest(
                    source_cpus,
                    source_memory_gib,
                    source_disk_gib,
                    &hardware_model_digest,
                )?,
                size,
                sha256: digest_file(&state)?,
            }
        } else {
            crate::backup::MacosArchiveMemory::None
        };
        let config = crate::backup::MacosArchiveConfig {
            payload_version: 1,
            architecture: "aarch64".into(),
            source_computer_id: record.id.clone(),
            source_name: name.clone(),
            cpus: source_cpus,
            memory_gib: source_memory_gib,
            disk_gib: source_disk_gib,
            guest_version: guest.version,
            guest_build: guest.build,
            host_build: checkpoints::host_build(),
            hardware_model_digest,
            setup_fingerprint,
            computer_use_fingerprint,
            source: source_kind,
            memory,
        };
        let mut components = vec![
            (crate::backup::MacosComponentRole::Disk, "disk.img"),
            (
                crate::backup::MacosComponentRole::Auxiliary,
                "auxiliary-storage.img",
            ),
            (
                crate::backup::MacosComponentRole::HardwareModel,
                "hardware-model.bin",
            ),
            (
                crate::backup::MacosComponentRole::MachineIdentifier,
                "machine-identifier.bin",
            ),
        ];
        if saved_memory {
            components.push((crate::backup::MacosComponentRole::Memory, "state.vzvmsave"));
        }
        sources.push(crate::backup::MacosArchiveSource {
            name: name.clone(),
            runtime_config: config,
            // AVF computers use a strict runtime configuration and no MicroSandbox row.
            computer_configuration: serde_json::json!({}),
            components: components
                .into_iter()
                .map(|(role, file)| crate::backup::MacosArchiveComponentInput {
                    role,
                    path: capture.path().join(file),
                })
                .collect(),
        });
        captures.push(capture);
        drop(held);
    }
    let result = service
        .create_macos_archive(destination, sources, cancellation, token)
        .map_err(|error| error.to_string());
    drop(captures);
    result
}

/// Capture folders carry the saved operation's id, so a relaunch can find and remove what an
/// interrupted export left. The id is journaled before the export starts; RAII cleans up
/// only when the process lives to run it.
const EXPORT_CAPTURE_PREFIX: &str = ".silo-macos-export-";

fn export_capture_dir(
    data: &std::path::Path,
    token: Option<&str>,
) -> std::io::Result<tempfile::TempDir> {
    let prefix = match token {
        Some(token) => export_capture_prefix(token)
            .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?,
        None => EXPORT_CAPTURE_PREFIX.to_owned(),
    };
    tempfile::Builder::new()
        .prefix(&prefix)
        .tempdir_in(store::root(data))
}

fn export_capture_prefix(token: &str) -> Result<String, String> {
    let token = uuid::Uuid::parse_str(token)
        .map_err(|_| "The export operation identity is invalid.".to_owned())?;
    Ok(format!("{EXPORT_CAPTURE_PREFIX}{token}-"))
}

/// Removes the capture folders an export with this saved operation id left behind: disk
/// clones and saved memory it copied before the process ended. Other operations' folders
/// stay.
pub(crate) fn remove_export_captures(data: &std::path::Path, token: &str) -> Result<(), String> {
    let prefix = export_capture_prefix(token)?;
    let entries = match std::fs::read_dir(store::root(data)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(store::io_error("inspect macOS export captures", &error)),
    };
    for entry in entries {
        let entry =
            entry.map_err(|error| store::io_error("inspect macOS export captures", &error))?;
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| store::io_error("inspect a macOS export capture", &error))?;
        if file_type.is_dir() && !file_type.is_symlink() {
            std::fs::remove_dir_all(entry.path())
                .map_err(|error| store::io_error("remove a macOS export capture", &error))?;
        }
    }
    Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn proxy_validate_computers(app: &MacosContext, computers: &[String]) -> Result<(), String> {
    for name in computers {
        let record = proxy_record(app, name)?
            .ok_or("Secrets can only be assigned to local Silo computers.")?;
        if !record.setup.complete() {
            return Err(
                "Finish setting up the selected macOS computer before assigning secrets.".into(),
            );
        }
    }
    Ok(())
}

fn parse_macos_version(value: &str) -> Option<store::RestoreImageInfo> {
    let (version, build) = value.rsplit_once(" (")?;
    let build = build.strip_suffix(')')?;
    (!version.is_empty() && !build.is_empty()).then(|| store::RestoreImageInfo {
        version: version.into(),
        build: build.into(),
    })
}

pub(crate) fn archive_memory_warning(
    source_name: &str,
    config: &crate::backup::MacosArchiveConfiguration,
) -> Option<String> {
    match &config.memory {
        crate::backup::MacosArchiveMemory::Saved { host_build, .. }
            if host_build != &checkpoints::host_build() => Some(format!(
                "{source_name} has saved memory from macOS build {host_build}. It cannot be reused on this Mac; import starts from the computer's disk."
            )),
        crate::backup::MacosArchiveMemory::Saved { .. } => Some(format!(
            "{source_name} has saved memory. Import verifies it but starts the new computer from its disk."
        )),
        _ => None,
    }
}

/// Whether this Mac's limits allow an archived computer's CPUs, memory, and disk. Imported
/// computers are created from exactly these values, so inspection checks them up front.
pub(crate) fn validate_archive_resources(
    config: &crate::backup::MacosArchiveConfiguration,
) -> Result<(), String> {
    require_supported()?;
    check_archive_resources(engine::host_limits(), config)
}

fn check_archive_resources(
    limits: store::HostLimits,
    config: &crate::backup::MacosArchiveConfiguration,
) -> Result<(), String> {
    store::validate_request(
        &CreateRequest {
            name: "archive-check".into(),
            cpus: config.cpus,
            memory_gib: config.memory_gib,
            disk_gib: config.disk_gib,
        },
        limits,
        &[],
        store::MIN_DISK_GIB,
    )
}

/// Whether this Mac supports the archived computer's hardware model, read from the bounded
/// component file extracted from the archive.
pub(crate) fn validate_archive_hardware_model(path: &std::path::Path) -> Result<(), String> {
    require_supported()?;
    engine::validate_hardware_model_file(path)
}

pub(crate) fn supports_archive_guest_build(build: &str) -> Result<bool, String> {
    developer_tools::supports_guest_build(build)
}

fn clone_archive_component(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    templates::clone_file(from, to).map_err(|error| error.message())
}

fn digest_file(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)
        .map_err(|error| store::io_error("read an archive component", &error))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| store::io_error("read an archive component", &error))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("sha256:{}", hex_sha256(&digest.finalize())))
}

fn hex_sha256(digest: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let byte = *byte;
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

/// Import a fully verified Apple Virtualization archive as a stopped computer.
/// The caller journals `id` before entering this function, so relaunch recovery
/// can remove only this import's deterministic staging folder.
#[derive(Debug)]
pub(crate) enum MacosArchiveImportError {
    Cancelled,
    Failed(String),
}

impl From<String> for MacosArchiveImportError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl From<&str> for MacosArchiveImportError {
    fn from(message: &str) -> Self {
        Self::Failed(message.into())
    }
}

impl From<crate::backup::BackupError> for MacosArchiveImportError {
    fn from(error: crate::backup::BackupError) -> Self {
        match error {
            crate::backup::BackupError::Cancelled => Self::Cancelled,
            error => Self::Failed(error.to_string()),
        }
    }
}

pub(crate) fn import_archive<R: crate::backup::MsbRunner>(
    app: &MacosContext,
    service: &crate::backup::BackupService<R>,
    archive: &std::path::Path,
    source_name: Option<&str>,
    new_name: &str,
    id: &str,
    cancellation: &crate::backup::Cancellation,
    before_publish: &dyn Fn() -> Result<(), String>,
) -> Result<String, MacosArchiveImportError> {
    require_supported()?;
    ensure_loaded(app)?;
    let id = uuid::Uuid::parse_str(id)
        .map_err(|_| "The imported computer identity is invalid.")?
        .to_string();
    let data = app_data(app)?;
    let root = store::root(&data);
    std::fs::create_dir_all(&root)
        .map_err(|error| store::io_error("prepare the macOS import", &error))?;
    let stage = root.join(format!(".partial-import-{id}"));
    let final_dir = root.join(&id);
    discard_import_folder(&stage, &detach_import_disk)?;
    if std::fs::symlink_metadata(&final_dir).is_ok() {
        return Err("The imported computer identity already exists. Choose the export file again and retry.".into());
    }

    let new_name = new_name.trim();
    let requested = source_name.unwrap_or_default();
    let taken = || {
        let mut names = names(app);
        names.extend(app.linux_computer_names());
        names
    };
    let _reservation = crate::computer_names::reserve(&[new_name.to_string()], &taken)?;
    let existing = {
        let registry = registry();
        registry
            .entries
            .iter()
            .map(|entry| entry.record.clone())
            .collect::<Vec<_>>()
    };
    let mut builder = std::fs::DirBuilder::new();
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    builder.mode(0o700);
    builder
        .create(&stage)
        .map_err(|error| store::io_error("create the macOS import staging folder", &error))?;

    let prepared = match service.extract_macos_components(
        archive,
        (!requested.is_empty()).then_some(requested),
        &[
            crate::backup::MacosComponentRole::Disk,
            crate::backup::MacosComponentRole::Auxiliary,
            crate::backup::MacosComponentRole::HardwareModel,
            crate::backup::MacosComponentRole::MachineIdentifier,
        ],
        &stage,
        cancellation,
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = discard_import_folder(&stage, &detach_import_disk);
            return Err(error.into());
        }
    };
    if source_name.is_some_and(|name| name != prepared.source_name)
        || prepared.config.source_name != prepared.source_name
        || prepared.config.architecture != "aarch64"
        || prepared.config.payload_version != 1
        || uuid::Uuid::parse_str(&prepared.config.source_computer_id).is_err()
        || prepared.computer_configuration != serde_json::json!({})
    {
        drop(prepared);
        let _ = discard_import_folder(&stage, &detach_import_disk);
        return Err(
            "The selected computer identity does not match the verified export file.".into(),
        );
    }
    if !developer_tools::supports_guest_build(&prepared.config.guest_build)? {
        let build = prepared.config.guest_build.clone();
        drop(prepared);
        let _ = discard_import_folder(&stage, &detach_import_disk);
        return Err(format!(
            "The export uses macOS build {build}, which this Silo version cannot finish setting up."
        )
        .into());
    }
    let imported_source_name = prepared.source_name.clone();
    let request = CreateRequest {
        name: new_name.trim().to_string(),
        cpus: prepared.config.cpus,
        memory_gib: prepared.config.memory_gib,
        disk_gib: prepared.config.disk_gib,
    };
    if let Err(error) = store::validate_request(
        &request,
        engine::host_limits(),
        &existing,
        store::MIN_DISK_GIB,
    ) {
        drop(prepared);
        let _ = discard_import_folder(&stage, &detach_import_disk);
        return Err(error.into());
    }
    let stage_layout = Layout {
        dir: stage.clone(),
        access: None,
    };
    for (role, destination) in [
        (crate::backup::MacosComponentRole::Disk, stage_layout.disk()),
        (
            crate::backup::MacosComponentRole::Auxiliary,
            stage_layout.auxiliary_storage(),
        ),
        (
            crate::backup::MacosComponentRole::HardwareModel,
            stage_layout.hardware_model(),
        ),
    ] {
        let source = prepared
            .component_paths
            .get(&role)
            .ok_or_else(|| format!("The export is missing its {role:?} component."))?;
        std::fs::rename(source, destination)
            .map_err(|error| store::io_error("stage an imported macOS component", &error))?;
    }
    // The source machine identifier is verified with every other component but
    // is never installed on the new computer.
    let machine_identifier = engine::new_machine_identifier();
    std::fs::write(stage_layout.machine_identifier(), machine_identifier)
        .map_err(|error| store::io_error("save the imported computer identity", &error))?;
    let actual_hardware_digest = digest_file(&stage_layout.hardware_model())?;
    if actual_hardware_digest != prepared.config.hardware_model_digest {
        drop(prepared);
        let _ = discard_import_folder(&stage, &detach_import_disk);
        return Err("The hardware model does not match the verified export metadata.".into());
    }

    let account = guest_access::ensure_account(&stage_layout)?;
    offline_setup::run_import_rekey(&stage_layout.disk(), &account)?;
    let mut record = store::new_record(&request, engine::random_mac());
    record.id = id.clone();
    record.installed = true;
    record.restore_image = Some(store::RestoreImageInfo {
        version: prepared.config.guest_version.clone(),
        build: prepared.config.guest_build.clone(),
    });
    record.setup = store::SetupProgress {
        account: true,
        sip: true,
        developer_tools: false,
        computer_use: false,
        clipboard: false,
        roots: false,
        needs_personalizing: true,
    };
    record.developer_tools_version = None;
    record.pristine = false;
    record.template = None;
    record.setup_version = None;
    record.computer_use_version = None;
    record.computer_use_approval = Some(crate::computer_use::initial_approval());
    record.pending_restore = None;
    record.inherited_access = false;
    record.imported = true;
    engine::validate_archive_configuration(app, &record, &stage_layout)?;
    store::save(&stage_layout, &record)?;
    std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| store::io_error("protect the macOS import staging folder", &error))?;
    store::sync_dir(&stage).map_err(|error| store::io_error("save the macOS import", &error))?;
    drop(prepared);
    if let Err(message) = before_publish() {
        discard_import_folder(&stage, &detach_import_disk)?;
        return Err(if cancellation.cancelled() {
            MacosArchiveImportError::Cancelled
        } else {
            MacosArchiveImportError::Failed(message)
        });
    }
    std::fs::rename(&stage, &final_dir)
        .map_err(|error| store::io_error("publish the imported computer", &error))?;
    store::sync_dir(&root)
        .map_err(|error| store::io_error("save the imported computer", &error))?;
    {
        let mut registry = registry();
        if registry.entries.iter().any(|entry| entry.record.id == id) {
            return Err(
                "The imported computer was saved and needs recovery. Relaunch Silo to finish."
                    .into(),
            );
        }
        registry.entries.push(Entry::new(
            record,
            State::Stopped,
            Some(store::NEEDS_PERSONALIZING.into()),
        ));
    }
    emit(app);
    Ok(imported_source_name)
}

/// Settle a journaled archive import. Returns true only when the final record
/// proves that its atomic directory rename committed. Any folder this import left is
/// removed only after its disk image is detached from this Mac; if a detach fails, the
/// folders stay and so does the caller's journal.
pub(crate) fn recover_archive_import(app: &MacosContext, id: &str) -> Result<bool, String> {
    recover_archive_import_in(&app_data(app)?, id, &detach_import_disk)
}

fn recover_archive_import_in(
    data: &std::path::Path,
    id: &str,
    detach: &dyn Fn(&std::path::Path) -> Result<(), String>,
) -> Result<bool, String> {
    let id = uuid::Uuid::parse_str(id)
        .map_err(|_| "The imported computer identity is invalid.")?
        .to_string();
    let root = store::root(data);
    let stage = root.join(format!(".partial-import-{id}"));
    let final_dir = root.join(&id);
    let committed = match store::read_record(&Layout::new(data, &id)) {
        Ok(record) if record.id == id => true,
        _ => false,
    };
    if committed {
        discard_import_folder(&stage, detach)?;
        return Ok(true);
    }
    discard_import_folder(&stage, detach)?;
    discard_import_folder(&final_dir, detach)?;
    Ok(false)
}

fn detach_import_disk(disk: &std::path::Path) -> Result<(), String> {
    offline_setup::ensure_detached(disk)
}

/// Removes an import's folder after its disk image is detached. The rekey attaches the image
/// persistently, so deleting the folder first would delete an attached disk. When the image
/// cannot be detached the folder is left in place.
fn discard_import_folder(
    path: &std::path::Path,
    detach: &dyn Fn(&std::path::Path) -> Result<(), String>,
) -> Result<(), String> {
    let disk = path.join("disk.img");
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
        && std::fs::symlink_metadata(&disk).is_ok()
    {
        detach(&disk)?;
    }
    remove_archive_import_folder(path)
}

fn remove_archive_import_folder(path: &std::path::Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            std::fs::remove_dir_all(path)
                .map_err(|error| store::io_error("remove an incomplete macOS import", &error))
        }
        Ok(_) => Err("Silo could not verify ownership of the macOS import folder.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(store::io_error("inspect the macOS import folder", &error)),
    }
}

/// Browse one macOS computer's `/Users/silo/workspace` through guest SFTP access tooling.
pub fn list_directory(
    app: &MacosContext,
    caller: String,
    computer_id: String,
    path: String,
    offset: usize,
    snapshot_id: Option<String>,
) -> Result<crate::files::DirectoryPage, String> {
    files::list(app, caller, computer_id, path, offset, snapshot_id)
}

pub fn ensure_ssh_alias(
    app: &MacosContext,
    computer_id: &str,
    path: Option<&str>,
    install_include: Option<bool>,
) -> Result<silo_api::methods::guest::GuestSshAliasEnsureResult, String> {
    ssh_alias::ensure(app, computer_id, path, install_include)
}

pub fn remove_ssh_alias(app: &MacosContext, computer_id: &str) -> Result<(), String> {
    ssh_alias::remove(app, computer_id)
}

pub fn terminal_command(
    app: &MacosContext,
    computer_id: &str,
) -> Result<silo_api::methods::terminal::CommandResult, String> {
    ssh_alias::terminal_command(app, computer_id)
}

pub fn editor_command(
    app: &MacosContext,
    computer_id: &str,
    path: Option<&str>,
    command: &crate::applications::launch::EditorCommand,
) -> Result<silo_api::methods::terminal::CommandResult, String> {
    ssh_alias::editor_command(app, computer_id, path, command)
}

pub fn upload_files(
    app: &MacosContext,
    owner: String,
    transfer_id: String,
    computer_id: String,
    directory: String,
    selection: String,
    conflict: crate::transfer::ConflictPolicy,
) -> Result<crate::transfer::UploadOutcome, String> {
    let (operation, generation) = acquire_transfer_operation(app, &computer_id)?;
    let connection = ssh_alias::ensure(app, &computer_id, None, Some(false))?;
    revalidate_transfer_target(&computer_id, generation)?;
    crate::transfer::upload_files_with_transport(
        PathBuf::from(connection.config_path),
        connection.alias,
        files::transfer_roots(),
        owner,
        transfer_id,
        computer_id,
        directory,
        selection,
        conflict,
        Some(operation),
    )
}

pub fn prepare_download(
    app: &MacosContext,
    transfer_id: String,
    computer_id: String,
    path: String,
) -> Result<(crate::transfer::DownloadSession, String), String> {
    let (operation, generation) = acquire_transfer_operation(app, &computer_id)?;
    let connection = ssh_alias::ensure(app, &computer_id, None, Some(false))?;
    revalidate_transfer_target(&computer_id, generation)?;
    crate::transfer::prepare_download_with_transport(
        PathBuf::from(connection.config_path),
        connection.alias,
        files::transfer_roots(),
        transfer_id,
        computer_id,
        path,
        Some(operation),
    )
}

/// Queue a macOS transfer with the other work for this computer. Capture its generation
/// before waiting, then refuse to transfer if the guest stopped or restarted in the queue.
fn acquire_transfer_operation(
    app: &MacosContext,
    computer_id: &str,
) -> Result<(runtime::operation_gate::OperationGuard<'static>, u64), String> {
    runtime::shutdown::ensure_accepting_operations()?;
    ensure_loaded(app)?;
    let (record, state) = computer(computer_id)?;
    if state != State::Running {
        return Err("Start this computer to transfer files.".into());
    }
    let generation = computer_generation(computer_id)?;
    let operation =
        acquire_macos_transfer_turn(&runtime::OPERATIONS, computer_id, &record.name, || {
            runtime::shutdown::ensure_accepting_operations()?;
            revalidate_transfer_target(computer_id, generation)
        })?;
    Ok((operation, generation))
}

fn acquire_macos_transfer_turn<'a>(
    gate: &'a runtime::operation_gate::OperationGate,
    computer_id: &str,
    name: &str,
    revalidate: impl FnOnce() -> Result<(), String>,
) -> Result<runtime::operation_gate::OperationGuard<'a>, String> {
    let mut operation = acquire_computer_operation(
        gate,
        computer_id,
        name,
        runtime::operation_gate::OperationKind::Other,
        &format!("Transferring files for {name}"),
    )?;
    operation.allow_cancel();
    revalidate()?;
    Ok(operation)
}

fn revalidate_transfer_target(computer_id: &str, generation: u64) -> Result<(), String> {
    let (_, state) = computer(computer_id)?;
    ensure_transfer_target(state, computer_generation(computer_id)?, generation)
}

fn ensure_transfer_target(
    state: State,
    current_generation: u64,
    expected_generation: u64,
) -> Result<(), String> {
    if state != State::Running {
        return Err("Start this computer to transfer files.".into());
    }
    if current_generation != expected_generation {
        return Err("This computer restarted while the transfer was waiting. Retry it.".into());
    }
    Ok(())
}

/// Observe and repair macOS guest listeners and their loopback SSH tunnels.
pub fn read_network_state(
    app: &MacosContext,
) -> Result<silo_api::methods::network::NetworkState, String> {
    ports::state(app)
}

pub fn publish_network_port(
    app: &MacosContext,
    computer_id: &str,
    port: u16,
    host_port: Option<u16>,
    scheme: Option<String>,
) -> Result<silo_api::methods::network::NetworkState, String> {
    ports::publish(app, computer_id, port, host_port, scheme)
}

pub fn remove_network_port(
    app: &MacosContext,
    computer_id: &str,
    port: u16,
) -> Result<silo_api::methods::network::NetworkState, String> {
    ports::unpublish(app, computer_id, port)
}

pub fn open_network_port(
    app: &MacosContext,
    computer_id: &str,
    port: u16,
) -> Result<String, String> {
    ports::open_url(app, computer_id, port)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn proxy_apply_secrets(app: &MacosContext, computer: &str) -> Result<Vec<String>, String> {
    let record = proxy_record(app, computer)?.ok_or("This macOS computer no longer exists.")?;
    let pending = proxy::with_policy_lock(&record.id, || {
        let material = crate::secrets::runtime_material(computer)?;
        proxy::apply_secrets(&record.id, material)
    })?;
    proxy::sync_guest_placeholders(
        app,
        &Layout::new(&app.paths().app_data, &record.id),
        &record,
    )?;
    Ok(pending)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn proxy_revoke_secret(app: &MacosContext, computer: &str, name: &str) -> Result<bool, String> {
    let Some(record) = proxy_record(app, computer)? else {
        return Ok(true);
    };
    let removed = proxy::with_policy_lock(&record.id, || proxy::revoke_secret(&record.id, name))?;
    proxy::sync_guest_placeholders(
        app,
        &Layout::new(&app.paths().app_data, &record.id),
        &record,
    )?;
    Ok(removed)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn proxy_scoped_cached_tokens(
    app: &MacosContext,
    computer: &str,
) -> Result<Vec<String>, String> {
    let Some(record) = proxy_record(app, computer)? else {
        return Ok(Vec::new());
    };
    proxy::scoped_cached_tokens(&record.id)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn proxy_github_policy_is_cached(
    app: &MacosContext,
    computer: &str,
    profile: &serde_json::Value,
) -> Result<bool, String> {
    let Some(record) = proxy_record(app, computer)? else {
        return Ok(false);
    };
    proxy::github_profile_is_cached(&record.id, profile)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn proxy_apply_github_policy(
    app: &MacosContext,
    computer: &str,
    revision: u64,
    profile: &serde_json::Value,
) -> Result<(), String> {
    let Some(record) = proxy_record(app, computer)? else {
        return Ok(());
    };
    proxy::with_policy_lock(&record.id, || {
        proxy::apply_github_profile(&record.id, revision, profile)
    })
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn proxy_apply_github_identity(
    app: &MacosContext,
    computer: &str,
    identity: &serde_json::Value,
) -> Result<(), String> {
    let Some(record) = proxy_record(app, computer)? else {
        return Err("This macOS computer no longer exists.".into());
    };
    if !identity["apply"].as_bool().unwrap_or(false) {
        return Ok(());
    }
    let name = identity["name"]
        .as_str()
        .ok_or("Invalid Git author configuration.")?;
    let email = identity["email"]
        .as_str()
        .ok_or("Invalid Git author configuration.")?;
    if name.trim().is_empty()
        || email.trim().is_empty()
        || name.len() > 1024
        || email.len() > 1024
        || name.chars().any(char::is_control)
        || email.chars().any(char::is_control)
    {
        return Err("Invalid Git author configuration.".into());
    }
    let command = format!(
        "git config --global --replace-all user.name {}; git config --global --replace-all user.email {}; test \"$(git config --global user.name)\" = {} && test \"$(git config --global user.email)\" = {}",
        guest_access::shell_quote(name),
        guest_access::shell_quote(email),
        guest_access::shell_quote(name),
        guest_access::shell_quote(email),
    );
    let output = guest_access::run(
        &Layout::new(&app.paths().app_data, &record.id),
        &record,
        &command,
        None,
        Duration::from_secs(20),
    )?;
    if output.status == 0 {
        Ok(())
    } else {
        Err("The macOS computer rejected its Git author configuration.".into())
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn proxy_is_pending_restore(app: &MacosContext, computer: &str) -> bool {
    proxy_record(app, computer)
        .is_ok_and(|record| record.is_some_and(|record| record.pending_restore.is_some()))
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub fn proxy_validate_computers(_app: &MacosContext, _computers: &[String]) -> Result<(), String> {
    Err(NOT_SUPPORTED.into())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub fn proxy_apply_secrets(_app: &MacosContext, _computer: &str) -> Result<Vec<String>, String> {
    Err(NOT_SUPPORTED.into())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub fn proxy_revoke_secret(
    _app: &MacosContext,
    _computer: &str,
    _name: &str,
) -> Result<bool, String> {
    Ok(true)
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub fn proxy_scoped_cached_tokens(
    _app: &MacosContext,
    _computer: &str,
) -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub fn proxy_github_policy_is_cached(
    _app: &MacosContext,
    _computer: &str,
    _profile: &serde_json::Value,
) -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub fn proxy_apply_github_policy(
    _app: &MacosContext,
    _computer: &str,
    _revision: u64,
    _profile: &serde_json::Value,
) -> Result<(), String> {
    Err(NOT_SUPPORTED.into())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub fn proxy_apply_github_identity(
    _app: &MacosContext,
    _computer: &str,
    _identity: &serde_json::Value,
) -> Result<(), String> {
    Err(NOT_SUPPORTED.into())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub fn proxy_is_pending_restore(_app: &MacosContext, _computer: &str) -> bool {
    false
}

fn create(app: &MacosContext, request: CreateRequest) -> Result<MacosComputer, String> {
    create_with_support_check(app, request, require_supported)
}

fn create_with_support_check(
    app: &MacosContext,
    request: CreateRequest,
    support_check: impl FnOnce() -> Result<(), String>,
) -> Result<MacosComputer, String> {
    support_check()?;
    let data = app_data(app)?;
    create_after_acceptance(app, data, request, &runtime::OPERATIONS, create_workflow)
}

fn create_after_acceptance(
    app: &MacosContext,
    data: PathBuf,
    request: CreateRequest,
    gate: &'static runtime::operation_gate::OperationGate,
    workflow: impl FnOnce(&MacosContext, &std::path::Path, Record, &AtomicU8) + Send + 'static,
) -> Result<MacosComputer, String> {
    launch_macos_job(app.engine.as_ref(), "computers.create", gate, move |job| {
        let (sender, receiver) = std::sync::mpsc::channel();
        let app = app.clone();
        std::thread::spawn(move || create_job_worker(&app, data, request, job, sender, workflow));
        receiver
            .recv()
            .map_err(|_| "The macOS computer creation worker stopped unexpectedly.".to_owned())?
    })
}

fn create_job_worker(
    app: &MacosContext,
    data: PathBuf,
    request: CreateRequest,
    job: AcceptedMacosJob,
    response: std::sync::mpsc::Sender<Result<MacosComputer, String>>,
    workflow: impl FnOnce(&MacosContext, &std::path::Path, Record, &AtomicU8) + Send + 'static,
) {
    let operation = match admit_macos_job(
        app.engine.as_ref(),
        &job,
        runtime::operation_gate::OperationKind::ComputerConfiguration,
        runtime::operation_gate::Scope::Device,
        None,
        &format!("Creating {}", request.name),
    ) {
        Ok(operation) => operation,
        Err(message) => {
            let _ = response.send(Err(message));
            return;
        }
    };
    let mut workflow_finished = false;
    let result: Result<(), String> = (|| {
        let admitted = admission()?;
        // Loading can settle an interrupted Restore and rewrite its journal. Keep that
        // recovery behind the durable creation acceptance and device gate.
        ensure_loaded(app)?;
        let reservation = crate::computer_names::reserve(&[request.name.clone()], &|| {
            app.linux_computer_names()
        })?;
        let mut computers = registry();
        let existing: Vec<Record> = computers
            .entries
            .iter()
            .map(|entry| entry.record.clone())
            .collect();
        computers.refresh_template(&data);
        let min_disk = computers.min_disk_gib;
        store::validate_request(&request, engine::host_limits(), &existing, min_disk)?;
        let record = store::new_record(&request, engine::random_mac());
        // Durable job acceptance and gate admission precede the computer record and installer.
        store::save(&Layout::new(&data, &record.id), &record)?;
        let entry = Entry::new(record.clone(), State::Preparing, None);
        let cancel = entry.cancel.clone();
        let row = entry.row();
        computers.entries.push(entry);
        track_macos_job(&record.id, job.job_id.clone());
        drop(computers);
        drop(reservation);
        drop(admitted);
        if !operation.narrow_to_computer(&record.id, &record.name) {
            let message = "Could not narrow the computer creation operation scope.".to_owned();
            end_workflow(
                app,
                &record.id,
                &Layout::new(&data, &record.id),
                Err(Stop::Failed(message.clone())),
            );
            workflow_finished = true;
            return Err(message);
        }
        emit(app);
        let _ = response.send(Ok(row));
        workflow(app, &data, record, &cancel);
        Ok(())
    })();
    if let Err(message) = result {
        if !workflow_finished {
            fail_macos_job(app.engine.as_ref(), &job, message.clone());
        }
        let _ = response.send(Err(message));
    }
}

#[derive(Clone)]
enum Stop {
    Cancelled,
    Failed(String),
}

impl From<String> for Stop {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl From<restore_image::DownloadError> for Stop {
    fn from(error: restore_image::DownloadError) -> Self {
        match error {
            restore_image::DownloadError::Cancelled => Self::Cancelled,
            restore_image::DownloadError::Failed(message) => Self::Failed(message),
        }
    }
}

impl From<engine::InstallError> for Stop {
    fn from(error: engine::InstallError) -> Self {
        match error {
            engine::InstallError::Cancelled => Self::Cancelled,
            engine::InstallError::Failed(message) => Self::Failed(message),
        }
    }
}

fn create_workflow(app: &MacosContext, data: &std::path::Path, record: Record, cancel: &AtomicU8) {
    let id = record.id.clone();
    let layout = Layout::new(data, &id);
    let result = run_creation(app, data, record, &layout, cancel);
    end_workflow(app, &id, &layout, result);
}

/// Cancelling a queued Create aborts its work but keeps the incomplete computer record.
fn creation_queue_cancelled(cancel: &AtomicU8) {
    request_abort(cancel);
}

/// Provisions an installed computer again, resuming at the first unfinished step.
fn setup_workflow(
    app: &MacosContext,
    data: &std::path::Path,
    mut record: Record,
    cancel: &AtomicU8,
) {
    let id = record.id.clone();
    let layout = Layout::new(data, &id);
    let result = run_setup(app, &layout, &mut record, cancel, None);
    end_workflow(app, &id, &layout, result);
}

fn end_workflow(app: &MacosContext, id: &str, layout: &Layout, result: Result<(), Stop>) {
    // The outcome is decided under the lock a Delete takes, so a Delete accepted
    // before it is never lost.
    let finish = registry().finish_workflow(id, result.clone());
    let deleted = matches!(&finish, Finish::Remove);
    match &finish {
        Finish::Remove => {
            let removal = remove_computer(layout);
            registry().finish_cancelled(id, removal);
        }
        Finish::Kept => {}
    }
    // A copy that ended no longer holds its template.
    prune_templates(app);
    // An installation that held a restore image may have been the last reason to keep it.
    remove_restore_images(app, matches!(finish, Finish::Kept).then_some(id));
    emit(app);
    finish_workflow_job(app, id, deleted, result);
}

/// Deletes a computer's files, after making sure this Mac no longer holds its disk image.
fn remove_computer(layout: &Layout) -> Result<(), String> {
    offline_setup::ensure_detached(&layout.disk())?;
    store::remove(layout)
}

fn run_creation(
    app: &MacosContext,
    data: &std::path::Path,
    mut record: Record,
    layout: &Layout,
    cancel: &AtomicU8,
) -> Result<(), Stop> {
    let id = record.id.clone();
    let cancelled =
        || cancel.load(Ordering::SeqCst) != RUN || runtime::OPERATIONS.removal_queued(&id);
    let check = || {
        if cancelled() {
            Err(Stop::Cancelled)
        } else {
            Ok(())
        }
    };

    set_state(app, &id, State::Preparing, None);
    let latest = engine::fetch_latest();
    check()?;
    // A template of the newest macOS (or, without a network, of any build) replaces the
    // download and the installation.
    let version = templates::setup_version();
    let computer_use = templates::computer_use_version();
    let build = latest.as_ref().ok().map(|latest| latest.build.as_str());
    if let Some(build) = build {
        note_latest_build(app, build);
    }
    if let Some(lease) = templates::lease_matching(data, build, &version, &computer_use) {
        // Held until this creation ends: the copy writes during its personalization.
        let space = templates::reserve_space(data, templates::COPY_ESTIMATE)?;
        if copy_template(app, layout, &mut record, &lease, cancel)? {
            // Persist while the computer is still in its creation state.
            if !registry().creation_continues(&id) {
                return Err(Stop::Cancelled);
            }
            store::save(layout, &record)?;
            if !registry().publish_installed(&id, &record, State::SettingUp) {
                return Err(Stop::Cancelled);
            }
            emit(app);
            // The computer now names its template, which keeps it from being removed.
            drop(lease);
            // The reservation stays with the computer until its personalization is done.
            return run_setup(app, layout, &mut record, cancel, Some(space));
        }
    }
    let latest = latest?;
    set_state(app, &id, State::Preparing, None);
    record.restore_image = Some(store::RestoreImageInfo {
        version: latest.version.clone(),
        build: latest.build.clone(),
    });
    store::save(layout, &record)?;
    update(app, &id, |entry| entry.record = record.clone());

    let images = store::restore_images(data);
    let image = images.join(restore_image::file_name(&latest.url)?);
    let in_use = restore_image::InUse::new(&image);
    set_state(app, &id, State::Downloading, None);
    update(app, &id, |entry| entry.progress = Some(0.0));
    let download_turn = loop {
        match DOWNLOAD_TURN.try_lock() {
            Ok(turn) => break turn,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => break poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                check()?;
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    };
    let image = restore_image::download(&latest.url, &images, &cancelled, &mut |done, total| {
        if let Some(total) = total.filter(|total| *total > 0) {
            set_progress(app, &id, done as f64 / total as f64);
        }
    })?;
    drop(download_turn);
    check()?;

    set_state(app, &id, State::Installing, None);
    update(app, &id, |entry| entry.progress = Some(0.0));
    engine::install(
        app,
        &mut record,
        layout,
        &image,
        &cancelled,
        &mut |fraction| {
            set_progress(app, &id, fraction);
        },
    )?;
    drop(in_use);
    record.installed = true;
    // Persist while the computer is still in its creation state, where nothing else
    // can start or delete it; a Delete accepted meanwhile only sets the cancel flag.
    if !registry().creation_continues(&id) {
        return Err(Stop::Cancelled);
    }
    store::save(layout, &record)?;
    if !registry().publish_installed(&id, &record, State::SettingUp) {
        return Err(Stop::Cancelled);
    }
    emit(app);
    run_setup(app, layout, &mut record, cancel, None)
}

/// Copies a template's files into the new computer and records what the copy still lacks.
/// Returns false when the volume cannot clone files, so the computer is installed instead.
fn copy_template(
    app: &MacosContext,
    layout: &Layout,
    record: &mut Record,
    lease: &templates::Lease,
    cancel: &AtomicU8,
) -> Result<bool, Stop> {
    let id = record.id.clone();
    set_state(app, &id, State::Copying, None);
    if cancel.load(Ordering::SeqCst) != RUN {
        return Err(Stop::Cancelled);
    }
    let template = &lease.template;
    // The form may have been checked against another template.
    templates::check_disk(record.disk_gib, template)?;
    let before = record.clone();
    record.restore_image = Some(store::RestoreImageInfo {
        version: template.meta.macos_version.clone(),
        build: template.meta.build.clone(),
    });
    record.pristine = false;
    record.template = Some(template.name.clone());
    record.setup_version = Some(template.meta.setup_version.clone());
    record.developer_tools_version = Some(developer_tools::version());
    // What the template's guest has; a stale part is updated during this copy's setup.
    record.computer_use_version = Some(template.meta.computer_use_version.clone());
    record.computer_use_approval = Some(crate::computer_use::initial_approval());
    match templates::clone_into(
        template,
        layout,
        &engine::new_machine_identifier(),
        record.disk_gib,
    ) {
        Ok(()) => {}
        Err(templates::CopyError::Unsupported) => {
            *record = before;
            return Ok(false);
        }
        Err(error) => return Err(Stop::Failed(error.message())),
    }
    record.installed = true;
    record.setup = store::SetupProgress {
        account: true,
        sip: true,
        developer_tools: true,
        computer_use: true,
        clipboard: true,
        roots: false,
        needs_personalizing: true,
    };
    Ok(true)
}

/// Runs the provisioning steps, then leaves the computer stopped. A computer that was just
/// installed and set up, and never started by the user, becomes the template.
fn run_setup(
    app: &MacosContext,
    layout: &Layout,
    record: &mut Record,
    cancel: &AtomicU8,
    reservation: Option<templates::SpaceReservation>,
) -> Result<(), Stop> {
    // A copy of a template with a stale computer use part, once updated, replaces it.
    let stale_copy = record.template.is_some() && record.computer_use_stale();
    provision::run(app, layout, record, cancel, reservation)?;
    if cancel.load(Ordering::SeqCst) == RUN {
        if templates::eligible(record) {
            save_template(app, layout, record, false);
        } else if stale_copy && app_data(app).is_ok_and(|data| templates::refreshes(&data, record))
        {
            save_template(app, layout, record, true);
        }
    }
    Ok(())
}

/// Makes the template of `record`. A failure only costs the speed of later computers.
fn save_template(app: &MacosContext, layout: &Layout, record: &Record, refresh: bool) {
    let Ok(data) = app_data(app) else {
        return;
    };
    set_detail(app, &record.id, "Saving a template");
    let Some(version) = record.setup_version.as_deref() else {
        return;
    };
    let Some(computer_use) = record.computer_use_version.as_deref() else {
        return;
    };
    let made = templates::make(
        &data,
        record,
        layout,
        version,
        computer_use,
        refresh,
        &protected_templates,
    );
    match made {
        Ok(Some(_)) => remove_restore_images(app, Some(&record.id)),
        Ok(None) => {}
        Err(message) => {
            if let Some(log) = setup_log::SetupLog::open(app, &record.id) {
                log.line(&format!("the template could not be saved: {message}"));
            }
        }
    }
    refresh_template(app);
}

/// Deletes the cached restore image of every macOS build that has a usable template (the
/// current base setup): an installation is not needed again while it exists. Installations
/// that are reading an image keep it, so this runs again whenever one ends. The image is
/// downloaded again when a new macOS build or a changed base setup needs an installation.
/// `log_to` names the computer whose log notes the removal, when it still exists.
fn remove_restore_images(app: &MacosContext, log_to: Option<&str>) {
    let Ok(data) = app_data(app) else {
        return;
    };
    let images = store::restore_images(&data);
    let builds = templates::builds_with_usable_template(
        &templates::list(&data),
        &templates::setup_version(),
    );
    // Writing to the log of a computer that is gone would recreate its folder.
    let log_to = log_to.filter(|id| computer(id).is_ok());
    for build in builds {
        for (name, bytes) in restore_image::remove_for_build(&images, &build) {
            if let Some(id) = log_to {
                log_line(
                    app,
                    id,
                    &format!(
                        "removed the cached macOS restore image {name} ({bytes} bytes): a template of macOS build {build} replaces the installation"
                    ),
                );
            }
        }
    }
}

/// Two computers created together would otherwise write the same partial image.
static DOWNLOAD_TURN: Mutex<()> = Mutex::new(());

// MARK: Actions

fn acquire_computer_operation<'a>(
    gate: &'a runtime::operation_gate::OperationGate,
    id: &str,
    name: &str,
    kind: runtime::operation_gate::OperationKind,
    label: &str,
) -> Result<runtime::operation_gate::OperationGuard<'a>, String> {
    gate.kind(kind)
        .computer(id, name, label)
        .map_err(|error| error.to_string())
}

fn with_computer_operation<T>(
    id: &str,
    name: &str,
    kind: runtime::operation_gate::OperationKind,
    label: &str,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _guard = acquire_computer_operation(&runtime::OPERATIONS, id, name, kind, label)?;
    work()
}

fn with_macos_computer_operation<T>(
    app: &MacosContext,
    id: &str,
    kind: runtime::operation_gate::OperationKind,
    label: &str,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    ensure_proxy_bootstrap_migration_ready()?;
    ensure_loaded(app)?;
    let (record, _) = computer(id)?;
    with_computer_operation(id, &record.name, kind, label, work)
}

fn perform(app: &MacosContext, id: &str, action: Action) -> Result<(), String> {
    match action {
        Action::Start => start(app, id),
        Action::Stop => stop(app, id),
        Action::ForceStop => force_stop(app, id),
        Action::Delete => {
            ensure_proxy_bootstrap_migration_ready()?;
            ensure_loaded(app)?;
            let (record, _) = computer(id)?;
            let _guard = runtime::OPERATIONS
                .removing(&[id.to_owned()], &format!("Deleting {}", record.name))
                .map_err(|error| error.to_string())?;
            delete(app, id)
        }
        Action::Setup => {
            ensure_proxy_bootstrap_migration_ready()?;
            begin_setup(app, id)
        }
        Action::ChangeResources {
            cpus,
            memory_gib,
            expected,
        } => with_macos_computer_operation(
            app,
            id,
            runtime::operation_gate::OperationKind::ComputerConfiguration,
            "Saving computer settings",
            || change_resource_configuration(app, id, cpus, memory_gib, expected),
        ),
    }
}

fn change_resource_configuration(
    app: &MacosContext,
    id: &str,
    cpus: u64,
    memory_gib: u64,
    expected: store::ExpectedResources,
) -> Result<(), String> {
    require_supported()?;
    let data = app_data(app)?;
    let (record, state) = computer(id)?;
    let restore_unfinished = checkpoints::restore_unfinished(&Layout::new(&data, id));
    let others: Vec<Record> = registry()
        .entries
        .iter()
        .filter(|entry| entry.record.id != id)
        .map(|entry| entry.record.clone())
        .collect();
    let updated = store::change_resources(
        &record,
        state,
        restore_unfinished,
        cpus,
        memory_gib,
        expected,
        engine::host_limits(),
        &others,
    )?;
    let layout = Layout::new(&data, id);
    persist_then_commit(
        updated,
        |record| store::save(&layout, record),
        || store::read_record(&layout).ok(),
        |record| {
            update(app, id, |entry| entry.record = record);
        },
    )?;
    emit(app);
    Ok(())
}

fn persist_then_commit(
    record: Record,
    persist: impl FnOnce(&Record) -> Result<(), String>,
    reconcile: impl FnOnce() -> Option<Record>,
    commit: impl FnOnce(Record),
) -> Result<(), String> {
    match persist(&record) {
        Ok(()) => {
            commit(record);
            Ok(())
        }
        Err(error) => {
            if let Some(saved) = reconcile() {
                commit(saved);
            }
            Err(error)
        }
    }
}

/// Starts a local macOS computer for the launch coordinator. Returns false when `id` belongs
/// to another runtime, so the caller can fall through to the Linux start path.
pub fn start_at_launch(
    app: &MacosContext,
    id: &str,
    cancelled: &AtomicBool,
) -> Result<bool, String> {
    ensure_proxy_bootstrap_migration_ready()?;
    ensure_loaded(app)?;
    let Some(record) = registry()
        .entries
        .iter()
        .find(|entry| entry.record.id == id)
        .map(|entry| entry.record.clone())
    else {
        return Ok(false);
    };
    let started = launch_start_after_queue(
        &runtime::OPERATIONS,
        id,
        &record.name,
        cancelled,
        || runtime::shutdown::ensure_accepting_operations().is_ok() && admission().is_ok(),
        || start(app, id),
    )?;
    Ok(started.is_some())
}

fn launch_start_after_queue<T>(
    gate: &runtime::operation_gate::OperationGate,
    id: &str,
    name: &str,
    cancelled: &AtomicBool,
    accepting: impl Fn() -> bool,
    start: impl FnOnce() -> Result<T, String>,
) -> Result<Option<T>, String> {
    let label = format!("Starting {name}");
    let keep_waiting = || !cancelled.load(Ordering::SeqCst) && accepting();
    let guard = gate
        .kind(runtime::operation_gate::OperationKind::Lifecycle)
        .acquire_while(
            runtime::operation_gate::Scope::Computer { id: id.into() },
            Some(name.to_owned()),
            &label,
            &keep_waiting,
        );
    let _guard = match guard {
        Ok(guard) => guard,
        Err(runtime::operation_gate::GateError::Cancelled)
            if cancelled.load(Ordering::SeqCst) || !accepting() =>
        {
            return Ok(None);
        }
        Err(runtime::operation_gate::GateError::Abandoned)
            if cancelled.load(Ordering::SeqCst) || !accepting() =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error.to_string()),
    };
    if cancelled.load(Ordering::SeqCst) || !accepting() {
        return Ok(None);
    }
    start().map(Some)
}

/// The stored macOS computer IDs available to the launch settings.
pub fn computer_ids(app: &MacosContext) -> Vec<String> {
    app_data(app)
        .map(|data| {
            store::load_all(&data)
                .into_iter()
                .map(|record| record.id)
                .collect()
        })
        .unwrap_or_default()
}

/// Runs the sole conversion path for installed macOS computers created before the
/// proxy bootstrap existed. The caller invokes this from startup after backup and
/// configuration recovery, before lifecycle recovery or launch preferences.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn migrate_proxy_bootstraps_at_launch(app: &MacosContext) -> Result<(), String> {
    let failures = {
        let _migration = runtime::OPERATIONS
            .kind(runtime::operation_gate::OperationKind::Other)
            .hidden()
            .device("Preparing macOS network access")
            .map_err(|error| error.to_string())?;
        ensure_loaded(app)?;
        let data = app_data(app)?;
        let mut failures = Vec::new();
        for id in store::computer_ids(&data) {
            let layout = Layout::new(&data, &id);
            let record = match store::read_record(&layout) {
                Ok(record) if record.id == id => record,
                Ok(_) => {
                    failures.push(format!(
                        "A macOS computer record in {} has a different identity.",
                        layout.dir.display()
                    ));
                    continue;
                }
                Err(_) => {
                    failures.push(format!(
                        "A macOS computer record in {} could not be read.",
                        layout.dir.display()
                    ));
                    continue;
                }
            };
            if !record.installed || !record.setup.complete() {
                continue;
            }
            if checkpoints::restore_unfinished(&layout) {
                failures.push(format!(
                    "{} has an unfinished Restore. Silo left its computer files untouched.",
                    record.name
                ));
                continue;
            }
            if record
                .pending_restore
                .as_ref()
                .is_some_and(|restore| restore.memory)
            {
                failures.push(format!(
                    "{} has saved Restore memory that cannot be cold-booted for network setup. Silo left it untouched.",
                    record.name
                ));
                continue;
            }
            if let Err(error) = proxy::migrate_legacy_guest_bootstrap(app, &record, &layout) {
                // Never move to another guest until Virtualization.framework has
                // released this one's machine, even after a timeout or stop error.
                let samples = engine::machine_samples(app).map_err(|_| {
                    "Silo could not verify that the temporary NAT computer stopped. Relaunch Silo before starting another macOS computer.".to_string()
                })?;
                if samples.iter().any(|sample| sample.id == id) {
                    return Err(format!(
                        "{} could not be stopped after network setup failed: {error}",
                        record.name
                    ));
                }
                failures.push(format!("{}: {error}", record.name));
            }
        }
        failures
    };
    // Per-computer failures remain fail-closed at `require_guest_bootstrap`; valid
    // computers can still start while the user retries this launch migration later.
    PROXY_BOOTSTRAP_MIGRATION_READY.store(true, Ordering::SeqCst);
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub fn migrate_proxy_bootstraps_at_launch(_app: &MacosContext) -> Result<(), String> {
    Ok(())
}

/// The name and retained-log folder of the macOS computer `id`, for the Logs page.
pub fn log_target(app: &MacosContext, id: &str) -> Option<(String, std::path::PathBuf)> {
    let (record, _) = computer(id).ok()?;
    Some((record.name, setup_log::directory(app, id)?))
}

fn computer(id: &str) -> Result<(Record, State), String> {
    registry()
        .entries
        .iter()
        .find(|entry| entry.record.id == id)
        .map(|entry| (entry.record.clone(), entry.state))
        .ok_or_else(|| "This computer no longer exists.".to_string())
}

fn computer_generation(id: &str) -> Result<u64, String> {
    registry()
        .entries
        .iter()
        .find(|entry| entry.record.id == id)
        .map(|entry| entry.attempt)
        .ok_or_else(|| "This computer no longer exists.".to_string())
}

fn start(app: &MacosContext, id: &str) -> Result<(), String> {
    require_supported()?;
    ensure_proxy_bootstrap_migration_ready()?;
    let (job, data, name) = accept_loaded_lifecycle_job(app, id, "computers.start")?;
    let _operation = match admit_macos_job(
        app.engine.as_ref(),
        &job,
        runtime::operation_gate::OperationKind::Lifecycle,
        runtime::operation_gate::Scope::Computer { id: id.to_owned() },
        Some(name),
        "Starting Mac",
    ) {
        Ok(operation) => operation,
        Err(message) => return Err(message),
    };
    // A queued operation must re-read and validate the row after it reaches the gate.
    let begin: Result<_, String> = (|| {
        let admitted = admission()?;
        let mut computers = registry();
        let index = computers
            .entries
            .iter()
            .position(|entry| entry.record.id == id)
            .ok_or("This computer no longer exists.")?;
        let entry = &computers.entries[index];
        if entry.deleting {
            return Err("This computer is being deleted.".into());
        }
        if !entry.record.setup.complete() {
            return Err(store::NEEDS_PERSONALIZING.into());
        }
        store::start_allowed(entry.state, &entry.record)?;
        if entry.operation.is_some() {
            return Err(checkpoints::BUSY.into());
        }
        if checkpoints::restore_unfinished(&Layout::new(&data, id)) {
            return Err(UNFINISHED_RESTORE.into());
        }
        // A computer the user starts is no longer the clean result of its setup. The
        // pending Restore stays in the record, which keeps its checkpoint from being
        // deleted, until the start has read it.
        let mut started = entry.record.clone();
        started.pristine = false;
        let pending = entry.record.pending_restore.clone();
        if started != entry.record {
            store::save(&Layout::new(&data, id), &started)?;
        }
        let entry = &mut computers.entries[index];
        entry.record = started;
        entry.state = State::Starting;
        entry.attempt += 1;
        entry.detail = None;
        entry.since = Instant::now();
        let attempt = entry.attempt;
        let record = entry.record.clone();
        track_macos_job(id, job.job_id.clone());
        drop(computers);
        drop(admitted);
        Ok((record, pending, attempt))
    })();
    let (record, pending, attempt) = match begin {
        Ok(begin) => begin,
        Err(message) => {
            fail_macos_job(app.engine.as_ref(), &job, message.clone());
            return Err(message);
        }
    };
    emit(app);
    watch(app);
    let layout = Layout::new(&data, id);
    let plan = checkpoints::start_plan(&layout, pending.as_ref(), &checkpoints::host_build());
    // The saved memory is used up durably before any machine runs on the disk; a plan that
    // restores it does so itself, once the memory is in the machine and before it resumes.
    let consumed = match &pending {
        Some(pending) if !matches!(plan, checkpoints::StartPlan::RestoreMemory { .. }) => {
            clear_pending_restore(app, id, &layout, attempt, pending)
        }
        _ => Ok(()),
    };
    let started = consumed
        .and_then(|()| offline_setup::ensure_detached(&layout.disk()))
        .and_then(|()| start_machine(app, &record, &layout, plan, attempt, pending.as_ref()));
    match started {
        Ok(note) => {
            let running = update(app, id, |entry| {
                if entry.state == State::Starting && entry.attempt == attempt {
                    entry.state = State::Running;
                    entry.since = Instant::now();
                    entry.detail = note;
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
            if running {
                ports::computer_started(app, id);
            }
            guest_computer_use::update_in_background(app, id, attempt);
            finish_macos_job(
                app.engine.as_ref(),
                Some(id),
                job.job_id,
                lifecycle_job_outcome(Ok(())),
            );
            Ok(())
        }
        Err(message) => {
            // Nothing may report the computer as stopped while its machine can still run.
            let released = ensure_released(app, id, attempt);
            registry().settle_failed_start(id, attempt, released);
            emit(app);
            finish_macos_job(
                app.engine.as_ref(),
                Some(id),
                job.job_id,
                lifecycle_job_outcome(Err(message.clone())),
            );
            Err(message)
        }
    }
}

/// Where a computer stands once a failed start has tried to release its machine. One whose
/// machine is still held stays stopping: busy, so it can't be deleted, restored or ignored by
/// Quit, and open to Force stop.
fn state_after_failed_start(released: Result<(), String>) -> (State, Option<String>) {
    match released {
        Ok(()) => (State::Stopped, None),
        Err(held) => (State::Stopping, Some(held)),
    }
}

/// Stops the machine of failed Start number `attempt` and returns only once the framework has
/// released it, or says that it could not be. It acts only while that Start still owns the
/// computer: once something else has moved the computer on (the machine ended and a later Start
/// began), the machine is not this Start's to stop.
fn ensure_released(app: &MacosContext, id: &str, attempt: u64) -> Result<(), String> {
    for _ in 0..5 {
        let guard = {
            let id = id.to_string();
            move || registry().start_is_current(&id, attempt)
        };
        match engine::force_stop_if(app, id, guard) {
            // No machine, or no longer this Start's.
            Ok(None) => return Ok(()),
            Ok(Some(generation)) => {
                if engine::wait_until_released(app, id, generation, FORCED_STOP_WAIT) {
                    return Ok(());
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
    Err("The computer could not be stopped after it failed to start. Use Force stop.".into())
}

const UNFINISHED_RESTORE: &str =
    "A Restore of this computer did not finish. Restart Silo to settle it before starting the computer.";

/// Marks the pending Restore that Start number `attempt` read as used, durably.
fn clear_pending_restore(
    app: &MacosContext,
    id: &str,
    layout: &Layout,
    attempt: u64,
    consumed: &checkpoints::PendingRestore,
) -> Result<(), String> {
    registry()
        .consume_pending_restore(id, attempt, consumed, |record| store::save(layout, record))?;
    emit(app);
    Ok(())
}

/// Boots the machine as `plan` says. Saved memory the framework refuses is not fatal: the
/// computer boots from its disk and the returned note says why. A note is also returned when
/// the plan itself gave up on the memory.
fn start_machine(
    app: &MacosContext,
    record: &Record,
    layout: &Layout,
    plan: checkpoints::StartPlan,
    attempt: u64,
    pending: Option<&checkpoints::PendingRestore>,
) -> Result<Option<String>, String> {
    use checkpoints::StartPlan;
    let note = match plan {
        StartPlan::Boot => None,
        StartPlan::BootBecause(note) => Some(note),
        StartPlan::RestoreMemory { state } => {
            log_line(app, &record.id, "restoring the checkpoint's memory");
            match engine::start_from_state(app, record, layout, &state) {
                Ok(generation) => {
                    // The saved memory is used up before the machine runs, so no later
                    // Start can apply it to a disk that has changed since. Everything here
                    // concerns this attempt's machine and this attempt's Restore only.
                    let used = match pending {
                        Some(pending) => {
                            clear_pending_restore(app, &record.id, layout, attempt, pending)
                        }
                        None => Err(engine::SUPERSEDED.to_string()),
                    };
                    if let Err(message) = used {
                        let _ = engine::force_stop_generation(app, &record.id, generation);
                        return Err(format!(
                            "The checkpoint's memory was not resumed because Silo could not record that it was used: {message}"
                        ));
                    }
                    let guard = {
                        let id = record.id.clone();
                        move || registry().start_is_current(&id, attempt)
                    };
                    if let Err(message) = engine::resume_if(app, &record.id, generation, guard) {
                        let _ = engine::force_stop_generation(app, &record.id, generation);
                        return Err(message);
                    }
                    log_line(app, &record.id, "memory restored");
                    return Ok(None);
                }
                Err(engine::StateStartError::Failed(message)) => return Err(message),
                Err(engine::StateStartError::Rejected(why)) => {
                    log_line(app, &record.id, &format!("memory not restored: {why}"));
                    // The memory is not coming back: mark it used before booting the disk.
                    match pending {
                        Some(pending) => {
                            clear_pending_restore(app, &record.id, layout, attempt, pending)?
                        }
                        None => return Err(engine::SUPERSEDED.into()),
                    }
                    Some(checkpoints::rejected_note(&why))
                }
            }
        }
    };
    if let Some(note) = &note {
        log_line(app, &record.id, &format!("booting from the disk: {note}"));
    }
    engine::start(app, record, layout)?;
    Ok(note)
}

fn begin_setup(app: &MacosContext, id: &str) -> Result<(), String> {
    require_supported()?;

    let (job, data, name) = accept_loaded_lifecycle_job(app, id, "macos.setup.retry")?;
    let (sender, receiver) = std::sync::mpsc::channel();
    let app = app.clone();
    let worker_id = id.to_owned();
    std::thread::spawn(move || setup_job_worker(&app, data, worker_id, name, job, sender));
    receiver
        .recv()
        .map_err(|_| "The macOS setup worker stopped unexpectedly.".to_owned())?
}

fn setup_job_worker(
    app: &MacosContext,
    data: PathBuf,
    id: String,
    name: String,
    job: AcceptedMacosJob,
    response: std::sync::mpsc::Sender<Result<(), String>>,
) {
    let _operation = match admit_macos_job(
        app.engine.as_ref(),
        &job,
        runtime::operation_gate::OperationKind::ComputerConfiguration,
        runtime::operation_gate::Scope::Computer { id: id.clone() },
        Some(name),
        "Setting up Mac",
    ) {
        Ok(operation) => operation,
        Err(message) => {
            let _ = response.send(Err(message));
            return;
        }
    };
    let begin: Result<_, String> = (|| {
        let admitted = admission()?;
        let mut computers = registry();
        let index = computers
            .entries
            .iter()
            .position(|entry| entry.record.id == id)
            .ok_or("This computer no longer exists.")?;

        let entry = &computers.entries[index];
        if entry.deleting {
            return Err("This computer is being deleted.".into());
        }
        store::setup_allowed(entry.state, &entry.record)?;
        let entry = &mut computers.entries[index];
        entry.cancel.store(RUN, Ordering::SeqCst);
        entry.state = State::SettingUp;
        entry.detail = None;
        entry.progress = None;
        entry.since = Instant::now();
        let record = entry.record.clone();
        let cancel = entry.cancel.clone();
        track_macos_job(&id, job.job_id.clone());
        drop(computers);
        drop(admitted);
        Ok((record, cancel))
    })();
    let (record, cancel) = match begin {
        Ok(begin) => begin,
        Err(message) => {
            fail_macos_job(app.engine.as_ref(), &job, message.clone());
            let _ = response.send(Err(message));
            return;
        }
    };
    emit(app);
    let _ = response.send(Ok(()));
    setup_workflow(app, &data, record, &cancel);
}

/// Reserves a Retry request without publishing `SettingUp` before the operation queue admits it.
fn reserve_setup(entry: &mut Entry) -> Result<(String, Arc<AtomicU8>), String> {
    if entry.deleting {
        return Err("This computer is being deleted.".into());
    }
    store::setup_allowed(entry.state, &entry.record)?;
    if entry.setup_pending {
        return Err("This computer is already waiting for setup.".into());
    }
    entry.setup_pending = true;
    entry.cancel.store(RUN, Ordering::SeqCst);
    Ok((entry.record.name.clone(), entry.cancel.clone()))
}

/// Reads the latest saved record after queue admission, then publishes `SettingUp`.
fn admit_queued_setup(entry: &mut Entry) -> Result<Record, String> {
    if entry.deleting {
        return Err("This computer is being deleted.".into());
    }
    if !entry.setup_pending {
        return Err("The setup request is no longer waiting.".into());
    }
    store::setup_allowed(entry.state, &entry.record)?;
    entry.state = State::SettingUp;
    entry.detail = None;
    entry.progress = None;
    entry.since = Instant::now();
    Ok(entry.record.clone())
}

/// What the guest shows when Stop reaches it through the framework: macOS asks its user to
/// confirm the shutdown.
const CONFIRM_IN_SCREEN: &str =
    "macOS is asking to confirm in the computer's screen. Confirm there, or use Force stop.";
/// The marker is printed before the shutdown starts, so that it proves the command ran on the
/// guest and not just that `ssh` ended with its connection-failure status.
const SHUTDOWN_MARKER: &str = "SILO_SHUTDOWN";
const SSH_SHUTDOWN: &str = "echo SILO_SHUTDOWN; sudo -n /sbin/shutdown -h now";
const SSH_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);

/// How a graceful stop is first attempted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StopPlan {
    /// The account exists: shut down inside the guest, with no prompt.
    Shutdown,
    /// No account to log in with: ask the framework, which makes macOS ask for confirmation.
    RequestStop,
}

fn stop_plan(setup_complete: bool) -> StopPlan {
    if setup_complete {
        StopPlan::Shutdown
    } else {
        StopPlan::RequestStop
    }
}

/// Whether the shutdown command ran and was accepted. `ssh` exits with 255 when the guest closes
/// the connection as it goes down, but also when it could not connect or authenticate, so only
/// the marker the command prints first shows that it ran.
fn shutdown_accepted(result: &Result<guest_access::CommandOutput, String>) -> bool {
    matches!(
        result,
        Ok(output) if (output.status == 0 || output.status == 255)
            && output.stdout.contains(SHUTDOWN_MARKER)
    )
}

/// When Quit stops waiting for guests to shut down: `GRACEFUL_QUIT` from `now` at most, and never
/// into the last `FORCED_QUIT` before `deadline`, which the forced stop needs.
fn graceful_until(now: Instant, deadline: Option<Instant>) -> Instant {
    let at = now + GRACEFUL_QUIT;
    deadline.map_or(at, |deadline| {
        at.min(deadline.checked_sub(FORCED_QUIT).unwrap_or(now).max(now))
    })
}

/// How long Quit may spend on SSH shutdowns: `QUIT_SHUTDOWN_TIMEOUT` at most, and never so
/// long that less than `FORCED_QUIT` of the time left is kept for the forced stop. `None` when
/// too little time is left to try at all.
fn quit_shutdown_timeout(remaining: Option<Duration>) -> Option<Duration> {
    let budget = remaining.map_or(QUIT_SHUTDOWN_TIMEOUT, |remaining| {
        remaining.saturating_sub(FORCED_QUIT)
    });
    let timeout = budget.min(QUIT_SHUTDOWN_TIMEOUT);
    (timeout >= MIN_QUIT_SHUTDOWN).then_some(timeout)
}

/// Shuts the guest down over SSH; false when it could not be done.
fn shutdown_over_ssh(app: &MacosContext, record: &Record, timeout: Duration) -> bool {
    let Ok(data) = app_data(app) else {
        return false;
    };
    let layout = Layout::new(&data, &record.id);
    shutdown_accepted(&guest_access::run(
        &layout,
        record,
        SSH_SHUTDOWN,
        None,
        timeout,
    ))
}

/// Asks a running computer to stop. Returns the note to show when the framework's request was
/// used, because the guest is then waiting for a confirmation nobody gave.
fn request_graceful_stop(
    app: &MacosContext,
    record: &Record,
    timeout: Duration,
) -> Result<Option<&'static str>, String> {
    if stop_plan(record.setup.complete()) == StopPlan::Shutdown
        && shutdown_over_ssh(app, record, timeout)
    {
        return Ok(None);
    }
    engine::request_stop(app, &record.id)?;
    Ok(Some(CONFIRM_IN_SCREEN))
}

fn stop(app: &MacosContext, id: &str) -> Result<(), String> {
    let (job, _, name) = accept_loaded_lifecycle_job(app, id, "computers.stop")?;
    let _operation = match admit_macos_job(
        app.engine.as_ref(),
        &job,
        runtime::operation_gate::OperationKind::Lifecycle,
        runtime::operation_gate::Scope::Computer { id: id.to_owned() },
        Some(name),
        "Stopping Mac",
    ) {
        Ok(operation) => operation,
        Err(message) => return Err(message),
    };
    let begin: Result<_, String> = (|| {
        let mut computers = registry();
        let index = computers
            .entries
            .iter()
            .position(|entry| entry.record.id == id)
            .ok_or("This computer no longer exists.")?;
        let entry = &computers.entries[index];
        if !matches!(entry.state, State::Running | State::Stopping) {
            return Err("This computer isn't running.".into());
        }
        if entry.operation.is_some() {
            return Err(checkpoints::BUSY.into());
        }
        let before = entry.state;
        let entry = &mut computers.entries[index];
        if before == State::Running {
            entry.state = State::Stopping;
            entry.since = Instant::now();
        }
        let record = entry.record.clone();
        track_macos_job(id, job.job_id.clone());
        Ok((record, before))
    })();
    let (record, before) = match begin {
        Ok(begin) => begin,
        Err(message) => {
            fail_macos_job(app.engine.as_ref(), &job, message.clone());
            return Err(message);
        }
    };
    emit(app);
    let note = match request_graceful_stop(app, &record, SSH_SHUTDOWN_TIMEOUT) {
        Ok(note) => note,
        Err(message) => {
            registry().undo_force_stop(id, before);
            emit(app);
            finish_macos_job(
                app.engine.as_ref(),
                Some(id),
                job.job_id,
                lifecycle_job_outcome(Err(message.clone())),
            );
            return Err(message);
        }
    };
    update(app, id, |entry| {
        if entry.state == State::Running {
            entry.state = State::Stopping;
            entry.since = Instant::now();
        }
        if matches!(entry.state, State::Running | State::Stopping) {
            entry.detail = note.map(str::to_string);
        }
    });
    finish_macos_job(
        app.engine.as_ref(),
        Some(id),
        job.job_id,
        lifecycle_job_outcome(Ok(())),
    );
    Ok(())
}

fn force_stop(app: &MacosContext, id: &str) -> Result<(), String> {
    let (job, _, name) = accept_loaded_lifecycle_job(app, id, "computers.forceStop")?;
    let _operation = match admit_macos_job(
        app.engine.as_ref(),
        &job,
        runtime::operation_gate::OperationKind::Lifecycle,
        runtime::operation_gate::Scope::Computer { id: id.to_owned() },
        Some(name),
        "Force stopping Mac",
    ) {
        Ok(operation) => operation,
        Err(message) => return Err(message),
    };
    let begin: Result<_, String> = (|| {
        let mut computers = registry();
        let index = computers
            .entries
            .iter()
            .position(|entry| entry.record.id == id)
            .ok_or("This computer no longer exists.")?;
        let entry = &computers.entries[index];
        if !matches!(
            entry.state,
            State::Running | State::Starting | State::Stopping
        ) {
            return Err("This computer isn't running.".into());
        }
        if entry.operation.is_some() {
            return Err(checkpoints::BUSY.into());
        }
        let before = entry.state;
        let entry = &mut computers.entries[index];
        if entry.state == State::Running {
            entry.state = State::Stopping;
            entry.since = Instant::now();
        }
        entry.detail = None;
        track_macos_job(id, job.job_id.clone());
        Ok(before)
    })();
    let before = match begin {
        Ok(before) => before,
        Err(message) => {
            fail_macos_job(app.engine.as_ref(), &job, message.clone());
            return Err(message);
        }
    };
    emit(app);
    if let Err(message) = engine::force_stop(app, id) {
        registry().undo_force_stop(id, before);
        emit(app);
        finish_macos_job(
            app.engine.as_ref(),
            Some(id),
            job.job_id,
            lifecycle_job_outcome(Err(message.clone())),
        );
        return Err(message);
    }
    finish_macos_job(
        app.engine.as_ref(),
        Some(id),
        job.job_id,
        lifecycle_job_outcome(Ok(())),
    );
    Ok(())
}

fn delete(app: &MacosContext, id: &str) -> Result<(), String> {
    let data = app_data(app)?;
    let removed = {
        let mut registry = registry();
        let index = registry
            .entries
            .iter()
            .position(|entry| entry.record.id == id)
            .ok_or("This computer no longer exists.")?;
        if registry.entries[index].operation.is_some() {
            return Err(checkpoints::BUSY.into());
        }
        match store::delete_mode(registry.entries[index].state)? {
            DeleteMode::Cancel => {
                let entry = &mut registry.entries[index];
                entry.cancel.store(CANCEL_AND_REMOVE, Ordering::SeqCst);
                entry.detail = Some("Cancelling…".into());
                false
            }
            DeleteMode::Remove => {
                let entry = &mut registry.entries[index];
                if entry.deleting {
                    return Err("This computer is already being deleted.".into());
                }
                entry.deleting = true;
                true
            }
        }
    };
    if removed {
        close_display(app, id);
        // A computer use update stops once the deletion is marked; it must have ended before
        // the folder goes, or it could write to it again.
        if let Err(message) = guest_computer_use::wait_for_update_end(id) {
            update(app, id, |entry| entry.deleting = false);
            return Err(message);
        }
        // The entry stays until the files are gone, so a failed removal can be retried.
        match remove_computer(&Layout::new(&data, id)) {
            Ok(()) => {
                registry().entries.retain(|entry| entry.record.id != id);
                if let Err(message) = ports::computer_removed(app, id) {
                    eprintln!("A deleted macOS computer's published ports could not be removed: {message}");
                }
            }
            Err(message) => {
                update(app, id, |entry| {
                    entry.deleting = false;
                    entry.state = State::Failed;
                    entry.detail = Some(message.clone());
                });
                return Err(message);
            }
        }
    }
    prune_templates(app);
    emit(app);
    Ok(())
}

/// A force stop failed while the machine is still alive: keep it tracked as running.
fn force_stop_failed(app: &MacosContext, id: &str, message: String) {
    registry().force_stop_failed(id, message);
    emit(app);
}

/// Whether an event of the machine of generation `event` concerns the machine the computer
/// has now (`current`, `None` when it has none). Machines are never reused: a late event of
/// an earlier one concerns no one.
fn slot_is_current(current: Option<u64>, event: u64) -> bool {
    current == Some(event)
}

/// The machine ended on its own or at Silo's request.
fn machine_stopped(app: &MacosContext, id: &str, generation: u64, error: Option<String>) {
    machine_stopped_with_lifecycle(app, id, generation, error, true);
}

/// Ends a machine while respecting whether the boot contributes to the user's state.
fn machine_stopped_with_lifecycle(
    app: &MacosContext,
    id: &str,
    generation: u64,
    error: Option<String>,
    lifecycle_managed: bool,
) {
    let (app, id) = (app.clone(), id.to_string());
    // Deferred so the framework callback that reported the stop has returned before
    // its machine is released.
    engine::defer(move || {
        // An event of an earlier machine of this computer changes nothing.
        if !engine::release_slot(&id, generation) {
            return;
        }
        close_display(&app, &id);
        ports::computer_stopped(&id);
        if !lifecycle_managed {
            return;
        }
        update(&app, &id, |entry| {
            if matches!(
                entry.state,
                State::Starting | State::Running | State::Stopping
            ) {
                entry.state = if error.is_some() {
                    State::Failed
                } else {
                    State::Stopped
                };
                entry.detail = error;
                entry.progress = None;
                entry.display_open = false;
                entry.since = Instant::now();
            }
        });
    });
}

// MARK: Checkpoints

const GIB: u64 = 1 << 30;
/// How long a forced stop may take before a Restore gives up on it.
const FORCED_STOP_WAIT: Duration = Duration::from_secs(30);

/// Ends a checkpoint operation when dropped, whichever way the work ended.
struct Held<'a> {
    app: &'a MacosContext,
    id: String,
    record: Record,
    state: State,
    cancel: Arc<AtomicU8>,
    _operation: Option<runtime::operation_gate::OperationGuard<'static>>,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        update(self.app, &self.id, |entry| entry.operation = None);
    }
}

fn first_stage(kind: checkpoints::OperationKind) -> &'static str {
    match kind {
        checkpoints::OperationKind::Capture => "Preparing",
        checkpoints::OperationKind::Restore => "Preparing",
        checkpoints::OperationKind::Fork => "Copying the checkpoint",
        checkpoints::OperationKind::Delete => "Removing the checkpoint",
    }
}

fn checkpoint_gate_kind(
    kind: checkpoints::OperationKind,
) -> runtime::operation_gate::OperationKind {
    match kind {
        checkpoints::OperationKind::Capture => {
            runtime::operation_gate::OperationKind::CheckpointCapture
        }
        checkpoints::OperationKind::Restore => {
            runtime::operation_gate::OperationKind::CheckpointRestore
        }
        checkpoints::OperationKind::Fork => runtime::operation_gate::OperationKind::CheckpointFork,
        checkpoints::OperationKind::Delete => {
            runtime::operation_gate::OperationKind::CheckpointDelete
        }
    }
}

/// Claims the computer for one checkpoint operation, if nothing else is using it.
fn begin_operation<'a>(
    app: &'a MacosContext,
    id: &str,
    kind: checkpoints::OperationKind,
    job: &AcceptedMacosJob,
) -> Result<Held<'a>, String> {
    begin_operation_for(app, id, kind, Some(job))
}

/// Claims the computer for `kind`. A checkpoint job admits through the operation queue; an
/// export holds its own export operation for the whole archive and passes no job.
fn begin_operation_for<'a>(
    app: &'a MacosContext,
    id: &str,
    kind: checkpoints::OperationKind,
    job: Option<&AcceptedMacosJob>,
) -> Result<Held<'a>, String> {
    drop(admission()?);
    let operation = match job {
        Some(job) => {
            let computer_name = registry()
                .entry(id)
                .map(|entry| entry.record.name.clone())
                .ok_or("This computer no longer exists.")?;
            let gate_kind = checkpoint_gate_kind(kind);
            let label = match kind {
                checkpoints::OperationKind::Capture => "Creating checkpoint",
                checkpoints::OperationKind::Restore => "Restoring checkpoint",
                checkpoints::OperationKind::Fork => "Forking checkpoint",
                checkpoints::OperationKind::Delete => "Deleting checkpoint",
            };
            Some(admit_checkpoint_job(
                app.engine.as_ref(),
                job,
                gate_kind,
                runtime::operation_gate::Scope::Computer { id: id.to_owned() },
                Some(computer_name),
                label,
            )?)
        }
        None => None,
    };
    let _admitted = admission()?;
    let (record, state, cancel) = {
        let mut registry = registry();
        let entry = registry
            .entry(id)
            .ok_or("This computer no longer exists.")?;
        store::checkpoint_allowed(
            entry.state,
            &entry.record,
            entry.deleting,
            entry.operation.is_some(),
            kind,
        )?;
        entry.cancel.store(RUN, Ordering::SeqCst);
        entry.operation = Some(checkpoints::Operation::running(kind, first_stage(kind)));
        (entry.record.clone(), entry.state, entry.cancel.clone())
    };
    if let Some(job) = job {
        MACOS_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.to_owned(), job.job_id.clone());
    }
    emit(app);
    Ok(Held {
        app,
        id: id.to_string(),
        record,
        state,
        cancel,
        _operation: operation,
    })
}

/// The machine of a computer as the checkpoint workflows reach it.
struct LiveMachine<'a> {
    app: &'a MacosContext,
    id: &'a str,
    cancel: &'a AtomicU8,
}

impl checkpoints::Machine for LiveMachine<'_> {
    fn memory_support(&self) -> Result<(), String> {
        engine::memory_support(self.app, self.id)
    }

    fn save_running(
        &self,
        state: &std::path::Path,
        copy: &mut dyn FnMut() -> Result<(), String>,
    ) -> Result<(), String> {
        engine::save_running(self.app, self.id, state, copy)
    }

    fn force_stop(&self) -> Result<(), String> {
        engine::force_stop(self.app, self.id)?;
        if engine::wait_until_stopped(self.app, self.id, FORCED_STOP_WAIT) {
            Ok(())
        } else {
            Err("The computer did not stop in time.".into())
        }
    }

    fn stage(&self, stage: &str) {
        update(self.app, self.id, |entry| {
            if let Some(operation) = &mut entry.operation {
                operation.stage = stage.to_string();
            }
        });
    }

    fn log(&self, line: &str) {
        if let Some(log) = setup_log::SetupLog::open(self.app, self.id) {
            log.line(line);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst) != RUN
    }
}

fn log_line(app: &MacosContext, id: &str, line: &str) {
    if let Some(log) = setup_log::SetupLog::open(app, id) {
        log.line(line);
    }
}

/// Reads the checkpoints on disk again, after an operation that may have changed them.
fn refresh_checkpoints(app: &MacosContext, id: &str, layout: &Layout) {
    let found = checkpoints::list(layout);
    update(app, id, |entry| entry.checkpoints = found);
}

/// Space for the memory a checkpoint of a running computer saves.
fn reserve_for_memory(
    data: &std::path::Path,
    record: &Record,
    running: bool,
) -> Result<Option<templates::SpaceReservation>, String> {
    running
        .then(|| templates::reserve_space(data, record.memory_gib * GIB))
        .transpose()
}

fn create_checkpoint(
    app: &MacosContext,
    id: &str,
    name: &str,
    job: AcceptedMacosJob,
) -> Result<checkpoints::Meta, String> {
    require_supported()?;
    ensure_loaded(app)?;
    let name = checkpoints::validate_name(name)?;
    let data = app_data(app)?;
    let held = begin_operation(app, id, checkpoints::OperationKind::Capture, &job)?;
    let layout = Layout::new(&data, id);
    if checkpoints::restore_unfinished(&layout) {
        return Err(UNFINISHED_RESTORE.into());
    }
    let running = held.state == State::Running;
    log_line(
        app,
        id,
        &format!(
            "creating checkpoint \"{name}\" ({})",
            if running { "memory and disk" } else { "disk" }
        ),
    );
    let _space = reserve_for_memory(&data, &held.record, running)?;
    let machine = LiveMachine {
        app,
        id,
        cancel: &held.cancel,
    };
    let subject = checkpoints::Subject {
        layout: &layout,
        machine: &machine,
        running,
        macos_version: held.record.os_version(),
        computer_use_version: held.record.computer_use_version.clone(),
        developer_tools: held.record.setup.developer_tools,
        developer_tools_version: held.record.developer_tools_version.clone(),
        cpus: held.record.cpus,
        memory_gib: held.record.memory_gib,
        disk_gib: held.record.disk_gib,
        setup_fingerprint: held.record.setup_version.clone(),
    };
    let result = checkpoints::create(&subject, &name, checkpoints::Reason::Manual);
    refresh_checkpoints(app, id, &layout);
    result
}

fn restore_checkpoint(
    app: &MacosContext,
    id: &str,
    checkpoint_id: &str,
    job: AcceptedMacosJob,
) -> Result<(), String> {
    require_supported()?;
    ensure_loaded(app)?;
    let data = app_data(app)?;
    let held = begin_operation(app, id, checkpoints::OperationKind::Restore, &job)?;
    let layout = Layout::new(&data, id);
    if checkpoints::restore_unfinished(&layout) {
        return Err(UNFINISHED_RESTORE.into());
    }
    let target = checkpoints::find(&layout, checkpoint_id)?;
    let running = held.state == State::Running;
    log_line(
        app,
        id,
        &format!("restoring checkpoint \"{}\" ({})", target.name, target.id),
    );
    let _space = reserve_for_memory(&data, &held.record, running)?;
    offline_setup::ensure_detached(&layout.disk())?;
    let machine = LiveMachine {
        app,
        id,
        cancel: &held.cancel,
    };
    let subject = checkpoints::Subject {
        layout: &layout,
        machine: &machine,
        running,
        macos_version: held.record.os_version(),
        computer_use_version: held.record.computer_use_version.clone(),
        developer_tools: held.record.setup.developer_tools,
        developer_tools_version: held.record.developer_tools_version.clone(),
        cpus: held.record.cpus,
        memory_gib: held.record.memory_gib,
        disk_gib: held.record.disk_gib,
        setup_fingerprint: held.record.setup_version.clone(),
    };
    let restored = checkpoints::restore(&subject, checkpoint_id);
    // The recovery checkpoint exists whether or not the rest succeeded.
    refresh_checkpoints(app, id, &layout);
    let pending = restored?;
    let (mut record, _) = computer(id)?;
    record.pending_restore = Some(pending);
    // Derive setup metadata from the restored disk, including older checkpoints with no tools
    // metadata. The saved computer must never claim tools that are absent from its disk.
    restore_checkpoint_metadata(&mut record, Some(&target));
    // The disk no longer is what the user left, and only what Start restores is the result.
    record.pristine = false;
    store::save(&layout, &record)?;
    // The record and the files agree: the journal has nothing left to settle.
    checkpoints::finish_restore(&layout);
    update(app, id, |entry| {
        entry.record = record;
        if matches!(entry.state, State::Running | State::Stopping) {
            entry.state = State::Stopped;
            entry.since = Instant::now();
        }
        entry.detail = None;
    });
    Ok(())
}

fn delete_checkpoint(
    app: &MacosContext,
    id: &str,
    checkpoint_id: &str,
    job: AcceptedMacosJob,
) -> Result<(), String> {
    require_supported()?;
    ensure_loaded(app)?;
    let data = app_data(app)?;
    let held = begin_operation(app, id, checkpoints::OperationKind::Delete, &job)?;
    let layout = Layout::new(&data, id);
    let target = checkpoints::find(&layout, checkpoint_id)?;
    if checkpoints::journal_pins(&layout, &target.id) {
        return Err(UNFINISHED_RESTORE.into());
    }
    if held
        .record
        .pending_restore
        .as_ref()
        .is_some_and(|pending| pending.checkpoint_id == target.id)
    {
        return Err("The next Start of this computer continues from this checkpoint. Start the computer first, or restore another checkpoint.".into());
    }
    let removed = checkpoints::remove(&layout, &target.id);
    refresh_checkpoints(app, id, &layout);
    if removed.is_ok() {
        log_line(
            app,
            id,
            &format!("checkpoint deleted: {} ({})", target.name, target.id),
        );
    }
    removed
}

/// Creates a stopped computer with a copy of a checkpoint's disk, and sets it up in the
/// background: it gets its own password, keys and name, like a copy of a template.
type ForkWorkflow = Box<
    dyn FnOnce(
            &MacosContext,
            &Layout,
            &mut Record,
            &AtomicU8,
            Option<templates::SpaceReservation>,
        ) -> Result<(), Stop>
        + Send
        + 'static,
>;

struct ForkContinuation {
    data: PathBuf,
    record: Record,
    cancel: Arc<AtomicU8>,
    space: templates::SpaceReservation,
    gate: &'static runtime::operation_gate::OperationGate,
    workflow: ForkWorkflow,
}

fn fork_checkpoint(
    app: &MacosContext,
    id: &str,
    checkpoint_id: &str,
    new_name: &str,
    job: AcceptedMacosJob,
) -> Result<ForkContinuation, String> {
    require_supported()?;
    ensure_loaded(app)?;
    let data = app_data(app)?;
    fork_checkpoint_with_workflow(
        app,
        &data,
        id,
        checkpoint_id,
        new_name,
        job,
        &runtime::OPERATIONS,
        run_setup,
    )
}

fn fork_checkpoint_with_workflow(
    app: &MacosContext,
    data: &std::path::Path,
    id: &str,
    checkpoint_id: &str,
    new_name: &str,
    job: AcceptedMacosJob,
    gate: &'static runtime::operation_gate::OperationGate,
    workflow: impl FnOnce(
            &MacosContext,
            &Layout,
            &mut Record,
            &AtomicU8,
            Option<templates::SpaceReservation>,
        ) -> Result<(), Stop>
        + Send
        + 'static,
) -> Result<ForkContinuation, String> {
    let new_name = new_name.trim().to_string();
    // Held until the fork is registered and saved, so a Linux creation sees it.
    let reservation =
        crate::computer_names::reserve(&[new_name.clone()], &|| app.linux_computer_names())?;
    let held = begin_operation(app, id, checkpoints::OperationKind::Fork, &job)?;
    let source = Layout::new(data, id);
    let checkpoint = checkpoints::find(&source, checkpoint_id)?;
    let space = templates::reserve_space(data, templates::COPY_ESTIMATE)?;
    let prepared: Result<(Record, Arc<AtomicU8>), String> = (|| {
        let _admitted = admission()?;
        let mut registry = registry();
        let existing: Vec<Record> = registry.entries.iter().map(|e| e.record.clone()).collect();
        let request = CreateRequest {
            name: new_name,
            cpus: held.record.cpus,
            memory_gib: held.record.memory_gib,
            disk_gib: held.record.disk_gib,
        };
        store::validate_request(&request, engine::host_limits(), &existing, 0)?;
        let mut record = store::new_record(&request, engine::random_mac());
        record.restore_image = held.record.restore_image.clone();
        record.pristine = false;
        record.setup_version = held.record.setup_version.clone();
        record.computer_use_version = checkpoint.computer_use_version.clone();
        record.computer_use_approval = held.record.computer_use_approval;
        record.computer_use_desired_approval = held.record.computer_use_desired_approval;
        // A crash before the files are copied leaves a failed computer that can be deleted.
        store::save(&Layout::new(&data, &record.id), &record)?;
        let entry = Entry::new(record.clone(), State::Copying, None);
        let cancel = entry.cancel.clone();
        registry.entries.push(entry);
        Ok((record, cancel))
    })();
    let (record, cancel) = match prepared {
        Ok(prepared) => prepared,
        Err(message) => return Err(message),
    };
    drop(reservation);
    let fork_id = record.id.clone();
    emit(app);
    let layout = Layout::new(data, &fork_id);
    log_line(
        app,
        id,
        &format!(
            "forking checkpoint \"{}\" ({}) into {}",
            checkpoint.name, checkpoint.id, record.name
        ),
    );
    let copied = copy_for_fork(&source, &checkpoint.id, &layout, record, &fork_id);
    // The source command owns the copy. The setup continuation gets its own gate job only
    // after the source guard is released, so the two scopes never nest on this thread.
    drop(held);
    match copied {
        Ok(record) => {
            emit(app);
            Ok(ForkContinuation {
                data: data.to_path_buf(),
                record,
                cancel,
                space,
                gate,
                workflow: Box::new(workflow),
            })
        }
        Err(stop) => {
            let message = match &stop {
                Stop::Failed(message) => message.clone(),
                Stop::Cancelled => "Creating the fork was cancelled.".to_string(),
            };
            end_workflow(app, &fork_id, &layout, Err(stop));
            Err(message)
        }
    }
}

/// The source checkpoint job is durable before this function runs. Accept the continuation's
/// own job before starting its worker; the worker enters the operation gate before it invokes
/// the personalization callback.
fn start_fork_continuation(app: &MacosContext, continuation: ForkContinuation) {
    let id = continuation.record.id.clone();
    let job = match accept_macos_job(app.engine.as_ref(), "checkpoints.fork", continuation.gate) {
        Ok(job) => job,
        Err(message) => {
            let layout = Layout::new(&continuation.data, &id);
            end_workflow(app, &id, &layout, Err(Stop::Failed(message)));
            return;
        }
    };
    let app = app.clone();
    std::thread::spawn(move || {
        fork_workflow(
            &app,
            &continuation.data,
            continuation.record,
            &continuation.cancel,
            continuation.space,
            job,
            continuation.workflow,
        )
    });
}

/// Clones the checkpoint into the fork's folder and publishes the fork as installed.
fn copy_for_fork(
    source: &Layout,
    checkpoint: &str,
    layout: &Layout,
    mut record: Record,
    fork_id: &str,
) -> Result<Record, Stop> {
    if !registry().creation_continues(fork_id) {
        return Err(Stop::Cancelled);
    }
    checkpoints::clone_for_fork(
        source,
        checkpoint,
        layout,
        &engine::new_machine_identifier(),
    )
    .map_err(|error| Stop::Failed(error.message()))?;
    record.installed = true;
    record.inherited_access = true;
    record.setup = store::SetupProgress {
        account: true,
        sip: true,
        developer_tools: false,
        computer_use: true,
        clipboard: true,
        roots: false,
        needs_personalizing: true,
    };
    // Persist while the fork is still in its creation state, where nothing else can start it.
    if !registry().creation_continues(fork_id) {
        return Err(Stop::Cancelled);
    }
    store::save(layout, &record)?;
    if !registry().publish_installed(fork_id, &record, State::SettingUp) {
        return Err(Stop::Cancelled);
    }
    Ok(record)
}

fn fork_workflow(
    app: &MacosContext,
    data: &std::path::Path,
    mut record: Record,
    cancel: &AtomicU8,
    space: templates::SpaceReservation,
    job: AcceptedMacosJob,
    workflow: ForkWorkflow,
) {
    let id = record.id.clone();
    let layout = Layout::new(data, &id);

    let _operation = match admit_and_track_computer_workflow(
        app.engine.as_ref(),
        &job,
        &id,
        &record.name,
        "Setting up forked Mac",
    ) {
        Ok(operation) => operation,
        Err(message) => {
            untrack_macos_job(&id, &job.job_id);
            let result = if job.cancellation.load(Ordering::SeqCst) {
                Stop::Cancelled
            } else {
                Stop::Failed(message)
            };
            end_workflow(app, &id, &layout, Err(result));
            return;
        }
    };
    let result = workflow(app, &layout, &mut record, cancel, Some(space));
    end_workflow(app, &id, &layout, result);
}

// MARK: Display

fn open_display(app: &MacosContext, id: &str) -> Result<(), String> {
    require_supported()?;
    let (record, state) = computer(id)?;
    if state != State::Running {
        return Err("Start the computer to open its display.".into());
    }
    show_display(app, id, &record.name, true)
}

/// Opens the window that shows a running computer's screen, or brings the
/// existing one forward.
/// `toolbar` adds the clipboard buttons, which belong to the user's own window and not to the
/// window of a setup that types into the computer itself.
fn show_display(app: &MacosContext, id: &str, title: &str, toolbar: bool) -> Result<(), String> {
    app.show_display(id, title, toolbar)?;
    update(app, id, |entry| entry.display_open = true);
    Ok(())
}

fn close_display(app: &MacosContext, id: &str) {
    app.close_display(id);
}

/// Attaches the VM view to a native window supplied by the host's UI adapter.
pub fn attach_display(
    context: &MacosContext,
    id: &str,
    resolve_native_window: impl FnOnce() -> Result<usize, String> + Send + 'static,
    toolbar: bool,
) -> Result<(), String> {
    engine::attach_display(context, id, resolve_native_window, toolbar)
}

/// Called by the host when its display window closes.
pub fn display_window_closed(context: &MacosContext, id: &str) {
    engine::detach_display(context, id);
    update(context, id, |entry| entry.display_open = false);
}

// MARK: Watching

/// Notices machine states the framework did not report through its delegate: a
/// stop the guest declined, and a machine that ended without a callback.
fn watch(app: &MacosContext) {
    static STARTED: Once = Once::new();
    let app = app.clone();
    STARTED.call_once(move || {
        std::thread::spawn(move || loop {
            std::thread::sleep(WATCH_INTERVAL);
            if let Ok(states) = engine::machine_samples(&app) {
                reconcile(&app, &states);
            }
        });
    });
}

fn reconcile(app: &MacosContext, samples: &[engine::Sample]) {
    for sample in samples {
        let (id, machine, generation, lifecycle_managed) = (
            &sample.id,
            &sample.state,
            sample.generation,
            sample.lifecycle_managed,
        );
        let Some((_, state)) = computer(id).ok() else {
            continue;
        };
        match (machine, state) {
            (
                engine::MachineState::Stopped | engine::MachineState::Failed,
                State::Running | State::Stopping,
            ) => {
                let detail = (*machine == engine::MachineState::Failed)
                    .then(|| "The computer stopped unexpectedly.".to_string());
                machine_stopped_with_lifecycle(app, id, generation, detail, lifecycle_managed);
            }
            (engine::MachineState::Paused, State::Running | State::Stopping) => {
                let (operating, unconsumed) = (has_operation(id), has_pending_restore(id));
                match paused_action(operating, unconsumed) {
                    PausedAction::Leave => {}
                    PausedAction::Resume => {
                        let owned = {
                            let id = id.clone();
                            move || has_operation(&id) || has_pending_restore(&id)
                        };
                        let resumed = engine::resume_stray(app, id, generation, owned);
                        update(app, id, |entry| {
                            entry.detail = resumed.err().map(|why| {
                                format!("The computer is paused and could not be resumed ({why}). Use Force stop.")
                            });
                        });
                    }
                    // Memory that was never marked as used must not run: the machine is
                    // turned off, and the computer stays busy until it is released.
                    PausedAction::ForceStop => {
                        let claimed = registry().begin_force_stop(id);
                        emit(app);
                        if claimed.is_ok() {
                            if let Err(why) = engine::force_stop_generation(app, id, generation) {
                                force_stop_failed(app, id, why);
                            }
                        }
                    }
                }
            }
            (engine::MachineState::Running, State::Stopping) => {
                update(app, id, |entry| {
                    if entry.state == State::Stopping && entry.since.elapsed() > STOP_IGNORED_AFTER
                    {
                        entry.state = State::Running;
                        entry.since = Instant::now();
                    }
                });
            }
            _ => {}
        }
    }
    // A stop whose machine is already gone is finished here: the callback that would have
    // done it has already run, or never will.
    let held: Vec<String> = samples.iter().map(|sample| sample.id.clone()).collect();
    // Checked again, and applied, on the main thread where machines are registered, so a machine
    // that appeared since the sample is never mistaken for a missing one. A query that fails
    // settles nothing.
    settle_absent(&REGISTRY, &held, &|id| {
        let settle = {
            let (app, id) = (app.clone(), id.to_string());
            move || stop_finished_without_machine(&app, &id)
        };
        let _ = engine::run_if_no_machine(app, id, settle);
    });
}

/// Hands each stopping computer without a machine to `dispatch`. The registry is not held while
/// `dispatch` runs: it waits for the main thread, and the main thread's callbacks take the
/// registry's lock.
fn settle_absent(registry: &Mutex<Registry>, held: &[String], dispatch: &dyn Fn(&str)) {
    let stalled = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .stopping_without_machine(held);
    for id in stalled {
        dispatch(&id);
    }
}

/// Finishes a stop whose machine is gone: there is no slot left to release.
fn stop_finished_without_machine(app: &MacosContext, id: &str) {
    update(app, id, |entry| {
        if entry.state == State::Stopping && entry.operation.is_none() {
            entry.state = State::Stopped;
            entry.progress = None;
            entry.display_open = false;
            entry.since = Instant::now();
        }
    });
}

/// What the watcher does with a machine the framework reports as paused.
#[derive(Debug, PartialEq, Eq)]
enum PausedAction {
    Leave,
    Resume,
    ForceStop,
}

fn paused_action(operating: bool, pending_restore_unconsumed: bool) -> PausedAction {
    if operating {
        PausedAction::Leave
    } else if pending_restore_unconsumed {
        PausedAction::ForceStop
    } else {
        PausedAction::Resume
    }
}

fn has_pending_restore(id: &str) -> bool {
    registry()
        .entries
        .iter()
        .any(|entry| entry.record.id == id && entry.record.pending_restore.is_some())
}

// MARK: Quit

/// Asks a creation or setup to end and keep the computer, unless a Delete already asked it to end and remove it.
fn request_abort(cancel: &AtomicU8) {
    let _ = cancel.compare_exchange(RUN, ABORT_AND_KEEP, Ordering::SeqCst, Ordering::SeqCst);
}

/// A step that failed after a Delete was accepted ended because of the Delete.
fn classify(result: Result<(), Stop>, flag: u8) -> Result<(), Stop> {
    match result {
        Err(Stop::Failed(_)) if flag == CANCEL_AND_REMOVE => Err(Stop::Cancelled),
        other => other,
    }
}

/// States of a creation or setup, which Quit ends and Delete cancels.
fn is_creating(state: State) -> bool {
    matches!(
        state,
        State::Preparing
            | State::Copying
            | State::Downloading
            | State::Installing
            | State::SettingUp
    )
}

fn is_busy(state: State) -> bool {
    !matches!(state, State::Stopped | State::Failed)
}

/// Stops every macOS computer for Quit: a graceful request first, then a forced
/// stop. Runs on a worker thread; the main thread must stay free to run the
/// framework's callbacks.
pub fn stop_all(app: &MacosContext, deadline: Option<Instant>) -> Result<(), String> {
    let stopped = stop_busy(app, deadline);
    // A host-attached disk image outlives Silo; none may be left behind.
    let detached = release_disks(app);
    stopped.and(detached)
}

/// Refuses new macOS work and asks provisioning and checkpoint operations already in flight to
/// release their ownership before Quit waits for the device-wide operation guard.
pub(crate) fn prepare_stop_all() {
    closed().quit = true;
    for entry in &registry().entries {
        abort_workflow_for_quit(entry.state, entry.operation.is_some(), &entry.cancel);
    }
}

fn abort_workflow_for_quit(state: State, has_operation: bool, cancel: &AtomicU8) {
    abort_for_quit(is_creating(state), has_operation, cancel);
}

pub(crate) fn abort_for_quit(is_workflow: bool, has_operation: bool, cancel: &AtomicU8) {
    if is_workflow || has_operation {
        request_abort(cancel);
    }
}

/// Detaches the disk image of every macOS computer on disk from this Mac, except
/// those a creation or setup still owns: their worker detaches its own image, and a
/// forced detach under it would corrupt the patch. Those report through `stop_busy`.
fn release_disks(app: &MacosContext) -> Result<(), String> {
    // Computers a previous launch left behind count even if nothing loaded them yet.
    let _ = ensure_loaded(app);
    let Ok(data) = app_data(app) else {
        return Ok(());
    };
    let owned: Vec<String> = registry()
        .entries
        .iter()
        .filter(|entry| is_busy(entry.state))
        .map(|entry| entry.record.id.clone())
        .collect();
    let mut result = Ok(());
    for id in disks_to_release(store::computer_ids(&data), &owned) {
        if let Err(message) = offline_setup::ensure_detached(&Layout::new(&data, &id).disk()) {
            result = Err(message);
        }
    }
    result
}

/// The computers whose disk Quit may detach: all of them but the ones a worker owns.
fn disks_to_release(ids: Vec<String>, owned: &[String]) -> Vec<String> {
    ids.into_iter().filter(|id| !owned.contains(id)).collect()
}

fn stop_busy(app: &MacosContext, deadline: Option<Instant>) -> Result<(), String> {
    // Also makes the admission close visible when Quit acquired the global guard first.
    prepare_stop_all();
    let busy: Vec<(String, State, Arc<AtomicU8>, bool)> = registry()
        .entries
        .iter()
        .filter(|entry| is_busy(entry.state) || entry.operation.is_some())
        .map(|entry| {
            (
                entry.record.id.clone(),
                entry.state,
                entry.cancel.clone(),
                entry.operation.is_some(),
            )
        })
        .collect();
    if busy.is_empty() {
        return Ok(());
    }
    let machines: Vec<&String> = busy
        .iter()
        .filter(|(_, state, _, _)| {
            matches!(state, State::Running | State::Starting | State::Stopping)
        })
        .map(|(id, _, _, _)| id)
        .collect();
    // A start in flight cannot be asked to stop, and a checkpoint operation ends at its next
    // step (a paused machine can't answer a shutdown); let both settle first.
    let settle = Instant::now() + Duration::from_secs(30);
    while busy
        .iter()
        .any(|(id, _, _, _)| state_of(id) == Some(State::Starting) || has_operation(id))
        && Instant::now() < settle
        && deadline.is_none_or(|deadline| Instant::now() < deadline)
    {
        std::thread::sleep(Duration::from_millis(250));
    }
    // Over SSH each guest shuts itself down without a prompt; the rest are asked through the
    // framework, as is every guest when too little time is left for SSH.
    let ssh_timeout = quit_shutdown_timeout(
        deadline.map(|deadline| deadline.saturating_duration_since(Instant::now())),
    );
    std::thread::scope(|scope| {
        for id in &machines {
            scope.spawn(move || match (ssh_timeout, computer(id)) {
                (Some(timeout), Ok((record, _))) => {
                    let _ = request_graceful_stop(app, &record, timeout);
                }
                _ => {
                    let _ = engine::request_stop(app, id);
                }
            });
        }
    });
    let limit = |wait: Duration| {
        let at = Instant::now() + wait;
        deadline.map_or(at, |deadline| at.min(deadline))
    };
    let all_stopped = |ids: &[&String]| ids.iter().all(|id| !is_active(id));
    let wait_until = |until: Instant, ids: &[&String]| {
        while !all_stopped(ids) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(250));
        }
        all_stopped(ids)
    };
    let everything: Vec<&String> = busy.iter().map(|(id, _, _, _)| id).collect();
    // Installations and setups were aborted; they release their machine (and, for a
    // setup, the disk image) before they stop being busy.
    if wait_until(graceful_until(Instant::now(), deadline), &machines)
        && wait_until(
            limit(FORCED_QUIT).min(graceful_until(Instant::now(), deadline)),
            &everything,
        )
    {
        return Ok(());
    }
    // A machine that ignored the request, or a setup stuck in a guest command, still holds its machine.
    for id in &everything {
        if state_of(id).is_some_and(|state| state != State::Stopped && state != State::Failed)
            && engine::machine_states(app)
                .map_or(true, |states| states.iter().any(|(held, _)| held == *id))
        {
            let _ = engine::force_stop(app, id);
        }
    }
    if wait_until(limit(FORCED_QUIT), &everything) {
        Ok(())
    } else {
        Err("A macOS computer did not stop in time.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silo_engine::context::{
        EngineIdentity, EnginePaths, EngineRuntimeState, MainThread, PackageKind, ProcessControl,
    };
    use std::sync::atomic::AtomicUsize;

    struct JobTestContext {
        paths: silo_engine::EnginePaths,
        identity: EngineIdentity,
        runtime_state: Arc<EngineRuntimeState>,
        published_events: std::sync::Mutex<Vec<Event>>,
    }

    struct JobTestHost;

    impl MacosHost for JobTestHost {
        fn show_display(
            &self,
            _context: &MacosContext,
            _id: &str,
            _title: &str,
            _toolbar: bool,
        ) -> Result<(), String> {
            Ok(())
        }

        fn close_display(&self, _id: &str) {}

        fn linux_computer_names(&self) -> Vec<String> {
            Vec::new()
        }
    }

    impl JobTestContext {
        fn new(app_data: PathBuf) -> Self {
            Self {
                paths: EnginePaths {
                    app_data,
                    ..EnginePaths::default()
                },
                identity: EngineIdentity {
                    channel: silo_engine::channel::Channel::Production,
                    version: "test".into(),
                    package_kind: PackageKind::MacApp,
                },
                runtime_state: Arc::new(EngineRuntimeState::default()),
                published_events: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl EngineContext for JobTestContext {
        fn paths(&self) -> &EnginePaths {
            &self.paths
        }

        fn identity(&self) -> &EngineIdentity {
            &self.identity
        }

        fn runtime_state(&self) -> Arc<EngineRuntimeState> {
            self.runtime_state.clone()
        }

        fn publish(&self, event: Event, _audience: Audience) {
            self.published_events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event);
        }

        fn main_thread(&self) -> Option<&dyn MainThread> {
            None
        }

        fn process(&self) -> &dyn ProcessControl {
            self
        }
    }

    impl ProcessControl for JobTestContext {
        fn exit(&self, _code: i32) {}

        fn restart(&self) -> ! {
            panic!("macOS job tests must not restart the process")
        }
    }

    fn isolated_operation_gate() -> &'static runtime::operation_gate::OperationGate {
        Box::leak(Box::new(runtime::operation_gate::OperationGate::new()))
    }

    fn macos_context(engine: Arc<dyn EngineContext>) -> MacosContext {
        MacosContext::new(engine, Arc::new(JobTestHost))
    }

    fn checkpoint_fixture(data: &std::path::Path, include_access: bool) -> (Record, String) {
        let mut source = store::new_record(
            &CreateRequest {
                name: "source-mac".into(),
                cpus: 2,
                memory_gib: 4,
                disk_gib: 64,
            },
            "02:00:00:00:00:01".into(),
        );
        source.installed = true;
        source.setup = store::SetupProgress {
            account: true,
            sip: true,
            developer_tools: true,
            computer_use: true,
            clipboard: true,
            roots: true,
            needs_personalizing: false,
        };
        source.developer_tools_version = Some(developer_tools::version());
        let source_layout = Layout::new(data, &source.id);
        store::save(&source_layout, &source).unwrap();
        std::fs::write(source_layout.disk(), b"source disk").unwrap();
        std::fs::write(source_layout.auxiliary_storage(), b"source auxiliary").unwrap();
        std::fs::write(source_layout.hardware_model(), b"source hardware").unwrap();
        if include_access {
            let source_access = guest_access::credentials_dir(&source_layout);
            std::fs::create_dir_all(&source_access).unwrap();
            std::fs::write(source_access.join("password"), b"fixture-password").unwrap();
            std::fs::write(source_access.join("id_ed25519"), b"fixture-private-key").unwrap();
            std::fs::write(source_access.join("id_ed25519.pub"), b"fixture-public-key").unwrap();
        }

        let checkpoint_id = uuid::Uuid::new_v4().to_string();
        let checkpoint_dir = checkpoints::dir(&source_layout, &checkpoint_id);
        std::fs::create_dir_all(&checkpoint_dir).unwrap();
        std::fs::write(checkpoint_dir.join("disk.img"), b"checkpoint disk").unwrap();
        std::fs::write(
            checkpoint_dir.join("auxiliary-storage.img"),
            b"checkpoint auxiliary",
        )
        .unwrap();
        let checkpoint_meta = checkpoints::Meta {
            schema_version: 1,
            id: checkpoint_id.clone(),
            name: "Before fork".into(),
            created_at: "2026-10-14T00:00:00Z".into(),
            kind: checkpoints::Kind::Disk,
            reason: checkpoints::Reason::Manual,
            macos_version: None,
            host_build: String::new(),
            size_bytes: None,
            cpus: Some(source.cpus),
            memory_gib: Some(source.memory_gib),
            disk_gib: Some(source.disk_gib),
            setup_fingerprint: source.setup_version.clone(),
            computer_use_version: source.computer_use_version.clone(),
            developer_tools: true,
            developer_tools_version: source.developer_tools_version.clone(),
        };
        std::fs::write(
            checkpoint_dir.join("checkpoint.json"),
            serde_json::to_vec(&checkpoint_meta).unwrap(),
        )
        .unwrap();
        (source, checkpoint_id)
    }

    #[test]
    fn import_cleanup_detaches_the_disk_before_removing_its_folder() {
        use std::cell::RefCell;

        let directory = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let root = store::root(directory.path());
        let stage = root.join(format!(".partial-import-{id}"));
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(stage.join("disk.img"), b"attached disk").unwrap();

        // The rekey could not detach: recovery must keep the folder and say so, so the caller
        // keeps its journal and the next launch tries again.
        let detached_when_asked = RefCell::new(Vec::new());
        let refuse = |disk: &std::path::Path| {
            detached_when_asked.borrow_mut().push(disk.exists());
            Err("The computer's disk is still attached to this Mac.".to_string())
        };
        let error = recover_archive_import_in(directory.path(), &id, &refuse).unwrap_err();
        assert!(error.contains("still attached"));
        assert!(
            stage.join("disk.img").exists(),
            "an attached disk was deleted"
        );
        assert_eq!(*detached_when_asked.borrow(), [true]);

        // Once the detach works, the folder goes, after the detach, and nothing was committed.
        let detached = RefCell::new(Vec::new());
        let accept = |disk: &std::path::Path| {
            detached.borrow_mut().push(disk.exists());
            Ok(())
        };
        assert!(!recover_archive_import_in(directory.path(), &id, &accept).unwrap());
        assert_eq!(*detached.borrow(), [true], "removal must follow the detach");
        assert!(!stage.exists());
        // Nothing left: recovery is repeatable and detaches nothing.
        assert!(!recover_archive_import_in(directory.path(), &id, &accept).unwrap());
        assert_eq!(detached.borrow().len(), 1);
    }

    #[test]
    fn import_cleanup_keeps_a_published_folder_whose_disk_will_not_detach() {
        let directory = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let root = store::root(directory.path());
        let stage = root.join(format!(".partial-import-{id}"));
        let published = root.join(&id);
        for folder in [&stage, &published] {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::write(folder.join("disk.img"), b"disk").unwrap();
        }
        let refuse = |_: &std::path::Path| Err("still attached".to_string());
        // No record proves the rename committed, so both folders are leftovers.
        assert!(recover_archive_import_in(directory.path(), &id, &refuse).is_err());
        assert!(stage.join("disk.img").exists());
        assert!(published.join("disk.img").exists());
        let detached = std::cell::RefCell::new(Vec::new());
        let accept = |disk: &std::path::Path| {
            detached.borrow_mut().push(disk.to_path_buf());
            Ok(())
        };
        assert!(!recover_archive_import_in(directory.path(), &id, &accept).unwrap());
        assert_eq!(detached.borrow().len(), 2);
        assert!(!stage.exists() && !published.exists());
    }

    #[test]
    fn import_cleanup_of_a_committed_import_detaches_its_leftover_staging_first() {
        let directory = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let root = store::root(directory.path());
        let stage = root.join(format!(".partial-import-{id}"));
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(stage.join("disk.img"), b"disk").unwrap();
        let mut record = store::new_record(
            &CreateRequest {
                name: "imported".into(),
                cpus: 2,
                memory_gib: 4,
                disk_gib: 64,
            },
            "02:00:00:00:00:09".into(),
        );
        record.id = id.clone();
        store::save(&Layout::new(directory.path(), &id), &record).unwrap();

        let refuse = |_: &std::path::Path| Err("still attached".to_string());
        assert!(recover_archive_import_in(directory.path(), &id, &refuse).is_err());
        assert!(stage.join("disk.img").exists());
        assert!(root.join(&id).join("computer.json").exists());
        let accept = |_: &std::path::Path| Ok(());
        assert!(recover_archive_import_in(directory.path(), &id, &accept).unwrap());
        assert!(!stage.exists());
        assert!(root.join(&id).join("computer.json").exists());
    }

    #[test]
    fn interrupted_export_captures_are_found_by_their_operation_and_removed() {
        let directory = tempfile::tempdir().unwrap();
        let root = store::root(directory.path());
        std::fs::create_dir_all(&root).unwrap();
        let operation = uuid::Uuid::new_v4().to_string();
        let other = uuid::Uuid::new_v4().to_string();

        // Process termination skips the destructors that would remove a capture.
        let mut left = Vec::new();
        for token in [&operation, &operation, &other] {
            let capture = export_capture_dir(directory.path(), Some(token)).unwrap();
            std::fs::write(capture.path().join("disk.img"), b"cloned disk").unwrap();
            std::fs::write(capture.path().join("state.vzvmsave"), b"saved memory").unwrap();
            left.push(capture.keep());
        }
        // A computer folder and a file that merely resembles a capture are never touched.
        let computer = root.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&computer).unwrap();
        let lookalike = root.join(format!(".silo-macos-export-{operation}-file"));
        std::fs::write(&lookalike, b"not a directory").unwrap();

        remove_export_captures(directory.path(), &operation).unwrap();
        assert!(!left[0].exists() && !left[1].exists());
        assert!(
            left[2].join("disk.img").exists(),
            "another operation's capture was removed"
        );
        assert!(computer.exists());
        assert!(lookalike.exists());
        // Reconciling again, or on a folder that never existed, is harmless.
        remove_export_captures(directory.path(), &operation).unwrap();
        remove_export_captures(&directory.path().join("missing"), &operation).unwrap();
        assert!(remove_export_captures(directory.path(), "../escape").is_err());
    }

    #[test]
    fn a_live_export_capture_is_named_for_its_operation_and_removed_on_drop() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(store::root(directory.path())).unwrap();
        let operation = uuid::Uuid::new_v4().to_string();
        let capture = export_capture_dir(directory.path(), Some(&operation)).unwrap();
        let path = capture.path().to_path_buf();
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(&format!(".silo-macos-export-{operation}-")));
        drop(capture);
        assert!(!path.exists());
        assert!(export_capture_dir(directory.path(), Some("not-an-id")).is_err());
    }

    #[test]
    fn archive_resources_are_checked_against_this_macs_limits() {
        let config = |cpus, memory_gib, disk_gib| crate::backup::MacosArchiveConfiguration {
            payload_version: 1,
            architecture: "aarch64".into(),
            source_computer_id: uuid::Uuid::new_v4().to_string(),
            source_name: "source".into(),
            cpus,
            memory_gib,
            disk_gib,
            guest_version: "26.0".into(),
            guest_build: "25A1".into(),
            host_build: "25F1".into(),
            hardware_model_digest: String::new(),
            setup_fingerprint: String::new(),
            computer_use_fingerprint: String::new(),
            source: crate::backup::MacosArchiveSourceKind::WholeComputer,
            memory: crate::backup::MacosArchiveMemory::None,
            components: Vec::new(),
        };
        let small_mac = store::HostLimits {
            cpus: 4,
            memory_gib: 16,
        };
        assert!(check_archive_resources(small_mac, &config(4, 8, 64)).is_ok());
        // Twelve CPUs or 64 GiB of memory do not fit this Mac; neither does a leaner Mac's disk floor.
        assert!(check_archive_resources(small_mac, &config(12, 8, 64))
            .unwrap_err()
            .contains("CPUs"));
        assert!(check_archive_resources(small_mac, &config(4, 64, 64))
            .unwrap_err()
            .contains("Memory"));
        assert!(check_archive_resources(small_mac, &config(4, 8, 8))
            .unwrap_err()
            .contains("Disk"));
    }

    #[test]
    fn run_markers_follow_the_registry_state_and_start_generation() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context);
        *PUBLISHED_RUNS.lock().unwrap() = None;
        let mut record = store::new_record(
            &CreateRequest {
                name: "marked".into(),
                cpus: 2,
                memory_gib: 4,
                disk_gib: 64,
            },
            "02:00:00:00:00:0a".into(),
        );
        record.installed = true;
        let id = record.id.clone();
        let layout = Layout::new(directory.path(), &id);
        store::save(&layout, &record).unwrap();
        {
            let mut computers = registry();
            let mut entry = Entry::new(record, State::Running, None);
            entry.attempt = 4;
            computers.entries = vec![entry];
            computers.loaded = true;
        }
        let current = || run_marker::current(directory.path(), &id, |_| true);

        sync_run_markers(&app);
        assert_eq!(current(), Some(4));

        registry().entries[0].state = State::Stopping;
        sync_run_markers(&app);
        assert_eq!(current(), None);

        let mut restarted = registry();
        restarted.entries[0].state = State::Running;
        restarted.entries[0].attempt = 5;
        drop(restarted);
        sync_run_markers(&app);
        assert_eq!(current(), Some(5));

        registry().entries.clear();
        *PUBLISHED_RUNS.lock().unwrap() = None;
    }

    #[test]
    fn macos_create_acceptance_precedes_first_restore_recovery() {
        let _test_state = crate::test_support::global_state();
        reopen_after_quit();
        reopen_after_update();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context.clone());
        {
            let mut computers = registry();
            computers.entries.clear();
            computers.loaded = false;
            computers.template = None;
            computers.min_disk_gib = store::MIN_DISK_GIB;
        }
        let (source, checkpoint_id) = checkpoint_fixture(directory.path(), true);
        let layout = Layout::new(directory.path(), &source.id);
        let restore_journal = layout.dir.join("restore-journal.json");
        std::fs::write(
            &restore_journal,
            format!(r#"{{"phase":"staging","target":"{checkpoint_id}","memory":false}}"#),
        )
        .unwrap();
        let job_registry = context.job_registry().unwrap();
        let connection = rusqlite::Connection::open(job_registry.path()).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_acceptance BEFORE INSERT ON jobs
                 BEGIN SELECT RAISE(ABORT, 'injected allocation failure'); END;",
            )
            .unwrap();
        drop(connection);

        let result = create_with_support_check(
            &app,
            CreateRequest {
                name: "new-mac".into(),
                cpus: 2,
                memory_gib: 4,
                disk_gib: 64,
            },
            || Ok(()),
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("injected allocation failure"));
        assert!(
            restore_journal.exists(),
            "failed job acceptance must not recover a journal"
        );
        assert!(
            !registry().loaded,
            "failed job acceptance must not load the computer inventory"
        );
        assert!(registry().entries.is_empty());
    }

    #[test]
    fn macos_remote_lifecycle_acceptance_precedes_first_restore_recovery() {
        let _test_state = crate::test_support::global_state();
        reopen_after_quit();
        reopen_after_update();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context.clone());
        {
            let mut computers = registry();
            computers.entries.clear();
            computers.loaded = false;
            computers.template = None;
            computers.min_disk_gib = store::MIN_DISK_GIB;
        }
        let (source, checkpoint_id) = checkpoint_fixture(directory.path(), true);
        let layout = Layout::new(directory.path(), &source.id);
        let restore_journal = layout.dir.join("restore-journal.json");
        std::fs::write(
            &restore_journal,
            format!(r#"{{"phase":"staging","target":"{checkpoint_id}","memory":false}}"#),
        )
        .unwrap();
        let job_registry = context.job_registry().unwrap();
        let connection = rusqlite::Connection::open(job_registry.path()).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_acceptance BEFORE INSERT ON jobs
                 BEGIN SELECT RAISE(ABORT, 'injected allocation failure'); END;",
            )
            .unwrap();

        let result = action_for_remote(&app, &source.id, Action::Stop);

        assert!(
            result
                .as_ref()
                .is_err_and(|message| message.contains("injected allocation failure")),
            "unexpected remote Stop result: {result:?}"
        );
        assert!(
            restore_journal.exists(),
            "failed lifecycle job acceptance must not recover a Restore journal"
        );
        assert!(
            !registry().loaded,
            "failed lifecycle job acceptance must not load the computer inventory"
        );
        assert!(registry().entries.is_empty());
    }

    #[test]
    fn macos_checkpoint_jobs_wait_on_the_computer_gate_and_cancel_before_admission() {
        let _test_state = crate::test_support::global_state();
        reopen_after_quit();
        reopen_after_update();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context.clone());
        {
            let mut computers = registry();
            computers.entries.clear();
            computers.loaded = false;
            computers.template = None;
            computers.min_disk_gib = store::MIN_DISK_GIB;
        }
        MACOS_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
        MACOS_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
        MACOS_PENDING_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
        MACOS_PENDING_FORK_WORKFLOWS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
        let (source, _) = checkpoint_fixture(directory.path(), true);
        let source_id = source.id.clone();
        ensure_loaded(&app).unwrap();

        let gate = isolated_operation_gate();
        let blocker = gate
            .kind(runtime::operation_gate::OperationKind::Lifecycle)
            .acquire(
                runtime::operation_gate::Scope::Computer {
                    id: source_id.clone(),
                },
                Some(source.name),
                "Holding source Mac",
                None,
            )
            .unwrap();
        let job = accept_macos_job(context.as_ref(), "checkpoints.create", gate).unwrap();
        let job_id = job.job_id.clone();
        let cancellation = job.cancellation.clone();
        let worker_app = app.clone();
        let worker = std::thread::spawn(move || {
            begin_operation(
                &worker_app,
                &source_id,
                checkpoints::OperationKind::Capture,
                &job,
            )
            .map(|held| drop(held))
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        let waiting_entry = loop {
            let snapshot = gate.snapshot();
            if let Some(entry) = snapshot
                .waiting
                .into_iter()
                .find(|entry| entry.job_id.as_ref() == Some(&job_id))
            {
                break Some(entry);
            }
            if worker.is_finished() || Instant::now() >= deadline {
                break None;
            }
            std::thread::yield_now();
        };
        let queued_on_expected_scope = waiting_entry.as_ref().is_some_and(|entry| {
            entry.kind == runtime::operation_gate::OperationKind::CheckpointCapture
                && entry.computer_id.as_deref() == Some(source.id.as_str())
        });
        let cancel_result = waiting_entry
            .as_ref()
            .map(|_| job_id.clone())
            .map(|id| context.job_registry().unwrap().cancel(&id));
        drop(blocker);
        let operation_result = worker.join().unwrap();
        let completion_result: Result<(), String> = operation_result;
        publish_checkpoint_completion(
            &app,
            job_id.clone(),
            &source.id,
            &completion_result,
            cancellation.load(Ordering::SeqCst),
        );
        let finished = context
            .job_registry()
            .unwrap()
            .get(&job_id)
            .unwrap()
            .unwrap();
        let no_waiter_remains = !gate
            .snapshot()
            .waiting
            .iter()
            .any(|entry| entry.job_id.as_ref() == Some(&job_id));
        {
            let mut computers = registry();
            computers.entries.clear();
            computers.loaded = false;
            computers.template = None;
            computers.min_disk_gib = store::MIN_DISK_GIB;
        }
        MACOS_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();

        assert!(
            queued_on_expected_scope,
            "checkpoint capture must wait in its typed, computer-scoped queue"
        );
        assert!(matches!(cancel_result, Some(Ok(()))));
        assert!(no_waiter_remains, "cancel must remove the waiting job");
        assert_eq!(finished.state, silo_api::jobs::JobState::Finished);
        let outcome = finished.outcome.unwrap();
        assert_eq!(outcome.result, None);
        assert_eq!(
            outcome.error.unwrap().code,
            silo_api::jobs::ErrorCode::Cancelled
        );
    }

    #[test]
    fn macos_checkpoint_operations_use_typed_computer_scopes_while_running() {
        let _test_state = crate::test_support::global_state();
        reopen_after_quit();
        reopen_after_update();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context.clone());
        {
            let mut computers = registry();
            computers.entries.clear();
            computers.loaded = false;
            computers.template = None;
            computers.min_disk_gib = store::MIN_DISK_GIB;
        }
        MACOS_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
        let (source, _) = checkpoint_fixture(directory.path(), true);
        ensure_loaded(&app).unwrap();

        let gate = isolated_operation_gate();
        let cases = [
            (
                checkpoints::OperationKind::Capture,
                "checkpoints.create",
                runtime::operation_gate::OperationKind::CheckpointCapture,
            ),
            (
                checkpoints::OperationKind::Restore,
                "checkpoints.restore",
                runtime::operation_gate::OperationKind::CheckpointRestore,
            ),
            (
                checkpoints::OperationKind::Fork,
                "checkpoints.fork",
                runtime::operation_gate::OperationKind::CheckpointFork,
            ),
            (
                checkpoints::OperationKind::Delete,
                "checkpoints.delete",
                runtime::operation_gate::OperationKind::CheckpointDelete,
            ),
        ];

        for (operation_kind, method, expected_gate_kind) in cases {
            let job = accept_macos_job(context.as_ref(), method, gate).unwrap();
            let job_id = job.job_id.clone();
            let held = begin_operation(&app, &source.id, operation_kind, &job).unwrap();
            let running = gate
                .snapshot()
                .running
                .into_iter()
                .find(|entry| entry.job_id.as_ref() == Some(&job_id))
                .expect("checkpoint operation must remain admitted while its guard is held");

            assert_eq!(running.kind, expected_gate_kind);
            assert_eq!(running.computer_id.as_deref(), Some(source.id.as_str()));
            assert!(
                !running.cancellable,
                "admitted checkpoint work is NotCancellable"
            );
            assert_eq!(
                job.registry.get(&job_id).unwrap().unwrap().state,
                silo_api::jobs::JobState::Running
            );

            drop(held);
            let result: Result<(), String> = Ok(());
            publish_checkpoint_completion(&app, job_id.clone(), &source.id, &result, false);
            assert_eq!(
                job.registry.get(&job_id).unwrap().unwrap().state,
                silo_api::jobs::JobState::Finished
            );
        }

        {
            let mut computers = registry();
            computers.entries.clear();
            computers.loaded = false;
            computers.template = None;
            computers.min_disk_gib = store::MIN_DISK_GIB;
        }
        MACOS_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
    }

    #[test]
    fn checkpoint_create_job_returns_the_created_checkpoint() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context.clone());
        let (source, checkpoint_id) = checkpoint_fixture(directory.path(), true);
        let meta =
            checkpoints::find(&Layout::new(directory.path(), &source.id), &checkpoint_id).unwrap();
        let job = accept_macos_job(
            context.as_ref(),
            "checkpoints.create",
            isolated_operation_gate(),
        )
        .unwrap();
        let job_id = job.job_id.clone();
        job.registry.mark_running(&job_id).unwrap();
        let result = Ok(meta);
        publish_checkpoint_create_completion(&app, job_id.clone(), &source.id, &result, false);

        let finished = job.registry.get(&job_id).unwrap().unwrap();
        assert_eq!(finished.state, silo_api::jobs::JobState::Finished);
        assert_eq!(
            finished.outcome.unwrap().result,
            Some(serde_json::json!({
                "checkpoint": {
                    "id": checkpoint_id,
                    "name": "Before fork",
                    "createdAt": "2026-10-14T00:00:00Z",
                    "scope": "disk",
                    "reason": "manual",
                    "kind": "manual"
                }
            }))
        );
    }

    #[test]
    fn creation_scope_narrows_atomically_and_wakes_unrelated_computer_waiters() {
        let directory = tempfile::tempdir().unwrap();
        let context = JobTestContext::new(directory.path().to_path_buf());
        let gate = isolated_operation_gate();
        let accepted = accept_macos_job(&context, "computers.create", gate).unwrap();
        let operation = admit_macos_job(
            &context,
            &accepted,
            runtime::operation_gate::OperationKind::ComputerConfiguration,
            runtime::operation_gate::Scope::Device,
            None,
            "Creating forked Mac",
        )
        .unwrap();

        let (same_started, same_started_rx) = std::sync::mpsc::channel();
        let same_gate = gate;
        let same_job = std::thread::spawn(move || {
            let _operation = same_gate
                .kind(runtime::operation_gate::OperationKind::Lifecycle)
                .acquire(
                    runtime::operation_gate::Scope::Computer {
                        id: "forked-id".into(),
                    },
                    Some("Forked Mac".into()),
                    "Starting forked Mac",
                    None,
                )
                .unwrap();
            same_started.send(()).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while !gate
            .snapshot()
            .waiting
            .iter()
            .any(|entry| entry.label == "Starting forked Mac")
        {
            assert!(
                Instant::now() < deadline,
                "same-computer waiter was not queued"
            );
            std::thread::yield_now();
        }

        let (unrelated_started, unrelated_started_rx) = std::sync::mpsc::channel();
        let (release_unrelated, release_unrelated_rx) = std::sync::mpsc::channel();
        let unrelated_gate = gate;
        let unrelated_job = std::thread::spawn(move || {
            let _operation = unrelated_gate
                .kind(runtime::operation_gate::OperationKind::Lifecycle)
                .acquire(
                    runtime::operation_gate::Scope::Computer {
                        id: "existing-id".into(),
                    },
                    Some("Existing Mac".into()),
                    "Starting existing Mac",
                    None,
                )
                .unwrap();
            unrelated_started.send(()).unwrap();
            release_unrelated_rx.recv().unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while !gate
            .snapshot()
            .waiting
            .iter()
            .any(|entry| entry.label == "Starting existing Mac")
        {
            assert!(Instant::now() < deadline, "unrelated waiter was not queued");
            std::thread::yield_now();
        }

        let before_narrow = gate.generations();
        assert!(operation.narrow_to_computer("forked-id", "Forked Mac"));
        assert_eq!(
            gate.generation("forked-id"),
            before_narrow.of("forked-id") + 2,
            "narrowing touches the old Device and new Computer generations"
        );
        assert_eq!(
            gate.generation("untouched-id"),
            before_narrow.of("untouched-id") + 1,
            "the old Device scope still invalidates other-computer reads"
        );
        let creator = gate
            .snapshot()
            .running
            .into_iter()
            .find(|entry| entry.job_id.as_ref() == Some(&accepted.job_id))
            .expect("creation keeps its same running queue entry");
        assert_eq!(creator.computer_id.as_deref(), Some("forked-id"));
        assert_eq!(creator.computer_name.as_deref(), Some("Forked Mac"));
        assert!(!creator.cancellable, "narrowing preserves cancel semantics");

        unrelated_started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("unrelated computer waiter should wake and pass");
        let queue = gate.snapshot();
        assert!(queue
            .waiting
            .iter()
            .any(|entry| entry.label == "Starting forked Mac"));
        assert!(!queue
            .running
            .iter()
            .any(|entry| entry.label == "Starting forked Mac"));

        drop(operation);
        same_started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("same-computer waiter should run after creation releases its scope");
        release_unrelated.send(()).unwrap();
        same_job.join().unwrap();
        unrelated_job.join().unwrap();
        assert!(gate.is_idle());
    }

    #[test]
    fn create_worker_narrows_device_scope_before_running_the_long_workflow() {
        let _test_state = crate::test_support::global_state();
        reopen_after_quit();
        reopen_after_update();
        {
            let mut computers = registry();
            computers.entries.clear();
            computers.loaded = false;
            computers.template = None;
            computers.min_disk_gib = store::MIN_DISK_GIB;
        }
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context.clone());
        ensure_loaded(&app).unwrap();
        let gate = isolated_operation_gate();
        let accepted = accept_macos_job(context.as_ref(), "computers.create", gate).unwrap();
        let job_id = accepted.job_id.clone();
        let (response, response_rx) = std::sync::mpsc::channel();
        let (workflow_started, workflow_started_rx) = std::sync::mpsc::channel();
        let (release_workflow, release_workflow_rx) = std::sync::mpsc::channel();
        let worker_app = app.clone();
        let worker_data = directory.path().to_path_buf();
        let worker = std::thread::spawn(move || {
            create_job_worker(
                &worker_app,
                worker_data,
                CreateRequest {
                    name: "created-mac".into(),
                    cpus: 2,
                    memory_gib: 4,
                    disk_gib: 64,
                },
                accepted,
                response,
                move |app, data, record, _cancel| {
                    workflow_started.send(record.id.clone()).unwrap();
                    release_workflow_rx.recv().unwrap();
                    end_workflow(
                        app,
                        &record.id,
                        &Layout::new(data, &record.id),
                        Err(Stop::Failed("test workflow stopped".into())),
                    );
                },
            )
        });
        let row = response_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("worker must register the computer before entering provisioning")
            .unwrap();
        let started_id = workflow_started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the injected workflow must hold the creation operation");
        assert_eq!(row.id, started_id);

        let (same_started, same_started_rx) = std::sync::mpsc::channel();
        let same_gate = gate;
        let same_id = row.id.clone();
        let same_name = row.name.clone();
        let same_job = std::thread::spawn(move || {
            let _operation = same_gate
                .kind(runtime::operation_gate::OperationKind::Lifecycle)
                .acquire(
                    runtime::operation_gate::Scope::Computer { id: same_id },
                    Some(same_name),
                    "Starting created Mac",
                    None,
                )
                .unwrap();
            same_started.send(()).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while !gate
            .snapshot()
            .waiting
            .iter()
            .any(|entry| entry.label == "Starting created Mac")
            && Instant::now() < deadline
        {
            std::thread::yield_now();
        }
        let same_queued = gate
            .snapshot()
            .waiting
            .iter()
            .any(|entry| entry.label == "Starting created Mac");

        let (unrelated_started, unrelated_started_rx) = std::sync::mpsc::channel();
        let (release_unrelated, release_unrelated_rx) = std::sync::mpsc::channel();
        let unrelated_gate = gate;
        let unrelated_job = std::thread::spawn(move || {
            let _operation = unrelated_gate
                .kind(runtime::operation_gate::OperationKind::Lifecycle)
                .acquire(
                    runtime::operation_gate::Scope::Computer {
                        id: "existing-computer".into(),
                    },
                    Some("Existing Mac".into()),
                    "Starting existing Mac during creation",
                    None,
                )
                .unwrap();
            unrelated_started.send(()).unwrap();
            release_unrelated_rx.recv().unwrap();
        });
        let unrelated_admitted_while_workflow_runs = unrelated_started_rx
            .recv_timeout(Duration::from_millis(250))
            .is_ok();
        let queue = gate.snapshot();
        let creator = queue
            .running
            .iter()
            .find(|entry| entry.job_id.as_ref() == Some(&job_id));
        let creator_scope_is_narrowed = creator.is_some_and(|entry| {
            entry.computer_id.as_deref() == Some(row.id.as_str())
                && entry.computer_name.as_deref() == Some(row.name.as_str())
        });
        let creator_remains_non_cancellable = creator.is_some_and(|entry| !entry.cancellable);
        let same_started_before_release = same_started_rx.try_recv().is_ok();
        let same_computer_waiter_stays_blocked = queue
            .waiting
            .iter()
            .any(|entry| entry.label == "Starting created Mac")
            && !same_started_before_release;

        release_workflow.send(()).unwrap();
        worker.join().unwrap();
        release_unrelated.send(()).unwrap();
        let same_admitted_after_workflow = same_started_before_release
            || same_started_rx.recv_timeout(Duration::from_secs(5)).is_ok();
        same_job.join().unwrap();
        let unrelated_finished = unrelated_job.join().is_ok();
        assert!(gate.is_idle());
        {
            let mut computers = registry();
            computers.entries.clear();
            computers.loaded = false;
            computers.template = None;
            computers.min_disk_gib = store::MIN_DISK_GIB;
        }
        assert!(same_queued, "same-computer waiter was not queued");
        assert!(
            unrelated_admitted_while_workflow_runs,
            "creation must narrow to its computer and wake unrelated lifecycle work"
        );
        assert!(
            same_computer_waiter_stays_blocked,
            "same-computer work must remain blocked during creation"
        );
        assert!(
            creator_scope_is_narrowed,
            "creator entry must name its computer"
        );
        assert!(
            creator_remains_non_cancellable,
            "creation remains cancellable through Delete"
        );
        assert!(
            same_admitted_after_workflow,
            "same-computer operation must run afterward"
        );
        assert!(
            unrelated_finished,
            "unrelated operation must finish after release"
        );
    }

    #[test]
    fn fork_workflow_job_records_setup_failure_without_overwriting_fork_command() {
        let _test_state = crate::test_support::global_state();
        reopen_after_quit();
        reopen_after_update();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context.clone());
        {
            let mut computers = registry();
            computers.entries.clear();
            computers.loaded = false;
            computers.template = None;
            computers.min_disk_gib = store::MIN_DISK_GIB;
        }

        let (source, checkpoint_id) = checkpoint_fixture(directory.path(), true);
        let source_id = source.id.clone();
        ensure_loaded(&app).unwrap();
        MACOS_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
        let gate = isolated_operation_gate();
        let command = accept_macos_job(context.as_ref(), "checkpoints.fork", gate).unwrap();
        let command_job_id = command.job_id.clone();
        let command_cancellation = command.cancellation.clone();
        let command_registry = command.registry.clone();
        let (workflow_started, workflow_started_rx) = std::sync::mpsc::channel();
        let (release_workflow, release_workflow_rx) = std::sync::mpsc::channel();
        let workflow_registry = command_registry.clone();
        let workflow_parent_id = command_job_id.clone();
        let continuation = fork_checkpoint_with_workflow(
            &app,
            directory.path(),
            &source_id,
            &checkpoint_id,
            "forked-mac",
            command,
            gate,
            move |_app, _layout, record, _cancel, _space| {
                let parent_state = workflow_registry
                    .get(&workflow_parent_id)
                    .unwrap()
                    .map(|job| job.state);
                workflow_started
                    .send((record.id.clone(), parent_state))
                    .unwrap();
                release_workflow_rx.recv().unwrap();
                Err(Stop::Failed("Personalization failed.".into()))
            },
        )
        .unwrap();

        let connection = rusqlite::Connection::open(command_registry.path()).unwrap();
        connection
            .execute_batch(&format!(
                "CREATE TRIGGER reject_fork_finish BEFORE UPDATE OF state ON jobs \
                 WHEN OLD.job_id = '{}' AND NEW.state = 'finished' \
                 BEGIN SELECT RAISE(ABORT, 'injected finish persistence failure'); END;",
                command_job_id.as_str()
            ))
            .unwrap();
        let source_result: Result<(), String> = Ok(());
        finish_fork_command(
            &app,
            command_job_id.clone(),
            &source_id,
            &source_result,
            command_cancellation.load(Ordering::SeqCst),
            Some(continuation),
        );
        let source_while_finish_is_blocked = command_registry.get(&command_job_id).unwrap();
        let accepted_jobs_while_finish_is_blocked: i64 = connection
            .query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            source_while_finish_is_blocked.unwrap().state,
            silo_api::jobs::JobState::Running
        );
        assert_eq!(
            accepted_jobs_while_finish_is_blocked, 1,
            "the fork continuation must not be accepted before source completion is durable"
        );
        assert!(workflow_started_rx.try_recv().is_err());
        connection
            .execute_batch("DROP TRIGGER reject_fork_finish;")
            .unwrap();
        retry_pending_checkpoint_completions(&app);

        let workflow_start = workflow_started_rx
            .recv_timeout(Duration::from_secs(5))
            .ok();
        let fork_id = workflow_start.as_ref().map(|(fork_id, _)| fork_id.clone());
        let parent_state_at_setup_start = workflow_start.map(|(_, state)| state);
        let fork_job_id = fork_id.as_ref().and_then(|fork_id| {
            MACOS_JOBS
                .get_or_init(Default::default)
                .lock()
                .unwrap()
                .get(fork_id)
                .cloned()
        });
        let fork_operation_is_tracked = fork_id.as_ref().is_some_and(|fork_id| {
            fork_job_id.as_ref().is_some_and(|fork_job_id| {
                gate.snapshot().running.into_iter().any(|entry| {
                    entry.job_id.as_ref() == Some(fork_job_id)
                        && entry.computer_id.as_deref() == Some(fork_id.as_str())
                        && entry.label == "Setting up forked Mac"
                })
            })
        });
        let command_result_before_setup_failure = command_registry.get(&command_job_id).unwrap();

        let _ = release_workflow.send(());
        let fork_result = if let Some(fork_job_id) = fork_job_id.as_ref() {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(result) = command_registry.get(fork_job_id).unwrap() {
                    if result.state == silo_api::jobs::JobState::Finished {
                        break Some(result);
                    }
                }
                if Instant::now() >= deadline {
                    break None;
                }
                std::thread::yield_now();
            }
        } else {
            None
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !gate.is_idle() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        let command_result_after_setup_failure = command_registry.get(&command_job_id).unwrap();

        {
            let mut computers = registry();
            computers.entries.clear();
            computers.loaded = false;
            computers.template = None;
            computers.min_disk_gib = store::MIN_DISK_GIB;
        }
        MACOS_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
        MACOS_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
        MACOS_PENDING_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
        MACOS_PENDING_FORK_WORKFLOWS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();

        let command_before = command_result_before_setup_failure
            .as_ref()
            .expect("source command must finish before personalization is released");
        assert_eq!(command_before.state, silo_api::jobs::JobState::Finished);
        assert_eq!(
            parent_state_at_setup_start,
            Some(Some(silo_api::jobs::JobState::Finished)),
            "the parent source result must be durable before personalization starts"
        );
        assert!(
            command_before.outcome.as_ref().unwrap().error.is_none(),
            "the copy command succeeds before personalization starts"
        );
        assert_eq!(
            command_before.outcome,
            command_result_after_setup_failure
                .as_ref()
                .expect("source command remains discoverable after personalization")
                .outcome,
            "fork personalization must not overwrite the source command outcome: {command_result_before_setup_failure:?} -> {command_result_after_setup_failure:?}"
        );
        assert_ne!(fork_job_id.as_ref(), Some(&command_job_id));
        assert!(
            fork_result.is_some(),
            "fork setup outcome was not persisted"
        );
        let error = fork_result.unwrap().outcome.unwrap().error.unwrap();
        assert_eq!(error.code, silo_api::jobs::ErrorCode::Internal);
        assert_eq!(error.message, "Personalization failed.");
        assert!(
            gate.is_idle(),
            "fork workflow must release its operation scope"
        );
        assert!(
            fork_operation_is_tracked,
            "fork workflow must enter the operation gate before setup"
        );
    }

    #[test]
    fn fork_copy_failure_finishes_the_source_command_without_a_setup_job() {
        let _test_state = crate::test_support::global_state();
        reopen_after_quit();
        reopen_after_update();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context.clone());
        {
            let mut computers = registry();
            computers.entries.clear();
            computers.loaded = false;
            computers.template = None;
            computers.min_disk_gib = store::MIN_DISK_GIB;
        }
        MACOS_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
        MACOS_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
        let (source, checkpoint_id) = checkpoint_fixture(directory.path(), false);
        let source_id = source.id.clone();
        ensure_loaded(&app).unwrap();
        let gate = isolated_operation_gate();
        let command = accept_macos_job(context.as_ref(), "checkpoints.fork", gate).unwrap();
        let command_job_id = command.job_id.clone();
        let command_cancellation = command.cancellation.clone();
        let command_registry = command.registry.clone();

        let result = fork_checkpoint_with_workflow(
            &app,
            directory.path(),
            &source_id,
            &checkpoint_id,
            "fork-without-source-credentials",
            command,
            gate,
            run_setup,
        );
        let message = match result {
            Err(message) => message,
            Ok(_) => panic!("copy must fail because the source credentials are absent"),
        };
        let source_result: Result<(), String> = Err(message.clone());
        finish_fork_command(
            &app,
            command_job_id.clone(),
            &source_id,
            &source_result,
            command_cancellation.load(Ordering::SeqCst),
            None,
        );

        let job_ids: Vec<String> = {
            let connection = rusqlite::Connection::open(command_registry.path()).unwrap();
            let mut statement = connection
                .prepare(
                    "SELECT job_id FROM jobs WHERE method = 'checkpoints.fork' ORDER BY accepted_order",
                )
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(job_ids, vec![command_job_id.as_str().to_owned()]);
        let command_result = command_registry.get(&command_job_id).unwrap().unwrap();
        assert_eq!(command_result.state, silo_api::jobs::JobState::Finished);
        let error = command_result.outcome.unwrap().error.unwrap();
        assert_eq!(error.code, silo_api::jobs::ErrorCode::Internal);
        assert!(error.message.contains("copy"));

        {
            let mut computers = registry();
            computers.entries.clear();
            computers.loaded = false;
            computers.template = None;
            computers.min_disk_gib = store::MIN_DISK_GIB;
        }
        MACOS_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
        MACOS_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
    }

    #[test]
    fn admission_persistence_failure_finishes_the_job_before_returning() {
        let directory = tempfile::tempdir().unwrap();
        let context = JobTestContext::new(directory.path().to_path_buf());
        let gate = isolated_operation_gate();
        let accepted = accept_macos_job(&context, "computers.start", gate).unwrap();
        let connection = rusqlite::Connection::open(accepted.registry.path()).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_running BEFORE UPDATE OF state ON jobs
                 WHEN NEW.state = 'running'
                 BEGIN SELECT RAISE(ABORT, 'injected mark_running failure'); END;",
            )
            .unwrap();
        drop(connection);

        let error = match admit_macos_job(
            &context,
            &accepted,
            runtime::operation_gate::OperationKind::Lifecycle,
            runtime::operation_gate::Scope::Device,
            None,
            "Starting Mac",
        ) {
            Ok(_) => panic!("injected persistence failure must reject admission"),
            Err(error) => error,
        };

        assert!(error.contains("injected mark_running failure"));
        let finished = accepted.registry.get(&accepted.job_id).unwrap().unwrap();
        assert_eq!(finished.state, silo_api::jobs::JobState::Finished);
        assert_eq!(
            finished.outcome.unwrap().error.unwrap().code,
            silo_api::jobs::ErrorCode::Internal
        );
    }

    #[test]
    fn failed_lifecycle_command_is_not_overridden_by_the_stopped_row_snapshot() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context.clone());
        let gate = isolated_operation_gate();
        let accepted = accept_macos_job(context.as_ref(), "computers.start", gate).unwrap();
        accepted.registry.mark_running(&accepted.job_id).unwrap();
        let computer_id = accepted.job_id.as_str().replace(':', "-");
        track_macos_job(&computer_id, accepted.job_id.clone());
        let state = MacosComputersState {
            supported: true,
            unsupported_reason: None,
            computers: vec![MacosComputer {
                id: computer_id.clone(),
                name: "Dev Mac".into(),
                cpus: 4,
                memory_gib: 8,
                disk_gib: 64,
                os_version: None,
                state: State::Stopped,
                progress: None,
                detail: None,
                display_open: false,
                installed: false,
                setup_complete: false,
                needs_personalizing: false,
                checkpoints: Vec::new(),
                checkpoint_operation: None,
                pending_restore: None,
            }],
            template: None,
            min_disk_gib: 1,
        };

        // A failed Start settles its row back to Stopped. A snapshot is progress only;
        // it must not finish the command successfully before its real result is recorded.
        publish_job_activity(&app, &state);
        let before_result = accepted.registry.get(&accepted.job_id).unwrap().unwrap();
        assert_eq!(before_result.state, silo_api::jobs::JobState::Running);

        finish_macos_job(
            context.as_ref(),
            Some(&computer_id),
            accepted.job_id.clone(),
            lifecycle_job_outcome(Err("The framework rejected the start.".into())),
        );
        let finished = accepted.registry.get(&accepted.job_id).unwrap().unwrap();
        assert_eq!(finished.state, silo_api::jobs::JobState::Finished);
        let error = finished.outcome.unwrap().error.unwrap();
        assert_eq!(error.code, silo_api::jobs::ErrorCode::Internal);
        assert_eq!(error.message, "The framework rejected the start.");
        assert_eq!(state_after_failed_start(Ok(())).0, State::Stopped);
    }

    #[test]
    fn stale_macos_progress_snapshot_is_not_published_after_job_finish() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let context = JobTestContext::new(directory.path().to_path_buf());
        let gate = isolated_operation_gate();
        let accepted = accept_macos_job(&context, "computers.start", gate).unwrap();
        accepted.registry.mark_running(&accepted.job_id).unwrap();
        let computer_id = format!("computer-for-{}", accepted.job_id);
        track_macos_job(&computer_id, accepted.job_id.clone());

        let stale = silo_api::jobs::JobProgress::Macos {
            job_id: accepted.job_id.clone(),
            kind: None,
            computer_id: Some(computer_id.clone()),
            data: silo_api::jobs::MacosJobProgress {
                computer_id: computer_id.clone(),
                state: silo_api::methods::computers::MacosComputerState::Starting,
                progress: Some(0.5),
                detail: None,
                checkpoint_operation: None,
            },
        };

        finish_macos_job(
            &context,
            Some(&computer_id),
            accepted.job_id.clone(),
            lifecycle_job_outcome(Ok(())),
        );
        publish_macos_job_progress(&context, &accepted.job_id, stale);

        let published = context
            .published_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let names = published
            .iter()
            .map(|event| event.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["operations.job.finished"]);
    }

    #[test]
    fn checkpoint_completion_removes_both_progress_guards_under_the_publication_lock() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context.clone());
        let accepted =
            crate::jobs::allocate(context.as_ref(), "computers.checkpoint.create").unwrap();
        context
            .job_registry()
            .unwrap()
            .mark_running(&accepted.job_id)
            .unwrap();
        let computer_id = "computer-checkpoint-completion-race".to_owned();
        let finished = macos_job_finished(accepted.job_id.clone(), Ok(()));
        MACOS_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(computer_id.clone(), accepted.job_id.clone());
        MACOS_PENDING_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(accepted.job_id.clone(), (computer_id, finished));

        let (at_gap_tx, at_gap_rx) = std::sync::mpsc::channel();
        let (resume_completion_tx, resume_completion_rx) = std::sync::mpsc::channel();
        let completion_app = app.clone();
        let completion = std::thread::spawn(move || {
            retry_pending_checkpoint_completions_with_hook(&completion_app, || {
                // This is the reviewed gap: the pending guard has been removed,
                // but the active checkpoint mapping is the last progress guard.
                at_gap_tx.send(()).unwrap();
                resume_completion_rx.recv().unwrap();
            });
        });
        at_gap_rx.recv().unwrap();

        let stale = silo_api::jobs::JobProgress::Macos {
            job_id: accepted.job_id.clone(),
            kind: None,
            computer_id: Some("computer-checkpoint-completion-race".into()),
            data: silo_api::jobs::MacosJobProgress {
                computer_id: "computer-checkpoint-completion-race".into(),
                state: silo_api::methods::computers::MacosComputerState::Starting,
                progress: Some(0.5),
                detail: None,
                checkpoint_operation: None,
            },
        };
        let (progress_waiting_tx, progress_waiting_rx) = std::sync::mpsc::channel();
        let (allow_progress_tx, allow_progress_rx) = std::sync::mpsc::channel();
        let (progress_done_tx, progress_done_rx) = std::sync::mpsc::channel();
        let progress_context = context.clone();
        let progress_job = accepted.job_id.clone();
        let progress = std::thread::spawn(move || {
            publish_macos_job_progress_with_hook(
                progress_context.as_ref(),
                &progress_job,
                stale,
                || {
                    progress_waiting_tx.send(()).unwrap();
                    allow_progress_rx.recv().unwrap();
                },
            );
            progress_done_tx.send(()).unwrap();
        });
        progress_waiting_rx.recv().unwrap();
        let publication_lock_held = match MACOS_JOB_EVENT_PUBLISH_LOCK.try_lock() {
            Ok(guard) => {
                drop(guard);
                false
            }
            Err(std::sync::TryLockError::WouldBlock) => true,
            Err(std::sync::TryLockError::Poisoned(_)) => {
                panic!("the macOS job publication lock was poisoned")
            }
        };
        allow_progress_tx.send(()).unwrap();
        // In the reviewed ordering the lock is free, so wait for stale progress
        // to publish before allowing active-map cleanup to continue. In the fix,
        // it is held across both removals and the progress thread waits on it.
        if !publication_lock_held {
            progress_done_rx.recv().unwrap();
        }
        resume_completion_tx.send(()).unwrap();
        completion.join().unwrap();
        progress.join().unwrap();

        let names = context
            .published_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|event| event.name.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec!["operations.job.finished"],
            "stale checkpoint progress must not publish after the terminal result"
        );
        assert!(
            publication_lock_held,
            "checkpoint result publication and both active-map removals must share the progress lock"
        );
        assert!(!MACOS_PENDING_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&accepted.job_id));
        assert!(!MACOS_CHECKPOINT_JOBS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .any(|job_id| job_id == &accepted.job_id));
    }

    #[test]
    fn generic_cancel_of_admitted_creation_does_not_delete_it() {
        let directory = tempfile::tempdir().unwrap();
        let context = JobTestContext::new(directory.path().to_path_buf());
        let gate = isolated_operation_gate();
        let accepted =
            accept_macos_job(&context, "computers.create", gate).expect("job should be accepted");
        let _operation = admit_macos_job(
            &context,
            &accepted,
            runtime::operation_gate::OperationKind::ComputerConfiguration,
            runtime::operation_gate::Scope::Device,
            None,
            "Creating Mac",
        )
        .expect("job should be admitted");

        let error = accepted.registry.cancel(&accepted.job_id).unwrap_err();

        assert_eq!(error.message(), "This operation can't be cancelled.");
        assert!(!accepted.cancellation.load(Ordering::SeqCst));
        assert_eq!(
            store::delete_mode(State::Preparing),
            Ok(DeleteMode::Cancel),
            "creation cancellation remains available through computers.delete"
        );
    }

    #[test]
    fn a_waiting_macos_job_can_be_cancelled_before_its_effect() {
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let gate = isolated_operation_gate();
        let _blocker = gate
            .kind(runtime::operation_gate::OperationKind::Other)
            .acquire(
                runtime::operation_gate::Scope::Device,
                None,
                "Blocking test operation",
                None,
            )
            .unwrap();
        let accepted = accept_macos_job(context.as_ref(), "computers.create", gate).unwrap();
        let job_id = accepted.job_id.clone();
        let effects = Arc::new(AtomicUsize::new(0));
        let worker_context = context.clone();
        let worker_effects = effects.clone();
        let worker = std::thread::spawn(move || {
            let operation = admit_macos_job(
                worker_context.as_ref(),
                &accepted,
                runtime::operation_gate::OperationKind::ComputerConfiguration,
                runtime::operation_gate::Scope::Device,
                None,
                "Creating Mac",
            );
            if operation.is_ok() {
                worker_effects.fetch_add(1, Ordering::SeqCst);
            }
            operation.map(drop)
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        while !gate
            .snapshot()
            .waiting
            .iter()
            .any(|entry| entry.job_id.as_ref() == Some(&job_id))
        {
            assert!(
                Instant::now() < deadline,
                "macOS job did not enter the wait queue"
            );
            std::thread::yield_now();
        }
        context
            .job_registry()
            .unwrap()
            .cancel(&job_id)
            .expect("waiting job cancellation should be accepted");

        let error = worker.join().unwrap().unwrap_err();
        assert_eq!(error, "The operation was cancelled.");
        assert_eq!(effects.load(Ordering::SeqCst), 0);
        assert!(!gate.contains_job(&job_id));
        let result = context
            .job_registry()
            .unwrap()
            .get(&job_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            result.outcome.unwrap().error.unwrap().code,
            silo_api::jobs::ErrorCode::Cancelled
        );
    }

    #[test]
    fn macos_transfer_turn_waits_with_stop_restore_and_export_then_holds_the_scope() {
        use runtime::operation_gate::{OperationGate, OperationKind};
        use std::sync::mpsc;

        for kind in [
            OperationKind::Lifecycle,
            OperationKind::CheckpointRestore,
            OperationKind::Export,
        ] {
            let gate = OperationGate::new();
            let (changed, notifications) = mpsc::channel();
            gate.set_listener(move || {
                let _ = changed.send(());
            });
            let transfer =
                acquire_macos_transfer_turn(&gate, "computer-id", "dev", || Ok(())).unwrap();
            while notifications.try_recv().is_ok() {}

            let (entered, admitted) = mpsc::channel();
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    let _operation = gate
                        .kind(kind)
                        .computer("computer-id", "dev", "Conflicting operation")
                        .unwrap();
                    entered.send(()).unwrap();
                });

                notifications.recv().unwrap();
                let queue = gate.snapshot();
                assert_eq!(queue.running.len(), 1, "transfer should retain its gate");
                assert_eq!(queue.waiting.len(), 1, "conflicting work should wait");
                assert!(queue.waiting[0].cancellable);

                drop(transfer);
                admitted.recv().unwrap();
            });
        }
    }

    #[test]
    fn macos_transfer_revalidates_running_generation_after_waiting() {
        use runtime::operation_gate::{OperationGate, OperationKind};
        use std::sync::{mpsc, Mutex};

        let gate = OperationGate::new();
        let (changed, notifications) = mpsc::channel();
        gate.set_listener(move || {
            let _ = changed.send(());
        });
        for (current_state, current_generation, expected_error) in [
            (State::Stopped, 8, "Start this computer to transfer files."),
            (
                State::Running,
                8,
                "This computer restarted while the transfer was waiting. Retry it.",
            ),
        ] {
            let observed = Arc::new(Mutex::new((State::Running, 7)));
            let waiter_state = observed.clone();
            let (result_tx, result_rx) = mpsc::channel();
            let stop = gate
                .kind(OperationKind::Lifecycle)
                .computer("computer-id", "dev", "Stopping dev")
                .unwrap();
            while notifications.try_recv().is_ok() {}

            std::thread::scope(|scope| {
                scope.spawn(|| {
                    let result = acquire_macos_transfer_turn(&gate, "computer-id", "dev", || {
                        let (state, generation) = *waiter_state.lock().unwrap();
                        ensure_transfer_target(state, generation, 7)
                    });
                    result_tx.send(result.map(drop)).unwrap();
                });

                notifications.recv().unwrap();
                let queue = gate.snapshot();
                assert_eq!(
                    queue.waiting.len(),
                    1,
                    "transfer should be visible while waiting"
                );
                *observed.lock().unwrap() = (current_state, current_generation);
                drop(stop);
                assert_eq!(result_rx.recv().unwrap().unwrap_err(), expected_error);
            });
        }
    }

    struct ProbeEngineContext {
        paths: silo_engine::EnginePaths,
        identity: silo_engine::EngineIdentity,
        runtime_state: Arc<silo_engine::EngineRuntimeState>,
    }

    impl silo_engine::EngineContext for ProbeEngineContext {
        fn paths(&self) -> &silo_engine::EnginePaths {
            &self.paths
        }

        fn identity(&self) -> &silo_engine::EngineIdentity {
            &self.identity
        }

        fn runtime_state(&self) -> Arc<silo_engine::EngineRuntimeState> {
            self.runtime_state.clone()
        }

        fn publish(&self, _event: Event, _audience: Audience) {}

        fn main_thread(&self) -> Option<&dyn silo_engine::MainThread> {
            None
        }

        fn process(&self) -> &dyn silo_engine::ProcessControl {
            self
        }
    }

    impl silo_engine::ProcessControl for ProbeEngineContext {
        fn exit(&self, _code: i32) {}

        fn restart(&self) -> ! {
            panic!("the live probe context cannot restart")
        }
    }

    struct ProbeMacosHost;

    impl MacosHost for ProbeMacosHost {
        fn show_display(
            &self,
            _context: &MacosContext,
            _id: &str,
            _title: &str,
            _toolbar: bool,
        ) -> Result<(), String> {
            Err("The live probe does not open a display.".into())
        }

        fn close_display(&self, _id: &str) {}

        fn linux_computer_names(&self) -> Vec<String> {
            Vec::new()
        }
    }

    struct ProbeCleanup {
        context: MacosContext,
        computer_id: String,
        guest_port: Option<u16>,
        guest_pid: Option<u32>,
        port_config: PathBuf,
        original_port_config: Option<(Vec<u8>, std::fs::Permissions)>,
    }

    impl Drop for ProbeCleanup {
        fn drop(&mut self) {
            if let (Some(port), Some(pid)) = (self.guest_port, self.guest_pid) {
                if let Ok(alias) =
                    ensure_ssh_alias(&self.context, &self.computer_id, None, Some(false))
                {
                    let _ = probe_ssh_command(
                        &alias.config_path,
                        &alias.alias,
                        &format!("kill -TERM {pid}"),
                    );
                }
                let _ = remove_network_port(&self.context, &self.computer_id, port);
            }
            let _ = remove_ssh_alias(&self.context, &self.computer_id);
            match &self.original_port_config {
                Some((bytes, permissions)) => {
                    if let Some(parent) = self.port_config.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if std::fs::write(&self.port_config, bytes).is_ok() {
                        let _ = std::fs::set_permissions(&self.port_config, permissions.clone());
                    }
                }
                None => {
                    let _ = std::fs::remove_file(&self.port_config);
                }
            }
            ssh_alias::set_proxy_helper_for_test(None);
        }
    }

    fn probe_ssh_command(config: &str, alias: &str, remote_command: &str) -> std::process::Output {
        let mut command = std::process::Command::new("/usr/bin/ssh");
        crate::applications::launch::sanitize_child(&mut command)
            .args(["-F", config, "-o", "BatchMode=yes", alias, remote_command])
            .output()
            .expect("launch the disposable guest SSH command")
    }

    #[test]
    #[ignore = "opt-in live probe; requires one running disposable Silo Dev macOS computer"]
    fn live_probe_files_ports_and_local_ssh_alias() {
        let computer_id = std::env::var("SILO_MACOS_LIVE_PROBE_ID")
            .expect("set the disposable macOS computer UUID");
        let app_data = PathBuf::from(
            std::env::var_os("SILO_MACOS_LIVE_PROBE_APP_DATA")
                .expect("set the Silo Dev application data path"),
        );
        let helper = PathBuf::from(
            std::env::var_os("SILO_MACOS_LIVE_PROBE_HELPER")
                .expect("set the disposable Dev engine helper path"),
        );
        let home = PathBuf::from(
            std::env::var_os("SILO_MACOS_LIVE_PROBE_HOME").expect("set the host home path"),
        );
        uuid::Uuid::parse_str(&computer_id).expect("the probe computer id is a UUID");
        silo_engine::channel::init(silo_engine::channel::DEVELOPMENT_IDENTIFIER);

        let context = Arc::new(ProbeEngineContext {
            paths: silo_engine::EnginePaths {
                app_data: app_data.clone(),
                app_config: app_data.clone(),
                app_cache: app_data.clone(),
                app_log: app_data.clone(),
                resources: PathBuf::new(),
                home,
            },
            identity: silo_engine::EngineIdentity {
                channel: silo_engine::channel::Channel::Development,
                version: env!("CARGO_PKG_VERSION").into(),
                package_kind: silo_engine::PackageKind::MacApp,
            },
            runtime_state: Arc::new(silo_engine::EngineRuntimeState::default()),
        });
        let context = MacosContext::new(context, Arc::new(ProbeMacosHost));
        ssh_alias::set_proxy_helper_for_test(Some(helper));
        let layout = Layout::new(&app_data, &computer_id);
        let record = store::read_record(&layout).expect("read the disposable computer record");
        assert!(record.setup.roots, "the guest must have workspace roots");
        assert!(
            record.setup_complete(),
            "the guest must be set up before probing"
        );

        {
            let mut registry = registry();
            assert!(
                !registry.loaded,
                "run this focused probe in a fresh test process"
            );
            registry.entries = vec![Entry::new(record, State::Running, None)];
            registry.loaded = true;
        }

        let port_config = app_data.join("network/macos-ports.json");
        let original_port_config = std::fs::read(&port_config).ok().map(|bytes| {
            let permissions = std::fs::metadata(&port_config)
                .expect("read port config permissions")
                .permissions();
            (bytes, permissions)
        });
        let mut cleanup = ProbeCleanup {
            context: context.clone(),
            computer_id: computer_id.clone(),
            guest_port: None,
            guest_pid: None,
            port_config,
            original_port_config,
        };

        let files = list_directory(
            &context,
            "p3-macos-parity-2b-live".into(),
            computer_id.clone(),
            files::WORKSPACE.into(),
            0,
            None,
        )
        .expect("list the live guest workspace");
        eprintln!(
            "live_files entries={} next_page={}",
            files.entries.len(),
            files.next_offset.is_some()
        );

        let alias = ensure_ssh_alias(&context, &computer_id, None, Some(false))
            .expect("ensure and probe the local-engine SSH alias");
        eprintln!("live_alias alias={} include_not_changed=true", alias.alias);

        let before_ports = read_network_state(&context).expect("read live guest listeners");
        let initial = before_ports
            .computers
            .iter()
            .find(|computer| computer.computer_id.as_deref() == Some(&computer_id))
            .expect("find the disposable computer in network state");
        let guest_port = (49152..=49215)
            .find(|port| {
                !initial
                    .ports
                    .iter()
                    .any(|candidate| candidate.port == *port)
            })
            .expect("find an unused disposable guest port");
        cleanup.guest_port = Some(guest_port);
        let start = probe_ssh_command(
            &alias.config_path,
            &alias.alias,
            &format!(
                "(/usr/bin/nohup /usr/bin/nc -lk 127.0.0.1 {guest_port} >/dev/null 2>&1 </dev/null & echo $!)"
            ),
        );
        assert!(
            start.status.success(),
            "start the disposable guest listener: {}",
            String::from_utf8_lossy(&start.stderr)
        );
        cleanup.guest_pid = String::from_utf8_lossy(&start.stdout)
            .split_whitespace()
            .find_map(|value| value.parse::<u32>().ok());
        assert!(cleanup.guest_pid.is_some(), "capture the listener PID");

        let listeners = read_network_state(&context).expect("discover the live guest listener");
        let observed = listeners
            .computers
            .iter()
            .find(|computer| computer.computer_id.as_deref() == Some(&computer_id))
            .expect("find the disposable computer after listener discovery");
        assert!(
            observed.ports.iter().any(|port| port.port == guest_port),
            "lsof should report the disposable guest listener"
        );

        let published = publish_network_port(
            &context,
            &computer_id,
            guest_port,
            None,
            Some("http".into()),
        )
        .expect("publish the live guest listener through SSH");
        let published = published
            .computers
            .iter()
            .find(|computer| computer.computer_id.as_deref() == Some(&computer_id))
            .and_then(|computer| computer.ports.iter().find(|port| port.port == guest_port))
            .expect("find the published guest port");
        assert_eq!(
            published.state,
            silo_api::methods::network::PublishedPortState::Reachable
        );
        let host_port = published
            .host_port
            .expect("published port has a host endpoint");
        let socket = std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], host_port)),
            Duration::from_secs(3),
        )
        .expect("connect through the published loopback tunnel");
        drop(socket);
        eprintln!("live_port guest={guest_port} host={host_port} state=reachable");

        if std::env::var("SILO_MACOS_LIVE_PROBE_SHUTDOWN").as_deref() == Ok("1") {
            let shutdown = probe_ssh_command(
                &alias.config_path,
                &alias.alias,
                "sudo -n /sbin/shutdown -h now",
            );
            let shutdown_stderr = String::from_utf8_lossy(&shutdown.stderr);
            assert!(
                shutdown.status.success() || shutdown_stderr.contains("closed by remote host"),
                "gracefully shut down the disposable guest: {shutdown_stderr}"
            );
            eprintln!(
                "live_guest_shutdown=ssh_exit_{}",
                shutdown.status.code().unwrap_or(-1),
            );
        }
    }

    #[test]
    fn saved_memory_import_warning_says_import_starts_from_disk() {
        let config = crate::backup::MacosArchiveConfiguration {
            payload_version: 1,
            architecture: "aarch64".into(),
            source_computer_id: "e38c0e77-28bc-44a0-9b3d-3b4f68f0d85f".into(),
            source_name: "Source".into(),
            cpus: 4,
            memory_gib: 8,
            disk_gib: 64,
            guest_version: "26.6.2".into(),
            guest_build: "25G83".into(),
            host_build: checkpoints::host_build(),
            hardware_model_digest: "0".repeat(64),
            setup_fingerprint: "setup-1".into(),
            computer_use_fingerprint: "computer-use-1".into(),
            source: crate::backup::MacosArchiveSourceKind::WholeComputer,
            memory: crate::backup::MacosArchiveMemory::Saved {
                host_build: checkpoints::host_build(),
                configuration_digest: crate::backup::memory_configuration_digest(
                    4,
                    8,
                    64,
                    &format!("sha256:{}", "0".repeat(64)),
                )
                .unwrap(),
                size: 1,
                sha256: "2".repeat(64),
            },
            components: Vec::new(),
        };

        let warning = archive_memory_warning("Source", &config).unwrap();

        assert!(warning.contains("Import verifies it"));
        assert!(warning.contains("starts the new computer from its disk"));
    }

    #[test]
    fn stop_shuts_down_inside_the_guest_only_once_setup_is_complete() {
        assert_eq!(stop_plan(true), StopPlan::Shutdown);
        assert_eq!(stop_plan(false), StopPlan::RequestStop);
    }

    #[test]
    fn a_shutdown_counts_when_the_command_ran_or_the_guest_hung_up() {
        let output = |status, stdout: &str| {
            Ok(guest_access::CommandOutput {
                status,
                stdout: stdout.into(),
                stderr: String::new(),
            })
        };
        assert!(shutdown_accepted(&output(0, "SILO_SHUTDOWN\n")));
        assert!(shutdown_accepted(&output(255, "SILO_SHUTDOWN\n")));
        assert!(!shutdown_accepted(&output(255, "")));
        assert!(!shutdown_accepted(&output(0, "")));
        assert!(!shutdown_accepted(&output(1, "SILO_SHUTDOWN\n")));
        assert!(SSH_SHUTDOWN.starts_with("echo SILO_SHUTDOWN;"));
        assert!(!shutdown_accepted(&Err("took too long".into())));
    }

    #[test]
    fn the_graceful_wait_leaves_the_forced_stop_its_time() {
        let now = Instant::now();
        let secs = Duration::from_secs;
        assert_eq!(graceful_until(now, None), now + GRACEFUL_QUIT);
        assert_eq!(
            graceful_until(now, Some(now + secs(600))),
            now + GRACEFUL_QUIT
        );
        assert_eq!(
            graceful_until(now, Some(now + FORCED_QUIT + secs(10))),
            now + secs(10)
        );
        assert_eq!(graceful_until(now, Some(now + FORCED_QUIT)), now);
        assert_eq!(graceful_until(now, Some(now + secs(1))), now);
        assert_eq!(graceful_until(now, Some(now)), now);
    }

    #[test]
    fn restoring_a_pre_tools_checkpoint_clears_tools_readiness_and_fingerprint() {
        let (mut registry, _) = registry_with(State::Stopped);
        let record = &mut registry.entries[0].record;
        record.setup.developer_tools = true;
        record.developer_tools_version = Some("tools-current".into());
        let target = checkpoints::Meta {
            schema_version: 1,
            id: "checkpoint".into(),
            name: "Before tools".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            kind: checkpoints::Kind::Disk,
            reason: checkpoints::Reason::Manual,
            macos_version: None,
            host_build: String::new(),
            size_bytes: None,
            cpus: None,
            memory_gib: None,
            disk_gib: None,
            setup_fingerprint: None,
            computer_use_version: Some("cu-before".into()),
            developer_tools: false,
            developer_tools_version: None,
        };

        restore_checkpoint_metadata(record, Some(&target));

        assert!(!record.setup.developer_tools);
        assert_eq!(record.developer_tools_version, None);
        assert_eq!(record.computer_use_version.as_deref(), Some("cu-before"));
    }

    #[test]
    fn quit_keeps_time_for_the_forced_stop() {
        let secs = Duration::from_secs;
        assert_eq!(quit_shutdown_timeout(None), Some(QUIT_SHUTDOWN_TIMEOUT));
        assert_eq!(
            quit_shutdown_timeout(Some(secs(600))),
            Some(QUIT_SHUTDOWN_TIMEOUT)
        );
        assert_eq!(
            quit_shutdown_timeout(Some(FORCED_QUIT + secs(5))),
            Some(secs(5))
        );
        assert_eq!(quit_shutdown_timeout(Some(FORCED_QUIT + secs(1))), None);
        assert_eq!(quit_shutdown_timeout(Some(secs(3))), None);
        assert_eq!(quit_shutdown_timeout(Some(Duration::ZERO)), None);
    }

    #[test]
    fn rows_serialize_to_the_frontend_contract() {
        let record = store::new_record(
            &CreateRequest {
                name: "mac-one".into(),
                cpus: 4,
                memory_gib: 8,
                disk_gib: 64,
            },
            "02:00:00:00:00:01".into(),
        );
        let mut entry = Entry::new(record, State::Downloading, None);
        entry.progress = Some(0.25);
        let json = serde_json::to_value(entry.row()).unwrap();
        assert_eq!(json["state"], "downloading");
        assert_eq!(json["memoryGiB"], 8);
        assert_eq!(json["diskGiB"], 64);
        assert_eq!(json["osVersion"], serde_json::Value::Null);
        assert_eq!(json["displayOpen"], false);
        assert_eq!(json["progress"], 0.25);
        assert_eq!(json["setupComplete"], false);
        assert_eq!(json["installed"], false);
        assert_eq!(json["needsPersonalizing"], false);
        assert!(json.get("detail").is_some());
        let mut done = entry;
        done.state = State::SettingUp;
        done.detail = Some("Creating the account".into());
        done.record.setup = store::SetupProgress {
            account: true,
            sip: true,
            developer_tools: true,
            computer_use: true,
            clipboard: true,
            roots: true,
            needs_personalizing: false,
        };
        done.record.developer_tools_version = Some(developer_tools::version());
        let json = serde_json::to_value(done.row()).unwrap();
        assert_eq!(json["state"], "setting-up");
        assert_eq!(json["detail"], "Creating the account");
        assert_eq!(json["setupComplete"], true);
        let state = serde_json::to_value(MacosComputersState {
            supported: false,
            unsupported_reason: Some("no".into()),
            computers: vec![],
            template: Some(TemplateSummary {
                macos_version: "26.6.2".into(),
                build: "25G83".into(),
                current: true,
            }),
            min_disk_gib: 64,
        })
        .unwrap();
        assert_eq!(state["unsupportedReason"], "no");
        assert_eq!(
            state["template"],
            serde_json::json!({"macosVersion": "26.6.2", "build": "25G83", "current": true})
        );
        assert_eq!(state["minDiskGiB"], 64);
        assert!(state["computers"].as_array().unwrap().is_empty());
    }

    #[test]
    fn macos_operations_wait_in_the_shared_queue_and_a_waiter_can_be_cancelled() {
        let gate: &'static runtime::operation_gate::OperationGate =
            Box::leak(Box::new(runtime::operation_gate::OperationGate::new()));
        let id = "macos-computer";
        let _running = acquire_computer_operation(
            gate,
            id,
            "builder",
            runtime::operation_gate::OperationKind::Lifecycle,
            "Starting builder",
        )
        .unwrap();
        let worker = std::thread::spawn(move || {
            acquire_computer_operation(
                gate,
                id,
                "builder",
                runtime::operation_gate::OperationKind::ComputerConfiguration,
                "Setting up builder",
            )
            .map(drop)
        });
        let waiting = (0..100)
            .find_map(|_| {
                let waiting = gate.snapshot().waiting;
                if waiting
                    .iter()
                    .any(|entry| entry.computer_id.as_deref() == Some(id))
                {
                    Some(waiting)
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                    None
                }
            })
            .expect("the macOS operation should be visible while waiting");
        let queued = waiting
            .iter()
            .find(|entry| entry.computer_id.as_deref() == Some(id))
            .unwrap();
        assert_eq!(queued.computer_name.as_deref(), Some("builder"));
        assert_eq!(queued.label, "Setting up builder");
        assert_eq!(
            queued.kind,
            runtime::operation_gate::OperationKind::ComputerConfiguration
        );
        gate.cancel(queued.id).unwrap();
        assert!(worker.join().unwrap().unwrap_err().contains("cancelled"));
    }

    #[test]
    fn rows_carry_checkpoints_the_operation_and_the_pending_restore() {
        let (mut registry, _) = registry_with(State::Stopped);
        let entry = &mut registry.entries[0];
        let row = serde_json::to_value(entry.row()).unwrap();
        assert_eq!(row["checkpoints"], serde_json::json!([]));
        assert_eq!(row["checkpointOperation"], serde_json::Value::Null);
        assert_eq!(row["pendingRestore"], serde_json::Value::Null);
        entry.operation = Some(checkpoints::Operation::running(
            checkpoints::OperationKind::Restore,
            "Saving a recovery checkpoint",
        ));
        entry.record.pending_restore = Some(checkpoints::PendingRestore {
            checkpoint_id: "c".into(),
            memory: true,
        });
        let row = serde_json::to_value(entry.row()).unwrap();
        assert_eq!(
            row["checkpointOperation"],
            serde_json::json!({"kind": "restore", "status": "running", "stage": "Saving a recovery checkpoint"})
        );
        assert_eq!(
            row["pendingRestore"],
            serde_json::json!({"checkpointId": "c", "memory": true})
        );
    }

    #[test]
    fn a_force_stop_is_claimed_under_the_lock_and_refused_during_an_operation() {
        let (mut registry, id) = registry_with(State::Running);
        registry.entries[0].operation = Some(checkpoints::Operation::running(
            checkpoints::OperationKind::Capture,
            "Copying the disk",
        ));
        assert_eq!(
            registry.begin_force_stop(&id),
            Err(checkpoints::BUSY.to_string())
        );
        assert_eq!(registry.entries[0].state, State::Running);
        registry.entries[0].operation = None;
        assert_eq!(registry.begin_force_stop(&id), Ok(State::Running));
        assert_eq!(registry.entries[0].state, State::Stopping);
        let (mut registry, id) = registry_with(State::Stopped);
        assert!(registry.begin_force_stop(&id).is_err());
    }

    #[test]
    fn a_machine_a_failed_start_could_not_release_stays_busy_and_open_to_force_stop() {
        assert_eq!(state_after_failed_start(Ok(())), (State::Stopped, None));
        let (state, detail) = state_after_failed_start(Err("still held".into()));
        assert_eq!(state, State::Stopping);
        assert_eq!(detail.as_deref(), Some("still held"));
        assert!(is_busy(state));
        assert!(store::delete_mode(state).is_err());
        assert!(store::checkpoint_allowed(
            state,
            &registry_with(state).0.entries[0].record,
            false,
            false,
            checkpoints::OperationKind::Restore
        )
        .is_err());
        let (mut registry, id) = registry_with(state);
        assert_eq!(registry.begin_force_stop(&id), Ok(State::Stopping));
    }

    #[test]
    fn an_event_of_an_earlier_machine_concerns_no_one_once_the_computer_has_a_later_one() {
        // Machine 1 is replaced by machine 2 while 1's stop callback is still on its way.
        assert!(slot_is_current(Some(1), 1));
        assert!(!slot_is_current(Some(2), 1));
        // Its slot is already gone, e.g. released by a failed start's cleanup.
        assert!(!slot_is_current(None, 1));
        // The late callback of machine 1 cannot stop or release machine 2.
        let mut slots = std::collections::HashMap::new();
        slots.insert("a", 1u64);
        let release = |slots: &mut std::collections::HashMap<&str, u64>, event: u64| {
            if slot_is_current(slots.get("a").copied(), event) {
                slots.remove("a");
                true
            } else {
                false
            }
        };
        assert!(release(&mut slots, 1));
        slots.insert("a", 2);
        assert!(!release(&mut slots, 1));
        assert_eq!(slots.get("a"), Some(&2));
        assert!(release(&mut slots, 2));
    }

    fn registry_starting(attempt: u64) -> (Registry, String) {
        let (mut registry, id) = registry_with(State::Starting);
        registry.entries[0].attempt = attempt;
        (registry, id)
    }

    #[test]
    fn settling_absent_machines_never_holds_the_registry_while_it_waits_for_the_main_thread() {
        let (registry, id) = registry_with(State::Stopping);
        let registry = Mutex::new(registry);
        let dispatched = std::cell::Cell::new(0);
        // The main thread's callbacks need this lock while `dispatch` waits for them; from
        // another thread, as they would, it must be free at that moment (a regression
        // blocks here, and the test fails after the timeout instead of hanging).
        settle_absent(&registry, &[], &|stalled| {
            dispatched.set(dispatched.get() + 1);
            assert_eq!(stalled, id);
            std::thread::scope(|scope| {
                let free = scope
                    .spawn(|| {
                        let start = Instant::now();
                        while start.elapsed() < Duration::from_secs(5) {
                            if registry.try_lock().is_ok() {
                                return true;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        false
                    })
                    .join()
                    .unwrap();
                assert!(free, "the registry stayed locked during dispatch");
            });
        });
        assert_eq!(dispatched.get(), 1);
    }

    #[test]
    fn an_older_start_never_clears_a_newer_restores_reference() {
        let a = checkpoints::PendingRestore {
            checkpoint_id: "a".into(),
            memory: true,
        };
        let b = checkpoints::PendingRestore {
            checkpoint_id: "b".into(),
            memory: true,
        };
        // Start 1 read restore A; the computer was force stopped and restored to B; start 2
        // is now the current one.
        let (mut registry, id) = registry_starting(2);
        registry.entries[0].record.pending_restore = Some(b.clone());
        let saved = std::cell::Cell::new(false);
        let save = |_: &Record| {
            saved.set(true);
            Ok(())
        };
        assert!(registry.consume_pending_restore(&id, 1, &a, save).is_err());
        assert!(registry.consume_pending_restore(&id, 1, &b, save).is_err());
        assert!(registry.consume_pending_restore(&id, 2, &a, save).is_err());
        assert!(!saved.get());
        assert_eq!(registry.entries[0].record.pending_restore, Some(b.clone()));
        // The attempt that read B consumes it.
        assert!(registry.consume_pending_restore(&id, 2, &b, save).is_ok());
        assert!(saved.get());
        assert_eq!(registry.entries[0].record.pending_restore, None);
        // A computer no longer starting refuses too, and a failed save changes nothing.
        let (mut registry, id) = registry_starting(2);
        registry.entries[0].record.pending_restore = Some(b.clone());
        assert!(registry
            .consume_pending_restore(&id, 2, &b, |_| Err("disk full".into()))
            .is_err());
        assert_eq!(registry.entries[0].record.pending_restore, Some(b.clone()));
        registry.entries[0].state = State::Stopped;
        assert!(registry.consume_pending_restore(&id, 2, &b, save).is_err());
    }

    #[test]
    fn a_failed_starts_cleanup_does_nothing_once_a_later_start_owns_the_computer() {
        // Start 1 failed and its machine's stop callback set the computer stopped; start 2
        // was admitted. Start 1's settlement and guard must leave start 2 alone.
        let (mut registry, id) = registry_starting(2);
        assert!(!registry.start_is_current(&id, 1));
        assert!(registry.start_is_current(&id, 2));
        registry.settle_failed_start(&id, 1, Ok(()));
        assert_eq!(registry.entries[0].state, State::Starting);
        registry.settle_failed_start(&id, 1, Err("still held".into()));
        assert_eq!(registry.entries[0].state, State::Starting);
        assert_eq!(registry.entries[0].detail, None);
        // Its own settlement still works.
        registry.settle_failed_start(&id, 2, Ok(()));
        assert_eq!(registry.entries[0].state, State::Stopped);
        assert!(!registry.start_is_current(&id, 2));
    }

    #[test]
    fn a_start_is_current_only_while_the_computer_is_starting() {
        let (mut registry, id) = registry_starting(3);
        registry.entries[0].state = State::Stopped;
        assert!(!registry.start_is_current(&id, 3));
        registry.entries[0].state = State::Running;
        assert!(!registry.start_is_current(&id, 3));
    }

    #[test]
    fn a_stale_cleanup_of_a_failed_start_does_not_touch_the_state_of_a_later_one() {
        // The failed start's cleanup finishes after the computer was stopped and started
        // again: the new start is `starting`, but its machine's callbacks are its own, and
        // the settle only acts on a computer still in the failed start's own `starting`.
        let (mut registry, id) = registry_with(State::Running);
        registry.settle_failed_start(&id, 1, Err("still held".into()));
        assert_eq!(registry.entries[0].state, State::Running);
        registry.entries[0].state = State::Stopping;
        registry.settle_failed_start(&id, 1, Ok(()));
        assert_eq!(registry.entries[0].state, State::Stopping);
    }

    #[test]
    fn a_paused_machine_is_resumed_only_when_nothing_owns_it_and_no_memory_is_pending() {
        assert_eq!(paused_action(true, false), PausedAction::Leave);
        assert_eq!(paused_action(true, true), PausedAction::Leave);
        assert_eq!(paused_action(false, false), PausedAction::Resume);
        assert_eq!(paused_action(false, true), PausedAction::ForceStop);
    }

    #[test]
    fn a_late_stop_callback_is_not_overwritten_by_a_failed_start_settling() {
        // The machine was released and the computer set stopped before the settle ran.
        let (mut registry, id) = registry_with(State::Stopped);
        registry.settle_failed_start(&id, 1, Err("still held".into()));
        assert_eq!(registry.entries[0].state, State::Stopped);
        assert_eq!(registry.entries[0].detail, None);
        // Otherwise the outcome of releasing decides.
        let (mut registry, id) = registry_with(State::Starting);
        registry.settle_failed_start(&id, 1, Err("still held".into()));
        assert_eq!(registry.entries[0].state, State::Stopping);
        let (mut registry, id) = registry_with(State::Starting);
        registry.settle_failed_start(&id, 1, Ok(()));
        assert_eq!(registry.entries[0].state, State::Stopped);
    }

    #[test]
    fn a_stopping_computer_without_a_machine_is_found_for_the_watcher() {
        let (mut registry, id) = registry_with(State::Stopping);
        assert_eq!(registry.stopping_without_machine(&[]), [id.clone()]);
        assert!(registry.stopping_without_machine(&[id.clone()]).is_empty());
        registry.entries[0].operation = Some(checkpoints::Operation::running(
            checkpoints::OperationKind::Restore,
            "Stopping the computer",
        ));
        assert!(registry.stopping_without_machine(&[]).is_empty());
        let (registry, _) = registry_with(State::Running);
        assert!(registry.stopping_without_machine(&[]).is_empty());
    }

    #[test]
    fn quit_and_updates_count_an_operation_on_a_stopped_computer_as_busy() {
        let (mut registry, _) = registry_with(State::Stopped);
        assert!(!is_busy(registry.entries[0].state));
        registry.entries[0].operation = Some(checkpoints::Operation::running(
            checkpoints::OperationKind::Capture,
            "Copying the disk",
        ));
        let mut closed = Closed::default();
        assert!(registry.any_busy());
        assert!(closed.close_for_update(registry.any_busy()).is_err());
        assert!(!closed.update);
    }

    fn registry_with(state: State) -> (Registry, String) {
        let record = store::new_record(
            &CreateRequest {
                name: "mac-one".into(),
                cpus: 4,
                memory_gib: 8,
                disk_gib: 64,
            },
            "02:00:00:00:00:01".into(),
        );
        let id = record.id.clone();
        let mut registry = Registry {
            loaded: true,
            entries: vec![Entry::new(record, state, None)],
            template: None,
            min_disk_gib: store::MIN_DISK_GIB,
            latest_build: None,
        };
        registry.entries[0].attempt = 1;
        (registry, id)
    }

    #[test]
    fn a_computer_use_update_runs_only_for_the_start_that_owns_a_free_running_computer() {
        let (mut registry, id) = registry_with(State::Running);
        assert!(registry.update_may_run(&id, 1));
        // Another Start, a stop, a deletion or a checkpoint operation ends it.
        assert!(!registry.update_may_run(&id, 2));
        assert!(!registry.update_may_run("other", 1));
        registry.entries[0].operation = Some(checkpoints::Operation::running(
            checkpoints::OperationKind::Capture,
            "Copying the disk",
        ));
        assert!(!registry.update_may_run(&id, 1));
        registry.entries[0].operation = None;
        registry.entries[0].deleting = true;
        assert!(!registry.update_may_run(&id, 1));
        registry.entries[0].deleting = false;
        for state in [State::Stopping, State::Stopped, State::Starting] {
            registry.entries[0].state = state;
            assert!(!registry.update_may_run(&id, 1), "{state:?}");
        }
    }

    #[test]
    fn the_updated_version_is_saved_only_while_the_computer_is_still_this_starts() {
        use crate::computer_use::Approval;
        let (mut registry, id) = registry_with(State::Running);
        let saved = std::cell::RefCell::new(Vec::new());
        let save = |record: &Record| {
            saved.borrow_mut().push(record.computer_use_version.clone());
            Ok(())
        };
        assert_eq!(registry.entries[0].record.computer_use_version, None);
        assert_eq!(
            registry.finish_computer_use_update(&id, 1, "v2", Approval::Auto, save),
            Ok(true)
        );
        assert_eq!(
            registry.entries[0].record.computer_use_version.as_deref(),
            Some("v2")
        );
        assert_eq!(
            registry.entries[0].record.computer_use_approval,
            Some(Approval::Auto)
        );
        assert_eq!(*saved.borrow(), [Some("v2".to_string())]);
        // A computer that was stopped and started again, restored or deleted meanwhile
        // keeps its record: the update runs again later.
        registry.entries[0].record.computer_use_version = None;
        let never = |_: &Record| -> Result<(), String> { panic!("must not save") };
        assert_eq!(
            registry.finish_computer_use_update(&id, 2, "v2", Approval::Ask, never),
            Ok(false)
        );
        registry.entries[0].state = State::Stopped;
        assert_eq!(
            registry.finish_computer_use_update(&id, 1, "v2", Approval::Ask, never),
            Ok(false)
        );
        registry.entries[0].state = State::Running;
        registry.entries[0].operation = Some(checkpoints::Operation::running(
            checkpoints::OperationKind::Restore,
            "Restoring",
        ));
        assert_eq!(
            registry.finish_computer_use_update(&id, 1, "v2", Approval::Ask, never),
            Ok(false)
        );
        registry.entries[0].operation = None;
        // A failed save leaves the old version in the record.
        let failing = |_: &Record| Err("disk full".to_string());
        assert_eq!(
            registry.finish_computer_use_update(&id, 1, "v2", Approval::Ask, failing),
            Err("disk full".to_string())
        );
        assert_eq!(registry.entries[0].record.computer_use_version, None);
    }

    #[test]
    fn a_finished_installation_is_published_unless_cancelled() {
        let (mut registry, id) = registry_with(State::Installing);
        let mut record = registry.entries[0].record.clone();
        record.installed = true;
        assert!(registry.creation_continues(&id));
        assert!(registry.publish_installed(&id, &record, State::SettingUp));
        assert_eq!(registry.entries[0].state, State::SettingUp);
        assert!(registry.entries[0].record.installed);

        let (mut registry, id) = registry_with(State::Installing);
        registry.entries[0]
            .cancel
            .store(CANCEL_AND_REMOVE, Ordering::SeqCst);
        assert!(!registry.creation_continues(&id));
        assert!(!registry.publish_installed(&id, &record, State::SettingUp));
        assert_eq!(registry.entries[0].state, State::Installing);
    }

    #[test]
    fn a_cancelled_creation_stays_failed_when_its_files_cannot_be_removed() {
        let (mut registry, id) = registry_with(State::Installing);
        registry.entries[0]
            .cancel
            .store(CANCEL_AND_REMOVE, Ordering::SeqCst);
        registry.finish_cancelled(&id, Err("permission denied".into()));
        let entry = &registry.entries[0];
        assert_eq!(entry.state, State::Failed);
        assert_eq!(entry.detail.as_deref(), Some("permission denied"));
        assert_eq!(store::delete_mode(entry.state), Ok(DeleteMode::Remove));
        registry.finish_cancelled(&id, Ok(()));
        assert!(registry.entries.is_empty());
    }

    #[test]
    fn a_setup_in_progress_is_cancelled_like_an_installation() {
        let (mut registry, id) = registry_with(State::SettingUp);
        assert!(registry.creation_continues(&id));
        registry.entries[0]
            .cancel
            .store(CANCEL_AND_REMOVE, Ordering::SeqCst);
        assert!(!registry.creation_continues(&id));
        assert_eq!(store::delete_mode(State::SettingUp), Ok(DeleteMode::Cancel));
        registry.finish_cancelled(&id, Ok(()));
        assert!(registry.entries.is_empty());

        // Quit keeps the computer; its flag differs from a Delete's.
        let (registry, id) = registry_with(State::SettingUp);
        registry.entries[0]
            .cancel
            .store(ABORT_AND_KEEP, Ordering::SeqCst);
        let mut registry = registry;
        assert!(registry.creation_continues(&id));
    }

    #[test]
    fn cancelling_a_queued_creation_keeps_its_folder_record_and_inventory_row() {
        let data = tempfile::tempdir().unwrap();
        let (mut registry, id) = registry_with(State::Downloading);
        let layout = Layout::new(data.path(), &id);
        let record = registry.entries[0].record.clone();
        store::save(&layout, &record).unwrap();

        // This is the cancellation branch used by create_workflow after its queue wait.
        creation_queue_cancelled(&registry.entries[0].cancel);
        let finish = registry.finish_workflow(&id, Err(Stop::Cancelled));
        if matches!(&finish, Finish::Remove) {
            store::remove(&layout).unwrap();
            registry.finish_cancelled(&id, Ok(()));
        }

        assert!(matches!(finish, Finish::Kept));
        assert!(layout.record().exists());
        assert_eq!(store::load_all(data.path()), [record]);
        assert_eq!(registry.entries.len(), 1);
        assert_eq!(registry.entries[0].row().id, id);
        assert_eq!(registry.entries[0].state, State::Failed);
    }

    #[test]
    fn retry_setup_stays_stopped_until_queue_admission_and_uses_the_current_record() {
        let (mut registry, _) = registry_with(State::Stopped);
        registry.entries[0].record.installed = true;
        let entry = &mut registry.entries[0];

        let (_, cancel) = reserve_setup(entry).unwrap();
        assert_eq!(entry.state, State::Stopped);
        assert!(entry.setup_pending);
        assert_eq!(cancel.load(Ordering::SeqCst), RUN);

        // A setup request can wait behind work that updates the saved record.
        entry.record.cpus = 8;
        entry.record.memory_gib = 16;
        let admitted = admit_queued_setup(entry).unwrap();

        assert_eq!(entry.state, State::SettingUp);
        assert_eq!(admitted.cpus, 8);
        assert_eq!(admitted.memory_gib, 16);
        assert!(reserve_setup(entry).is_err());
    }

    #[test]
    fn stale_developer_tools_make_a_complete_computer_retryable_through_provisioning() {
        let (mut registry, _) = registry_with(State::Stopped);
        let entry = &mut registry.entries[0];
        entry.record.installed = true;
        entry.record.setup = store::SetupProgress {
            account: true,
            sip: true,
            developer_tools: true,
            computer_use: true,
            clipboard: true,
            roots: true,
            needs_personalizing: false,
        };
        entry.record.developer_tools_version = Some("stale-tools".into());
        assert!(!entry.row().setup_complete);

        reserve_setup(entry).unwrap();
        let record = admit_queued_setup(entry).unwrap();

        assert_eq!(entry.state, State::SettingUp);
        assert!(provision::developer_tools_outdated(
            &record,
            &developer_tools::version()
        ));
    }

    #[test]
    fn failed_resource_save_rolls_back_disk_registry_and_reload_state() {
        let data = tempfile::tempdir().unwrap();
        let (mut registry, id) = registry_with(State::Stopped);
        let record = &mut registry.entries[0].record;
        record.installed = true;
        record.setup = store::SetupProgress {
            account: true,
            sip: true,
            developer_tools: true,
            computer_use: true,
            clipboard: true,
            roots: true,
            needs_personalizing: false,
        };
        record.developer_tools_version = Some(developer_tools::version());
        let old = record.clone();
        let layout = Layout::new(data.path(), &id);
        store::save(&layout, &old).unwrap();
        let mut registry_record = old.clone();
        let mut updated = old.clone();
        updated.cpus += 2;
        updated.memory_gib += 4;
        let mut sync_calls = 0;

        let error = persist_then_commit(
            updated,
            |record| {
                store::save_with_sync(&layout, record, |_directory| {
                    sync_calls += 1;
                    if sync_calls == 1 {
                        Err(std::io::Error::other("injected directory sync failure"))
                    } else {
                        store::sync_dir(&layout.dir)
                    }
                })
            },
            || store::read_record(&layout).ok(),
            |record| registry_record = record,
        )
        .unwrap_err();

        assert!(error.contains("injected directory sync failure"));
        assert_eq!(
            (registry_record.cpus, registry_record.memory_gib),
            (old.cpus, old.memory_gib)
        );
        let reloaded = store::load_all(data.path());
        assert_eq!(reloaded, [old.clone()]);
        assert_eq!(
            (reloaded[0].cpus, reloaded[0].memory_gib),
            (old.cpus, old.memory_gib)
        );
    }

    #[test]
    fn a_queued_launch_start_skips_boot_after_startup_cancel_or_quit() {
        for cancel_startup in [true, false] {
            let gate: &'static runtime::operation_gate::OperationGate =
                Box::leak(Box::new(runtime::operation_gate::OperationGate::new()));
            let blocker = gate
                .computer("mac-id", "Mac", "Blocking computer operation")
                .unwrap();
            let cancelled = Arc::new(AtomicBool::new(false));
            let accepting = Arc::new(AtomicBool::new(true));
            let started = Arc::new(AtomicBool::new(false));
            let worker_cancelled = cancelled.clone();
            let worker_accepting = accepting.clone();
            let worker_started = started.clone();
            let (send, receive) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let result = launch_start_after_queue(
                    gate,
                    "mac-id",
                    "Mac",
                    &worker_cancelled,
                    || worker_accepting.load(Ordering::SeqCst),
                    || {
                        worker_started.store(true, Ordering::SeqCst);
                        Ok(())
                    },
                );
                send.send(result).unwrap();
            });
            let deadline = Instant::now() + Duration::from_secs(5);
            while gate.snapshot().waiting.is_empty() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(
                !gate.snapshot().waiting.is_empty(),
                "launch Start did not queue"
            );
            if cancel_startup {
                cancelled.store(true, Ordering::SeqCst);
            } else {
                accepting.store(false, Ordering::SeqCst);
            }
            let result_before_release = receive.recv_timeout(Duration::from_secs(1));
            let finished_early = result_before_release.is_ok();
            drop(blocker);
            let result = result_before_release
                .unwrap_or_else(|_| receive.recv_timeout(Duration::from_secs(5)).unwrap());
            worker.join().unwrap();
            assert!(finished_early, "launch wait ignored cancellation");
            assert_eq!(result.unwrap(), None);
            assert!(!started.load(Ordering::SeqCst));
        }
    }

    fn finish(
        state: State,
        flag: u8,
        installed: bool,
        result: Result<(), Stop>,
    ) -> (Finish, Registry) {
        let (mut registry, id) = registry_with(state);
        registry.entries[0].cancel.store(flag, Ordering::SeqCst);
        registry.entries[0].record.installed = installed;
        let finish = registry.finish_workflow(&id, result);
        (finish, registry)
    }

    #[test]
    fn a_delete_accepted_before_the_outcome_is_published_wins() {
        // Success, failure and a Quit-style abort all give way to a Delete.
        for result in [
            Ok(()),
            Err(Stop::Failed("boom".into())),
            Err(Stop::Cancelled),
        ] {
            let (outcome, registry) = finish(State::SettingUp, CANCEL_AND_REMOVE, true, result);
            assert!(matches!(outcome, Finish::Remove));
            // The computer stays busy until its files are gone.
            assert_eq!(registry.entries[0].state, State::SettingUp);
        }
    }

    #[test]
    fn outcomes_without_a_delete_are_published() {
        let (outcome, registry) = finish(State::SettingUp, RUN, true, Ok(()));
        assert!(matches!(outcome, Finish::Kept));
        assert_eq!(registry.entries[0].state, State::Stopped);

        let (_, registry) = finish(
            State::SettingUp,
            RUN,
            true,
            Err(Stop::Failed("boom".into())),
        );
        assert_eq!(registry.entries[0].state, State::Failed);
        assert_eq!(registry.entries[0].detail.as_deref(), Some("boom"));

        let (_, registry) = finish(State::SettingUp, ABORT_AND_KEEP, true, Err(Stop::Cancelled));
        assert_eq!(registry.entries[0].state, State::Stopped);

        let (_, registry) = finish(
            State::Installing,
            ABORT_AND_KEEP,
            false,
            Err(Stop::Cancelled),
        );
        assert_eq!(registry.entries[0].state, State::Failed);
        assert_eq!(
            registry.entries[0].detail.as_deref(),
            Some(store::INTERRUPTED_INSTALL)
        );
    }

    #[test]
    fn a_queued_setup_failure_does_not_overwrite_a_prior_start_state() {
        let (mut registry, id) = registry_with(State::Stopped);
        registry.entries[0].record.installed = true;
        reserve_setup(&mut registry.entries[0]).unwrap();

        // A Start ahead of Retry Setup in the operation queue runs first.
        registry.entries[0].state = State::Running;
        let _ = registry.finish_workflow(
            &id,
            Err(Stop::Failed(
                "Stop the computer before setting it up.".into(),
            )),
        );

        assert_eq!(registry.entries[0].state, State::Running);
        assert!(!registry.entries[0].setup_pending);
        assert_eq!(
            registry.entries[0].detail.as_deref(),
            Some("Stop the computer before setting it up.")
        );
    }

    #[test]
    fn cancelling_a_queued_setup_does_not_stop_a_prior_start() {
        let (mut registry, id) = registry_with(State::Stopped);
        registry.entries[0].record.installed = true;
        reserve_setup(&mut registry.entries[0]).unwrap();

        // The queued Start has marked itself Starting before Retry Setup is cancelled.
        registry.entries[0].state = State::Starting;
        let _ = registry.finish_workflow(&id, Err(Stop::Cancelled));

        assert_eq!(registry.entries[0].state, State::Starting);
        assert!(!registry.entries[0].setup_pending);
    }

    #[test]
    fn a_copy_in_progress_is_cancelled_and_aborted_like_an_installation() {
        assert!(is_creating(State::Copying));
        assert!(is_busy(State::Copying));
        assert!(!is_creating(State::Running));
        assert_eq!(store::delete_mode(State::Copying), Ok(DeleteMode::Cancel));
        let (mut registry, id) = registry_with(State::Copying);
        assert!(registry.creation_continues(&id));
        registry.entries[0]
            .cancel
            .store(CANCEL_AND_REMOVE, Ordering::SeqCst);
        assert!(!registry.creation_continues(&id));
        registry.finish_cancelled(&id, Ok(()));
        assert!(registry.entries.is_empty());
        // Quit before the files were copied leaves a failed computer to delete; after
        // them, one whose personalization can be retried.
        let (_, registry) = finish(State::Copying, ABORT_AND_KEEP, false, Err(Stop::Cancelled));
        assert_eq!(registry.entries[0].state, State::Failed);
        let (_, registry) = finish(State::SettingUp, ABORT_AND_KEEP, true, Err(Stop::Cancelled));
        assert_eq!(registry.entries[0].state, State::Stopped);
    }

    #[test]
    fn only_a_template_that_would_be_copied_sets_the_smallest_disk() {
        let data = tempfile::tempdir().unwrap();
        let dir = templates::root(data.path()).join(format!(
            "25G83-{}-{}",
            templates::setup_version(),
            templates::computer_use_version()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let meta = templates::Meta {
            schema_version: 1,
            macos_version: "26.6.2".into(),
            build: "25G83".into(),
            setup_version: templates::setup_version(),
            computer_use_version: templates::computer_use_version(),
            disk_gib: 128,
            created_at: "2026-10-09T10:00:00Z".into(),
            source_computer_id: "source".into(),
        };
        std::fs::write(
            dir.join("template.json"),
            serde_json::to_vec(&meta).unwrap(),
        )
        .unwrap();
        let (mut registry, _) = registry_with(State::Stopped);
        // The newest macOS is not known yet: the template counts, as it does offline.
        registry.refresh_template(data.path());
        assert_eq!(registry.min_disk_gib, 128);
        assert!(registry.template.as_ref().unwrap().current);
        registry.latest_build = Some("25G83".into());
        registry.refresh_template(data.path());
        assert_eq!(registry.min_disk_gib, 128);
        // A newer macOS is installed from scratch, so the old template's size does not matter.
        registry.latest_build = Some("25H1".into());
        registry.refresh_template(data.path());
        assert_eq!(registry.min_disk_gib, store::MIN_DISK_GIB);
        let shown = registry.template.as_ref().unwrap();
        assert_eq!(shown.build, "25G83");
        assert!(!shown.current);
    }

    #[test]
    fn a_copy_that_still_needs_personalizing_is_not_set_up() {
        let (mut registry, _) = registry_with(State::Stopped);
        let record = &mut registry.entries[0].record;
        record.installed = true;
        record.setup = store::SetupProgress {
            account: true,
            sip: true,
            developer_tools: true,
            computer_use: true,
            clipboard: true,
            roots: false,
            needs_personalizing: true,
        };
        record.template = Some("25G83-abcd".into());
        assert!(!registry.entries[0].row().setup_complete);
        assert!(store::setup_allowed(State::Stopped, &registry.entries[0].record).is_ok());
        assert!(registry.entries[0].row().installed);
    }

    #[test]
    fn quit_leaves_the_disks_of_active_workers_to_their_workers() {
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(
            disks_to_release(ids.clone(), &["b".to_string()]),
            ["a", "c"]
        );
        assert_eq!(disks_to_release(ids, &[]), ["a", "b", "c"]);
    }

    #[test]
    fn quit_keeps_a_stronger_removal_request() {
        let cancel = AtomicU8::new(RUN);
        request_abort(&cancel);
        assert_eq!(cancel.load(Ordering::SeqCst), ABORT_AND_KEEP);
        let cancel = AtomicU8::new(CANCEL_AND_REMOVE);
        request_abort(&cancel);
        assert_eq!(cancel.load(Ordering::SeqCst), CANCEL_AND_REMOVE);
    }

    #[test]
    fn a_failure_after_an_accepted_delete_counts_as_the_delete() {
        let failed = || Err(Stop::Failed("boom".into()));
        assert!(matches!(
            classify(failed(), CANCEL_AND_REMOVE),
            Err(Stop::Cancelled)
        ));
        assert!(matches!(classify(failed(), RUN), Err(Stop::Failed(_))));
        assert!(matches!(
            classify(failed(), ABORT_AND_KEEP),
            Err(Stop::Failed(_))
        ));
        assert!(classify(Ok(()), CANCEL_AND_REMOVE).is_ok());
    }

    #[test]
    fn a_force_stop_failure_is_kept_whenever_it_arrives() {
        // Marked stopping before the framework call: the failure returns it to running.
        let (mut registry, id) = registry_with(State::Running);
        assert_eq!(registry.begin_force_stop(&id), Ok(State::Running));
        registry.force_stop_failed(&id, "busy".into());
        assert_eq!(registry.entries[0].state, State::Running);
        assert_eq!(registry.entries[0].detail.as_deref(), Some("busy"));
        // A failure that races ahead of the marking is not lost either.
        let (mut registry, id) = registry_with(State::Running);
        registry.force_stop_failed(&id, "busy".into());
        assert_eq!(registry.entries[0].detail.as_deref(), Some("busy"));
        // A call the framework refuses restores the state.
        let (mut registry, id) = registry_with(State::Running);
        let before = registry.begin_force_stop(&id).unwrap();
        assert_eq!(registry.entries[0].state, State::Stopping);
        registry.undo_force_stop(&id, before);
        assert_eq!(registry.entries[0].state, State::Running);
    }

    #[test]
    fn quit_and_update_close_admission_independently() {
        let mut closed = Closed::default();
        assert!(closed.check().is_ok());
        // Update closes, Quit closes, update reopens: still closed for Quit.
        closed.close_for_update(false).unwrap();
        closed.quit = true;
        closed.update = false;
        assert!(closed.check().unwrap_err().contains("quitting"));
        // The reverse: Quit reopens while an update holds admission closed.
        closed.update = true;
        closed.quit = false;
        assert!(closed.check().unwrap_err().contains("update"));
        closed.update = false;
        assert!(closed.check().is_ok());
    }

    #[test]
    fn a_refused_update_changes_nothing() {
        let mut closed = Closed::default();
        closed.quit = true;
        assert!(closed.close_for_update(true).is_err());
        assert!(closed.quit && !closed.update);
        let mut open = Closed::default();
        assert!(open.close_for_update(true).is_err());
        assert!(open.check().is_ok());
    }

    /// Replaces the shared computer inventory with one finished computer in `state`.
    fn install_computer(
        directory: &std::path::Path,
        state: State,
        approval: crate::computer_use::Approval,
    ) -> String {
        registry().entries.clear();
        let id = install_test_computer(
            directory,
            "mac-approval",
            TestComputerState::Stopped,
            approval,
        );
        registry().entries[0].state = state;
        id
    }

    #[test]
    fn computer_use_state_reports_desired_and_applied_approval() {
        use crate::computer_use::Approval;
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context);
        let id = install_computer(directory.path(), State::Stopped, Approval::Ask);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let state = runtime
            .block_on(macos_computer_use_state(app.clone(), id.clone()))
            .unwrap();
        assert_eq!(state["approval"], "ask");
        assert_eq!(state["appliedApproval"], "ask");
        assert_eq!(state["approvalApply"], "applied");
        assert!(state["state"].is_null() && state["lcuVersion"].is_null());
        assert!(runtime
            .block_on(has_macos_computer(app.clone(), id))
            .unwrap());
        assert!(!runtime
            .block_on(has_macos_computer(app, "missing".into()))
            .unwrap());
    }

    #[test]
    fn setting_the_approval_of_a_stopped_computer_waits_for_its_next_start() {
        use crate::computer_use::Approval;
        let _test_state = crate::test_support::global_state();
        reopen_after_quit();
        reopen_after_update();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context.clone());
        let id = install_computer(directory.path(), State::Stopped, Approval::Ask);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let state = runtime
            .block_on(set_macos_computer_use_approval(
                app.clone(),
                id.clone(),
                Approval::Auto,
            ))
            .unwrap();
        assert_eq!(state["approval"], "auto");
        assert_eq!(state["appliedApproval"], "unknown");
        assert_eq!(state["approvalApply"], "pending");
        let (record, _) = computer(&id).unwrap();
        assert_eq!(record.computer_use_desired_approval, Some(Approval::Auto));
        assert_eq!(record.computer_use_approval, Some(Approval::Ask));
        assert!(record.computer_use_stale());
        let saved = store::load_all(directory.path());
        assert_eq!(saved[0].computer_use_desired_approval, Some(Approval::Auto));
        assert!(!guest_computer_use::update_running(&id));
        // Putting the applied mode back is already applied.
        let state = runtime
            .block_on(set_macos_computer_use_approval(app, id, Approval::Ask))
            .unwrap();
        assert_eq!(state["approvalApply"], "applied");
    }

    #[test]
    fn setting_the_approval_of_an_unknown_computer_fails() {
        use crate::computer_use::Approval;
        let _test_state = crate::test_support::global_state();
        reopen_after_quit();
        reopen_after_update();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context);
        install_computer(directory.path(), State::Stopped, Approval::Ask);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let error = runtime
            .block_on(set_macos_computer_use_approval(
                app,
                "missing".into(),
                Approval::Auto,
            ))
            .unwrap_err();
        assert_eq!(error, "This computer no longer exists.");
    }

    #[test]
    fn closing_a_display_is_refused_during_setup_and_leaves_the_computer_running() {
        use crate::computer_use::Approval;
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(JobTestContext::new(directory.path().to_path_buf()));
        let app = macos_context(context);
        let id = install_computer(directory.path(), State::SettingUp, Approval::Ask);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let refused = runtime
            .block_on(close_macos_display(app.clone(), id.clone()))
            .unwrap_err();
        assert!(refused.contains("setup"), "{refused}");
        registry().entries[0].state = State::Running;
        runtime
            .block_on(close_macos_display(app.clone(), id.clone()))
            .unwrap();
        assert_eq!(computer(&id).unwrap().1, State::Running);
        let missing = runtime
            .block_on(close_macos_display(app, "missing".into()))
            .unwrap_err();
        assert_eq!(missing, "This computer no longer exists.");
    }

    #[test]
    fn busy_means_anything_but_stopped_or_failed() {
        assert!(!is_busy(State::Stopped));
        assert!(!is_busy(State::Failed));
        for state in [
            State::Preparing,
            State::Copying,
            State::Downloading,
            State::Installing,
            State::SettingUp,
            State::Starting,
            State::Running,
            State::Stopping,
        ] {
            assert!(is_busy(state));
        }
    }
}
