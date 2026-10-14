//! Tauri adapters for the host-owned signed update flow.
#[path = "../../engine/src/updates/debian.rs"]
mod debian;
use crate::updates as engine;
use engine::{Snapshot, State, MAX_DOWNLOAD_BYTES, RELEASE_URL};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime},
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

struct ManagedState {
    policy: State,
    update: Option<Update>,
}

struct Controller {
    preferences: PathBuf,
    state: Mutex<ManagedState>,
}

fn modify(app: &AppHandle, f: impl FnOnce(&mut State)) -> Result<Snapshot, String> {
    modify_managed(app, |state| f(&mut state.policy))
}

fn update_snapshot_payload(snapshot: &Snapshot) -> Result<serde_json::Value, serde_json::Error> {
    let mut payload = serde_json::to_value(snapshot)?;
    // `UpdateSnapshot` omits the legacy Debian helper field while job progress
    // intentionally carries the complete source Snapshot.
    if let Some(fields) = payload.as_object_mut() {
        fields.remove("installStatus");
    }
    Ok(payload)
}

fn publish_job_progress(
    context: &dyn silo_engine::EngineContext,
    job_id: &silo_api::jobs::JobId,
    snapshot: &Snapshot,
) -> Result<(), String> {
    let value = serde_json::to_value(snapshot).map_err(|error| error.to_string())?;
    let data = serde_json::from_value::<silo_api::methods::updates::UpdateProgressData>(value)
        .map_err(|error| error.to_string())?;
    silo_engine::jobs::publish_progress(
        context,
        silo_api::jobs::JobProgress::Update {
            job_id: job_id.clone(),
            kind: None,
            computer_id: None,
            data,
        },
        silo_engine::context::Audience::All,
    )
    .map_err(|error| error.to_string())
}

fn emit_update_state(app: &AppHandle, snapshot: &Snapshot) {
    let _ = app.emit("silo://update-state", snapshot);
    if let (Some(context), Ok(payload)) = (
        silo_engine::context::current(),
        update_snapshot_payload(snapshot),
    ) {
        context.publish(
            silo_engine::context::Event::new("updates.changed", payload),
            silo_engine::context::Audience::All,
        );
    }
}

fn modify_managed(app: &AppHandle, f: impl FnOnce(&mut ManagedState)) -> Result<Snapshot, String> {
    let controller = app.state::<Controller>();
    let mut state = controller
        .state
        .lock()
        .map_err(|_| "Update state is unavailable.")?;
    f(&mut state);
    let snapshot = state.policy.snapshot.clone();
    emit_update_state(app, &snapshot);
    Ok(snapshot)
}
fn fail(app: &AppHandle, message: &str, details: impl ToString) -> Result<Snapshot, String> {
    modify(app, |state| {
        engine::record_failure(state, message, details);
    })
}
pub(crate) fn recovery_failed(app: &AppHandle, message: String) {
    let _ = fail(
        app,
        "Some computers could not resume after updating. Relaunch Silo to retry.",
        &message,
    );
    crate::commands::notifications::notify(
        app,
        crate::notifications::failure(
            "update:resume",
            "Computers couldn\u{2019}t resume after updating",
            &message,
            None,
        ),
    );
}
fn ready(app: &AppHandle) -> Result<(), String> {
    engine::readiness(|| {
        crate::commands::backup_controller::update_ready(app)?;
        // Fast readiness check only: refuse if any computer operation is active or queued.
        if !crate::runtime::OPERATIONS.is_idle() {
            return Err("Wait for computer operations to finish before updating.".into());
        }
        crate::runtime::shutdown::ensure_accepting_operations()
    })
}
pub(crate) fn install(app: &AppHandle) -> Result<(), String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| "Update storage is unavailable.")?
        .join("update-preferences.json");
    let preference = engine::read_preferences(&path);
    let error = preference.as_ref().err().cloned();
    // Silo Dev is built from source and never replaces itself with a production release.
    let automatic = preference.unwrap_or(false) && !crate::channel::current().is_development();
    let executable = std::env::current_exe().unwrap_or_default();
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    let bundle = match tauri::utils::platform::bundle_type() {
        Some(tauri::utils::config::BundleType::AppImage) => Some(engine::BundleKind::AppImage),
        Some(tauri::utils::config::BundleType::Deb) => Some(engine::BundleKind::Debian),
        _ => None,
    };
    let snapshot = Snapshot {
        last_checked: None,
        retry_action: None,
        phase: if error.is_some() { "error" } else { "idle" }.into(),
        current_version: app.package_info().version.to_string(),
        available_version: None,
        release_notes: None,
        downloaded_bytes: 0,
        total_bytes: None,
        automatic_checks: automatic,
        package_kind: engine::package_kind(&executable, appimage.as_deref(), bundle).into(),
        release_url: RELEASE_URL.into(),
        error,
        error_details: None,
        running_computers: vec![],
        can_install: false,
        install_block_reason: None,
        install_status: None,
    };
    app.manage(Controller {
        preferences: path,
        state: Mutex::new(ManagedState {
            policy: State::new(snapshot, SystemTime::now()),
            update: None,
        }),
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(engine::POLL_INTERVAL).await;
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
        engine::focused(&mut state.policy, SystemTime::now());
    };
}
#[tauri::command]
pub(crate) async fn get_update_state(app: AppHandle) -> Result<Snapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let installing = app
            .state::<Controller>()
            .state
            .lock()
            .map_or(true, |state| state.policy.snapshot.phase == "installing");
        let readiness = if installing {
            Err("An update operation is already running.".to_string())
        } else {
            ready(&app)
        };
        // Do not compete with an active runtime mutation just to refresh a settings card.
        let running = readiness.and_then(|_| {
            crate::runtime::update_recovery::running_names(&crate::commands::runtime::host(&app))
                .map_err(|_| {
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
        let controller = app.state::<Controller>();
        let mut state = controller
            .state
            .lock()
            .map_err(|_| "Update state is unavailable.")?;
        let snapshot = engine::update_automatic_checks(
            &mut state.policy,
            &controller.preferences,
            enabled,
            engine::save_preferences,
        )?;
        emit_update_state(&app, &snapshot);
        Ok(snapshot)
    })
    .await
    .map_err(|_| "Update preferences could not be saved.".to_owned())?
}

#[tauri::command]
pub(crate) async fn check_for_update(app: AppHandle) -> Result<Snapshot, String> {
    check(app, false).await
}
async fn check(app: AppHandle, automatic: bool) -> Result<Snapshot, String> {
    if crate::channel::current().is_development() {
        // No network request: the development channel has no update feed.
        return modify(&app, engine::no_feed_available);
    }
    let previous_phase = {
        let controller = app.state::<Controller>();
        let mut state = controller
            .state
            .lock()
            .map_err(|_| "Update state is unavailable.")?;
        // Admit automatic checks under the same lock as manual actions.
        if automatic && !engine::automatic_check_due(&state.policy, SystemTime::now()) {
            return Ok(state.policy.snapshot.clone());
        }
        // Keep a discovered update and verified bytes until the result is known.
        let previous = engine::begin_check(&mut state.policy)?;
        emit_update_state(&app, &state.policy.snapshot);
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
        Ok(update) => modify_managed(&app, |managed| {
            engine::complete_check(&mut managed.policy, SystemTime::now(), true);
            let found = update.as_ref().map(|update| engine::UpdateInfo {
                version: update.version.clone(),
                body: update.body.clone(),
            });
            let keep = engine::settle_check(&mut managed.policy, found);
            if !keep {
                managed.update = update;
                managed.policy.update = managed.update.as_ref().map(|update| engine::UpdateInfo {
                    version: update.version.clone(),
                    body: update.body.clone(),
                });
            }
            managed.policy.snapshot.last_checked = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .ok();
        }),
        Err(e) => {
            let pending = app
                .state::<Controller>()
                .state
                .lock()
                .map_or(false, |s| s.policy.update.is_some());
            if !pending {
                modify(&app, |state| {
                    engine::complete_check(state, SystemTime::now(), false)
                })?;
                return fail(&app, check_error_message(&e), e);
            }
            // A failed re-check leaves a found or downloaded update actionable.
            modify(&app, |state| {
                engine::check_failed(
                    state,
                    SystemTime::now(),
                    &previous_phase,
                    check_error_message(&e),
                    &e,
                );
            })
        }
    }
}

fn check_error_message(error: &tauri_plugin_updater::Error) -> &'static str {
    use tauri_plugin_updater::Error;
    let kind = match error {
        Error::ReleaseNotFound => engine::CheckErrorKind::ReleaseUnavailable,
        Error::Serialization(_) | Error::Semver(_) => engine::CheckErrorKind::InvalidRelease,
        Error::Reqwest(error) if error.is_decode() => engine::CheckErrorKind::InvalidRelease,
        Error::TargetNotFound(_) | Error::TargetsNotFound(_) => {
            engine::CheckErrorKind::NoPlatformRelease
        }
        _ => engine::CheckErrorKind::Other,
    };
    engine::check_error_message(kind)
}
#[tauri::command]
pub(crate) async fn download_update(app: AppHandle) -> Result<Snapshot, String> {
    let context = silo_engine::context::current();
    let (mut update, job, initial_snapshot) = {
        let controller = app.state::<Controller>();
        let mut state = controller
            .state
            .lock()
            .map_err(|_| "Update state is unavailable.")?;
        if matches!(
            state.policy.snapshot.phase.as_str(),
            "checking" | "downloading" | "installing"
        ) {
            return Err("An update operation is already running.".into());
        }
        if matches!(
            state.policy.snapshot.package_kind.as_str(),
            "manual" | "debian"
        ) {
            return Err("Download the package from Releases to update this installation.".into());
        }
        if state.policy.update.is_none() {
            return Err("Check for an update before downloading.".into());
        }
        let update = state
            .update
            .clone()
            .ok_or("Check for an update before downloading.")?;
        let job = if let Some(context) = context.as_ref() {
            let acceptance = silo_engine::jobs::allocate(context.as_ref(), "updates.download")
                .map_err(|error| error.to_string())?;
            Some((context.clone(), acceptance.job_id))
        } else {
            None
        };
        if let Err(error) = engine::begin_download(&mut state.policy) {
            if let Some((job_context, job_id)) = job.as_ref() {
                if let Err(persist_error) = silo_engine::jobs::finish_with_error(
                    job_context.as_ref(),
                    job_id.clone(),
                    silo_api::jobs::ErrorCode::Internal,
                    error.clone(),
                    silo_engine::context::Audience::All,
                ) {
                    return Err(persist_error.to_string());
                }
            }
            return Err(error);
        }
        let initial_snapshot = state.policy.snapshot.clone();
        emit_update_state(&app, &state.policy.snapshot);
        (update, job, initial_snapshot)
    };
    if let Some((context, job_id)) = job.as_ref() {
        if let Err(error) = publish_job_progress(context.as_ref(), job_id, &initial_snapshot) {
            eprintln!("Silo could not persist initial update progress: {error}");
        }
    }
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
                if let Ok(snapshot) = modify(&app, |state| engine::download_progress(state, received, total)) {
                    if let Some((context, job_id)) = job.as_ref() {
                        if let Err(error) = publish_job_progress(context.as_ref(), job_id, &snapshot) {
                            eprintln!("Silo could not persist update progress: {error}");
                        }
                    }
                }
                last_report = std::time::Instant::now();
            }
        }, || {}) => result.map_err(|e| e.to_string()),
    };
    // The completed download and notification can both be ready in the same
    // select poll. Check the final verified buffer independently of that race.
    let result = result.and_then(|bytes| {
        engine::validate_download_size(bytes.len() as u64)?;
        Ok(bytes)
    });
    let result = match result {
        Ok(bytes) => match modify(&app, |state| engine::download_ready(state, bytes)) {
            Ok(_) => get_update_state(app).await,
            Err(error) => fail(
                &app,
                "The update could not be downloaded or verified. Your installation was not changed. Try again.",
                error,
            ),
        },
        Err(error) => fail(
            &app,
            "The update could not be downloaded or verified. Your installation was not changed. Try again.",
            error,
        ),
    };
    if let Some((context, job_id)) = job {
        let finish = match &result {
            Ok(snapshot) => update_snapshot_payload(snapshot)
                .map_err(|error| error.to_string())
                .and_then(|value| {
                    silo_engine::jobs::finish_with_result(
                        context.as_ref(),
                        job_id,
                        value,
                        silo_engine::context::Audience::All,
                    )
                    .map_err(|error| error.to_string())
                }),
            Err(message) => silo_engine::jobs::finish_with_error(
                context.as_ref(),
                job_id,
                silo_api::jobs::ErrorCode::Internal,
                message.clone(),
                silo_engine::context::Audience::All,
            )
            .map_err(|error| error.to_string()),
        };
        if let Err(error) = finish {
            return Err(format!(
                "The update finished but its job outcome could not be saved: {error}"
            ));
        }
    }
    result
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
    engine::preflight_destination(
        &destination,
        bytes,
        cfg!(target_os = "macos"),
        engine::available_install_space,
    )
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
    let admission = engine::installation_guard()?;
    let backup = crate::commands::backup_controller::update_guard(app)?;
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
            crate::settings::flush_for_update()?;
            let (closed, prepared) = prepare_after_transfers(close_transfers, || {
                crate::runtime::update_recovery::prepare(
                    &crate::commands::runtime::host(app),
                    consent,
                )
            });
            transfers = Some(closed);
            prepared
        },
    );
    if let Err(error) = result {
        // Computers can only have stopped once the install stage took the gate.
        if runtime.is_some() {
            if let Err(resume) = crate::runtime::update_recovery::restore_locked(
                &crate::commands::runtime::host(app),
            ) {
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
            crate::macos_remote_viewer::close_all();
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
        let update = state
            .update
            .clone()
            .ok_or("Download and verify an update before installing.")?;
        let (bytes, is_debian) = engine::begin_install(&mut state.policy, true)?;
        emit_update_state(&app, &state.policy.snapshot);
        (update, bytes, is_debian)
    };
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<(), InstallError> {
        crate::startup::cancel_and_wait(&worker);
        if is_debian {
            return install_debian(&worker, &update.version, stop_computers);
        }
        // Reservation uses the same atomic gate as backup/restore admission.
        let closed_macos = std::cell::Cell::new(false);
        let admission = (|| {
            // Linux computers are stopped and restored from the update journal after the
            // restart. macOS computers live in this process and cannot be restored, so
            // the update refuses while one is busy and admits no new one until it ends.
            let admission = engine::installation_guard()?;
            crate::macos_computers::close_for_update()?;
            closed_macos.set(true);
            let backup = crate::commands::backup_controller::update_guard(&worker)?;
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
            Err(error) => { if closed_macos.get() { crate::macos_computers::reopen_after_update(); } let _ = modify(&worker, |s| engine::restore_install_bytes(s, bytes)); return Err(error.into()); }
        };
        let mut transfers = None;
        let result = installation_preflight(&bytes)
            .and_then(|_| crate::settings::flush_for_update())
            .and_then(|_| {
                let (closed, prepared) = prepare_after_transfers(close_transfers, || {
                    crate::runtime::update_recovery::prepare(&crate::commands::runtime::host(&worker), stop_computers)
                });
                transfers = Some(closed);
                prepared
            })
            .and_then(|_| update.install(&bytes).map_err(|e| e.to_string()));
        if let Err(error) = result {
            crate::macos_computers::reopen_after_update();
            let restore = crate::runtime::update_recovery::restore_locked(&crate::commands::runtime::host(&worker));
            drop(transfers);
            let _ = modify(&worker, |s| engine::restore_install_bytes(s, bytes));
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
        crate::commands::applications::open_browser(&app, RELEASE_URL)
    })
    .await
    .map_err(|_| "The browser could not be opened.".to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

    #[test]
    fn reexec_replaces_or_exits_without_resuming_writes() {
        use std::{os::unix::process::CommandExt, process::Command};
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
                    "commands::updates::tests::reexec_replaces_or_exits_without_resuming_writes",
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
            assert!(!directory.path().join("mutation").exists());
            assert_eq!(output.status.code(), Some(if replace { 0 } else { 1 }));
            if !replace {
                let error = String::from_utf8_lossy(&output.stderr);
                assert!(error.contains("exec failed") && error.contains("Reopen Silo"));
            }
        }
    }
}
