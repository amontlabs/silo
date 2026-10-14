//! Tauri command and startup adapters for runtime storage maintenance.
use crate::{runtime::storage::StorageState, runtime::RuntimeHost};
use std::sync::Arc;
use tauri::AppHandle;

#[tauri::command]
pub async fn read_workspace_storage(
    app: AppHandle,
    computer_id: String,
) -> Result<StorageState, String> {
    command(app, computer_id, false).await
}

#[tauri::command]
pub async fn reclaim_workspace_storage(
    app: AppHandle,
    computer_id: String,
) -> Result<StorageState, String> {
    command(app, computer_id, true).await
}

async fn command(app: AppHandle, id: String, reclaim: bool) -> Result<StorageState, String> {
    tokio::task::spawn_blocking(move || {
        crate::runtime::storage::workspace_storage(&app, &id, reclaim)
    })
    .await
    .map_err(|_| {
        "Silo could not finish the workspace storage action. Refresh the Storage tab and retry."
            .to_string()
    })?
}

/// Starts the engine-owned monitor with the host's runtime capabilities.
pub(crate) fn start_monitor(app: &AppHandle) {
    let host: Arc<dyn RuntimeHost> = Arc::new(app.clone());
    crate::runtime::storage::start_monitor(host);
}
