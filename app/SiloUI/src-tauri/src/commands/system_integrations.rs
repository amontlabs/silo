use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::system_integrations::{self, IntegrationStatus, SystemIntegrations};

#[cfg(target_os = "macos")]
mod macos;

#[tauri::command]
pub(crate) async fn read_system_integrations(
    _app: AppHandle,
    window: WebviewWindow,
) -> Result<SystemIntegrations, String> {
    system_integrations::require_main_label(window.label())?;
    tokio::task::spawn_blocking(|| Ok(system_integrations::read()))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn set_login_item(
    _app: AppHandle,
    window: WebviewWindow,
    enabled: bool,
) -> Result<IntegrationStatus, String> {
    system_integrations::require_main_label(window.label())?;
    tokio::task::spawn_blocking(move || system_integrations::set_login_item(enabled))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn request_notification_authorization(
    _app: AppHandle,
    window: WebviewWindow,
) -> Result<IntegrationStatus, String> {
    system_integrations::require_main_label(window.label())?;
    tokio::task::spawn_blocking(system_integrations::request_notification_authorization)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn open_integration_settings(
    _app: AppHandle,
    window: WebviewWindow,
    integration: String,
) -> Result<(), String> {
    system_integrations::require_main_label(window.label())?;
    tokio::task::spawn_blocking(move || system_integrations::open_settings(&integration))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) fn show_integration_error(
    app: AppHandle,
    window: WebviewWindow,
    message: String,
) -> Result<(), String> {
    system_integrations::require_main_label(window.label())?;
    app.dialog()
        .message(message)
        .title("Silo couldn’t complete the request")
        .kind(MessageDialogKind::Error)
        .parent(&window)
        .show(|_| {});
    Ok(())
}

pub(crate) fn install_notifications(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos::install_notifications(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}
