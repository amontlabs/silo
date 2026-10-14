//! Tauri adapters for checkpoint commands.
use crate::{runtime, runtime::checkpoints};
use serde_json::Value;
use tauri::AppHandle;

struct AppForkAssignments;

impl checkpoints::ForkAssignments for AppForkAssignments {
    fn copy_github(&self, source: &str, target: &str) -> Result<(), String> {
        crate::github::fork_assignment(source, target)
    }

    fn forget_github(&self, target: &str) -> Result<(), String> {
        crate::github::forget_fork_assignment(target)
    }

    fn copy_secrets(&self, source: &str, target: &str) -> Result<(), String> {
        crate::secrets::fork_assignments(source, target)
    }

    fn forget_secrets(&self, target: &str) -> Result<(), String> {
        crate::secrets::computer_removed(target)
    }
}

fn publish_application_state_changed() {
    if let Some(context) = silo_engine::context::current() {
        context.publish(
            silo_engine::context::Event::new("silo://application-state-changed", Value::Null),
            silo_engine::context::Audience::All,
        );
    }
}

fn refreshed_state(
    app: &AppHandle,
    paths: &runtime::RuntimePaths,
) -> Result<runtime::ApplicationSource, String> {
    let state = runtime::application_state_response(&crate::commands::runtime::host(app), paths)
        .map_err(|error| error.to_string())?;
    publish_application_state_changed();
    Ok(state)
}

#[tauri::command]
pub(crate) async fn create_checkpoint(
    app: AppHandle,
    computer_id: String,
    name: String,
) -> Result<runtime::ApplicationSource, String> {
    crate::commands::runtime_migration::ensure_ready_async(&app).await?;
    let worker_app = app.clone();
    tokio::task::spawn_blocking(move || {
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&worker_app))?;
        checkpoints::create(&paths, &computer_id, &name)?;
        refreshed_state(&worker_app, &paths)
    })
    .await
    .map_err(|_| {
        "Silo could not finish creating the checkpoint. Refresh its history before trying again."
            .to_string()
    })?
}

#[tauri::command]
pub(crate) async fn fork_checkpoint(
    app: AppHandle,
    computer_id: String,
    checkpoint_id: Option<String>,
    new_name: String,
) -> Result<runtime::ApplicationSource, String> {
    crate::commands::runtime_migration::ensure_ready_async(&app).await?;
    let worker_app = app.clone();
    tokio::task::spawn_blocking(move || {
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&worker_app))?;
        checkpoints::fork(
            &paths,
            &AppForkAssignments,
            &computer_id,
            checkpoint_id.as_deref(),
            &new_name,
            &|| crate::commands::macos_computers::names(&worker_app),
        )?;
        refreshed_state(&worker_app, &paths)
    })
    .await
    .map_err(|_| {
        "Silo could not finish creating the fork. Refresh the computer list before trying again."
            .to_string()
    })?
}

#[tauri::command]
pub(crate) async fn restore_checkpoint(
    app: AppHandle,
    computer_id: String,
    checkpoint_id: String,
) -> Result<runtime::ApplicationSource, String> {
    crate::commands::runtime_migration::ensure_ready_async(&app).await?;
    let worker_app = app.clone();
    tokio::task::spawn_blocking(move || {
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&worker_app))?;
        checkpoints::restore(&paths, &computer_id, &checkpoint_id)?;
        refreshed_state(&worker_app, &paths)
    })
    .await
    .map_err(|_| {
        "Silo could not finish Restore. Open the Checkpoints tab to review recovery and retry Restore."
            .to_string()
    })?
}

#[tauri::command]
pub(crate) async fn abandon_restore(
    app: AppHandle,
    computer_id: String,
) -> Result<runtime::ApplicationSource, String> {
    crate::commands::runtime_migration::ensure_ready_async(&app).await?;
    let worker_app = app.clone();
    tokio::task::spawn_blocking(move || {
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&worker_app))?;
        checkpoints::abandon_restore(&paths, &computer_id)?;
        refreshed_state(&worker_app, &paths)
    })
    .await
    .map_err(|_| {
        "Silo could not abandon Restore. Open the Checkpoints tab to review recovery and retry."
            .to_string()
    })?
}

#[tauri::command]
pub(crate) async fn delete_checkpoint(
    app: AppHandle,
    computer_id: String,
    checkpoint_id: String,
) -> Result<runtime::ApplicationSource, String> {
    crate::commands::runtime_migration::ensure_ready_async(&app).await?;
    let worker_app = app.clone();
    tokio::task::spawn_blocking(move || {
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&worker_app))?;
        checkpoints::delete(&paths, &computer_id, &checkpoint_id)?;
        refreshed_state(&worker_app, &paths)
    })
    .await
    .map_err(|_| {
        "Silo could not finish deleting the checkpoint. Refresh its history before trying again."
            .to_string()
    })?
}

#[tauri::command]
pub(crate) async fn read_checkpoint_usage(
    app: AppHandle,
    computer_id: String,
) -> Result<checkpoints::CheckpointUsage, String> {
    tokio::task::spawn_blocking(move || {
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&app))?;
        checkpoints::read_usage(&paths, &computer_id)
    })
    .await
    .map_err(|_| {
        "Silo could not read checkpoint disk usage. Refresh the Storage tab and retry.".to_string()
    })?
}
