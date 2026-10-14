use crate::pre_upgrade_backup::{self, Status};
use std::path::PathBuf;
use tauri::WebviewWindow;

fn require_main(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Only the main window can manage the pre-upgrade backup.".into())
    }
}

fn paths() -> Result<(PathBuf, PathBuf), String> {
    let context = silo_engine::context::current()
        .ok_or_else(|| "Silo application storage is unavailable.".to_string())?;
    Ok((
        context.paths().app_data.clone(),
        context.paths().home.clone(),
    ))
}

/// File reads, a directory walk and deletion run off the async workers.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| "The pre-upgrade backup could not be read. Relaunch Silo and retry.")?
}

/// Whether a pre-upgrade backup exists and when it is deleted. Cheap: it measures nothing.
#[tauri::command]
pub(crate) async fn read_pre_upgrade_backup(
    window: WebviewWindow,
) -> Result<Option<Status>, String> {
    require_main(&window)?;
    let (app_data, _) = paths()?;
    blocking(move || pre_upgrade_backup::read_status(&app_data)).await
}

/// Allocated bytes of the backup, or `None` once it is gone. Walks the folder.
#[tauri::command]
pub(crate) async fn measure_pre_upgrade_backup(
    window: WebviewWindow,
) -> Result<Option<u64>, String> {
    require_main(&window)?;
    let (app_data, _) = paths()?;
    blocking(move || pre_upgrade_backup::measure_backup(&app_data)).await
}

#[tauri::command]
pub(crate) async fn delete_pre_upgrade_backup(window: WebviewWindow) -> Result<(), String> {
    require_main(&window)?;
    let (app_data, user_home) = paths()?;
    let removed =
        blocking(move || pre_upgrade_backup::delete_backup_for_user(&app_data, &user_home)).await?;
    if removed {
        pre_upgrade_backup::publish_changed();
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn reveal_pre_upgrade_backup(window: WebviewWindow) -> Result<(), String> {
    require_main(&window)?;
    let (app_data, _) = paths()?;
    let previous = blocking(move || pre_upgrade_backup::reveal_path(&app_data)).await?;
    tauri_plugin_opener::reveal_item_in_dir(&previous)
        .map_err(|error| format!("Silo could not show the pre-upgrade backup: {error}"))
}

/// The migration screen told the user about the backup.
#[tauri::command]
pub(crate) async fn acknowledge_pre_upgrade_backup_notice(
    window: WebviewWindow,
) -> Result<(), String> {
    require_main(&window)?;
    let (app_data, _) = paths()?;
    blocking(move || pre_upgrade_backup::acknowledge_notice(&app_data)).await
}
