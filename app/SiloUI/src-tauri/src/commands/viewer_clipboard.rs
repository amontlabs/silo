use silo_engine::{Audience, ClientId, Event};
use tauri::{AppHandle, Window};

use crate::viewer_clipboard::{self, Action, ActiveGuard, Report, Status};

/// Pastes the device clipboard into the computer, or copies the computer's clipboard to the
/// device. Only the viewer's own shell window can start it, from its toolbar.
#[tauri::command]
pub(crate) async fn desktop_viewer_clipboard(
    app: AppHandle,
    window: Window,
    computer: String,
    action: Action,
) -> Result<Report, String> {
    if !crate::desktop_viewer::is_viewer_label(window.label()) {
        return Err("Use the clipboard from a desktop viewer.".into());
    }
    crate::commands::desktop_viewer::require_computer(&window, &computer)?;
    let label = window.label().to_string();
    tokio::task::spawn_blocking(move || run_blocking(&app, &label, action))
        .await
        .map_err(|_| "Clipboard transfer failed.".into())
}

/// Runs one transfer for a viewer. The bridge and device clipboard can block, so this runs on a
/// worker thread.
fn run_blocking(app: &AppHandle, label: &str, action: Action) -> Report {
    let Some(_guard) = ActiveGuard::acquire(label) else {
        return Report::new(action, Status::Busy, None);
    };
    let device = crate::clipboard::system();
    crate::commands::desktop_viewer::with_bridge(app, label, |bridge| {
        Ok(match action {
            Action::Paste => viewer_clipboard::paste(bridge, device),
            Action::Copy => viewer_clipboard::copy(bridge, device),
        })
    })
    .unwrap_or_else(|_| Report::new(action, Status::NotConnected, None))
}

/// Starts a transfer from a shortcut or menu item and reports the outcome to that viewer's shell.
pub(crate) fn spawn(app: &AppHandle, label: &str, action: Action) {
    let (app, label) = (app.clone(), label.to_string());
    tauri::async_runtime::handle()
        .inner()
        .spawn_blocking(move || {
            let report = run_blocking(&app, &label, action);
            if let Some(context) = silo_engine::context::current() {
                // Desktop viewer labels are the current host's client ids.
                context.publish(
                    Event::new(
                        viewer_clipboard::EVENT,
                        serde_json::to_value(report).unwrap_or(serde_json::Value::Null),
                    ),
                    Audience::Client(ClientId::new(label)),
                );
            }
        });
}
