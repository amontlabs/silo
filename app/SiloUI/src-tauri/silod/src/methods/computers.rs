//! Handlers for `computers.*`, `activity.*`, `logs.*`, `storage.*` and `macos.*`.
//!
//! Linux computers run through the engine's runtime code, macOS computers through its
//! Virtualization code; both reach clients with the same methods (10, 30). Long work is a
//! job: the handler accepts it, answers `{jobId}` and a worker thread runs it while the
//! engine publishes `operations.job.progress`, `operations.job.finished` and
//! `computers.changed` on the catalogue topics.
//!
//! Requests that name a computer on another device answer `unsupported_remote_operation`:
//! routing through the local engine is the connections domain's work (95 A-3), and the
//! owner-side bridge still serves the legacy names.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::{json, Value};
use silo_api::{
    jobs::{ErrorCode, JobId},
    methods::{
        common::{ComputerTargetParams, DeviceParams, EmptyParams, JobStartResult},
        computers::{
            ApplicationSource, ComputerConfigurationChange as ApiChange,
            ComputerConfigurationChangeParams, ComputerConfigurationRetryParams,
            ComputerCreateParams, ComputerCreateResult, ComputerCreationSkipWaitParams,
            ComputerDeleteParams, ComputerIdentitiesParams, ComputerIdentity, ComputerListParams,
            ComputerShellParams, ComputersListResult, LaunchStatus,
        },
        logs::{LogSource, LogsExportParams, LogsQueryParams},
        macos::{
            MacosDisplayConnectParams, MacosDisplayConnectResult, MacosDisplayResizeParams,
            MacosDisplayResizeResult, MacosDisplaySessionParams, MacosDisplaySessionResult,
        },
        storage::StorageParams,
        MethodName,
    },
};
use silo_engine::{
    context::{Audience, EngineContext},
    macos_computers::{Action, MacosContext, Refusal},
    runtime::{RuntimeHost, RuntimeLifecycleOutcome, RuntimePackageKind},
};

use crate::{
    context::SilodContext,
    dispatcher::{method_name, DispatchError, HandlerOutput},
};

use super::{ownership::owns_prefix, HandlerContext};

pub(super) fn owns(method: MethodName) -> bool {
    owns_prefix(
        method,
        &["computers.", "activity.", "logs.", "storage.", "macos."],
    )
}

// MARK: Hosts

/// `silod` supplies the engine's paths and policy to the runtime engine. The migration
/// controller is rebuilt per call so every call sees the same durable storage selection as
/// the other silod domains.
#[derive(Clone)]
pub(crate) struct SilodRuntimeHost {
    context: Arc<SilodContext>,
}

impl SilodRuntimeHost {
    fn new(context: &HandlerContext) -> Self {
        Self {
            context: Arc::new(context.silod_context().clone()),
        }
    }

    pub(crate) fn for_engine(context: Arc<SilodContext>) -> Self {
        Self { context }
    }

    fn migration(&self) -> silo_engine::runtime_migration::Controller {
        silo_engine::runtime_migration::Controller::new(self.context.paths().app_data.clone())
    }

    pub(crate) fn macos_context(&self) -> MacosContext {
        let engine: Arc<dyn EngineContext> = self.context.clone();
        MacosContext::new(
            engine,
            Arc::new(SilodMacosHost {
                runtime: self.clone(),
            }),
        )
    }
}

impl RuntimeHost for SilodRuntimeHost {
    fn engine_context(&self) -> Arc<dyn EngineContext> {
        self.context.clone()
    }

    fn runtime_storage(&self) -> Result<PathBuf, String> {
        self.migration().runtime_storage()
    }

    fn blocks_operations(&self) -> bool {
        self.migration().blocks_operations()
    }

    fn bundled_tools_directory(&self) -> Result<PathBuf, String> {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let bundle = matches!(
            self.context.identity().package_kind,
            silo_engine::PackageKind::MacApp
        )
        .then_some(silo_engine::bundled_tools::BundleKind::App);
        silo_engine::bundled_tools::resolve(
            &executable,
            &self.context.paths().resources,
            bundle,
            None,
        )
    }

    fn ensure_runtime_ready(&self) -> Result<(), String> {
        self.migration().ensure_ready()
    }

    fn runtime_package_kind(&self) -> RuntimePackageKind {
        if matches!(
            self.context.identity().package_kind,
            silo_engine::PackageKind::MacApp
        ) {
            RuntimePackageKind::MacApp
        } else {
            RuntimePackageKind::Other
        }
    }

    fn repository_push_operations(&self, local: Vec<Value>) -> Vec<Value> {
        silo_engine::runtime::runtime_paths(self)
            .and_then(|paths| silo_engine::host_push_operations::merge(&paths, local.clone()))
            .unwrap_or(local)
    }

    fn github_snapshot(&self) -> Result<Value, String> {
        silo_engine::github::snapshot()
    }

    fn macos_computer_names(&self) -> Vec<String> {
        silo_engine::macos_computers::names(&self.macos_context())
    }

    fn macos_log_target(&self, computer_id: &str) -> Option<(String, PathBuf)> {
        silo_engine::macos_computers::log_target(&self.macos_context(), computer_id)
    }

    /// The engine posts no OS notification (95 A-27); it keeps the latest notice per key so
    /// Fjord shows it when it connects.
    fn notify(&self, notice: silo_engine::notifications::Notice) {
        let result = serde_json::to_value(notice)
            .map_err(|error| error.to_string())
            .and_then(|notice| serde_json::from_value(notice).map_err(|error| error.to_string()))
            .and_then(|notice| self.context.retain_notice(&notice, Audience::All));
        if let Err(error) = result {
            eprintln!("silod: could not retain a computer notice: {error}");
        }
    }

    fn clear_computer_notices(&self, computer_ids: &[String]) {
        for computer_id in computer_ids {
            let _ = self
                .context
                .clear_notices_for_computer(computer_id, Audience::All);
        }
    }

    fn stop_macos_computers(&self, deadline: Option<Instant>) -> Result<(), String> {
        silo_engine::macos_computers::stop_all(&self.macos_context(), deadline)
    }

    fn lifecycle_notice(
        &self,
        action: &str,
        name: &str,
        computer_id: Option<String>,
        elapsed: Duration,
        outcome: RuntimeLifecycleOutcome,
    ) {
        use silo_engine::notifications::{lifecycle_notice, NoticeComputer, Outcome};

        let computer = computer_id.map(|id| NoticeComputer {
            id,
            name: name.to_owned(),
        });
        let outcome = match &outcome {
            RuntimeLifecycleOutcome::Cancelled => Outcome::Cancelled,
            RuntimeLifecycleOutcome::AlreadyQueued => Outcome::AlreadyQueued,
            RuntimeLifecycleOutcome::Succeeded => Outcome::Succeeded,
            RuntimeLifecycleOutcome::Failed(message) => Outcome::Failed(message),
        };
        if let Some(notice) = lifecycle_notice(action, name, computer, elapsed, outcome) {
            self.notify(notice);
        }
    }
}

/// The health watch of Linux computers: it compares readings of the runtime and reports a
/// change nobody asked for as a retained notice and as `computers.unexpectedChange`.
struct SilodHealthWatch {
    host: SilodRuntimeHost,
}

impl silo_engine::health_watch::HealthWatchServices for SilodHealthWatch {
    fn observe(&self) -> silo_engine::health_watch::Reading {
        silo_engine::runtime::health_observations(&self.host)
    }

    fn wait_for_computer_exit(&self, name: &str) -> bool {
        silo_engine::runtime::wait_for_computer_exit(&self.host, name)
    }

    fn notify(&self, notice: silo_engine::notifications::Notice) {
        self.host.notify(notice);
    }

    fn unexpected_change(&self, change: silo_engine::health_watch::UnexpectedChange) {
        publish_unexpected_change(self.host.context.as_ref(), change);
    }
}

fn publish_unexpected_change(
    context: &dyn EngineContext,
    change: silo_engine::health_watch::UnexpectedChange,
) {
    context.publish(
        silo_engine::context::Event::new(
            "computers.unexpectedChange",
            json!({
                "computerId": change.computer_id,
                "name": change.name,
                "state": change.state,
            }),
        ),
        Audience::All,
    );
}

/// Starts the engine's own health watch, which needs no client to be connected (95 A-26).
pub fn install_health_watch(context: Arc<SilodContext>) {
    silo_engine::health_watch::install_services(Arc::new(SilodHealthWatch {
        host: SilodRuntimeHost::for_engine(context),
    }));
}

/// The engine-owned VM window (95 A-21). A computer's own work, such as a setup that needs
/// the installer screen, opens the same native window `display.show` does, through the
/// display domain's main-thread runner and window table.
struct SilodMacosHost {
    runtime: SilodRuntimeHost,
}

impl silo_engine::macos_computers::MacosHost for SilodMacosHost {
    fn show_display(
        &self,
        context: &MacosContext,
        id: &str,
        title: &str,
        toolbar: bool,
    ) -> Result<(), String> {
        super::display::show_native_display(context, id, title, toolbar)
    }

    fn close_display(&self, id: &str) {
        super::display::close_native_display(id);
    }

    fn linux_computer_names(&self) -> Vec<String> {
        silo_engine::runtime::computer_names(&self.runtime)
    }
}

// MARK: Errors

fn error_code(message: &str) -> ErrorCode {
    if let Some(refusal) = silo_engine::macos_computers::classify_refusal(message) {
        return match refusal {
            Refusal::NotRunning => ErrorCode::NotRunning,
            Refusal::NeedsPersonalizing => ErrorCode::NeedsPersonalizing,
            Refusal::SetupIncomplete => ErrorCode::SetupIncomplete,
            Refusal::ScreenSharingUnavailable => ErrorCode::ScreenSharingUnavailable,
            Refusal::RunningLimit => ErrorCode::RunningLimit,
        };
    }
    let lower = message.to_ascii_lowercase();
    if lower.contains("no longer exists") || lower.contains("does not exist") {
        ErrorCode::NotFound
    } else if lower.contains("timed out") {
        ErrorCode::TimedOut
    } else if lower.contains("permission denied") {
        ErrorCode::PermissionDenied
    } else if lower.contains("cancelled") || lower.contains("canceled") {
        ErrorCode::Cancelled
    } else {
        ErrorCode::Internal
    }
}

fn invalid(message: impl ToString) -> DispatchError {
    let message = message.to_string();
    DispatchError {
        code: error_code(&message),
        message,
    }
}

fn not_found(message: &str) -> DispatchError {
    DispatchError {
        code: ErrorCode::NotFound,
        message: message.to_owned(),
    }
}

fn unsupported(message: &str) -> DispatchError {
    DispatchError {
        code: ErrorCode::UnsupportedRemoteOperation,
        message: message.to_owned(),
    }
}

fn join_failure(error: tokio::task::JoinError) -> DispatchError {
    DispatchError::internal(format!(
        "The computer operation stopped unexpectedly: {error}"
    ))
}

fn encode<T: Serialize>(value: T) -> Result<Value, DispatchError> {
    serde_json::to_value(value).map_err(|error| DispatchError::internal(error.to_string()))
}

fn plain(value: Value) -> HandlerOutput {
    HandlerOutput {
        value,
        event_barrier: None,
    }
}

fn job_output(job: JobStartResult) -> Result<HandlerOutput, DispatchError> {
    Ok(plain(encode(job)?))
}

/// Typed rows need the discriminator the unified `computers.list` adds (10 §1.3).
fn with_kind(mut state: Value, kind: &str) -> Value {
    if let Some(computers) = state.get_mut("computers").and_then(Value::as_array_mut) {
        for computer in computers {
            computer["kind"] = Value::String(kind.to_owned());
        }
    }
    state
}

/// `deviceId` absent, `"local"` or this device means this engine. Anything else is another
/// device, which the local engine does not route to yet.
fn require_local_device(
    context: &HandlerContext,
    device_id: Option<&str>,
) -> Result<(), DispatchError> {
    match device_id {
        None | Some("local") => Ok(()),
        Some(device) if device == context.silod_context().local_device().device_id => Ok(()),
        Some(_) => Err(unsupported(
            "This engine cannot route a computer request to another device yet.",
        )),
    }
}

// MARK: Computer identity

enum Resolved {
    Linux(String),
    Macos,
}

fn linux_name_for_id(host: &SilodRuntimeHost, computer_id: &str) -> Result<String, DispatchError> {
    let paths = silo_engine::runtime::runtime_paths(host).map_err(invalid)?;
    let metadata = silo_engine::runtime::read_metadata(&paths.metadata).map_err(invalid)?;
    metadata
        .computers
        .into_iter()
        .find(|configuration| configuration.id() == computer_id)
        .map(|configuration| configuration.name().to_owned())
        .ok_or_else(|| not_found("This computer no longer exists on this device."))
}

fn is_macos_computer(host: &SilodRuntimeHost, computer_id: &str) -> bool {
    silo_engine::macos_computers::computer_ids(&host.macos_context())
        .iter()
        .any(|id| id == computer_id)
}

/// Finds a computer by id across both inventories.
fn resolve(host: &SilodRuntimeHost, computer_id: &str) -> Result<Resolved, DispatchError> {
    if is_macos_computer(host, computer_id) {
        return Ok(Resolved::Macos);
    }
    linux_name_for_id(host, computer_id).map(Resolved::Linux)
}

fn require_macos(host: &SilodRuntimeHost, computer_id: &str) -> Result<(), DispatchError> {
    match resolve(host, computer_id)? {
        Resolved::Macos => Ok(()),
        Resolved::Linux(_) => Err(unsupported("This is a macOS computer method.")),
    }
}

// MARK: Jobs

fn new_request_id(request_id: Option<String>) -> Result<String, DispatchError> {
    let request_id = request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if request_id.trim().is_empty() || request_id.len() > 256 {
        return Err(invalid("Invalid computer configuration request ID."));
    }
    Ok(request_id)
}

/// Accepts the job `setup:<requestId>`.
fn accept_setup_job(
    context: &HandlerContext,
    method: &str,
    request_id: &str,
) -> Result<JobStartResult, DispatchError> {
    let accepted =
        silo_engine::jobs::accept_domain_id(context.engine_context(), "setup", request_id, method)
            .map_err(HandlerContext::map_registry_error)?;
    if !accepted.created {
        return Err(DispatchError {
            code: ErrorCode::Busy,
            message: if accepted.acknowledged {
                "This setup request ID was already acknowledged. Start a new request."
            } else {
                "This setup request ID is already in use."
            }
            .to_owned(),
        });
    }
    Ok(JobStartResult {
        job_id: accepted.job_id,
    })
}

/// A Linux setup job, whose progress and outcome go only to the client that asked
/// (10 `computers.configuration.change`).
fn accept_linux_setup_job(
    context: &HandlerContext,
    method: &str,
    request_id: &str,
) -> Result<JobStartResult, DispatchError> {
    let job = accept_setup_job(context, method, request_id)?;
    silo_engine::runtime::bind_setup_audience(request_id, context.client_id().clone());
    Ok(job)
}

/// A fallback for failures before the engine accepted its work. After acceptance the engine
/// has already ended the job, and a second, different outcome is refused by the registry.
fn finish_job_error(context: &SilodContext, job_id: JobId, message: String) {
    let _ = silo_engine::jobs::finish_with_error(
        context,
        job_id,
        error_code(&message),
        message,
        Audience::All,
    );
}

fn finish_job_result(context: &SilodContext, job_id: JobId, value: Value) {
    if let Err(error) = silo_engine::jobs::finish_with_result(context, job_id, value, Audience::All)
    {
        eprintln!("silod: could not persist a computer job result: {error}");
    }
}

/// Runs work that ends its own job; an error before that point ends it here.
fn run_engine_job(
    context: Arc<SilodContext>,
    job_id: JobId,
    work: impl FnOnce() -> Result<(), String> + Send + 'static,
) {
    tokio::task::spawn_blocking(move || {
        if let Err(message) = work() {
            if let Some(request_id) = job_id.as_str().strip_prefix("setup:") {
                silo_engine::runtime::release_setup_audience(request_id);
            }
            finish_job_error(&context, job_id, message);
        }
    });
}

/// Runs work whose value is the job's result.
fn run_owned_job<T: Serialize + Send + 'static>(
    context: Arc<SilodContext>,
    job_id: JobId,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) {
    tokio::task::spawn_blocking(move || match work() {
        Ok(value) => match serde_json::to_value(value) {
            Ok(value) => finish_job_result(&context, job_id, value),
            Err(error) => finish_job_error(&context, job_id, error.to_string()),
        },
        Err(message) => finish_job_error(&context, job_id, message),
    });
}

/// Starts a macOS action under `job_id`; the engine ends the job itself.
fn run_macos_action(host: &SilodRuntimeHost, job_id: JobId, computer_id: String, action: Action) {
    let macos = host.macos_context();
    let engine = host.context.clone();
    let method = method_for(&action);
    tokio::spawn(async move {
        let result = silo_engine::macos_computers::macos_computer_action_with_job(
            macos,
            computer_id,
            action,
            job_id.clone(),
            method,
        )
        .await;
        if let Err(message) = result {
            finish_job_error(&engine, job_id, message);
        }
    });
}

fn method_for(action: &Action) -> &'static str {
    match action {
        Action::Start => "computers.start",
        Action::Stop => "computers.stop",
        Action::ForceStop => "computers.forceStop",
        Action::Delete => "computers.delete",
        Action::Setup => "macos.setup.retry",
        Action::ChangeResources { .. } => "computers.configuration.change",
    }
}

// MARK: Reads

async fn read_computers(
    context: &HandlerContext,
    params: ComputerListParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    // The barrier comes first: an event published while the state is read is delivered
    // afterwards (a harmless repeat), never lost.
    let barrier = context
        .event_bus()
        .domain_barrier(context.client_id(), "computers")
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    let host = SilodRuntimeHost::new(context);
    let refresh = params.refresh_repositories;
    let linux_host = host.clone();
    let linux = tokio::task::spawn_blocking(move || {
        silo_engine::runtime::read_application_state_sync(&linux_host, refresh)
            .map_err(HandlerContext::map_engine_error)
    })
    .await
    .map_err(join_failure)??;
    let macos = silo_engine::macos_computers::read_macos_computers(host.macos_context())
        .await
        .map_err(invalid)?;
    let result: ComputersListResult = serde_json::from_value(json!({
        "linux": with_kind(encode(linux)?, "linux"),
        "macos": with_kind(encode(macos)?, "macos"),
    }))
    .map_err(|error| DispatchError::internal(error.to_string()))?;
    Ok(HandlerOutput {
        value: encode(result)?,
        event_barrier: Some(barrier),
    })
}

async fn read_shell(
    context: &HandlerContext,
    params: ComputerShellParams,
) -> Result<HandlerOutput, DispatchError> {
    let host = SilodRuntimeHost::new(context);
    let source: ApplicationSource = tokio::task::spawn_blocking(move || {
        let state = silo_engine::runtime::read_application_shell_with_host(&host, params.error)
            .map_err(invalid)?;
        serde_json::from_value(with_kind(encode(state)?, "linux"))
            .map_err(|error| DispatchError::internal(error.to_string()))
    })
    .await
    .map_err(join_failure)??;
    Ok(plain(encode(source)?))
}

async fn read_configuration(
    context: &HandlerContext,
    params: DeviceParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    let value = tokio::task::spawn_blocking(move || {
        let configuration =
            silo_engine::runtime::read_computer_configuration_with_host(&host).map_err(invalid)?;
        encode(configuration)
    })
    .await
    .map_err(join_failure)??;
    Ok(plain(value))
}

async fn read_setup_activity(context: &HandlerContext) -> Result<HandlerOutput, DispatchError> {
    let host = SilodRuntimeHost::new(context);
    let value = tokio::task::spawn_blocking(move || {
        let activity =
            silo_engine::runtime::read_setup_activity_with_host(&host).map_err(invalid)?;
        encode(activity)
    })
    .await
    .map_err(join_failure)??;
    Ok(plain(value))
}

/// The launch selection (both kinds) and the outcome of the last launch pass (10 §12.1).
async fn read_launch_status(
    context: &HandlerContext,
    params: DeviceParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    let settings = silo_engine::settings::settings_of(context.engine_context()).map_err(invalid)?;
    let enabled = settings
        .get("startComputersAtLaunch")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let macos_ids = silo_engine::macos_computers::computer_ids(&host.macos_context());
    let computer_ids = tokio::task::spawn_blocking(move || {
        let paths = silo_engine::runtime::runtime_paths(&host)?;
        silo_engine::startup::effective_selection(settings, &paths.metadata, &macos_ids)
    })
    .await
    .map_err(join_failure)?
    .map_err(invalid)?;
    let last_pass = context
        .engine_context()
        .runtime_state()
        .startup()
        .last_pass();
    Ok(plain(encode(LaunchStatus {
        enabled,
        computer_ids,
        last_pass,
    })?))
}

fn runtime_log_query(params: LogsQueryParams) -> silo_engine::runtime::runtime_logs::Query {
    let source = params.source.map(|source| {
        match source {
            LogSource::All => "all",
            LogSource::Stdout => "stdout",
            LogSource::Stderr => "stderr",
            LogSource::Output => "output",
            LogSource::System => "system",
            LogSource::Runtime => "runtime",
            LogSource::Kernel => "kernel",
        }
        .to_owned()
    });
    silo_engine::runtime::runtime_logs::Query {
        computer_id: params.computer_id,
        device_id: params.device_id,
        query: params.query,
        source,
        since: params.since,
        until: params.until,
        cursor: params.cursor,
        limit: params.limit.map(usize::from),
        around_id: params.around_id,
        follow: params.follow,
    }
}

/// One log page for either kind: a macOS computer keeps its setup log beside the engine's
/// own logs, a Linux computer its runtime records.
fn query_logs(
    host: &SilodRuntimeHost,
    query: silo_engine::runtime::runtime_logs::Query,
    device_id: &str,
    device_name: &str,
) -> Result<silo_engine::runtime::runtime_logs::Page, String> {
    if let Some((computer_name, directory)) = host.macos_log_target(&query.computer_id) {
        return silo_engine::runtime::runtime_logs::query_directory(
            &directory,
            query,
            &computer_name,
            device_id,
            device_name,
        );
    }
    let paths = silo_engine::runtime::runtime_paths(host)?;
    silo_engine::runtime::runtime_logs::query_local(&paths, query, device_id, device_name)
}

async fn read_logs(
    context: &HandlerContext,
    params: LogsQueryParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    let device = context.silod_context().local_device().clone();
    let query = runtime_log_query(params);
    let page = tokio::task::spawn_blocking(move || {
        query_logs(&host, query, &device.device_id, &device.name)
    })
    .await
    .map_err(join_failure)?
    .map_err(invalid)?;
    Ok(plain(encode(page)?))
}

async fn read_storage(
    context: &HandlerContext,
    params: StorageParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    if matches!(resolve(&host, &params.computer_id)?, Resolved::Macos) {
        return Err(unsupported(
            "Storage reporting for macOS computers is not available in this engine.",
        ));
    }
    let state = tokio::task::spawn_blocking(move || {
        silo_engine::runtime::storage::workspace_storage(&host, &params.computer_id, false)
    })
    .await
    .map_err(join_failure)?
    .map_err(invalid)?;
    let mut value = encode(state)?;
    // The engine reads history written before the catalogue existed, tagged `legacy`; the
    // API has no such trigger (95 §3), so the entry keeps its amount and time as a manual one.
    if let Some(history) = value.get_mut("history").and_then(Value::as_array_mut) {
        for entry in history {
            if entry["trigger"] == "legacy" {
                entry["trigger"] = Value::String("manual".to_owned());
            }
        }
    }
    Ok(plain(value))
}

// MARK: Lifecycle

/// Linux start/stop/restart, and macOS start/stop, as jobs.
async fn begin_lifecycle(
    context: &HandlerContext,
    params: ComputerTargetParams,
    method: &'static str,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    let resolved = resolve(&host, &params.computer_id)?;
    if matches!(resolved, Resolved::Macos) && method == "computers.restart" {
        return Err(unsupported(
            "Restart is not available for macOS computers. Stop it, then start it.",
        ));
    }
    let job = context.start_job(method)?;
    let job_id = job.job_id.clone();
    match resolved {
        Resolved::Macos => {
            let action = if method == "computers.start" {
                Action::Start
            } else {
                Action::Stop
            };
            run_macos_action(&host, job_id, params.computer_id, action);
        }
        Resolved::Linux(name) => {
            let action = method.trim_start_matches("computers.").to_owned();
            let engine = host.context.clone();
            let worker_job = job_id.clone();
            run_engine_job(engine, job_id, move || {
                silo_engine::runtime::computer_action_with_host_and_job(
                    &host, action, name, None, worker_job, method,
                )
                .map(|_| ())
                .map_err(|error| error.message)
            });
        }
    }
    job_output(job)
}

async fn force_stop(
    context: &HandlerContext,
    params: ComputerTargetParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    match resolve(&host, &params.computer_id)? {
        Resolved::Linux(_) => Err(unsupported("Linux computers have no force stop.")),
        Resolved::Macos => {
            silo_engine::macos_computers::macos_computer_action(
                host.macos_context(),
                params.computer_id,
                Action::ForceStop,
            )
            .await
            .map_err(invalid)?;
            Ok(plain(Value::Null))
        }
    }
}

async fn dismiss_error(
    context: &HandlerContext,
    params: ComputerTargetParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    let name = match resolve(&host, &params.computer_id)? {
        Resolved::Linux(name) => name,
        Resolved::Macos => {
            return Err(unsupported(
                "A macOS computer's failure is cleared by retrying its setup or starting it.",
            ))
        }
    };
    // The engine records every lifecycle action as a job. A command has none of its own, so
    // the record is acknowledged as soon as the action ends.
    let internal = silo_engine::jobs::allocate(context.engine_context(), "computers.dismissError")
        .map_err(HandlerContext::map_registry_error)?
        .job_id;
    let engine = host.context.clone();
    let worker_job = internal.clone();
    let state = tokio::task::spawn_blocking(move || {
        silo_engine::runtime::computer_action_with_host_and_job(
            &host,
            "dismiss-error".to_owned(),
            name,
            None,
            worker_job,
            "computers.dismissError",
        )
    })
    .await
    .map_err(join_failure)?;
    if let Ok(registry) = engine.job_registry() {
        let _ = registry.acknowledge(&internal);
    }
    let state = state.map_err(HandlerContext::map_engine_error)?;
    let source: ApplicationSource = serde_json::from_value(with_kind(encode(state)?, "linux"))
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    Ok(plain(encode(source)?))
}

// MARK: Configuration

fn contains_macos_resources(change: &ApiChange) -> bool {
    match change {
        ApiChange::MacosResources { .. } => true,
        ApiChange::Batch { changes } => changes.iter().any(contains_macos_resources),
        _ => false,
    }
}

fn linux_change(
    change: ApiChange,
) -> Result<silo_engine::runtime::ComputerConfigurationChange, DispatchError> {
    if contains_macos_resources(&change) {
        return Err(invalid(
            "A macOS resource change is its own change and cannot join a batch.",
        ));
    }
    serde_json::from_value(encode(change)?).map_err(invalid)
}

/// Runs a Linux configuration change. The engine ends the job (`setup:<requestId>`) with the
/// resulting state and publishes `computers.changed`.
fn run_linux_change(
    host: &SilodRuntimeHost,
    job_id: JobId,
    request_id: String,
    change: silo_engine::runtime::ComputerConfigurationChange,
    retry_name: Option<String>,
) {
    let worker = host.clone();
    let worker_job = job_id.clone();
    run_engine_job(host.context.clone(), job_id, move || {
        silo_engine::runtime::change_computer_configuration_with_host_and_job(
            &worker,
            change,
            Some(request_id),
            retry_name,
            Some(worker_job),
        )
        .map(|_| ())
    });
}

async fn begin_configuration_change(
    context: &HandlerContext,
    params: ComputerConfigurationChangeParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    if let ApiChange::MacosResources {
        computer_id,
        expected,
        cpus,
        memory_gib,
        request_id,
    } = params.change
    {
        require_macos(&host, &computer_id)?;
        let request_id = new_request_id(Some(request_id))?;
        let job = accept_setup_job(context, "computers.configuration.change", &request_id)?;
        let action = Action::ChangeResources {
            cpus,
            memory_gib,
            expected: silo_engine::macos_computers::ExpectedResources {
                cpus: expected.cpus,
                memory_gib: expected.memory_gib,
                disk_gib: expected.disk_gib,
            },
        };
        run_macos_action(&host, job.job_id.clone(), computer_id, action);
        return job_output(job);
    }

    let retry_name = params
        .retry_computer_id
        .as_deref()
        .map(|id| linux_name_for_id(&host, id))
        .transpose()?;
    let change = linux_change(params.change)?;
    let request_id = new_request_id(params.request_id)?;
    let job = accept_linux_setup_job(context, "computers.configuration.change", &request_id)?;
    run_linux_change(&host, job.job_id.clone(), request_id, change, retry_name);
    job_output(job)
}

async fn begin_configuration_retry(
    context: &HandlerContext,
    params: ComputerConfigurationRetryParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    let retry_name = params
        .retry_computer_id
        .as_deref()
        .map(|id| linux_name_for_id(&host, id))
        .transpose()?;
    let request_id = new_request_id(params.request_id)?;
    let job = accept_linux_setup_job(context, "computers.configuration.retry", &request_id)?;
    let worker = host.clone();
    let job_id = job.job_id.clone();
    let worker_job = job_id.clone();
    run_engine_job(host.context.clone(), job_id, move || {
        silo_engine::runtime::retry_computer_configuration_with_host_and_job(
            &worker,
            Some(request_id),
            retry_name,
            Some(worker_job),
        )
        .map(|_| ())
    });
    job_output(job)
}

async fn begin_create(
    context: &HandlerContext,
    params: ComputerCreateParams,
) -> Result<HandlerOutput, DispatchError> {
    let host = SilodRuntimeHost::new(context);
    match params {
        ComputerCreateParams::Linux {
            configuration,
            request_id,
            device_id,
        } => {
            require_local_device(context, device_id.as_deref())?;
            let configuration = serde_json::from_value(encode(configuration)?).map_err(invalid)?;
            let request_id = new_request_id(request_id)?;
            let job = accept_linux_setup_job(context, "computers.create", &request_id)?;
            let change = silo_engine::runtime::ComputerConfigurationChange::Upsert {
                configuration,
                expected: None,
            };
            run_linux_change(&host, job.job_id.clone(), request_id, change, None);
            Ok(plain(encode(ComputerCreateResult {
                job_id: job.job_id,
                computer: None,
            })?))
        }
        ComputerCreateParams::Macos { request, device_id } => {
            require_local_device(context, device_id.as_deref())?;
            let request: silo_engine::macos_computers::CreateRequest =
                serde_json::from_value(encode(request)?).map_err(invalid)?;
            let job = context.start_job("computers.create")?;
            let job_id = job.job_id.clone();
            let created = silo_engine::macos_computers::create_macos_computer_with_job(
                host.macos_context(),
                request,
                job_id.clone(),
            )
            .await;
            // A refused request ends the job with its reason; the job id is still the answer,
            // as for Linux, and the row is absent.
            let computer = match created {
                Ok(row) => {
                    let value = with_kind(json!({"computers": [encode(row)?]}), "macos");
                    serde_json::from_value(value["computers"][0].clone()).ok()
                }
                Err(message) => {
                    finish_job_error(&host.context, job_id.clone(), message);
                    None
                }
            };
            Ok(plain(encode(ComputerCreateResult { job_id, computer })?))
        }
    }
}

async fn begin_delete(
    context: &HandlerContext,
    params: ComputerDeleteParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    match resolve(&host, &params.computer_id)? {
        Resolved::Macos => {
            let job = context.start_job("computers.delete")?;
            run_macos_action(
                &host,
                job.job_id.clone(),
                params.computer_id,
                Action::Delete,
            );
            job_output(job)
        }
        Resolved::Linux(_) => {
            let expected = params.expected.ok_or_else(|| {
                invalid("Deleting a Linux computer needs its expected configuration.")
            })?;
            let change = silo_engine::runtime::ComputerConfigurationChange::Delete {
                computer_id: params.computer_id,
                expected: serde_json::from_value(encode(expected)?).map_err(invalid)?,
            };
            let request_id = new_request_id(params.request_id)?;
            let job = accept_linux_setup_job(context, "computers.delete", &request_id)?;
            run_linux_change(&host, job.job_id.clone(), request_id, change, None);
            job_output(job)
        }
    }
}

fn skip_creation_wait(
    params: ComputerCreationSkipWaitParams,
) -> Result<HandlerOutput, DispatchError> {
    silo_engine::runtime::skip_computer_use_wait_request(params.request_id);
    Ok(plain(Value::Null))
}

// MARK: Identities

fn engine_identities(
    host: &SilodRuntimeHost,
    identities: Vec<ComputerIdentity>,
) -> Result<Vec<silo_engine::runtime::ComputerIdentity>, DispatchError> {
    let paths = silo_engine::runtime::runtime_paths(host).map_err(invalid)?;
    let metadata = silo_engine::runtime::read_metadata(&paths.metadata).map_err(invalid)?;
    identities
        .into_iter()
        .map(|identity| {
            let computer = metadata
                .computers
                .iter()
                .find(|configuration| configuration.id() == identity.computer_id)
                .map(|configuration| configuration.name().to_owned())
                .ok_or_else(|| invalid("Each identity must name a configured local computer."))?;
            serde_json::from_value(json!({
                "computer": computer,
                "name": identity.name,
                "email": identity.email,
                "apply": identity.apply,
            }))
            .map_err(invalid)
        })
        .collect()
}

async fn begin_identity_configuration(
    context: &HandlerContext,
    params: ComputerIdentitiesParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    let identities = engine_identities(&host, params.identities)?;
    let job = context.start_job("computers.identities.configure")?;
    let job_id = job.job_id.clone();
    let worker_job = job_id.clone();
    run_engine_job(host.context.clone(), job_id, {
        let host = host.clone();
        move || {
            silo_engine::runtime::configure_computer_identities_with_host_and_job(
                &host, identities, worker_job,
            )
        }
    });
    job_output(job)
}

async fn verify_identities(
    context: &HandlerContext,
    params: ComputerIdentitiesParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    let identities = engine_identities(&host, params.identities)?;
    let verified = tokio::task::spawn_blocking(move || {
        silo_engine::runtime::verify_computer_identities_with_host(&host, identities)
    })
    .await
    .map_err(join_failure)?
    .map_err(invalid)?;
    Ok(plain(Value::Bool(verified)))
}

// MARK: Storage and logs

async fn begin_storage_reclaim(
    context: &HandlerContext,
    params: StorageParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    if matches!(resolve(&host, &params.computer_id)?, Resolved::Macos) {
        return Err(unsupported(
            "Storage reclamation for macOS computers is not available in this engine.",
        ));
    }
    let job = context.start_job("storage.reclaim")?;
    let job_id = job.job_id.clone();
    let cancellation = Arc::new(AtomicBool::new(false));
    let (cancel_flag, cancel_id) = (cancellation.clone(), job_id.clone());
    context.job_registry()?.register_cancel(
        job_id.clone(),
        silo_engine::jobs::CancelRoute::JobGate,
        move || {
            // Before the reclaim reaches the queue only the flag can stop it.
            if !silo_engine::runtime::OPERATIONS.contains_job(&cancel_id) {
                cancel_flag.store(true, Ordering::SeqCst);
                return Ok(());
            }
            silo_engine::runtime::OPERATIONS
                .cancel_job(&cancel_id)
                .map_err(|error| error.to_string())
        },
    );
    let worker_job = job_id.clone();
    run_owned_job(host.context.clone(), job_id, move || {
        silo_engine::runtime::storage::workspace_storage_with_job(
            &host,
            &params.computer_id,
            worker_job,
            cancellation,
        )
    });
    job_output(job)
}

async fn begin_logs_export(
    context: &HandlerContext,
    params: LogsExportParams,
) -> Result<HandlerOutput, DispatchError> {
    for request in &params.requests {
        require_local_device(context, request.device_id.as_deref())?;
    }
    let requests: Vec<_> = params.requests.into_iter().map(runtime_log_query).collect();
    silo_engine::log_export::validate_requests(&requests).map_err(invalid)?;
    let job = context.start_job("logs.export")?;
    let job_id = job.job_id.clone();
    // Each export is cancelled on its own (95 A-30).
    let cancellation = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancellation.clone();
    context.job_registry()?.register_cancel(
        job_id.clone(),
        silo_engine::jobs::CancelRoute::LogExport,
        move || {
            cancel_flag.store(true, Ordering::SeqCst);
            Ok(())
        },
    );
    let host = SilodRuntimeHost::new(context);
    let device = context.silod_context().local_device().clone();
    let destination = PathBuf::from(params.destination);
    run_owned_job(host.context.clone(), job_id, move || {
        let permit = silo_engine::log_export::begin_export()?;
        let query_host = host.clone();
        let is_cancelled = cancellation.clone();
        silo_engine::log_export::export_to(
            permit,
            requests,
            destination,
            move |query| query_logs(&query_host, query, &device.device_id, &device.name),
            move || is_cancelled.load(Ordering::SeqCst),
        )
    });
    job_output(job)
}

// MARK: macOS display and templates

async fn display_credentials(
    host: &SilodRuntimeHost,
    computer_id: String,
) -> Result<MacosDisplayConnectResult, DispatchError> {
    let macos = host.macos_context();
    let session = tokio::task::spawn_blocking(move || {
        silo_engine::macos_computers::display_session(&macos, &computer_id)
    })
    .await
    .map_err(join_failure)?
    .map_err(invalid)?;
    serde_json::from_value(encode(session)?)
        .map_err(|error| DispatchError::internal(error.to_string()))
}

async fn macos_display_connect(
    context: &HandlerContext,
    params: MacosDisplayConnectParams,
) -> Result<HandlerOutput, DispatchError> {
    let session = display_credentials(&SilodRuntimeHost::new(context), params.computer_id).await?;
    Ok(plain(encode(session)?))
}

async fn macos_display_resize(
    context: &HandlerContext,
    params: MacosDisplayResizeParams,
) -> Result<HandlerOutput, DispatchError> {
    let macos = SilodRuntimeHost::new(context).macos_context();
    let (width_px, height_px) = tokio::task::spawn_blocking(move || {
        silo_engine::macos_computers::resize_display(
            &macos,
            &params.computer_id,
            params.width_px,
            params.height_px,
        )
    })
    .await
    .map_err(join_failure)?
    .map_err(invalid)?;
    Ok(plain(encode(MacosDisplayResizeResult {
        width_px,
        height_px,
    })?))
}

const MAX_DISPLAYS_PER_CLIENT: usize = 8;

type DisplayKey = (String, String, String);

/// Loopback gateways of the screens a client has open, one per (client, device, computer):
/// a client reopening a screen replaces its own gateway and no other client's (95 A-28).
fn display_gateways(
) -> std::sync::MutexGuard<'static, HashMap<DisplayKey, silo_engine::macos_display_gateway::Gateway>>
{
    static GATEWAYS: OnceLock<
        Mutex<HashMap<DisplayKey, silo_engine::macos_display_gateway::Gateway>>,
    > = OnceLock::new();
    GATEWAYS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn close_displays_where(matches: impl Fn(&DisplayKey) -> bool) {
    let removed: Vec<_> = {
        let mut gateways = display_gateways();
        let keys: Vec<_> = gateways
            .keys()
            .filter(|key| matches(key))
            .cloned()
            .collect();
        keys.into_iter()
            .filter_map(|key| gateways.remove(&key))
            .collect()
    };
    // Dropping a gateway stops it; that must not happen under the registry lock.
    drop(removed);
}

/// Ends the screens of one client, as it disconnects.
pub(crate) fn close_client_displays(client_id: &str) {
    close_displays_where(|(client, _, _)| client == client_id);
}

/// Ends the screens of one device, or all of them when `device` is `None`.
pub(crate) fn close_displays(device: Option<&str>) {
    close_displays_where(|(_, owner, _)| device.is_none_or(|device| device == owner));
}

fn display_origins(context: &HandlerContext) -> Vec<String> {
    let mut origins = vec![
        "tauri://localhost".to_owned(),
        "http://tauri.localhost".to_owned(),
        "https://tauri.localhost".to_owned(),
    ];
    if matches!(
        context.engine_context().identity().channel,
        silo_engine::Channel::Development
    ) {
        origins.push("http://localhost:1420".to_owned());
    }
    origins
}

/// A loopback WebSocket gateway to the guest's Screen Sharing, for this client. On this
/// device it relays to the guest directly; for another device it relays through that
/// device's `macos.display.stream`.
async fn macos_display_session(
    context: &HandlerContext,
    params: MacosDisplaySessionParams,
) -> Result<HandlerOutput, DispatchError> {
    let local = params.device_id == "local"
        || params.device_id == context.silod_context().local_device().device_id;
    let host = SilodRuntimeHost::new(context);
    let (device_id, computer_id) = (params.device_id, params.computer_id);
    let key = (
        context.client_identity().client_id.clone(),
        device_id.clone(),
        computer_id.clone(),
    );
    {
        let gateways = display_gateways();
        if !gateways.contains_key(&key)
            && gateways
                .keys()
                .filter(|(client, _, _)| *client == key.0)
                .count()
                >= MAX_DISPLAYS_PER_CLIENT
        {
            return Err(DispatchError {
                code: ErrorCode::Busy,
                message: "Close an unused screen first.".to_owned(),
            });
        }
    }

    let (credentials, open): (
        MacosDisplayConnectResult,
        silo_engine::macos_display_gateway::Open,
    ) = if local {
        let credentials = display_credentials(&host, computer_id.clone()).await?;
        let macos = host.macos_context();
        let stream_id = computer_id.clone();
        let open: silo_engine::macos_display_gateway::Open = Arc::new(move |cancelled| {
            if cancelled() {
                return Err("Opening the screen connection was cancelled.".to_owned());
            }
            let stream = silo_engine::macos_computers::display_stream(&macos, &stream_id)?;
            let reader = stream.try_clone().map_err(|error| error.to_string())?;
            Ok(silo_engine::macos_display_gateway::Upstream {
                reader: Box::new(reader),
                writer: Box::new(stream),
                keep: Box::new(()),
            })
        });
        (credentials, open)
    } else {
        let (remote_device, remote_computer) = (device_id.clone(), computer_id.clone());
        let credentials = tokio::task::spawn_blocking(move || {
            silo_engine::remote::call_remote_typed(
                &remote_device,
                "macos.display.connect",
                json!({"computerId": remote_computer}),
            )
        })
        .await
        .map_err(join_failure)?
        .map_err(HandlerContext::map_engine_error)?;
        let credentials: MacosDisplayConnectResult =
            serde_json::from_value(credentials).map_err(|_| {
                DispatchError::internal("The computer returned no sign-in for its screen.")
            })?;
        let (open_device, open_computer) = (device_id.clone(), computer_id.clone());
        let open: silo_engine::macos_display_gateway::Open = Arc::new(move |cancelled| {
            let stream = silo_engine::remote::open_bridge_stream(
                &open_device,
                "macos.display.stream",
                json!({"computerId": open_computer}),
                cancelled,
            )
            .map_err(|error| error.message)?;
            let silo_engine::remote::BridgeStream {
                child,
                input,
                output,
            } = stream;
            Ok(silo_engine::macos_display_gateway::Upstream {
                reader: Box::new(output),
                writer: Box::new(input),
                keep: Box::new(child),
            })
        });
        (credentials, open)
    };

    let gateway =
        silo_engine::macos_display_gateway::Gateway::start(open, display_origins(context))
            .map_err(invalid)?;
    let url = gateway.url();
    let replaced = display_gateways().insert(key, gateway);
    drop(replaced);
    Ok(plain(encode(MacosDisplaySessionResult {
        url,
        username: credentials.username,
        password: credentials.password,
        width: credentials.width,
        height: credentials.height,
    })?))
}

async fn begin_setup_retry(
    context: &HandlerContext,
    params: ComputerTargetParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    let host = SilodRuntimeHost::new(context);
    require_macos(&host, &params.computer_id)?;
    let job = context.start_job("macos.setup.retry")?;
    run_macos_action(&host, job.job_id.clone(), params.computer_id, Action::Setup);
    job_output(job)
}

async fn delete_template(
    context: &HandlerContext,
    params: DeviceParams,
) -> Result<HandlerOutput, DispatchError> {
    require_local_device(context, params.device_id.as_deref())?;
    silo_engine::macos_computers::delete_macos_template(
        SilodRuntimeHost::new(context).macos_context(),
    )
    .await
    .map_err(invalid)?;
    Ok(plain(Value::Null))
}

// MARK: Dispatch

pub async fn handle(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Option<Result<HandlerOutput, DispatchError>> {
    if !owns(method) {
        return None;
    }
    Some(route(method, params, context).await)
}

async fn route(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    macro_rules! decoded {
        () => {
            context.decode_params(params)?
        };
    }
    match method {
        MethodName::ComputersList => read_computers(context, decoded!()).await,
        MethodName::ComputersShell => read_shell(context, decoded!()).await,
        MethodName::ComputersConfigurationRead => read_configuration(context, decoded!()).await,
        MethodName::ActivitySetupRead => {
            let _: EmptyParams = decoded!();
            read_setup_activity(context).await
        }
        MethodName::ComputersLaunch => read_launch_status(context, decoded!()).await,
        MethodName::LogsQuery => read_logs(context, decoded!()).await,
        MethodName::LogsExport => begin_logs_export(context, decoded!()).await,
        MethodName::StorageRead => read_storage(context, decoded!()).await,
        MethodName::StorageReclaim => begin_storage_reclaim(context, decoded!()).await,
        MethodName::ComputersStart => begin_lifecycle(context, decoded!(), "computers.start").await,
        MethodName::ComputersStop => begin_lifecycle(context, decoded!(), "computers.stop").await,
        MethodName::ComputersRestart => {
            begin_lifecycle(context, decoded!(), "computers.restart").await
        }
        MethodName::ComputersForceStop => force_stop(context, decoded!()).await,
        MethodName::ComputersDismissError => dismiss_error(context, decoded!()).await,
        MethodName::ComputersConfigurationChange => {
            begin_configuration_change(context, decoded!()).await
        }
        MethodName::ComputersConfigurationRetry => {
            begin_configuration_retry(context, decoded!()).await
        }
        MethodName::ComputersCreate => begin_create(context, decoded!()).await,
        MethodName::ComputersDelete => begin_delete(context, decoded!()).await,
        MethodName::ComputersCreationSkipComputerUseWait => skip_creation_wait(decoded!()),
        MethodName::ComputersIdentitiesConfigure => {
            begin_identity_configuration(context, decoded!()).await
        }
        MethodName::ComputersIdentitiesVerify => verify_identities(context, decoded!()).await,
        MethodName::MacosDisplayConnect => macos_display_connect(context, decoded!()).await,
        MethodName::MacosDisplayResize => macos_display_resize(context, decoded!()).await,
        MethodName::MacosDisplaySession => macos_display_session(context, decoded!()).await,
        MethodName::MacosSetupRetry => begin_setup_retry(context, decoded!()).await,
        MethodName::MacosTemplateDelete => delete_template(context, decoded!()).await,
        // A raw byte stream needs the socket's stream transport, which the engine does not
        // have yet; the loopback gateway of `macos.display.session` is the supported route.
        MethodName::MacosDisplayStream => Err(unsupported(
            "macos.display.stream needs the engine's raw stream transport, which is not available yet.",
        )),
        other => Err(DispatchError::unsupported(method_name(other))),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{json, Value};
    use silo_api::{jobs::ErrorCode, methods::MethodName};
    use silo_engine::{
        context::{ClientId, EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl},
        events::{EventDelivery, EventReceiver},
    };

    use super::*;
    use crate::{dispatcher::RequestContext, identity::LocalDeviceIdentity};

    const UNKNOWN: &str = "99999999-9999-4999-8999-999999999999";

    struct Main;
    impl MainThread for Main {
        fn run(&self, work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
            work();
            Ok(())
        }
    }

    struct Process;
    impl ProcessControl for Process {
        fn exit(&self, _code: i32) {}
        fn restart(&self) -> ! {
            panic!("restart is not expected in a computers test")
        }
    }

    /// An engine over empty temporary state, one connected client and its event stream.
    struct Fixture {
        _root: tempfile::TempDir,
        context: HandlerContext,
        events: EventReceiver,
        _gate: crate::methods::ProcessGateLease,
    }

    fn fixture() -> Fixture {
        let gate = crate::methods::process_gate_lock();
        let root = tempfile::tempdir().unwrap();
        let path = |name: &str| root.path().join(name);
        let paths = EnginePaths {
            home: path("home"),
            app_data: path("data"),
            app_config: path("config"),
            app_cache: path("cache"),
            app_log: path("log"),
            resources: path("resources"),
        };
        let silod = Arc::new(SilodContext::new(
            paths,
            EngineIdentity {
                channel: silo_engine::channel::Channel::Development,
                version: "0.1.0-test".into(),
                package_kind: PackageKind::MacApp,
            },
            LocalDeviceIdentity {
                device_id: "test-device".into(),
                name: "Test Mac".into(),
            },
            Arc::new(Main),
            Arc::new(Process),
        ));
        let client_id = format!("client-{}", uuid::Uuid::new_v4());
        let mut events = silod.connect_event_stream(ClientId::new(client_id.clone()));
        let subscription = silod
            .subscribe_events(
                &ClientId::new(client_id.clone()),
                vec!["operations.*".into(), "computers.*".into()],
            )
            .unwrap();
        silod
            .activate_events(&ClientId::new(client_id.clone()), &subscription)
            .unwrap();
        let _ = &mut events;
        let context = HandlerContext::new(
            silod,
            RequestContext {
                client_id,
                protocol: silo_api::transport::PROTOCOL_VERSION,
                client: None,
            },
        );
        Fixture {
            _root: root,
            context,
            events,
            _gate: gate,
        }
    }

    impl Fixture {
        async fn call(&self, method: MethodName, params: Value) -> Result<Value, DispatchError> {
            handle(method, &params, &self.context)
                .await
                .expect("the computers domain owns this method")
                .map(|output| output.value)
        }

        /// The next event of `name`, skipping the rest.
        async fn next_event(&mut self, name: &str) -> Value {
            tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    match self.events.recv().await {
                        Some(EventDelivery::Event(event)) if event.event.name == name => {
                            return event.event.payload;
                        }
                        Some(_) => continue,
                        None => panic!("the event stream ended"),
                    }
                }
            })
            .await
            .unwrap_or_else(|_| panic!("no {name} event arrived"))
        }
    }

    async fn code_of(fixture: &Fixture, method: MethodName, params: Value) -> ErrorCode {
        fixture.call(method, params).await.unwrap_err().code
    }

    #[test]
    fn owns_only_the_assigned_domain_prefixes() {
        assert!(owns(MethodName::ComputersList));
        assert!(owns(MethodName::ActivitySetupRead));
        assert!(owns(MethodName::LogsQuery));
        assert!(owns(MethodName::StorageRead));
        assert!(owns(MethodName::MacosDisplayConnect));
        assert!(!owns(MethodName::CheckpointsCreate));
        assert!(!owns(MethodName::GuestExec));
    }

    const LINUX: &str = "00000000-0000-4000-8000-000000000001";

    /// Log exports are device-wide one at a time, so the tests that export take turns.
    static EXPORTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Saves one Linux computer in the engine's inventory without starting anything.
    fn seed_linux(fixture: &Fixture) {
        let host = SilodRuntimeHost::new(&fixture.context);
        let paths = silo_engine::runtime::runtime_paths(&host).unwrap();
        std::fs::create_dir_all(paths.metadata.parent().unwrap()).unwrap();
        std::fs::write(
            &paths.metadata,
            json!({"schemaVersion": 1, "computers": [linux_configuration()]}).to_string(),
        )
        .unwrap();
    }

    fn linux_configuration() -> Value {
        json!({"id": LINUX, "name": "dev", "cpus": 2, "maxCPUs": 2, "memoryGiB": 2,
            "maxMemoryGiB": 2, "workspaceStorageGiB": 10, "runtimeStorageGiB": 10})
    }

    #[tokio::test]
    async fn computers_list_returns_both_kinds_with_a_barrier() {
        let f = fixture();
        let output = handle(MethodName::ComputersList, &json!({}), &f.context)
            .await
            .unwrap()
            .unwrap();
        let result: ComputersListResult = serde_json::from_value(output.value).unwrap();
        assert!(result.linux.computers.is_empty());
        assert!(result.macos.computers.is_empty());
        assert!(output.event_barrier.is_some());
    }

    #[tokio::test]
    async fn computers_list_rows_carry_their_kind() {
        assert_eq!(
            with_kind(json!({"computers": [{"id": "a"}, {"id": "b"}]}), "macos")["computers"][1]
                ["kind"],
            "macos"
        );
    }

    #[tokio::test]
    async fn the_shell_read_reports_the_runtime_repair_reason() {
        let f = fixture();
        let source = f
            .call(MethodName::ComputersShell, json!({"error": "msb is down"}))
            .await
            .unwrap();
        let source: ApplicationSource = serde_json::from_value(source).unwrap();
        assert!(source.computers.is_empty());
        assert_eq!(source.runtime_repair.unwrap()["reason"], "msb is down");
    }

    #[tokio::test]
    async fn configuration_activity_and_launch_reads_answer_on_empty_state() {
        let f = fixture();
        let configuration = f
            .call(MethodName::ComputersConfigurationRead, json!({}))
            .await
            .unwrap();
        assert_eq!(configuration, json!({"schemaVersion": 1, "computers": []}));
        assert_eq!(
            f.call(MethodName::ActivitySetupRead, json!({}))
                .await
                .unwrap(),
            json!([])
        );
        assert_eq!(
            f.call(MethodName::ComputersLaunch, json!({}))
                .await
                .unwrap(),
            json!({"enabled": false, "computerIds": [], "lastPass": null})
        );
    }

    #[tokio::test]
    async fn launch_status_reports_the_last_launch_pass() {
        let f = fixture();
        // The launch pass records its outcome on the engine's startup state.
        let state = f.context.engine_context().runtime_state();
        let pass = silo_api::methods::computers::LaunchPass {
            finished_at: 1,
            failures: vec!["dev could not start".into()],
        };
        state.startup().record_pass(pass.failures.clone());
        let status: LaunchStatus = serde_json::from_value(
            f.call(MethodName::ComputersLaunch, json!({}))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(status.last_pass.unwrap().failures, pass.failures);
    }

    #[tokio::test]
    async fn requests_for_another_device_are_refused_by_every_device_aware_method() {
        let f = fixture();
        let device = json!("other-device");
        let target = json!({"computerId": LINUX, "deviceId": device});
        let cases = [
            (MethodName::ComputersList, json!({"deviceId": device})),
            (
                MethodName::ComputersConfigurationRead,
                json!({"deviceId": device}),
            ),
            (MethodName::ComputersLaunch, json!({"deviceId": device})),
            (MethodName::ComputersStart, target.clone()),
            (MethodName::ComputersStop, target.clone()),
            (MethodName::ComputersRestart, target.clone()),
            (MethodName::ComputersForceStop, target.clone()),
            (MethodName::ComputersDismissError, target.clone()),
            (MethodName::MacosSetupRetry, target.clone()),
            (MethodName::MacosTemplateDelete, json!({"deviceId": device})),
            (
                MethodName::LogsQuery,
                json!({"computerId": LINUX, "deviceId": device}),
            ),
            (MethodName::StorageRead, target.clone()),
            (MethodName::StorageReclaim, target.clone()),
            (
                MethodName::ComputersDelete,
                json!({"computerId": LINUX, "deviceId": device}),
            ),
            (
                MethodName::ComputersIdentitiesVerify,
                json!({"identities": [], "deviceId": device}),
            ),
            (
                MethodName::ComputersIdentitiesConfigure,
                json!({"identities": [], "deviceId": device}),
            ),
            (
                MethodName::ComputersConfigurationRetry,
                json!({"deviceId": device}),
            ),
            (
                MethodName::LogsExport,
                json!({"requests": [{"computerId": LINUX, "deviceId": device}], "destination": "/tmp/x"}),
            ),
        ];
        for (method, params) in cases {
            assert_eq!(
                code_of(&f, method, params).await,
                ErrorCode::UnsupportedRemoteOperation,
                "{method:?}"
            );
        }
        // The device's own id and "local" mean this engine.
        for device in ["test-device", "local"] {
            assert!(f
                .call(
                    MethodName::ComputersConfigurationRead,
                    json!({"deviceId": device})
                )
                .await
                .is_ok());
        }
    }

    #[tokio::test]
    async fn an_unknown_computer_is_not_found_before_any_job_exists() {
        let f = fixture();
        let target = json!({"computerId": UNKNOWN});
        for method in [
            MethodName::ComputersStart,
            MethodName::ComputersStop,
            MethodName::ComputersRestart,
            MethodName::ComputersForceStop,
            MethodName::ComputersDismissError,
            MethodName::MacosSetupRetry,
            MethodName::StorageRead,
            MethodName::StorageReclaim,
            MethodName::ComputersDelete,
        ] {
            assert_eq!(
                code_of(&f, method, target.clone()).await,
                ErrorCode::NotFound,
                "{method:?}"
            );
        }
        assert_eq!(
            code_of(&f, MethodName::LogsQuery, json!({"computerId": UNKNOWN}),).await,
            ErrorCode::NotFound
        );
        let registry = f.context.job_registry().unwrap();
        assert!(registry.list_unacknowledged().unwrap().is_empty());
    }

    #[tokio::test]
    async fn linux_start_is_a_job_that_ends_with_the_runtime_failure() {
        let mut f = fixture();
        seed_linux(&f);
        let started = f
            .call(MethodName::ComputersStart, json!({"computerId": LINUX}))
            .await
            .unwrap();
        let job_id = started["jobId"].as_str().unwrap().to_owned();
        assert!(job_id.starts_with("job:"), "{job_id}");
        let finished = f.next_event("operations.job.finished").await;
        assert_eq!(finished["jobId"], job_id);
        // This test engine has no bundled runtime, so the start itself fails.
        assert!(finished["error"]["message"].is_string(), "{finished}");
    }

    #[tokio::test]
    async fn a_configuration_change_is_the_job_setup_request_and_reports_to_its_client() {
        let mut f = fixture();
        seed_linux(&f);
        // The saved computer differs from what the client edited from, so the engine
        // rejects the stale change without touching a runtime.
        let mut stale = linux_configuration();
        stale["cpus"] = json!(1);
        let started = f
            .call(
                MethodName::ComputersConfigurationChange,
                json!({
                    "change": {"kind": "delete", "computerId": LINUX, "expected": stale},
                    "requestId": "request-stale"
                }),
            )
            .await
            .unwrap();
        assert_eq!(started, json!({"jobId": "setup:request-stale"}));
        // The outcome reaches this socket client, not the in-process "main" one.
        let finished = f.next_event("operations.job.finished").await;
        assert_eq!(finished["jobId"], "setup:request-stale");
        assert!(finished["error"]["message"]
            .as_str()
            .unwrap()
            .contains("changed"));
        // The same request id cannot start twice.
        let again = f
            .call(
                MethodName::ComputersConfigurationChange,
                json!({
                    "change": {"kind": "reorder", "order": [LINUX], "expectedOrder": [LINUX]},
                    "requestId": "request-stale"
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(again.code, ErrorCode::Busy);
    }

    #[tokio::test]
    async fn request_ids_must_be_usable() {
        let f = fixture();
        seed_linux(&f);
        let change = |request_id: Value| {
            json!({
                "change": {"kind": "reorder", "order": [LINUX], "expectedOrder": [LINUX]},
                "requestId": request_id
            })
        };
        for bad in [json!(" "), json!("x".repeat(257))] {
            let error = f
                .call(MethodName::ComputersConfigurationChange, change(bad))
                .await
                .unwrap_err();
            assert!(error.message.contains("request ID"), "{}", error.message);
        }
        assert!(f
            .call(
                MethodName::ComputersConfigurationRetry,
                json!({"requestId": ""}),
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn configuration_retry_names_its_computer_by_id() {
        let f = fixture();
        assert_eq!(
            code_of(
                &f,
                MethodName::ComputersConfigurationRetry,
                json!({"retryComputerId": UNKNOWN}),
            )
            .await,
            ErrorCode::NotFound
        );
        seed_linux(&f);
        let mut f = f;
        let started = f
            .call(
                MethodName::ComputersConfigurationRetry,
                json!({"requestId": "retry-1", "retryComputerId": LINUX}),
            )
            .await
            .unwrap();
        assert_eq!(started, json!({"jobId": "setup:retry-1"}));
        let finished = f.next_event("operations.job.finished").await;
        assert_eq!(finished["jobId"], "setup:retry-1");
    }

    #[tokio::test]
    async fn the_macos_resource_change_needs_a_macos_computer_and_stays_out_of_batches() {
        let f = fixture();
        seed_linux(&f);
        let resources = |computer: &str| {
            json!({
                "kind": "macosResources", "computerId": computer,
                "expected": {"cpus": 4, "memoryGiB": 8, "diskGiB": 64},
                "cpus": 6, "memoryGiB": 12, "requestId": "resources-1"
            })
        };
        assert_eq!(
            code_of(
                &f,
                MethodName::ComputersConfigurationChange,
                json!({"change": resources(UNKNOWN)}),
            )
            .await,
            ErrorCode::NotFound
        );
        assert_eq!(
            code_of(
                &f,
                MethodName::ComputersConfigurationChange,
                json!({"change": resources(LINUX)}),
            )
            .await,
            ErrorCode::UnsupportedRemoteOperation
        );
        let error = f
            .call(
                MethodName::ComputersConfigurationChange,
                json!({"change": {"kind": "batch", "changes": [resources(LINUX)]}}),
            )
            .await
            .unwrap_err();
        assert!(error.message.contains("batch"), "{}", error.message);
    }

    #[tokio::test]
    async fn creating_a_macos_computer_that_is_refused_ends_its_job_with_the_reason() {
        let mut f = fixture();
        let result = f
            .call(
                MethodName::ComputersCreate,
                json!({
                    "kind": "macos",
                    "request": {"name": "t", "cpus": 1, "memoryGiB": 1, "diskGiB": 1}
                }),
            )
            .await
            .unwrap();
        let created: ComputerCreateResult = serde_json::from_value(result).unwrap();
        assert!(created.computer.is_none());
        let finished = f.next_event("operations.job.finished").await;
        assert_eq!(finished["jobId"], created.job_id.as_str());
        assert!(finished["error"]["message"].is_string());
    }

    #[tokio::test]
    async fn deleting_a_linux_computer_needs_its_expected_configuration() {
        let f = fixture();
        seed_linux(&f);
        let error = f
            .call(MethodName::ComputersDelete, json!({"computerId": LINUX}))
            .await
            .unwrap_err();
        assert!(error.message.contains("expected"), "{}", error.message);
        let mut f = f;
        let started = f
            .call(
                MethodName::ComputersDelete,
                json!({
                    "computerId": LINUX,
                    "expected": {"id": LINUX, "name": "other", "cpus": 2, "maxCPUs": 2,
                        "memoryGiB": 2, "maxMemoryGiB": 2, "workspaceStorageGiB": 10,
                        "runtimeStorageGiB": 10},
                    "requestId": "delete-1"
                }),
            )
            .await
            .unwrap();
        assert_eq!(started, json!({"jobId": "setup:delete-1"}));
        let finished = f.next_event("operations.job.finished").await;
        assert!(finished["error"].is_object());
    }

    #[tokio::test]
    async fn linux_computers_have_no_force_stop_and_macos_setup_is_macos_only() {
        let f = fixture();
        seed_linux(&f);
        let target = json!({"computerId": LINUX});
        assert_eq!(
            code_of(&f, MethodName::ComputersForceStop, target.clone()).await,
            ErrorCode::UnsupportedRemoteOperation
        );
        assert_eq!(
            code_of(&f, MethodName::MacosSetupRetry, target.clone()).await,
            ErrorCode::UnsupportedRemoteOperation
        );
        assert_eq!(
            code_of(&f, MethodName::StorageRead, json!({"computerId": UNKNOWN})).await,
            ErrorCode::NotFound
        );
    }

    #[tokio::test]
    async fn dismissing_an_error_leaves_no_job_behind() {
        let f = fixture();
        seed_linux(&f);
        // Without a runtime the action itself fails; either way no job outcome may remain.
        let _ = f
            .call(
                MethodName::ComputersDismissError,
                json!({"computerId": LINUX}),
            )
            .await;
        let registry = f.context.job_registry().unwrap();
        assert!(registry.list_unacknowledged().unwrap().is_empty());
    }

    #[tokio::test]
    async fn skipping_the_computer_use_wait_accepts_any_request_id() {
        let f = fixture();
        assert_eq!(
            f.call(
                MethodName::ComputersCreationSkipComputerUseWait,
                json!({"requestId": "never-started"}),
            )
            .await
            .unwrap(),
            Value::Null
        );
    }

    #[tokio::test]
    async fn identities_verify_and_configure_by_computer_id() {
        let mut f = fixture();
        // With no computers an empty list is verified.
        assert_eq!(
            f.call(
                MethodName::ComputersIdentitiesVerify,
                json!({"identities": []})
            )
            .await
            .unwrap(),
            json!(true)
        );
        let unknown = json!({"identities": [
            {"computerId": UNKNOWN, "name": "A", "email": "a@example.com", "apply": true}
        ]});
        assert!(f
            .call(MethodName::ComputersIdentitiesVerify, unknown.clone())
            .await
            .is_err());
        assert!(f
            .call(MethodName::ComputersIdentitiesConfigure, unknown)
            .await
            .is_err());
        let started = f
            .call(
                MethodName::ComputersIdentitiesConfigure,
                json!({"identities": []}),
            )
            .await
            .unwrap();
        let finished = f.next_event("operations.job.finished").await;
        assert_eq!(finished["jobId"], started["jobId"]);
        assert!(finished["error"].is_null());
    }

    #[tokio::test]
    async fn logs_export_writes_the_selected_computers_and_reports_true() {
        let _turn = EXPORTS.lock().await;
        let mut f = fixture();
        seed_linux(&f);
        assert!(f
            .call(
                MethodName::LogsExport,
                json!({"requests": [], "destination": "/tmp/none.jsonl"}),
            )
            .await
            .is_err());
        let destination = f._root.path().join("export.jsonl");
        let started = f
            .call(
                MethodName::LogsExport,
                json!({
                    "requests": [{"computerId": LINUX}],
                    "destination": destination.to_string_lossy()
                }),
            )
            .await
            .unwrap();
        let finished = f.next_event("operations.job.finished").await;
        assert_eq!(finished["jobId"], started["jobId"]);
        assert_eq!(finished["result"], json!(true), "{finished}");
        let text = std::fs::read_to_string(&destination).unwrap();
        assert!(text.lines().next().unwrap().contains("silo-log-export"));
    }

    #[tokio::test]
    async fn a_second_log_export_waits_for_none_and_reports_why() {
        let _turn = EXPORTS.lock().await;
        let mut f = fixture();
        seed_linux(&f);
        let destination = f._root.path().join("busy.jsonl");
        // One export runs at a time; holding its permit makes the job end at once.
        let permit = silo_engine::log_export::begin_export().unwrap();
        let started = f
            .call(
                MethodName::LogsExport,
                json!({
                    "requests": [{"computerId": LINUX}],
                    "destination": destination.to_string_lossy()
                }),
            )
            .await
            .unwrap();
        let finished = f.next_event("operations.job.finished").await;
        assert_eq!(finished["jobId"], started["jobId"]);
        assert!(finished["error"]["message"]
            .as_str()
            .unwrap()
            .contains("already in progress"));
        drop(permit);
        assert!(!destination.exists());
    }

    #[tokio::test]
    async fn cancelling_a_log_export_ends_only_that_job_without_an_error() {
        let _turn = EXPORTS.lock().await;
        let mut f = fixture();
        seed_linux(&f);
        let destination = f._root.path().join("cancelled.jsonl");
        let started = f
            .call(
                MethodName::LogsExport,
                json!({
                    "requests": [{"computerId": LINUX}],
                    "destination": destination.to_string_lossy()
                }),
            )
            .await
            .unwrap();
        let job_id: JobId = serde_json::from_value(started["jobId"].clone()).unwrap();
        // The export may already be done; either way it never fails because of the cancel.
        let _ = f.context.job_registry().unwrap().cancel(&job_id);
        let finished = f.next_event("operations.job.finished").await;
        assert_eq!(finished["jobId"], started["jobId"]);
        assert!(finished["error"].is_null(), "{finished}");
        assert!(finished["result"].is_boolean());
    }

    #[tokio::test]
    async fn macos_display_methods_refuse_what_is_not_there() {
        let f = fixture();
        for (method, params) in [
            (
                MethodName::MacosDisplayConnect,
                json!({"computerId": UNKNOWN}),
            ),
            (
                MethodName::MacosDisplayResize,
                json!({"computerId": UNKNOWN, "widthPx": 1200, "heightPx": 800}),
            ),
            (
                MethodName::MacosDisplaySession,
                json!({"deviceId": "test-device", "computerId": UNKNOWN}),
            ),
        ] {
            assert!(f.call(method, params).await.is_err(), "{method:?}");
        }
        let stream = f
            .call(
                MethodName::MacosDisplayStream,
                json!({"computerId": UNKNOWN}),
            )
            .await
            .unwrap_err();
        assert_eq!(stream.code, ErrorCode::UnsupportedRemoteOperation);
        assert!(f
            .call(MethodName::MacosTemplateDelete, json!({}))
            .await
            .map(|value| value.is_null())
            .unwrap_or(true));
    }

    #[test]
    fn engine_messages_map_to_stable_error_codes() {
        for (message, code) in [
            ("This computer isn't running.", ErrorCode::NotRunning),
            (
                "This computer is not ready yet. Use Retry setup to finish it, or delete it.",
                ErrorCode::NeedsPersonalizing,
            ),
            (
                "Setup has not finished. The screen is available once the computer has been set up for computer use.",
                ErrorCode::SetupIncomplete,
            ),
            (
                "macOS allows at most two macOS computers running at once on this Mac.",
                ErrorCode::RunningLimit,
            ),
            ("This computer no longer exists.", ErrorCode::NotFound),
            ("The operation was cancelled.", ErrorCode::Cancelled),
            ("The request timed out.", ErrorCode::TimedOut),
            ("Something else broke.", ErrorCode::Internal),
        ] {
            assert_eq!(error_code(message), code, "{message}");
        }
        assert_eq!(
            error_code("Screen Sharing accepted TCP but did not return a valid RFB greeting."),
            ErrorCode::ScreenSharingUnavailable
        );
    }

    #[tokio::test]
    async fn a_failed_lifecycle_keeps_its_notice_for_fjord() {
        let f = fixture();
        let host = SilodRuntimeHost::new(&f.context);
        host.lifecycle_notice(
            "start",
            "dev",
            Some(LINUX.to_owned()),
            Duration::from_secs(1),
            RuntimeLifecycleOutcome::Failed("The runtime would not start.".into()),
        );
        let notices = silo_engine::notices::list(f.context.silod_context()).unwrap();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].body.contains("would not start"));
        // A cancelled action is expected and posts nothing new.
        host.lifecycle_notice(
            "stop",
            "dev",
            Some(LINUX.to_owned()),
            Duration::from_secs(1),
            RuntimeLifecycleOutcome::Cancelled,
        );
        assert_eq!(
            silo_engine::notices::list(f.context.silod_context())
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn an_unexpected_change_is_published_on_its_topic() {
        let mut f = fixture();
        publish_unexpected_change(
            f.context.silod_context(),
            silo_engine::health_watch::UnexpectedChange {
                computer_id: LINUX.into(),
                name: "dev".into(),
                state: "stopped".into(),
            },
        );
        let payload = f.next_event("computers.unexpectedChange").await;
        assert_eq!(
            payload,
            json!({"computerId": LINUX, "name": "dev", "state": "stopped"})
        );
        let typed: silo_api::events::UnexpectedComputerChange =
            serde_json::from_value(payload).unwrap();
        assert_eq!(typed.state, "stopped");
    }

    #[tokio::test]
    async fn each_clients_screens_end_on_their_own() {
        fn gateway() -> silo_engine::macos_display_gateway::Gateway {
            let open: silo_engine::macos_display_gateway::Open =
                Arc::new(|_| Err("no guest in this test".to_owned()));
            silo_engine::macos_display_gateway::Gateway::start(open, vec![]).unwrap()
        }
        let (a, b) = (
            format!("screens-a-{}", uuid::Uuid::new_v4()),
            format!("screens-b-{}", uuid::Uuid::new_v4()),
        );
        let (first, second) = (gateway(), gateway());
        let second_url = second.url();
        {
            let mut gateways = display_gateways();
            gateways.insert((a.clone(), "d".into(), "c".into()), first);
            gateways.insert((b.clone(), "d".into(), "c".into()), second);
        }
        close_client_displays(&a);
        {
            let gateways = display_gateways();
            assert!(!gateways.contains_key(&(a.clone(), "d".into(), "c".into())));
            let kept = gateways.get(&(b.clone(), "d".into(), "c".into())).unwrap();
            assert!(kept.running());
            assert_eq!(kept.url(), second_url);
        }
        close_displays(Some("d"));
        assert!(!display_gateways().contains_key(&(b, "d".into(), "c".into())));
    }

    #[tokio::test]
    async fn storage_is_read_and_reclaimed_for_a_linux_computer_only() {
        let mut f = fixture();
        seed_linux(&f);
        // Reading never starts anything: a computer without a runtime reports zeros or fails
        // with the runtime's reason, never with the macOS refusal.
        match f
            .call(MethodName::StorageRead, json!({"computerId": LINUX}))
            .await
        {
            Ok(state) => assert!(state["history"].is_array()),
            Err(error) => assert_ne!(error.code, ErrorCode::UnsupportedRemoteOperation),
        }
        let started = f
            .call(MethodName::StorageReclaim, json!({"computerId": LINUX}))
            .await
            .unwrap();
        let finished = f.next_event("operations.job.finished").await;
        assert_eq!(finished["jobId"], started["jobId"]);
        // Without a started runtime the reclaim ends with a reason instead of a state.
        assert!(finished["error"]["message"].is_string(), "{finished}");
    }

    #[tokio::test]
    async fn creating_a_linux_computer_validates_its_request_before_a_job_exists() {
        let f = fixture();
        let create = |request_id: &str| {
            json!({
                "kind": "linux",
                "configuration": linux_configuration(),
                "requestId": request_id
            })
        };
        assert!(f
            .call(MethodName::ComputersCreate, create(" "))
            .await
            .is_err());
        assert!(f
            .call(
                MethodName::ComputersCreate,
                json!({"kind": "linux", "configuration": {"id": 5}}),
            )
            .await
            .is_err());
        let registry = f.context.job_registry().unwrap();
        assert!(registry.list_unacknowledged().unwrap().is_empty());
    }

    #[tokio::test]
    async fn every_owned_method_is_wired_to_a_typed_handler() {
        let f = fixture();
        for descriptor in silo_api::methods::method_registry() {
            let method = descriptor.method;
            if !owns(method) {
                continue;
            }
            // Malformed params reach the method's own decoder, never the generic refusal.
            let error = match handle(method, &json!({"unexpected": [1, 2]}), &f.context)
                .await
                .expect("an owned method answers")
            {
                Ok(_) => continue,
                Err(error) => error,
            };
            assert!(
                !error.message.starts_with("The engine does not support"),
                "{} is not implemented",
                descriptor.name
            );
        }
    }

    #[tokio::test]
    async fn creating_a_linux_computer_is_a_setup_job_that_ends_with_its_reason() {
        let mut f = fixture();
        let started = f
            .call(
                MethodName::ComputersCreate,
                json!({
                    "kind": "linux",
                    "configuration": linux_configuration(),
                    "requestId": "create-1"
                }),
            )
            .await
            .unwrap();
        assert_eq!(started, json!({"jobId": "setup:create-1"}));
        // The engine has no creation inputs here, so the job ends with that reason.
        let finished = f.next_event("operations.job.finished").await;
        assert_eq!(finished["jobId"], "setup:create-1");
        assert!(finished["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Creation inputs"));
    }
}
