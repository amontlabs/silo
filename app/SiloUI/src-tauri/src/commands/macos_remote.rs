//! Tauri adapters for the macOS-computer bridge and remote-management commands.
use crate::{bridge_error::BridgeError, macos_remote_engine as engine, remote};
use serde_json::{json, Value};
use tauri::{AppHandle, Window};

pub(crate) const DISPLAY_STREAM: &str = engine::DISPLAY_STREAM;

/// Compatibility entry point used by the remote bridge until E4 switches to EngineContext.
pub(crate) fn dispatch(
    app: &AppHandle,
    method: &str,
    params: &Value,
) -> Result<Value, BridgeError> {
    let context = crate::commands::macos_computers::new_context(app).map_err(BridgeError::from)?;
    engine::dispatch(&context, method, params)
}

/// Opens the Screen Sharing stream for the owner-side bridge.
pub(crate) fn open_display_stream(
    app: &AppHandle,
    params: &Value,
) -> Result<std::net::TcpStream, String> {
    let context = crate::commands::macos_computers::new_context(app)?;
    engine::open_display_stream(&context, params)
}

/// The controlling side of the methods below, for commands the main window calls.
async fn call(
    app: AppHandle,
    window: Window,
    device_id: String,
    method: &'static str,
    params: Value,
) -> Result<Value, BridgeError> {
    if window.label() != "main" {
        return Err("Manage computers from the main Silo window."
            .to_string()
            .into());
    }
    tokio::task::spawn_blocking(move || remote::call_remote_typed(&app, &device_id, method, params))
        .await
        .map_err(|_| {
            BridgeError::from("The remote macOS computer operation failed unexpectedly.")
        })?
}

/// The macOS computers of a connected device, and whether that device can host them.
#[tauri::command]
pub(crate) async fn remote_macos_snapshot(
    app: AppHandle,
    window: Window,
    device_id: String,
) -> Result<Value, BridgeError> {
    call(app, window, device_id, "macos.snapshot", json!({})).await
}

/// Creates a macOS computer on a connected device.
#[tauri::command]
pub(crate) async fn remote_macos_create(
    app: AppHandle,
    window: Window,
    device_id: String,
    request: Value,
) -> Result<Value, BridgeError> {
    let params = json!({ "request": request });
    // The owner validates limits; this only refuses malformed requests early.
    engine::parse_create(&params)?;
    call(app, window, device_id, "macos.create", params).await
}

/// Starts, stops, force stops, deletes or sets up a macOS computer on a connected device.
#[tauri::command]
pub(crate) async fn remote_macos_action(
    app: AppHandle,
    window: Window,
    device_id: String,
    computer_id: String,
    action: String,
) -> Result<Value, BridgeError> {
    let params = json!({ "computerId": computer_id, "action": action });
    engine::parse_action(&params)?;
    call(app, window, device_id, "macos.action", params).await
}
