//! Tauri adapters for checkpoint commands: each starts a `checkpoints.*` job in `silod`,
//! waits for its outcome and returns the Linux application state the UI renders.
use crate::engine_client::{Engine, EngineClient, EngineError};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

/// Runs one checkpoint job and returns the resulting Linux state.
pub(crate) async fn checkpoint_job(
    engine: &EngineClient,
    method: &str,
    params: Value,
) -> Result<Value, EngineError> {
    engine.run_job(method, params).await?;
    engine.application_source(false).await
}

pub(crate) async fn create(
    engine: &EngineClient,
    computer_id: &str,
    name: &str,
) -> Result<Value, EngineError> {
    checkpoint_job(
        engine,
        "checkpoints.create",
        json!({"computerId": computer_id, "name": name}),
    )
    .await
}

pub(crate) async fn fork(
    engine: &EngineClient,
    computer_id: &str,
    checkpoint_id: Option<&str>,
    new_name: &str,
) -> Result<Value, EngineError> {
    checkpoint_job(
        engine,
        "checkpoints.fork",
        json!({"computerId": computer_id, "checkpointId": checkpoint_id, "newName": new_name}),
    )
    .await
}

pub(crate) async fn restore(
    engine: &EngineClient,
    computer_id: &str,
    checkpoint_id: &str,
) -> Result<Value, EngineError> {
    checkpoint_job(
        engine,
        "checkpoints.restore",
        json!({"computerId": computer_id, "checkpointId": checkpoint_id}),
    )
    .await
}

pub(crate) async fn abandon(
    engine: &EngineClient,
    computer_id: &str,
) -> Result<Value, EngineError> {
    checkpoint_job(
        engine,
        "checkpoints.abandonRestore",
        json!({"computerId": computer_id}),
    )
    .await
}

pub(crate) async fn delete(
    engine: &EngineClient,
    computer_id: &str,
    checkpoint_id: &str,
) -> Result<Value, EngineError> {
    checkpoint_job(
        engine,
        "checkpoints.delete",
        json!({"computerId": computer_id, "checkpointId": checkpoint_id}),
    )
    .await
}

pub(crate) async fn usage(engine: &EngineClient, computer_id: &str) -> Result<Value, EngineError> {
    engine
        .call("checkpoints.usage", json!({"computerId": computer_id}))
        .await
}

// The Connections owner adapter (`commands/remote.rs`) still calls the first three by
// `AppHandle`; they take the engine from the app's managed state and go with that adapter.

#[tauri::command]
pub(crate) async fn create_checkpoint(
    app: AppHandle,
    computer_id: String,
    name: String,
) -> Result<Value, String> {
    let engine = app.state::<std::sync::Arc<EngineClient>>();
    create(&engine, &computer_id, &name)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn fork_checkpoint(
    app: AppHandle,
    computer_id: String,
    checkpoint_id: Option<String>,
    new_name: String,
) -> Result<Value, String> {
    let engine = app.state::<std::sync::Arc<EngineClient>>();
    fork(&engine, &computer_id, checkpoint_id.as_deref(), &new_name)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn restore_checkpoint(
    app: AppHandle,
    computer_id: String,
    checkpoint_id: String,
) -> Result<Value, String> {
    let engine = app.state::<std::sync::Arc<EngineClient>>();
    restore(&engine, &computer_id, &checkpoint_id)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn abandon_restore(
    engine: Engine<'_>,
    computer_id: String,
) -> Result<Value, String> {
    abandon(&engine, &computer_id).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn delete_checkpoint(
    engine: Engine<'_>,
    computer_id: String,
    checkpoint_id: String,
) -> Result<Value, String> {
    delete(&engine, &computer_id, &checkpoint_id)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn read_checkpoint_usage(
    engine: Engine<'_>,
    computer_id: String,
) -> Result<Value, String> {
    usage(&engine, &computer_id).await.map_err(String::from)
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";

    fn replies(method: &str, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "checkpoints.create"
            | "checkpoints.fork"
            | "checkpoints.restore"
            | "checkpoints.delete"
            | "checkpoints.abandonRestore" => Ok(json!({"jobId": "job:1"})),
            "operations.job.get" => {
                Ok(json!({"jobId": "job:1", "state": "finished", "outcome": {"result": null}}))
            }
            "computers.list" => {
                Ok(json!({"linux": {"computers": [{"configuration": {"id": DEV}}]}, "macos": {}}))
            }
            "checkpoints.usage" => Ok(json!({"totalBytes": 5, "checkpoints": []})),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn each_command_runs_its_job_and_returns_the_linux_state() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        for state in [
            create(&engine, DEV, "before").await.unwrap(),
            fork(&engine, DEV, None, "copy").await.unwrap(),
            restore(&engine, DEV, "cp1").await.unwrap(),
            abandon(&engine, DEV).await.unwrap(),
            delete(&engine, DEV, "cp1").await.unwrap(),
        ] {
            assert_eq!(state["computers"][0]["configuration"]["id"], DEV);
        }
        assert_eq!(
            fake.calls("checkpoints.create"),
            vec![json!({"computerId": DEV, "name": "before"})]
        );
        assert_eq!(
            fake.calls("checkpoints.fork"),
            vec![json!({"computerId": DEV, "checkpointId": null, "newName": "copy"})]
        );
        assert_eq!(
            fake.calls("checkpoints.restore"),
            vec![json!({"computerId": DEV, "checkpointId": "cp1"})]
        );
        assert_eq!(
            fake.calls("checkpoints.abandonRestore"),
            vec![json!({"computerId": DEV})]
        );
        assert_eq!(
            fake.calls("checkpoints.delete"),
            vec![json!({"computerId": DEV, "checkpointId": "cp1"})]
        );
        assert_eq!(fake.calls("operations.job.acknowledge").len(), 5);
    }

    #[tokio::test]
    async fn usage_is_read_through_the_query() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(usage(&engine, DEV).await.unwrap()["totalBytes"], 5);
        assert_eq!(
            fake.calls("checkpoints.usage"),
            vec![json!({"computerId": DEV})]
        );
    }

    #[tokio::test]
    async fn a_failed_job_is_the_commands_error_and_no_state_is_read() {
        let fake = FakeEngine::start(|method, params| match method {
            "operations.job.get" => Ok(json!({"jobId": "job:1", "state": "finished",
                "outcome": {"error": {"code": "busy", "message": "A checkpoint operation is in progress."}}})),
            other => replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let error = create(&engine, DEV, "x").await.unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.message.as_str()),
            ("busy", "A checkpoint operation is in progress.")
        );
        assert!(fake.calls("computers.list").is_empty());
    }

    #[tokio::test]
    async fn a_refusal_at_start_is_typed() {
        let fake = FakeEngine::start(|method, params| match method {
            "checkpoints.restore" => Err(("not_found", "This computer no longer exists.".into())),
            other => replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            restore(&engine, DEV, "cp").await.unwrap_err().code,
            "not_found"
        );
    }
}
