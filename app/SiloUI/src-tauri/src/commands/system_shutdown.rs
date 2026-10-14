use tauri::AppHandle;

use crate::system_shutdown::{ShutdownReason, SESSION_END_BUDGET};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

/// Route an operating-system quit request into Silo's Quit paths.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn route(app: &AppHandle, reason: ShutdownReason) {
    match reason {
        ShutdownReason::UserQuit => crate::settings::request_quit(app),
        ShutdownReason::Logout | ShutdownReason::Shutdown => {
            crate::settings::end_session(app, SESSION_END_BUDGET)
        }
    }
}

/// Install the operating-system quit hooks that need the running app.
pub(crate) fn install(app: &AppHandle) {
    listen_for_sigterm(app);
    #[cfg(target_os = "linux")]
    linux::install(app);
}

/// Install `applicationShouldTerminate:` before the event loop starts.
#[cfg(target_os = "macos")]
pub(crate) fn install_terminate_handler(app: &AppHandle) {
    if let Err(error) = macos::install(app) {
        eprintln!("Silo quit handling: {error}");
    }
}

fn listen_for_sigterm(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::handle().inner().spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(terminate) => terminate,
            Err(error) => {
                eprintln!("Silo quit handling: SIGTERM cannot be handled: {error}");
                return;
            }
        };
        // Logout and `systemctl stop` send SIGTERM; later signals only repeat it.
        while terminate.recv().await.is_some() {
            crate::settings::end_session(&app, SESSION_END_BUDGET);
        }
    });
}

/// The graceful Quit path finished: let the operating system or Silo end.
pub(crate) fn exit(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    linux::release();
    #[cfg(target_os = "macos")]
    if macos::reply(app, true) {
        return;
    }
    if let Some(context) = silo_engine::context::current() {
        context.process().exit(0);
    } else {
        app.exit(0);
    }
}

/// The user cancelled Quit or it failed: tell AppKit Silo keeps running.
pub(crate) fn cancel(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos::reply(app, false);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}
