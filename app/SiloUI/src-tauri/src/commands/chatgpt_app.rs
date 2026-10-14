//! Tauri adapters for the ChatGPT app engine.
use crate::{chatgpt_app, computer_use, runtime};
use std::{path::PathBuf, sync::Arc};
use tauri::{AppHandle, Emitter, Manager};

const STATUS_EVENT: &str = "chatgpt-app-status";

/// The ChatGPT app folder of this channel: `<app data>/chatgpt`.
pub(crate) fn storage_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("chatgpt"))
        .map_err(|error| format!("Silo could not locate its application storage: {error}"))
}

fn status_publisher(app: AppHandle) -> chatgpt_app::StatusPublisher {
    Arc::new(move |status| {
        if let Ok(value) = serde_json::to_value(status) {
            let _ = app.emit(STATUS_EVENT, chatgpt_app::event_payload(value, None));
        }
    })
}

fn idle_check(app: AppHandle) -> chatgpt_app::IdleCheck {
    Arc::new(move || {
        runtime::update_recovery::running_names(&app).is_ok_and(|names| names.is_empty())
    })
}

fn collect_unused(app: &AppHandle, root: PathBuf) {
    chatgpt_app::collect_unused(root, idle_check(app.clone()));
}

fn ready_hook(app: &AppHandle, root: PathBuf) -> chatgpt_app::ReadyHook {
    let app = app.clone();
    Arc::new(move || {
        // Running built-in computers set computer use up now instead of at their next boot.
        computer_use::app_ready(&app);
        collect_unused(&app, root.clone());
    })
}

/// Starts or wakes the device-wide app preparation worker.
pub(crate) fn ensure_in_background(app: &AppHandle) -> bool {
    let Ok(root) = storage_root(app) else {
        return false;
    };
    chatgpt_app::ensure_in_background(
        root.clone(),
        status_publisher(app.clone()),
        ready_hook(app, root),
    )
}

pub(crate) fn local_status(app: &AppHandle) -> Result<chatgpt_app::Status, String> {
    chatgpt_app::local_status(&storage_root(app)?)
}

/// Wakes a waiting worker or starts a new attempt after a retryable failure.
pub(crate) fn retry_now(app: &AppHandle) -> Result<chatgpt_app::Status, String> {
    let root = storage_root(app)?;
    chatgpt_app::retry_now(&root, status_publisher(app.clone()), ready_hook(app, root))
}

/// Startup status check and automatic background preparation.
pub(crate) fn start_automatic(app: &AppHandle) {
    let Ok(root) = storage_root(app) else {
        return;
    };
    let publisher = status_publisher(app.clone());
    let status = chatgpt_app::refresh_status_blocking(&root, &publisher);
    collect_unused(app, root.clone());
    if matches!(status, chatgpt_app::Status::Ready { .. }) {
        // Finish approval changes saved but never launched before the last quit.
        computer_use::reconcile(app);
        return;
    }
    // Let the app finish starting first; this is not urgent.
    std::thread::sleep(chatgpt_app::auto::START_DELAY);
    ensure_in_background(app);
}

fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> impl std::future::Future<Output = Result<T, String>> {
    let task = tauri::async_runtime::spawn_blocking(work);
    async move {
        task.await
            .map_err(|_| "The ChatGPT app task stopped unexpectedly.".to_owned())?
    }
}

/// The ChatGPT app status of this device, or of the remote device `device` (its
/// device id). A remote device running a Silo without computer use answers `unknown`.
#[tauri::command]
pub(crate) async fn chatgpt_app_status(
    app: AppHandle,
    device: Option<String>,
) -> Result<serde_json::Value, String> {
    blocking(
        move || match chatgpt_app::remote_device(device.as_deref())? {
            Some(device) => chatgpt_app::owner_status(
                crate::remote::device_capabilities(&device).map_err(chatgpt_app::owner_error),
                || chatgpt_app::call_owner(&device, "chatgpt.status", serde_json::json!({})),
            ),
            None => chatgpt_app::to_value(local_status(&app)?),
        },
    )
    .await
}

/// Asks a device to try the download again now. It prepares its own copy; this only
/// wakes it. Resolves with its status at once, progress follows from the status reads.
#[tauri::command]
pub(crate) async fn chatgpt_app_retry(
    app: AppHandle,
    device: Option<String>,
) -> Result<serde_json::Value, String> {
    blocking(
        move || match chatgpt_app::remote_device(device.as_deref())? {
            Some(device) => {
                chatgpt_app::call_owner(&device, "chatgpt.retry", serde_json::json!({}))
            }
            None => chatgpt_app::to_value(retry_now(&app)?),
        },
    )
    .await
}
