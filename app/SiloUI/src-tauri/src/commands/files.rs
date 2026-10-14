use crate::{files, remote, remote_access, runtime};
use tauri::AppHandle;

#[tauri::command]
pub(crate) async fn list_computer_directory(
    app: AppHandle,
    computer: String,
    path: String,
    offset: usize,
    snapshot_id: Option<String>,
) -> Result<files::DirectoryPage, String> {
    tokio::task::spawn_blocking(move || {
        if let Some((device, computer)) = remote_access::target(&computer)? {
            let value = remote::call_remote(
                &device,
                "files.list",
                serde_json::json!({
                    "computerId": computer,
                    "path": path,
                    "offset": offset,
                    "snapshotId": snapshot_id,
                }),
            )?;
            return serde_json::from_value(value)
                .map_err(|_| "The remote device returned an invalid folder listing.".into());
        }
        let paths = runtime::runtime_paths(&app).map_err(|_| "Could not load this folder.")?;
        files::list_computer_directory(&paths, computer, path, offset, snapshot_id)
    })
    .await
    .map_err(|_| "Could not load this folder.".to_owned())?
}
