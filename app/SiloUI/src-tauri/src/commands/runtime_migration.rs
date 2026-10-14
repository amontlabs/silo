//! Launch-migration state, through `migration.state`. The engine converts the old storage
//! before it serves; it has no retry or continue method, so those two commands report the
//! typed unavailable state.
use crate::engine_client::{Engine, EngineClient, EngineError};
use crate::runtime_migration::{self as engine, Controller};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

// The functions up to `read_state` are still called by `startup.rs`, `runtime.rs`,
// `runtime_checkpoints.rs` and `main.rs` on the in-process controller; they leave with
// those callers.

fn controller(app: &AppHandle) -> Result<Arc<Controller>, String> {
    app.try_state::<Arc<Controller>>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "Migration state is unavailable.".into())
}

pub(crate) fn runtime_storage(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    controller(app)?.runtime_storage()
}

pub(crate) fn blocks_operations(app: &AppHandle) -> bool {
    controller(app).map_or(true, |controller| controller.blocks_operations())
}

pub(crate) fn ensure_ready(app: &AppHandle) -> Result<(), String> {
    controller(app)?.ensure_ready()
}

pub(crate) async fn ensure_ready_async(app: &AppHandle) -> Result<(), String> {
    let controller = controller(app)?;
    let state = engine::migration_snapshot(controller.clone()).await?;
    controller.ensure_ready_for(&state)
}

pub(crate) fn install(app: &AppHandle) -> Result<(), String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|_| "Silo application storage is unavailable.")?;
    app.manage(Arc::new(Controller::new(app_data)));
    crate::commands::macos_computers::begin_proxy_bootstrap_migration();
    Ok(())
}

pub(crate) fn migrate_macos_proxy_bootstraps(app: &AppHandle) -> Result<(), String> {
    crate::commands::macos_computers::migrate_proxy_bootstraps_at_launch(app)
}

// MARK: Operations

pub(crate) async fn read_state(client: &EngineClient) -> Result<Value, EngineError> {
    client.call("migration.state", json!({})).await
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

    #[tokio::test]
    async fn the_state_is_the_engines_and_errors_stay_typed() {
        let fake = FakeEngine::start(|method, _| match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "migration.state" => {
                Ok(json!({"status": "done", "stage": "ready", "canContinue": false}))
            }
            _ => Ok(Value::Null),
        });
        let (client, _) = connected_client(&fake).await;
        assert_eq!(read_state(&client).await.unwrap()["status"], "done");

        let broken = FakeEngine::start(|method, _| match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "engine.state" => Ok(Value::Null),
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
