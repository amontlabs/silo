//! Image and computer-use preparation status: read from `silod`, which prepares them.
use crate::engine_client::{Engine, EngineClient, EngineError};
use serde_json::{json, Value};

pub(crate) async fn status(engine: &EngineClient) -> Result<Value, EngineError> {
    engine.call("preparation.status", json!({})).await
}

pub(crate) async fn retry(engine: &EngineClient) -> Result<Value, EngineError> {
    engine.call("preparation.retry", json!({})).await
}

#[tauri::command]
pub(crate) async fn read_preparation_status(engine: Engine<'_>) -> Result<Value, EngineError> {
    status(&engine).await
}

#[tauri::command]
pub(crate) async fn retry_preparation(engine: Engine<'_>) -> Result<Value, EngineError> {
    retry(&engine).await
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};

    #[tokio::test]
    async fn status_and_retry_forward_and_errors_are_typed() {
        let fake = FakeEngine::start(|method, _| match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "preparation.status" => Ok(json!({"image": {"state": "ready"}})),
            "preparation.retry" => Err(("busy", "Already preparing.".into())),
            _ => Ok(Value::Null),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(status(&engine).await.unwrap()["image"]["state"], "ready");
        assert_eq!(retry(&engine).await.unwrap_err().code, "busy");
        assert_eq!(fake.calls("preparation.retry"), vec![json!({})]);
    }
}
