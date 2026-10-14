//! Tauri adapters for backup and restore dialogs, command dispatch, and engine startup.
use crate::{backup_controller as engine, runtime};
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

fn require_main(window: &WebviewWindow) -> Result<(), String> {
    (window.label() == "main")
        .then_some(())
        .ok_or_else(|| "Only the main Silo window can export or import computers.".into())
}

pub(crate) fn install(app: &AppHandle) -> Result<(), String> {
    let host: Arc<dyn runtime::RuntimeHost> = Arc::new(crate::commands::runtime::host(app));
    let controller = engine::install(host)?;
    app.manage(controller);
    Ok(())
}

pub(crate) fn wait_for_recovery(app: &AppHandle) -> Result<(), String> {
    let controller = app.state::<Arc<engine::Controller>>();
    engine::wait_for_recovery(&controller, &|| crate::startup::is_cancelled(app))
}

pub(crate) fn wait_for_migration_recovery(app: &AppHandle) -> Result<(), String> {
    let controller = app.state::<Arc<engine::Controller>>();
    engine::wait_for_migration_recovery(&controller)
}

pub(crate) fn update_ready(app: &AppHandle) -> Result<(), String> {
    engine::update_ready(&app.state::<Arc<engine::Controller>>())
}

pub(crate) fn update_guard(app: &AppHandle) -> Result<engine::UpdateGuard, String> {
    engine::update_guard(app.state::<Arc<engine::Controller>>().inner().clone())
}

#[tauri::command]
pub(crate) async fn read_backup_state(
    controller: State<'_, Arc<engine::Controller>>,
) -> Result<engine::BackupState, String> {
    let controller = controller.inner().clone();
    tokio::task::spawn_blocking(move || engine::backup_state(&controller))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn choose_backup_destination(
    app: AppHandle,
    window: WebviewWindow,
    controller: State<'_, Arc<engine::Controller>>,
) -> Result<Option<String>, String> {
    require_main(&window)?;
    let controller = controller.inner().clone();
    tokio::task::spawn_blocking(move || {
        let starting_directory = engine::destination(&controller)?;
        let mut dialog = app
            .dialog()
            .file()
            .set_title("Choose where to export")
            .set_parent(&window);
        if let Some(directory) = starting_directory {
            dialog = dialog.set_directory(directory);
        }
        let Some(selected) = dialog.blocking_pick_folder() else {
            return Ok(None);
        };
        let path = selected.into_path().map_err(|error| error.to_string())?;
        let path = std::fs::canonicalize(&path)
            .map_err(|error| format!("Silo could not use the selected destination: {error}"))?;
        let selected = engine::selected_path_text(&path)?;
        engine::remember_destination(&controller, path);
        engine::publish(&controller);
        Ok(Some(selected))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn choose_backup_archive(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<Option<String>, String> {
    require_main(&window)?;
    tokio::task::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("Choose a Silo export to import")
            .set_parent(&window)
            .add_filter("Silo export", &["silo-backup"])
            .blocking_pick_file();
        selected
            .map(|path| path.into_path().map_err(|error| error.to_string()))
            .transpose()
            .and_then(|path| {
                path.map(|path| engine::selected_path_text(&path))
                    .transpose()
            })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn inspect_backup_archive(
    window: WebviewWindow,
    controller: State<'_, Arc<engine::Controller>>,
    archive_path: String,
    request_id: Option<String>,
) -> Result<engine::ArchiveInspectionResult, String> {
    require_main(&window)?;
    let controller = controller.inner().clone();
    let path = std::path::PathBuf::from(archive_path);
    let request_id = request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    tokio::task::spawn_blocking(engine::inspection_worker(controller, path, request_id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) fn cancel_backup_inspection(
    window: WebviewWindow,
    controller: State<'_, Arc<engine::Controller>>,
    request_id: String,
) -> Result<bool, String> {
    require_main(&window)?;
    Ok(engine::cancel_inspection(&controller, &request_id))
}

#[tauri::command]
pub(crate) async fn reveal_backup_archive(
    window: WebviewWindow,
    controller: State<'_, Arc<engine::Controller>>,
    archive_path: String,
) -> Result<(), String> {
    require_main(&window)?;
    let controller = controller.inner().clone();
    tokio::task::spawn_blocking(move || {
        let path = engine::authorize_reveal_archive(&controller, &archive_path)?;
        tauri_plugin_opener::reveal_item_in_dir(&path)
            .map_err(|error| format!("Silo could not show the export file: {error}"))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn start_backup(
    window: WebviewWindow,
    controller: State<'_, Arc<engine::Controller>>,
    destination: String,
    computers: Vec<String>,
    checkpoint_id: Option<String>,
) -> Result<String, String> {
    require_main(&window)?;
    let controller = controller.inner().clone();
    tokio::task::spawn_blocking(move || {
        engine::begin_export(controller, destination, computers, checkpoint_id)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn start_restore(
    window: WebviewWindow,
    controller: State<'_, Arc<engine::Controller>>,
    archive_path: String,
    new_name: String,
    source_name: Option<String>,
) -> Result<(), String> {
    require_main(&window)?;
    let controller = controller.inner().clone();
    tokio::task::spawn_blocking(move || {
        engine::begin_import(controller, archive_path, new_name, source_name)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn cancel_backup_operation(
    window: WebviewWindow,
    controller: State<'_, Arc<engine::Controller>>,
) -> Result<(), String> {
    require_main(&window)?;
    let controller = controller.inner().clone();
    tokio::task::spawn_blocking(move || engine::cancel_operation(&controller))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn dismiss_backup_operation(
    window: WebviewWindow,
    controller: State<'_, Arc<engine::Controller>>,
    expected_operation: Value,
    expected_operation_id: Option<String>,
) -> Result<bool, String> {
    require_main(&window)?;
    let controller = controller.inner().clone();
    tokio::task::spawn_blocking(move || {
        let dismissed = engine::dismiss_finished_operation(
            &controller,
            Some(&expected_operation),
            expected_operation_id.as_deref(),
        )?;
        if dismissed {
            engine::publish(&controller);
        }
        Ok(dismissed)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn acknowledge_backup_result(
    window: WebviewWindow,
    controller: State<'_, Arc<engine::Controller>>,
    expected_operation_id: String,
) -> Result<bool, String> {
    require_main(&window)?;
    let controller = controller.inner().clone();
    tokio::task::spawn_blocking(move || {
        let changed = engine::acknowledge_result(&controller, &expected_operation_id)?;
        if changed {
            engine::publish(&controller);
        }
        Ok(changed)
    })
    .await
    .map_err(|error| error.to_string())?
}
