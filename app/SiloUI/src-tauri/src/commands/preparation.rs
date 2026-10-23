//! Image and computer-use preparation status: read from `silod`, which prepares them.
use crate::engine_client::params::device;
use crate::engine_client::{Engine, EngineClient, EngineError};
use serde_json::Value;
use silo_api::methods::requests;

pub(crate) async fn status(engine: &EngineClient) -> Result<Value, EngineError> {
    engine
        .call(requests::preparation_status(device(None)))
        .await
}

pub(crate) async fn retry(engine: &EngineClient) -> Result<Value, EngineError> {
    engine.call(requests::preparation_retry(device(None))).await
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
    use serde_json::json;
    use silo_api::methods::MethodName;

    #[tokio::test]
    async fn status_and_retry_forward_and_errors_are_typed() {
        let fake = FakeEngine::start(|method, _| match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::PreparationStatus => Ok(json!({"image": {"state": "ready"}})),
            MethodName::PreparationRetry => Err(("busy", "Already preparing.".into())),
            _ => Ok(Value::Null),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(status(&engine).await.unwrap()["image"]["state"], "ready");
        assert_eq!(retry(&engine).await.unwrap_err().code, "busy");
        assert_eq!(fake.calls(MethodName::PreparationRetry), vec![json!({})]);
    }
}
