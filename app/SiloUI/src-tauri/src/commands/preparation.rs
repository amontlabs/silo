use crate::{preparation, runtime::RuntimePaths};

#[tauri::command]
pub(crate) fn read_preparation_status() -> preparation::PreparationStatus {
    preparation::snapshot()
}

#[tauri::command]
pub(crate) fn retry_preparation() -> preparation::PreparationStatus {
    preparation::retry()
}

/// Host lifecycle adapter retained for the startup track's existing call site.
pub(crate) fn start(_app: &tauri::AppHandle, paths: RuntimePaths) {
    preparation::start_with_paths(paths);
}
