use crate::github::{self, RuntimeAccess};
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Manager, WebviewWindow};

struct TauriGitHubRuntime {
    app: AppHandle,
}

impl RuntimeAccess for TauriGitHubRuntime {
    fn scoped_cached_tokens(&self, computer: &str) -> Result<crate::runtime::ScopedTokens, String> {
        crate::runtime::scoped_cached_tokens(&self.app, computer)
    }

    fn github_policy_is_cached(&self, computer: &str, profile: &Value) -> Result<bool, String> {
        crate::runtime::github_policy_is_cached(&self.app, computer, profile)
    }

    fn apply_github_policy(
        &self,
        computer: &str,
        revision: u64,
        profile: &Value,
    ) -> Result<(), String> {
        crate::runtime::apply_github_policy(&self.app, computer, revision, profile)
    }

    fn apply_github_identity(&self, computer: &str, identity: &Value) -> Result<(), String> {
        crate::runtime::apply_github_identity(&self.app, computer, identity)
    }

    fn is_pending_restore(&self, computer: &str) -> bool {
        crate::runtime::runtime_paths(&self.app)
            .is_ok_and(|paths| crate::runtime::is_pending_restore(&paths, computer))
    }

    fn is_device_removed(&self, computer: &str) -> bool {
        crate::runtime::runtime_paths(&self.app).is_ok_and(|paths| {
            matches!(
                crate::runtime::resolve_computer_id(&paths, computer),
                Err(crate::runtime::RuntimeError::Invalid(_))
            )
        })
    }
}

pub(crate) fn install(app: &AppHandle) -> Result<(), String> {
    github::install_runtime(Arc::new(TauriGitHubRuntime { app: app.clone() }))?;
    github::install()
}

fn require_main(window: &WebviewWindow) -> Result<(), String> {
    require_main_label(window.label())
}

fn require_main_label(label: &str) -> Result<(), String> {
    if label == "main" {
        Ok(())
    } else {
        Err("Only the main window can manage GitHub access.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::require_main_label;

    #[test]
    fn only_main_window_can_manage_github() {
        assert!(require_main_label("main").is_ok());
        for label in ["status", "other", ""] {
            assert!(require_main_label(label).is_err());
        }
    }
}

async fn blocking<T: Send + 'static>(
    message: &'static str,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| message.to_owned())?
}

#[tauri::command]
pub async fn read_github_state() -> Result<Value, String> {
    blocking("GitHub state could not be read.", github::snapshot).await
}

#[tauri::command]
pub async fn connect_github(window: WebviewWindow) -> Result<Value, String> {
    require_main(&window)?;
    let generation = github::prepare_connect();
    let app = window.app_handle().clone();
    blocking("GitHub operation failed.", move || {
        let result = github::connect(generation, |url| {
            crate::applications::open_browser(&app, url)
        });
        if result.is_ok() {
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.show();
                let _ = main.set_focus();
            }
        }
        result
    })
    .await
}

#[tauri::command]
pub async fn cancel_github_connection(window: WebviewWindow) -> Result<Value, String> {
    require_main(&window)?;
    blocking("GitHub cancellation failed.", github::cancel_connection).await
}

#[tauri::command]
pub async fn reopen_github_authorization(window: WebviewWindow) -> Result<(), String> {
    require_main(&window)?;
    let app = window.app_handle().clone();
    blocking("Cannot reopen GitHub authorization.", move || {
        let url = github::authorization_url()?;
        crate::applications::open_browser(&app, &url)
    })
    .await
}

#[tauri::command]
pub async fn manage_github_repositories(window: WebviewWindow) -> Result<(), String> {
    require_main(&window)?;
    let app = window.app_handle().clone();
    blocking("Cannot open GitHub repository access.", move || {
        let url = github::repository_authorization_url()?;
        crate::applications::open_browser(&app, &url)
    })
    .await
}

#[tauri::command]
pub async fn refresh_github_repositories(window: WebviewWindow) -> Result<Value, String> {
    require_main(&window)?;
    github::prepare_refresh();
    blocking("GitHub operation failed.", || {
        let _update = crate::updates::operation_guard()?;
        github::run_operation(github::refresh_repositories)
    })
    .await
}

#[tauri::command]
pub async fn disconnect_github(window: WebviewWindow) -> Result<Value, String> {
    require_main(&window)?;
    let ticket = github::prepare_disconnect();
    blocking("GitHub operation failed.", move || {
        github::disconnect(ticket, crate::updates::operation_guard)
    })
    .await
}

#[tauri::command]
pub async fn set_github_access_enabled(
    window: WebviewWindow,
    enabled: bool,
) -> Result<Value, String> {
    require_main(&window)?;
    let ticket = github::prepare_access_change(enabled);
    blocking("GitHub operation failed.", move || {
        github::set_access_enabled(enabled, ticket, crate::updates::operation_guard)
    })
    .await
}

#[tauri::command]
pub async fn save_github_configuration(
    window: WebviewWindow,
    configuration: Value,
) -> Result<Value, String> {
    require_main(&window)?;
    github::validate_configuration(&configuration)?;
    let ticket = github::intent_ticket();
    blocking("GitHub operation failed.", move || {
        github::save_configuration(configuration, ticket, crate::updates::operation_guard)
    })
    .await
}

#[tauri::command]
pub async fn retry_github_configuration(
    window: WebviewWindow,
    computer: Option<String>,
) -> Result<Value, String> {
    require_main(&window)?;
    let ticket = github::prepare_retry_configuration();
    blocking("GitHub operation failed.", move || {
        github::retry_configuration(computer, ticket, crate::updates::operation_guard)
    })
    .await
}

#[tauri::command]
pub async fn save_github_personal_token(
    window: WebviewWindow,
    token: String,
) -> Result<Value, String> {
    require_main(&window)?;
    blocking("Could not save GitHub token.", move || {
        let _update = crate::updates::operation_guard()?;
        github::personal_token::save_github_personal_token(token)
    })
    .await
}

#[tauri::command]
pub async fn remove_github_personal_token(window: WebviewWindow) -> Result<Value, String> {
    require_main(&window)?;
    blocking("Could not remove GitHub token.", || {
        let _update = crate::updates::operation_guard()?;
        github::personal_token::remove_github_personal_token()
    })
    .await
}
