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
mod checkpoints;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod engine;
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
#[path = "macos_computers/unsupported.rs"]
mod engine;
mod guest_access;
mod guest_clipboard;
mod guest_computer_use;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod guest_screen;
mod input;
mod offline_setup;
mod personalize;
mod provision;
mod recovery;
mod restore_image;
mod setup_log;
mod store;
mod templates;

use crate::runtime;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex, Once,
    },
    time::{Duration, Instant},
};
use store::{Action, CreateRequest, DeleteMode, Layout, Record, State};
use tauri::{AppHandle, Emitter, Manager, Window};

const CHANGED_EVENT: &str = "silo://macos-computers-changed";
const NOT_SUPPORTED: &str = "macOS computers need a Mac with Apple silicon.";
const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);
const WATCH_INTERVAL: Duration = Duration::from_secs(2);
/// How long a guest may ignore a stop request before the computer counts as running again.
const STOP_IGNORED_AFTER: Duration = Duration::from_secs(20);
const GRACEFUL_QUIT: Duration = Duration::from_secs(60);
/// Quit waits less for a guest's SSH shutdown than a user-initiated Stop does.
const QUIT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);
/// Less than this is not worth an SSH attempt.
const MIN_QUIT_SHUTDOWN: Duration = Duration::from_secs(2);
const FORCED_QUIT: Duration = Duration::from_secs(8);

/// Why a creation workflow was asked to end.
const RUN: u8 = 0;
const CANCEL_AND_REMOVE: u8 = 1;
const ABORT_AND_KEEP: u8 = 2;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MacosComputer {
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
pub(crate) struct MacosComputersState {
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
            setup_complete: self.record.setup.complete(),
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
            return Finish::Remove;
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
pub(crate) fn reopen_after_quit() {
    closed().quit = false;
}

/// Admits macOS computers again after an update did not go ahead.
pub(crate) fn reopen_after_update() {
    closed().update = false;
}

/// Closes admission for an update unless a macOS computer is busy.
pub(crate) fn close_for_update() -> Result<(), String> {
    let mut closed = closed();
    let busy = registry().any_busy();
    closed.close_for_update(busy)
}

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn app_data(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|_| "Silo could not find its data folder.".to_string())
}

/// Settles a Restore that Silo was interrupted in, so the disk, the auxiliary storage and the
/// pending Restore in the record agree before anything can start the computer. A Restore that
/// can't be settled leaves its journal, which keeps the computer from starting.
fn settle_restore(layout: &Layout, mut record: Record) -> Record {
    match checkpoints::recover(layout) {
        Ok(Some(pending)) => {
            // Without the checkpoint's record the guest's computer use is unknown: stale.
            record.computer_use_version = checkpoints::find(layout, &pending.checkpoint_id)
                .ok()
                .and_then(|meta| meta.computer_use_version);
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

fn ensure_loaded(app: &AppHandle) -> Result<(), String> {
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

fn refresh_template(app: &AppHandle) {
    if let Ok(data) = app_data(app) {
        registry().refresh_template(&data);
    }
}

/// Removes the templates that nothing needs any more.
fn prune_templates(app: &AppHandle) {
    if let Ok(data) = app_data(app) {
        if let Err(message) = templates::prune_stale(&data, &protected_templates) {
            eprintln!("macOS templates could not be pruned: {message}");
        }
    }
    refresh_template(app);
}

/// Asks the framework for the newest macOS in the background, at most every few minutes, so
/// the form knows which template a new computer would use.
fn refresh_latest_build(app: &AppHandle) {
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

fn note_latest_build(app: &AppHandle, build: &str) {
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

fn emit(app: &AppHandle) {
    let _ = app.emit(CHANGED_EVENT, snapshot());
}

/// Applies `change` to one computer and tells the UI. Returns `None` for an unknown id.
fn update<R>(app: &AppHandle, id: &str, change: impl FnOnce(&mut Entry) -> R) -> Option<R> {
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

fn set_detail(app: &AppHandle, id: &str, detail: &str) {
    update(app, id, |entry| entry.detail = Some(detail.to_string()));
}

/// The files and latest record of a computer.
fn layout_and_record(app: &AppHandle, id: &str) -> Result<(Layout, Record), String> {
    let (record, _) = computer(id)?;
    Ok((Layout::new(&app_data(app)?, id), record))
}

fn set_state(app: &AppHandle, id: &str, state: State, detail: Option<String>) {
    update(app, id, |entry| {
        entry.state = state;
        entry.detail = detail;
        entry.progress = None;
        entry.since = Instant::now();
    });
}

/// Records progress and tells the UI at most once per `PROGRESS_INTERVAL`.
fn set_progress(app: &AppHandle, id: &str, fraction: f64) {
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

fn display_label(id: &str) -> String {
    format!("macos-display-{id}")
}

fn main_window_only(window: &Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Manage macOS computers from the main Silo window.".into())
    }
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
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| "The macOS computer operation failed unexpectedly.".to_string())?
}

// MARK: Commands

#[tauri::command]
pub(crate) async fn read_macos_computers(
    app: AppHandle,
    window: Window,
) -> Result<MacosComputersState, String> {
    main_window_only(&window)?;
    blocking(move || {
        ensure_loaded(&app)?;
        refresh_template(&app);
        refresh_latest_build(&app);
        Ok(snapshot())
    })
    .await
}

/// Removes the template new computers are copied from. Computers made from it keep working.
#[tauri::command]
pub(crate) async fn delete_macos_template(app: AppHandle, window: Window) -> Result<(), String> {
    main_window_only(&window)?;
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || {
        require_supported()?;
        ensure_loaded(&app)?;
        templates::remove_all(&app_data(&app)?, &protected_templates)?;
        refresh_template(&app);
        emit(&app);
        Ok(())
    })
    .await
}

#[tauri::command]
pub(crate) async fn create_macos_computer(
    app: AppHandle,
    window: Window,
    request: CreateRequest,
) -> Result<MacosComputer, String> {
    main_window_only(&window)?;
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || create(&app, request)).await
}

#[tauri::command]
pub(crate) async fn macos_computer_action(
    app: AppHandle,
    window: Window,
    id: String,
    action: Action,
) -> Result<(), String> {
    main_window_only(&window)?;
    if matches!(action, Action::Start | Action::Setup) {
        runtime::shutdown::ensure_accepting_operations()?;
    }
    blocking(move || perform(&app, &id, action)).await
}

#[tauri::command]
pub(crate) async fn open_macos_display(
    app: AppHandle,
    window: Window,
    id: String,
) -> Result<(), String> {
    main_window_only(&window)?;
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || open_display(&app, &id)).await
}

/// Saves a checkpoint of a computer: its memory too when it runs.
#[tauri::command]
pub(crate) async fn create_macos_checkpoint(
    app: AppHandle,
    window: Window,
    id: String,
    name: String,
) -> Result<(), String> {
    main_window_only(&window)?;
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || create_checkpoint(&app, &id, &name)).await
}

/// Rewinds a computer to a checkpoint. The computer stays stopped.
#[tauri::command]
pub(crate) async fn restore_macos_checkpoint(
    app: AppHandle,
    window: Window,
    id: String,
    checkpoint_id: String,
) -> Result<(), String> {
    main_window_only(&window)?;
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || restore_checkpoint(&app, &id, &checkpoint_id)).await
}

/// Creates a new stopped computer from a checkpoint's disk. It is set up in the background.
#[tauri::command]
pub(crate) async fn fork_macos_checkpoint(
    app: AppHandle,
    window: Window,
    id: String,
    checkpoint_id: String,
    new_name: String,
) -> Result<(), String> {
    main_window_only(&window)?;
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || fork_checkpoint(&app, &id, &checkpoint_id, &new_name)).await
}

#[tauri::command]
pub(crate) async fn delete_macos_checkpoint(
    app: AppHandle,
    window: Window,
    id: String,
    checkpoint_id: String,
) -> Result<(), String> {
    main_window_only(&window)?;
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || delete_checkpoint(&app, &id, &checkpoint_id)).await
}

/// Pastes this Mac's clipboard into a running computer, or copies the computer's
/// clipboard to this Mac. Only the main window can start it.
#[tauri::command]
pub(crate) async fn macos_computer_clipboard(
    app: AppHandle,
    window: Window,
    id: String,
    direction: guest_clipboard::Direction,
) -> Result<crate::viewer_clipboard::Report, String> {
    main_window_only(&window)?;
    runtime::shutdown::ensure_accepting_operations()?;
    blocking(move || guest_clipboard::run(&app, &id, direction)).await
}

// MARK: Create

/// The lowercased names of the macOS computers on this device, loaded or not.
pub(crate) fn names(app: &AppHandle) -> Vec<String> {
    app_data(app)
        .map(|data| store::names(&data))
        .unwrap_or_default()
}

fn create(app: &AppHandle, request: CreateRequest) -> Result<MacosComputer, String> {
    require_supported()?;
    ensure_loaded(app)?;
    let data = app_data(app)?;
    // Held until the computer is registered and saved, so a Linux creation sees it.
    let reservation =
        crate::computer_names::reserve(&[request.name.clone()], &|| runtime::computer_names(app))?;
    let (record, cancel, row) = {
        let _admitted = admission()?;
        let mut registry = registry();
        let existing: Vec<Record> = registry.entries.iter().map(|e| e.record.clone()).collect();
        registry.refresh_template(&data);
        let min_disk = registry.min_disk_gib;
        store::validate_request(&request, engine::host_limits(), &existing, min_disk)?;
        let record = store::new_record(&request, engine::random_mac());
        // A crash after this point leaves a visible computer that reports the interruption.
        store::save(&Layout::new(&data, &record.id), &record)?;
        let entry = Entry::new(record.clone(), State::Preparing, None);
        let (cancel, row) = (entry.cancel.clone(), entry.row());
        registry.entries.push(entry);
        (record, cancel, row)
    };
    drop(reservation);
    emit(app);
    let app = app.clone();
    std::thread::spawn(move || create_workflow(&app, &data, record, &cancel));
    Ok(row)
}

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

fn create_workflow(app: &AppHandle, data: &std::path::Path, record: Record, cancel: &AtomicU8) {
    let id = record.id.clone();
    let layout = Layout::new(data, &id);
    let result = run_creation(app, data, record, &layout, cancel);
    end_workflow(app, &id, &layout, result);
}

/// Provisions an installed computer again, resuming at the first unfinished step.
fn setup_workflow(app: &AppHandle, data: &std::path::Path, mut record: Record, cancel: &AtomicU8) {
    let id = record.id.clone();
    let layout = Layout::new(data, &id);
    let result = run_setup(app, &layout, &mut record, cancel, None);
    end_workflow(app, &id, &layout, result);
}

fn end_workflow(app: &AppHandle, id: &str, layout: &Layout, result: Result<(), Stop>) {
    // The outcome is decided under the lock a Delete takes, so a Delete accepted
    // before it is never lost.
    let finish = registry().finish_workflow(id, result);
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
}

/// Deletes a computer's files, after making sure this Mac no longer holds its disk image.
fn remove_computer(layout: &Layout) -> Result<(), String> {
    offline_setup::ensure_detached(&layout.disk())?;
    store::remove(layout)
}

fn run_creation(
    app: &AppHandle,
    data: &std::path::Path,
    mut record: Record,
    layout: &Layout,
    cancel: &AtomicU8,
) -> Result<(), Stop> {
    let id = record.id.clone();
    let cancelled = || cancel.load(Ordering::SeqCst) != RUN;
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
    app: &AppHandle,
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
        computer_use: true,
        clipboard: true,
        needs_personalizing: true,
    };
    Ok(true)
}

/// Runs the provisioning steps, then leaves the computer stopped. A computer that was just
/// installed and set up, and never started by the user, becomes the template.
fn run_setup(
    app: &AppHandle,
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
fn save_template(app: &AppHandle, layout: &Layout, record: &Record, refresh: bool) {
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
fn remove_restore_images(app: &AppHandle, log_to: Option<&str>) {
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

fn perform(app: &AppHandle, id: &str, action: Action) -> Result<(), String> {
    ensure_loaded(app)?;
    match action {
        Action::Start => start(app, id),
        Action::Stop => stop(app, id),
        Action::ForceStop => force_stop(app, id),
        Action::Delete => delete(app, id),
        Action::Setup => begin_setup(app, id),
    }
}

/// The name and retained-log folder of the macOS computer `id`, for the Logs page.
pub(crate) fn log_target(app: &AppHandle, id: &str) -> Option<(String, std::path::PathBuf)> {
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

fn start(app: &AppHandle, id: &str) -> Result<(), String> {
    require_supported()?;
    let data = app_data(app)?;
    // The state check and the change to `starting` happen under one lock.
    let record = {
        let _admitted = admission()?;
        let mut registry = registry();
        let entry = registry
            .entries
            .iter_mut()
            .find(|entry| entry.record.id == id)
            .ok_or("This computer no longer exists.")?;
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
        if started != entry.record {
            store::save(&Layout::new(&data, id), &started)?;
        }
        let pending = entry.record.pending_restore.clone();
        entry.record = started;
        entry.state = State::Starting;
        entry.attempt += 1;
        entry.detail = None;
        entry.since = Instant::now();
        (entry.record.clone(), pending, entry.attempt)
    };
    let (record, pending, attempt) = record;
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
            Ok(())
        }
        Err(message) => {
            // Nothing may report the computer as stopped while its machine can still run.
            let released = ensure_released(app, id, attempt);
            registry().settle_failed_start(id, attempt, released);
            emit(app);
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
fn ensure_released(app: &AppHandle, id: &str, attempt: u64) -> Result<(), String> {
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
    app: &AppHandle,
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
    app: &AppHandle,
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

fn begin_setup(app: &AppHandle, id: &str) -> Result<(), String> {
    require_supported()?;
    let data = app_data(app)?;
    let (record, cancel) = {
        let _admitted = admission()?;
        let mut registry = registry();
        let entry = registry
            .entries
            .iter_mut()
            .find(|entry| entry.record.id == id)
            .ok_or("This computer no longer exists.")?;
        if entry.deleting {
            return Err("This computer is being deleted.".into());
        }
        store::setup_allowed(entry.state, &entry.record)?;
        entry.cancel.store(RUN, Ordering::SeqCst);
        entry.state = State::SettingUp;
        entry.detail = None;
        entry.progress = None;
        entry.since = Instant::now();
        (entry.record.clone(), entry.cancel.clone())
    };
    emit(app);
    let app = app.clone();
    std::thread::spawn(move || setup_workflow(&app, &data, record, &cancel));
    Ok(())
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
fn shutdown_over_ssh(app: &AppHandle, record: &Record, timeout: Duration) -> bool {
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
    app: &AppHandle,
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

fn stop(app: &AppHandle, id: &str) -> Result<(), String> {
    // The ownership check and the claim of the transition happen under one lock.
    let (record, before) = {
        let mut registry = registry();
        let entry = registry
            .entry(id)
            .ok_or("This computer no longer exists.")?;
        if !matches!(entry.state, State::Running | State::Stopping) {
            return Err("This computer isn't running.".into());
        }
        if entry.operation.is_some() {
            return Err(checkpoints::BUSY.into());
        }
        let before = entry.state;
        if before == State::Running {
            entry.state = State::Stopping;
            entry.since = Instant::now();
        }
        (entry.record.clone(), before)
    };
    emit(app);
    let note = match request_graceful_stop(app, &record, SSH_SHUTDOWN_TIMEOUT) {
        Ok(note) => note,
        Err(message) => {
            registry().undo_force_stop(id, before);
            emit(app);
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
    Ok(())
}

fn force_stop(app: &AppHandle, id: &str) -> Result<(), String> {
    let before = registry().begin_force_stop(id)?;
    emit(app);
    if let Err(message) = engine::force_stop(app, id) {
        registry().undo_force_stop(id, before);
        emit(app);
        return Err(message);
    }
    Ok(())
}

fn delete(app: &AppHandle, id: &str) -> Result<(), String> {
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
fn force_stop_failed(app: &AppHandle, id: &str, message: String) {
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
fn machine_stopped(app: &AppHandle, id: &str, generation: u64, error: Option<String>) {
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
    app: &'a AppHandle,
    id: String,
    record: Record,
    state: State,
    cancel: Arc<AtomicU8>,
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

/// Claims the computer for one checkpoint operation, if nothing else is using it.
fn begin_operation<'a>(
    app: &'a AppHandle,
    id: &str,
    kind: checkpoints::OperationKind,
) -> Result<Held<'a>, String> {
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
    emit(app);
    Ok(Held {
        app,
        id: id.to_string(),
        record,
        state,
        cancel,
    })
}

/// The machine of a computer as the checkpoint workflows reach it.
struct LiveMachine<'a> {
    app: &'a AppHandle,
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

fn log_line(app: &AppHandle, id: &str, line: &str) {
    if let Some(log) = setup_log::SetupLog::open(app, id) {
        log.line(line);
    }
}

/// Reads the checkpoints on disk again, after an operation that may have changed them.
fn refresh_checkpoints(app: &AppHandle, id: &str, layout: &Layout) {
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

fn create_checkpoint(app: &AppHandle, id: &str, name: &str) -> Result<(), String> {
    require_supported()?;
    ensure_loaded(app)?;
    let name = checkpoints::validate_name(name)?;
    let data = app_data(app)?;
    let held = begin_operation(app, id, checkpoints::OperationKind::Capture)?;
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
    };
    let result = checkpoints::create(&subject, &name, checkpoints::Reason::Manual);
    refresh_checkpoints(app, id, &layout);
    result.map(|_| ())
}

fn restore_checkpoint(app: &AppHandle, id: &str, checkpoint_id: &str) -> Result<(), String> {
    require_supported()?;
    ensure_loaded(app)?;
    let data = app_data(app)?;
    let held = begin_operation(app, id, checkpoints::OperationKind::Restore)?;
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
    };
    let restored = checkpoints::restore(&subject, checkpoint_id);
    // The recovery checkpoint exists whether or not the rest succeeded.
    refresh_checkpoints(app, id, &layout);
    let pending = restored?;
    let (mut record, _) = computer(id)?;
    record.pending_restore = Some(pending);
    // The guest comes back with the computer use it had when the checkpoint was taken.
    record.computer_use_version = target.computer_use_version.clone();
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

fn delete_checkpoint(app: &AppHandle, id: &str, checkpoint_id: &str) -> Result<(), String> {
    require_supported()?;
    ensure_loaded(app)?;
    let data = app_data(app)?;
    let held = begin_operation(app, id, checkpoints::OperationKind::Delete)?;
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
fn fork_checkpoint(
    app: &AppHandle,
    id: &str,
    checkpoint_id: &str,
    new_name: &str,
) -> Result<(), String> {
    require_supported()?;
    ensure_loaded(app)?;
    let data = app_data(app)?;
    let new_name = new_name.trim().to_string();
    // Held until the fork is registered and saved, so a Linux creation sees it.
    let reservation =
        crate::computer_names::reserve(&[new_name.clone()], &|| runtime::computer_names(app))?;
    let held = begin_operation(app, id, checkpoints::OperationKind::Fork)?;
    let source = Layout::new(&data, id);
    let checkpoint = checkpoints::find(&source, checkpoint_id)?;
    let space = templates::reserve_space(&data, templates::COPY_ESTIMATE)?;
    let (record, cancel) = {
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
        (record, cancel)
    };
    drop(reservation);
    emit(app);
    let fork_id = record.id.clone();
    let layout = Layout::new(&data, &fork_id);
    log_line(
        app,
        id,
        &format!(
            "forking checkpoint \"{}\" ({}) into {}",
            checkpoint.name, checkpoint.id, record.name
        ),
    );
    let copied = copy_for_fork(&source, &checkpoint.id, &layout, record, &fork_id);
    // The source is free again; the rest happens on the fork.
    drop(held);
    match copied {
        Ok(record) => {
            let (app, data) = (app.clone(), data.clone());
            emit(&app);
            std::thread::spawn(move || fork_workflow(&app, &data, record, &cancel, space));
            Ok(())
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
    app: &AppHandle,
    data: &std::path::Path,
    mut record: Record,
    cancel: &AtomicU8,
    space: templates::SpaceReservation,
) {
    let id = record.id.clone();
    let layout = Layout::new(data, &id);
    let result = run_setup(app, &layout, &mut record, cancel, Some(space));
    end_workflow(app, &id, &layout, result);
}

// MARK: Display

fn open_display(app: &AppHandle, id: &str) -> Result<(), String> {
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
fn show_display(app: &AppHandle, id: &str, title: &str, toolbar: bool) -> Result<(), String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    let label = display_label(id);
    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.show();
        return existing
            .set_focus()
            .map_err(|_| "Could not focus the display.".to_string());
    }
    // A blank page that loads nothing: the label matches no capability, so the window
    // has no access to Silo's commands. The machine's screen is a native view on top.
    let blank = tauri::Url::parse("about:blank").map_err(|error| error.to_string())?;
    let display = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(blank))
        .title(title)
        .inner_size(1440., 900.)
        .min_inner_size(640., 400.)
        .resizable(true)
        .build()
        .map_err(|_| "Silo could not open the display.".to_string())?;
    let (handle, closed) = (app.clone(), id.to_string());
    display.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            engine::detach_display(&handle, &closed);
            update(&handle, &closed, |entry| entry.display_open = false);
        }
    });
    update(app, id, |entry| entry.display_open = true);
    if let Err(message) = engine::attach_display(app, id, &display, toolbar) {
        let _ = display.destroy();
        return Err(message);
    }
    Ok(())
}

fn close_display(app: &AppHandle, id: &str) {
    if let Some(display) = app.get_webview_window(&display_label(id)) {
        // The destroy waits for the main thread, which may be the caller.
        std::thread::spawn(move || {
            let _ = display.destroy();
        });
    }
}

// MARK: Watching

/// Notices machine states the framework did not report through its delegate: a
/// stop the guest declined, and a machine that ended without a callback.
fn watch(app: &AppHandle) {
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

fn reconcile(app: &AppHandle, samples: &[engine::Sample]) {
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
fn stop_finished_without_machine(app: &AppHandle, id: &str) {
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
pub(crate) fn stop_all(app: &AppHandle, deadline: Option<Instant>) -> Result<(), String> {
    let stopped = stop_busy(app, deadline);
    // A host-attached disk image outlives Silo; none may be left behind.
    let detached = release_disks(app);
    stopped.and(detached)
}

/// Detaches the disk image of every macOS computer on disk from this Mac, except
/// those a creation or setup still owns: their worker detaches its own image, and a
/// forced detach under it would corrupt the patch. Those report through `stop_busy`.
fn release_disks(app: &AppHandle) -> Result<(), String> {
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

fn stop_busy(app: &AppHandle, deadline: Option<Instant>) -> Result<(), String> {
    // From here on nothing new is admitted; anything admitted before is in the snapshot.
    closed().quit = true;
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
    for (_, state, cancel, operating) in &busy {
        if is_creating(*state) || *operating {
            request_abort(cancel);
        }
    }
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
            computer_use: true,
            clipboard: true,
            needs_personalizing: false,
        };
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
