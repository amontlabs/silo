//! Tauri adapters for desktop and computer-use operations.
use crate::{desktop, remote_access, runtime};
use serde_json::{json, Value};
use tauri::{AppHandle, Window};

#[tauri::command]
pub(crate) async fn set_computer_use_approval(
    app: AppHandle,
    window: Window,
    computer: String,
    mode: String,
) -> Result<Value, String> {
    crate::commands::desktop_viewer::require_computer(&window, &computer)?;
    tokio::task::spawn_blocking(move || {
        if let Some((device, computer)) = remote_access::target(&computer)? {
            crate::chatgpt_app::call_owner(
                &device,
                "computerUse.approval",
                json!({"computerId":computer,"mode":mode}),
            )
        } else {
            let paths = runtime::runtime_paths(&app)?;
            desktop::local_approval(&paths, &computer, &mode, None)
        }
    })
    .await
    .map_err(|_| "Silo could not change computer use. Retry.".to_string())?
}

async fn execute(
    app: AppHandle,
    computer: String,
    action: Option<String>,
) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        runtime::shutdown::ensure_accepting_operations()?;
        if let Some((device, computer)) = remote_access::target(&computer)? {
            let setup = action.as_deref() == Some("setup-computer-use");
            crate::remote::call_remote(
                &device,
                if action.is_some() {
                    "desktop.action"
                } else {
                    "desktop.status"
                },
                json!({"computerId":computer,"action":action}),
            )
            .map_err(|error| {
                if setup && error == "Unsupported desktop action." {
                    "Update Silo on that device to use computer use.".into()
                } else {
                    error
                }
            })
        } else {
            let paths = runtime::runtime_paths(&app)?;
            desktop::local(&paths, &computer, action.as_deref(), None)
        }
    })
    .await
    .map_err(|_| {
        "Silo could not finish the Linux desktop action. Reopen the viewer and retry.".to_string()
    })?
}

#[tauri::command]
pub(crate) async fn read_desktop_state(
    app: AppHandle,
    window: Window,
    computer: String,
) -> Result<Value, String> {
    crate::commands::desktop_viewer::require_computer(&window, &computer)?;
    execute(app, computer, None).await
}

#[tauri::command]
pub(crate) async fn desktop_action(
    app: AppHandle,
    window: Window,
    computer: String,
    action: String,
) -> Result<Value, String> {
    crate::commands::desktop_viewer::require_computer(&window, &computer)?;
    execute(app, computer, Some(action)).await
}
