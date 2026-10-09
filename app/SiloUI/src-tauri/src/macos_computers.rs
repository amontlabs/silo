//! macOS computers: macOS guests on Apple silicon through Virtualization.framework,
//! shown in a native display window. They live in the Silo process, apart from the
//! Linux computers MicroSandbox runs.
//!
//! This module owns the state the UI sees and the workflows around it (creating,
//! setting up, starting, stopping, deleting, Quit). `store` and `restore_image` are
//! plain Rust; `engine` holds every Virtualization.framework call. `provision`
//! prepares an installed computer for computer use, with `offline_setup`,
//! `guest_access`, `recovery`, `guest_computer_use` and `guest_clipboard` behind it.
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
mod provision;
mod recovery;
mod restore_image;
mod store;

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
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MacosComputersState {
    supported: bool,
    unsupported_reason: Option<String>,
    computers: Vec<MacosComputer>,
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
        }
    }
}

/// What `Registry::finish_workflow` decided for the computer's files.
enum Finish {
    Remove,
    Kept,
}

struct Registry {
    loaded: bool,
    entries: Vec<Entry>,
}

impl Registry {
    fn entry(&mut self, id: &str) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|entry| entry.record.id == id)
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
    fn begin_force_stop(&mut self, id: &str) -> Option<State> {
        let entry = self.entry(id)?;
        let before = entry.state;
        if entry.state == State::Running {
            entry.state = State::Stopping;
            entry.since = Instant::now();
        }
        entry.detail = None;
        Some(before)
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
    let busy = registry().entries.iter().any(|entry| is_busy(entry.state));
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
                Entry::new(record, state, detail)
            })
            .collect();
    }
    registry.loaded = true;
    Ok(())
}

fn snapshot() -> MacosComputersState {
    let reason = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        engine::unsupported_reason()
    } else {
        Some(NOT_SUPPORTED.to_string())
    };
    MacosComputersState {
        supported: reason.is_none(),
        unsupported_reason: reason,
        computers: registry().entries.iter().map(Entry::row).collect(),
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
        Ok(snapshot())
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
        store::validate_request(&request, engine::host_limits(), &existing)?;
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
    let result = run_setup(app, &layout, &mut record, cancel);
    end_workflow(app, &id, &layout, result);
}

fn end_workflow(app: &AppHandle, id: &str, layout: &Layout, result: Result<(), Stop>) {
    // The outcome is decided under the lock a Delete takes, so a Delete accepted
    // before it is never lost.
    let finish = registry().finish_workflow(id, result);
    match finish {
        Finish::Remove => {
            let removal = remove_computer(layout);
            registry().finish_cancelled(id, removal);
        }
        Finish::Kept => {}
    }
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
    let latest = engine::fetch_latest()?;
    check()?;
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
    run_setup(app, layout, &mut record, cancel)
}

/// Runs the provisioning steps, then leaves the computer stopped.
fn run_setup(
    app: &AppHandle,
    layout: &Layout,
    record: &mut Record,
    cancel: &AtomicU8,
) -> Result<(), Stop> {
    provision::run(app, layout, record, cancel)
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
        entry.state = State::Starting;
        entry.detail = None;
        entry.since = Instant::now();
        entry.record.clone()
    };
    emit(app);
    watch(app);
    let layout = Layout::new(&data, id);
    let started = offline_setup::ensure_detached(&layout.disk())
        .and_then(|()| engine::start(app, &record, &layout));
    match started {
        Ok(()) => {
            update(app, id, |entry| {
                if entry.state == State::Starting {
                    entry.state = State::Running;
                    entry.since = Instant::now();
                }
            });
            Ok(())
        }
        Err(message) => {
            set_state(app, id, State::Stopped, None);
            Err(message)
        }
    }
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
    let (record, state) = computer(id)?;
    if !matches!(state, State::Running | State::Stopping) {
        return Err("This computer isn't running.".into());
    }
    let note = request_graceful_stop(app, &record, SSH_SHUTDOWN_TIMEOUT)?;
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
    let (_, state) = computer(id)?;
    if !matches!(state, State::Running | State::Starting | State::Stopping) {
        return Err("This computer isn't running.".into());
    }
    let before = registry().begin_force_stop(id);
    emit(app);
    if let Err(message) = engine::force_stop(app, id) {
        if let Some(before) = before {
            registry().undo_force_stop(id, before);
            emit(app);
        }
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
    emit(app);
    Ok(())
}

/// A force stop failed while the machine is still alive: keep it tracked as running.
fn force_stop_failed(app: &AppHandle, id: &str, message: String) {
    registry().force_stop_failed(id, message);
    emit(app);
}

/// The machine ended on its own or at Silo's request.
fn machine_stopped(app: &AppHandle, id: &str, error: Option<String>) {
    let (app, id) = (app.clone(), id.to_string());
    // Deferred so the framework callback that reported the stop has returned before
    // its machine is released.
    engine::defer(move || {
        engine::release_slot(&id);
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

// MARK: Display

fn open_display(app: &AppHandle, id: &str) -> Result<(), String> {
    require_supported()?;
    let (record, state) = computer(id)?;
    if state != State::Running {
        return Err("Start the computer to open its display.".into());
    }
    show_display(app, id, &record.name)
}

/// Opens the window that shows a running computer's screen, or brings the
/// existing one forward.
fn show_display(app: &AppHandle, id: &str, title: &str) -> Result<(), String> {
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
    if let Err(message) = engine::attach_display(app, id, &display) {
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
            if let Ok(states) = engine::machine_states(&app) {
                reconcile(&app, &states);
            }
        });
    });
}

fn reconcile(app: &AppHandle, states: &[(String, engine::MachineState)]) {
    for (id, machine) in states {
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
                machine_stopped(app, id, detail);
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
    let busy: Vec<(String, State, Arc<AtomicU8>)> = registry()
        .entries
        .iter()
        .filter(|entry| is_busy(entry.state))
        .map(|entry| (entry.record.id.clone(), entry.state, entry.cancel.clone()))
        .collect();
    if busy.is_empty() {
        return Ok(());
    }
    let machines: Vec<&String> = busy
        .iter()
        .filter(|(_, state, _)| matches!(state, State::Running | State::Starting | State::Stopping))
        .map(|(id, _, _)| id)
        .collect();
    for (_, state, cancel) in &busy {
        if matches!(
            state,
            State::Preparing | State::Downloading | State::Installing | State::SettingUp
        ) {
            request_abort(cancel);
        }
    }
    // A start in flight cannot be asked to stop; let it settle first.
    let settle = Instant::now() + Duration::from_secs(30);
    while machines
        .iter()
        .any(|id| state_of(id) == Some(State::Starting))
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
    let all_stopped = |ids: &[&String]| {
        ids.iter()
            .all(|id| state_of(id).is_none_or(|state| !is_busy(state)))
    };
    let wait_until = |until: Instant, ids: &[&String]| {
        while !all_stopped(ids) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(250));
        }
        all_stopped(ids)
    };
    let everything: Vec<&String> = busy.iter().map(|(id, _, _)| id).collect();
    // Installations and setups were aborted; they release their machine (and, for a
    // setup, the disk image) before they stop being busy.
    if wait_until(limit(GRACEFUL_QUIT), &machines) && wait_until(limit(FORCED_QUIT), &everything) {
        return Ok(());
    }
    // A machine that ignored the request, or a setup stuck in a guest command, still holds its machine.
    for id in &everything {
        if state_of(id).is_some_and(|state| state != State::Stopped && state != State::Failed)
            && engine::machine_states(app)
                .is_ok_and(|states| states.iter().any(|(held, _)| held == *id))
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
        assert!(json.get("detail").is_some());
        let mut done = entry;
        done.state = State::SettingUp;
        done.detail = Some("Creating the account".into());
        done.record.setup = store::SetupProgress {
            account: true,
            sip: true,
            computer_use: true,
            clipboard: true,
        };
        let json = serde_json::to_value(done.row()).unwrap();
        assert_eq!(json["state"], "setting-up");
        assert_eq!(json["detail"], "Creating the account");
        assert_eq!(json["setupComplete"], true);
        let state = serde_json::to_value(MacosComputersState {
            supported: false,
            unsupported_reason: Some("no".into()),
            computers: vec![],
        })
        .unwrap();
        assert_eq!(state["unsupportedReason"], "no");
        assert!(state["computers"].as_array().unwrap().is_empty());
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
        let registry = Registry {
            loaded: true,
            entries: vec![Entry::new(record, state, None)],
        };
        (registry, id)
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
        assert_eq!(registry.begin_force_stop(&id), Some(State::Running));
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
