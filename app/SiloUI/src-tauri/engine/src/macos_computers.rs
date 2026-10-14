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
#[path = "macos_computers/provision.rs"]
mod provision;
#[path = "macos_computers/recovery.rs"]
mod recovery;
#[path = "macos_computers/remote_display.rs"]
mod remote_display;
#[path = "macos_computers/restore_image.rs"]
mod restore_image;
#[path = "macos_computers/setup_log.rs"]
mod setup_log;
#[path = "macos_computers/store.rs"]
mod store;
#[path = "macos_computers/templates.rs"]
mod templates;

use crate::runtime;
pub use guest_clipboard::Direction as ClipboardDirection;
pub(crate) use remote_display::DisplaySession;
use serde::Serialize;
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
pub use store::{Action, CreateRequest, ExpectedResources};
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
            needs_personalizing: self.record.setup.needs_personalizing,
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

fn closed() -> std::sync::MutexGuard<'static, Closed> {
    CLOSED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn admission() -> Result<std::sync::MutexGuard<'static, Closed>, String> {
    let closed = closed();
    closed.check()?;
    Ok(closed)
}

/// Admits macOS computers again after a Quit was cancelled.
pub fn reopen_after_quit() {
    closed().quit = false;
}

/// Admits macOS computers again after an update did not go ahead.
/// A macOS refusal a client can act on, with its own stable error code in the API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NotRunning,
    NeedsPersonalizing,
    SetupIncomplete,
    ScreenSharingUnavailable,
    RunningLimit,
}

/// Recognises the refusals above in an engine message. The messages are the engine's own
/// sentences, so the match is on their stable phrases and tested against the constants.
pub fn classify_refusal(message: &str) -> Option<Refusal> {
    if message.contains(store::NEEDS_PERSONALIZING) {
        Some(Refusal::NeedsPersonalizing)
    } else if message.contains(engine::LIMIT_EXCEEDED) {
        Some(Refusal::RunningLimit)
    } else if message.contains(remote_display::SHARING_UNAVAILABLE)
        || message.contains("Screen Sharing accepted TCP")
    {
        Some(Refusal::ScreenSharingUnavailable)
    } else if message.contains("Setup has not finished") || message.contains("still being set up") {
        Some(Refusal::SetupIncomplete)
    } else if message.contains("isn't running") || message.contains("is not running") {
        Some(Refusal::NotRunning)
    } else {
        None
    }
}

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

fn emit(app: &MacosContext) {
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
    accept_macos_job_with_id(registry, accepted.job_id, gate)
}

fn accept_macos_job_with_id(
    registry: Arc<crate::jobs::JobRegistry>,
    job_id: silo_api::jobs::JobId,
    gate: &'static runtime::operation_gate::OperationGate,
) -> Result<AcceptedMacosJob, String> {
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
    let acquired = job.gate.kind(kind).acquire_for_job(
        scope,
        computer_name,
        label,
        Some(job.job_id.as_str().to_owned()),
        job.job_id.clone(),
        Some(job.cancellation.clone()),
    );
    finish_admission(context, job, acquired, finish_on_error)
}

/// Waits in the queue for a device-wide turn that deletes `ids`, linked to `job`.
fn admit_macos_removal(
    context: &dyn EngineContext,
    job: &AcceptedMacosJob,
    ids: &[String],
    label: &str,
) -> Result<runtime::operation_gate::OperationGuard<'static>, String> {
    let acquired = job.gate.removing_for_job(
        ids,
        label,
        job.job_id.clone(),
        Some(job.cancellation.clone()),
    );
    finish_admission(context, job, acquired, true)
}

fn finish_admission(
    context: &dyn EngineContext,
    job: &AcceptedMacosJob,
    acquired: Result<
        runtime::operation_gate::OperationGuard<'static>,
        runtime::operation_gate::GateError,
    >,
    finish_on_error: bool,
) -> Result<runtime::operation_gate::OperationGuard<'static>, String> {
    let guard = match acquired {
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
        macos_job_finished(job.job_id.clone(), Err((job_error_code(&message), message))),
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

/// The stable API code of a failed macOS job: an actionable refusal keeps its own code.
fn job_error_code(message: &str) -> silo_api::jobs::ErrorCode {
    use silo_api::jobs::ErrorCode;
    match classify_refusal(message) {
        Some(Refusal::NotRunning) => ErrorCode::NotRunning,
        Some(Refusal::NeedsPersonalizing) => ErrorCode::NeedsPersonalizing,
        Some(Refusal::SetupIncomplete) => ErrorCode::SetupIncomplete,
        Some(Refusal::ScreenSharingUnavailable) => ErrorCode::ScreenSharingUnavailable,
        Some(Refusal::RunningLimit) => ErrorCode::RunningLimit,
        None if message.contains("no longer exists") => ErrorCode::NotFound,
        None => ErrorCode::Internal,
    }
}

fn lifecycle_job_outcome(
    result: Result<(), String>,
) -> Result<(), (silo_api::jobs::ErrorCode, String)> {
    result.map_err(|message| (job_error_code(&message), message))
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
            Err(Stop::Failed(message)) => Err((job_error_code(&message), message)),
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
    accept_loaded_lifecycle_job_with_id(app, id, method, None)
}

fn accept_loaded_lifecycle_job_with_id(
    app: &MacosContext,
    id: &str,
    method: &str,
    job_id: Option<silo_api::jobs::JobId>,
) -> Result<(AcceptedMacosJob, PathBuf, String), String> {
    let job = match job_id {
        Some(job_id) => {
            accept_macos_job_with_id(app.engine.job_registry()?, job_id, &runtime::OPERATIONS)?
        }
        None => accept_macos_job(app.engine.as_ref(), method, &runtime::OPERATIONS)?,
    };
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

/// Creates a macOS computer under a job id accepted by a canonical API caller.
pub async fn create_macos_computer_with_job(
    app: MacosContext,
    request: CreateRequest,
    job_id: silo_api::jobs::JobId,
) -> Result<MacosComputer, String> {
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || create_with_job(&app, request, job_id)).await
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

/// Runs a macOS lifecycle operation under a job id accepted by a canonical API caller.
pub async fn macos_computer_action_with_job(
    app: MacosContext,
    id: String,
    action: Action,
    job_id: silo_api::jobs::JobId,
    method: &'static str,
) -> Result<(), String> {
    if matches!(
        action,
        Action::Start | Action::Setup | Action::ChangeResources { .. }
    ) {
        runtime::shutdown::ensure_accepting_operations()?;
    }
    blocking(move || perform_with_job(&app, &id, action, Some((job_id, method)))).await
}

pub async fn open_macos_display(app: MacosContext, id: String) -> Result<(), String> {
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || open_display(&app, &id)).await
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
pub fn display_session(app: &MacosContext, id: &str) -> Result<DisplaySession, String> {
    runtime::shutdown::ensure_accepting_operations()?;
    ensure_loaded(app)?;
    remote_display::session(app, id)
}

/// A connection to the Screen Sharing port of a running computer, for the owner to relay.
pub fn display_stream(app: &MacosContext, id: &str) -> Result<std::net::TcpStream, String> {
    runtime::shutdown::ensure_accepting_operations()?;
    ensure_loaded(app)?;
    remote_display::connect(app, id)
}

/// Resizes a running computer's display to a viewer's window size (pixels, clamped).
pub fn resize_display(
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

fn create(app: &MacosContext, request: CreateRequest) -> Result<MacosComputer, String> {
    create_with_support_check(app, request, require_supported)
}

fn create_with_job(
    app: &MacosContext,
    request: CreateRequest,
    job_id: silo_api::jobs::JobId,
) -> Result<MacosComputer, String> {
    create_with_support_check_and_job(app, request, require_supported, Some(job_id))
}

fn create_with_support_check(
    app: &MacosContext,
    request: CreateRequest,
    support_check: impl FnOnce() -> Result<(), String>,
) -> Result<MacosComputer, String> {
    create_with_support_check_and_job(app, request, support_check, None)
}

fn create_with_support_check_and_job(
    app: &MacosContext,
    request: CreateRequest,
    support_check: impl FnOnce() -> Result<(), String>,
    job_id: Option<silo_api::jobs::JobId>,
) -> Result<MacosComputer, String> {
    support_check()?;
    let data = app_data(app)?;
    create_after_acceptance_with_job(
        app,
        data,
        request,
        &runtime::OPERATIONS,
        create_workflow,
        job_id,
    )
}

fn create_after_acceptance(
    app: &MacosContext,
    data: PathBuf,
    request: CreateRequest,
    gate: &'static runtime::operation_gate::OperationGate,
    workflow: impl FnOnce(&MacosContext, &std::path::Path, Record, &AtomicU8) + Send + 'static,
) -> Result<MacosComputer, String> {
    create_after_acceptance_with_job(app, data, request, gate, workflow, None)
}

fn create_after_acceptance_with_job(
    app: &MacosContext,
    data: PathBuf,
    request: CreateRequest,
    gate: &'static runtime::operation_gate::OperationGate,
    workflow: impl FnOnce(&MacosContext, &std::path::Path, Record, &AtomicU8) + Send + 'static,
    requested_job_id: Option<silo_api::jobs::JobId>,
) -> Result<MacosComputer, String> {
    let launch = move |job| {
        let (sender, receiver) = std::sync::mpsc::channel();
        let app = app.clone();
        std::thread::spawn(move || create_job_worker(&app, data, request, job, sender, workflow));
        receiver
            .recv()
            .map_err(|_| "The macOS computer creation worker stopped unexpectedly.".to_owned())?
    };
    match requested_job_id {
        Some(job_id) => {
            let registry = app.engine.job_registry()?;
            let job = accept_macos_job_with_id(registry, job_id, gate)?;
            launch(job)
        }
        None => launch_macos_job(app.engine.as_ref(), "computers.create", gate, launch),
    }
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
    ensure_loaded(app)?;
    let (record, _) = computer(id)?;
    with_computer_operation(id, &record.name, kind, label, work)
}

fn perform(app: &MacosContext, id: &str, action: Action) -> Result<(), String> {
    perform_with_job(app, id, action, None)
}

fn perform_with_job(
    app: &MacosContext,
    id: &str,
    action: Action,
    requested_job: Option<(silo_api::jobs::JobId, &'static str)>,
) -> Result<(), String> {
    match action {
        Action::Start => start_with_job(app, id, requested_job.map(|(job_id, _)| job_id)),
        Action::Stop => stop_with_job(app, id, requested_job.map(|(job_id, _)| job_id)),
        Action::ForceStop => force_stop(app, id),
        Action::Delete => delete_action(app, id, requested_job.map(|(job_id, _)| job_id)),
        Action::Setup => begin_setup_with_job(app, id, requested_job.map(|(job_id, _)| job_id)),
        Action::ChangeResources {
            cpus,
            memory_gib,
            expected,
        } => change_resources_action(
            app,
            id,
            cpus,
            memory_gib,
            expected,
            requested_job.map(|(job_id, _)| job_id),
        ),
    }
}

/// Deletes a computer. Under a canonical job the deletion waits in the queue as
/// "Deleting <name>", can be cancelled while waiting, and ends the job when it is done.
fn delete_action(
    app: &MacosContext,
    id: &str,
    requested_job: Option<silo_api::jobs::JobId>,
) -> Result<(), String> {
    let job = requested_job
        .map(|job_id| {
            accept_macos_job_with_id(app.engine.job_registry()?, job_id, &runtime::OPERATIONS)
        })
        .transpose()?;
    let result = (|| {
        ensure_loaded(app).map_err(|message| (false, message))?;
        let (record, _) = computer(id).map_err(|message| (false, message))?;
        let label = format!("Deleting {}", record.name);
        let ids = [id.to_owned()];
        let _guard = match &job {
            Some(job) => admit_macos_removal(app.engine.as_ref(), job, &ids, &label)
                .map_err(|message| (true, message))?,
            None => runtime::OPERATIONS
                .removing(&ids, &label)
                .map_err(|error| (false, error.to_string()))?,
        };
        delete(app, id).map_err(|message| (false, message))
    })();
    finish_job_after_work(app, id, job, result)
}

/// Changes a stopped computer's CPU and memory, queued like any other computer operation.
fn change_resources_action(
    app: &MacosContext,
    id: &str,
    cpus: u64,
    memory_gib: u64,
    expected: store::ExpectedResources,
    requested_job: Option<silo_api::jobs::JobId>,
) -> Result<(), String> {
    let job = requested_job
        .map(|job_id| {
            accept_macos_job_with_id(app.engine.job_registry()?, job_id, &runtime::OPERATIONS)
        })
        .transpose()?;
    let result = (|| {
        ensure_loaded(app).map_err(|message| (false, message))?;
        let (record, _) = computer(id).map_err(|message| (false, message))?;
        let label = "Saving computer settings";
        let _guard = match &job {
            Some(job) => admit_macos_job(
                app.engine.as_ref(),
                job,
                runtime::operation_gate::OperationKind::ComputerConfiguration,
                runtime::operation_gate::Scope::Computer { id: id.to_owned() },
                Some(record.name.clone()),
                label,
            )
            .map_err(|message| (true, message))?,
            None => acquire_computer_operation(
                &runtime::OPERATIONS,
                id,
                &record.name,
                runtime::operation_gate::OperationKind::ComputerConfiguration,
                label,
            )
            .map_err(|message| (false, message))?,
        };
        change_resource_configuration(app, id, cpus, memory_gib, expected)
            .map_err(|message| (false, message))
    })();
    finish_job_after_work(app, id, job, result)
}

/// Ends the job of a queued action. Admission failures already ended it (`true`).
fn finish_job_after_work(
    app: &MacosContext,
    id: &str,
    job: Option<AcceptedMacosJob>,
    result: Result<(), (bool, String)>,
) -> Result<(), String> {
    let already_finished = matches!(&result, Err((true, _)));
    let result = result.map_err(|(_, message)| message);
    if let Some(job) = job {
        if !already_finished {
            finish_macos_job(
                app.engine.as_ref(),
                Some(id),
                job.job_id,
                lifecycle_job_outcome(result.clone()),
            );
        }
    }
    result
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

fn start(app: &MacosContext, id: &str) -> Result<(), String> {
    start_with_job(app, id, None)
}

fn start_with_job(
    app: &MacosContext,
    id: &str,
    requested_job_id: Option<silo_api::jobs::JobId>,
) -> Result<(), String> {
    require_supported()?;
    let (job, data, name) =
        accept_loaded_lifecycle_job_with_id(app, id, "computers.start", requested_job_id)?;
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
            update(app, id, |entry| {
                if entry.state == State::Starting && entry.attempt == attempt {
                    entry.state = State::Running;
                    entry.since = Instant::now();
                    entry.detail = note;
                }
            });
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
    begin_setup_with_job(app, id, None)
}

fn begin_setup_with_job(
    app: &MacosContext,
    id: &str,
    requested_job_id: Option<silo_api::jobs::JobId>,
) -> Result<(), String> {
    require_supported()?;

    let (job, data, name) =
        accept_loaded_lifecycle_job_with_id(app, id, "macos.setup.retry", requested_job_id)?;
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
    stop_with_job(app, id, None)
}

fn stop_with_job(
    app: &MacosContext,
    id: &str,
    requested_job_id: Option<silo_api::jobs::JobId>,
) -> Result<(), String> {
    let (job, _, name) =
        accept_loaded_lifecycle_job_with_id(app, id, "computers.stop", requested_job_id)?;
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
            Ok(()) => registry().entries.retain(|entry| entry.record.id != id),
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
    let (app, id) = (app.clone(), id.to_string());
    // Deferred so the framework callback that reported the stop has returned before
    // its machine is released.
    engine::defer(move || {
        // An event of an earlier machine of this computer changes nothing.
        if !engine::release_slot(&id, generation) {
            return;
        }
        close_display(&app, &id);
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
    _operation: runtime::operation_gate::OperationGuard<'static>,
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
    drop(admission()?);
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
    let operation = admit_checkpoint_job(
        app.engine.as_ref(),
        job,
        gate_kind,
        runtime::operation_gate::Scope::Computer { id: id.to_owned() },
        Some(computer_name),
        label,
    )?;
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
    MACOS_CHECKPOINT_JOBS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id.to_owned(), job.job_id.clone());
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
        let (id, machine, generation) = (&sample.id, &sample.state, sample.generation);
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
                machine_stopped(app, id, generation, detail);
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

    #[test]
    fn engine_refusals_are_classified_for_stable_api_codes() {
        let cases = [
            ("This computer isn't running.", Refusal::NotRunning),
            (
                "The computer is not running. Start it to show its screen.",
                Refusal::NotRunning,
            ),
            (store::NEEDS_PERSONALIZING, Refusal::NeedsPersonalizing),
            (engine::LIMIT_EXCEEDED, Refusal::RunningLimit),
            (
                remote_display::SHARING_UNAVAILABLE,
                Refusal::ScreenSharingUnavailable,
            ),
            (
                "Setup has not finished. The screen is available once the computer has been set up for computer use.",
                Refusal::SetupIncomplete,
            ),
        ];
        for (message, refusal) in cases {
            assert_eq!(classify_refusal(message), Some(refusal), "{message}");
        }
        assert_eq!(classify_refusal("Stop the computer first."), None);
        assert_eq!(
            job_error_code("This computer isn't running."),
            silo_api::jobs::ErrorCode::NotRunning
        );
        assert_eq!(
            job_error_code("This computer no longer exists."),
            silo_api::jobs::ErrorCode::NotFound
        );
        assert_eq!(
            job_error_code("Stop the computer first."),
            silo_api::jobs::ErrorCode::Internal
        );
    }

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
    fn a_waiting_macos_delete_shows_its_job_and_cancels_before_removing_anything() {
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
        let accepted = accept_macos_job(context.as_ref(), "computers.delete", gate).unwrap();
        let job_id = accepted.job_id.clone();
        let removed = Arc::new(AtomicUsize::new(0));
        let worker_context = context.clone();
        let worker_removed = removed.clone();
        let worker = std::thread::spawn(move || {
            let admitted = admit_macos_removal(
                worker_context.as_ref(),
                &accepted,
                &["computer-1".to_owned()],
                "Deleting Mac",
            );
            if admitted.is_ok() {
                worker_removed.fetch_add(1, Ordering::SeqCst);
            }
            admitted.map(drop)
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        while !gate
            .snapshot()
            .waiting
            .iter()
            .any(|entry| entry.job_id.as_ref() == Some(&job_id) && entry.label == "Deleting Mac")
        {
            assert!(
                Instant::now() < deadline,
                "the deletion did not enter the wait queue"
            );
            std::thread::yield_now();
        }
        assert!(gate.removal_queued("computer-1"));
        context.job_registry().unwrap().cancel(&job_id).unwrap();

        assert_eq!(
            worker.join().unwrap().unwrap_err(),
            "The operation was cancelled."
        );
        assert_eq!(removed.load(Ordering::SeqCst), 0);
        let outcome = context
            .job_registry()
            .unwrap()
            .get(&job_id)
            .unwrap()
            .unwrap()
            .outcome
            .unwrap();
        assert_eq!(
            outcome.error.unwrap().code,
            silo_api::jobs::ErrorCode::Cancelled
        );
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
