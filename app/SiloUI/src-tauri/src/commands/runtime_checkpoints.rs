//! Tauri adapters for checkpoint commands.
use crate::{runtime, runtime::checkpoints};
use serde_json::Value;
use tauri::AppHandle;

fn spawn_checkpoint_blocking<F, R>(work: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    // Connections runs remote checkpoint handlers under a StartCondition. Carry it
    // across this host adapter so queued work can expire before gate admission.
    runtime::operation_gate::spawn_blocking(work)
}

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
    Ok(state)
}

fn mutate_then_refresh<T>(
    mutation: impl FnOnce() -> Result<(), String>,
    refresh: impl FnOnce() -> Result<T, String>,
    publish: impl FnOnce(),
) -> Result<T, String> {
    let mutation_result = mutation();
    let refreshed = refresh();
    publish();
    mutation_result?;
    refreshed
}

#[tauri::command]
pub(crate) async fn create_checkpoint(
    app: AppHandle,
    computer_id: String,
    name: String,
) -> Result<runtime::ApplicationSource, String> {
    crate::commands::runtime_migration::ensure_ready_async(&app).await?;
    let worker_app = app.clone();
    spawn_checkpoint_blocking(move || {
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&worker_app))?;
        mutate_then_refresh(
            || checkpoints::create(&paths, &computer_id, &name),
            || refreshed_state(&worker_app, &paths),
            publish_application_state_changed,
        )
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
    spawn_checkpoint_blocking(move || {
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&worker_app))?;
        mutate_then_refresh(
            || {
                checkpoints::fork(
                    &paths,
                    &AppForkAssignments,
                    &computer_id,
                    checkpoint_id.as_deref(),
                    &new_name,
                    &|| crate::commands::macos_computers::names(&worker_app),
                )
            },
            || refreshed_state(&worker_app, &paths),
            publish_application_state_changed,
        )
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
    spawn_checkpoint_blocking(move || {
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&worker_app))?;
        mutate_then_refresh(
            || checkpoints::restore(&paths, &computer_id, &checkpoint_id),
            || refreshed_state(&worker_app, &paths),
            publish_application_state_changed,
        )
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
    spawn_checkpoint_blocking(move || {
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&worker_app))?;
        mutate_then_refresh(
            || checkpoints::abandon_restore(&paths, &computer_id),
            || refreshed_state(&worker_app, &paths),
            publish_application_state_changed,
        )
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
    spawn_checkpoint_blocking(move || {
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&worker_app))?;
        mutate_then_refresh(
            || checkpoints::delete(&paths, &computer_id, &checkpoint_id),
            || refreshed_state(&worker_app, &paths),
            publish_application_state_changed,
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::operation_gate::{GateError, OperationGate, OperationKind, StartCondition};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn failed_checkpoint_mutation_still_publishes_after_refresh() {
        let calls = std::cell::RefCell::new(Vec::new());
        let result = mutate_then_refresh(
            || {
                calls.borrow_mut().push("mutation");
                Err("mutation failed".into())
            },
            || {
                calls.borrow_mut().push("refresh");
                Ok("refreshed")
            },
            || calls.borrow_mut().push("publish"),
        );

        assert_eq!(result.unwrap_err(), "mutation failed");
        assert_eq!(*calls.borrow(), ["mutation", "refresh", "publish"]);
    }

    #[test]
    fn failed_checkpoint_refresh_still_publishes_after_mutation() {
        let calls = std::cell::RefCell::new(Vec::new());
        let result = mutate_then_refresh(
            || {
                calls.borrow_mut().push("mutation");
                Ok(())
            },
            || {
                calls.borrow_mut().push("refresh");
                Err::<(), _>("refresh failed".into())
            },
            || calls.borrow_mut().push("publish"),
        );

        assert_eq!(result.unwrap_err(), "refresh failed");
        assert_eq!(*calls.borrow(), ["mutation", "refresh", "publish"]);
    }

    #[test]
    fn queued_remote_checkpoint_expires_before_gate_admission() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let gate = Arc::new(OperationGate::new());
        let busy = gate
            .kind(OperationKind::Lifecycle)
            .computer("computer-id", "computer", "Running another operation")
            .unwrap();
        let wanted = Arc::new(AtomicBool::new(true));
        let condition = StartCondition::new({
            let wanted = wanted.clone();
            move || wanted.load(Ordering::SeqCst)
        });
        let checkpoint = {
            let gate = gate.clone();
            runtime.block_on(async {
                StartCondition::scope(Some(condition.clone()), || {
                    spawn_checkpoint_blocking(move || {
                        gate.kind(OperationKind::CheckpointCapture)
                            .computer("computer-id", "computer", "Creating checkpoint")
                            .map(drop)
                    })
                })
            })
        };

        let deadline = Instant::now() + Duration::from_secs(5);
        while gate.snapshot().waiting.is_empty() {
            assert!(Instant::now() < deadline, "checkpoint did not queue");
            std::thread::sleep(Duration::from_millis(2));
        }
        wanted.store(false, Ordering::SeqCst);
        drop(busy);

        assert_eq!(
            runtime.block_on(checkpoint).unwrap(),
            Err(GateError::Abandoned)
        );
        assert!(condition.expired());
        assert!(!condition.started());
        assert!(gate.snapshot().waiting.is_empty());
    }
}
