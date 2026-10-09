//! Turning off System Integrity Protection from macOS Recovery.
use tauri::AppHandle;

/// Turns off System Integrity Protection on the stopped computer `id`.
pub(super) fn disable_sip(_app: &AppHandle, _id: &str) -> Result<(), String> {
    Err("Turning off System Integrity Protection is not implemented yet.".into())
}
