//! Workspace storage commands: `silod` reads and reclaims the storage.
use crate::engine_client::{Engine, EngineClient, EngineError};
use serde_json::Value;
use silo_api::methods::{requests, storage::StorageParams};

pub(crate) async fn read(engine: &EngineClient, computer_id: &str) -> Result<Value, EngineError> {
    engine
        .call(requests::storage_read(StorageParams {
            computer_id: computer_id.to_owned(),
            device_id: None,
        }))
        .await
}

/// Reclaims, waits for the job, and returns the storage state afterwards.
pub(crate) async fn reclaim(
    engine: &EngineClient,
    computer_id: &str,
) -> Result<Value, EngineError> {
    engine
        .run_job(requests::storage_reclaim(StorageParams {
            computer_id: computer_id.to_owned(),
            device_id: None,
        }))
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
    use serde_json::json;
    use silo_api::methods::MethodName;

    fn replies(method: MethodName, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::StorageRead => Ok(json!({"history": [], "checkpointCount": 2})),
            MethodName::StorageReclaim => Ok(json!({"jobId": "job:1"})),
            MethodName::OperationsJobGet => {
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
            fake.calls(MethodName::StorageReclaim),
            vec![json!({"computerId": "c1"})]
        );
        assert_eq!(fake.calls(MethodName::StorageRead).len(), 2);
    }

    #[tokio::test]
    async fn a_failed_reclaim_reports_the_job_error() {
        let fake = FakeEngine::start(|method, params| match method {
            MethodName::OperationsJobGet => Ok(json!({"jobId": "job:1", "state": "finished",
                "outcome": {"error": {"code": "busy", "message": "The computer is busy."}}})),
            other => replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(reclaim(&engine, "c1").await.unwrap_err().code, "busy");
        assert!(fake.calls(MethodName::StorageRead).is_empty());
    }
}
