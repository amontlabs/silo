//! Tauri adapters for settings commands, native appearance, and process shutdown.
use crate::settings::{self, ExitRequest, Snapshot, Theme};
use serde_json::{Map, Value};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

fn require_main(label: &str) -> Result<(), String> {
    if label == "main" {
        Ok(())
    } else {
        Err("Only the main window can change settings".into())
    }
}

fn apply_theme(app: &AppHandle, snapshot: &Snapshot) {
    // Native glass, titlebars, dialogs, and both webviews inherit app appearance.
    // None clears an explicit appearance so System follows the OS again.
    app.set_theme(snapshot.theme().map(|theme| match theme {
        Theme::Light => tauri::Theme::Light,
        Theme::Dark => tauri::Theme::Dark,
    }));
}

fn publish_snapshot(app: &AppHandle, snapshot: &Snapshot) {
    apply_theme(app, snapshot);
    if let Some(error) = snapshot.save_error() {
        eprintln!("Silo settings: {error}");
    }
    if let Some(context) = silo_engine::context::current() {
        context.publish(
            silo_engine::context::Event::new(
                "settings:changed",
                serde_json::to_value(settings::public_snapshot(snapshot)).unwrap_or(Value::Null),
            ),
            silo_engine::context::Audience::All,
        );
    }
}

pub(crate) fn install(_app: &AppHandle) {
    settings::install();
}

#[tauri::command]
pub(crate) async fn initialize_settings(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<Snapshot, String> {
    require_main(window.label())?;
    let path = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?
        .join("settings.json");
    let snapshot = tokio::task::spawn_blocking(move || settings::initialize(Some(path)))
        .await
        .map_err(|error| error.to_string())??;
    publish_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn read_settings(
    _app: AppHandle,
    window: WebviewWindow,
) -> Result<Snapshot, String> {
    let main = window.label() == "main";
    tokio::task::spawn_blocking(move || settings::read(main))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn update_settings(
    app: AppHandle,
    window: WebviewWindow,
    patch: Map<String, Value>,
) -> Result<Snapshot, String> {
    require_main(window.label())?;
    let snapshot = tokio::task::spawn_blocking(move || settings::update(patch))
        .await
        .map_err(|error| error.to_string())??;
    publish_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn update_onboarding_draft(
    app: AppHandle,
    window: WebviewWindow,
    draft: Value,
) -> Result<Snapshot, String> {
    require_main(window.label())?;
    let snapshot = tokio::task::spawn_blocking(move || settings::update_draft(draft))
        .await
        .map_err(|error| error.to_string())??;
    publish_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn import_legacy_theme(
    app: AppHandle,
    window: WebviewWindow,
    theme: String,
) -> Result<Snapshot, String> {
    require_main(window.label())?;
    let snapshot = tokio::task::spawn_blocking(move || settings::import_legacy_theme(theme))
        .await
        .map_err(|error| error.to_string())??;
    publish_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn flush_settings(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    require_main(window.label())?;
    let (snapshot, saved) = tokio::task::spawn_blocking(settings::flush)
        .await
        .map_err(|error| error.to_string())??;
    publish_snapshot(&app, &snapshot);
    saved
}

/// Stop local computers, bounded by `deadline` when the session is ending.
fn stop_local_computers(app: &AppHandle, deadline: Option<Instant>) -> Result<(), String> {
    let Some(deadline) = deadline else {
        crate::startup::cancel_and_wait(app);
        return crate::runtime::shutdown::stop_local_computers(app, None);
    };
    let app = app.clone();
    run_before(deadline, move || {
        crate::startup::cancel_and_wait(&app);
        crate::runtime::shutdown::stop_local_computers(&app, Some(deadline))
    })
}

/// Run `work` on its own thread and stop waiting at `deadline`. The work keeps
/// running; the process is about to exit.
fn run_before<T: Send + 'static>(
    deadline: Instant,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .unwrap_or_else(|_| Err("Local computers did not finish stopping in time.".into()))
}

fn finish_exit(app: &AppHandle, frontend_completed: bool, generation: Option<u64>) {
    let state = settings::shutdown_state();
    if !state.claim_exit_for(frontend_completed, generation) {
        return;
    }
    let stopped = stop_local_computers(app, state.session_deadline());
    if state.approved() {
        return;
    }
    // Read the session state again: logout can begin while a Quit is stopping computers.
    let session_end = state.session_deadline().is_some();
    if let Err(error) = stopped {
        if !session_end {
            cancel_exit(app, format!("Silo stayed open because its local computers could not shut down safely.\n\n{error}\n\nCheck the affected computers and choose Quit Silo again. Computers on other devices were not stopped."));
            return;
        }
        eprintln!("Silo is exiting because the session ended: {error}");
    }
    // The frontend has drained its invoke queue. Wait for any native write already in progress.
    if let Err(error) = settings::flush_pending() {
        if !session_end {
            cancel_exit(app, format!("Local computers stopped, but Silo could not save its settings.\n\n{error}\n\nResolve the storage issue and choose Quit Silo again."));
            return;
        }
        eprintln!("Silo is exiting because the session ended; settings were not saved: {error}");
    }
    crate::remote_network::close_all();
    if state.allow_exit() {
        crate::system_shutdown::exit(app);
    }
}

fn cancel_exit(app: &AppHandle, message: String) {
    if !settings::shutdown_state().cancel_with(|| {
        crate::runtime::shutdown::cancel();
        crate::system_shutdown::cancel(app);
        let _ = app.emit("silo://shutdown-state-changed", false);
    }) {
        return;
    }
    // Startup remains cancelled: a failed Quit must not automatically restart computers
    // that have already stopped. Manual controls become available again.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        let _ = crate::system_integrations::show_integration_error(app.clone(), window, message);
    }
}

pub(crate) fn prevent_exit_until_saved(
    app: &AppHandle,
    api: &tauri::ExitRequestApi,
    code: Option<i32>,
) {
    let state = settings::shutdown_state();
    match settings::exit_request(code == Some(tauri::RESTART_EXIT_CODE), state.approved()) {
        ExitRequest::Restart => {
            state.mark_restart();
            return;
        }
        ExitRequest::Approved => return,
        ExitRequest::Gated => {}
    }
    api.prevent_exit();
    begin_exit(app);
}

/// Start graceful exit: stop admission, ask the frontend to save, and fall
/// back to finishing natively when it never answers.
fn begin_exit(app: &AppHandle) {
    let state = settings::shutdown_state();
    crate::startup::cancel(app);
    let Some(generation) = state.request_generation() else {
        return;
    };
    crate::runtime::shutdown::begin();
    let _ = app.emit("silo://shutdown-state-changed", true);
    if app.get_webview_window("main").is_some() {
        crate::status_panel::report(app.emit_to("main", "settings:flush-request", ()));
    }
    let app = app.clone();
    std::thread::spawn(move || {
        // Only a webview that never acknowledges may use the fallback. A responsive
        // frontend can take as long as it needs to drain its pending changes.
        std::thread::sleep(settings::FRONTEND_FLUSH_FALLBACK);
        finish_exit(&app, false, Some(generation));
    });
}

/// Startup failed before this process took ownership of local computers: any later exit
/// request exits directly instead of stopping them.
pub(crate) fn exit_without_shutdown(_app: &AppHandle) {
    settings::allow_exit_if_installed();
}

/// Logout, restart, shutdown or SIGTERM: no prompt, local computers stop within
/// `budget`, and a failed stop or save never cancels the exit.
pub(crate) fn end_session(app: &AppHandle, budget: Duration) {
    let Some(state) = settings::shutdown_state_if_installed() else {
        crate::system_shutdown::exit(app);
        return;
    };
    if state.approved() {
        crate::system_shutdown::exit(app);
        return;
    }
    state.begin_session_end(Instant::now() + budget);
    let deadline = state.session_deadline().unwrap();
    // A user Quit may already be blocked stopping computers or saving settings. Its
    // worker stays the sole stop owner; session termination cannot wait for it.
    let deadline_app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        if settings::shutdown_state().expire_session(Instant::now()) {
            eprintln!("Silo is exiting because the session shutdown deadline elapsed.");
            crate::system_shutdown::exit(&deadline_app);
        }
    });
    // An open Quit prompt no longer applies; its answer is ignored.
    settings::quit_confirmation().close();
    begin_exit(app);
    // Also bound a Quit whose frontend flush started before the session ended.
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(settings::session_flush_wait(deadline, Instant::now()));
        finish_exit(&app, false, None);
    });
}

/// Whether AppKit should wait for Silo's Quit path instead of terminating now.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn accepts_terminate_request() -> bool {
    settings::shutdown_state_if_installed().is_some_and(|state| !state.approved())
}

/// `RunEvent::Exit` without the graceful path: stop local computers within a bound.
pub(crate) fn exit_backstop(app: &AppHandle) {
    let Some(state) = settings::shutdown_state_if_installed() else {
        return;
    };
    if state.approved() || state.restarting() {
        return;
    }
    crate::startup::cancel(app);
    crate::runtime::shutdown::begin();
    let deadline = Instant::now() + crate::system_shutdown::SESSION_END_BUDGET;
    if let Err(error) = stop_local_computers(app, Some(deadline)) {
        eprintln!("Silo exited without its Quit path: {error}");
    }
}

#[tauri::command]
pub(crate) fn read_shutdown_state() -> bool {
    settings::shutdown_state().active()
}

/// Every user Quit entry point delegates here. This shows the main window and asks
/// the frontend only when local computers run and it can answer.
pub(crate) fn request_quit(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let confirmation = settings::quit_confirmation();
        let main = app.get_webview_window("main");
        if !confirmation.enabled() || main.is_none() || settings::shutdown_state().active() {
            app.exit(0);
            return;
        }
        let running = crate::runtime::update_recovery::running_names(&app);
        let Some(request) = confirmation.ask(running) else {
            app.exit(0);
            return;
        };
        if let Some(window) = main {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        if app
            .emit_to("main", "silo://quit-requested", &request)
            .is_err()
        {
            let _ = confirmation.answer(request.request_id, true);
            app.exit(0);
        }
    });
}

#[tauri::command]
pub(crate) fn enable_quit_confirmation(
    _app: AppHandle,
    window: WebviewWindow,
) -> Result<(), String> {
    require_main(window.label())?;
    settings::quit_confirmation().enable();
    Ok(())
}

#[tauri::command]
pub(crate) fn answer_quit_request(
    app: AppHandle,
    window: WebviewWindow,
    request_id: u64,
    confirmed: bool,
) -> Result<(), String> {
    require_main(window.label())?;
    if settings::quit_confirmation().answer(request_id, confirmed)? {
        app.exit(0);
    } else {
        crate::system_shutdown::cancel(&app);
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn begin_settings_flush(_app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    require_main(window.label())?;
    if settings::shutdown_state().begin_flush() {
        Ok(())
    } else {
        Err("A settings flush is not awaiting acknowledgment".into())
    }
}

#[tauri::command]
pub(crate) fn cancel_settings_flush(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    require_main(window.label())?;
    let state = settings::shutdown_state();
    if state.session_deadline().is_some() {
        // The session is ending: stop computers and exit without the unsaved changes.
        let app = app.clone();
        std::thread::spawn(move || finish_exit(&app, true, None));
    } else if state.claim_exit(true) {
        cancel_exit(&app, "Silo stayed open because pending changes could not be saved. Check the reported save error, then choose Quit Silo again. Local computers have not been shut down.".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn complete_settings_flush(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<(), String> {
    require_main(window.label())?;
    tokio::task::spawn_blocking(move || finish_exit(&app, true, None))
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{require_main, run_before};
    use std::time::{Duration, Instant};

    #[test]
    fn settings_mutations_are_limited_to_the_main_window() {
        assert!(require_main("main").is_ok());
        assert_eq!(
            require_main("status"),
            Err("Only the main window can change settings".into())
        );
    }

    #[test]
    fn bounded_stop_returns_the_result_or_gives_up_at_the_deadline() {
        let soon = Instant::now() + Duration::from_secs(5);
        assert_eq!(run_before(soon, || Ok(7)), Ok(7));
        assert_eq!(
            run_before(soon, || Err::<(), _>("stop failed".to_string())),
            Err("stop failed".into())
        );
        let started = Instant::now();
        let late = run_before(Instant::now() + Duration::from_millis(50), || {
            std::thread::sleep(Duration::from_secs(2));
            Ok(())
        });
        assert_eq!(
            late,
            Err("Local computers did not finish stopping in time.".into())
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
