//! Tauri adapters for the runtime engine and compatibility services used by
//! other domains while they are extracted.
use crate::runtime;
use crate::runtime::{RuntimeHost, RuntimeLifecycleOutcome, RuntimePackageKind};
use silo_engine::context::{self, EngineContext};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{utils::config::BundleType, AppHandle};

fn computer_action_worker_failure(action: &str) -> crate::bridge_error::BridgeError {
    let message = if matches!(action, "open-editor" | "open-terminal") {
        "The application launcher failed.".to_owned()
    } else {
        runtime::internal_failure("running the computer action")
    };
    crate::bridge_error::BridgeError::from(message)
}

pub(crate) struct TauriRuntimeHost(AppHandle);

pub(crate) fn host(app: &AppHandle) -> TauriRuntimeHost {
    TauriRuntimeHost(app.clone())
}

impl RuntimeHost for TauriRuntimeHost {
    fn engine_context(&self) -> std::sync::Arc<dyn EngineContext> {
        context::current().expect("the engine context is installed during app setup")
    }

    fn macos_context(&self) -> Option<silo_engine::macos_computers::MacosContext> {
        crate::commands::macos_computers::new_context(&self.0).ok()
    }

    fn runtime_storage(&self) -> Result<PathBuf, String> {
        crate::commands::runtime_migration::runtime_storage(&self.0)
    }

    fn blocks_operations(&self) -> bool {
        crate::commands::runtime_migration::blocks_operations(&self.0)
    }

    fn bundled_tools_directory(&self) -> Result<PathBuf, String> {
        crate::commands::bundled_tools::directory(&self.0)
    }

    fn ensure_runtime_ready(&self) -> Result<(), String> {
        crate::commands::runtime_migration::ensure_ready(&self.0)
    }

    fn runtime_package_kind(&self) -> RuntimePackageKind {
        package_kind(tauri::utils::platform::bundle_type())
    }

    fn schedule_secret_revocations(&self) {
        crate::secrets::schedule_revocations();
    }

    fn repository_push_operations(&self, local: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        runtime::runtime_paths(self)
            .and_then(|paths| crate::host_push_operations::merge(&paths, local.clone()))
            .unwrap_or(local)
    }

    fn github_snapshot(&self) -> Result<serde_json::Value, String> {
        crate::github::snapshot()
    }

    fn open_terminal(&self, name: &str) -> Result<(), String> {
        crate::commands::terminal::open(&self.0, name)
    }

    fn open_editor(&self, name: &str, path: Option<&str>) -> Result<(), String> {
        crate::commands::editor::open(&self.0, name, path)
    }

    fn macos_computer_names(&self) -> Vec<String> {
        crate::macos_computers::names(&self.0)
    }

    fn macos_log_target(&self, computer_id: &str) -> Option<(String, PathBuf)> {
        crate::macos_computers::log_target(&self.0, computer_id)
    }

    fn notify(&self, notice: crate::notifications::Notice) {
        deliver_runtime_notice(self, notice);
    }

    fn clear_computer_notices(&self, computer_ids: &[String]) {
        withdraw_runtime_notices(self, computer_ids);
    }

    fn stop_macos_computers(&self, deadline: Option<std::time::Instant>) -> Result<(), String> {
        crate::macos_computers::stop_all(&self.0, deadline)
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
            deliver_runtime_notice(self, notice);
        }
    }
}

trait RuntimeNoticeDelivery {
    fn notify_native(&self, notice: crate::notifications::Notice);
    fn clear_computer(&self, computer_id: &str);
}

impl RuntimeNoticeDelivery for TauriRuntimeHost {
    fn notify_native(&self, notice: crate::notifications::Notice) {
        crate::commands::notifications::notify_native(&self.0, notice);
    }

    fn clear_computer(&self, computer_id: &str) {
        crate::commands::notifications::clear_computer(&self.0, computer_id);
    }
}

fn deliver_runtime_notice(
    delivery: &impl RuntimeNoticeDelivery,
    notice: crate::notifications::Notice,
) {
    delivery.notify_native(notice);
}

fn withdraw_runtime_notices(delivery: &impl RuntimeNoticeDelivery, computer_ids: &[String]) {
    for computer_id in computer_ids {
        delivery.clear_computer(computer_id);
    }
}

pub(crate) fn package_kind(bundle: Option<BundleType>) -> RuntimePackageKind {
    match bundle {
        Some(BundleType::App) => RuntimePackageKind::MacApp,
        Some(BundleType::AppImage) => RuntimePackageKind::AppImage,
        Some(BundleType::Deb) => RuntimePackageKind::Deb,
        Some(BundleType::Rpm) => RuntimePackageKind::Rpm,
        _ => RuntimePackageKind::Other,
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
    let host: Arc<dyn RuntimeHost> = Arc::new(host(&app));
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
    let host: Arc<dyn RuntimeHost> = Arc::new(host(&app));
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
    let host: Arc<dyn RuntimeHost> = Arc::new(host(&app));
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
    let host: Arc<dyn RuntimeHost> = Arc::new(host(&app));
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
    let host: Arc<dyn RuntimeHost> = Arc::new(host(&app));
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
    let host: Arc<dyn RuntimeHost> = Arc::new(host(&app));
    let action_for_error = action.clone();
    tokio::task::spawn_blocking(move || {
        runtime::computer_action_with_host(host.as_ref(), action, name, path)
    })
    .await
    .map_err(|_| computer_action_worker_failure(&action_for_error))?
}

#[tauri::command]
pub async fn read_setup_activity(
    app: AppHandle,
) -> Result<Vec<runtime::ComputerConfigurationProgress>, String> {
    let host: Arc<dyn RuntimeHost> = Arc::new(host(&app));
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
    let host: Arc<dyn RuntimeHost> = Arc::new(host(&app));
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
    let host: Arc<dyn RuntimeHost> = Arc::new(host(&app));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingNoticeDelivery {
        native: Mutex<Vec<crate::notifications::Notice>>,
        cleared: Mutex<Vec<String>>,
    }

    impl RuntimeNoticeDelivery for RecordingNoticeDelivery {
        fn notify_native(&self, notice: crate::notifications::Notice) {
            self.native.lock().unwrap().push(notice);
        }

        fn clear_computer(&self, computer_id: &str) {
            self.cleared.lock().unwrap().push(computer_id.to_owned());
        }
    }

    #[test]
    fn launcher_worker_failure_keeps_the_launcher_error() {
        for action in ["open-editor", "open-terminal"] {
            assert_eq!(
                computer_action_worker_failure(action).message,
                "The application launcher failed."
            );
        }
    }

    #[test]
    fn runtime_notice_uses_native_only_delivery() {
        let notice = crate::notifications::Notice {
            category: crate::notifications::Category::Failures,
            key: "lifecycle:dev".into(),
            title: "Couldn’t start dev".into(),
            body: "The framework rejected the start.".into(),
            computer: None,
        };
        let delivery = RecordingNoticeDelivery::default();

        deliver_runtime_notice(&delivery, notice.clone());

        assert_eq!(*delivery.native.lock().unwrap(), vec![notice]);
        assert!(delivery.cleared.lock().unwrap().is_empty());
    }

    #[test]
    fn runtime_notice_withdrawal_uses_native_notification_clear() {
        let delivery = RecordingNoticeDelivery::default();
        let computer_ids = vec!["local:dev".to_owned(), "remote:dev".to_owned()];

        withdraw_runtime_notices(&delivery, &computer_ids);

        assert_eq!(*delivery.cleared.lock().unwrap(), computer_ids);
        assert!(delivery.native.lock().unwrap().is_empty());
    }
}
