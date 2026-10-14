//! Workspace storage commands: `silod` reads and reclaims the storage.
use crate::engine_client::{Engine, EngineClient, EngineError};
use serde_json::{json, Value};

pub(crate) async fn read(engine: &EngineClient, computer_id: &str) -> Result<Value, EngineError> {
    engine
        .call("storage.read", json!({"computerId": computer_id}))
        .await
}

/// Reclaims, waits for the job, and returns the storage state afterwards.
pub(crate) async fn reclaim(
    engine: &EngineClient,
    computer_id: &str,
) -> Result<Value, EngineError> {
    engine
        .run_job("storage.reclaim", json!({"computerId": computer_id}))
        .await?;
    read(engine, computer_id).await
}

#[tauri::command]
pub async fn read_workspace_storage(
    engine: Engine<'_>,
    computer_id: String,
) -> Result<Value, String> {
    read(&engine, &computer_id).await.map_err(String::from)
}

#[tauri::command]
pub async fn reclaim_workspace_storage(
    engine: Engine<'_>,
    computer_id: String,
) -> Result<Value, String> {
    reclaim(&engine, &computer_id).await.map_err(String::from)
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};

    fn replies(method: &str, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "storage.read" => Ok(json!({"history": [], "checkpointCount": 2})),
            "storage.reclaim" => Ok(json!({"jobId": "job:1"})),
            "operations.job.get" => {
                Ok(json!({"jobId": "job:1", "state": "finished", "outcome": {"result": null}}))
            }
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn read_and_reclaim_forward_and_reclaim_returns_the_new_state() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(read(&engine, "c1").await.unwrap()["checkpointCount"], 2);
        assert_eq!(reclaim(&engine, "c1").await.unwrap()["checkpointCount"], 2);
        assert_eq!(
            fake.calls("storage.reclaim"),
            vec![json!({"computerId": "c1"})]
        );
        assert_eq!(fake.calls("storage.read").len(), 2);
    }

    #[tokio::test]
    async fn a_failed_reclaim_reports_the_job_error() {
        let fake = FakeEngine::start(|method, params| match method {
            "operations.job.get" => Ok(json!({"jobId": "job:1", "state": "finished",
                "outcome": {"error": {"code": "busy", "message": "The computer is busy."}}})),
            other => replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(reclaim(&engine, "c1").await.unwrap_err().code, "busy");
        assert!(fake.calls("storage.read").is_empty());
    }
}
