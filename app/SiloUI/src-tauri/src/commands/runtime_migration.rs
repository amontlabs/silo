//! Tauri adapters for runtime migration state and restart coordination.
use crate::runtime_migration::{self as engine, Controller, MigrationState};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

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

/// Automatic conversion starts only after the export/import controller is installed,
/// so its worker can settle any journal left by the previous process first.
pub(crate) fn start_if_pending(app: &AppHandle) -> Result<(), String> {
    let migration = controller(app)?;
    if migration.snapshot()?.status == "scanning" {
        let _ = retry_runtime_migration_blocking(app.clone())?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn read_runtime_migration_state(app: AppHandle) -> Result<MigrationState, String> {
    engine::migration_snapshot(controller(&app)?).await
}

async fn blocking(
    app: AppHandle,
    work: fn(AppHandle) -> Result<MigrationState, String>,
) -> Result<MigrationState, String> {
    tauri::async_runtime::spawn_blocking(move || work(app))
        .await
        .map_err(|_| "Migration state is unavailable.".to_string())?
}

#[tauri::command]
pub(crate) async fn retry_runtime_migration(app: AppHandle) -> Result<MigrationState, String> {
    blocking(app, retry_runtime_migration_blocking).await
}

fn retry_runtime_migration_blocking(app: AppHandle) -> Result<MigrationState, String> {
    let migration = controller(&app)?;
    let result = migration.prepare_retry()?;
    let _ = app.emit("silo://application-state-changed", ());
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // A panic must still leave a retryable failed state, not "running".
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            convert(&worker_app, &migration)
        }))
        .unwrap_or_else(|_| Err("Migration stopped unexpectedly. Retry the migration.".into()));
        if let Err(error) = outcome {
            let _ = migration.record_failure(error);
            let _ = worker_app.emit("silo://application-state-changed", ());
        }
    });
    Ok(result)
}

fn convert(app: &AppHandle, migration: &Controller) -> Result<(), String> {
    let paths = crate::runtime::migration_runtime_paths(
        &crate::commands::runtime::host(app),
        &migration.staged_storage(),
    )?;
    if migration.needs_recovery()? {
        crate::commands::backup_controller::wait_for_migration_recovery(app)?;
    }
    migration.convert_with_progress(&crate::runtime::ProcessRunner, &paths, &|| {
        let _ = app.emit("silo://application-state-changed", ());
    })?;
    let restart = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        restart.restart();
    });
    Ok(())
}

#[tauri::command]
pub(crate) async fn continue_after_migration_failure(
    app: AppHandle,
) -> Result<MigrationState, String> {
    blocking(app, continue_after_migration_failure_blocking).await
}

fn continue_after_migration_failure_blocking(app: AppHandle) -> Result<MigrationState, String> {
    let result = controller(&app)?.continue_after_failure()?;
    let _ = app.emit("silo://application-state-changed", ());
    let restart = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        restart.restart();
    });
    Ok(result)
}
