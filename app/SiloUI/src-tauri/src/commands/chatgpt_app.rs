//! The ChatGPT app preparation: the engine downloads it; these commands read its status and
//! ask for a retry through `silod`.
use crate::engine_client::{Engine, EngineClient, EngineError};
use crate::{chatgpt_app, runtime};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};
use tauri::{AppHandle, Emitter, Manager};

const STATUS_EVENT: &str = "chatgpt-app-status";

fn device_params(device: Option<String>) -> Value {
    match device {
        Some(device) => json!({"deviceId": device}),
        None => json!({}),
    }
}

pub(crate) async fn status(
    engine: &EngineClient,
    device: Option<String>,
) -> Result<Value, EngineError> {
    engine
        .call("preparation.chatgpt.status", device_params(device))
        .await
}

pub(crate) async fn retry(
    engine: &EngineClient,
    device: Option<String>,
) -> Result<Value, EngineError> {
    engine
        .call("preparation.chatgpt.retry", device_params(device))
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

// MARK: Left for the Connections owner adapter
//
// `commands/remote.rs` (`TauriLocalAccess`) still answers a remote device's ChatGPT status
// and retry from this process. These go with that adapter; nothing else uses them.

/// The ChatGPT app folder of this channel: `<app data>/chatgpt`.
pub(crate) fn storage_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("chatgpt"))
        .map_err(|error| format!("Silo could not locate its application storage: {error}"))
}

fn status_publisher(app: AppHandle) -> chatgpt_app::StatusPublisher {
    Arc::new(move |status| {
        if let Ok(value) = serde_json::to_value(status) {
            let _ = app.emit(STATUS_EVENT, chatgpt_app::event_payload(value, None));
        }
    })
}

pub(crate) fn local_status(app: &AppHandle) -> Result<chatgpt_app::Status, String> {
    chatgpt_app::local_status(&storage_root(app)?)
}

/// Wakes a waiting worker or starts a new attempt after a retryable failure.
pub(crate) fn retry_now(app: &AppHandle) -> Result<chatgpt_app::Status, String> {
    let root = storage_root(app)?;
    let app = app.clone();
    let idle_root = root.clone();
    chatgpt_app::retry_now(
        &root,
        status_publisher(app.clone()),
        Arc::new(move || {
            chatgpt_app::collect_unused(
                idle_root.clone(),
                Arc::new({
                    let app = app.clone();
                    move || {
                        runtime::update_recovery::running_names(&crate::commands::runtime::host(
                            &app,
                        ))
                        .is_ok_and(|names| names.is_empty())
                    }
                }),
            );
        }),
    )
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};

    #[tokio::test]
    async fn status_and_retry_address_the_device_only_when_one_is_named() {
        let fake = FakeEngine::start(|method, _| match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "preparation.chatgpt.status" => Ok(json!({"state": "idle"})),
            "preparation.chatgpt.retry" => Err((
                "unsupported_remote_operation",
                "That device is not reachable.".into(),
            )),
            _ => Ok(Value::Null),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(status(&engine, None).await.unwrap()["state"], "idle");
        status(&engine, Some("d1".into())).await.unwrap();
        assert_eq!(
            fake.calls("preparation.chatgpt.status"),
            vec![json!({}), json!({"deviceId": "d1"})]
        );
        let error = retry(&engine, Some("d1".into())).await.unwrap_err();
        assert_eq!(error.code, "unsupported_remote_operation");
        assert_eq!(
            fake.calls("preparation.chatgpt.retry"),
            vec![json!({"deviceId": "d1"})]
        );
    }
}
