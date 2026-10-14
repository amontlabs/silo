use crate::{remote, remote_access, remote_network, remote_ssh_access, runtime};
use serde_json::Value;
use std::sync::Arc;
use tauri::AppHandle;

struct TauriLocalAccess(AppHandle);

impl remote_access::LocalAccess for TauriLocalAccess {
    fn desktop_connection(&self, computer: &str, computer_id: &str) -> Result<Value, String> {
        let paths = runtime::runtime_paths(&self.0)?;
        crate::desktop_viewer::local_connection(&paths, computer, Some(computer_id))
    }

    fn desktop_dispatch(&self, method: &str, params: &Value) -> Result<Value, String> {
        let paths = runtime::runtime_paths(&self.0)?;
        crate::desktop::dispatch(&paths, method, params)
    }

    fn chatgpt_status(&self) -> Result<Value, String> {
        serde_json::to_value(crate::commands::chatgpt_app::local_status(&self.0)?)
            .map_err(|_| "Could not encode the ChatGPT app status.".to_owned())
    }

    fn chatgpt_retry(&self) -> Result<Value, String> {
        serde_json::to_value(crate::commands::chatgpt_app::retry_now(&self.0)?)
            .map_err(|_| "Could not encode the ChatGPT app status.".to_owned())
    }
}

/// Starts the owner-side Connections listener and installs its Tauri boundary adapters.
pub(crate) fn start(app: &AppHandle) -> Result<(), String> {
    let resolver_app = app.clone();
    let paths: Arc<remote::RuntimePathsResolver> =
        Arc::new(move || runtime::runtime_paths(&resolver_app));
    let checkpoints = checkpoint_handler(app.clone());
    let runtime_host: Arc<dyn runtime::RuntimeHost> = Arc::new(app.clone());
    let macos = Arc::new(crate::commands::macos_computers::new_context(app)?);
    let local_access = Arc::new(TauriLocalAccess(app.clone()));
    remote::start(paths, checkpoints, runtime_host, macos, local_access);
    Ok(())
}

fn checkpoint_handler(app: AppHandle) -> Arc<remote_access::CheckpointHandler> {
    Arc::new(move |method, params| {
        let app = app.clone();
        let method = method.to_owned();
        let computer_id = params
            .get("computerId")
            .and_then(Value::as_str)
            .ok_or("Missing computer identity.")?
            .to_owned();
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let checkpoint_id = params
            .get("checkpointId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let new_name = params
            .get("newName")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "Could not run the remote checkpoint operation.")?;
        let result = runtime.block_on(async move {
            match method.as_str() {
                "checkpoint.create" => {
                    crate::commands::runtime_checkpoints::create_checkpoint(
                        app,
                        computer_id,
                        name.ok_or("Missing checkpoint name.")?,
                    )
                    .await
                }
                "checkpoint.fork" => {
                    crate::commands::runtime_checkpoints::fork_checkpoint(
                        app,
                        computer_id,
                        checkpoint_id,
                        new_name.ok_or("Missing fork name.")?,
                    )
                    .await
                }
                "checkpoint.restore" => {
                    crate::commands::runtime_checkpoints::restore_checkpoint(
                        app,
                        computer_id,
                        checkpoint_id.ok_or("Missing checkpoint identity.")?,
                    )
                    .await
                }
                _ => Err("Unsupported checkpoint operation.".into()),
            }
        })?;
        serde_json::to_value(result)
            .map_err(|_| "Could not encode the remote checkpoint result.".to_owned())
    })
}

#[tauri::command]
pub(crate) async fn connections_status() -> Result<remote::ConnectionsStatus, String> {
    remote::connections_status().await
}

#[tauri::command]
pub(crate) async fn set_connections_enabled(
    enabled: bool,
) -> Result<remote::ConnectionsStatus, String> {
    remote::set_connections_enabled(enabled).await
}

#[tauri::command]
pub(crate) async fn device_list() -> Result<Vec<remote::Device>, String> {
    remote::device_list().await
}

#[tauri::command]
pub(crate) async fn remove_device(device_id: String) -> Result<(), String> {
    remote::remove_device(device_id).await
}

#[tauri::command]
pub(crate) async fn authorize_device(app: AppHandle, address: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let command = remote::authorization_command(&address)?;
        let application = crate::applications::selected_terminal(&app)?;
        crate::applications::open_terminal(&app, &application, &command)
    })
    .await
    .map_err(|_| "Could not open the terminal.".to_owned())?
}

#[tauri::command]
pub(crate) async fn setup_device_key(app: AppHandle, address: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let (address, command) = remote::prepare_device_key_setup(&address)?;
        let application = crate::applications::selected_terminal(&app)?;
        crate::applications::open_terminal(&app, &application, &command)?;
        remote::remember_device_key_setup(&address);
        Ok(())
    })
    .await
    .map_err(|_| "Could not open the terminal.".to_owned())?
}

#[tauri::command]
pub(crate) async fn connect_device(
    address: String,
    replace: Option<bool>,
) -> Result<remote::Device, String> {
    remote::connect_device(address, replace).await
}

#[tauri::command]
pub(crate) async fn device_snapshot(
    device_id: String,
    refresh_repositories: Option<bool>,
) -> Result<Value, crate::bridge_error::BridgeError> {
    remote::device_snapshot(device_id, refresh_repositories).await
}

#[tauri::command]
pub(crate) async fn remote_computer_action(
    app: AppHandle,
    device_id: String,
    computer_id: String,
    action: String,
    name: Option<String>,
) -> Result<Value, crate::bridge_error::BridgeError> {
    let started = std::time::Instant::now();
    let notice_id = computer_id.clone();
    let notice_action = action.clone();
    let notice_device = device_id.clone();
    let result = remote::remote_computer_action(device_id, computer_id, action).await;
    let name = name
        .or_else(|| {
            result
                .as_ref()
                .ok()
                .and_then(|state| remote::computer_name(state, &notice_id))
        })
        .unwrap_or_else(|| "this computer".into());
    let computer = remote::remote_notice_computer(&notice_device, &notice_id, &name);
    let outcome = match &result {
        Ok(_) => crate::notifications::Outcome::Succeeded,
        Err(error) if error.code == crate::bridge_error::ErrorCode::Cancelled => {
            crate::notifications::Outcome::Cancelled
        }
        Err(error) if error.code == crate::bridge_error::ErrorCode::AlreadyQueued => {
            crate::notifications::Outcome::AlreadyQueued
        }
        Err(error) => crate::notifications::Outcome::Failed(&error.message),
    };
    if let Some(notice) = crate::notifications::lifecycle_notice(
        &notice_action,
        &name,
        Some(computer),
        started.elapsed(),
        outcome,
    ) {
        crate::notifications::notify_native(&app, notice);
    }
    result
}

#[tauri::command]
pub(crate) async fn remote_checkpoint_action(
    device_id: String,
    computer_id: String,
    action: String,
    name: Option<String>,
    checkpoint_id: Option<String>,
    new_name: Option<String>,
) -> Result<(), crate::bridge_error::BridgeError> {
    remote::remote_checkpoint_action(
        device_id,
        computer_id,
        action,
        name,
        checkpoint_id,
        new_name,
    )
    .await
}

#[tauri::command]
pub(crate) async fn remote_upsert_computer(
    app: AppHandle,
    device_id: String,
    configuration: crate::runtime::ComputerConfiguration,
    expected: Option<crate::runtime::ComputerConfiguration>,
) -> Result<Value, crate::bridge_error::BridgeError> {
    let created = expected.is_none();
    let computer_id = configuration.id().to_owned();
    let state = remote::remote_upsert_computer(device_id.clone(), configuration, expected).await?;
    if created {
        // Keep reading the preference after the remote computer was saved, as before.
        let app_for_settings = app.clone();
        let mode = tokio::task::spawn_blocking(move || {
            crate::settings::current_settings(&app_for_settings)
                .map(|settings| {
                    crate::computer_use::initial_approval_from(&settings)
                        .as_str()
                        .to_owned()
                })
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        if let Err(error) = remote::remote_set_computer_approval(device_id, computer_id, mode).await
        {
            eprintln!("Computer use approval was not set on the new remote computer: {error:?}");
        }
    }
    Ok(state)
}

#[tauri::command]
pub(crate) async fn remote_delete_computer(
    app: AppHandle,
    device_id: String,
    computer_id: String,
    expected: crate::runtime::ComputerConfiguration,
) -> Result<Value, crate::bridge_error::BridgeError> {
    let deleted = format!("silo-remote:{device_id}:{computer_id}");
    let result = remote::remote_delete_computer(device_id, computer_id, expected).await;
    if result.is_ok() {
        crate::notifications::clear_computer(&app, &deleted);
    }
    result
}

#[tauri::command]
pub(crate) async fn remote_network_state(
    device_id: String,
) -> Result<Value, crate::bridge_error::BridgeError> {
    tokio::task::spawn_blocking(move || remote_network::remote_network_state(&device_id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn remote_save_network_port(
    device_id: String,
    computer_id: String,
    port: u16,
    host_port: Option<u16>,
    scheme: Option<String>,
) -> Result<Value, crate::bridge_error::BridgeError> {
    tokio::task::spawn_blocking(move || {
        remote_network::remote_save_network_port(device_id, computer_id, port, host_port, scheme)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn remote_remove_network_port(
    device_id: String,
    computer_id: String,
    port: u16,
) -> Result<Value, crate::bridge_error::BridgeError> {
    tokio::task::spawn_blocking(move || {
        remote_network::remote_remove_network_port(device_id, computer_id, port)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn remote_open_network_port(
    app: AppHandle,
    device_id: String,
    computer_id: String,
    port: u16,
) -> Result<(), String> {
    let url = tokio::task::spawn_blocking(move || {
        remote_network::remote_open_network_port(device_id, computer_id, port)
    })
    .await
    .map_err(|error| error.to_string())??;
    crate::applications::open_browser(&app, &url)
}

#[tauri::command]
pub(crate) async fn remote_ssh_access_state(
    device_id: String,
) -> Result<Value, crate::bridge_error::BridgeError> {
    tokio::task::spawn_blocking(move || remote_ssh_access::remote_ssh_access_state(&device_id))
        .await
        .map_err(|_| "Could not read remote SSH access.".to_owned())?
}

#[tauri::command]
pub(crate) async fn remote_save_ssh_access(
    device_id: String,
    computer_id: String,
    enabled: bool,
    port: u16,
    bind_address: String,
    keys: Option<Vec<String>>,
) -> Result<Value, crate::bridge_error::BridgeError> {
    tokio::task::spawn_blocking(move || {
        remote_ssh_access::remote_save_ssh_access(
            &device_id,
            computer_id,
            enabled,
            port,
            bind_address,
            keys,
        )
    })
    .await
    .map_err(|_| "Could not update remote SSH access.".to_owned())?
}
