//! Tauri adapters for the macOS-computer bridge and remote-management commands.
use crate::{bridge_error::BridgeError, macos_remote_engine as engine, remote};
use serde_json::{json, Value};
use tauri::Window;

pub(crate) const DISPLAY_STREAM: &str = engine::DISPLAY_STREAM;

/// The controlling side of the methods below, for commands the main window calls.
async fn call(
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
    tokio::task::spawn_blocking(move || remote::call_remote_typed(&device_id, method, params))
        .await
        .map_err(|_| {
            BridgeError::from("The remote macOS computer operation failed unexpectedly.")
        })?
}

/// The macOS computers of a connected device, and whether that device can host them.
#[tauri::command]
pub(crate) async fn remote_macos_snapshot(
    window: Window,
    device_id: String,
) -> Result<Value, BridgeError> {
    call(window, device_id, "macos.snapshot", json!({})).await
}

/// Creates a macOS computer on a connected device.
#[tauri::command]
pub(crate) async fn remote_macos_create(
    window: Window,
    device_id: String,
    request: Value,
) -> Result<Value, BridgeError> {
    let params = json!({ "request": request });
    // The owner validates limits; this only refuses malformed requests early.
    engine::parse_create(&params)?;
    call(window, device_id, "macos.create", params).await
}

/// Starts, stops, force stops, deletes or sets up a macOS computer on a connected device.
#[tauri::command]
pub(crate) async fn remote_macos_action(
    window: Window,
    device_id: String,
    computer_id: String,
    action: String,
) -> Result<Value, BridgeError> {
    let params = json!({ "computerId": computer_id, "action": action });
    engine::parse_action(&params)?;
    call(window, device_id, "macos.action", params).await
}
