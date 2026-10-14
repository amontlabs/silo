use silo_engine::{Audience, Event};
use tauri::{AppHandle, Manager};

use crate::notifications::{self, Notice, PendingDelivery};

pub(crate) fn install(app: &AppHandle) {
    crate::system_integrations::install_notifications(app);
    crate::health_watch::install(app);
}

/// A notice with no other UI owner: in-app toast and, when unfocused, a system notification.
pub(crate) fn notify(app: &AppHandle, notice: Notice) {
    if let Some(context) = silo_engine::context::current() {
        context.publish(
            Event::new(
                notifications::NOTICE_EVENT,
                serde_json::to_value(&notice).unwrap_or(serde_json::Value::Null),
            ),
            Audience::All,
        );
    }
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
            let Ok(settings) = crate::settings::current_settings(app) else {
                return false;
            };
            notifications::enabled(&settings, notice.category)
        },
        crate::system_integrations::deliver_notification,
        crate::system_integrations::clear_notifications,
    );
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
