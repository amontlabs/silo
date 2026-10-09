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
            setup_complete: self.record.setup.complete(),
        }
    }
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

    /// Marks a force stop as under way. Returns the state to restore if it cannot be issued.
    fn begin_force_stop(&mut self, id: &str) -> Option<State> {
        let entry = self.entry(id)?;
        let before = entry.state;
        if entry.state == State::Running {
            entry.state = State::Stopping;
            entry.since = Instant::now();
        }
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

fn create(app: &AppHandle, request: CreateRequest) -> Result<MacosComputer, String> {
    require_supported()?;
    ensure_loaded(app)?;
    let data = app_data(app)?;
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
    end_workflow(app, &id, &layout, cancel, result);
}

/// Provisions an installed computer again, resuming at the first unfinished step.
fn setup_workflow(app: &AppHandle, data: &std::path::Path, mut record: Record, cancel: &AtomicU8) {
    let id = record.id.clone();
    let layout = Layout::new(data, &id);
    let result = run_setup(app, &layout, &mut record, cancel);
    end_workflow(app, &id, &layout, cancel, result);
}

fn end_workflow(
    app: &AppHandle,
    id: &str,
    layout: &Layout,
    cancel: &AtomicU8,
    result: Result<(), Stop>,
) {
    match result {
        Ok(()) => {}
        Err(Stop::Failed(message)) => set_state(app, id, State::Failed, Some(message)),
        Err(Stop::Cancelled) => {
            if cancel.load(Ordering::SeqCst) == CANCEL_AND_REMOVE {
                let removal = store::remove(layout);
                registry().finish_cancelled(id, removal);
                emit(app);
            } else if computer(id).is_ok_and(|(record, _)| record.installed) {
                // Quit ended the work; what finished is kept and the rest can be retried.
                set_state(app, id, State::Stopped, None);
            } else {
                set_state(
                    app,
                    id,
                    State::Failed,
                    Some(store::INTERRUPTED_INSTALL.into()),
                );
            }
        }
    }
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
    let id = record.id.clone();
    provision::run(app, layout, record, cancel)?;
    if cancel.load(Ordering::SeqCst) == CANCEL_AND_REMOVE {
        return Err(Stop::Cancelled);
    }
    set_state(app, &id, State::Stopped, None);
    Ok(())
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
    match engine::start(app, &record, &Layout::new(&data, id)) {
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

fn stop(app: &AppHandle, id: &str) -> Result<(), String> {
    let (_, state) = computer(id)?;
    if !matches!(state, State::Running | State::Stopping) {
        return Err("This computer isn't running.".into());
    }
    engine::request_stop(app, id)?;
    update(app, id, |entry| {
        if entry.state == State::Running {
            entry.state = State::Stopping;
            entry.since = Instant::now();
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
        match store::remove(&Layout::new(&data, id)) {
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
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    require_supported()?;
    let (record, state) = computer(id)?;
    if state != State::Running {
        return Err("Start the computer to open its display.".into());
    }
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
        .title(&record.name)
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

fn is_busy(state: State) -> bool {
    !matches!(state, State::Stopped | State::Failed)
}

/// Stops every macOS computer for Quit: a graceful request first, then a forced
/// stop. Runs on a worker thread; the main thread must stay free to run the
/// framework's callbacks.
pub(crate) fn stop_all(app: &AppHandle, deadline: Option<Instant>) -> Result<(), String> {
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
            cancel.store(ABORT_AND_KEEP, Ordering::SeqCst);
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
    for id in &machines {
        let _ = engine::request_stop(app, id);
    }
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
    if wait_until(limit(GRACEFUL_QUIT), &machines) {
        // Installations and setups were aborted; give them a moment to release their machine.
        if wait_until(limit(FORCED_QUIT), &everything) {
            return Ok(());
        }
        // A setup stuck in a guest command still holds its machine.
        for id in &everything {
            if state_of(id) == Some(State::SettingUp) {
                let _ = engine::force_stop(app, id);
            }
        }
        wait_until(limit(FORCED_QUIT), &everything);
        return Ok(());
    }
    for id in &machines {
        if state_of(id).is_some_and(is_busy) {
            let _ = engine::force_stop(app, id);
        }
    }
    if wait_until(limit(FORCED_QUIT), &machines) {
        Ok(())
    } else {
        Err("A macOS computer did not stop in time.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
