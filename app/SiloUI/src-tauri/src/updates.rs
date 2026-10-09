//! Host-owned signed updates. The webview never chooses an endpoint, key or installer.
mod debian;
mod schedule;
use schedule::Schedule;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex, RwLock, RwLockReadGuard,
    },
    time::{Duration, SystemTime},
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

// Existing module locks still order their own operations. This one admission gate
// prevents an update from overtaking a queued write, and rejects new writes while
// installation owns the process. Readers never wait behind the installer.
static ADMISSION: RwLock<()> = RwLock::new(());
/// Admitted operations, so readiness can be probed without taking any lock.
static ADMITTED: AtomicUsize = AtomicUsize::new(0);
pub(crate) struct AdmissionGuard(#[allow(dead_code)] RwLockReadGuard<'static, ()>);
impl Drop for AdmissionGuard {
    fn drop(&mut self) {
        ADMITTED.fetch_sub(1, Ordering::SeqCst);
    }
}
pub(crate) fn operation_guard() -> Result<AdmissionGuard, String> {
    crate::runtime::shutdown::ensure_accepting_operations()?;
    let guard = ADMISSION
        .try_read()
        .map_err(|_| "Silo is installing an update. Try again after it restarts.")?;
    ADMITTED.fetch_add(1, Ordering::SeqCst);
    Ok(AdmissionGuard(guard))
}
const RELEASE_URL: &str = "https://github.com/amontlabs/silo/releases/latest";
const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    phase: String,
    last_checked: Option<String>,
    retry_action: Option<String>,
    current_version: String,
    available_version: Option<String>,
    release_notes: Option<String>,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    automatic_checks: bool,
    package_kind: String,
    release_url: String,
    error: Option<String>,
    error_details: Option<String>,
    running_computers: Vec<String>,
    can_install: bool,
    install_block_reason: Option<String>,
    install_status: Option<String>,
}
struct State {
    schedule: Schedule,
    snapshot: Snapshot,
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
}
struct Controller {
    state: Mutex<State>,
    preferences: PathBuf,
}
#[derive(Serialize, Deserialize)]
struct Preferences {
    automatic_checks: bool,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}
const MAX_PREFERENCE_BYTES: u64 = 1024 * 1024;
const PREFERENCE_READ_ERROR: &str =
    "Update preferences could not be read. Save your preference again.";
fn read_preferences(path: &Path) -> Result<bool, String> {
    Ok(read_saved_preferences(path)?
        .map(|preferences| preferences.automatic_checks)
        .unwrap_or(true))
}
fn read_saved_preferences(path: &Path) -> Result<Option<Preferences>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(PREFERENCE_READ_ERROR.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_PREFERENCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PREFERENCE_READ_ERROR)?;
    if bytes.len() as u64 > MAX_PREFERENCE_BYTES {
        return Err(PREFERENCE_READ_ERROR.into());
    }
    serde_json::from_slice::<Preferences>(&bytes)
        .map(Some)
        .map_err(|_| PREFERENCE_READ_ERROR.into())
}

fn save_preferences(path: &Path, enabled: bool) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Update preference storage is unavailable.")?;
    fs::create_dir_all(parent).map_err(|_| "Update preferences could not be saved.")?;
    let extra = read_saved_preferences(path)
        .ok()
        .flatten()
        .map(|preferences| preferences.extra)
        .unwrap_or_default();
    let bytes = serde_json::to_vec(&Preferences {
        automatic_checks: enabled,
        extra,
    })
    .map_err(|_| "Update preferences could not be saved.")?;
    if bytes.len() as u64 > MAX_PREFERENCE_BYTES {
        return Err(
            "Update preferences are too large to save. Your saved preference was not changed."
                .into(),
        );
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "Update preferences could not be saved.")?;
    std::io::Write::write_all(&mut file, &bytes)
        .map_err(|_| "Update preferences could not be saved.")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "Update preferences could not be saved.")?;
    file.persist(path)
        .map_err(|_| "Update preferences could not be saved.")?;
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|_| "Update preferences could not be saved.".into())
}
fn package_kind(
    executable: &Path,
    appimage: Option<&Path>,
    bundle: Option<tauri::utils::config::BundleType>,
) -> &'static str {
    if cfg!(target_os = "macos")
        && executable
            .ancestors()
            .any(|p| p.extension().is_some_and(|e| e == "app"))
    {
        "macos"
    } else if cfg!(target_os = "linux")
        && matches!(bundle, Some(tauri::utils::config::BundleType::AppImage))
        && appimage.is_some_and(|p| p.is_absolute() && p.is_file())
    {
        "appimage"
    } else if cfg!(target_os = "linux")
        && matches!(bundle, Some(tauri::utils::config::BundleType::Deb))
        && executable == Path::new("/usr/bin/silo-ui")
    {
        "debian"
    } else {
        "manual"
    }
}
fn modify(app: &AppHandle, f: impl FnOnce(&mut State)) -> Result<Snapshot, String> {
    let controller = app.state::<Controller>();
    let mut state = controller
        .state
        .lock()
        .map_err(|_| "Update state is unavailable.")?;
    f(&mut state);
    let snapshot = state.snapshot.clone();
    let _ = app.emit("silo://update-state", &snapshot);
    Ok(snapshot)
}
fn fail(app: &AppHandle, message: &str, details: impl ToString) -> Result<Snapshot, String> {
    modify(app, |s| {
        s.snapshot.phase = "error".into();
        s.snapshot.error = Some(message.into());
        s.snapshot.error_details = Some(details.to_string());
        s.snapshot.retry_action = Some(
            if s.bytes.is_some() || (s.snapshot.package_kind == "debian" && s.update.is_some()) {
                "install"
            } else if s.update.is_some() {
                "download"
            } else {
                "check"
            }
            .into(),
        );
    })
}
pub(crate) fn recovery_failed(app: &AppHandle, message: String) {
    let _ = fail(
        app,
        "Some computers could not resume after updating. Relaunch Silo to retry.",
        &message,
    );
    crate::notifications::notify(
        app,
        crate::notifications::failure(
            "update:resume",
            "Computers couldn\u{2019}t resume after updating",
            &message,
            None,
        ),
    );
}
fn busy(phase: &str) -> bool {
    matches!(phase, "checking" | "downloading" | "installing")
}
fn ready(app: &AppHandle) -> Result<(), String> {
    readiness(|| {
        crate::backup_controller::update_ready(app)?;
        // Fast readiness check only: refuse if any computer operation is active or queued.
        if !crate::runtime::OPERATIONS.is_idle() {
            return Err("Wait for computer operations to finish before updating.".into());
        }
        crate::runtime::shutdown::ensure_accepting_operations()
    })
}
/// Read-only probe for the settings card, polled every few seconds. Taking the
/// admission write lock or the GitHub/secret operation locks here, even briefly,
/// made concurrent operations fail as busy. Installation still takes every guard
/// and reports the exact blocker.
fn readiness(checks: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    if ADMITTED.load(Ordering::SeqCst) > 0 {
        return Err("Wait for active operations to finish before updating.".into());
    }
    checks()
}
pub(crate) fn install(app: &AppHandle) -> Result<(), String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| "Update storage is unavailable.")?
        .join("update-preferences.json");
    let preference = read_preferences(&path);
    let error = preference.as_ref().err().cloned();
    // Silo Dev is built from source and never replaces itself with a production release.
    let automatic = preference.unwrap_or(false) && !crate::channel::current().is_development();
    let executable = std::env::current_exe().unwrap_or_default();
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    app.manage(Controller {
        preferences: path,
        state: Mutex::new(State {
            schedule: Schedule::new(SystemTime::now()),
            update: None,
            bytes: None,
            snapshot: Snapshot {
                last_checked: None,
                retry_action: None,
                phase: if error.is_some() { "error" } else { "idle" }.into(),
                current_version: app.package_info().version.to_string(),
                available_version: None,
                release_notes: None,
                downloaded_bytes: 0,
                total_bytes: None,
                automatic_checks: automatic,
                package_kind: package_kind(
                    &executable,
                    appimage.as_deref(),
                    tauri::utils::platform::bundle_type(),
                )
                .into(),
                release_url: RELEASE_URL.into(),
                error,
                error_details: None,
                running_computers: vec![],
                can_install: false,
                install_block_reason: None,
                install_status: None,
            },
        }),
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(schedule::POLL_INTERVAL).await;
            let _ = check(app.clone(), true).await;
        }
    });
    Ok(())
}
pub(crate) fn focused(app: &AppHandle) {
    // The first window event can precede updater setup.
    let Some(controller) = app.try_state::<Controller>() else {
        return;
    };
    if let Ok(mut state) = controller.state.lock() {
        state.schedule.focus(SystemTime::now());
    };
}
#[tauri::command]
pub(crate) async fn get_update_state(app: AppHandle) -> Result<Snapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let installing = app
            .state::<Controller>()
            .state
            .lock()
            .map_or(true, |state| state.snapshot.phase == "installing");
        let readiness = if installing {
            Err("An update operation is already running.".to_string())
        } else {
            ready(&app)
        };
        // Do not compete with an active runtime mutation just to refresh a settings card.
        let running = readiness.and_then(|_| {
            crate::runtime::update_recovery::running_names(&app).map_err(|_| {
                "Silo could not verify computer status. Check Computers before updating."
                    .to_string()
            })
        });
        modify(&app, |s| {
            s.snapshot.can_install = running.is_ok();
            s.snapshot.install_block_reason = running.as_ref().err().cloned();
            if let Ok(names) = running {
                s.snapshot.running_computers = names;
            }
        })
    })
    .await
    .map_err(|_| "Update state could not be read.".to_string())?
}
#[tauri::command]
pub(crate) async fn set_update_automatic_checks(
    app: AppHandle,
    enabled: bool,
) -> Result<Snapshot, String> {
    // The durable write fsyncs twice; keep it off the main thread.
    tauri::async_runtime::spawn_blocking(move || {
        update_automatic_checks(
            &app.state::<Controller>(),
            enabled,
            save_preferences,
            |snapshot| {
                let _ = app.emit("silo://update-state", snapshot);
            },
        )
    })
    .await
    .map_err(|_| "Update preferences could not be saved.".to_owned())?
}
fn update_automatic_checks(
    controller: &Controller,
    enabled: bool,
    persist: impl FnOnce(&Path, bool) -> Result<(), String>,
    publish: impl FnOnce(&Snapshot),
) -> Result<Snapshot, String> {
    let mut state = controller
        .state
        .lock()
        .map_err(|_| "Update state is unavailable.")?;
    persist(&controller.preferences, enabled)?;
    if enabled && !state.snapshot.automatic_checks {
        state.schedule.enable(SystemTime::now());
    }
    state.snapshot.automatic_checks = enabled;
    if state.snapshot.phase == "error"
        && state.snapshot.error.as_deref() == Some(PREFERENCE_READ_ERROR)
    {
        state.snapshot.phase = "idle".into();
        state.snapshot.error = None;
        state.snapshot.error_details = None;
        state.snapshot.retry_action = None;
    }
    let snapshot = state.snapshot.clone();
    publish(&snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn check_for_update(app: AppHandle) -> Result<Snapshot, String> {
    check(app, false).await
}
/// Background checks never replace verified bytes, a pending download or install
/// retry, or an installed update waiting for a relaunch. An update that was only
/// found (not downloaded) can be superseded.
fn blocks_automatic_check(
    phase: &str,
    retry_action: Option<&str>,
    has_update: bool,
    has_bytes: bool,
) -> bool {
    has_bytes || (has_update && phase != "available") || retry_action == Some("relaunch")
}
/// Apply a successful check. A verified download of the same version survives the
/// check; returns true when the pending update (and its bytes) should be kept.
fn settle_check(
    snapshot: &mut Snapshot,
    bytes: &mut Option<Vec<u8>>,
    pending: Option<&str>,
    found: Option<(&str, Option<&str>)>,
) -> bool {
    let keep = bytes.is_some() && found.is_some_and(|(version, _)| Some(version) == pending);
    snapshot.retry_action = None;
    if keep {
        snapshot.phase = "ready".into();
    } else {
        *bytes = None;
        snapshot.phase = if found.is_some() { "available" } else { "idle" }.into();
        snapshot.downloaded_bytes = 0;
        snapshot.total_bytes = None;
    }
    snapshot.available_version = found.map(|(version, _)| version.to_owned());
    snapshot.release_notes = found.and_then(|(_, notes)| notes.map(str::to_owned));
    keep
}
async fn check(app: AppHandle, automatic: bool) -> Result<Snapshot, String> {
    if crate::channel::current().is_development() {
        // No network request: the development channel has no update feed.
        return modify(&app, |s| {
            s.snapshot.phase = "idle".into();
            s.snapshot.error = None;
            s.snapshot.error_details = None;
        });
    }
    let previous_phase = {
        let controller = app.state::<Controller>();
        let mut state = controller
            .state
            .lock()
            .map_err(|_| "Update state is unavailable.")?;
        // Admit automatic checks under the same lock as manual actions.
        if automatic
            && !state.schedule.due(
                SystemTime::now(),
                state.snapshot.automatic_checks,
                busy(&state.snapshot.phase),
                blocks_automatic_check(
                    &state.snapshot.phase,
                    state.snapshot.retry_action.as_deref(),
                    state.update.is_some(),
                    state.bytes.is_some(),
                ),
            )
        {
            return Ok(state.snapshot.clone());
        }
        if busy(&state.snapshot.phase) {
            return Err("An update operation is already running.".into());
        }
        // Keep a discovered update and verified bytes until the result is known.
        let previous = std::mem::replace(&mut state.snapshot.phase, "checking".into());
        state.snapshot.error = None;
        state.snapshot.error_details = None;
        let _ = app.emit("silo://update-state", &state.snapshot);
        previous
    };
    let result = async {
        app.updater_builder()
            .timeout(Duration::from_secs(30))
            .build()?
            .check()
            .await
    }
    .await;
    match result {
        Ok(update) => modify(&app, |s| {
            s.schedule.completed(SystemTime::now(), true);
            s.snapshot.last_checked = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .ok();
            let pending = s.update.as_ref().map(|u| u.version.clone());
            let found = update
                .as_ref()
                .map(|u| (u.version.as_str(), u.body.as_deref()));
            // The kept bytes were verified against the pending update's signature.
            if !settle_check(&mut s.snapshot, &mut s.bytes, pending.as_deref(), found) {
                s.update = update;
            }
        }),
        Err(e) => {
            modify(&app, |s| s.schedule.completed(SystemTime::now(), false))?;
            let pending = app
                .state::<Controller>()
                .state
                .lock()
                .map_or(false, |s| s.update.is_some());
            if !pending {
                return fail(&app, check_error_message(&e), e);
            }
            // A failed re-check leaves a found or downloaded update actionable.
            modify(&app, |s| {
                s.snapshot.phase = if matches!(previous_phase.as_str(), "available" | "ready") {
                    previous_phase
                } else {
                    "error".into()
                };
                s.snapshot.error = Some(check_error_message(&e).into());
                s.snapshot.error_details = Some(e.to_string());
                s.snapshot.retry_action = Some("check".into());
            })
        }
    }
}

fn check_error_message(error: &tauri_plugin_updater::Error) -> &'static str {
    use tauri_plugin_updater::Error;
    match error {
        Error::ReleaseNotFound => "The update service is unavailable. Try again later.",
        Error::Serialization(_) | Error::Semver(_) => {
            "The update service returned invalid release information. Try again later."
        }
        Error::Reqwest(error) if error.is_decode() => {
            "The update service returned invalid release information. Try again later."
        }
        Error::TargetNotFound(_) | Error::TargetsNotFound(_) => {
            "This release has no update for your platform. Try again later."
        }
        _ => "Could not check for updates. Try again later.",
    }
}
#[tauri::command]
pub(crate) async fn download_update(app: AppHandle) -> Result<Snapshot, String> {
    let mut update = {
        let controller = app.state::<Controller>();
        let mut state = controller
            .state
            .lock()
            .map_err(|_| "Update state is unavailable.")?;
        if busy(&state.snapshot.phase) {
            return Err("An update operation is already running.".into());
        }
        if matches!(state.snapshot.package_kind.as_str(), "manual" | "debian") {
            return Err("Download the package from Releases to update this installation.".into());
        }
        let update = state
            .update
            .clone()
            .ok_or("Check for an update before downloading.")?;
        state.snapshot.phase = "downloading".into();
        state.snapshot.error = None;
        state.snapshot.error_details = None;
        state.snapshot.downloaded_bytes = 0;
        state.snapshot.total_bytes = None;
        state.bytes = None;
        let _ = app.emit("silo://update-state", &state.snapshot);
        update
    };
    update.timeout = Some(Duration::from_secs(30 * 60));
    let limit = tokio::sync::Notify::new();
    let mut received = 0u64;
    let mut last_report = std::time::Instant::now() - Duration::from_secs(1);
    let result = tokio::select! {
        _ = limit.notified() => Err("The update exceeds the supported download size.".to_string()),
        result = update.download(|bytes, total| {
            received = received.saturating_add(bytes as u64);
            if received > MAX_DOWNLOAD_BYTES || total.is_some_and(|size| size > MAX_DOWNLOAD_BYTES) { limit.notify_one(); }
            if last_report.elapsed() >= Duration::from_millis(100) {
                let _ = modify(&app, |s| { s.snapshot.downloaded_bytes = received; s.snapshot.total_bytes = total; }); last_report = std::time::Instant::now();
            }
        }, || {}) => result.map_err(|e| e.to_string()),
    };
    // The completed download and notification can both be ready in the same
    // select poll. Check the final verified buffer independently of that race.
    let result = result.and_then(|bytes| {
        validate_download_size(bytes.len() as u64)?;
        Ok(bytes)
    });
    match result {
        Ok(bytes) => { modify(&app, |s| { s.snapshot.phase = "ready".into(); s.snapshot.downloaded_bytes = bytes.len() as u64; s.snapshot.total_bytes = Some(bytes.len() as u64); s.bytes = Some(bytes); })?; get_update_state(app).await },
        Err(e) => fail(&app, "The update could not be downloaded or verified. Your installation was not changed. Try again.", e),
    }
}
fn validate_download_size(size: u64) -> Result<(), String> {
    if size > MAX_DOWNLOAD_BYTES {
        Err("The update exceeds the supported download size.".into())
    } else {
        Ok(())
    }
}
fn unpacked_size(bytes: &[u8], macos: bool) -> Result<u64, String> {
    if !macos {
        return Ok(bytes.len() as u64);
    }
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    let mut total = 0u64;
    for entry in archive
        .entries()
        .map_err(|_| "The update archive could not be read.")?
    {
        let entry = entry.map_err(|_| "The update archive is invalid.")?;
        let size = entry
            .header()
            .size()
            .map_err(|_| "The update archive size is invalid.")?;
        total = total
            .checked_add(size)
            .filter(|sum| *sum <= 8 * 1024 * 1024 * 1024)
            .ok_or("The unpacked update exceeds the supported size.")?;
    }
    if total == 0 {
        return Err("The update archive is empty.".into());
    }
    Ok(total)
}
fn installation_preflight(bytes: &[u8]) -> Result<(), String> {
    let executable =
        std::env::current_exe().map_err(|_| "The installed application could not be located.")?;
    let destination = if cfg!(target_os = "macos") {
        executable
            .ancestors()
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .map(Path::to_path_buf)
    } else {
        std::env::var_os("APPIMAGE").map(PathBuf::from)
    }
    .ok_or("This installation must be updated using a downloaded package.")?;
    preflight_destination(
        &destination,
        bytes,
        cfg!(target_os = "macos"),
        available_install_space,
    )
}
fn preflight_destination(
    destination: &Path,
    bytes: &[u8],
    macos: bool,
    free_space: impl FnOnce(&Path) -> Result<u64, String>,
) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or("The installation folder could not be located.")?;
    // Test the actual staging directory, rather than assuming Unix mode bits mean writable.
    let probe = tempfile::NamedTempFile::new_in(parent).map_err(|_| "Silo cannot write to its installation folder. Move it to a writable folder or install the new package manually.")?;
    drop(probe);
    let needed = unpacked_size(bytes, macos)?;
    let free = free_space(parent)?;
    // Existing installation already occupies space. Atomic replacement stages only
    // one new copy beside it; the downloaded archive is held in memory.
    if free < needed {
        return Err(format!(
            "Not enough space to install the update. Free at least {} MiB and retry.",
            needed.saturating_sub(free).div_ceil(1024 * 1024)
        ));
    }
    Ok(())
}
fn available_install_space(parent: &Path) -> Result<u64, String> {
    use std::os::unix::ffi::OsStrExt;
    let encoded = std::ffi::CString::new(parent.as_os_str().as_bytes())
        .map_err(|_| "The installation folder is invalid.")?;
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: CString is NUL terminated and stats is a valid exclusive output pointer.
    if unsafe { libc::statvfs(encoded.as_ptr(), &mut stats) } != 0 {
        return Err("Available installation space could not be checked.".into());
    }
    (stats.f_bavail as u64)
        .checked_mul(stats.f_frsize as u64)
        .ok_or_else(|| "Available installation space is invalid.".into())
}

/// How an installation that did not restart Silo ended.
enum InstallError {
    /// Nothing was installed (computers were restored where possible); retry the install.
    Failed(String),
}
impl From<String> for InstallError {
    fn from(error: String) -> Self {
        Self::Failed(error)
    }
}
/// `exec` skips exit cleanup, so close helpers and release the instance claim first.
/// A failed replacement must exit without reopening admission or restoring computers:
/// this process no longer owns the claim. Startup restores the saved running set.
/// Closes file-transfer admission and lets the running transfer clean up (`close`)
/// before `prepare` stops computers, since cleanup needs its computer. The returned
/// guard keeps admission closed; the caller drops it once the update has failed and
/// its computers are running again, or leaves it to the replaced process.
fn prepare_after_transfers<P>(
    close: impl FnOnce() -> P,
    prepare: impl FnOnce() -> Result<(), String>,
) -> (P, Result<(), String>) {
    let closed = close();
    let result = prepare();
    (closed, result)
}

fn close_transfers() -> crate::transfer::Pause {
    crate::transfer::pause(crate::runtime::shutdown::TRANSFER_DRAIN)
}

fn restart_after_install(close: impl FnOnce(), restart: impl FnOnce() -> String) -> ! {
    close();
    let error = restart();
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr().lock(),
        "{error}\nSilo has closed. Reopen Silo to finish the update and restore its computers."
    );
    std::process::exit(1)
}
/// Debian installs through APT with system authentication. Authentication, the
/// source check, the refresh and the download all happen while computers keep
/// running; they are stopped only once the package is ready to install, and
/// restored if installation then fails.
fn install_debian(app: &AppHandle, version: &str, consent: bool) -> Result<(), InstallError> {
    debian::preflight()?;
    // Refuse new work while the update prepares; the helper's timeout bounds this.
    let admission = ADMISSION
        .try_write()
        .map_err(|_| "Wait for active operations to finish before updating.".to_string())?;
    let backup = crate::backup_controller::update_guard(app)?;
    let github = crate::github::update_guard()?;
    let secrets = crate::secrets::update_guard()?;
    crate::runtime::shutdown::ensure_accepting_operations()?;
    let mut runtime = None;
    let mut transfers = None;
    let result = debian::install(
        version,
        |status| {
            let _ = modify(app, |s| s.snapshot.install_status = Some(status.into()));
        },
        || {
            // Only now wait for device-wide work, so Quit is never queued behind
            // an authentication prompt or a download.
            let guard = crate::runtime::OPERATIONS
                .kind(crate::runtime::operation_gate::OperationKind::Shutdown)
                .device("Installing update")
                .map_err(|e| e.to_string())?;
            runtime = Some(guard);
            crate::runtime::shutdown::ensure_accepting_operations()?;
            crate::settings::flush_for_update(app)?;
            let (closed, prepared) = prepare_after_transfers(close_transfers, || {
                crate::runtime::update_recovery::prepare(app, consent)
            });
            transfers = Some(closed);
            prepared
        },
    );
    if let Err(error) = result {
        // Computers can only have stopped once the install stage took the gate.
        if runtime.is_some() {
            if let Err(resume) = crate::runtime::update_recovery::restore_locked(app) {
                return Err(InstallError::Failed(format!(
                    "{error}\nComputers could not resume: {resume}. Relaunch Silo to retry."
                )));
            }
        }
        drop(transfers);
        return Err(InstallError::Failed(error));
    }
    // Keep installation guards and shutdown admission closed until this process
    // is replaced or exits. Startup owns recovery from the retained update journal.
    crate::runtime::shutdown::begin();
    let _guards = (admission, backup, github, secrets, runtime, transfers);
    restart_after_install(
        || {
            crate::ssh_access::close_all();
            crate::remote_network::close_all();
            crate::desktop_viewer::close_all();
            // exec also skips the single-instance plugin's cleanup: release the
            // claim so the replacement process does not find it and exit (F-09).
            crate::single_instance::release(app);
        },
        debian::restart,
    )
}
#[tauri::command]
pub(crate) async fn install_update(
    app: AppHandle,
    stop_computers: bool,
) -> Result<Snapshot, String> {
    let (update, bytes, is_debian) = {
        let controller = app.state::<Controller>();
        let mut state = controller
            .state
            .lock()
            .map_err(|_| "Update state is unavailable.")?;
        if busy(&state.snapshot.phase) {
            return Err("An update operation is already running.".into());
        }
        if state.snapshot.package_kind == "manual" {
            return Err("Install the downloaded package to update this installation.".into());
        }
        let update = state
            .update
            .clone()
            .ok_or("Download and verify an update before installing.")?;
        let is_debian = state.snapshot.package_kind == "debian";
        let bytes = if is_debian {
            vec![]
        } else {
            state
                .bytes
                .take()
                .ok_or("Download and verify an update before installing.")?
        };
        state.snapshot.phase = "installing".into();
        state.snapshot.install_status = None;
        state.snapshot.error = None;
        state.snapshot.error_details = None;
        let _ = app.emit("silo://update-state", &state.snapshot);
        (update, bytes, is_debian)
    };
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<(), InstallError> {
        crate::startup::cancel_and_wait(&worker);
        if is_debian {
            return install_debian(&worker, &update.version, stop_computers);
        }
        // Reservation uses the same atomic gate as backup/restore admission.
        let admission = (|| {
            // Linux computers are stopped and restored from the update journal after the
            // restart. macOS computers live in this process and cannot be restored, so
            // the update refuses while one is busy and admits no new one until it ends.
            crate::macos_computers::close_for_update()?;
            let admission = ADMISSION.try_write().map_err(|_| "Wait for active operations to finish before updating.")?;
            let backup = crate::backup_controller::update_guard(&worker)?;
            let github = crate::github::update_guard()?;
            let secrets = crate::secrets::update_guard()?;
            // The installer waits its turn for device-wide work before stopping computers.
            let runtime = crate::runtime::OPERATIONS
                .kind(crate::runtime::operation_gate::OperationKind::Shutdown)
                .device("Installing update").map_err(|e| e.to_string())?;
            crate::runtime::shutdown::ensure_accepting_operations()?;
            Ok::<_, String>((admission, backup, github, secrets, runtime))
        })();
        let (_admission, _backup, _github, _secrets, _runtime) = match admission {
            Ok(guards) => guards,
            Err(error) => { crate::macos_computers::reopen(); let _ = modify(&worker, |s| s.bytes = Some(bytes)); return Err(error.into()); }
        };
        let mut transfers = None;
        let result = installation_preflight(&bytes)
            .and_then(|_| crate::settings::flush_for_update(&worker))
            .and_then(|_| {
                let (closed, prepared) = prepare_after_transfers(close_transfers, || {
                    crate::runtime::update_recovery::prepare(&worker, stop_computers)
                });
                transfers = Some(closed);
                prepared
            })
            .and_then(|_| update.install(&bytes).map_err(|e| e.to_string()));
        if let Err(error) = result {
            crate::macos_computers::reopen();
            let restore = crate::runtime::update_recovery::restore_locked(&worker);
            drop(transfers);
            let _ = modify(&worker, |s| s.bytes = Some(bytes));
            return Err(match restore { Ok(()) => error, Err(resume) => format!("{error}\nComputers could not resume: {resume}. Relaunch Silo to retry.") }.into());
        }
        // Settings are flushed before installation and the UI stays inert.
        // Tauri restart cannot be deferred by the ordinary exit flush handler.
        // Close admission before releasing installation guards. The update
        // journal retains the running set for startup to restore after restart.
        crate::runtime::shutdown::begin();
        drop((_admission, _backup, _github, _secrets, _runtime, transfers));
        worker.restart()
    }).await.unwrap_or_else(|_| Err(InstallError::Failed("Update installation was interrupted. Relaunch Silo to restore the saved computer state, then download the update again.".into())));
    match result {
        Ok(()) => get_update_state(app).await,
        Err(InstallError::Failed(e)) => fail(
            &app,
            if is_debian {
                debian::failure_message(&e)
            } else {
                "The update could not be installed. Try again, or download the latest installer."
            },
            e,
        ),
    }
}
#[tauri::command]
pub(crate) async fn open_update_release(app: tauri::AppHandle) -> Result<(), String> {
    // The shared browser opener honours the browser setting and leaves no
    // unreaped child process (F-25).
    tauri::async_runtime::spawn_blocking(move || {
        crate::applications::open_browser(&app, RELEASE_URL)
    })
    .await
    .map_err(|_| "The browser could not be opened.".to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfers_close_before_computers_stop_and_stay_closed_until_released() {
        let events = std::cell::RefCell::new(Vec::new());
        struct Closed<'a>(&'a std::cell::RefCell<Vec<&'static str>>);
        impl Drop for Closed<'_> {
            fn drop(&mut self) {
                self.0.borrow_mut().push("reopen");
            }
        }
        let (closed, result) = prepare_after_transfers(
            || {
                events.borrow_mut().push("close");
                Closed(&events)
            },
            || {
                events.borrow_mut().push("stop");
                Err("stopping failed".to_string())
            },
        );
        assert_eq!(result.unwrap_err(), "stopping failed");
        assert_eq!(*events.borrow(), ["close", "stop"]);
        // The caller restores computers, then drops the guard.
        events.borrow_mut().push("restore");
        drop(closed);
        assert_eq!(*events.borrow(), ["close", "stop", "restore", "reopen"]);
    }
    #[test]
    fn unavailable_feed_does_not_blame_the_connection_or_claim_success() {
        assert_eq!(
            check_error_message(&tauri_plugin_updater::Error::ReleaseNotFound),
            "The update service is unavailable. Try again later."
        );
        let invalid = serde_json::from_str::<serde_json::Value>("broken").unwrap_err();
        assert_eq!(
            check_error_message(&invalid.into()),
            "The update service returned invalid release information. Try again later."
        );
    }
    fn snapshot(phase: &str) -> Snapshot {
        Snapshot {
            phase: phase.into(),
            last_checked: None,
            retry_action: None,
            current_version: "1.0.0".into(),
            available_version: None,
            release_notes: None,
            downloaded_bytes: 0,
            total_bytes: None,
            automatic_checks: true,
            package_kind: "macos".into(),
            release_url: RELEASE_URL.into(),
            error: None,
            error_details: None,
            running_computers: vec![],
            can_install: false,
            install_block_reason: None,
            install_status: None,
        }
    }
    #[test]
    fn recheck_keeps_a_verified_download_and_replaces_a_superseded_release() {
        let mut state = snapshot("checking");
        state.downloaded_bytes = 3;
        state.total_bytes = Some(3);
        let mut bytes = Some(vec![1, 2, 3]);
        assert!(settle_check(
            &mut state,
            &mut bytes,
            Some("1.2.0"),
            Some(("1.2.0", None))
        ));
        assert_eq!(state.phase, "ready");
        assert_eq!(bytes.as_deref(), Some(&[1, 2, 3][..]));
        assert_eq!((state.downloaded_bytes, state.total_bytes), (3, Some(3)));
        assert_eq!(state.available_version.as_deref(), Some("1.2.0"));
        // A newer release supersedes the pending one; its download no longer applies.
        assert!(!settle_check(
            &mut state,
            &mut bytes,
            Some("1.2.0"),
            Some(("1.3.0", Some("notes")))
        ));
        assert_eq!(state.phase, "available");
        assert!(bytes.is_none());
        assert_eq!((state.downloaded_bytes, state.total_bytes), (0, None));
        assert_eq!(state.available_version.as_deref(), Some("1.3.0"));
        assert_eq!(state.release_notes.as_deref(), Some("notes"));
        // Same version without a download: refresh the release, nothing to keep.
        assert!(!settle_check(
            &mut state,
            &mut bytes,
            Some("1.3.0"),
            Some(("1.3.0", None))
        ));
        assert_eq!(state.phase, "available");
        assert!(!settle_check(&mut state, &mut bytes, Some("1.3.0"), None));
        assert_eq!(state.phase, "idle");
        assert!(state.available_version.is_none());
    }
    #[test]
    fn automatic_checks_run_while_an_update_is_only_available() {
        // A newer release can supersede one that was found but not downloaded.
        assert!(!blocks_automatic_check("available", None, true, false));
        assert!(!blocks_automatic_check("idle", None, false, false));
        // A failed check keeps retrying on its backoff.
        assert!(!blocks_automatic_check(
            "error",
            Some("check"),
            false,
            false
        ));
        // Verified bytes and download or install retries are never replaced in the background.
        assert!(blocks_automatic_check("ready", None, true, true));
        assert!(blocks_automatic_check(
            "error",
            Some("download"),
            true,
            false
        ));
        assert!(blocks_automatic_check("error", Some("install"), true, true));
        // An installed update waiting for a relaunch must not be offered again.
        assert!(blocks_automatic_check(
            "error",
            Some("relaunch"),
            false,
            false
        ));
    }
    #[test]
    fn reexec_replaces_or_exits_without_resuming_writes() {
        use std::os::unix::process::CommandExt;
        use std::process::Command;
        const CHILD: &str = "SILO_REEXEC_TEST_DIRECTORY";
        const REPLACE: &str = "SILO_REEXEC_TEST_REPLACE";
        if let Some(directory) = std::env::var_os(CHILD) {
            let directory = PathBuf::from(directory);
            let mutation = directory.join("mutation");
            let worker = std::thread::spawn(move || -> () {
                restart_after_install(
                    || fs::write(directory.join("released"), b"claim released").unwrap(),
                    || {
                        assert!(directory.join("released").exists());
                        let error = if std::env::var_os(REPLACE).is_some() {
                            Command::new("/bin/sh").args(["-c", "exit 0"]).exec()
                        } else {
                            Command::new(directory.join("missing-executable")).exec()
                        };
                        format!("exec failed: {error}")
                    },
                )
            });
            worker.join().unwrap();
            fs::write(mutation, b"ordinary write resumed").unwrap();
            return;
        }
        for replace in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "updates::tests::reexec_replaces_or_exits_without_resuming_writes",
                    "--nocapture",
                ])
                .env(CHILD, directory.path())
                .env("HOME", directory.path())
                .env_remove(REPLACE);
            if replace {
                command.env(REPLACE, "1");
            }
            let output = command.output().unwrap();
            assert!(directory.path().join("released").exists());
            assert!(
                !directory.path().join("mutation").exists(),
                "ordinary mutations must not resume after releasing the instance claim"
            );
            assert_eq!(output.status.code(), Some(if replace { 0 } else { 1 }));
            if !replace {
                let error = String::from_utf8_lossy(&output.stderr);
                assert!(
                    error.contains("exec failed") && error.contains("Reopen Silo"),
                    "{error}"
                );
            }
        }
    }

    #[test]
    fn readiness_probe_never_rejects_concurrent_operations() {
        // The settings card polls readiness every few seconds while an update is ready.
        // Operations admitted during a probe must not fail as if an update were installing.
        readiness(|| {
            let _admitted = operation_guard().expect("a readiness probe must not block admission");
            Ok(())
        })
        .unwrap();
        let active = operation_guard().unwrap();
        assert!(readiness(|| Ok(()))
            .unwrap_err()
            .contains("active operations"));
        drop(active);
        readiness(|| Ok(())).unwrap();
    }
    #[test]
    fn concurrent_preference_changes_keep_disk_and_snapshot_in_agreement() {
        let directory = tempfile::tempdir().unwrap();
        let controller = Controller {
            preferences: directory.path().join("prefs.json"),
            state: Mutex::new(State {
                schedule: Schedule::new(SystemTime::now()),
                snapshot: snapshot("idle"),
                update: None,
                bytes: None,
            }),
        };
        let (first_saved, saved) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let (second_saved, second) = std::sync::mpsc::channel();
        std::thread::scope(|threads| {
            let controller = &controller;
            let first = threads.spawn(move || {
                update_automatic_checks(
                    controller,
                    false,
                    |path, enabled| {
                        save_preferences(path, enabled)?;
                        first_saved.send(()).unwrap();
                        released.recv().unwrap();
                        Ok(())
                    },
                    |_| {},
                )
            });
            saved.recv().unwrap();
            let last = threads.spawn(move || {
                update_automatic_checks(
                    controller,
                    true,
                    |path, enabled| {
                        save_preferences(path, enabled)?;
                        second_saved.send(()).unwrap();
                        Ok(())
                    },
                    |_| {},
                )
            });
            // Give the second writer a chance to overtake the first publication.
            let overtook = second.recv_timeout(Duration::from_secs(1)).is_ok();
            release.send(()).unwrap();
            first.join().unwrap().unwrap();
            last.join().unwrap().unwrap();
            assert!(
                !overtook,
                "a second save overtook an unpublished preference"
            );
        });
        assert_eq!(
            read_preferences(&controller.preferences).unwrap(),
            controller.state.lock().unwrap().snapshot.automatic_checks
        );
    }
    #[test]
    fn failed_preference_save_preserves_snapshot_and_does_not_publish() {
        let directory = tempfile::tempdir().unwrap();
        let controller = Controller {
            preferences: directory.path().join("prefs.json"),
            state: Mutex::new(State {
                schedule: Schedule::new(SystemTime::now()),
                snapshot: snapshot("idle"),
                update: None,
                bytes: None,
            }),
        };
        let error = update_automatic_checks(
            &controller,
            false,
            |_, _| Err("save failed".into()),
            |_| panic!("failed persistence must not publish success"),
        )
        .unwrap_err();
        assert_eq!(error, "save failed");
        assert!(controller.state.lock().unwrap().snapshot.automatic_checks);
    }
    #[test]
    fn saving_preferences_clears_the_read_error_but_preserves_update_failures() {
        let directory = tempfile::tempdir().unwrap();
        let preferences = directory.path().join("prefs.json");
        fs::write(&preferences, "broken").unwrap();
        let read_error = read_preferences(&preferences).unwrap_err();
        let mut initial = snapshot("error");
        initial.error = Some(read_error);
        initial.automatic_checks = false;
        let controller = Controller {
            preferences,
            state: Mutex::new(State {
                schedule: Schedule::new(SystemTime::now()),
                snapshot: initial,
                update: None,
                bytes: None,
            }),
        };
        let repaired =
            update_automatic_checks(&controller, false, save_preferences, |_| {}).unwrap();
        assert!(repaired.error.is_none());
        assert_eq!(repaired.phase, "idle");
        assert!(!read_preferences(&controller.preferences).unwrap());
        {
            let mut state = controller.state.lock().unwrap();
            state.snapshot.phase = "error".into();
            state.snapshot.error = Some("Download failed".into());
            state.snapshot.retry_action = Some("download".into());
        }
        let unrelated =
            update_automatic_checks(&controller, false, save_preferences, |_| {}).unwrap();
        assert_eq!(unrelated.error.as_deref(), Some("Download failed"));
        assert_eq!(unrelated.phase, "error");
        assert_eq!(unrelated.retry_action.as_deref(), Some("download"));
    }
    #[test]
    fn preferences_large_input_memory_is_bounded() {
        const PROBE: &str = "SILO_TEST_UPDATE_PREFERENCES_MEMORY_PROBE";
        if std::env::var_os(PROBE).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "updates::tests::preferences_large_input_memory_is_bounded",
                    "--nocapture",
                ])
                .env(PROBE, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed;"));
            return;
        }

        fn peak_bytes() -> u64 {
            // SAFETY: getrusage initializes the supplied rusage structure.
            let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
            assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) }, 0);
            #[cfg(target_os = "macos")]
            return usage.ru_maxrss as u64;
            #[cfg(not(target_os = "macos"))]
            return usage.ru_maxrss as u64 * 1024;
        }

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("computers.json");
        fs::File::create(&path)
            .unwrap()
            .set_len(128 * 1024 * 1024)
            .unwrap();
        let before = peak_bytes();
        assert_eq!(read_preferences(&path).unwrap_err(), PREFERENCE_READ_ERROR);
        let extra = peak_bytes().saturating_sub(before);
        eprintln!("large preference peak RSS increase: {extra} bytes");
        assert!(
            extra < 32 * 1024 * 1024,
            "oversized preferences allocated {extra} bytes"
        );
        assert_eq!(fs::metadata(&path).unwrap().len(), 128 * 1024 * 1024);
        let before_save = peak_bytes();
        save_preferences(&path, false).unwrap();
        let save_extra = peak_bytes().saturating_sub(before_save);
        assert!(
            save_extra < 32 * 1024 * 1024,
            "preference repair allocated {save_extra} bytes"
        );
        assert!(!read_preferences(&path).unwrap());
    }
    #[test]
    fn a_preference_save_that_exceeds_the_limit_preserves_the_original() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("prefs.json");
        let mut preferences =
            serde_json::json!({"automatic_checks": true, "future_preference": ""});
        let padding =
            MAX_PREFERENCE_BYTES as usize - serde_json::to_vec(&preferences).unwrap().len();
        preferences["future_preference"] = serde_json::Value::String("x".repeat(padding));
        let bytes = serde_json::to_vec(&preferences).unwrap();
        assert_eq!(bytes.len(), MAX_PREFERENCE_BYTES as usize);
        fs::write(&path, &bytes).unwrap();
        assert!(save_preferences(&path, false).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(read_preferences(&path).unwrap());
    }
    #[test]
    fn preference_size_limit_accepts_boundary_and_rejects_larger_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("prefs.json");
        let mut bytes = br#"{"automatic_checks":false}"#.to_vec();
        bytes.resize(MAX_PREFERENCE_BYTES as usize, b' ');
        fs::write(&path, &bytes).unwrap();
        assert!(!read_preferences(&path).unwrap());
        bytes.push(b' ');
        fs::write(&path, &bytes).unwrap();
        assert_eq!(read_preferences(&path).unwrap_err(), PREFERENCE_READ_ERROR);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    #[test]
    fn additive_update_preferences_keep_the_choice_and_survive_saves() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("prefs.json");
        for automatic in [false, true] {
            let saved = serde_json::json!({
                "automatic_checks": automatic,
                "future_preference": {"channel": "preview", "days": [1, 3, 5]}
            });
            let bytes = serde_json::to_vec(&saved).unwrap();
            fs::write(&path, &bytes).unwrap();
            assert_eq!(read_preferences(&path).unwrap(), automatic);
            assert_eq!(fs::read(&path).unwrap(), bytes);

            save_preferences(&path, !automatic).unwrap();
            let written: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(written["automatic_checks"], !automatic);
            assert_eq!(written["future_preference"], saved["future_preference"]);
            assert_eq!(read_preferences(&path).unwrap(), !automatic);
        }
    }

    #[test]
    fn malformed_known_update_preferences_are_not_accepted_as_additive_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("prefs.json");
        for bytes in [
            br#"{"automatic_checks":"false","future_preference":true}"#.as_slice(),
            br#"{"automatic_checks":null,"future_preference":true}"#,
            br#"{"future_preference":true}"#,
        ] {
            fs::write(&path, bytes).unwrap();
            assert!(read_preferences(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        save_preferences(&path, false).unwrap();
        assert!(!read_preferences(&path).unwrap());
    }

    #[test]
    fn missing_preferences_enable_checks_but_corrupt_preferences_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("prefs.json");
        assert!(read_preferences(&p).unwrap());
        save_preferences(&p, false).unwrap();
        assert!(!read_preferences(&p).unwrap());
        fs::write(&p, "broken").unwrap();
        assert!(read_preferences(&p).is_err());
    }
    #[test]
    fn ordinary_executable_does_not_claim_updatable_package() {
        assert_eq!(
            package_kind(Path::new("/usr/bin/silo-ui"), None, None),
            "manual"
        );
    }
    #[test]
    fn preflight_counts_actual_tar_members_and_rejects_invalid_archive() {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for size in [16usize, 32] {
            let mut header = tar::Header::new_gnu();
            header.set_size(size as u64);
            header.set_mode(0o644);
            header.set_cksum();
            archive
                .append_data(
                    &mut header,
                    format!("Silo.app/file{size}"),
                    vec![0u8; size].as_slice(),
                )
                .unwrap();
        }
        let bytes = archive.into_inner().unwrap().finish().unwrap();
        assert_eq!(unpacked_size(&bytes, true).unwrap(), 48);
        assert_eq!(unpacked_size(&bytes, false).unwrap(), bytes.len() as u64);
        assert!(unpacked_size(b"not an archive", true).is_err());
    }
    #[test]
    fn debian_with_stray_appimage_environment_uses_system_installer() {
        let image = tempfile::NamedTempFile::new().unwrap();
        assert_eq!(
            package_kind(
                Path::new("/usr/bin/silo-ui"),
                Some(image.path()),
                Some(tauri::utils::config::BundleType::Deb)
            ),
            if cfg!(target_os = "linux") {
                "debian"
            } else {
                "manual"
            }
        );
    }

    #[test]
    fn final_download_size_rejects_overflow_even_if_completion_wins_notification() {
        assert!(validate_download_size(MAX_DOWNLOAD_BYTES).is_ok());
        assert!(validate_download_size(MAX_DOWNLOAD_BYTES + 1).is_err());
        assert!(validate_download_size(u64::MAX).is_err());
    }
    #[test]
    fn preflight_rejects_insufficient_or_unknown_space_without_touching_installation() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("Silo.AppImage");
        fs::write(&destination, "old app").unwrap();
        for (size, expected) in [(2 * 1024 * 1024, "2 MiB"), (2 * 1024 * 1024 + 1, "3 MiB")] {
            let payload = vec![0; size];
            let error =
                preflight_destination(&destination, &payload, false, |_| Ok(0)).unwrap_err();
            assert_eq!(
                error,
                format!(
                    "Not enough space to install the update. Free at least {expected} and retry."
                )
            );
        }
        assert!(
            preflight_destination(&destination, b"new app", false, |_| Err(
                "Space unavailable".into()
            ))
            .is_err()
        );
        assert_eq!(fs::read(&destination).unwrap(), b"old app");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[test]
    fn preflight_rejects_unwritable_parent_before_measuring_space() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("not-a-directory");
        fs::write(&parent, "preserve").unwrap();
        let error = preflight_destination(&parent.join("Silo.AppImage"), b"new app", false, |_| {
            panic!("must reject unwritable staging first")
        })
        .unwrap_err();
        assert!(error.contains("cannot write"));
        assert_eq!(fs::read(parent).unwrap(), b"preserve");
    }
}
