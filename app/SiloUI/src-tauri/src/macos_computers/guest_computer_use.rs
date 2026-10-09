//! Computer use inside the guest.
use tauri::AppHandle;

/// Installs computer use in the running computer `id`.
pub(super) fn install(_app: &AppHandle, _id: &str) -> Result<(), String> {
    Ok(())
}
