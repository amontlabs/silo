use crate::{remote_access, runtime, transfer};
use tauri::{AppHandle, DragDropEvent, Window};
use tauri_plugin_dialog::DialogExt;

fn require_main(window: &Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Open the main Silo window to transfer files.".into())
    }
}

fn local_paths(app: &AppHandle, computer: &str) -> Result<Option<runtime::RuntimePaths>, String> {
    if remote_access::target(computer)?.is_some() {
        Ok(None)
    } else {
        runtime::runtime_paths(app).map(Some)
    }
}

#[tauri::command]
pub(crate) async fn choose_upload_files(
    app: AppHandle,
    window: Window,
) -> Result<Option<transfer::UploadSelection>, String> {
    require_main(&window)?;
    let owner = window.label().to_owned();
    tokio::task::spawn_blocking(move || {
        let picked = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Upload files")
            .blocking_pick_files()
            .unwrap_or_default();
        let paths = picked
            .into_iter()
            .map(|file| {
                file.into_path()
                    .ok()
                    .and_then(|path| path.to_str().map(str::to_owned))
                    .ok_or_else(|| "A selected file is unavailable.".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((!paths.is_empty()).then(|| transfer::issue_upload_selection(&owner, paths)))
    })
    .await
    .map_err(|_| "The file picker failed.".to_owned())?
}

#[tauri::command]
pub(crate) async fn upload_files(
    app: AppHandle,
    window: Window,
    transfer_id: String,
    computer: String,
    directory: String,
    selection: String,
    conflict: transfer::ConflictPolicy,
) -> Result<transfer::UploadOutcome, String> {
    runtime::shutdown::ensure_accepting_operations()?;
    crate::commands::desktop_viewer::require_computer(&window, &computer)?;
    let owner = window.label().to_owned();
    tokio::task::spawn_blocking(move || {
        let paths = local_paths(&app, &computer)?;
        transfer::upload_files(
            paths.as_ref(),
            owner,
            transfer_id,
            computer,
            directory,
            selection,
            conflict,
        )
    })
    .await
    .map_err(|_| "The upload failed.".to_owned())?
}

#[tauri::command]
pub(crate) async fn download_file(
    app: AppHandle,
    window: Window,
    transfer_id: String,
    computer: String,
    path: String,
) -> Result<transfer::DownloadOutcome, String> {
    require_main(&window)?;
    runtime::shutdown::ensure_accepting_operations()?;
    tokio::task::spawn_blocking(move || {
        let paths = local_paths(&app, &computer)?;
        let (session, suggested) =
            transfer::prepare_download(paths.as_ref(), transfer_id, computer, path)?;
        let destination = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Save file")
            .set_file_name(&suggested)
            .blocking_save_file()
            .and_then(|selected| selected.into_path().ok());
        transfer::download_prepared(session, destination)
    })
    .await
    .map_err(|_| "The download failed.".to_owned())?
}

#[tauri::command]
pub(crate) fn cancel_transfer(window: Window, transfer_id: String) -> Result<(), String> {
    transfer::cancel_transfer(window.label(), &transfer_id)
}

/// Convert a native Tauri drag into an engine event after the window label is known.
pub(crate) fn native_drop(shell: &str, event: &DragDropEvent) {
    let event = match event {
        DragDropEvent::Enter { .. } => transfer::NativeDropEvent::Enter,
        DragDropEvent::Leave => transfer::NativeDropEvent::Leave,
        DragDropEvent::Drop { paths, .. } => transfer::NativeDropEvent::Drop(
            paths
                .iter()
                .filter_map(|path| path.to_str().map(str::to_owned))
                .collect(),
        ),
        _ => transfer::NativeDropEvent::Other,
    };
    transfer::native_drop(shell, event);
}
