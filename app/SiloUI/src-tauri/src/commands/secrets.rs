use crate::secrets::{self, PendingRevocation, RuntimeAccess};
use std::sync::Arc;
use tauri::{AppHandle, WebviewWindow};

struct TauriSecretsRuntime {
    app: AppHandle,
}

impl RuntimeAccess for TauriSecretsRuntime {
    fn apply_secrets(&self, computer: &str) -> Result<Vec<String>, String> {
        if crate::commands::macos_computers::proxy_contains_name(&self.app, computer) {
            return crate::commands::macos_computers::proxy_apply_secrets(&self.app, computer);
        }
        crate::runtime::apply_secrets(&crate::commands::runtime::host(&self.app), computer)
    }

    fn validate_computers(&self, computers: &[String]) -> Result<(), String> {
        let (macos, linux): (Vec<_>, Vec<_>) = computers.iter().cloned().partition(|computer| {
            crate::commands::macos_computers::proxy_contains_name(&self.app, computer)
        });
        if !macos.is_empty() {
            crate::commands::macos_computers::proxy_validate_computers(&self.app, &macos)?;
        }
        if !linux.is_empty() {
            crate::runtime::validate_secret_computers(
                &crate::commands::runtime::host(&self.app),
                &linux,
            )?;
        }
        Ok(())
    }

    fn revoke_secret(&self, record: &PendingRevocation) -> Result<bool, String> {
        if crate::commands::macos_computers::proxy_contains_name(&self.app, record.computer()) {
            return crate::commands::macos_computers::proxy_revoke_secret(
                &self.app,
                record.computer(),
                record.name(),
            );
        }
        crate::runtime::revoke_secret(&crate::commands::runtime::host(&self.app), record)
    }
}

pub(crate) fn install(app: &AppHandle) -> Result<(), String> {
    secrets::install_runtime(Arc::new(TauriSecretsRuntime { app: app.clone() }))?;
    secrets::install()
}

fn require_main(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Secret changes are only available in the main window.".into())
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
pub async fn read_secrets_state() -> Result<Vec<serde_json::Value>, String> {
    secrets::read_secrets_state().await
}

#[tauri::command]
pub async fn save_secret(
    window: WebviewWindow,
    request: secrets::Request,
) -> Result<Vec<serde_json::Value>, String> {
    require_main(&window)?;
    blocking("Secret changes could not finish.", move || {
        secrets::save_secret(request, crate::updates::operation_guard)
    })
    .await
}

#[tauri::command]
pub async fn remove_secret(
    window: WebviewWindow,
    id: String,
) -> Result<Vec<serde_json::Value>, String> {
    require_main(&window)?;
    blocking("Secret removal could not finish.", move || {
        secrets::remove_secret(id, crate::updates::operation_guard)
    })
    .await
}

#[tauri::command]
pub async fn retry_secret(
    window: WebviewWindow,
    id: String,
) -> Result<Vec<serde_json::Value>, String> {
    require_main(&window)?;
    blocking("Secret changes could not finish.", move || {
        secrets::retry_secret(id, crate::updates::operation_guard)
    })
    .await
}
