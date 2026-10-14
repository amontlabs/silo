//! Tauri adapters for the runtime engine and compatibility services used by
//! other domains while they are extracted.
use crate::runtime;
use crate::runtime::{RuntimeBundleKind, RuntimeHost, RuntimeLifecycleOutcome, RuntimePackageKind};
use silo_engine::context::{self, EngineContext};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{utils::config::BundleType, AppHandle};

impl RuntimeHost for AppHandle {
    fn engine_context(&self) -> std::sync::Arc<dyn EngineContext> {
        context::current().expect("the engine context is installed during app setup")
    }

    fn runtime_storage(&self) -> Result<PathBuf, String> {
        crate::runtime_migration::runtime_storage(self)
    }

    fn blocks_operations(&self) -> bool {
        crate::runtime_migration::blocks_operations(self)
    }

    fn bundled_tools_directory(&self) -> Result<PathBuf, String> {
        crate::bundled_tools::directory(self)
    }

    fn ensure_runtime_ready(&self) -> Result<(), String> {
        crate::runtime_migration::ensure_ready(self)
    }

    fn runtime_package_kind(&self) -> RuntimePackageKind {
        tauri::utils::platform::bundle_type().runtime_package_kind()
    }

    fn schedule_secret_revocations(&self) {
        crate::secrets::schedule_revocations(self);
    }

    fn repository_push_operations(&self, local: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        crate::host_push_operations::merge(self, local.clone()).unwrap_or(local)
    }

    fn github_snapshot(&self) -> Result<serde_json::Value, String> {
        crate::github::snapshot(self)
    }

    fn open_terminal(&self, name: &str) -> Result<(), String> {
        crate::terminal::open(self, name)
    }

    fn open_editor(&self, name: &str, path: Option<&str>) -> Result<(), String> {
        crate::editor::open(self, name, path)
    }

    fn macos_computer_names(&self) -> Vec<String> {
        crate::macos_computers::names(self)
    }

    fn macos_log_target(&self, computer_id: &str) -> Option<(String, PathBuf)> {
        crate::macos_computers::log_target(self, computer_id)
    }

    fn notify(&self, notice: crate::notifications::Notice) {
        crate::notifications::notify_native(self, notice);
    }

    fn clear_computer_notices(&self, computer_ids: &[String]) {
        for computer_id in computer_ids {
            crate::notifications::clear_computer(self, computer_id);
        }
    }

    fn stop_macos_computers(&self, deadline: Option<std::time::Instant>) -> Result<(), String> {
        crate::macos_computers::stop_all(self, deadline)
    }

    fn lifecycle_notice(
        &self,
        action: &str,
        name: &str,
        computer_id: Option<String>,
        elapsed: std::time::Duration,
        outcome: RuntimeLifecycleOutcome,
    ) {
        let computer = computer_id.map(|id| crate::notifications::NoticeComputer {
            id,
            name: name.to_owned(),
        });
        let outcome = match &outcome {
            RuntimeLifecycleOutcome::Cancelled => crate::notifications::Outcome::Cancelled,
            RuntimeLifecycleOutcome::AlreadyQueued => crate::notifications::Outcome::AlreadyQueued,
            RuntimeLifecycleOutcome::Succeeded => crate::notifications::Outcome::Succeeded,
            RuntimeLifecycleOutcome::Failed(message) => {
                crate::notifications::Outcome::Failed(message)
            }
        };
        if let Some(notice) =
            crate::notifications::lifecycle_notice(action, name, computer, elapsed, outcome)
        {
            crate::notifications::notify_native(self, notice);
        }
    }
}

impl RuntimeBundleKind for Option<BundleType> {
    fn runtime_package_kind(&self) -> RuntimePackageKind {
        match self {
            Some(BundleType::App) => RuntimePackageKind::MacApp,
            Some(BundleType::AppImage) => RuntimePackageKind::AppImage,
            Some(BundleType::Deb) => RuntimePackageKind::Deb,
            Some(BundleType::Rpm) => RuntimePackageKind::Rpm,
            _ => RuntimePackageKind::Other,
        }
    }
}

#[tauri::command]
pub fn read_operation_queue() -> runtime::operation_gate::OperationQueue {
    runtime::OPERATIONS.snapshot()
}

/// Ask to cancel a queued or running operation by its queue id. A waiting operation
/// leaves the queue; a running operation is stopped only when it opted in as cancellable.
#[tauri::command]
pub fn cancel_operation(id: u64) -> Result<(), crate::bridge_error::BridgeError> {
    runtime::OPERATIONS
        .cancel(id)
        .map_err(crate::bridge_error::BridgeError::from)
}

#[tauri::command]
pub async fn verify_computer_identities(
    app: AppHandle,
    identities: Vec<runtime::ComputerIdentity>,
) -> Result<bool, String> {
    let host: Arc<dyn RuntimeHost> = Arc::new(app);
    tokio::task::spawn_blocking(move || {
        runtime::verify_computer_identities_with_host(host.as_ref(), identities)
    })
    .await
    .map_err(|_| runtime::internal_failure("checking Git identities"))?
}

#[tauri::command]
pub async fn configure_computer_identities(
    app: AppHandle,
    identities: Vec<runtime::ComputerIdentity>,
) -> Result<(), String> {
    let host: Arc<dyn RuntimeHost> = Arc::new(app);
    let names = identities
        .iter()
        .map(|identity| identity.computer_name().to_owned())
        .collect::<Vec<_>>();
    let worker_host = Arc::clone(&host);
    match tokio::task::spawn_blocking(move || {
        runtime::configure_computer_identities_with_host(worker_host.as_ref(), identities)
    })
    .await
    {
        Ok(result) => result,
        Err(_) => {
            let message = runtime::internal_failure("saving Git identities");
            runtime::notify_git_identity_failure(host.as_ref(), &names, &message);
            Err(message)
        }
    }
}

#[tauri::command]
pub async fn read_computer_configuration(
    app: AppHandle,
) -> Result<runtime::ComputerConfigurationRequest, String> {
    let host: Arc<dyn RuntimeHost> = Arc::new(app);
    tokio::task::spawn_blocking(move || {
        runtime::read_computer_configuration_with_host(host.as_ref())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn read_application_state(
    app: AppHandle,
    refresh_repositories: Option<bool>,
) -> Result<runtime::ApplicationSource, crate::bridge_error::BridgeError> {
    let host: Arc<dyn RuntimeHost> = Arc::new(app);
    tokio::task::spawn_blocking(move || {
        runtime::read_application_state_sync(host.as_ref(), refresh_repositories)
    })
    .await
    .map_err(|_| {
        crate::bridge_error::BridgeError::from(runtime::internal_failure("reading computer state"))
    })?
}

#[tauri::command]
pub async fn read_application_shell(
    app: AppHandle,
    error: String,
) -> Result<runtime::ApplicationSource, String> {
    let host: Arc<dyn RuntimeHost> = Arc::new(app);
    tokio::task::spawn_blocking(move || {
        runtime::read_application_shell_with_host(host.as_ref(), error)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn computer_action(
    app: AppHandle,
    action: String,
    name: String,
    path: Option<String>,
) -> Result<runtime::ApplicationSource, crate::bridge_error::BridgeError> {
    let host: Arc<dyn RuntimeHost> = Arc::new(app);
    tokio::task::spawn_blocking(move || {
        runtime::computer_action_with_host(host.as_ref(), action, name, path)
    })
    .await
    .map_err(|_| {
        crate::bridge_error::BridgeError::from(runtime::internal_failure(
            "running the computer action",
        ))
    })?
}

#[tauri::command]
pub async fn read_setup_activity(
    app: AppHandle,
) -> Result<Vec<runtime::ComputerConfigurationProgress>, String> {
    let host: Arc<dyn RuntimeHost> = Arc::new(app);
    tokio::task::spawn_blocking(move || runtime::read_setup_activity_with_host(host.as_ref()))
        .await
        .map_err(|_| runtime::internal_failure("reading setup activity"))?
}

#[tauri::command]
pub fn skip_computer_use_wait(request_id: String) {
    runtime::skip_computer_use_wait_request(request_id);
}

#[tauri::command]
pub async fn retry_computer_configuration(
    app: AppHandle,
    request_id: Option<String>,
    retry_computer: Option<String>,
) -> Result<runtime::ApplicationSource, String> {
    let host: Arc<dyn RuntimeHost> = Arc::new(app);
    let worker_host = Arc::clone(&host);
    let failure_title = retry_computer.as_deref().map_or_else(
        || "Couldn’t finish computer setup".to_string(),
        |name| format!("Couldn’t finish setting up {name}"),
    );
    match tokio::task::spawn_blocking(move || {
        runtime::retry_computer_configuration_with_host(host.as_ref(), request_id, retry_computer)
    })
    .await
    {
        Ok(result) => result,
        Err(_) => {
            let message = runtime::internal_failure("changing computer settings");
            runtime::notify_configuration_failure(worker_host.as_ref(), &failure_title, &message);
            Err(message)
        }
    }
}

#[tauri::command]
pub async fn change_computer_configuration(
    app: AppHandle,
    change: runtime::ComputerConfigurationChange,
    request_id: Option<String>,
    retry_computer: Option<String>,
) -> Result<runtime::ApplicationSource, String> {
    let host: Arc<dyn RuntimeHost> = Arc::new(app);
    let worker_host = Arc::clone(&host);
    let failure_title = change.failure_title();
    match tokio::task::spawn_blocking(move || {
        runtime::change_computer_configuration_with_host(
            host.as_ref(),
            change,
            request_id,
            retry_computer,
        )
    })
    .await
    {
        Ok(result) => result,
        Err(_) => {
            let message = runtime::internal_failure("changing computer settings");
            runtime::notify_configuration_failure(worker_host.as_ref(), &failure_title, &message);
            Err(message)
        }
    }
}
