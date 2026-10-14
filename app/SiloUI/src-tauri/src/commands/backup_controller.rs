//! Exports and imports, through `exports.*`. The pickers and revealing a file stay on this
//! device: it picks the paths, the engine reads and writes them.
use crate::backup_controller as engine_controller;
use crate::engine_client::{
    forwarding::{not_found, roster},
    Engine, EngineClient, EngineError,
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

fn require_main(window: &WebviewWindow) -> Result<(), EngineError> {
    (window.label() == "main").then_some(()).ok_or_else(|| {
        EngineError::internal("Only the main Silo window can export or import computers.")
    })
}

// The three functions below are still called by `updates.rs` and `startup.rs`, which wait on
// the in-process controller; they leave with those callers.

pub(crate) fn wait_for_recovery(app: &AppHandle) -> Result<(), String> {
    let controller = app.state::<Arc<engine_controller::Controller>>();
    engine_controller::wait_for_recovery(&controller, &|| crate::startup::is_cancelled(app))
}

pub(crate) fn update_ready(app: &AppHandle) -> Result<(), String> {
    engine_controller::update_ready(&app.state::<Arc<engine_controller::Controller>>())
}

pub(crate) fn update_guard(app: &AppHandle) -> Result<engine_controller::UpdateGuard, String> {
    engine_controller::update_guard(
        app.state::<Arc<engine_controller::Controller>>()
            .inner()
            .clone(),
    )
}

// MARK: Operations

pub(crate) async fn state(engine: &EngineClient) -> Result<Value, EngineError> {
    engine.call("exports.state", json!({})).await
}

fn path_text(path: &std::path::Path) -> Result<String, EngineError> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        EngineError::internal("Silo could not use a path with unsupported characters.")
    })
}

/// Records the folder the user picked and returns the path the engine kept.
pub(crate) async fn set_destination(
    engine: &EngineClient,
    path: &std::path::Path,
) -> Result<String, EngineError> {
    let remembered = engine
        .call("exports.destination.set", json!({"path": path_text(path)?}))
        .await?;
    remembered
        .get("destination")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| EngineError::internal("The engine kept no export destination."))
}

pub(crate) async fn inspect(
    engine: &EngineClient,
    archive_path: String,
    request_id: Option<String>,
) -> Result<Value, EngineError> {
    let request_id = request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    engine
        .run_job(
            "exports.inspect",
            json!({"archivePath": archive_path, "requestId": request_id}),
        )
        .await
}

/// `true` when a check under `request_id` was running and has been asked to stop.
pub(crate) async fn cancel_inspection(
    engine: &EngineClient,
    request_id: &str,
) -> Result<bool, EngineError> {
    match engine
        .call(
            "operations.cancel",
            json!({"jobId": format!("inspect:{request_id}")}),
        )
        .await
    {
        Ok(_) => Ok(true),
        Err(error) if error.is("not_found") => Ok(false),
        Err(error) => Err(error),
    }
}

/// The archive file to reveal: only one the engine lists is shown.
fn listed_archive(state: &Value, archive_path: &str) -> Result<PathBuf, EngineError> {
    state
        .get("archives")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(state.pointer("/operation/archive"))
        .find(|archive| archive.get("archivePath").and_then(Value::as_str) == Some(archive_path))
        .map(|_| PathBuf::from(archive_path))
        .ok_or_else(|| not_found("That export is not in the list."))
}

pub(crate) async fn reveal_target(
    engine: &EngineClient,
    archive_path: &str,
) -> Result<PathBuf, EngineError> {
    listed_archive(&state(engine).await?, archive_path)
}

/// Starts an export and returns its operation id.
pub(crate) async fn start_export(
    engine: &EngineClient,
    destination: String,
    computers: Vec<String>,
    checkpoint_id: Option<String>,
) -> Result<String, EngineError> {
    let roster = roster(engine, None).await?;
    let computer_ids = computers
        .iter()
        .map(|name| roster.id_of(name))
        .collect::<Result<Vec<_>, _>>()?;
    let mut params = json!({"destination": destination, "computerIds": computer_ids});
    if let Some(checkpoint_id) = checkpoint_id {
        params["checkpointId"] = Value::String(checkpoint_id);
    }
    let started = engine.call("exports.create", params).await?;
    let job_id = started
        .get("jobId")
        .and_then(Value::as_str)
        .ok_or_else(|| EngineError::internal("The engine did not accept the export."))?;
    Ok(job_id.strip_prefix("exports:").unwrap_or(job_id).to_owned())
}

pub(crate) async fn start_import(
    engine: &EngineClient,
    archive_path: String,
    new_name: String,
    source_name: Option<String>,
) -> Result<(), EngineError> {
    let mut params = json!({"archivePath": archive_path, "newName": new_name});
    if let Some(source_name) = source_name {
        params["sourceName"] = Value::String(source_name);
    }
    engine.call("exports.import", params).await?;
    Ok(())
}

/// Stops the export or import in progress.
pub(crate) async fn cancel_operation(engine: &EngineClient) -> Result<(), EngineError> {
    let operation_id = state(engine)
        .await?
        .get("operationId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| not_found("No export or import is running."))?;
    engine
        .call(
            "operations.cancel",
            json!({"jobId": format!("exports:{operation_id}")}),
        )
        .await?;
    Ok(())
}

pub(crate) async fn dismiss(
    engine: &EngineClient,
    expected_operation: Value,
    expected_operation_id: Option<String>,
) -> Result<bool, EngineError> {
    let mut params = json!({"expectedOperation": expected_operation});
    if let Some(id) = expected_operation_id {
        params["expectedOperationId"] = Value::String(id);
    }
    let dismissed = engine.call("exports.dismiss", params).await?;
    Ok(dismissed.as_bool().unwrap_or(false))
}

pub(crate) async fn acknowledge(
    engine: &EngineClient,
    expected_operation_id: String,
) -> Result<bool, EngineError> {
    let changed = engine
        .call(
            "exports.acknowledge",
            json!({"expectedOperationId": expected_operation_id}),
        )
        .await?;
    Ok(changed.as_bool().unwrap_or(false))
}

// MARK: Commands

#[tauri::command]
pub(crate) async fn read_backup_state(engine: Engine<'_>) -> Result<Value, EngineError> {
    state(&engine).await
}

#[tauri::command]
pub(crate) async fn choose_backup_destination(
    engine: Engine<'_>,
    app: AppHandle,
    window: WebviewWindow,
) -> Result<Option<String>, EngineError> {
    require_main(&window)?;
    let starting_directory = state(&engine)
        .await?
        .get("destination")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let picked = tokio::task::spawn_blocking(move || {
        let mut dialog = app
            .dialog()
            .file()
            .set_title("Choose where to export")
            .set_parent(&window);
        if let Some(directory) = starting_directory {
            dialog = dialog.set_directory(directory);
        }
        dialog.blocking_pick_folder()
    })
    .await
    .map_err(|error| EngineError::internal(error.to_string()))?;
    let Some(selected) = picked else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|error| EngineError::internal(error.to_string()))?;
    let path = std::fs::canonicalize(&path).map_err(|error| {
        EngineError::internal(format!(
            "Silo could not use the selected destination: {error}"
        ))
    })?;
    set_destination(&engine, &path).await.map(Some)
}

#[tauri::command]
pub(crate) async fn choose_backup_archive(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<Option<String>, EngineError> {
    require_main(&window)?;
    tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose a Silo export to import")
            .set_parent(&window)
            .add_filter("Silo export", &["silo-backup"])
            .blocking_pick_file()
            .map(|path| {
                path.into_path()
                    .map_err(|error| EngineError::internal(error.to_string()))
                    .and_then(|path| path_text(&path))
            })
            .transpose()
    })
    .await
    .map_err(|error| EngineError::internal(error.to_string()))?
}

#[tauri::command]
pub(crate) async fn inspect_backup_archive(
    engine: Engine<'_>,
    window: WebviewWindow,
    archive_path: String,
    request_id: Option<String>,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    inspect(&engine, archive_path, request_id).await
}

#[tauri::command]
pub(crate) async fn cancel_backup_inspection(
    engine: Engine<'_>,
    window: WebviewWindow,
    request_id: String,
) -> Result<bool, EngineError> {
    require_main(&window)?;
    cancel_inspection(&engine, &request_id).await
}

#[tauri::command]
pub(crate) async fn reveal_backup_archive(
    engine: Engine<'_>,
    window: WebviewWindow,
    archive_path: String,
) -> Result<(), EngineError> {
    require_main(&window)?;
    let path = reveal_target(&engine, &archive_path).await?;
    tokio::task::spawn_blocking(move || {
        tauri_plugin_opener::reveal_item_in_dir(&path).map_err(|error| {
            EngineError::internal(format!("Silo could not show the export file: {error}"))
        })
    })
    .await
    .map_err(|error| EngineError::internal(error.to_string()))?
}

#[tauri::command]
pub(crate) async fn start_backup(
    engine: Engine<'_>,
    window: WebviewWindow,
    destination: String,
    computers: Vec<String>,
    checkpoint_id: Option<String>,
) -> Result<String, EngineError> {
    require_main(&window)?;
    start_export(&engine, destination, computers, checkpoint_id).await
}

#[tauri::command]
pub(crate) async fn start_restore(
    engine: Engine<'_>,
    window: WebviewWindow,
    archive_path: String,
    new_name: String,
    source_name: Option<String>,
) -> Result<(), EngineError> {
    require_main(&window)?;
    start_import(&engine, archive_path, new_name, source_name).await
}

#[tauri::command]
pub(crate) async fn cancel_backup_operation(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<(), EngineError> {
    require_main(&window)?;
    cancel_operation(&engine).await
}

#[tauri::command]
pub(crate) async fn dismiss_backup_operation(
    engine: Engine<'_>,
    window: WebviewWindow,
    expected_operation: Value,
    expected_operation_id: Option<String>,
) -> Result<bool, EngineError> {
    require_main(&window)?;
    dismiss(&engine, expected_operation, expected_operation_id).await
}

#[tauri::command]
pub(crate) async fn acknowledge_backup_result(
    engine: Engine<'_>,
    window: WebviewWindow,
    expected_operation_id: String,
) -> Result<bool, EngineError> {
    require_main(&window)?;
    acknowledge(&engine, expected_operation_id).await
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";

    fn backup_state() -> Value {
        json!({
            "snapshotId": "s", "operationId": "op9", "availability": "available",
            "archives": [{"name": "a", "archivePath": "/exports/a.silo-backup", "computers": ["dev"]}],
            "operation": null, "destination": "/exports"
        })
    }

    fn replies(method: &str, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "computers.list" => Ok(json!({
                "linux": {"computers": [{"configuration": {"id": DEV, "name": "dev"}}]},
                "macos": {"computers": []}
            })),
            "exports.state" => Ok(backup_state()),
            "exports.destination.set" => Ok(json!({"destination": "/exports"})),
            "exports.create" => Ok(json!({"jobId": "exports:op9"})),
            "exports.import" => Ok(json!({"jobId": "exports:op10"})),
            "exports.inspect" => Ok(json!({"jobId": "inspect:r1"})),
            "exports.dismiss" | "exports.acknowledge" => Ok(json!(true)),
            "operations.job.get" => Ok(json!({
                "jobId": "inspect:r1", "state": "finished",
                "outcome": {"result": {"archive": {"name": "a"}, "valid": true}}
            })),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn the_destination_and_state_come_from_the_engine() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(state(&engine).await.unwrap()["destination"], "/exports");
        let kept = set_destination(&engine, std::path::Path::new("/exports"))
            .await
            .unwrap();
        assert_eq!(kept, "/exports");
        assert_eq!(
            fake.calls("exports.destination.set"),
            vec![json!({"path": "/exports"})]
        );
    }

    #[tokio::test]
    async fn an_export_translates_names_and_returns_the_operation_id() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let id = start_export(
            &engine,
            "/exports".into(),
            vec!["dev".into()],
            Some("c1".into()),
        )
        .await
        .unwrap();
        assert_eq!(id, "op9");
        assert_eq!(
            fake.calls("exports.create"),
            vec![json!({"destination": "/exports", "computerIds": [DEV], "checkpointId": "c1"})]
        );
        let missing = start_export(&engine, "/exports".into(), vec!["ghost".into()], None)
            .await
            .unwrap_err();
        assert_eq!(missing.code, "not_found");
        assert_eq!(fake.calls("exports.create").len(), 1);
    }

    #[tokio::test]
    async fn an_import_starts_without_waiting_and_an_inspection_waits_for_its_result() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        start_import(
            &engine,
            "/exports/a.silo-backup".into(),
            "copy".into(),
            Some("dev".into()),
        )
        .await
        .unwrap();
        assert_eq!(
            fake.calls("exports.import"),
            vec![
                json!({"archivePath": "/exports/a.silo-backup", "newName": "copy", "sourceName": "dev"})
            ]
        );
        assert!(fake.calls("operations.job.get").is_empty());
        let inspected = inspect(&engine, "/exports/a.silo-backup".into(), Some("r1".into()))
            .await
            .unwrap();
        assert_eq!(inspected["valid"], true);
        assert_eq!(
            fake.calls("exports.inspect"),
            vec![json!({"archivePath": "/exports/a.silo-backup", "requestId": "r1"})]
        );
    }

    #[tokio::test]
    async fn cancelling_addresses_the_inspect_and_export_jobs() {
        let fake = FakeEngine::start(|method, params| match method {
            "operations.cancel" if params["jobId"] == "inspect:gone" => {
                Err(("not_found", "No such job.".into()))
            }
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        assert!(cancel_inspection(&engine, "r1").await.unwrap());
        assert!(!cancel_inspection(&engine, "gone").await.unwrap());
        cancel_operation(&engine).await.unwrap();
        assert_eq!(
            fake.calls("operations.cancel"),
            vec![
                json!({"jobId": "inspect:r1"}),
                json!({"jobId": "inspect:gone"}),
                json!({"jobId": "exports:op9"}),
            ]
        );
    }

    #[tokio::test]
    async fn only_listed_archives_are_revealed_and_results_are_dismissed_by_expectation() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            reveal_target(&engine, "/exports/a.silo-backup")
                .await
                .unwrap(),
            PathBuf::from("/exports/a.silo-backup")
        );
        assert_eq!(
            reveal_target(&engine, "/etc/passwd")
                .await
                .unwrap_err()
                .code,
            "not_found"
        );
        assert!(
            dismiss(&engine, json!({"kind": "result"}), Some("op9".into()))
                .await
                .unwrap()
        );
        assert!(acknowledge(&engine, "op9".into()).await.unwrap());
        assert_eq!(
            fake.calls("exports.dismiss"),
            vec![json!({"expectedOperation": {"kind": "result"}, "expectedOperationId": "op9"})]
        );
        assert_eq!(
            fake.calls("exports.acknowledge"),
            vec![json!({"expectedOperationId": "op9"})]
        );
    }

    #[tokio::test]
    async fn an_unsupported_export_is_the_typed_unavailable_state() {
        let fake = FakeEngine::start(|method, params| match method {
            "exports.state" => Err(("unsupported_remote_operation", "Not available yet.".into())),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            state(&engine).await.unwrap_err().code,
            "unsupported_remote_operation"
        );
        assert!(cancel_operation(&engine).await.is_err());
    }
}
