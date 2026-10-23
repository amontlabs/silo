//! Launch-migration state, through `migration.state`. The engine converts the old storage
//! before it serves; it has no retry or continue method, so those two commands report the
//! typed unavailable state.
use crate::engine_client::{Engine, EngineClient, EngineError};
use serde_json::Value;
use silo_api::methods::{common::DeviceParams, requests};

// MARK: Operations

pub(crate) async fn read_state(client: &EngineClient) -> Result<Value, EngineError> {
    client
        .call(requests::migration_state(DeviceParams { device_id: None }))
        .await
}

fn unavailable(what: &str) -> EngineError {
    EngineError::unavailable(format!(
        "{what} is not available from Silo's engine. Relaunch Silo to run the migration again."
    ))
}

#[tauri::command]
pub(crate) async fn read_runtime_migration_state(client: Engine<'_>) -> Result<Value, EngineError> {
    read_state(&client).await
}

/// The engine has no method to retry a migration.
#[tauri::command]
pub(crate) async fn retry_runtime_migration() -> Result<Value, EngineError> {
    Err(unavailable("Retrying the migration"))
}

/// The engine has no method to continue after a failed migration.
#[tauri::command]
pub(crate) async fn continue_after_migration_failure() -> Result<Value, EngineError> {
    Err(unavailable("Continuing after a failed migration"))
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;

    #[tokio::test]
    async fn the_state_is_the_engines_and_errors_stay_typed() {
        let fake = FakeEngine::start(|method, _| match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::MigrationState => {
                Ok(json!({"status": "done", "stage": "ready", "canContinue": false}))
            }
            _ => Ok(Value::Null),
        });
        let (client, _) = connected_client(&fake).await;
        assert_eq!(read_state(&client).await.unwrap()["status"], "done");

        let broken = FakeEngine::start(|method, _| match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::EngineState => Ok(Value::Null),
            _ => Err(("internal", "The migration journal is unreadable.".into())),
        });
        let (client, _) = connected_client(&broken).await;
        let error = read_state(&client).await.unwrap_err();
        assert_eq!(error.code, "internal");
    }

    #[tokio::test]
    async fn retry_and_continue_are_the_typed_unavailable_state() {
        for error in [
            retry_runtime_migration().await.unwrap_err(),
            continue_after_migration_failure().await.unwrap_err(),
        ] {
            assert_eq!(error.code, "unsupported_remote_operation");
            assert!(error.message.contains("not available"));
        }
    }
}
