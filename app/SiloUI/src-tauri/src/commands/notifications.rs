//! System notifications for notices. The engine retains and publishes notices
//! (`notifications.notice`); this app shows them in the window as toasts and, when the main
//! window is not focused and the category is enabled, as system notifications.

use tauri::{AppHandle, Emitter, Manager};

use crate::notifications::{self, Notice, PendingDelivery};

pub(crate) fn install(app: &AppHandle) {
    crate::commands::system_integrations::install_notifications(app);
}

/// A notice with no other UI owner: in-app toast and, when unfocused, a system notification.
pub(crate) fn notify(app: &AppHandle, notice: Notice) {
    let _ = app.emit(
        notifications::NOTICE_EVENT,
        serde_json::to_value(&notice).unwrap_or(serde_json::Value::Null),
    );
    notify_native(app, notice);
}

/// A system notification only, for notices whose toast another owner already shows.
/// Delivery runs off the calling thread and never changes the caller's result.
pub(crate) fn notify_native(app: &AppHandle, notice: Notice) {
    let pending = notifications::prepare_delivery(notice);
    let app = app.clone();
    tauri::async_runtime::handle()
        .inner()
        .spawn_blocking(move || deliver(&app, pending));
}

/// Remove delivered system notifications about one computer (for example, after deletion).
/// Runs off the calling thread; failures are ignored.
pub(crate) fn clear_computer(_app: &AppHandle, computer_id: &str) {
    let pending = notifications::withdraw_computer(computer_id);
    tauri::async_runtime::handle()
        .inner()
        .spawn_blocking(move || pending.clear(crate::system_integrations::clear_notifications));
}

fn main_window_focused(app: &AppHandle) -> bool {
    app.get_webview_window("main").is_some_and(|window| {
        window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false)
    })
}

fn deliver(app: &AppHandle, pending: PendingDelivery) {
    // A delivery failure must not change the result of the computer/backup operation.
    pending.deliver(
        |notice| {
            if main_window_focused(app) {
                return false;
            }
            let settings = crate::engine_client::of(app).cached_settings();
            notifications::enabled(&settings, notice.category)
        },
        crate::system_integrations::deliver_notification,
        crate::system_integrations::clear_notifications,
    );
}

/// An engine notice (`notifications.notice`): the toast is relayed with the event; this adds
/// the system notification.
pub(crate) fn engine_notice(app: &AppHandle, payload: &serde_json::Value) {
    if let Ok(notice) = serde_json::from_value::<Notice>(payload.clone()) {
        notify_native(app, notice);
    }
}

/// `notifications.cleared`: the computer's notices are gone.
pub(crate) fn engine_cleared(app: &AppHandle, payload: &serde_json::Value) {
    if let Some(computer_id) = payload.get("computerId").and_then(serde_json::Value::as_str) {
        clear_computer(app, computer_id);
    }
}

/// Frontend mirror of a toast it already shows. System notification only.
#[tauri::command]
pub(crate) fn deliver_notice(app: AppHandle, notice: Notice) {
    notify_native(&app, notice);
}

#[tauri::command]
pub(crate) fn clear_computer_notices(app: AppHandle, computer_id: String) {
    clear_computer(&app, &computer_id);
}
