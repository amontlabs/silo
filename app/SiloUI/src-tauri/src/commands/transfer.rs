//! File transfers between this device and the computers, through `files.upload` and
//! `files.download`. The pickers and the one-time selection tokens stay on this device: the
//! UI never holds paths, only the token a picker issued to its window, and the paths go to
//! the engine in the upload request.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, DragDropEvent, Window};
use tauri_plugin_dialog::DialogExt;

use crate::engine_client::{
    forwarding::{resolve, with_device},
    Engine, EngineClient, EngineError,
};

fn require_main(window: &Window) -> Result<(), EngineError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(EngineError::internal(
            "Open the main Silo window to transfer files.",
        ))
    }
}

// MARK: Selections

const SELECTION_TTL: Duration = Duration::from_secs(600);
const MAX_SELECTIONS: usize = 16;

struct Selection {
    token: String,
    owner: String,
    paths: Vec<String>,
    created: Instant,
}

static SELECTIONS: Mutex<Vec<Selection>> = Mutex::new(Vec::new());

fn held() -> std::sync::MutexGuard<'static, Vec<Selection>> {
    SELECTIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What the UI is told about a picked selection: an opaque token and the names to show.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadSelection {
    token: String,
    names: Vec<String>,
}

/// Remembers `paths` for `owner`, the window whose upload may use them.
fn issue_selection(owner: &str, paths: Vec<String>) -> UploadSelection {
    let names = paths
        .iter()
        .map(|path| {
            std::path::Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut held = held();
    held.retain(|selection| selection.created.elapsed() < SELECTION_TTL);
    while held.len() >= MAX_SELECTIONS {
        held.remove(0);
    }
    held.push(Selection {
        token: token.clone(),
        owner: owner.to_owned(),
        paths,
        created: Instant::now(),
    });
    UploadSelection { token, names }
}

/// Removes and returns a selection issued to `owner`.
fn take_selection(token: &str, owner: &str) -> Result<Vec<String>, EngineError> {
    let mut held = held();
    held.retain(|selection| selection.created.elapsed() < SELECTION_TTL);
    let index = held
        .iter()
        .position(|selection| selection.token == token && selection.owner == owner)
        .ok_or_else(|| EngineError::internal("Choose the files again."))?;
    Ok(held.remove(index).paths)
}

/// Puts a selection back after an upload that only asked a question.
fn restore_selection(token: &str, owner: &str, paths: Vec<String>) {
    held().push(Selection {
        token: token.to_owned(),
        owner: owner.to_owned(),
        paths,
        created: Instant::now(),
    });
}

// MARK: Operations

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ConflictPolicy {
    Ask,
    Replace,
    KeepBoth,
}

/// Uploads the files `selection` stands for. A conflict answer sends nothing and keeps the
/// selection, so the UI can ask and send again.
pub(crate) async fn upload(
    engine: &EngineClient,
    owner: &str,
    transfer_id: String,
    computer: &str,
    directory: String,
    selection: &str,
    conflict: ConflictPolicy,
    viewer_connection: Option<&str>,
) -> Result<Value, EngineError> {
    let (computer_id, device_id) = resolve(engine, computer).await?;
    let paths = take_selection(selection, owner)?;
    let mut params = with_device(
        json!({
            "transferId": transfer_id,
            "computerId": computer_id,
            "directory": directory,
            "conflict": conflict,
            "paths": paths,
            "origin": "client",
        }),
        device_id.as_deref(),
    );
    // A file dropped on a viewer window goes to that viewer's computer through its connection.
    if let Some(connection) = viewer_connection {
        params["origin"] = json!("viewer");
        params["connectionId"] = json!(connection);
    }
    let outcome = engine.run_job("files.upload", params).await;
    match &outcome {
        Ok(result) if result.get("status").and_then(Value::as_str) == Some("conflict") => {
            restore_selection(selection, owner, paths);
        }
        Ok(_) => {}
        // Nothing was sent when the engine refused the request: the selection is still valid.
        Err(error) if error.is("unsupported_remote_operation") || error.is("busy") => {
            restore_selection(selection, owner, paths);
        }
        Err(_) => {}
    }
    outcome
}

/// The name a download suggests: the last component of the computer's path.
fn suggested_name(path: &str) -> String {
    path.rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("download")
        .to_owned()
}

/// Downloads `path` to `destination`; no destination is a cancelled save dialog.
pub(crate) async fn download(
    engine: &EngineClient,
    transfer_id: String,
    computer: &str,
    path: String,
    destination: Option<String>,
) -> Result<Value, EngineError> {
    let Some(destination) = destination else {
        return Ok(json!({"status": "cancelled"}));
    };
    let (computer_id, device_id) = resolve(engine, computer).await?;
    let params = with_device(
        json!({
            "transferId": transfer_id,
            "computerId": computer_id,
            "path": path,
            "destination": destination,
        }),
        device_id.as_deref(),
    );
    engine.run_job("files.download", params).await
}

pub(crate) async fn cancel(engine: &EngineClient, transfer_id: &str) -> Result<(), EngineError> {
    engine
        .call(
            "operations.cancel",
            json!({"jobId": format!("transfer:{transfer_id}")}),
        )
        .await?;
    Ok(())
}

// MARK: Commands

#[tauri::command]
pub(crate) async fn choose_upload_files(
    app: AppHandle,
    window: Window,
) -> Result<Option<UploadSelection>, EngineError> {
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
                    .ok_or_else(|| EngineError::internal("A selected file is unavailable."))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((!paths.is_empty()).then(|| issue_selection(&owner, paths)))
    })
    .await
    .map_err(|_| EngineError::internal("The file picker failed."))?
}

#[tauri::command]
pub(crate) async fn upload_files(
    engine: Engine<'_>,
    window: Window,
    transfer_id: String,
    computer: String,
    directory: String,
    selection: String,
    conflict: ConflictPolicy,
) -> Result<Value, EngineError> {
    crate::commands::desktop_viewer::require_computer(&window, &computer)
        .map_err(EngineError::internal)?;
    let viewer_connection = if crate::commands::desktop_viewer::is_viewer_label(window.label()) {
        Some(
            crate::commands::desktop_viewer::connection_of(window.label())
                .ok_or_else(|| EngineError::internal("The desktop is not connected."))?,
        )
    } else {
        None
    };
    upload(
        &engine,
        window.label(),
        transfer_id,
        &computer,
        directory,
        &selection,
        conflict,
        viewer_connection.as_deref(),
    )
    .await
}

#[tauri::command]
pub(crate) async fn download_file(
    engine: Engine<'_>,
    app: AppHandle,
    window: Window,
    transfer_id: String,
    computer: String,
    path: String,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    let suggested = suggested_name(&path);
    let destination = tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_parent(&window)
            .set_title("Save file")
            .set_file_name(&suggested)
            .blocking_save_file()
            .and_then(|selected| selected.into_path().ok())
            .and_then(|path| path.to_str().map(str::to_owned))
    })
    .await
    .map_err(|_| EngineError::internal("The download failed."))?;
    download(&engine, transfer_id, &computer, path, destination).await
}

#[tauri::command]
pub(crate) async fn cancel_transfer(
    engine: Engine<'_>,
    transfer_id: String,
) -> Result<(), EngineError> {
    cancel(&engine, &transfer_id).await
}

const DRAG_EVENT: &str = "silo://viewer-drag";
const DROP_EVENT: &str = "silo://viewer-drop";

/// Turns a native drag onto a viewer window into the events that window's page follows: its
/// drag state, and a one-time selection of the dropped files that its upload names.
pub(crate) fn native_drop(app: &AppHandle, shell: &str, event: &DragDropEvent) {
    use tauri::Emitter;
    if !crate::commands::desktop_viewer::is_viewer_label(shell) {
        return;
    }
    let emit = |name: &str, payload: Value| {
        let _ = app.emit_to(shell, name, payload);
    };
    match event {
        DragDropEvent::Enter { .. } => emit(DRAG_EVENT, json!(true)),
        DragDropEvent::Leave => emit(DRAG_EVENT, json!(false)),
        DragDropEvent::Drop { paths, .. } => {
            emit(DRAG_EVENT, json!(false));
            let paths: Vec<String> = paths
                .iter()
                .filter_map(|path| path.to_str().map(str::to_owned))
                .collect();
            if !paths.is_empty() {
                let selection = issue_selection(shell, paths);
                emit(DROP_EVENT, serde_json::to_value(selection).unwrap_or(Value::Null));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";
    const DEVICE: &str = "00000000-0000-4000-8000-0000000000d1";

    fn finished(result: Value) -> Value {
        json!({"jobId": "transfer:t1", "state": "finished", "outcome": {"result": result}})
    }

    fn replies(method: &str, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "computers.list" => Ok(json!({
                "linux": {"computers": [{"configuration": {"id": DEV, "name": "dev"}}]},
                "macos": {"computers": []}
            })),
            "files.upload" | "files.download" => Ok(json!({"jobId": "transfer:t1"})),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn an_upload_sends_the_selected_paths_and_returns_the_outcome() {
        let fake = FakeEngine::start(|method, params| match method {
            "operations.job.get" => Ok(finished(json!({"status": "done", "names": ["a.txt"]}))),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let selection = issue_selection("main", vec!["/tmp/a.txt".into()]);
        assert_eq!(selection.names, vec!["a.txt"]);
        let outcome = upload(
            &engine,
            "main",
            "t1".into(),
            "dev",
            "/workspace".into(),
            &selection.token,
            ConflictPolicy::KeepBoth,
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome, json!({"status": "done", "names": ["a.txt"]}));
        assert_eq!(
            fake.calls("files.upload"),
            vec![json!({
                "transferId": "t1", "computerId": DEV, "directory": "/workspace",
                "conflict": "keepBoth", "paths": ["/tmp/a.txt"], "origin": "client"
            })]
        );
        // The token was consumed, and another window never held it.
        assert!(take_selection(&selection.token, "main").is_err());
    }

    #[tokio::test]
    async fn a_file_dropped_on_a_viewer_goes_through_that_viewers_connection() {
        let fake = FakeEngine::start(|method, params| match method {
            "operations.job.get" => Ok(finished(json!({"status": "done", "names": ["a.txt"]}))),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let selection = issue_selection("desktop-shell-1", vec!["/tmp/a.txt".into()]);
        upload(
            &engine,
            "desktop-shell-1",
            "t9".into(),
            "dev",
            "/home/user/Downloads".into(),
            &selection.token,
            ConflictPolicy::KeepBoth,
            Some("conn-1"),
        )
        .await
        .unwrap();
        let sent = &fake.calls("files.upload")[0];
        assert_eq!(sent["origin"], "viewer");
        assert_eq!(sent["connectionId"], "conn-1");
    }

    #[tokio::test]
    async fn a_conflict_answer_keeps_the_selection_for_the_next_attempt() {
        let answers = std::sync::Arc::new(Mutex::new(vec![
            finished(json!({"status": "done", "names": ["a.txt"]})),
            finished(json!({"status": "conflict", "names": ["a.txt"]})),
        ]));
        let queue = answers.clone();
        let fake = FakeEngine::start(move |method, params| match method {
            "operations.job.get" => Ok(queue.lock().unwrap().pop().unwrap()),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let selection = issue_selection("main", vec!["/tmp/a.txt".into()]);
        let target = format!("silo-remote:{DEVICE}:{DEV}");
        let first = upload(
            &engine,
            "main",
            "t1".into(),
            &target,
            "/workspace".into(),
            &selection.token,
            ConflictPolicy::Ask,
            None,
        )
        .await
        .unwrap();
        assert_eq!(first["status"], "conflict");
        let second = upload(
            &engine,
            "main",
            "t1".into(),
            &target,
            "/workspace".into(),
            &selection.token,
            ConflictPolicy::Replace,
            None,
        )
        .await
        .unwrap();
        assert_eq!(second["status"], "done");
        assert_eq!(fake.calls("files.upload")[0]["deviceId"], DEVICE);
        let wrong_window = upload(
            &engine,
            "other",
            "t2".into(),
            "dev",
            "/workspace".into(),
            "nope",
            ConflictPolicy::Ask,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(wrong_window.message, "Choose the files again.");
    }

    #[tokio::test]
    async fn downloads_go_to_the_chosen_destination_and_a_cancelled_dialog_sends_nothing() {
        let fake = FakeEngine::start(|method, params| match method {
            "operations.job.get" => Ok(finished(json!({"status": "done", "path": "/tmp/f.txt"}))),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let done = download(
            &engine,
            "t1".into(),
            "dev",
            "/workspace/f.txt".into(),
            Some("/tmp/f.txt".into()),
        )
        .await
        .unwrap();
        assert_eq!(done["path"], "/tmp/f.txt");
        assert_eq!(
            fake.calls("files.download"),
            vec![
                json!({"transferId": "t1", "computerId": DEV, "path": "/workspace/f.txt", "destination": "/tmp/f.txt"})
            ]
        );
        let cancelled = download(&engine, "t2".into(), "dev", "/workspace/f.txt".into(), None)
            .await
            .unwrap();
        assert_eq!(cancelled, json!({"status": "cancelled"}));
        assert_eq!(fake.calls("files.download").len(), 1);
        assert_eq!(suggested_name("/workspace/dir/f.txt"), "f.txt");
        assert_eq!(suggested_name("/workspace/dir/"), "dir");
    }

    #[tokio::test]
    async fn cancelling_addresses_the_transfer_job_and_failures_are_typed() {
        let fake = FakeEngine::start(|method, params| match method {
            "operations.cancel" if params["jobId"] == "transfer:missing" => {
                Err(("not_found", "That transfer is no longer running.".into()))
            }
            "operations.job.get" => Ok(json!({
                "jobId": "transfer:t1", "state": "finished",
                "outcome": {"error": {"code": "unsupported_remote_operation", "message": "Not yet."}}
            })),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        cancel(&engine, "t1").await.unwrap();
        assert_eq!(
            fake.calls("operations.cancel"),
            vec![json!({"jobId": "transfer:t1"})]
        );
        assert_eq!(
            cancel(&engine, "missing").await.unwrap_err().code,
            "not_found"
        );
        let failed = download(
            &engine,
            "t1".into(),
            "dev",
            "/workspace/f".into(),
            Some("/tmp/f".into()),
        )
        .await
        .unwrap_err();
        assert_eq!(failed.code, "unsupported_remote_operation");
    }
}
