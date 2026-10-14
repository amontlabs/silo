use crate::{runtime, ssh_access, ssh_connection};
use std::time::Duration;
use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

const FAILED: &str = "Could not read SSH access settings.";
const MONITOR_INTERVAL: Duration = Duration::from_secs(15);

/// Keep the periodic safety pass at the Tauri boundary, where each pass can
/// resolve the current runtime paths without making the SSH engine depend on an
/// application handle.
pub(crate) fn start_monitor(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        if let Ok(paths) = runtime::runtime_paths(&app) {
            let _ = ssh_access::monitor(&paths);
        }
        std::thread::sleep(MONITOR_INTERVAL);
    });
}

#[tauri::command]
pub(crate) async fn read_ssh_access_state(app: AppHandle) -> Result<ssh_access::State, String> {
    tokio::task::spawn_blocking(move || {
        let paths = runtime::runtime_paths(&app)?;
        ssh_access::read_state(&paths)
    })
    .await
    .map_err(|_| FAILED.to_owned())?
}

#[tauri::command]
pub(crate) async fn save_ssh_access(
    app: AppHandle,
    computer: String,
    enabled: bool,
    port: u16,
    bind_address: String,
    keys: Option<Vec<String>>,
) -> Result<ssh_access::State, String> {
    tokio::task::spawn_blocking(move || {
        let paths = runtime::runtime_paths(&app)?;
        ssh_access::save(&paths, computer, enabled, port, bind_address, keys)
    })
    .await
    .map_err(|_| FAILED.to_owned())?
}

#[tauri::command]
pub(crate) async fn ssh_connection(
    app: AppHandle,
    window: WebviewWindow,
    computer: Option<String>,
    device_id: Option<String>,
    computer_id: Option<String>,
    download: bool,
    network: Option<bool>,
) -> Result<Option<String>, String> {
    if window.label() != "main" {
        return Err("SSH keys can only be exported from the main window.".into());
    }

    tokio::task::spawn_blocking(move || {
        let paths = runtime::runtime_paths(&app)?;
        match ssh_connection::prepare(&paths, computer, device_id, computer_id, download, network)?
        {
            ssh_connection::ConnectionResult::Command(command) => Ok(Some(command)),
            ssh_connection::ConnectionResult::KeyExport { source, filename } => {
                let Some(selected) = app
                    .dialog()
                    .file()
                    .set_parent(&window)
                    .set_title("Save SSH connection key")
                    .set_file_name(filename)
                    .blocking_save_file()
                else {
                    return Ok(None);
                };
                let destination = selected.into_path().map_err(|_| "Choose a local file.")?;
                let private = crate::editor::read_regular(&source)?;
                crate::editor::write_private(&destination, &private)?;
                Ok(None)
            }
        }
    })
    .await
    .map_err(|_| "Could not prepare SSH connection.".to_owned())?
}
