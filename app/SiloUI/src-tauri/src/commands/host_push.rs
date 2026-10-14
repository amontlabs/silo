//! Host Push observation, through `push.*`. The UI keeps naming the computer the way it
//! does elsewhere (a display name, or a `silo-remote:` target for a connected device); the
//! operations it gets back carry that same string.

use serde_json::{json, Value};

use crate::engine_client::{
    forwarding::{resolve, with_device},
    Engine, EngineClient, EngineError,
};

fn repository_params(
    computer_id: &str,
    device_id: Option<&str>,
    repository_path: &str,
    operation_id: Option<&str>,
) -> Value {
    let mut params = json!({"computerId": computer_id, "repositoryPath": repository_path});
    if let Some(operation_id) = operation_id {
        params["operationId"] = Value::String(operation_id.to_owned());
    }
    with_device(params, device_id)
}

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
        .call(
            "push.status",
            repository_params(
                &computer_id,
                device_id.as_deref(),
                repository_path,
                Some(operation_id),
            ),
        )
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
    let mut params = repository_params(
        &computer_id,
        device_id.as_deref(),
        repository_path,
        Some(operation_id),
    );
    params["target"] = target.clone();
    engine.call("push.start", params).await?;
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
        .call(
            "push.dismiss",
            repository_params(&computer_id, device_id.as_deref(), repository_path, None),
        )
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

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";
    const DEVICE: &str = "00000000-0000-4000-8000-0000000000d1";
    const TARGET: &str = r#"{"repository":"a/b","branch":"main","commit":"abc"}"#;

    fn target() -> Value {
        serde_json::from_str(TARGET).unwrap()
    }

    fn replies(method: &str, params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "computers.list" => Ok(json!({
                "linux": {"computers": [{"configuration": {"id": DEV, "name": "dev"}}]},
                "macos": {"computers": []}
            })),
            "push.start" => Ok(json!({"jobId": "push:op1"})),
            "push.status" => Ok(json!({
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
            fake.calls("push.start"),
            vec![json!({
                "computerId": DEV, "repositoryPath": "/workspace/r", "operationId": "op1", "target": target()
            })]
        );
        // A push job is never awaited: the status is polled.
        assert!(fake.calls("operations.job.get").is_empty());
    }

    #[tokio::test]
    async fn status_and_dismiss_address_a_connected_device_computer() {
        let fake = FakeEngine::start(|method, params| match method {
            "push.status" if params["operationId"] == "gone" => Ok(Value::Null),
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
            fake.calls("push.dismiss"),
            vec![json!({"computerId": DEV, "deviceId": DEVICE, "repositoryPath": "/workspace/r"})]
        );
    }

    #[tokio::test]
    async fn a_refused_push_is_the_engines_typed_error() {
        let fake = FakeEngine::start(|method, params| match method {
            "push.start" => Err((
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
        assert!(fake.calls("push.status").is_empty());
    }
}
