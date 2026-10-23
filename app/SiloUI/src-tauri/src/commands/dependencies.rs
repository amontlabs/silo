//! The dependency report is the engine's: it probes the host `silod` runs on.
use crate::engine_client::{Engine, EngineClient, EngineError};
use serde_json::Value;
use silo_api::methods::{device::DependencyReportParams, requests};

pub(crate) async fn report(
    engine: &EngineClient,
    request_id: String,
) -> Result<Value, EngineError> {
    engine
        .call(requests::device_dependencies(DependencyReportParams {
            request_id,
            device_id: None,
        }))
        .await
}

#[tauri::command]
pub(crate) async fn read_dependencies(
    engine: Engine<'_>,
    request_id: String,
) -> Result<Value, String> {
    report(&engine, request_id).await.map_err(String::from)
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;

    #[tokio::test]
    async fn the_report_is_requested_by_id() {
        let fake = FakeEngine::start(|method, _| match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::DeviceDependencies => Ok(json!({"requestId": "r1", "checks": []})),
            _ => Ok(Value::Null),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            report(&engine, "r1".into()).await.unwrap()["requestId"],
            "r1"
        );
        assert_eq!(
            fake.calls(MethodName::DeviceDependencies),
            vec![json!({"requestId": "r1"})]
        );
    }

    #[tokio::test]
    async fn a_rejected_request_id_is_the_engines_error() {
        let fake = FakeEngine::start(|method, _| match method {
            MethodName::DeviceDependencies => Err(("internal", "Invalid request ID.".into())),
            _ => Ok(json!({"subscription": "sub"})),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            report(&engine, "".into()).await.unwrap_err().message,
            "Invalid request ID."
        );
    }
}
