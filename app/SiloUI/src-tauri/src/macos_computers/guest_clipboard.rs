//! Clipboard sharing between the host and the guest.
use tauri::AppHandle;

/// Installs the clipboard agent in the running computer `id`.
pub(super) fn install(_app: &AppHandle, _id: &str) -> Result<(), String> {
    Ok(())
}
