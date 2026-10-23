//! The ChatGPT app preparation: the engine downloads it; these commands read its status and
//! ask for a retry through `silod`.
use crate::engine_client::{Engine, EngineClient, EngineError};
use serde_json::Value;
use silo_api::methods::{common::DeviceParams, requests};

fn device_params(device: Option<String>) -> DeviceParams {
    DeviceParams { device_id: device }
}

pub(crate) async fn status(
    engine: &EngineClient,
    device: Option<String>,
) -> Result<Value, EngineError> {
    engine
        .call(requests::preparation_chatgpt_status(device_params(device)))
        .await
}

pub(crate) async fn retry(
    engine: &EngineClient,
    device: Option<String>,
) -> Result<Value, EngineError> {
    engine
        .call(requests::preparation_chatgpt_retry(device_params(device)))
        .await
}

/// The ChatGPT app status of this device, or of the remote device `device` (its device id).
#[tauri::command]
pub(crate) async fn chatgpt_app_status(
    engine: Engine<'_>,
    device: Option<String>,
) -> Result<Value, String> {
    status(&engine, device).await.map_err(String::from)
}

/// Asks a device to try the download again now. Progress follows from the status reads.
#[tauri::command]
pub(crate) async fn chatgpt_app_retry(
    engine: Engine<'_>,
    device: Option<String>,
) -> Result<Value, String> {
    retry(&engine, device).await.map_err(String::from)
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;

    #[tokio::test]
    async fn status_and_retry_address_the_device_only_when_one_is_named() {
        let fake = FakeEngine::start(|method, _| match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::PreparationChatgptStatus => Ok(json!({"state": "idle"})),
            MethodName::PreparationChatgptRetry => Err((
                "unsupported_remote_operation",
                "That device is not reachable.".into(),
            )),
            _ => Ok(Value::Null),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(status(&engine, None).await.unwrap()["state"], "idle");
        status(&engine, Some("d1".into())).await.unwrap();
        assert_eq!(
            fake.calls(MethodName::PreparationChatgptStatus),
            vec![json!({}), json!({"deviceId": "d1"})]
        );
        let error = retry(&engine, Some("d1".into())).await.unwrap_err();
        assert_eq!(error.code, "unsupported_remote_operation");
        assert_eq!(
            fake.calls(MethodName::PreparationChatgptRetry),
            vec![json!({"deviceId": "d1"})]
        );
    }
}
