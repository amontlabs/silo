use crate::{
    log_export::{self, ExportPermit},
    runtime::runtime_logs::Query,
};
use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

fn require_main(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Open the main Silo window to export logs.".into())
    }
}

#[tauri::command]
pub(crate) fn cancel_log_export(window: WebviewWindow) -> Result<(), String> {
    require_main(&window)?;
    log_export::cancel();
    Ok(())
}

#[tauri::command]
pub(crate) async fn export_computer_logs(
    app: AppHandle,
    window: WebviewWindow,
    requests: Vec<Query>,
) -> Result<bool, String> {
    require_main(&window)?;
    log_export::validate_requests(&requests)?;
    let cancelled = log_export::cancellation_check();
    tokio::task::spawn_blocking(move || {
        let permit = log_export::begin_export()?;
        if cancelled() {
            return Ok(false);
        }
        let destination = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Export logs")
            .set_file_name("silo-logs.jsonl")
            .add_filter("JSON Lines", &["jsonl"])
            .blocking_save_file()
            .map(|selected| {
                selected
                    .into_path()
                    .map_err(|_| "The export destination is unavailable.".to_string())
            })
            .transpose()?;
        let Some(destination) = destination else {
            return Ok(false);
        };
        if cancelled() {
            return Ok(false);
        }
        export(permit, &app, requests, destination, cancelled)
    })
    .await
    .map_err(|_| "The log export task failed.".to_owned())?
}

fn export(
    permit: ExportPermit,
    app: &AppHandle,
    requests: Vec<Query>,
    destination: std::path::PathBuf,
    cancelled: impl Fn() -> bool,
) -> Result<bool, String> {
    log_export::export_to(
        permit,
        requests,
        destination,
        |request| super::runtime_logs::query(app, request).map_err(|error| error.message),
        cancelled,
    )
}
