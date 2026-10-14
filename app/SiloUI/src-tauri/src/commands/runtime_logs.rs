use crate::{
    bridge_error::BridgeError,
    runtime,
    runtime::runtime_logs::{self, Page, Query},
};
use tauri::AppHandle;

#[tauri::command]
pub(crate) async fn query_computer_logs(
    app: AppHandle,
    request: Query,
) -> Result<Page, BridgeError> {
    tokio::task::spawn_blocking(move || query(&app, request))
        .await
        .map_err(|error| error.to_string())?
}

/// UI-host adapter for remote routing, macOS setup logs and the selected runtime paths.
pub(crate) fn query(app: &AppHandle, request: Query) -> Result<Page, BridgeError> {
    let (device_id, device_name) = crate::remote::log_identity()?;
    if let Some(owner) = request
        .device_id
        .as_deref()
        .filter(|id| *id != device_id && *id != "local")
    {
        let params = serde_json::to_value(&request).map_err(|error| error.to_string())?;
        return runtime_logs::remote_page(crate::remote::call_remote_typed(
            owner,
            "runtime.logs",
            params,
        ));
    }

    // A macOS computer keeps its setup log beside Silo's own logs, not in the runtime.
    if let Some((name, directory)) = crate::macos_computers::log_target(app, &request.computer_id) {
        return runtime_logs::query_directory(&directory, request, &name, &device_id, &device_name)
            .map_err(BridgeError::from);
    }

    let paths =
        runtime::runtime_paths(&crate::commands::runtime::host(app)).map_err(BridgeError::from)?;
    runtime_logs::query_local(&paths, request, &device_id, &device_name).map_err(BridgeError::from)
}
