use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::{json, Value};
use silo_api::{
    jobs::{ErrorCode, JobFinished},
    methods::{
        clipboard::{
            ClipboardAction, ClipboardTransferContent, ClipboardTransferResult,
            ClipboardTransferStatus,
        },
        computer_use::{ApprovalMode, ComputerUseState},
        desktop::{DesktopAction, DesktopStatus},
        display::{ViewerCookie, ViewerOpenResult, ViewerPageEvent, ViewerSoundCheckResult},
        MethodName,
    },
    transport::ErrorObject,
};
use silo_engine::{
    context::{Audience, ClientId, EngineContext, Event},
    desktop_bridge::{Bridge, Page},
    runtime::{self, RuntimeHost, RuntimePackageKind, RuntimePaths},
};

use crate::dispatcher::{method_name, DispatchError, HandlerOutput};

use super::{ownership::owns_prefix, HandlerContext};

pub(super) fn owns(method: MethodName) -> bool {
    owns_prefix(
        method,
        &["display.", "desktop.", "computerUse.", "clipboard."],
    )
}

pub async fn handle(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Option<Result<HandlerOutput, DispatchError>> {
    if owns(method) {
        Some(dispatch(method, params, context).await)
    } else {
        None
    }
}

async fn dispatch(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    match method {
        MethodName::DesktopStatus => desktop_status(context, params).await,
        MethodName::DesktopConnect => desktop_connect(context, params).await,
        MethodName::DesktopAction => desktop_action(context, params).await,
        MethodName::ComputerUseState => computer_use_state(context, params).await,
        MethodName::ComputerUseApproval => computer_use_approval(context, params).await,
        MethodName::ComputerUseSetup => computer_use_setup(context, params).await,
        MethodName::DisplayShow => display_show(context, params).await,
        MethodName::DisplayClose => display_close(context, params).await,
        MethodName::DisplayViewerOpen => display_viewer_open(context, params).await,
        MethodName::DisplayViewerClose => display_viewer_close(context, params).await,
        MethodName::DisplayViewerSoundCheck => display_viewer_sound_check(context, params).await,
        MethodName::DisplayViewerSoundCancel => display_viewer_sound_cancel(context, params).await,
        MethodName::DisplayViewerAudioSet => display_viewer_audio_set(context, params).await,
        MethodName::DisplayViewerScreenReset => display_viewer_screen_reset(context, params).await,
        MethodName::ClipboardTransfer => clipboard_transfer(context, params).await,
        MethodName::ClipboardRead => clipboard_read(context, params).await,
        MethodName::ClipboardWrite => clipboard_write(context, params).await,
        MethodName::DisplayNativeAttach => Err(DispatchError::unsupported("display.native.attach")),
        _ => Err(DispatchError::unsupported(method_name(method))),
    }
}

fn output<T: Serialize>(value: T) -> Result<HandlerOutput, DispatchError> {
    serde_json::to_value(value)
        .map(|value| HandlerOutput {
            value,
            event_barrier: None,
        })
        .map_err(|error| DispatchError::internal(error.to_string()))
}

fn output_value(value: Value) -> HandlerOutput {
    HandlerOutput {
        value,
        event_barrier: None,
    }
}

/// Maps the engine's text refusals onto the canonical error codes.
fn map_error(message: impl Into<String>) -> DispatchError {
    let message = message.into();
    let lower = message.to_ascii_lowercase();
    let has = |needle: &str| lower.contains(needle);
    let code = if message == silo_engine::desktop_viewer_api::SECOND_VIEWER_REFUSAL {
        ErrorCode::DesktopUpdateRequired
    } else if has("was cancelled") {
        ErrorCode::Cancelled
    } else if has("already waiting") {
        ErrorCode::AlreadyQueued
    } else if has("no longer exists") || has("unknown desktop viewer") || has("not found") {
        ErrorCode::NotFound
    } else if has("openssh is required") {
        ErrorCode::OpenSshMissing
    } else if has("not running")
        || has("isn't running")
        || has("start the computer")
        || has("is stopped")
    {
        ErrorCode::NotRunning
    } else if has("setup is using") {
        ErrorCode::Busy
    } else if has("not connected") {
        ErrorCode::NotRunning
    } else if has("timed out") || has("took too long") {
        ErrorCode::TimedOut
    } else if lower.starts_with("busy:")
        || has("already running")
        || has("still connecting")
        || has("still running")
        || has("is quitting")
    {
        ErrorCode::Busy
    } else if has("permission denied") {
        ErrorCode::PermissionDenied
    } else if has("unsupported") || has("not supported") {
        ErrorCode::UnsupportedRemoteOperation
    } else {
        ErrorCode::Internal
    };
    DispatchError { code, message }
}

fn decode<T: serde::de::DeserializeOwned>(
    context: &HandlerContext,
    params: &Value,
) -> Result<T, DispatchError> {
    context.decode_params(params)
}

fn runtime_host(context: &HandlerContext) -> DisplayRuntimeHost {
    DisplayRuntimeHost {
        context: Arc::new(context.silod_context().clone()),
    }
}

pub(super) fn macos_context(
    context: &HandlerContext,
) -> silo_engine::macos_computers_engine::MacosContext {
    let engine: Arc<dyn EngineContext> = Arc::new(context.silod_context().clone());
    silo_engine::macos_computers_engine::MacosContext::new(engine, Arc::new(SilodMacosHost))
}

struct DisplayRuntimeHost {
    context: Arc<dyn EngineContext>,
}

impl RuntimeHost for DisplayRuntimeHost {
    fn engine_context(&self) -> Arc<dyn EngineContext> {
        self.context.clone()
    }
    fn runtime_storage(&self) -> Result<PathBuf, String> {
        silo_engine::runtime_migration::Controller::new(self.context.paths().app_data.clone())
            .runtime_storage()
    }
    fn blocks_operations(&self) -> bool {
        silo_engine::runtime_migration::Controller::new(self.context.paths().app_data.clone())
            .blocks_operations()
    }
    fn bundled_tools_directory(&self) -> Result<PathBuf, String> {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        silo_engine::bundled_tools::resolve(
            &executable,
            &self.context.paths().resources,
            Some(silo_engine::bundled_tools::BundleKind::App),
            None,
        )
    }
    fn ensure_runtime_ready(&self) -> Result<(), String> {
        silo_engine::runtime_migration::Controller::new(self.context.paths().app_data.clone())
            .ensure_ready()
    }
    fn runtime_package_kind(&self) -> RuntimePackageKind {
        RuntimePackageKind::MacApp
    }
}

/// Opens (or focuses) the engine-owned VM window of macOS computer `id` on AppKit's main
/// thread and attaches its display. The one window table lives here, so every host that can
/// show a window (this domain's and the computers domain's) shares it.
pub(super) fn show_native_display(
    context: &silo_engine::macos_computers_engine::MacosContext,
    id: &str,
    title: &str,
    toolbar: bool,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        native_display::show(context, id, title, toolbar)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (context, id, title, toolbar);
        Err("macOS display is unavailable on this host.".into())
    }
}

/// Asks the window of `id` to close; the close reports back through its delegate.
pub(super) fn close_native_display(id: &str) {
    #[cfg(target_os = "macos")]
    native_display::close(id);
    #[cfg(not(target_os = "macos"))]
    let _ = id;
}

struct SilodMacosHost;

impl silo_engine::macos_computers_engine::MacosHost for SilodMacosHost {
    fn show_display(
        &self,
        context: &silo_engine::macos_computers_engine::MacosContext,
        id: &str,
        title: &str,
        toolbar: bool,
    ) -> Result<(), String> {
        show_native_display(context, id, title, toolbar)
    }

    fn close_display(&self, id: &str) {
        close_native_display(id);
    }

    fn linux_computer_names(&self) -> Vec<String> {
        silo_engine::context::current()
            .map(|context| runtime::computer_names(&DisplayRuntimeHost { context }))
            .unwrap_or_default()
    }
}

/// Whether `id` names one of this engine's macOS computers.
async fn is_macos_computer(context: &HandlerContext, id: &str) -> Result<bool, DispatchError> {
    silo_engine::macos_computers_engine::has_macos_computer(macos_context(context), id.to_owned())
        .await
        .map_err(map_error)
}

fn macos_unsupported(method: &str) -> DispatchError {
    DispatchError {
        code: ErrorCode::UnsupportedRemoteOperation,
        message: format!(
            "{method} is not available for macOS computers: they have no Linux desktop service."
        ),
    }
}

async fn local_runtime_paths(context: &HandlerContext) -> Result<RuntimePaths, DispatchError> {
    let host = runtime_host(context);
    tokio::task::spawn_blocking(move || runtime::runtime_paths(&host))
        .await
        .map_err(|error| DispatchError::internal(error.to_string()))?
        .map_err(map_error)
}

async fn desktop_status(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::common::ComputerTargetParams = decode(context, value)?;
    if let Some(device) = params
        .device_id
        .as_deref()
        .filter(|device| *device != context.silod_context().local_device().device_id)
    {
        let reply = silo_engine::remote::call_remote_typed(
            device,
            "desktop.status",
            json!({"computerId": params.computer_id}),
        )
        .map_err(HandlerContext::map_engine_error)?;
        return output(
            serde_json::from_value::<DesktopStatus>(reply)
                .map_err(|error| DispatchError::internal(error.to_string()))?,
        );
    }
    let row = silo_engine::macos_computers_engine::read_macos_computer(
        macos_context(context),
        params.computer_id.clone(),
    )
    .await
    .map_err(map_error)?;
    if let Some(row) = row {
        // A macOS computer has no desktop service: the session follows the machine.
        let state = match row["state"].as_str().unwrap_or("stopped") {
            "running" => "running",
            "starting" => "starting",
            "failed" => "failed",
            _ => "computer-stopped",
        };
        let session = match state {
            "running" => "running",
            "starting" => "starting",
            "failed" => "failed",
            _ => "stopped",
        };
        let computer_use = silo_engine::macos_computers_engine::macos_computer_use_state(
            macos_context(context),
            params.computer_id.clone(),
        )
        .await
        .map_err(map_error)?;
        let result = json!({
            "installed": row["installed"].as_bool().unwrap_or(false), "state": state,
            "autoStart": false, "version": null, "user": null, "display": null,
            "backend": null, "sessionState": session, "streamState": null,
            "updateRequired": false, "updateAvailable": false, "streamerVersion": null,
            "computerUse": computer_use, "name": row["name"],
        });
        return output(
            serde_json::from_value::<DesktopStatus>(result)
                .map_err(|error| DispatchError::internal(error.to_string()))?,
        );
    }
    let paths = local_runtime_paths(context).await?;
    let name = tokio::task::spawn_blocking({
        let paths = paths.clone();
        let id = params.computer_id.clone();
        move || runtime::remote_ops::local_computer_name_in(&paths, &id)
    })
    .await
    .map_err(|error| DispatchError::internal(error.to_string()))?
    .map_err(map_error)?;
    let id = params.computer_id;
    let status = tokio::task::spawn_blocking(move || {
        silo_engine::desktop::local(&paths, &name, None, Some(&id))
    })
    .await
    .map_err(|error| DispatchError::internal(error.to_string()))?
    .map_err(map_error)?;
    output(
        serde_json::from_value::<DesktopStatus>(status)
            .map_err(|error| DispatchError::internal(error.to_string()))?,
    )
}

/// `desktop.connect` returns the guest's desktop credentials, which a client never holds
/// (05 §7; 95 A-28). A peer engine reads them through the owner-side bridge, which does not
/// pass through `engine.sock`, so every request that reaches this handler is a local client
/// and is refused before any computer is looked up.
async fn desktop_connect(
    _context: &HandlerContext,
    _value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    Err(DispatchError {
        code: silo_api::jobs::ErrorCode::PermissionDenied,
        message: "desktop.connect is for peer engines. Open the computer with display.viewer.open."
            .to_owned(),
    })
}

async fn desktop_action(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::desktop::DesktopActionParams = decode(context, value)?;
    if params.device_id.is_none() && is_macos_computer(context, &params.computer_id).await? {
        return Err(macos_unsupported("desktop.action"));
    }
    let local_id = context.silod_context().local_device().device_id.clone();
    let host = runtime_host(context);
    start_job(context, "desktop.action", move |link| {
        if let Some(device) = params
            .device_id
            .as_deref()
            .filter(|device| *device != local_id)
        {
            let action = serde_json::to_value(params.action).map_err(|error| error.to_string())?;
            let (device, computer_id) = (device.to_owned(), params.computer_id.clone());
            return relay_remote(link, move || {
                silo_engine::remote::call_remote(
                    &device,
                    "desktop.action",
                    json!({"computerId": computer_id, "action": action}),
                )
            });
        }
        let paths = runtime::runtime_paths(&host).map_err(|error| error.to_string())?;
        let name = runtime::remote_ops::local_computer_name_in(&paths, &params.computer_id)?;
        let action = desktop_action_name(params.action);
        let status = silo_engine::desktop::local_for_job(
            &paths,
            &name,
            Some(action),
            Some(&params.computer_id),
            Some(link),
        )?;
        runtime::publish_computers_changed(&host, &paths);
        Ok(status)
    })
    .await
}

fn desktop_action_name(action: DesktopAction) -> &'static str {
    match action {
        DesktopAction::Start => "start",
        DesktopAction::Stop => "stop",
        DesktopAction::Restart => "restart",
        DesktopAction::RestartStreamer => "restart-streamer",
        DesktopAction::UpdateStreamer => "update-streamer",
    }
}

async fn computer_use_state(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::computer_use::ComputerUseStateParams = decode(context, value)?;
    if let Some(device) = params
        .device_id
        .as_deref()
        .filter(|device| *device != context.silod_context().local_device().device_id)
    {
        let reply = silo_engine::remote::call_remote_typed(
            device,
            "desktop.status",
            json!({"computerId": params.computer_id}),
        )
        .map_err(HandlerContext::map_engine_error)?;
        return output(
            serde_json::from_value::<ComputerUseState>(
                reply.get("computerUse").cloned().unwrap_or(Value::Null),
            )
            .map_err(|error| DispatchError::internal(error.to_string()))?,
        );
    }
    if is_macos_computer(context, &params.computer_id).await? {
        let state = silo_engine::macos_computers_engine::macos_computer_use_state(
            macos_context(context),
            params.computer_id,
        )
        .await
        .map_err(map_error)?;
        return output(
            serde_json::from_value::<ComputerUseState>(state)
                .map_err(|error| DispatchError::internal(error.to_string()))?,
        );
    }
    let params = params.computer_id;
    let value = json!({"computerId": params});
    let status = desktop_status(context, &value).await?.value;
    output(
        serde_json::from_value::<ComputerUseState>(
            status.get("computerUse").cloned().unwrap_or(Value::Null),
        )
        .map_err(|error| DispatchError::internal(error.to_string()))?,
    )
}

async fn computer_use_approval(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::computer_use::ComputerUseApprovalParams =
        decode(context, value)?;
    if let Some(device) = params
        .device_id
        .as_deref()
        .filter(|device| *device != context.silod_context().local_device().device_id)
    {
        let mode = match params.mode {
            ApprovalMode::Ask => "ask",
            ApprovalMode::Auto => "auto",
        };
        let reply = silo_engine::remote::call_remote_typed(
            device,
            "computerUse.approval",
            json!({"computerId": params.computer_id, "mode": mode}),
        )
        .map_err(HandlerContext::map_engine_error)?;
        return output(
            serde_json::from_value::<ComputerUseState>(reply)
                .map_err(|error| DispatchError::internal(error.to_string()))?,
        );
    }
    if is_macos_computer(context, &params.computer_id).await? {
        let mode = match params.mode {
            ApprovalMode::Ask => silo_engine::computer_use::Approval::Ask,
            ApprovalMode::Auto => silo_engine::computer_use::Approval::Auto,
        };
        let state = silo_engine::macos_computers_engine::set_macos_computer_use_approval(
            macos_context(context),
            params.computer_id,
            mode,
        )
        .await
        .map_err(map_error)?;
        return output(
            serde_json::from_value::<ComputerUseState>(state)
                .map_err(|error| DispatchError::internal(error.to_string()))?,
        );
    }
    let paths = local_runtime_paths(context).await?;
    let host = runtime_host(context);
    let mode = match params.mode {
        ApprovalMode::Ask => "ask",
        ApprovalMode::Auto => "auto",
    };
    let result = tokio::task::spawn_blocking(move || {
        let name = runtime::remote_ops::local_computer_name_in(&paths, &params.computer_id)?;
        let state =
            silo_engine::desktop::local_approval(&paths, &name, mode, Some(&params.computer_id))?;
        runtime::publish_computers_changed(&host, &paths);
        Ok::<_, String>(state)
    })
    .await
    .map_err(|error| DispatchError::internal(error.to_string()))?
    .map_err(map_error)?;
    output(
        serde_json::from_value::<ComputerUseState>(
            result.get("computerUse").cloned().unwrap_or(result),
        )
        .map_err(|error| DispatchError::internal(error.to_string()))?,
    )
}

async fn computer_use_setup(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::computer_use::ComputerUseSetupParams = decode(context, value)?;
    if params.device_id.is_none() && is_macos_computer(context, &params.computer_id).await? {
        return Err(DispatchError {
            code: ErrorCode::UnsupportedRemoteOperation,
            message: "A macOS computer sets up computer use itself; change its approval to apply a new mode."
                .into(),
        });
    }
    let local_id = context.silod_context().local_device().device_id.clone();
    let host = runtime_host(context);
    start_job(context, "computerUse.setup", move |link| {
        if let Some(device) = params
            .device_id
            .as_deref()
            .filter(|device| *device != local_id)
        {
            let (device, computer_id) = (device.to_owned(), params.computer_id.clone());
            return relay_remote(link, move || {
                silo_engine::remote::call_remote(
                    &device,
                    "desktop.action",
                    json!({"computerId": computer_id, "action": "setup-computer-use"}),
                )
            });
        }
        let paths = runtime::runtime_paths(&host).map_err(|error| error.to_string())?;
        let name = runtime::remote_ops::local_computer_name_in(&paths, &params.computer_id)?;
        let status = silo_engine::desktop::local_for_job(
            &paths,
            &name,
            Some("setup-computer-use"),
            Some(&params.computer_id),
            Some(link),
        )?;
        runtime::publish_computers_changed(&host, &paths);
        Ok(status)
    })
    .await
}

async fn display_show(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::common::ComputerTargetParams = decode(context, value)?;
    if params
        .device_id
        .as_deref()
        .is_some_and(|device| device != context.silod_context().local_device().device_id)
    {
        return Err(DispatchError::unsupported(
            "display.show on a remote computer",
        ));
    }
    silo_engine::macos_computers_engine::open_macos_display(
        macos_context(context),
        params.computer_id,
    )
    .await
    .map_err(map_error)?;
    Ok(output_value(Value::Null))
}

async fn display_close(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::common::ComputerTargetParams = decode(context, value)?;
    if params
        .device_id
        .as_deref()
        .is_some_and(|device| device != context.silod_context().local_device().device_id)
    {
        return Err(DispatchError::unsupported(
            "display.close on a remote computer",
        ));
    }
    silo_engine::macos_computers_engine::close_macos_display(
        macos_context(context),
        params.computer_id,
    )
    .await
    .map_err(map_error)?;
    Ok(output_value(Value::Null))
}

async fn display_viewer_open(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::common::ComputerTargetParams = decode(context, value)?;
    let client = context.client_identity().client_id.clone();
    let local_id = context.silod_context().local_device().device_id.clone();
    if params
        .device_id
        .as_deref()
        .is_none_or(|device| device == local_id)
        && is_macos_computer(context, &params.computer_id).await?
    {
        return Err(DispatchError {
            code: ErrorCode::UnsupportedRemoteOperation,
            message:
                "A macOS computer has no viewer connection: open its screen with display.show."
                    .into(),
        });
    }
    let engine_context: Arc<dyn EngineContext> = Arc::new(context.silod_context().clone());
    let host = runtime_host(context);
    let result = tokio::task::spawn_blocking(move || {
        open_viewer(
            &params.computer_id,
            params.device_id.as_deref(),
            &local_id,
            &client,
            engine_context,
            &host,
        )
    })
    .await
    .map_err(|error| DispatchError::internal(error.to_string()))?
    .map_err(map_error)?;
    output(ViewerOpenResult {
        connection_id: result.connection_id,
        url: result.url,
        cookie: ViewerCookie {
            name: result.cookie_name,
            value: result.cookie_value,
            domain: "127.0.0.1".into(),
            path: "/".into(),
            http_only: true,
        },
        init_script: silo_engine::desktop_viewer_api::INIT_SCRIPT.into(),
        name: result.name,
    })
}

fn open_viewer(
    computer_id: &str,
    target_device: Option<&str>,
    local_device: &str,
    client: &str,
    engine_context: Arc<dyn EngineContext>,
    host: &dyn RuntimeHost,
) -> Result<silo_engine::desktop_viewer_api::OpenResult, String> {
    silo_engine::editor::require_openssh("view computer desktops")?;
    uuid::Uuid::parse_str(computer_id).map_err(|_| "Invalid computer identity.")?;
    let owner_device = target_device.unwrap_or(local_device);
    let paths = if owner_device == local_device {
        Some(runtime::runtime_paths(host)?)
    } else {
        None
    };
    let mut remote_status = None;
    let (connection, name) = if let Some(paths) = &paths {
        let name = runtime::remote_ops::local_computer_name_in(paths, computer_id)?;
        let connection =
            silo_engine::desktop_viewer::local_connection(paths, &name, Some(computer_id))?;
        (connection, name)
    } else {
        let connection = silo_engine::remote::call_remote(
            owner_device,
            "desktop.connect",
            json!({"computerId": computer_id}),
        )?;
        let status = silo_engine::remote::call_remote(
            owner_device,
            "desktop.status",
            json!({"computerId": computer_id}),
        )?;
        let name = status["name"].as_str().unwrap_or(computer_id).to_owned();
        remote_status = Some(status);
        (connection, name)
    };
    let plan = silo_engine::desktop_viewer_api::claim(
        client,
        owner_device,
        computer_id,
        &name,
        owner_device == local_device,
    )?;
    let (connection_id, generation, stale) = match plan {
        silo_engine::desktop_viewer_api::OpenPlan::Existing(result) => return Ok(result),
        silo_engine::desktop_viewer_api::OpenPlan::Connect {
            connection_id,
            generation,
            stale,
        } => (connection_id, generation, stale),
    };
    drop(stale);
    // The first viewer is always allowed. A later one evicts the first on a desktop older than
    // recipe 5, so it is refused unless the status shows the multi-client patch.
    if let Err(message) = silo_engine::desktop_viewer_api::gate_second_viewer(
        client,
        owner_device,
        computer_id,
        || match (&paths, remote_status) {
            (_, Some(status)) => Ok(status),
            (Some(paths), None) => {
                silo_engine::desktop::local(paths, &name, None, Some(computer_id))
            }
            (None, None) => Err("Computer runtime is unavailable.".to_owned()),
        },
    ) {
        silo_engine::desktop_viewer_api::abort(client, &connection_id, generation);
        return Err(message);
    }
    let connect = || -> Result<(silo_engine::desktop_proxy::Proxy, silo_engine::owned_tunnel::Tunnel), String> {
        let port = connection["port"].as_u64().and_then(|port| u16::try_from(port).ok()).filter(|port| *port > 0).ok_or("Invalid desktop endpoint.")?;
        let username = connection["username"].as_str().ok_or("Missing desktop credentials.")?;
        let password = connection["password"].as_str().ok_or("Missing desktop credentials.")?;
        let (alias, config) = if owner_device == local_device {
            let paths = paths.as_ref().ok_or("Computer runtime is unavailable.")?;
            let directory = paths.home.join("ssh/desktop-viewer").join(&name);
            silo_engine::editor::prepare_private_transport(paths, &name, &directory)?
        } else {
            silo_engine::editor::prepare_remote_private(owner_device, computer_id, "/workspace")?
        };
        let home = &engine_context.paths().home;
        let directory = silo_engine::desktop_viewer::socket_directory(&engine_context.identity().channel.state_dir(home))?;
        let socket = directory.path().join("desktop.sock");
        let command = silo_engine::desktop_viewer::forward_command(&config, &alias, &socket, port)?;
        let mut tunnel = silo_engine::owned_tunnel::Tunnel::spawn(&command, Some(directory)).map_err(|_| "Could not connect to the desktop.")?;
        let uid = unsafe { libc::geteuid() };
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if !tunnel.running() { return Err("Desktop tunnel closed. Check the computer connection.".into()); }
            if silo_engine::desktop_viewer::socket_ready(&socket, uid)? { break; }
            if Instant::now() >= deadline { return Err("Desktop connection timed out. Reconnect the computer.".into()); }
            std::thread::sleep(Duration::from_millis(80));
        }
        let proxy = silo_engine::desktop_proxy::Proxy::start(socket, port, username, password)?;
        Ok((proxy, tunnel))
    };
    match connect() {
        Ok((proxy, tunnel)) => match silo_engine::desktop_viewer_api::finish(
            client,
            &connection_id,
            generation,
            proxy,
            tunnel,
            &name,
        ) {
            Ok(result) => Ok(result),
            Err((message, proxy, tunnel)) => {
                drop((proxy, tunnel));
                Err(message)
            }
        },
        Err(message) => {
            silo_engine::desktop_viewer_api::abort(client, &connection_id, generation);
            Err(message)
        }
    }
}

async fn display_viewer_close(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::display::ViewerConnectionParams = decode(context, value)?;
    let client = context.client_identity().client_id.clone();
    // Stopping the tunnel can wait up to half a second, so it runs off the async workers.
    tokio::task::spawn_blocking(move || {
        silo_engine::desktop_viewer_api::close(&client, &params.connection_id)
    })
    .await
    .map_err(|error| DispatchError::internal(error.to_string()))?
    .map_err(map_error)?;
    Ok(output_value(Value::Null))
}

/// Refuses a connection id the calling client does not hold, before any state keyed by it
/// is touched.
fn require_owned(context: &HandlerContext, connection: &str) -> Result<(), DispatchError> {
    silo_engine::desktop_viewer_api::ensure_owned(&context.client_identity().client_id, connection)
        .map_err(map_error)
}

struct ClientPage {
    context: Arc<dyn EngineContext>,
    client: ClientId,
    connection: String,
}

impl Page for ClientPage {
    fn eval(&self, script: &str) -> Result<(), String> {
        let event = ViewerPageEvent {
            connection_id: self.connection.clone(),
            script: script.to_owned(),
        };
        let payload = serde_json::to_value(event).map_err(|error| error.to_string())?;
        self.context.publish(
            Event::new("display.viewer.page", payload),
            Audience::Client(self.client.clone()),
        );
        Ok(())
    }
}

fn with_bridge<R>(
    context: Arc<dyn EngineContext>,
    client: String,
    connection: String,
    work: impl FnOnce(&Bridge<'_>) -> Result<R, String>,
) -> Result<R, String> {
    let ticket = silo_engine::desktop_viewer_api::bridge_ticket(&client, &connection)?;
    let page = ClientPage {
        context,
        client: ClientId::new(client.clone()),
        connection: connection.clone(),
    };
    if !silo_engine::desktop_viewer_api::ticket_is_current(&client, &connection, &ticket) {
        return Err("The desktop reconnected.".into());
    }
    let bridge = Bridge {
        page: &page,
        inbox: &ticket.inbox,
    };
    work(&bridge)
}

async fn run_bridge<R: Send + 'static>(
    context: &HandlerContext,
    connection: String,
    work: impl FnOnce(&Bridge<'_>) -> Result<R, String> + Send + 'static,
) -> Result<R, DispatchError> {
    let client = context.client_identity().client_id.clone();
    let engine: Arc<dyn EngineContext> = Arc::new(context.silod_context().clone());
    tokio::task::spawn_blocking(move || with_bridge(engine, client, connection, work))
        .await
        .map_err(|_| DispatchError::internal("The desktop page worker stopped unexpectedly."))?
        .map_err(map_error)
}

async fn display_viewer_sound_check(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::display::ViewerConnectionParams = decode(context, value)?;
    let connection = params.connection_id;
    require_owned(context, &connection)?;
    let client = context.client_identity().client_id.clone();
    let engine: Arc<dyn EngineContext> = Arc::new(context.silod_context().clone());
    let id = connection.clone();
    let (receiver, start) = silo_engine::desktop_viewer_media::probes()
        .enlist(&connection)
        .map_err(map_error)?;
    if start {
        tokio::task::spawn_blocking(move || {
            silo_engine::desktop_viewer_media::probes().work(
                &id,
                silo_engine::desktop_viewer_media::PROBE_DEADLINE,
                |obsolete| {
                    silo_engine::desktop_viewer_media::probe(
                        || {
                            with_bridge(engine.clone(), client.clone(), id.clone(), |bridge| {
                                silo_engine::desktop_viewer_media::probe_step(bridge)
                            })
                        },
                        silo_engine::desktop_viewer_media::PROBE_ATTEMPTS,
                        silo_engine::desktop_viewer_media::PROBE_PAUSE,
                        obsolete,
                    )
                },
            );
        });
    }
    let sound = receiver
        .await
        .map_err(|_| map_error("The desktop sound check was cancelled."))?
        .map_err(map_error)?;
    output(ViewerSoundCheckResult { sound })
}

async fn display_viewer_sound_cancel(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::display::ViewerConnectionParams = decode(context, value)?;
    require_owned(context, &params.connection_id)?;
    silo_engine::desktop_viewer_media::probes().cancel(&params.connection_id);
    Ok(output_value(Value::Null))
}

async fn display_viewer_audio_set(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::display::ViewerAudioSetParams = decode(context, value)?;
    require_owned(context, &params.connection_id)?;
    let latest = silo_engine::desktop_viewer_media::sound_revision(&params.connection_id)
        .map_err(map_error)?;
    let connection = params.connection_id;
    run_bridge(context, connection, move |bridge| {
        silo_engine::desktop_viewer_media::apply_audio_ordered(
            &latest,
            params.revision,
            bridge,
            params.muted,
            params.active,
        )
    })
    .await?;
    Ok(output_value(Value::Null))
}

async fn display_viewer_screen_reset(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::display::ViewerConnectionParams = decode(context, value)?;
    run_bridge(context, params.connection_id, |bridge| {
        silo_engine::desktop_viewer_media::reset_screen(bridge)
    })
    .await?;
    Ok(output_value(Value::Null))
}

async fn clipboard_transfer(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::clipboard::ClipboardTransferParams = decode(context, value)?;
    if params
        .device_id
        .as_deref()
        .is_some_and(|device| device != context.silod_context().local_device().device_id)
    {
        return Err(DispatchError::unsupported(
            "clipboard.transfer on a remote device",
        ));
    }
    if is_macos_computer(context, &params.computer_id).await? {
        let direction = match params.action {
            ClipboardAction::Copy => {
                silo_engine::macos_computers_engine::ClipboardDirection::CopyFrom
            }
            ClipboardAction::Paste => {
                silo_engine::macos_computers_engine::ClipboardDirection::PasteInto
            }
        };
        let report = silo_engine::macos_computers_engine::macos_computer_clipboard(
            macos_context(context),
            params.computer_id,
            direction,
        )
        .await
        .map_err(map_error)?;
        return clipboard_result(report.action, report.status, report.content, report.message);
    }
    let connection = params.connection_id.ok_or_else(|| {
        DispatchError::internal("Invalid params: connectionId is required for a Linux computer.")
    })?;
    let action = params.action;
    // The viewer must show the computer on the device the request names; a connection id
    // alone does not select the computer a transfer reaches.
    let local_device = context.silod_context().local_device().device_id.clone();
    silo_engine::desktop_viewer_api::ensure_target(
        &context.client_identity().client_id,
        &connection,
        params.device_id.as_deref().unwrap_or(&local_device),
        &params.computer_id,
    )
    .map_err(map_error)?;
    // One transfer per viewer: a second one reports `busy`.
    let label = format!("api-viewer-{connection}");
    let result = run_bridge(context, connection, move |bridge| {
        let Some(_active) = silo_engine::viewer_clipboard::ActiveGuard::acquire(&label) else {
            return Ok(silo_engine::viewer_clipboard::Report::new(
                match action {
                    ClipboardAction::Copy => silo_engine::viewer_clipboard::Action::Copy,
                    ClipboardAction::Paste => silo_engine::viewer_clipboard::Action::Paste,
                },
                silo_engine::viewer_clipboard::Status::Busy,
                None,
            ));
        };
        let device = silo_engine::clipboard::system();
        Ok(match action {
            ClipboardAction::Copy => silo_engine::viewer_clipboard::copy(bridge, device),
            ClipboardAction::Paste => silo_engine::viewer_clipboard::paste(bridge, device),
        })
    })
    .await?;
    clipboard_result(result.action, result.status, result.content, result.message)
}

fn clipboard_result(
    action: silo_engine::viewer_clipboard::Action,
    status: silo_engine::viewer_clipboard::Status,
    content: Option<silo_engine::viewer_clipboard::Content>,
    message: Option<String>,
) -> Result<HandlerOutput, DispatchError> {
    use silo_engine::viewer_clipboard::{Action as A, Content as C, Status as S};
    output(ClipboardTransferResult {
        action: match action {
            A::Copy => ClipboardAction::Copy,
            A::Paste => ClipboardAction::Paste,
        },
        status: match status {
            S::Pasted => ClipboardTransferStatus::Pasted,
            S::Copied => ClipboardTransferStatus::Copied,
            S::DeviceEmpty => ClipboardTransferStatus::DeviceEmpty,
            S::ComputerEmpty => ClipboardTransferStatus::ComputerEmpty,
            S::TooLarge => ClipboardTransferStatus::TooLarge,
            S::NotConnected => ClipboardTransferStatus::NotConnected,
            S::Unsupported => ClipboardTransferStatus::Unsupported,
            S::Busy => ClipboardTransferStatus::Busy,
            S::Failed => ClipboardTransferStatus::Failed,
        },
        content: content.map(|content| match content {
            C::Text => ClipboardTransferContent::Text,
            C::Image => ClipboardTransferContent::Image,
        }),
        message,
    })
}

async fn clipboard_read(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::common::ComputerTargetParams = decode(context, value)?;
    if params
        .device_id
        .as_deref()
        .is_some_and(|device| device != context.silod_context().local_device().device_id)
    {
        return Err(DispatchError::unsupported(
            "clipboard.read on a remote device",
        ));
    }
    silo_engine::macos_computers_engine::macos_clipboard_read(
        macos_context(context),
        params.computer_id,
    )
    .await
    .map_err(map_error)
    .and_then(output)
}

async fn clipboard_write(
    context: &HandlerContext,
    value: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: silo_api::methods::clipboard::ClipboardWriteParams = decode(context, value)?;
    if params
        .device_id
        .as_deref()
        .is_some_and(|device| device != context.silod_context().local_device().device_id)
    {
        return Err(DispatchError::unsupported(
            "clipboard.write on a remote device",
        ));
    }
    silo_engine::macos_computers_engine::macos_clipboard_write(
        macos_context(context),
        params.computer_id,
        params.content,
    )
    .await
    .map_err(map_error)?;
    Ok(output_value(Value::Null))
}

/// How often a relayed remote call looks at the job's cancellation flag.
const REMOTE_CANCEL_POLL: Duration = Duration::from_millis(50);

/// Runs a call to the computer's owner for a job. A job for a remote computer never enters
/// this device's gate, so `operations.cancel` reaches it only through the job's flag: a
/// cancellation before the call starts it never makes, and one while it runs ends the job
/// as cancelled at once. The owner's bridge has no cancel method yet, so a request it has
/// already accepted runs to its end there; its result is dropped.
fn relay_remote(
    link: &silo_engine::desktop::JobLink,
    call: impl FnOnce() -> Result<Value, String> + Send + 'static,
) -> Result<Value, String> {
    relay_cancellable(&link.cancellation, REMOTE_CANCEL_POLL, call, || {
        link.registry
            .mark_running(&link.job_id)
            .map_err(|error| error.to_string())
    })
}

fn relay_cancellable(
    cancellation: &AtomicBool,
    poll: Duration,
    call: impl FnOnce() -> Result<Value, String> + Send + 'static,
    start: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    let cancelled = || "The operation was cancelled.".to_owned();
    if cancellation.load(Ordering::SeqCst) {
        return Err(cancelled());
    }
    start()?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("desktop-remote-relay".into())
        .spawn(move || {
            let _ = sender.send(call());
        })
        .map_err(|error| error.to_string())?;
    loop {
        match receiver.recv_timeout(poll) {
            Ok(result) => return result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if cancellation.load(Ordering::SeqCst) {
                    return Err(cancelled());
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("The remote action stopped unexpectedly.".to_owned());
            }
        }
    }
}

/// Starts a durable job that runs `work` on a blocking thread and publishes its outcome.
/// `work` receives the job's link; it marks the job running when its queue turn arrives.
async fn start_job(
    context: &HandlerContext,
    method: &'static str,
    work: impl FnOnce(&silo_engine::desktop::JobLink) -> Result<Value, String> + Send + 'static,
) -> Result<HandlerOutput, DispatchError> {
    let accepted = context.start_job(method)?;
    let job_id = accepted.job_id.clone();
    let registry = context.job_registry()?;
    let cancellation = Arc::new(AtomicBool::new(false));
    let cancel_id = job_id.clone();
    let cancel_flag = cancellation.clone();
    registry.register_cancel(
        job_id.clone(),
        silo_engine::jobs::CancelRoute::JobGate,
        move || {
            // Before the gate admits the work (or when it never enters the gate, as for a
            // remote computer) the request is recorded in the job's flag, which the gate
            // admission and the remote relay both observe. That accepts the cancellation,
            // so it is reported as accepted. Once the work holds a gate entry, the gate
            // decides.
            if !silo_engine::runtime::OPERATIONS.contains_job(&cancel_id) {
                cancel_flag.store(true, Ordering::SeqCst);
            }
            silo_engine::runtime::OPERATIONS
                .cancel_job(&cancel_id)
                .map_err(|error| error.to_string())
        },
    );
    let engine: Arc<dyn EngineContext> = Arc::new(context.silod_context().clone());
    let link = silo_engine::desktop::JobLink {
        registry,
        job_id: job_id.clone(),
        cancellation,
    };
    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || work(&link))
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result);
        let finished = match result {
            Ok(result) => JobFinished {
                job_id: job_id.clone(),
                result: Some(result),
                error: None,
                outcome_chunked: false,
            },
            Err(message) => JobFinished {
                job_id: job_id.clone(),
                result: None,
                error: Some(ErrorObject {
                    code: map_error(&message).code,
                    message,
                    diagnostics: None,
                }),
                outcome_chunked: false,
            },
        };
        let _ = silo_engine::jobs::publish_finished(engine.as_ref(), finished, Audience::All);
    });
    output(accepted)
}

/// The parts of showing a native window that do not need AppKit: the protocol with the main
/// thread and the window table with its rollback. They are separate so tests can inject a
/// failing activation policy and a main thread that runs late.
mod window_core {
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicU8, Ordering},
            Arc,
        },
        time::Duration,
    };

    const PENDING: u8 = 0;
    const RUNNING: u8 = 1;
    const ABANDONED: u8 = 2;

    /// One piece of work handed to the main thread. Either the main thread starts it or the
    /// caller gives up on it, never both: work the caller gave up on is skipped when the
    /// main thread reaches it, so it cannot create a window nobody is waiting for.
    #[derive(Default)]
    pub(super) struct MainRequest {
        state: AtomicU8,
    }

    impl MainRequest {
        /// The main thread asks to start. False when the caller gave up.
        pub(super) fn begin(&self) -> bool {
            self.state
                .compare_exchange(PENDING, RUNNING, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        }

        /// The caller gives up. False when the main thread already started: its result is
        /// then awaited, since the work is under way and will report.
        pub(super) fn abandon(&self) -> bool {
            self.state
                .compare_exchange(PENDING, ABANDONED, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        }
    }

    /// Runs `work` on the main thread through `run` and returns its result. After `timeout`
    /// without a start the work is cancelled and the call fails; work that has started is
    /// waited for.
    pub(super) fn run_on_main<T: Send + 'static>(
        run: impl FnOnce(Box<dyn FnOnce() + Send>) -> Result<(), String>,
        timeout: Duration,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, String> {
        let request = Arc::new(MainRequest::default());
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let main_request = request.clone();
        run(Box::new(move || {
            if main_request.begin() {
                let _ = sender.send(work());
            }
        }))?;
        match receiver.recv_timeout(timeout) {
            Ok(result) => Ok(result),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if request.abandon() {
                    Err("Timed out waiting for the AppKit display window.".to_owned())
                } else {
                    receiver
                        .recv()
                        .map_err(|_| "The AppKit display window work stopped.".to_owned())
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err("The AppKit display window work was dropped.".to_owned())
            }
        }
    }

    pub(super) enum Shown<W> {
        Existing(W),
        Created(W),
    }

    /// Shows the window of `id`: the table's own when there is one, else a created one that is
    /// entered into the table before the activation policy follows it. When the policy
    /// refuses, the window leaves the table again and is discarded, and the policy is set
    /// for the windows that remain, so a failed show leaves nothing behind.
    pub(super) fn show_window<W: Clone>(
        windows: &mut HashMap<String, W>,
        id: &str,
        create: impl FnOnce() -> W,
        set_policy: &dyn Fn(bool) -> Result<(), String>,
        discard: impl FnOnce(W),
    ) -> Result<Shown<W>, String> {
        if let Some(window) = windows.get(id) {
            return Ok(Shown::Existing(window.clone()));
        }
        let window = create();
        windows.insert(id.to_owned(), window.clone());
        if let Err(error) = set_policy(true) {
            windows.remove(id);
            discard(window);
            let _ = set_policy(!windows.is_empty());
            return Err(error);
        }
        Ok(Shown::Created(window))
    }
}

#[cfg(target_os = "macos")]
mod native_display {
    use objc2::{
        define_class, msg_send, rc::Retained, runtime::ProtocolObject, DefinedClass,
        MainThreadMarker, MainThreadOnly,
    };
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
    };
    use objc2_foundation::{
        NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
    };
    use std::{cell::RefCell, collections::HashMap};

    struct WindowState {
        context: silo_engine::macos_computers_engine::MacosContext,
        id: String,
    }
    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "SilodMacosDisplayDelegate"]
        #[thread_kind = MainThreadOnly]
        #[ivars = WindowState]
        struct DisplayDelegate;
        unsafe impl NSObjectProtocol for DisplayDelegate {}
        unsafe impl NSWindowDelegate for DisplayDelegate {}
        impl DisplayDelegate {
            #[unsafe(method(windowWillClose:))]
            fn window_will_close(&self, _notification: &NSNotification) {
                let state = self.ivars();
                // The delegate is dropped with its entry, and this method still reads its
                // state: keep the entry alive until the end of the callback.
                let _entry = WINDOWS.with(|windows| windows.borrow_mut().remove(&state.id));
                if let Some(mtm) = MainThreadMarker::new() {
                    let open = WINDOWS.with(|windows| !windows.borrow().is_empty());
                    let _ = apply_policy(mtm, open);
                }
                silo_engine::macos_computers_engine::display_window_closed(
                    &state.context,
                    &state.id,
                );
            }
        }
    );

    impl DisplayDelegate {
        fn new(
            mtm: MainThreadMarker,
            context: silo_engine::macos_computers_engine::MacosContext,
            id: String,
        ) -> Retained<Self> {
            let this = mtm.alloc::<Self>().set_ivars(WindowState { context, id });
            unsafe { msg_send![super(this), init] }
        }
    }

    thread_local! {
        static WINDOWS: RefCell<HashMap<String, (Retained<NSWindow>, Retained<DisplayDelegate>)>> = RefCell::new(HashMap::new());
    }

    /// Follows the activation policy to whether any window is open.
    fn apply_policy(mtm: MainThreadMarker, open: bool) -> Result<(), String> {
        let policy = crate::app::activation_policy(open);
        let application = NSApplication::sharedApplication(mtm);
        if application.activationPolicy() == policy || application.setActivationPolicy(policy) {
            Ok(())
        } else {
            Err("NSApplication refused the display window activation policy".into())
        }
    }

    /// Builds the window of computer `id` with its delegate.
    fn build_window(
        mtm: MainThreadMarker,
        context: &silo_engine::macos_computers_engine::MacosContext,
        id: &str,
        title: &str,
    ) -> (Retained<NSWindow>, Retained<DisplayDelegate>) {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // The registry owns the window; AppKit must not release it again on close.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(title));
        window.setMinSize(NSSize::new(640.0, 400.0));
        let content = NSView::initWithFrame(mtm.alloc(), frame);
        window.setContentView(Some(&content));
        let delegate = DisplayDelegate::new(mtm, context.clone(), id.to_owned());
        window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        (window, delegate)
    }

    pub(super) fn show(
        context: &silo_engine::macos_computers_engine::MacosContext,
        id: &str,
        title: &str,
        toolbar: bool,
    ) -> Result<(), String> {
        let context = context.clone();
        let id = id.to_owned();
        let title = title.to_owned();
        let engine =
            silo_engine::context::current().ok_or("The engine context is not installed.")?;
        let main = engine
            .main_thread()
            .ok_or("The AppKit main-thread runner is unavailable.")?;
        let (id_for_main, context_for_main) = (id.clone(), context.clone());
        // The main thread skips this work when the wait below gave up, so a late run never
        // creates a window that no caller attached.
        super::window_core::run_on_main(
            |work| main.run(work),
            std::time::Duration::from_secs(10),
            move || -> Result<(), String> {
                let mtm = MainThreadMarker::new()
                    .ok_or("Could not open the display on AppKit's main thread.")?;
                let shown = WINDOWS.with(|windows| {
                    super::window_core::show_window(
                        &mut windows.borrow_mut(),
                        &id_for_main,
                        || build_window(mtm, &context_for_main, &id_for_main, &title),
                        &|open| apply_policy(mtm, open),
                        |(window, _delegate)| {
                            // Without its delegate the close reports nothing back.
                            window.setDelegate(None);
                            window.close();
                        },
                    )
                })?;
                let (super::window_core::Shown::Existing((window, _))
                | super::window_core::Shown::Created((window, _))) = shown;
                #[allow(deprecated)]
                NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
                window.makeKeyAndOrderFront(None);
                Ok(())
            },
        )??;

        let resolving_id = id.clone();
        if let Err(error) = silo_engine::macos_computers_engine::attach_display(
            &context,
            &id,
            move || window_ptr(&resolving_id),
            toolbar,
        ) {
            close(&id);
            return Err(error);
        }
        Ok(())
    }

    fn window_ptr(id: &str) -> Result<usize, String> {
        WINDOWS.with(|windows| {
            windows
                .borrow()
                .get(id)
                .map(|(window, _)| &**window as *const NSWindow as usize)
                .ok_or_else(|| "Silo could not open the display.".into())
        })
    }

    pub(super) fn close(id: &str) {
        let id = id.to_owned();
        if let Some(engine) = silo_engine::context::current() {
            if let Some(main) = engine.main_thread() {
                let _ = main.run(Box::new(move || {
                    let window = WINDOWS.with(|windows| {
                        windows.borrow().get(&id).map(|(window, _)| window.clone())
                    });
                    if let Some(window) = window {
                        window.performClose(None);
                    }
                }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silo_api::{
        jobs::JobState,
        methods::{method_registry, MethodName},
    };
    use silo_engine::{
        channel::Channel,
        computer_use::Approval,
        context::{EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl},
        events::EventDelivery,
        macos_computers_engine::{install_test_computer, TestComputerState},
    };

    use crate::{context::SilodContext, dispatcher::RequestContext, identity::LocalDeviceIdentity};

    struct NoMain;
    impl MainThread for NoMain {
        fn run(&self, _work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
            Ok(())
        }
    }
    struct NoProcess;
    impl ProcessControl for NoProcess {
        fn exit(&self, _code: i32) {}
        fn restart(&self) -> ! {
            panic!("restart is not expected")
        }
    }

    struct Fixture {
        _registry: std::sync::MutexGuard<'static, ()>,
        _gate_jobs: std::sync::MutexGuard<'static, ()>,
        directory: tempfile::TempDir,
        engine: Arc<SilodContext>,
        _gate: crate::methods::ProcessGateLease,
    }

    fn fixture() -> Fixture {
        let gate = crate::methods::process_gate_lock();
        let registry = crate::methods::MACOS_REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let engine = Arc::new(SilodContext::new(
            EnginePaths {
                home: root.join("home"),
                app_data: root.join("data"),
                resources: root.join("resources"),
                ..EnginePaths::default()
            },
            EngineIdentity {
                channel: Channel::Development,
                version: "0.1.0".into(),
                package_kind: PackageKind::MacApp,
            },
            LocalDeviceIdentity {
                device_id: "local-device".into(),
                name: "Local Mac".into(),
            },
            Arc::new(NoMain),
            Arc::new(NoProcess),
        ));
        Fixture {
            _registry: registry,
            _gate_jobs: crate::serialize_gate_jobs(),
            directory,
            engine,
            _gate: gate,
        }
    }

    impl Fixture {
        fn data(&self) -> std::path::PathBuf {
            self.directory.path().join("data")
        }

        fn context(&self, client: &str) -> HandlerContext {
            HandlerContext::new(
                self.engine.clone(),
                RequestContext {
                    client_id: client.into(),
                    protocol: 6,
                    client: None,
                },
            )
        }

        async fn call(
            &self,
            client: &str,
            method: MethodName,
            params: Value,
        ) -> Result<Value, DispatchError> {
            let context = self.context(client);
            handle(method, &params, &context)
                .await
                .expect("the display module owns this method")
                .map(|output| output.value)
        }

        fn macos(&self, state: TestComputerState, approval: Approval) -> String {
            install_test_computer(&self.data(), "mac-fixture", state, approval)
        }

        /// Waits for a job's terminal state and returns its error code, if any.
        async fn finished_error(&self, started: Value) -> Option<ErrorCode> {
            let job_id: silo_api::jobs::JobId =
                serde_json::from_value(started["jobId"].clone()).unwrap();
            let registry = self.engine.job_registry().unwrap();
            for _ in 0..200 {
                if let Some(found) = registry.get(&job_id).unwrap() {
                    if found.state == JobState::Finished {
                        return found
                            .outcome
                            .and_then(|outcome| outcome.error)
                            .map(|e| e.code);
                    }
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            panic!("the job did not finish");
        }
    }

    fn unknown() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    /// Registers a healthy viewer connection the way a finished `display.viewer.open` does,
    /// without a guest: a proxy on a loopback port and a tunnel process that only sleeps.
    fn hold_viewer(client: &str, computer: &str) -> (String, String, String, u16) {
        hold_viewer_on(client, "local-device", computer)
    }

    /// Like [`hold_viewer`], for a computer of `device`.
    fn hold_viewer_on(client: &str, device: &str, computer: &str) -> (String, String, String, u16) {
        use silo_engine::desktop_viewer_api::{claim, finish, open_client, OpenPlan};
        open_client(client);
        let OpenPlan::Connect {
            connection_id,
            generation,
            ..
        } = claim(
            client,
            device,
            computer,
            "Fixture",
            device == "local-device",
        )
        .unwrap()
        else {
            panic!("expected a new connection");
        };
        let proxy = silo_engine::desktop_proxy::Proxy::start(
            "/nonexistent/desktop.sock".into(),
            6901,
            "silo",
            "secret",
        )
        .unwrap();
        let port = proxy.port;
        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("60");
        let tunnel = {
            let _spawning = crate::FORK_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            silo_engine::owned_tunnel::Tunnel::spawn(&command, None).unwrap()
        };
        let opened = finish(client, &connection_id, generation, proxy, tunnel, "Fixture")
            .unwrap_or_else(|(message, _, _)| panic!("{message}"));
        (connection_id, opened.cookie_name, opened.cookie_value, port)
    }

    async fn code(result: Result<Value, DispatchError>) -> ErrorCode {
        result.expect_err("the call must be refused").code
    }

    #[test]
    fn owns_all_four_domain_prefixes() {
        for descriptor in method_registry() {
            let name = descriptor.name;
            if ["display.", "desktop.", "computerUse.", "clipboard."]
                .iter()
                .any(|prefix| name.starts_with(prefix))
            {
                assert!(owns(descriptor.method), "{name} is not owned");
            }
        }
        assert!(!owns(MethodName::ComputersList));
    }

    #[test]
    fn errors_map_to_stable_codes() {
        for (message, expected) in [
            ("This computer no longer exists.", ErrorCode::NotFound),
            ("Unknown desktop viewer.", ErrorCode::NotFound),
            ("The desktop is not running.", ErrorCode::NotRunning),
            ("This computer isn't running.", ErrorCode::NotRunning),
            ("Desktop connection timed out.", ErrorCode::TimedOut),
            ("The desktop is still connecting.", ErrorCode::Busy),
            ("The operation was cancelled.", ErrorCode::Cancelled),
            (
                "This action is already waiting to run.",
                ErrorCode::AlreadyQueued,
            ),
            (
                "OpenSSH is required to view computer desktops.",
                ErrorCode::OpenSshMissing,
            ),
            (
                silo_engine::desktop_viewer_api::SECOND_VIEWER_REFUSAL,
                ErrorCode::DesktopUpdateRequired,
            ),
            ("Something else broke.", ErrorCode::Internal),
        ] {
            assert_eq!(map_error(message).code, expected, "{message}");
        }
    }

    #[tokio::test]
    async fn desktop_status_projects_macos_computers_and_refuses_unknown_ones() {
        let fixture = fixture();
        let id = fixture.macos(TestComputerState::Running, Approval::Ask);
        let status = fixture
            .call("a", MethodName::DesktopStatus, json!({"computerId": id}))
            .await
            .unwrap();
        assert_eq!(status["state"], "running");
        assert_eq!(status["sessionState"], "running");
        assert_eq!(status["name"], "mac-fixture");
        assert_eq!(status["computerUse"]["approval"], "ask");
        let unknown = fixture
            .call(
                "a",
                MethodName::DesktopStatus,
                json!({"computerId": unknown()}),
            )
            .await;
        assert_eq!(code(unknown).await, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn desktop_connect_is_refused_for_every_local_client() {
        let fixture = fixture();
        let id = fixture.macos(TestComputerState::Running, Approval::Ask);
        for computer in [id, unknown()] {
            let refused = fixture
                .call(
                    "a",
                    MethodName::DesktopConnect,
                    json!({"computerId": computer}),
                )
                .await;
            assert_eq!(code(refused).await, ErrorCode::PermissionDenied);
        }
    }

    #[tokio::test]
    async fn desktop_action_is_a_job_that_fails_for_an_unknown_computer() {
        let fixture = fixture();
        let macos = fixture.macos(TestComputerState::Running, Approval::Ask);
        let refused = fixture
            .call(
                "a",
                MethodName::DesktopAction,
                json!({"computerId": macos, "action": "start"}),
            )
            .await;
        assert_eq!(code(refused).await, ErrorCode::UnsupportedRemoteOperation);
        let started = fixture
            .call(
                "a",
                MethodName::DesktopAction,
                json!({"computerId": unknown(), "action": "restart-streamer"}),
            )
            .await
            .unwrap();
        assert!(started["jobId"].as_str().unwrap().starts_with("job:"));
        assert_eq!(
            fixture.finished_error(started).await,
            Some(ErrorCode::NotFound)
        );
    }

    /// Starts a job whose work waits for `release`, and reports on `started` that it runs.
    async fn blocked_job(
        fixture: &Fixture,
    ) -> (
        silo_api::jobs::JobId,
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::Sender<()>,
    ) {
        let (started, running) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let accepted = start_job(&fixture.context("a"), "desktop.action", move |link| {
            started.send(()).unwrap();
            released.recv().unwrap();
            if link.cancellation.load(Ordering::SeqCst) {
                Err("The operation was cancelled.".to_owned())
            } else {
                Ok(Value::Null)
            }
        })
        .await
        .unwrap();
        let job_id = serde_json::from_value(accepted.value["jobId"].clone()).unwrap();
        (job_id, running, release)
    }

    /// The error code of the next finished job.
    async fn next_finished(events: &mut silo_engine::events::EventReceiver) -> (String, Value) {
        let Some(EventDelivery::Event(event)) =
            tokio::time::timeout(Duration::from_secs(5), events.recv())
                .await
                .unwrap()
        else {
            panic!("expected a job event");
        };
        assert_eq!(event.event.name, "operations.job.finished");
        let payload = event.event.payload;
        (payload["jobId"].as_str().unwrap().to_owned(), payload)
    }

    fn watch_finished_jobs(fixture: &Fixture) -> silo_engine::events::EventReceiver {
        let watcher = silo_engine::context::ClientId::new("job-watcher");
        let events = fixture.engine.connect_event_stream(watcher.clone());
        let subscription = fixture
            .engine
            .subscribe_events(&watcher, vec!["operations.job.finished".into()])
            .unwrap();
        fixture
            .engine
            .activate_events(&watcher, &subscription)
            .unwrap();
        events
    }

    #[tokio::test]
    async fn cancelling_a_job_that_has_not_been_admitted_is_accepted_and_ends_it_cancelled() {
        let fixture = fixture();
        let mut events = watch_finished_jobs(&fixture);
        let (job_id, running, release) = blocked_job(&fixture).await;
        tokio::task::spawn_blocking(move || running.recv().unwrap())
            .await
            .unwrap();
        // The work is not in the gate (it may never enter it): the request is accepted, not
        // reported as refused while it takes effect.
        fixture
            .engine
            .job_registry()
            .unwrap()
            .cancel(&job_id)
            .expect("the cancellation is accepted");
        release.send(()).unwrap();
        let (finished, payload) = next_finished(&mut events).await;
        assert_eq!(finished, job_id.as_str());
        assert_eq!(payload["error"]["code"], "cancelled", "{payload}");
    }

    #[test]
    fn a_relayed_remote_call_observes_the_cancellation_flag() {
        let calls = Arc::new(AtomicBool::new(false));
        let poll = Duration::from_millis(1);
        // Cancelled before it starts: the call is never made.
        let cancelled = AtomicBool::new(true);
        let made = calls.clone();
        let result = relay_cancellable(
            &cancelled,
            poll,
            move || {
                made.store(true, Ordering::SeqCst);
                Ok(Value::Null)
            },
            || panic!("a cancelled job does not start"),
        );
        assert!(result.unwrap_err().contains("was cancelled"));
        assert!(!calls.load(Ordering::SeqCst));
        // A call that finishes returns its value.
        let flag = AtomicBool::new(false);
        let result = relay_cancellable(&flag, poll, || Ok(json!({"state": "running"})), || Ok(()));
        assert_eq!(result.unwrap(), json!({"state": "running"}));
        // Cancelled while the call is outstanding: the job ends at once, whatever the call
        // does afterwards.
        let (entered, inside) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let flag = Arc::new(AtomicBool::new(false));
        let waiter = {
            let flag = flag.clone();
            std::thread::spawn(move || {
                relay_cancellable(
                    &flag,
                    poll,
                    move || {
                        entered.send(()).unwrap();
                        let _ = released.recv();
                        Ok(Value::Null)
                    },
                    || Ok(()),
                )
            })
        };
        inside.recv().unwrap();
        flag.store(true, Ordering::SeqCst);
        let result = waiter.join().unwrap();
        assert!(result.unwrap_err().contains("was cancelled"));
        release.send(()).unwrap();
    }

    #[tokio::test]
    async fn a_remote_job_is_cancellable_and_ends_cancelled() {
        let fixture = fixture();
        let mut events = watch_finished_jobs(&fixture);
        let (entered, inside) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let accepted = start_job(&fixture.context("a"), "desktop.action", move |link| {
            relay_cancellable(
                &link.cancellation,
                Duration::from_millis(1),
                move || {
                    entered.send(()).unwrap();
                    let _ = released.recv();
                    Ok(Value::Null)
                },
                || Ok(()),
            )
        })
        .await
        .unwrap();
        let job_id: silo_api::jobs::JobId =
            serde_json::from_value(accepted.value["jobId"].clone()).unwrap();
        tokio::task::spawn_blocking(move || inside.recv().unwrap())
            .await
            .unwrap();
        fixture
            .engine
            .job_registry()
            .unwrap()
            .cancel(&job_id)
            .expect("a remote job accepts cancellation");
        let (finished, payload) = next_finished(&mut events).await;
        assert_eq!(finished, job_id.as_str());
        assert_eq!(payload["error"]["code"], "cancelled", "{payload}");
        release.send(()).unwrap();
    }

    #[tokio::test]
    async fn computer_use_state_reads_the_macos_record() {
        let fixture = fixture();
        let id = fixture.macos(TestComputerState::Stopped, Approval::Auto);
        let state = fixture
            .call("a", MethodName::ComputerUseState, json!({"computerId": id}))
            .await
            .unwrap();
        assert_eq!(state["approval"], "auto");
        assert_eq!(state["appliedApproval"], "auto");
        assert_eq!(state["approvalApply"], "applied");
        assert!(state["state"].is_null());
        let missing = fixture
            .call(
                "a",
                MethodName::ComputerUseState,
                json!({"computerId": unknown()}),
            )
            .await;
        assert_eq!(code(missing).await, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn computer_use_approval_stores_the_desired_mode_of_a_macos_computer() {
        let fixture = fixture();
        let id = fixture.macos(TestComputerState::Stopped, Approval::Ask);
        let state = fixture
            .call(
                "a",
                MethodName::ComputerUseApproval,
                json!({"computerId": id, "mode": "auto"}),
            )
            .await
            .unwrap();
        assert_eq!(state["approval"], "auto");
        assert_eq!(state["appliedApproval"], "unknown");
        assert_eq!(state["approvalApply"], "pending");
        let read = fixture
            .call("a", MethodName::ComputerUseState, json!({"computerId": id}))
            .await
            .unwrap();
        assert_eq!(read, state);
        let missing = fixture
            .call(
                "a",
                MethodName::ComputerUseApproval,
                json!({"computerId": unknown(), "mode": "ask"}),
            )
            .await;
        assert_eq!(code(missing).await, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn computer_use_setup_is_a_job_for_linux_and_refused_for_macos() {
        let fixture = fixture();
        let macos = fixture.macos(TestComputerState::Running, Approval::Ask);
        let refused = fixture
            .call(
                "a",
                MethodName::ComputerUseSetup,
                json!({"computerId": macos}),
            )
            .await;
        assert_eq!(code(refused).await, ErrorCode::UnsupportedRemoteOperation);
        let started = fixture
            .call(
                "a",
                MethodName::ComputerUseSetup,
                json!({"computerId": unknown()}),
            )
            .await
            .unwrap();
        assert_eq!(
            fixture.finished_error(started).await,
            Some(ErrorCode::NotFound)
        );
    }

    #[tokio::test]
    async fn display_show_refuses_remote_and_unusable_computers() {
        let fixture = fixture();
        let remote = fixture
            .call(
                "a",
                MethodName::DisplayShow,
                json!({"computerId": unknown(), "deviceId": "other-device"}),
            )
            .await;
        assert_eq!(code(remote).await, ErrorCode::UnsupportedRemoteOperation);
        // A stopped computer has no screen to show. An unsigned test build refuses first.
        let id = fixture.macos(TestComputerState::Stopped, Approval::Ask);
        let stopped = fixture
            .call("a", MethodName::DisplayShow, json!({"computerId": id}))
            .await
            .unwrap_err();
        assert!(
            matches!(stopped.code, ErrorCode::NotRunning) || stopped.message.contains("signed"),
            "{stopped:?}"
        );
    }

    #[tokio::test]
    async fn display_close_is_idempotent_for_running_computers_and_refused_during_setup() {
        let fixture = fixture();
        let running = fixture.macos(TestComputerState::Running, Approval::Ask);
        for _ in 0..2 {
            let closed = fixture
                .call(
                    "a",
                    MethodName::DisplayClose,
                    json!({"computerId": running}),
                )
                .await
                .unwrap();
            assert!(closed.is_null());
        }
        let setting_up = fixture.macos(TestComputerState::SettingUp, Approval::Ask);
        let refused = fixture
            .call(
                "a",
                MethodName::DisplayClose,
                json!({"computerId": setting_up}),
            )
            .await;
        assert_eq!(code(refused).await, ErrorCode::Busy);
        let missing = fixture
            .call(
                "a",
                MethodName::DisplayClose,
                json!({"computerId": unknown()}),
            )
            .await;
        assert_eq!(code(missing).await, ErrorCode::NotFound);
        let remote = fixture
            .call(
                "a",
                MethodName::DisplayClose,
                json!({"computerId": unknown(), "deviceId": "other-device"}),
            )
            .await;
        assert_eq!(code(remote).await, ErrorCode::UnsupportedRemoteOperation);
    }

    #[tokio::test]
    async fn display_viewer_open_validates_the_target() {
        let fixture = fixture();
        let macos = fixture.macos(TestComputerState::Running, Approval::Ask);
        let refused = fixture
            .call(
                "a",
                MethodName::DisplayViewerOpen,
                json!({"computerId": macos}),
            )
            .await;
        assert_eq!(code(refused).await, ErrorCode::UnsupportedRemoteOperation);
        let invalid = fixture
            .call(
                "a",
                MethodName::DisplayViewerOpen,
                json!({"computerId": "not-a-uuid"}),
            )
            .await;
        assert_eq!(code(invalid).await, ErrorCode::Internal);
        let missing = fixture
            .call(
                "a",
                MethodName::DisplayViewerOpen,
                json!({"computerId": unknown()}),
            )
            .await;
        assert_eq!(code(missing).await, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn display_viewer_close_belongs_to_the_opening_client() {
        let fixture = fixture();
        let (connection, ..) = hold_viewer("close-owner", &unknown());
        let params = json!({"connectionId": connection});
        let other = fixture
            .call(
                "close-other",
                MethodName::DisplayViewerClose,
                params.clone(),
            )
            .await;
        assert_eq!(code(other).await, ErrorCode::NotFound);
        assert!(silo_engine::desktop_viewer_api::ensure_owned("close-owner", &connection).is_ok());
        let closed = fixture
            .call(
                "close-owner",
                MethodName::DisplayViewerClose,
                params.clone(),
            )
            .await
            .unwrap();
        assert!(closed.is_null());
        let again = fixture
            .call("close-owner", MethodName::DisplayViewerClose, params)
            .await;
        assert_eq!(code(again).await, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn display_viewer_sound_check_requires_an_owned_connection() {
        let fixture = fixture();
        let (connection, ..) = hold_viewer("sound-owner", &unknown());
        let params = json!({"connectionId": connection});
        let other = fixture
            .call("sound-other", MethodName::DisplayViewerSoundCheck, params)
            .await;
        assert_eq!(code(other).await, ErrorCode::NotFound);
        let missing = fixture
            .call(
                "sound-owner",
                MethodName::DisplayViewerSoundCheck,
                json!({"connectionId": "missing"}),
            )
            .await;
        assert_eq!(code(missing).await, ErrorCode::NotFound);
        silo_engine::desktop_viewer_api::close("sound-owner", &connection).unwrap();
    }

    #[tokio::test]
    async fn display_viewer_sound_cancel_requires_an_owned_connection() {
        let fixture = fixture();
        let (connection, ..) = hold_viewer("cancel-owner", &unknown());
        let params = json!({"connectionId": connection});
        let other = fixture
            .call(
                "cancel-other",
                MethodName::DisplayViewerSoundCancel,
                params.clone(),
            )
            .await;
        assert_eq!(code(other).await, ErrorCode::NotFound);
        let cancelled = fixture
            .call("cancel-owner", MethodName::DisplayViewerSoundCancel, params)
            .await
            .unwrap();
        assert!(cancelled.is_null());
        silo_engine::desktop_viewer_api::close("cancel-owner", &connection).unwrap();
    }

    #[tokio::test]
    async fn display_viewer_audio_set_reaches_only_the_owner_page() {
        let fixture = fixture();
        let owner = silo_engine::context::ClientId::new("audio-owner");
        let mut pages = fixture.engine.connect_event_stream(owner.clone());
        let subscription = fixture
            .engine
            .subscribe_events(&owner, vec!["display.viewer.page".into()])
            .unwrap();
        fixture
            .engine
            .activate_events(&owner, &subscription)
            .unwrap();
        let (connection, ..) = hold_viewer("audio-owner", &unknown());
        let params =
            json!({"connectionId": connection, "muted": true, "active": false, "revision": 1});
        let other = fixture
            .call(
                "audio-other",
                MethodName::DisplayViewerAudioSet,
                params.clone(),
            )
            .await;
        assert_eq!(code(other).await, ErrorCode::NotFound);
        let applied = fixture
            .call("audio-owner", MethodName::DisplayViewerAudioSet, params)
            .await
            .unwrap();
        assert!(applied.is_null());
        let mut scripts = Vec::new();
        for _ in 0..2 {
            let Some(EventDelivery::Event(event)) =
                tokio::time::timeout(Duration::from_secs(2), pages.recv())
                    .await
                    .unwrap()
            else {
                panic!("expected a page event");
            };
            assert_eq!(event.event.name, "display.viewer.page");
            assert_eq!(event.event.payload["connectionId"], connection);
            scripts.push(event.event.payload["script"].as_str().unwrap().to_owned());
        }
        assert_eq!(
            scripts,
            [
                "window.__silo&&window.__silo.invoke(\"setMute\",[true]);",
                "window.__silo&&window.__silo.invoke(\"setAudioActive\",[false]);",
            ]
        );
        silo_engine::desktop_viewer_api::close("audio-owner", &connection).unwrap();
    }

    #[tokio::test]
    async fn display_viewer_screen_reset_requires_an_owned_connection() {
        let fixture = fixture();
        let (connection, ..) = hold_viewer("reset-owner", &unknown());
        let other = fixture
            .call(
                "reset-other",
                MethodName::DisplayViewerScreenReset,
                json!({"connectionId": connection}),
            )
            .await;
        assert_eq!(code(other).await, ErrorCode::NotFound);
        silo_engine::desktop_viewer_api::close("reset-owner", &connection).unwrap();
        let closed = fixture
            .call(
                "reset-owner",
                MethodName::DisplayViewerScreenReset,
                json!({"connectionId": connection}),
            )
            .await;
        assert_eq!(code(closed).await, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn clipboard_transfer_needs_a_viewer_for_linux_and_never_serves_remote_devices() {
        let fixture = fixture();
        let remote = fixture
            .call(
                "a",
                MethodName::ClipboardTransfer,
                json!({"computerId": unknown(), "deviceId": "other-device", "action": "copy"}),
            )
            .await;
        assert_eq!(code(remote).await, ErrorCode::UnsupportedRemoteOperation);
        let without_viewer = fixture
            .call(
                "a",
                MethodName::ClipboardTransfer,
                json!({"computerId": unknown(), "action": "paste"}),
            )
            .await;
        assert_eq!(code(without_viewer).await, ErrorCode::Internal);
        let foreign = fixture
            .call(
                "a",
                MethodName::ClipboardTransfer,
                json!({"computerId": unknown(), "action": "paste", "connectionId": "missing"}),
            )
            .await;
        assert_eq!(code(foreign).await, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn clipboard_transfer_refuses_a_viewer_of_another_computer_or_device() {
        let fixture = fixture();
        let (first, second) = (unknown(), unknown());
        let (shows_first, ..) = hold_viewer("clip-owner", &first);
        let (shows_second, ..) = hold_viewer("clip-owner", &second);
        let (shows_remote, ..) = hold_viewer_on("clip-owner", "other-device", &first);
        // The connection ids are all owned by the caller, but each shows another target
        // than the request names: nothing is transferred and nothing is learned.
        for (computer, connection) in [
            (&first, &shows_second),
            (&second, &shows_first),
            // A viewer of a remote computer cannot be used under a local target.
            (&first, &shows_remote),
        ] {
            let refused = fixture
                .call(
                    "clip-owner",
                    MethodName::ClipboardTransfer,
                    json!({"computerId": computer, "action": "copy", "connectionId": connection}),
                )
                .await;
            assert_eq!(code(refused).await, ErrorCode::NotFound);
        }
        // Another client's matching viewer is refused as before.
        let foreign = fixture
            .call(
                "clip-other",
                MethodName::ClipboardTransfer,
                json!({"computerId": first, "action": "copy", "connectionId": shows_first}),
            )
            .await;
        assert_eq!(code(foreign).await, ErrorCode::NotFound);
        for connection in [shows_first, shows_second, shows_remote] {
            silo_engine::desktop_viewer_api::close("clip-owner", &connection).unwrap();
        }
    }

    #[tokio::test]
    async fn clipboard_read_and_write_need_a_running_macos_computer() {
        let fixture = fixture();
        let stopped = fixture.macos(TestComputerState::Stopped, Approval::Ask);
        let read = fixture
            .call(
                "a",
                MethodName::ClipboardRead,
                json!({"computerId": stopped}),
            )
            .await;
        assert_eq!(code(read).await, ErrorCode::NotRunning);
        let write = fixture
            .call(
                "a",
                MethodName::ClipboardWrite,
                json!({"computerId": stopped, "content": {"kind": "text", "text": "hello"}}),
            )
            .await;
        assert_eq!(code(write).await, ErrorCode::NotRunning);
        let missing = fixture
            .call(
                "a",
                MethodName::ClipboardRead,
                json!({"computerId": unknown()}),
            )
            .await;
        assert_eq!(code(missing).await, ErrorCode::NotFound);
        let remote = fixture
            .call(
                "a",
                MethodName::ClipboardWrite,
                json!({
                    "computerId": unknown(),
                    "deviceId": "other-device",
                    "content": {"kind": "text", "text": "hello"}
                }),
            )
            .await;
        assert_eq!(code(remote).await, ErrorCode::UnsupportedRemoteOperation);
    }

    #[test]
    fn a_window_whose_activation_policy_fails_is_rolled_back() {
        use std::{cell::RefCell, collections::HashMap};
        let mut windows: HashMap<String, u32> = HashMap::new();
        let policies = RefCell::new(Vec::new());
        let discarded = RefCell::new(Vec::new());
        let failing = |open: bool| {
            policies.borrow_mut().push(open);
            if open && policies.borrow().len() == 1 {
                Err("NSApplication refused the activation policy".to_owned())
            } else {
                Ok(())
            }
        };
        let refused = window_core::show_window(
            &mut windows,
            "mac",
            || 7,
            &failing,
            |window| discarded.borrow_mut().push(window),
        );
        assert_eq!(
            refused.err().unwrap(),
            "NSApplication refused the activation policy"
        );
        // Nothing stays registered, the window was discarded once, and the policy was set
        // back for the windows that remain (none).
        assert!(windows.is_empty());
        assert_eq!(*discarded.borrow(), [7]);
        assert_eq!(*policies.borrow(), [true, false]);
        // The next show starts clean, and a second show of the same id reuses its window.
        let shown = window_core::show_window(
            &mut windows,
            "mac",
            || 8,
            &failing,
            |_| panic!("nothing to discard"),
        );
        assert!(matches!(shown, Ok(window_core::Shown::Created(8))));
        let again = window_core::show_window(
            &mut windows,
            "mac",
            || 9,
            &failing,
            |_| panic!("nothing to discard"),
        );
        assert!(matches!(again, Ok(window_core::Shown::Existing(8))));
        assert_eq!(windows.len(), 1);
    }

    #[test]
    fn a_main_thread_request_that_timed_out_never_creates_its_window() {
        use std::sync::Mutex;
        // A main thread that has not got to the work yet: the closure is only kept.
        let late: Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>> = Arc::new(Mutex::new(None));
        let created = Arc::new(AtomicBool::new(false));
        let result = window_core::run_on_main(
            |work| {
                *late.lock().unwrap() = Some(work);
                Ok(())
            },
            Duration::ZERO,
            {
                let created = created.clone();
                move || created.store(true, Ordering::SeqCst)
            },
        );
        assert_eq!(
            result.unwrap_err(),
            "Timed out waiting for the AppKit display window."
        );
        // The main thread reaches the work afterwards: it is skipped.
        let work = late.lock().unwrap().take().unwrap();
        work();
        assert!(!created.load(Ordering::SeqCst));
        // On time, the result comes back.
        let on_time = window_core::run_on_main(
            |work| {
                work();
                Ok(())
            },
            Duration::from_secs(5),
            || 42,
        );
        assert_eq!(on_time.unwrap(), 42);
    }

    #[test]
    fn a_request_is_either_started_by_the_main_thread_or_abandoned_by_the_caller() {
        let started_first = window_core::MainRequest::default();
        assert!(started_first.begin());
        // Work that has started cannot be abandoned: the caller waits for its result.
        assert!(!started_first.abandon());
        let abandoned_first = window_core::MainRequest::default();
        assert!(abandoned_first.abandon());
        assert!(!abandoned_first.begin());
    }

    #[tokio::test]
    async fn native_display_attach_is_not_available_yet() {
        let fixture = fixture();
        let attach = fixture
            .call(
                "a",
                MethodName::DisplayNativeAttach,
                json!({"computerId": unknown()}),
            )
            .await;
        assert_eq!(code(attach).await, ErrorCode::UnsupportedRemoteOperation);
    }
}
