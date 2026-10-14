//! Log export: the user picks the destination here, `silod` writes the file as a job.
use crate::engine_client::{Engine, EngineClient, EngineError};
use serde_json::{json, Value};
use std::sync::Mutex;
use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

fn require_main(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Open the main Silo window to export logs.".into())
    }
}

/// The export in flight: one at a time, from the destination dialog until the job ends.
#[derive(Default)]
pub(crate) struct ExportState {
    active: bool,
    cancelled: bool,
    job_id: Option<String>,
}

static EXPORT: Mutex<ExportState> = Mutex::new(ExportState {
    active: false,
    cancelled: false,
    job_id: None,
});

fn begin(state: &Mutex<ExportState>) -> Result<(), EngineError> {
    let mut state = state.lock().unwrap();
    if state.active {
        return Err(EngineError::internal(
            "A log export is already in progress.",
        ));
    }
    *state = ExportState {
        active: true,
        ..ExportState::default()
    };
    Ok(())
}

fn finish(state: &Mutex<ExportState>) {
    *state.lock().unwrap() = ExportState::default();
}

fn cancelled(state: &Mutex<ExportState>) -> bool {
    state.lock().unwrap().cancelled
}

/// Marks the export cancelled and returns the job to cancel, if it has started.
fn request_cancel(state: &Mutex<ExportState>) -> Option<String> {
    let mut state = state.lock().unwrap();
    if !state.active {
        return None;
    }
    state.cancelled = true;
    state.job_id.clone()
}

fn validate(requests: &[Value]) -> Result<(), EngineError> {
    if requests.is_empty() || requests.len() > 100 {
        return Err(EngineError::internal(
            "Select between one and 100 computers to export logs.",
        ));
    }
    Ok(())
}

/// Starts `logs.export` for the chosen destination and waits for it. `Ok(false)` when the
/// export was cancelled. The dialog has already run: `destination` is the picked path.
async fn run_export(
    engine: &EngineClient,
    state: &Mutex<ExportState>,
    requests: Vec<Value>,
    destination: String,
) -> Result<bool, EngineError> {
    let requests: Vec<Value> = requests
        .into_iter()
        .map(super::runtime_logs::without_nulls)
        .collect();
    let started = engine
        .call(
            "logs.export",
            json!({"requests": requests, "destination": destination}),
        )
        .await?;
    let job_id = started
        .get("jobId")
        .and_then(Value::as_str)
        .ok_or_else(|| EngineError::internal("The engine did not accept the log export."))?
        .to_owned();
    state.lock().unwrap().job_id = Some(job_id.clone());
    // A cancel that arrived while the job was starting.
    if cancelled(state) {
        let _ = engine
            .call("operations.cancel", json!({"jobId": job_id}))
            .await;
    }
    match engine.wait_job(&job_id).await {
        Ok(result) => Ok(result.as_bool().unwrap_or(true) && !cancelled(state)),
        Err(error) if error.is("cancelled") => Ok(false),
        Err(error) => Err(error),
    }
}

/// The export end to end except the dialog: `pick` returns the chosen path, or none.
pub(crate) async fn export_logs(
    engine: &EngineClient,
    state: &Mutex<ExportState>,
    requests: Vec<Value>,
    pick: impl FnOnce() -> Result<Option<String>, String> + Send + 'static,
) -> Result<bool, EngineError> {
    validate(&requests)?;
    begin(state)?;
    let outcome = async {
        let picked = tokio::task::spawn_blocking(pick)
            .await
            .map_err(|_| EngineError::internal("The log export task failed."))?
            .map_err(EngineError::internal)?;
        let Some(destination) = picked else {
            return Ok(false);
        };
        if cancelled(state) {
            return Ok(false);
        }
        run_export(engine, state, requests, destination).await
    }
    .await;
    finish(state);
    outcome
}

pub(crate) async fn cancel_export(
    engine: &EngineClient,
    state: &Mutex<ExportState>,
) -> Result<(), EngineError> {
    if let Some(job_id) = request_cancel(state) {
        engine
            .call("operations.cancel", json!({"jobId": job_id}))
            .await?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn cancel_log_export(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<(), String> {
    require_main(&window)?;
    cancel_export(&engine, &EXPORT).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn export_computer_logs(
    engine: Engine<'_>,
    app: AppHandle,
    window: WebviewWindow,
    requests: Vec<Value>,
) -> Result<bool, String> {
    require_main(&window)?;
    let pick = move || {
        app.dialog()
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
                    .and_then(|path| {
                        path.to_str()
                            .map(str::to_owned)
                            .ok_or_else(|| "The export destination is unavailable.".to_string())
                    })
            })
            .transpose()
    };
    export_logs(&engine, &EXPORT, requests, pick)
        .await
        .map_err(String::from)
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};

    fn requests() -> Vec<Value> {
        vec![json!({"computerId": "c1", "deviceId": null})]
    }

    fn replies(method: &str, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "logs.export" => Ok(json!({"jobId": "export:1"})),
            "operations.job.get" => {
                Ok(json!({"jobId": "export:1", "state": "finished", "outcome": {"result": true}}))
            }
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn the_picked_destination_and_requests_start_a_job_that_is_waited_for() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let state = Mutex::new(ExportState::default());
        let saved = export_logs(&engine, &state, requests(), || {
            Ok(Some("/tmp/silo-logs.jsonl".into()))
        })
        .await
        .unwrap();
        assert!(saved);
        assert_eq!(
            fake.calls("logs.export"),
            vec![
                json!({"requests": [{"computerId": "c1"}], "destination": "/tmp/silo-logs.jsonl"})
            ]
        );
        assert_eq!(
            fake.calls("operations.job.acknowledge"),
            vec![json!({"jobId": "export:1"})]
        );
        // The flow is free again afterwards.
        assert!(begin(&state).is_ok());
    }

    #[tokio::test]
    async fn dismissing_the_dialog_exports_nothing() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let state = Mutex::new(ExportState::default());
        assert!(!export_logs(&engine, &state, requests(), || Ok(None))
            .await
            .unwrap());
        assert!(fake.calls("logs.export").is_empty());
    }

    #[tokio::test]
    async fn the_number_of_computers_and_concurrent_exports_are_limited() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let state = Mutex::new(ExportState::default());
        assert!(export_logs(&engine, &state, vec![], || Ok(None))
            .await
            .is_err());
        begin(&state).unwrap();
        let error = export_logs(&engine, &state, requests(), || Ok(None))
            .await
            .unwrap_err();
        assert_eq!(error.message, "A log export is already in progress.");
    }

    #[tokio::test]
    async fn cancelling_cancels_the_running_job_and_reports_no_file() {
        let fake = FakeEngine::start(|method, params| match method {
            "operations.job.get" => Ok(json!({"jobId": "export:1", "state": "finished",
                "outcome": {"error": {"code": "cancelled", "message": "Cancelled."}}})),
            other => replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let state = Mutex::new(ExportState::default());
        begin(&state).unwrap();
        state.lock().unwrap().job_id = Some("export:1".into());
        cancel_export(&engine, &state).await.unwrap();
        assert_eq!(
            fake.calls("operations.cancel"),
            vec![json!({"jobId": "export:1"})]
        );
        // Nothing to cancel when no export runs.
        finish(&state);
        cancel_export(&engine, &state).await.unwrap();
        assert_eq!(fake.calls("operations.cancel").len(), 1);

        // A job the engine ends as cancelled is not an error.
        assert!(!export_logs(&engine, &state, requests(), || Ok(Some(
            "/tmp/x.jsonl".into()
        )))
        .await
        .unwrap());
    }

    #[tokio::test]
    async fn an_engine_failure_is_typed_and_frees_the_flow() {
        let fake = FakeEngine::start(|method, params| match method {
            "logs.export" => Err(("not_found", "This computer no longer exists.".into())),
            other => replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let state = Mutex::new(ExportState::default());
        let error = export_logs(&engine, &state, requests(), || {
            Ok(Some("/tmp/x.jsonl".into()))
        })
        .await
        .unwrap_err();
        assert_eq!(error.code, "not_found");
        assert!(begin(&state).is_ok());
    }
}
