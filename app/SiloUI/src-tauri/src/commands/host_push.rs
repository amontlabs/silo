//! Host Push observation, through `push.*`. The UI keeps naming the computer the way it
//! does elsewhere (a display name, or a `silo-remote:` target for a connected device); the
//! operations it gets back carry that same string.

use serde_json::{json, Value};
use silo_api::methods::{
    push::{PushDismissParams, PushStartParams, PushStatusParams},
    requests,
};

use crate::engine_client::{
    forwarding::resolve, params::decode, Engine, EngineClient, EngineError,
};

/// The operation as the UI keys it: under the computer string it asked about.
fn for_ui(mut operation: Value, computer: &str) -> Value {
    if let Some(object) = operation.as_object_mut() {
        object.insert("computer".to_owned(), Value::String(computer.to_owned()));
    }
    operation
}

pub(crate) async fn status(
    engine: &EngineClient,
    computer: &str,
    repository_path: &str,
    operation_id: &str,
) -> Result<Value, EngineError> {
    let (computer_id, device_id) = resolve(engine, computer).await?;
    let operation = engine
        .call(requests::push_status(PushStatusParams {
            computer_id,
            device_id,
            repository_path: repository_path.to_owned(),
            operation_id: operation_id.to_owned(),
        }))
        .await?;
    Ok(if operation.is_null() {
        operation
    } else {
        for_ui(operation, computer)
    })
}

/// Starts the push observation and returns the operation it began as, without waiting for
/// the push to finish: the UI polls the status.
pub(crate) async fn start(
    engine: &EngineClient,
    computer: &str,
    repository_path: &str,
    operation_id: &str,
    target: Value,
) -> Result<Value, EngineError> {
    let (computer_id, device_id) = resolve(engine, computer).await?;
    engine
        .call(requests::push_start(PushStartParams {
            computer_id,
            device_id,
            repository_path: repository_path.to_owned(),
            operation_id: operation_id.to_owned(),
            target: decode(target.clone(), "push target")?,
        }))
        .await?;
    let began = status(engine, computer, repository_path, operation_id).await?;
    Ok(if began.is_null() {
        json!({
            "operationId": operation_id, "computer": computer, "repositoryPath": repository_path,
            "status": "pushing", "commitCount": 0, "target": target,
        })
    } else {
        began
    })
}

pub(crate) async fn dismiss(
    engine: &EngineClient,
    computer: &str,
    repository_path: &str,
) -> Result<(), EngineError> {
    let (computer_id, device_id) = resolve(engine, computer).await?;
    engine
        .call(requests::push_dismiss(PushDismissParams {
            computer_id,
            device_id,
            repository_path: repository_path.to_owned(),
        }))
        .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn dismiss_repository_push(
    engine: Engine<'_>,
    computer: String,
    repository_path: String,
) -> Result<(), EngineError> {
    dismiss(&engine, &computer, &repository_path).await
}

#[tauri::command]
pub(crate) async fn start_repository_push(
    engine: Engine<'_>,
    computer: String,
    repository_path: String,
    operation_id: String,
    target: Value,
) -> Result<Value, EngineError> {
    start(&engine, &computer, &repository_path, &operation_id, target).await
}

#[tauri::command]
pub(crate) async fn repository_push_status(
    engine: Engine<'_>,
    computer: String,
    repository_path: String,
    operation_id: String,
) -> Result<Value, EngineError> {
    status(&engine, &computer, &repository_path, &operation_id).await
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";
    const DEVICE: &str = "00000000-0000-4000-8000-0000000000d1";
    const TARGET: &str = r#"{"repository":"a/b","branch":"main","commit":"abc"}"#;

    fn target() -> Value {
        serde_json::from_str(TARGET).unwrap()
    }

    fn replies(method: MethodName, params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::ComputersList => Ok(json!({
                "linux": {"computers": [{"configuration": {"id": DEV, "name": "dev"}}]},
                "macos": {"computers": []}
            })),
            MethodName::PushStart => Ok(json!({"jobId": "push:op1"})),
            MethodName::PushStatus => Ok(json!({
                "operationId": params["operationId"], "computer": DEV, "repositoryPath": "/workspace/r",
                "status": "pushing", "commitCount": 2, "target": target()
            })),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn starting_a_push_returns_the_operation_keyed_by_the_ui_computer() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let operation = start(&engine, "dev", "/workspace/r", "op1", target())
            .await
            .unwrap();
        assert_eq!(operation["computer"], "dev");
        assert_eq!(operation["status"], "pushing");
        assert_eq!(
            fake.calls(MethodName::PushStart),
            vec![json!({
                "computerId": DEV, "repositoryPath": "/workspace/r", "operationId": "op1", "target": target()
            })]
        );
        // A push job is never awaited: the status is polled.
        assert!(fake.calls(MethodName::OperationsJobGet).is_empty());
    }

    #[tokio::test]
    async fn status_and_dismiss_address_a_connected_device_computer() {
        let fake = FakeEngine::start(|method, params| match method {
            MethodName::PushStatus if params["operationId"] == "gone" => Ok(Value::Null),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let remote = format!("silo-remote:{DEVICE}:{DEV}");
        let operation = status(&engine, &remote, "/workspace/r", "op1")
            .await
            .unwrap();
        assert_eq!(operation["computer"], remote.as_str());
        assert!(status(&engine, &remote, "/workspace/r", "gone")
            .await
            .unwrap()
            .is_null());
        dismiss(&engine, &remote, "/workspace/r").await.unwrap();
        assert_eq!(
            fake.calls(MethodName::PushDismiss),
            vec![json!({"computerId": DEV, "deviceId": DEVICE, "repositoryPath": "/workspace/r"})]
        );
    }

    #[tokio::test]
    async fn a_refused_push_is_the_engines_typed_error() {
        let fake = FakeEngine::start(|method, params| match method {
            MethodName::PushStart => Err((
                "unsupported_remote_operation",
                "Update Silo on the remote device.".into(),
            )),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let error = start(&engine, "dev", "/workspace/r", "op1", target())
            .await
            .unwrap_err();
        assert_eq!(error.code, "unsupported_remote_operation");
        assert!(fake.calls(MethodName::PushStatus).is_empty());
    }
}
