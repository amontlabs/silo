//! Guest, Files page, terminal and editor methods: `guest.*`, `files.*`, `terminal.*` and
//! `editor.*`. This module decodes the wire types, resolves the computer by `computerId`,
//! runs the engine's blocking guest operations off the async runtime and maps failures onto
//! stable error codes. Behavior lives in `silo_engine::guest`.
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::Value;
use silo_api::{
    jobs::{ErrorCode, JobId},
    methods::{
        common::{EmptyParams, JobStartResult},
        files::{
            ConflictPolicy, FileDownloadParams, FileListParams, FileListResult, FileUploadParams,
            UploadOrigin,
        },
        guest::{
            GuestAccount, GuestAccountParams, GuestEnvVar, GuestExecGetParams, GuestExecGetResult,
            GuestExecInputParams, GuestExecOutput, GuestExecParams, GuestExecResult,
            GuestExecStartParams, GuestExecUser, GuestFileEntry, GuestFileKind, GuestFileReplace,
            GuestFilesChangedPayload, GuestFilesListParams, GuestFilesListResult,
            GuestFilesReadParams, GuestFilesReadResult, GuestFilesRemoveParams,
            GuestFilesRemoveResult, GuestFilesWatchParams, GuestFilesWatchResult,
            GuestFilesWriteParams, GuestFilesWriteResult, GuestPrepareParams, GuestPrepareResult,
            GuestSshAliasEnsureParams, GuestSshAliasEnsureResult, GuestSshAliasRemoveParams,
        },
        terminal::{CommandResult, EditorCommandParams, TerminalCommandParams},
        MethodName,
    },
};
use silo_engine::{
    context::{EngineContext, Event},
    guest::{self, GuestError, GuestUser, Target},
    runtime::{RuntimeHost, RuntimePackageKind, RuntimePaths},
};

use crate::{
    context::SilodContext,
    dispatcher::{method_name, DispatchError, HandlerOutput},
};

use super::{ownership::owns_prefix, HandlerContext};

pub(super) fn owns(method: MethodName) -> bool {
    owns_prefix(method, &["guest.", "files.", "terminal.", "editor."])
}

pub async fn handle(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Option<Result<HandlerOutput, DispatchError>> {
    if !owns(method) {
        return None;
    }
    Some(match method {
        MethodName::GuestAccount => guest_account(params, context).await,
        MethodName::GuestExec => guest_exec(params, context).await,
        MethodName::GuestExecStart => guest_exec_start(params, context).await,
        MethodName::GuestExecInput => guest_exec_input(params, context).await,
        MethodName::GuestExecGet => guest_exec_get(params, context).await,
        MethodName::GuestFilesRead => guest_files_read(params, context).await,
        MethodName::GuestFilesWrite => guest_files_write(params, context).await,
        MethodName::GuestFilesList => guest_files_list(params, context).await,
        MethodName::GuestFilesRemove => guest_files_remove(params, context).await,
        MethodName::GuestFilesWatch => guest_files_watch(params, context).await,
        MethodName::GuestPrepare => guest_prepare(params, context).await,
        MethodName::GuestSshAliasEnsure => ssh_alias_ensure(params, context).await,
        MethodName::GuestSshAliasRemove => ssh_alias_remove(params, context).await,
        MethodName::FilesList => files_list(params, context).await,
        MethodName::FilesUpload => files_upload(params, context).await,
        MethodName::FilesDownload => files_download(params, context).await,
        MethodName::TerminalCommand => terminal_command(params, context).await,
        MethodName::EditorCommand => editor_command(params, context).await,
        MethodName::EditorIncludeNotice => editor_include_notice(params, context).await,
        // `guest.ssh` is a raw duplex stream upgraded after a framed reply; the socket layer
        // has no stream upgrade yet.
        _ => Err(DispatchError::unsupported(method_name(method))),
    })
}

// ---------------------------------------------------------------------------------------
// Shared plumbing

fn output(value: impl Serialize) -> Result<HandlerOutput, DispatchError> {
    serde_json::to_value(value)
        .map(|value| HandlerOutput {
            value,
            event_barrier: None,
        })
        .map_err(|error| DispatchError::internal(error.to_string()))
}

/// A result that must fit one 4 MiB response frame. An oversized value is refused instead of
/// truncated or silently paged.
fn output_fitting(value: impl Serialize, what: &str) -> Result<HandlerOutput, DispatchError> {
    let value =
        serde_json::to_value(value).map_err(|error| DispatchError::internal(error.to_string()))?;
    let encoded = serde_json::to_vec(&value)
        .map_err(|error| DispatchError::internal(error.to_string()))?
        .len();
    if encoded > guest::MAX_RESULT_ENCODED_BYTES {
        return Err(DispatchError::internal(format!(
            "{what} is larger than one response can carry."
        )));
    }
    Ok(HandlerOutput {
        value,
        event_barrier: None,
    })
}

pub(super) fn guest_error(error: GuestError) -> DispatchError {
    DispatchError {
        code: error.code,
        message: error.message,
    }
}

fn engine_of(context: &HandlerContext) -> Arc<SilodContext> {
    Arc::new(context.silod_context().clone())
}

/// For the methods that cannot name another device: a request naming one is refused. The
/// methods that can (launchers, SSH wiring, transfers, watches) take their own branch to
/// `guest_remote` first.
fn local_device(context: &HandlerContext, device_id: Option<&str>) -> Result<(), DispatchError> {
    if device_id
        .is_some_and(|device_id| device_id != context.silod_context().local_device().device_id)
    {
        return Err(DispatchError {
            code: ErrorCode::UnsupportedRemoteOperation,
            message: "This engine does not route requests to remote devices.".to_owned(),
        });
    }
    Ok(())
}

/// Runs blocking guest work off the async runtime.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, GuestError> + Send + 'static,
) -> Result<T, DispatchError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| DispatchError::internal(format!("The guest task failed: {error}")))?
        .map_err(guest_error)
}

fn resolve(engine: &SilodContext, computer_id: &str) -> Result<Target, GuestError> {
    guest::resolve(runtime_paths(engine), &engine.paths().app_data, computer_id)
}

fn user_home(engine: &SilodContext) -> PathBuf {
    engine.paths().home.clone()
}

// ---------------------------------------------------------------------------------------
// The Linux runtime this engine drives

/// Runtime paths that tests install for one `app_data` folder instead of the bundled tools.
fn overrides() -> &'static Mutex<HashMap<PathBuf, RuntimePaths>> {
    static OVERRIDES: OnceLock<Mutex<HashMap<PathBuf, RuntimePaths>>> = OnceLock::new();
    OVERRIDES.get_or_init(Default::default)
}

/// Makes engines whose `app_data` is `app_data` use `paths` as their Linux runtime. For tests
/// that stand in for the bundled `msb`.
#[doc(hidden)]
pub fn use_runtime_paths(app_data: PathBuf, paths: RuntimePaths) {
    overrides()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(app_data, paths);
}

fn runtime_paths(engine: &SilodContext) -> Result<RuntimePaths, String> {
    if let Some(paths) = overrides()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&engine.paths().app_data)
    {
        return Ok(paths.clone());
    }
    silo_engine::runtime::runtime_paths(&GuestRuntimeHost::new(engine))
}

struct GuestRuntimeHost {
    context: Arc<SilodContext>,
    migration: silo_engine::runtime_migration::Controller,
}

impl GuestRuntimeHost {
    fn new(engine: &SilodContext) -> Self {
        Self {
            context: Arc::new(engine.clone()),
            migration: silo_engine::runtime_migration::Controller::new(
                engine.paths().app_data.clone(),
            ),
        }
    }
}

impl RuntimeHost for GuestRuntimeHost {
    fn engine_context(&self) -> Arc<dyn EngineContext> {
        self.context.clone()
    }

    fn runtime_storage(&self) -> Result<PathBuf, String> {
        self.migration.runtime_storage()
    }

    fn blocks_operations(&self) -> bool {
        self.migration.blocks_operations()
    }

    fn bundled_tools_directory(&self) -> Result<PathBuf, String> {
        let executable = std::env::current_exe()
            .map_err(|_| "Silo could not locate its bundled tools.".to_owned())?;
        silo_engine::bundled_tools::resolve(
            &executable,
            &self.context.paths().resources,
            Some(silo_engine::bundled_tools::BundleKind::App),
            None,
        )
    }

    fn ensure_runtime_ready(&self) -> Result<(), String> {
        self.migration.ensure_ready()
    }

    fn runtime_package_kind(&self) -> RuntimePackageKind {
        RuntimePackageKind::MacApp
    }
}

// ---------------------------------------------------------------------------------------
// guest.account, guest.exec, guest.exec.start/input/get

async fn guest_account(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: GuestAccountParams = context.decode_params(params)?;
    local_device(context, params.device_id.as_deref())?;
    let engine = engine_of(context);
    let account = blocking(move || {
        let target = resolve(&engine, &params.computer_id)?;
        target.ensure_running()?;
        target.account()
    })
    .await?;
    output(GuestAccount {
        user: account.user,
        uid: account.uid,
        home: account.home,
    })
}

fn env_pairs(env: Option<Vec<GuestEnvVar>>) -> Vec<(String, String)> {
    env.unwrap_or_default()
        .into_iter()
        .map(|variable| (variable.name, variable.value))
        .collect()
}

fn run_as(user: Option<GuestExecUser>) -> GuestUser {
    match user {
        Some(GuestExecUser::Root) => GuestUser::Root,
        _ => GuestUser::Account,
    }
}

async fn guest_exec(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: GuestExecParams = context.decode_params(params)?;
    local_device(context, params.device_id.as_deref())?;
    if params.timeout_ms == 0 || params.timeout_ms > guest::MAX_BOUNDED_TIMEOUT_MS {
        return Err(DispatchError::internal(format!(
            "Invalid params: timeoutMs must be between 1 and {}. Use guest.exec.start for longer runs.",
            guest::MAX_BOUNDED_TIMEOUT_MS
        )));
    }
    let max_stdout = match params.max_output_bytes {
        None => guest::DEFAULT_MAX_OUTPUT_BYTES,
        Some(bytes) if (1..=guest::MAX_OUTPUT_BYTES as u64).contains(&bytes) => bytes as usize,
        Some(_) => {
            return Err(DispatchError::internal(format!(
                "Invalid params: maxOutputBytes must be between 1 and {}.",
                guest::MAX_OUTPUT_BYTES
            )))
        }
    };
    let request = guest::ExecRequest {
        argv: params.argv,
        cwd: params.cwd,
        env: env_pairs(params.env),
        user: run_as(params.as_user),
        stdin: params.stdin,
        timeout: Duration::from_millis(params.timeout_ms),
        max_stdout,
    };
    guest::validate_request(&request.argv, request.cwd.as_deref(), &request.env)
        .map_err(guest_error)?;
    let engine = engine_of(context);
    let computer_id = params.computer_id;
    let result = blocking(move || {
        let target = resolve(&engine, &computer_id)?;
        guest::run_bounded(&target, &request)
    })
    .await?;
    output_fitting(
        GuestExecResult {
            exit_code: result.exit_code,
            signal: result.signal,
            stdout: result.stdout,
            stderr: result.stderr,
        },
        "The command's output",
    )
}

async fn guest_exec_start(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed: GuestExecStartParams = context.decode_params(params)?;
    if let Some(device) = remote_device(context, typed.device_id.as_deref()) {
        return remote_exec::<JobStartResult>(device, "guest.exec.start", &typed).await;
    }
    let engine = engine_of(context);
    let accepted = tokio::task::spawn_blocking(move || start_exec(engine, typed))
        .await
        .map_err(|error| DispatchError::internal(format!("The guest task failed: {error}")))??;
    output(accepted)
}

/// The device a request names when it is not this one.
fn remote_device(context: &HandlerContext, device_id: Option<&str>) -> Option<String> {
    device_id
        .filter(|device| *device != context.silod_context().local_device().device_id)
        .map(str::to_owned)
}

/// A command on another device's computer is a job of that device: it runs there, its
/// output stays there for `guest.exec.get`, and the client follows it by `(deviceId, jobId)`.
/// `guest.exec.input` names the device too, so a remote command can keep stdin open.
async fn remote_exec<T: serde::de::DeserializeOwned + Serialize + Send + 'static>(
    device: String,
    method: &'static str,
    params: &(impl Serialize + ?Sized),
) -> Result<HandlerOutput, DispatchError> {
    let mut request =
        serde_json::to_value(params).map_err(|error| DispatchError::internal(error.to_string()))?;
    if let Some(object) = request.as_object_mut() {
        object.remove("deviceId");
    }
    let reply =
        tokio::task::spawn_blocking(move || super::access::call_owner(&device, method, request))
            .await
            .map_err(|error| DispatchError::internal(format!("The remote task failed: {error}")))?
            .map_err(HandlerContext::map_engine_error)?;
    let typed: T = serde_json::from_value(reply).map_err(|error| {
        DispatchError::internal(format!(
            "The other device returned an invalid reply to {method}: {error}"
        ))
    })?;
    output(typed)
}

/// Starts a command on one of this engine's own computers as a job and returns its id. An
/// owner serving a controller's `guest.exec.start` runs the same steps.
pub(crate) fn start_exec(
    engine: Arc<SilodContext>,
    params: GuestExecStartParams,
) -> Result<JobStartResult, DispatchError> {
    let request = guest::ExecStart {
        argv: params.argv,
        cwd: params.cwd,
        env: env_pairs(params.env),
        user: run_as(params.as_user),
        stdin: params.stdin,
        stdin_open: params.stdin_open,
        retain_output_bytes: params.retain_output_bytes,
    };
    // Refuse before a job exists: only a valid start on a running computer is accepted.
    let target = resolve(&engine, &params.computer_id).map_err(guest_error)?;
    let prepared = guest::StartedExec::prepare(&target, &request).map_err(guest_error)?;
    let accepted = silo_engine::jobs::allocate(engine.as_ref(), "guest.exec.start")
        .map_err(HandlerContext::map_registry_error)?;
    prepared
        .launch(engine, accepted.job_id.clone())
        .map_err(guest_error)?;
    Ok(JobStartResult {
        job_id: accepted.job_id,
    })
}

async fn guest_exec_input(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: GuestExecInputParams = context.decode_params(params)?;
    if let Some(device) = remote_device(context, params.device_id.as_deref()) {
        return remote_exec::<Value>(device, "guest.exec.input", &params).await;
    }
    exec_input_owner(&params)?;
    output(Value::Null)
}

/// Feeds or closes the stdin of a job of this engine, for a local caller or a controller.
pub(crate) fn exec_input_owner(params: &GuestExecInputParams) -> Result<(), DispatchError> {
    guest::exec_input(
        &params.job_id,
        params.data.clone(),
        params.close.unwrap_or(false),
    )
    .map_err(guest_error)
}

async fn guest_exec_get(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed: GuestExecGetParams = context.decode_params(params)?;
    if let Some(device) = remote_device(context, typed.device_id.as_deref()) {
        return remote_exec::<GuestExecGetResult>(device, "guest.exec.get", &typed).await;
    }
    exec_get_output(context.engine_context(), &typed)
}

/// The retained output of a job of this engine, for a local caller or for a controller.
pub(crate) fn exec_get_output(
    engine: &dyn EngineContext,
    params: &GuestExecGetParams,
) -> Result<HandlerOutput, DispatchError> {
    let snapshot =
        guest::exec_get(engine, &params.job_id, params.since_seq).map_err(guest_error)?;
    output_fitting(
        GuestExecGetResult {
            state: snapshot.state,
            exit_code: snapshot.exit_code,
            output: snapshot
                .output
                .into_iter()
                .map(|(seq, stream, data)| GuestExecOutput { seq, stream, data })
                .collect(),
            first_retained_seq: snapshot.first_retained_seq,
        },
        "The replayed output",
    )
}

// ---------------------------------------------------------------------------------------
// guest.files.*

async fn guest_files_read(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: GuestFilesReadParams = context.decode_params(params)?;
    local_device(context, params.device_id.as_deref())?;
    let engine = engine_of(context);
    let read = blocking(move || {
        let target = resolve(&engine, &params.computer_id)?;
        guest::read(&target, &params.path, params.offset, params.length)
    })
    .await?;
    output_fitting(
        GuestFilesReadResult {
            data: read.data,
            size: read.size,
            mtime_ms: read.mtime_ms,
            inode: read.inode,
            mode: read.mode,
        },
        "The file range",
    )
}

async fn guest_files_write(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: GuestFilesWriteParams = context.decode_params(params)?;
    local_device(context, params.device_id.as_deref())?;
    // `expectedSha256` is present-or-absent and may be an explicit null ("must not exist").
    let replace = match (params.replace, params.expected_sha256) {
        (GuestFileReplace::Always, None) => guest::Replace::Always,
        (GuestFileReplace::Never, None) => guest::Replace::Never,
        (GuestFileReplace::IfSha256, Some(expected)) => guest::Replace::IfSha256(expected),
        (GuestFileReplace::IfSha256, None) => {
            return Err(DispatchError::internal(
                "Invalid params: replace ifSha256 requires expectedSha256, which may be null for a file that must not exist.",
            ))
        }
        (_, Some(_)) => {
            return Err(DispatchError::internal(
                "Invalid params: expectedSha256 applies only with replace ifSha256.",
            ))
        }
    };
    let request = guest::WriteRequest {
        path: params.path,
        data: params.data,
        mode: params.mode,
        create_parents: params.create_parents.unwrap_or(false),
        replace,
    };
    let engine = engine_of(context);
    let computer_id = params.computer_id;
    let written = blocking(move || {
        let target = resolve(&engine, &computer_id)?;
        guest::write(&target, &request)
    })
    .await?;
    output(GuestFilesWriteResult {
        size: written.size,
        mtime_ms: written.mtime_ms,
        inode: written.inode,
    })
}

async fn guest_files_list(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: GuestFilesListParams = context.decode_params(params)?;
    local_device(context, params.device_id.as_deref())?;
    let engine = engine_of(context);
    let entries = blocking(move || {
        let target = resolve(&engine, &params.computer_id)?;
        guest::list(&target, &params.path)
    })
    .await?;
    output_fitting(
        GuestFilesListResult {
            entries: entries
                .into_iter()
                .map(|entry| GuestFileEntry {
                    name: entry.name,
                    kind: match entry.kind {
                        guest::FileKind::File => GuestFileKind::File,
                        guest::FileKind::Dir => GuestFileKind::Dir,
                        guest::FileKind::Link => GuestFileKind::Link,
                        guest::FileKind::Other => GuestFileKind::Other,
                    },
                    size: entry.size,
                    mtime_ms: entry.mtime_ms,
                })
                .collect(),
        },
        "The folder listing",
    )
}

async fn guest_files_remove(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: GuestFilesRemoveParams = context.decode_params(params)?;
    local_device(context, params.device_id.as_deref())?;
    let engine = engine_of(context);
    let removed = blocking(move || {
        let target = resolve(&engine, &params.computer_id)?;
        guest::remove(&target, &params.paths)
    })
    .await?;
    output(GuestFilesRemoveResult { removed })
}

/// Starts the folder watch of `computer_id` on this engine: `emit` gets the watched folder of
/// each hint and the watch ends when `wanted` turns false.
fn start_target_watch(
    engine: &SilodContext,
    computer_id: &str,
    paths: &[String],
    emit: Arc<dyn Fn(String) + Send + Sync>,
    wanted: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<(), GuestError> {
    let target = resolve(engine, computer_id)?;
    guest::start_watch(&target, paths, emit, wanted).map(|_| ())
}

/// The owner's half of a controller's `guest.files.watch` stream (`remote_watch`).
pub(crate) fn start_owner_watch(
    engine: &SilodContext,
    params: &Value,
    emit: Arc<dyn Fn(String) + Send + Sync>,
    wanted: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<(), String> {
    let computer_id = params["computerId"]
        .as_str()
        .ok_or("Invalid params: computerId is required.")?;
    let paths: Vec<String> = serde_json::from_value(params["paths"].clone())
        .map_err(|_| "Invalid params: paths must be a list of folders.".to_owned())?;
    start_target_watch(engine, computer_id, &paths, emit, wanted).map_err(|error| error.message)
}

async fn guest_files_watch(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: GuestFilesWatchParams = context.decode_params(params)?;
    if let Some(device) = super::guest_remote::remote_device(context, params.device_id.as_deref()) {
        return super::guest_remote::files_watch(device, params, context).await;
    }
    local_device(context, params.device_id.as_deref())?;
    let engine = engine_of(context);
    let client = context.client_id().clone();
    // The watch is the caller's own subscription: it carries the sequence, is activated with
    // the response, and ends with `events.unsubscribe` or when the client disconnects.
    let subscription = context
        .silod_context()
        .subscribe_events(&client, vec!["guest.files.changed".to_owned()])
        .map_err(DispatchError::internal)?;
    let bus = context.event_bus().clone();
    let started = {
        let (client, subscription, bus) = (client.clone(), subscription.clone(), bus);
        blocking(move || {
            let emit = {
                let (client, subscription, bus) =
                    (client.clone(), subscription.clone(), bus.clone());
                Arc::new(move |path: String| {
                    let payload = GuestFilesChangedPayload {
                        subscription_id: subscription.clone(),
                        path,
                    };
                    if let Ok(payload) = serde_json::to_value(payload) {
                        bus.publish_to_subscription(
                            &client,
                            &subscription,
                            Event::new("guest.files.changed", payload),
                        );
                    }
                })
            };
            let wanted = Arc::new(move || bus.has_subscription(&client, &subscription));
            start_target_watch(&engine, &params.computer_id, &params.paths, emit, wanted)
        })
        .await
    };
    if let Err(error) = started {
        context
            .silod_context()
            .unsubscribe_events(&client, &subscription);
        return Err(error);
    }
    output(GuestFilesWatchResult {
        subscription_id: subscription,
    })
}

// ---------------------------------------------------------------------------------------
// guest.prepare, guest.sshAlias.*

async fn guest_prepare(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed: GuestPrepareParams = context.decode_params(params)?;
    local_device(context, typed.device_id.as_deref())?;
    let request = params.clone();
    let engine = engine_of(context);
    let result = blocking(move || {
        let target = resolve(&engine, &typed.computer_id)?;
        guest::prepare(&target, &request)
    })
    .await?;
    let result: GuestPrepareResult = serde_json::from_value(result).map_err(|error| {
        DispatchError::internal(format!(
            "The computer returned an invalid preparation: {error}"
        ))
    })?;
    output(result)
}

async fn ssh_alias_ensure(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: GuestSshAliasEnsureParams = context.decode_params(params)?;
    if let Some(device) = super::guest_remote::remote_device(context, params.device_id.as_deref()) {
        return super::guest_remote::ssh_alias_ensure(device, params, context).await;
    }
    local_device(context, params.device_id.as_deref())?;
    let engine = engine_of(context);
    let macos = super::display::macos_context(context);
    let outcome = blocking(move || {
        let target = resolve(&engine, &params.computer_id)?;
        let request = guest::AliasEnsure {
            install_include: params.install_include.unwrap_or(true),
            path: params.path,
        };
        match target {
            Target::Macos(_) => guest::macos::ensure_alias(&macos, &target, &request),
            Target::Linux { .. } => guest::ensure_alias(&target, &user_home(&engine), &request),
        }
    })
    .await?;
    output(GuestSshAliasEnsureResult {
        alias: outcome.alias,
        user: outcome.user,
        home: outcome.home,
        config_path: outcome.config_path,
        include_line: outcome.include_line,
        include_installed: outcome.include_installed,
    })
}

async fn ssh_alias_remove(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: GuestSshAliasRemoveParams = context.decode_params(params)?;
    if let Some(device) = super::guest_remote::remote_device(context, params.device_id.as_deref()) {
        return super::guest_remote::ssh_alias_remove(device, params, context).await;
    }
    local_device(context, params.device_id.as_deref())?;
    let engine = engine_of(context);
    let macos = super::display::macos_context(context);
    blocking(move || {
        let target = resolve(&engine, &params.computer_id)?;
        match target {
            Target::Macos(_) => guest::macos::remove_alias(&macos, &target),
            Target::Linux { .. } => guest::remove_alias(&target, &user_home(&engine)),
        }
    })
    .await?;
    output(Value::Null)
}

// ---------------------------------------------------------------------------------------
// files.* (the Files page)

/// The Linux computer a transfer or listing runs against, or `None` for a macOS computer,
/// which goes through the macOS access adapters instead.
fn linux_computer(target: &Target) -> Option<(RuntimePaths, String)> {
    match target {
        Target::Linux { paths, name, .. } => Some((paths.clone(), name.clone())),
        Target::Macos(_) => None,
    }
}

/// The page-cache owner of a macOS folder listing: this connection's client id.
fn listing_caller(context: &HandlerContext) -> String {
    context
        .client_id()
        .as_str()
        .chars()
        .filter(|character| !character.is_control())
        .take(128)
        .collect::<String>()
}

async fn files_list(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: FileListParams = context.decode_params(params)?;
    local_device(context, params.device_id.as_deref())?;
    let offset = usize::try_from(params.offset)
        .map_err(|_| DispatchError::internal("Invalid folder request."))?;
    let engine = engine_of(context);
    let macos = super::display::macos_context(context);
    let caller = listing_caller(context);
    let page = blocking(move || {
        let target = resolve(&engine, &params.computer_id)?;
        match linux_computer(&target) {
            Some((paths, name)) => Ok(silo_engine::files::list_computer_directory(
                &paths,
                name,
                params.path,
                offset,
                params.snapshot_id,
            )?),
            None => Ok(silo_engine::macos_computers::list_directory(
                &macos,
                caller,
                params.computer_id,
                params.path,
                offset,
                params.snapshot_id,
            )?),
        }
    })
    .await?;
    // The Files page and the wire share one paging shape.
    let page =
        serde_json::to_value(page).map_err(|error| DispatchError::internal(error.to_string()))?;
    let page: FileListResult =
        serde_json::from_value(page).map_err(|error| DispatchError::internal(error.to_string()))?;
    output(page)
}

pub(super) fn check_transfer_id(id: &str) -> Result<(), DispatchError> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(DispatchError::internal(
            "Invalid params: transferId must be 1 to 64 letters, digits, '_' or '-'.",
        ));
    }
    Ok(())
}

/// Starts a transfer on a worker thread and returns once its job exists, so the client
/// always gets a `jobId` it can follow, cancel and read back later.
pub(super) async fn run_transfer(
    engine: Arc<SilodContext>,
    job_id: JobId,
    work: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<HandlerOutput, DispatchError> {
    let registry = engine.job_registry().map_err(DispatchError::internal)?;
    let exists = || {
        registry
            .get(&job_id)
            .map(|lookup| lookup.is_some())
            .map_err(HandlerContext::map_registry_error)
    };
    if exists()? {
        return Err(DispatchError {
            code: ErrorCode::Busy,
            message: "This transfer ID is already in use.".to_owned(),
        });
    }
    let mut task = tokio::task::spawn_blocking(work);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if exists()? {
            return output(JobStartResult { job_id });
        }
        match tokio::time::timeout(Duration::from_millis(5), &mut task).await {
            Ok(Ok(Ok(()))) => return output(JobStartResult { job_id }),
            Ok(Ok(Err(message))) => {
                // A transfer that failed after it was accepted reports through its job.
                return if exists()? {
                    output(JobStartResult { job_id })
                } else {
                    Err(guest_error(GuestError::from(message)))
                };
            }
            Ok(Err(error)) => {
                return Err(DispatchError::internal(format!(
                    "The transfer task failed: {error}"
                )))
            }
            Err(_) => {}
        }
        if Instant::now() >= deadline {
            return Err(DispatchError {
                code: ErrorCode::TimedOut,
                message: "The transfer did not start in time.".to_owned(),
            });
        }
    }
}

async fn files_upload(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: FileUploadParams = context.decode_params(params)?;
    if let Some(device) = super::guest_remote::remote_device(context, params.device_id.as_deref()) {
        return super::guest_remote::files_upload(device, params, context).await;
    }
    local_device(context, params.device_id.as_deref())?;
    check_transfer_id(&params.transfer_id)?;
    if params.selection.is_some() {
        return Err(DispatchError::internal(
            "Invalid params: pass the files to upload as paths; selection tokens belong to the app's own pickers.",
        ));
    }
    let Some(paths) = params.paths.filter(|paths| !paths.is_empty()) else {
        return Err(DispatchError::internal(
            "Invalid params: pass the files to upload as paths.",
        ));
    };
    // A viewer drop is held to the Downloads folder by the transfer owner's name; whether the
    // connection belongs to the caller is the viewer domain's check.
    let owner = match params.origin {
        Some(UploadOrigin::Viewer) => {
            let Some(connection) = params.connection_id.as_deref().filter(|id| !id.is_empty())
            else {
                return Err(DispatchError::internal(
                    "Invalid params: a viewer upload needs its connectionId.",
                ));
            };
            format!("desktop-shell-{connection}")
        }
        _ => "main".to_owned(),
    };
    let conflict = match params.conflict {
        ConflictPolicy::Ask => silo_engine::transfer::ConflictPolicy::Ask,
        ConflictPolicy::Replace => silo_engine::transfer::ConflictPolicy::Replace,
        ConflictPolicy::KeepBoth => silo_engine::transfer::ConflictPolicy::KeepBoth,
    };
    let engine = engine_of(context);
    let target = {
        let (engine, computer_id) = (engine.clone(), params.computer_id.clone());
        blocking(move || resolve(&engine, &computer_id)).await?
    };
    let linux = linux_computer(&target);
    let macos = super::display::macos_context(context);
    let job_id = JobId::new(format!("transfer:{}", params.transfer_id))
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    let (transfer, directory, computer_id) =
        (params.transfer_id, params.directory, params.computer_id);
    run_transfer(engine, job_id, move || {
        let selection = silo_engine::transfer::issue_upload_selection(&owner, paths);
        let token = serde_json::to_value(&selection)
            .ok()
            .and_then(|value| value["token"].as_str().map(str::to_owned))
            .ok_or("Could not prepare the upload.")?;
        match linux {
            Some((runtime, name)) => silo_engine::transfer::upload_files(
                Some(&runtime),
                owner,
                transfer,
                name,
                directory,
                token,
                conflict,
            ),
            None => silo_engine::macos_computers::upload_files(
                &macos,
                owner,
                transfer,
                computer_id,
                directory,
                token,
                conflict,
            ),
        }
        .map(|_| ())
    })
    .await
}

async fn files_download(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: FileDownloadParams = context.decode_params(params)?;
    if let Some(device) = super::guest_remote::remote_device(context, params.device_id.as_deref()) {
        return super::guest_remote::files_download(device, params, context).await;
    }
    local_device(context, params.device_id.as_deref())?;
    check_transfer_id(&params.transfer_id)?;
    if params
        .destination
        .as_deref()
        .is_some_and(|destination| !destination.starts_with('/'))
    {
        return Err(DispatchError::internal(
            "Invalid params: destination must be an absolute path.",
        ));
    }
    let engine = engine_of(context);
    let target = {
        let (engine, computer_id) = (engine.clone(), params.computer_id.clone());
        blocking(move || resolve(&engine, &computer_id)).await?
    };
    let linux = linux_computer(&target);
    let macos = super::display::macos_context(context);
    let job_id = JobId::new(format!("transfer:{}", params.transfer_id))
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    let downloads = user_home(&engine).join("Downloads");
    let (transfer, path, destination, computer_id) = (
        params.transfer_id,
        params.path,
        params.destination,
        params.computer_id,
    );
    run_transfer(engine, job_id, move || {
        let (session, file_name) = match linux {
            Some((runtime, name)) => {
                silo_engine::transfer::prepare_download(Some(&runtime), transfer, name, path)?
            }
            None => {
                silo_engine::macos_computers::prepare_download(&macos, transfer, computer_id, path)?
            }
        };
        let destination = destination
            .map(PathBuf::from)
            .unwrap_or_else(|| downloads.join(file_name));
        silo_engine::transfer::download_prepared(session, Some(destination)).map(|_| ())
    })
    .await
}

// ---------------------------------------------------------------------------------------
// terminal.command, editor.command, editor.includeNotice

async fn terminal_command(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: TerminalCommandParams = context.decode_params(params)?;
    if let Some(device) = super::guest_remote::remote_device(context, params.device_id.as_deref()) {
        return super::guest_remote::terminal_command(device, params, context).await;
    }
    local_device(context, params.device_id.as_deref())?;
    let engine = engine_of(context);
    let macos = super::display::macos_context(context);
    let argv = blocking(move || {
        let target = resolve(&engine, &params.computer_id)?;
        match target {
            Target::Macos(_) => guest::macos::terminal_command(&macos, &target),
            Target::Linux { .. } => guest::terminal_command(&target),
        }
    })
    .await?;
    output(CommandResult { argv })
}

async fn editor_command(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: EditorCommandParams = context.decode_params(params)?;
    if let Some(device) = super::guest_remote::remote_device(context, params.device_id.as_deref()) {
        return super::guest_remote::editor_command(device, params, context).await;
    }
    local_device(context, params.device_id.as_deref())?;
    let engine = engine_of(context);
    let macos = super::display::macos_context(context);
    let argv = blocking(move || {
        let target = resolve(&engine, &params.computer_id)?;
        let settings = silo_engine::settings::current_settings()?;
        match target {
            Target::Macos(_) => {
                guest::macos::editor_command(&macos, &target, params.path.as_deref(), &settings)
            }
            Target::Linux { .. } => guest::editor_command(
                &target,
                &user_home(&engine),
                params.path.as_deref(),
                &settings,
            ),
        }
    })
    .await?;
    output(CommandResult { argv })
}

async fn editor_include_notice(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let _: EmptyParams = context.decode_params(params)?;
    let notice =
        silo_engine::editor::read_editor_include_notice(&context.engine_context().paths().home)
            .map_err(|message| guest_error(message.into()))?;
    output(notice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dispatcher::RequestContext, identity::LocalDeviceIdentity};
    use serde_json::json;
    use silo_engine::context::{
        ClientId, EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl,
    };
    use silo_engine::events::EventDelivery;

    const COMPUTER: &str = "00000000-0000-4000-8000-0000000000aa";
    const FAKE_MSB: &str = r#"#!/bin/sh
case "$1" in
  inspect)
    printf '%s\n' '{"name":"dev","status":"Running","config":{"labels":{"silo.managed":"true"}},"runtime_instance_id":"instance-1"}'
    exit 0 ;;
  --silo-working-account-protocol)
    printf '1\n'; exit 0 ;;
  exec)
    shift
    case " $* " in *" test -d /workspace "*|*" test -d /workspace/project "*) exit 0 ;; esac
    while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do
      if [ "$1" = "--env" ]; then shift; export "$1"; fi
      shift
    done
    shift
    exec "$@" ;;
esac
exit 2
"#;

    /// Command sessions live in a process-wide table keyed by job id, and every fixture's job
    /// registry hands out `job:1`, `job:2`, ... again. Tests that start local command jobs
    /// hold this lock so two fixtures never own the same job id at the same time.
    static EXEC_JOB_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn exclusive_exec_jobs() -> std::sync::MutexGuard<'static, ()> {
        EXEC_JOB_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    struct NoopMainThread;
    impl MainThread for NoopMainThread {
        fn run(&self, _work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
            Ok(())
        }
    }

    struct NoopProcess;
    impl ProcessControl for NoopProcess {
        fn exit(&self, _code: i32) {}

        fn restart(&self) -> ! {
            panic!("guest handlers never restart the engine")
        }
    }

    /// An engine whose Linux runtime is a fake `msb` that runs commands on this machine.
    struct Fixture {
        directory: tempfile::TempDir,
        engine: Arc<SilodContext>,
        context: HandlerContext,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path();
            let executable = root.join("msb");
            std::fs::write(&executable, FAKE_MSB).unwrap();
            std::os::unix::fs::PermissionsExt::set_mode(
                &mut std::fs::metadata(&executable).unwrap().permissions(),
                0o700,
            );
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
                    .unwrap();
            }
            std::fs::write(root.join("libkrunfw"), b"").unwrap();
            std::fs::create_dir_all(root.join("home")).unwrap();
            let paths = RuntimePaths {
                guest_image: root.join("guest-image"),
                executable,
                home: root.join("home"),
                storage_home: None,
                library: root.join("libkrunfw"),
                metadata: root.join("computers.json"),
                volumes: root.join("volumes"),
            };
            std::fs::write(
                &paths.metadata,
                json!({
                    "schemaVersion": 1,
                    "computers": [{
                        "id": COMPUTER, "name": "dev", "cpus": 1, "maxCPUs": 2,
                        "memoryGiB": 2, "maxMemoryGiB": 4,
                        "workspaceStorageGiB": 10, "runtimeStorageGiB": 10
                    }]
                })
                .to_string(),
            )
            .unwrap();
            let app_data = root.join("app-data");
            let engine = Arc::new(SilodContext::new(
                EnginePaths {
                    app_data: app_data.clone(),
                    home: root.join("user"),
                    ..EnginePaths::default()
                },
                EngineIdentity {
                    channel: silo_engine::channel::Channel::Development,
                    version: "0.1.0-test".into(),
                    package_kind: PackageKind::MacApp,
                },
                LocalDeviceIdentity {
                    device_id: "this-device".into(),
                    name: "This Mac".into(),
                },
                Arc::new(NoopMainThread),
                Arc::new(NoopProcess),
            ));
            std::fs::create_dir_all(root.join("user")).unwrap();
            use_runtime_paths(app_data, paths);
            let context = HandlerContext::new(
                engine.clone(),
                RequestContext {
                    client_id: "client-a".into(),
                    protocol: 6,
                    client: None,
                },
            );
            Self {
                directory,
                engine,
                context,
            }
        }

        fn path(&self, name: &str) -> String {
            self.directory
                .path()
                .join(name)
                .to_string_lossy()
                .into_owned()
        }

        async fn call(&self, method: MethodName, params: Value) -> Result<Value, DispatchError> {
            handle(method, &params, &self.context)
                .await
                .expect("the guest module owns this method")
                .map(|output| output.value)
        }

        async fn ok(&self, method: MethodName, params: Value) -> Value {
            self.call(method, params).await.unwrap()
        }

        async fn code(&self, method: MethodName, params: Value) -> ErrorCode {
            self.call(method, params).await.unwrap_err().code
        }
    }

    fn bytes(text: &str) -> Value {
        json!(text.as_bytes())
    }

    fn decode(value: &Value) -> Vec<u8> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|byte| byte.as_u64().unwrap() as u8)
            .collect()
    }

    async fn settled(fixture: &Fixture, job: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let snapshot = fixture
                .ok(
                    MethodName::GuestExecGet,
                    json!({"jobId": job, "deviceId": "this-device"}),
                )
                .await;
            if snapshot["state"] != "running" {
                return snapshot;
            }
            assert!(Instant::now() < deadline, "the job never ended");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    #[tokio::test]
    async fn guest_account_reports_the_user_uid_and_home_read_from_the_guest() {
        let fixture = Fixture::new();
        let account = fixture
            .ok(MethodName::GuestAccount, json!({"computerId": COMPUTER}))
            .await;
        assert_eq!(account["user"], "silo");
        assert_eq!(account["uid"], unsafe { libc::getuid() });
        assert!(account["home"].as_str().unwrap().starts_with('/'));
        assert_eq!(
            fixture
                .code(
                    MethodName::GuestAccount,
                    json!({"computerId": "00000000-0000-4000-8000-00000000ffff"})
                )
                .await,
            ErrorCode::NotFound
        );
    }

    #[tokio::test]
    async fn other_devices_are_refused_by_every_per_computer_method() {
        let fixture = Fixture::new();
        for (method, params) in [
            (MethodName::GuestAccount, json!({})),
            (
                MethodName::GuestExec,
                json!({"argv": ["true"], "timeoutMs": 1000}),
            ),
            (MethodName::GuestFilesList, json!({"path": "/"})),
            (MethodName::GuestFilesRead, json!({"path": "/"})),
            (MethodName::GuestFilesRemove, json!({"paths": []})),
            (
                MethodName::GuestFilesWrite,
                json!({"path": "/x", "data": [], "mode": 384, "replace": "always"}),
            ),
        ] {
            let mut params = params;
            params["computerId"] = json!(COMPUTER);
            params["deviceId"] = json!("another-device");
            assert_eq!(
                fixture.code(method, params).await,
                ErrorCode::UnsupportedRemoteOperation,
                "{method:?}"
            );
        }
    }

    #[tokio::test]
    async fn launchers_aliases_and_watches_for_another_device_ask_that_device() {
        let fixture = Fixture::new();
        // They act on this device's own SSH files and clients, so they do the local half
        // themselves and reach for the owner; a device that is not connected is the answer.
        for (method, params) in [
            (MethodName::GuestFilesWatch, json!({"paths": ["/"]})),
            (MethodName::GuestSshAliasEnsure, json!({})),
            (MethodName::GuestSshAliasRemove, json!({})),
            (MethodName::TerminalCommand, json!({})),
            (MethodName::EditorCommand, json!({})),
        ] {
            let mut params = params;
            params["computerId"] = json!(COMPUTER);
            params["deviceId"] = json!("another-device");
            let code = fixture.code(method, params).await;
            assert_ne!(code, ErrorCode::UnsupportedRemoteOperation, "{method:?}");
        }
    }

    #[tokio::test]
    async fn a_command_on_another_device_is_the_owners_job_and_its_output_stays_there() {
        let fixture = Fixture::new();
        let device = "99999999-9999-4999-8999-999999999911";
        let seen = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
        let log = seen.clone();
        let _owner = super::super::access::tests::stub_owner(device, move |method, params| {
            log.lock()
                .unwrap()
                .push((method.to_owned(), params.clone()));
            match method {
                "guest.exec.start" => Ok(json!({"jobId": "job:31"})),
                "guest.exec.get" => Ok(json!({
                    "state": "running",
                    "output": [{"seq": 4, "stream": "stdout", "data": bytes("hi")}],
                    "firstRetainedSeq": 1,
                })),
                other => panic!("unexpected owner request {other}"),
            }
        });
        let started = fixture
            .ok(
                MethodName::GuestExecStart,
                json!({
                    "computerId": COMPUTER, "deviceId": device, "argv": ["true"],
                    "stdinOpen": false, "retainOutputBytes": 1024,
                }),
            )
            .await;
        // The id is the owner's: no job of this engine stands in for it.
        assert_eq!(started, json!({"jobId": "job:31"}));
        let replay = fixture
            .ok(
                MethodName::GuestExecGet,
                json!({"jobId": "job:31", "deviceId": device, "sinceSeq": 3}),
            )
            .await;
        assert_eq!(replay["state"], "running");
        assert_eq!(replay["output"][0]["seq"], 4);
        let seen = seen.lock().unwrap();
        // The owner is told what to run, not which device was meant.
        assert_eq!(seen[0].0, "guest.exec.start");
        assert!(seen[0].1.get("deviceId").is_none());
        assert_eq!(seen[0].1["computerId"], COMPUTER);
        assert_eq!(seen[1].0, "guest.exec.get");
        assert_eq!(seen[1].1, json!({"jobId": "job:31", "sinceSeq": 3}));
    }

    #[tokio::test]
    async fn a_command_on_another_device_keeps_stdin_open_and_takes_input_there() {
        let fixture = Fixture::new();
        let device = "99999999-9999-4999-8999-999999999912";
        let seen = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
        let log = seen.clone();
        let _owner = super::super::access::tests::stub_owner(device, move |method, params| {
            log.lock()
                .unwrap()
                .push((method.to_owned(), params.clone()));
            match method {
                "guest.exec.start" => Ok(json!({"jobId": "job:41"})),
                "guest.exec.input" => Ok(Value::Null),
                other => panic!("unexpected owner request {other}"),
            }
        });
        let started = fixture
            .ok(
                MethodName::GuestExecStart,
                json!({
                    "computerId": COMPUTER, "deviceId": device, "argv": ["cat"],
                    "stdinOpen": true, "retainOutputBytes": 1024,
                }),
            )
            .await;
        assert_eq!(started, json!({"jobId": "job:41"}));
        fixture
            .ok(
                MethodName::GuestExecInput,
                json!({"jobId": "job:41", "deviceId": device, "data": [104, 105], "close": true}),
            )
            .await;
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].1["stdinOpen"], true);
        assert_eq!(seen[1].0, "guest.exec.input");
        // The owner is told what to feed, not which device was meant.
        assert!(seen[1].1.get("deviceId").is_none());
        assert_eq!(seen[1].1["jobId"], "job:41");
        assert_eq!(seen[1].1["close"], true);
    }

    #[tokio::test]
    async fn an_owner_serves_a_controllers_command_as_its_own_job_with_replayable_output() {
        let _jobs = exclusive_exec_jobs();
        let fixture = Fixture::new();
        let typed: GuestExecStartParams = serde_json::from_value(json!({
            "computerId": COMPUTER, "argv": ["sh", "-c", "printf owner-output"],
            "stdinOpen": false, "retainOutputBytes": 1024,
        }))
        .unwrap();
        let accepted = start_exec(fixture.engine.clone(), typed).expect("the owner accepts it");
        let get = GuestExecGetParams {
            job_id: accepted.job_id.clone(),
            device_id: None,
            since_seq: None,
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        let replay = loop {
            let replay = exec_get_output(fixture.engine.as_ref(), &get)
                .unwrap()
                .value;
            if replay["state"] == "exited" {
                break replay;
            }
            assert!(
                Instant::now() < deadline,
                "the command never ended: {replay}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        assert_eq!(replay["exitCode"], 0);
        assert!(replay["output"]
            .as_array()
            .is_some_and(|chunks| !chunks.is_empty()));
        // The job is in the owner's registry under the id it returned.
        let found = silo_engine::jobs::get_at(fixture.engine.as_ref(), &accepted.job_id, None)
            .unwrap()
            .expect("the owner keeps the job");
        assert_eq!(found.job_id, accepted.job_id);
    }

    #[tokio::test]
    async fn guest_exec_runs_bounded_commands_with_a_configurable_stdout_cap() {
        let fixture = Fixture::new();
        let result = fixture
            .ok(
                MethodName::GuestExec,
                json!({
                    "computerId": COMPUTER,
                    "argv": ["sh", "-c", "cat; printf %s \"$GREETING\" >&2; exit 4"],
                    "env": [{"name": "GREETING", "value": "hi"}],
                    "as": "account",
                    "stdin": bytes("input"),
                    "timeoutMs": 20000
                }),
            )
            .await;
        assert_eq!(result["exitCode"], 4);
        assert_eq!(decode(&result["stdout"]), b"input");
        assert_eq!(decode(&result["stderr"]), b"hi");

        let overflow = json!({
            "computerId": COMPUTER,
            "argv": ["sh", "-c", "head -c 100 /dev/zero"],
            "timeoutMs": 20000,
            "maxOutputBytes": 99
        });
        assert_eq!(
            fixture.code(MethodName::GuestExec, overflow).await,
            ErrorCode::OutputTooLarge
        );
        for bad in [
            json!({"argv": ["true"], "timeoutMs": 0}),
            json!({"argv": ["true"], "timeoutMs": guest::MAX_BOUNDED_TIMEOUT_MS + 1}),
            json!({"argv": ["true"], "timeoutMs": 1000, "maxOutputBytes": 0}),
            json!({"argv": ["true"], "timeoutMs": 1000, "maxOutputBytes": guest::MAX_OUTPUT_BYTES + 1}),
            json!({"argv": [], "timeoutMs": 1000}),
            json!({"argv": ["true"], "timeoutMs": 1000, "env": [{"name": "A=B", "value": ""}]}),
        ] {
            let mut bad = bad;
            bad["computerId"] = json!(COMPUTER);
            assert_eq!(
                fixture.code(MethodName::GuestExec, bad).await,
                ErrorCode::Internal
            );
        }
        let slow = json!({
            "computerId": COMPUTER, "argv": ["sleep", "30"], "timeoutMs": 300
        });
        assert_eq!(
            fixture.code(MethodName::GuestExec, slow).await,
            ErrorCode::TimedOut
        );
    }

    #[tokio::test]
    async fn exec_jobs_take_input_stream_output_replay_it_and_cancel() {
        let _jobs = exclusive_exec_jobs();
        let fixture = Fixture::new();
        let started = fixture
            .ok(
                MethodName::GuestExecStart,
                json!({
                    "computerId": COMPUTER,
                    "argv": ["sh", "-c", "cat; exit 2"],
                    "stdinOpen": true,
                    "retainOutputBytes": 1024
                }),
            )
            .await;
        let job = started["jobId"].as_str().unwrap().to_owned();
        assert!(job.starts_with("job:"));
        fixture
            .ok(
                MethodName::GuestExecInput,
                json!({"jobId": job, "data": bytes("hello ")}),
            )
            .await;
        fixture
            .ok(
                MethodName::GuestExecInput,
                json!({"jobId": job, "data": bytes("world"), "close": true}),
            )
            .await;
        // Either the input side is closed or the job already ended.
        let late = fixture
            .code(
                MethodName::GuestExecInput,
                json!({"jobId": job, "data": bytes("late")}),
            )
            .await;
        assert!(
            matches!(late, ErrorCode::StdinClosed | ErrorCode::NotRunning),
            "{late:?}"
        );
        let snapshot = settled(&fixture, &job).await;
        assert_eq!(snapshot["state"], "exited");
        assert_eq!(snapshot["exitCode"], 2);
        let replay: Vec<u8> = snapshot["output"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|chunk| decode(&chunk["data"]))
            .collect();
        assert_eq!(replay, b"hello world");
        assert_eq!(snapshot["firstRetainedSeq"], 1);
        let last = snapshot["output"].as_array().unwrap().last().unwrap()["seq"].clone();
        let after = fixture
            .ok(
                MethodName::GuestExecGet,
                json!({"jobId": job, "sinceSeq": last}),
            )
            .await;
        assert!(after["output"].as_array().unwrap().is_empty());

        // The outcome is saved before the job reads as finished, and cancel is the generic route.
        let registry = fixture.engine.job_registry().unwrap();
        let outcome = registry.get(&started["jobId"].as_str().unwrap().parse().unwrap());
        assert_eq!(
            outcome.unwrap().unwrap().outcome.unwrap().result.unwrap()["exitCode"],
            2
        );

        let sleeper = fixture
            .ok(
                MethodName::GuestExecStart,
                json!({"computerId": COMPUTER, "argv": ["sleep", "60"],
                       "stdinOpen": false, "retainOutputBytes": 0}),
            )
            .await;
        let sleeper_job = sleeper["jobId"].as_str().unwrap().to_owned();
        registry.cancel(&sleeper_job.parse().unwrap()).unwrap();
        assert_eq!(settled(&fixture, &sleeper_job).await["state"], "cancelled");

        let oversized = json!({"computerId": COMPUTER, "argv": ["true"], "stdinOpen": false,
                               "retainOutputBytes": guest::MAX_RESPONSE_RAW_BYTES + 1});
        assert_eq!(
            fixture.code(MethodName::GuestExecStart, oversized).await,
            ErrorCode::Internal
        );
        assert_eq!(
            fixture
                .code(MethodName::GuestExecGet, json!({"jobId": "job:987654"}))
                .await,
            ErrorCode::NotFound
        );
        assert_eq!(
            fixture
                .code(MethodName::GuestExecInput, json!({"jobId": "job:987654"}))
                .await,
            ErrorCode::NotRunning
        );
    }

    #[tokio::test]
    async fn guest_files_write_read_list_and_remove_round_trip_with_conditional_writes() {
        let fixture = Fixture::new();
        let path = fixture.path("settings/hook.sh");
        let write = |replace: &str, expected: Option<Value>, text: &str| {
            let mut params = json!({
                "computerId": COMPUTER, "path": path, "data": bytes(text),
                "mode": 0o755, "createParents": true, "replace": replace
            });
            if let Some(expected) = expected {
                params["expectedSha256"] = expected;
            }
            params
        };
        let written = fixture
            .ok(MethodName::GuestFilesWrite, write("never", None, "v1"))
            .await;
        assert_eq!(written["size"], 2);
        // `never` and an explicit null both mean "must not exist".
        assert_eq!(
            fixture
                .code(MethodName::GuestFilesWrite, write("never", None, "x"))
                .await,
            ErrorCode::Exists
        );
        assert_eq!(
            fixture
                .code(
                    MethodName::GuestFilesWrite,
                    write("ifSha256", Some(Value::Null), "x")
                )
                .await,
            ErrorCode::Changed
        );
        let digest = |text: &str| {
            use std::io::Write;
            let mut child = std::process::Command::new("/usr/bin/shasum")
                .args(["-a", "256"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            String::from_utf8(output.stdout).unwrap()[..64].to_owned()
        };
        assert_eq!(
            fixture
                .code(
                    MethodName::GuestFilesWrite,
                    write("ifSha256", Some(json!(digest("stale"))), "x")
                )
                .await,
            ErrorCode::Changed
        );
        fixture
            .ok(
                MethodName::GuestFilesWrite,
                write("ifSha256", Some(json!(digest("v1"))), "v2"),
            )
            .await;
        for invalid in [
            write("ifSha256", None, "x"),
            write("always", Some(Value::Null), "x"),
        ] {
            assert_eq!(
                fixture.code(MethodName::GuestFilesWrite, invalid).await,
                ErrorCode::Internal
            );
        }
        let read = fixture
            .ok(
                MethodName::GuestFilesRead,
                json!({"computerId": COMPUTER, "path": path, "offset": 1, "length": 10}),
            )
            .await;
        assert_eq!(decode(&read["data"]), b"2");
        assert_eq!(read["size"], 2);
        assert_eq!(read["mode"], 0o755);
        assert!(read["inode"].as_u64().unwrap() > 0);
        let folder = fixture.path("settings");
        let listing = fixture
            .ok(
                MethodName::GuestFilesList,
                json!({"computerId": COMPUTER, "path": folder}),
            )
            .await;
        assert_eq!(listing["entries"][0]["name"], "hook.sh");
        assert_eq!(listing["entries"][0]["type"], "file");
        let missing = fixture.path("settings/none");
        let removed = fixture
            .ok(
                MethodName::GuestFilesRemove,
                json!({"computerId": COMPUTER, "paths": [path, missing]}),
            )
            .await;
        assert_eq!(removed["removed"].as_array().unwrap().len(), 2);
        assert_eq!(
            fixture
                .code(
                    MethodName::GuestFilesRead,
                    json!({"computerId": COMPUTER, "path": path})
                )
                .await,
            ErrorCode::NotFound
        );
        assert_eq!(
            fixture
                .code(
                    MethodName::GuestFilesRemove,
                    json!({"computerId": COMPUTER, "paths": [folder]})
                )
                .await,
            ErrorCode::NotAFile
        );
    }

    #[tokio::test]
    async fn a_file_watch_is_the_callers_own_subscription_and_ends_when_it_is_dropped() {
        let fixture = Fixture::new();
        let client = ClientId::new("client-a");
        let mut events = fixture.engine.connect_event_stream(client.clone());
        let folder = fixture.path("inbox");
        std::fs::create_dir(&folder).unwrap();
        let watch = fixture
            .ok(
                MethodName::GuestFilesWatch,
                json!({"computerId": COMPUTER, "paths": [folder]}),
            )
            .await;
        let subscription = watch["subscriptionId"].as_str().unwrap().to_owned();
        // The dispatcher activates it with the response.
        fixture
            .engine
            .activate_events(&client, &subscription)
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1200)).await;
        std::fs::write(format!("{folder}/message"), b"hi").unwrap();
        let delivery = tokio::time::timeout(Duration::from_secs(15), events.recv())
            .await
            .unwrap()
            .unwrap();
        let EventDelivery::Event(event) = delivery else {
            panic!("expected a change event");
        };
        assert_eq!(event.subscription, subscription);
        assert_eq!(event.event.name, "guest.files.changed");
        assert_eq!(event.event.payload["subscriptionId"], subscription);
        assert_eq!(event.event.payload["path"], folder);

        fixture.engine.unsubscribe_events(&client, &subscription);
        tokio::time::sleep(Duration::from_millis(1500)).await;
        std::fs::write(format!("{folder}/after"), b"x").unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(800), events.recv())
                .await
                .map_or(true, |delivery| !matches!(
                    delivery,
                    Some(EventDelivery::Event(_))
                ))
        );
        assert_eq!(
            fixture
                .code(
                    MethodName::GuestFilesWatch,
                    json!({"computerId": COMPUTER, "paths": []})
                )
                .await,
            ErrorCode::Internal
        );
    }

    #[tokio::test]
    async fn guest_prepare_authorizes_the_controllers_key_and_pins_the_host_key() {
        let fixture = Fixture::new();
        let key = fixture.directory.path().join("controller_key");
        let generated = std::process::Command::new("/usr/bin/ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(&key)
            .status()
            .unwrap();
        assert!(generated.success());
        // The wire form is the key type and the key, without a comment.
        let public = std::fs::read_to_string(key.with_extension("pub"))
            .unwrap()
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        let result = fixture
            .ok(
                MethodName::GuestPrepare,
                json!({
                    "computerId": COMPUTER, "publicKey": public.trim(),
                    "path": "/workspace", "accountProtocol": 1
                }),
            )
            .await;
        assert_eq!(result["user"], "silo");
        assert!(result["hostPublicKey"]
            .as_str()
            .unwrap()
            .starts_with("ssh-ed25519 "));
        assert!(result["guestAddress"].is_null());
        let authorized =
            std::fs::read_to_string(fixture.directory.path().join("home/ssh/authorized_keys"))
                .unwrap();
        assert!(authorized.lines().any(|line| line == public.trim()));
        for bad in [
            json!({"publicKey": "ssh-rsa AAAA", "path": "/workspace", "accountProtocol": 1}),
            json!({"publicKey": public.trim(), "path": "/etc", "accountProtocol": 1}),
            json!({"publicKey": public.trim(), "path": "/workspace", "accountProtocol": 2}),
        ] {
            let mut bad = bad;
            bad["computerId"] = json!(COMPUTER);
            assert!(fixture.call(MethodName::GuestPrepare, bad).await.is_err());
        }
    }

    /// Another engine for the owner side of a remote request: the controller's request
    /// reaches this engine's real handlers, and only the SSH hop is missing.
    fn engine_as_owner(device: &str, owner: &Fixture) -> crate::methods::access::tests::StubOwner {
        let engine = owner.engine.clone();
        crate::methods::access::tests::stub_owner(device, move |method, params| {
            crate::methods::remote::serve_owner(&engine, method, params)
        })
    }

    #[tokio::test]
    async fn a_remote_alias_authorizes_the_controllers_key_on_the_owners_real_guest() {
        const OWNER_DEVICE: &str = "88888888-8888-4888-8888-888888888831";
        let owner = Fixture::new();
        let controller = Fixture::new();
        let _owner = engine_as_owner(OWNER_DEVICE, &owner);
        let result = controller
            .ok(
                MethodName::GuestSshAliasEnsure,
                json!({"computerId": COMPUTER, "deviceId": OWNER_DEVICE, "path": "/workspace/project"}),
            )
            .await;
        // The account comes from the owner's guest, and the alias lives on the controller.
        assert_eq!(result["user"], "silo");
        assert!(result["home"].as_str().unwrap().starts_with('/'));
        let config = std::path::PathBuf::from(result["configPath"].as_str().unwrap());
        assert!(
            config.starts_with(controller.directory.path()),
            "{config:?}"
        );
        assert!(!config.starts_with(owner.directory.path()));
        let config_text = std::fs::read_to_string(&config).unwrap();
        assert!(config_text.contains(&format!("'--remote-guest' '{OWNER_DEVICE}' '{COMPUTER}'")));
        // The controller's per-device key is now authorized in the owner's guest account.
        let key = config.parent().unwrap().join(format!("{OWNER_DEVICE}.key"));
        let public = std::process::Command::new("/usr/bin/ssh-keygen")
            .arg("-y")
            .arg("-f")
            .arg(&key)
            .output()
            .unwrap();
        let public = String::from_utf8(public.stdout).unwrap();
        let authorized =
            std::fs::read_to_string(owner.directory.path().join("home/ssh/authorized_keys"))
                .unwrap();
        let wire_key = public
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            authorized.lines().any(|line| line == wire_key),
            "{authorized}"
        );
        // The pinned host key is the owner's.
        let known_hosts = std::fs::read_to_string(config.with_extension("known_hosts")).unwrap();
        assert!(known_hosts.contains("ssh-ed25519 "), "{known_hosts}");
        // A folder outside the workspace is refused by the owner, with its message.
        let error = controller
            .call(
                MethodName::GuestSshAliasEnsure,
                json!({"computerId": COMPUTER, "deviceId": OWNER_DEVICE, "path": "/etc"}),
            )
            .await
            .unwrap_err();
        assert!(error.message.contains("/workspace"), "{error:?}");
    }

    #[tokio::test]
    async fn a_remote_watch_runs_on_the_owner_and_reaches_the_controllers_subscription() {
        const OWNER_DEVICE: &str = "88888888-8888-4888-8888-888888888832";
        let owner = Fixture::new();
        let controller = Fixture::new();
        let folder = owner.path("inbox");
        std::fs::create_dir(&folder).unwrap();
        // The owner side is the real stream loop over a socket pair; the controller reads it
        // with the real frame decoder.
        let engine = owner.engine.clone();
        let _stream = super::super::guest_remote::tests::stub_watch(OWNER_DEVICE, move |params| {
            let (mut client, mut server) =
                std::os::unix::net::UnixStream::pair().map_err(|error| error.to_string())?;
            let watching =
                silo_engine::remote_watch::Watching::start(params, |params, emit, wanted| {
                    super::start_owner_watch(&engine, params, emit, wanted)
                })?;
            let peer = server.try_clone().map_err(|error| error.to_string())?;
            std::thread::spawn(move || {
                silo_engine::remote_watch::serve(
                    &mut server,
                    &watching,
                    &|| silo_engine::remote::connection_open(&peer),
                    &|| true,
                    Duration::from_millis(200),
                );
            });
            let reader = client.try_clone().map_err(|error| error.to_string())?;
            let (sender, frames) = std::sync::mpsc::sync_channel(64);
            std::thread::spawn(move || {
                let mut reader = reader;
                loop {
                    let frame = silo_engine::remote_watch::read_watch_frame(&mut reader);
                    let failed = frame.is_err();
                    if sender.send(frame).is_err() || failed {
                        break;
                    }
                }
            });
            let _ = &mut client;
            Ok(super::super::guest_remote::WatchLink {
                frames,
                _guard: Box::new(client),
            })
        });
        let client = ClientId::new("client-a");
        let mut events = controller.engine.connect_event_stream(client.clone());
        let watch = controller
            .ok(
                MethodName::GuestFilesWatch,
                json!({"computerId": COMPUTER, "deviceId": OWNER_DEVICE, "paths": [folder]}),
            )
            .await;
        let subscription = watch["subscriptionId"].as_str().unwrap().to_owned();
        controller
            .engine
            .activate_events(&client, &subscription)
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1200)).await;
        std::fs::write(format!("{folder}/message"), b"hi").unwrap();
        let delivery = tokio::time::timeout(Duration::from_secs(20), events.recv())
            .await
            .unwrap()
            .unwrap();
        let EventDelivery::Event(event) = delivery else {
            panic!("expected a change event");
        };
        assert_eq!(event.subscription, subscription);
        assert_eq!(event.event.name, "guest.files.changed");
        assert_eq!(event.event.payload["path"], folder);
        // Unsubscribing ends the relay, which lets go of the stream and so of the watch.
        controller.engine.unsubscribe_events(&client, &subscription);
    }

    #[tokio::test]
    async fn ssh_aliases_are_written_by_id_and_removal_is_idempotent() {
        let fixture = Fixture::new();
        // The fake runtime cannot speak SSH, so the final connection check fails; everything
        // before it has already happened and the entry is in place for the next attempt.
        let error = fixture
            .call(
                MethodName::GuestSshAliasEnsure,
                json!({"computerId": COMPUTER, "path": "/workspace/project"}),
            )
            .await
            .unwrap_err();
        assert!(error
            .message
            .starts_with("Could not connect to this computer over SSH"));
        let config = fixture
            .directory
            .path()
            .join(format!("home/ssh/{COMPUTER}.conf"));
        assert!(config.exists());
        let user_config =
            std::fs::read_to_string(fixture.directory.path().join("user/.ssh/config")).unwrap();
        assert!(user_config.starts_with("Include "));
        assert_eq!(
            fixture
                .code(
                    MethodName::GuestSshAliasEnsure,
                    json!({"computerId": COMPUTER, "path": "/etc"})
                )
                .await,
            ErrorCode::Internal
        );
        for _ in 0..2 {
            let removed = fixture
                .ok(
                    MethodName::GuestSshAliasRemove,
                    json!({"computerId": COMPUTER}),
                )
                .await;
            assert!(removed.is_null());
        }
        assert!(!config.exists());
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("user/.ssh/config")).unwrap(),
            ""
        );
    }

    #[tokio::test]
    async fn terminal_and_editor_commands_are_returned_for_the_client_to_launch() {
        let fixture = Fixture::new();
        let terminal = fixture
            .ok(MethodName::TerminalCommand, json!({"computerId": COMPUTER}))
            .await;
        let argv: Vec<&str> = terminal["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_str().unwrap())
            .collect();
        assert_eq!(argv[0], "/usr/bin/env");
        assert!(argv[1].starts_with("MSB_HOME="));
        assert_eq!(
            argv[5..].join(" "),
            "exec dev --user silo --env USER=silo --env LOGNAME=silo --no-start --workdir /workspace --tty"
        );
        // The settings store belongs to the installed engine context, which a unit test lacks.
        assert!(fixture
            .call(MethodName::EditorCommand, json!({"computerId": COMPUTER}))
            .await
            .is_err());
        assert!(fixture
            .ok(MethodName::EditorIncludeNotice, json!({}))
            .await
            .is_null());
    }

    #[tokio::test]
    async fn the_files_page_and_transfers_validate_before_any_effect() {
        let fixture = Fixture::new();
        assert_eq!(
            fixture
                .code(
                    MethodName::FilesList,
                    json!({"computerId": COMPUTER, "path": "/etc", "offset": 0})
                )
                .await,
            ErrorCode::Internal
        );
        let upload = |extra: Value| {
            let mut params = json!({
                "transferId": "t-1", "computerId": COMPUTER,
                "directory": "/workspace", "conflict": "ask"
            });
            for (key, value) in extra.as_object().unwrap() {
                params[key] = value.clone();
            }
            params
        };
        for bad in [
            upload(json!({})),
            upload(json!({"paths": []})),
            upload(json!({"selection": "token"})),
            upload(json!({"paths": ["/a"], "transferId": "bad id"})),
            upload(json!({"paths": ["/a"], "origin": "viewer"})),
        ] {
            assert_eq!(
                fixture.code(MethodName::FilesUpload, bad).await,
                ErrorCode::Internal
            );
        }
        let download = |extra: Value| {
            let mut params = json!({
                "transferId": "t-2", "computerId": COMPUTER, "path": "/workspace/a"
            });
            for (key, value) in extra.as_object().unwrap() {
                params[key] = value.clone();
            }
            params
        };
        assert_eq!(
            fixture
                .code(
                    MethodName::FilesDownload,
                    download(json!({"destination": "relative"}))
                )
                .await,
            ErrorCode::Internal
        );
        assert_eq!(
            fixture
                .code(
                    MethodName::FilesDownload,
                    download(json!({"transferId": ""}))
                )
                .await,
            ErrorCode::Internal
        );
        // The fake runtime has no SFTP: the job never exists, so the failure is the response.
        let failure = fixture
            .call(MethodName::FilesDownload, download(json!({})))
            .await
            .unwrap_err();
        assert!(!failure.message.is_empty());
    }

    #[tokio::test]
    async fn a_transfer_answers_with_its_job_as_soon_as_the_job_exists() {
        let fixture = Fixture::new();
        let job_id: JobId = "transfer:accepted-early".parse().unwrap();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let (engine, id) = (fixture.engine.clone(), job_id.clone());
        let started = Instant::now();
        let response = run_transfer(fixture.engine.clone(), job_id.clone(), move || {
            silo_engine::jobs::accept_domain_id(
                &*engine,
                "transfer",
                "accepted-early",
                "files.upload",
            )
            .map_err(|error| error.to_string())?;
            wait.recv().ok();
            silo_engine::jobs::finish_with_result(
                &*engine,
                id,
                json!({"status": "done", "names": []}),
                silo_engine::context::Audience::All,
            )
            .map_err(|error| error.to_string())
        })
        .await
        .unwrap();
        assert_eq!(response.value["jobId"], "transfer:accepted-early");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "must not wait for the transfer"
        );
        release.send(()).unwrap();
        // A second transfer with the same ID is refused while the job exists.
        match run_transfer(fixture.engine.clone(), job_id, || Ok(())).await {
            Err(error) => assert_eq!(error.code, ErrorCode::Busy),
            Ok(_) => panic!("a transfer ID in use must be refused"),
        }
    }

    /// A running macOS computer in the shared inventory whose access material does not exist,
    /// so every adapter stops at its login with a message only the macOS adapters produce.
    fn running_macos_computer(fixture: &Fixture) -> String {
        silo_engine::macos_computers_engine::install_test_computer(
            &fixture.engine.paths().app_data,
            "mac-fixture",
            silo_engine::macos_computers_engine::TestComputerState::Running,
            silo_engine::computer_use::Approval::Ask,
        )
    }

    #[tokio::test]
    async fn macos_computers_reach_the_macos_adapters_for_every_access_method() {
        const NO_ACCESS: &str = "This computer's access is missing.";
        let fixture = Fixture::new();
        let id = running_macos_computer(&fixture);
        let folder = "/Users/silo/workspace";
        let key =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOchaFYV1PlX7OWxqxGXrnZTgG+EqFVeLNIJn42NJJxA";
        let calls = [
            (MethodName::GuestSshAliasEnsure, json!({"computerId": id})),
            (MethodName::TerminalCommand, json!({"computerId": id})),
            (
                MethodName::FilesList,
                json!({"computerId": id, "path": folder, "offset": 0}),
            ),
            (
                MethodName::FilesUpload,
                json!({
                    "transferId": "mac-up", "computerId": id, "directory": folder,
                    "conflict": "ask", "paths": ["/tmp/a"]
                }),
            ),
            (
                MethodName::FilesDownload,
                json!({
                    "transferId": "mac-down", "computerId": id,
                    "path": format!("{folder}/a")
                }),
            ),
            (
                MethodName::GuestPrepare,
                json!({
                    "computerId": id, "publicKey": key, "path": folder,
                    "accountProtocol": 1
                }),
            ),
        ];
        for (method, params) in calls {
            let error = fixture.call(method, params).await.unwrap_err();
            assert_eq!(error.message, NO_ACCESS, "{method:?}");
            assert!(!error.message.contains("pending parity-2"));
        }
        // Removing an alias that was never written is a no-op, like on Linux.
        assert!(fixture
            .ok(MethodName::GuestSshAliasRemove, json!({"computerId": id}))
            .await
            .is_null());
        // Folders outside the macOS workspace are refused before the guest is asked.
        let error = fixture
            .call(
                MethodName::FilesList,
                json!({"computerId": id, "path": "/workspace", "offset": 0}),
            )
            .await
            .unwrap_err();
        assert_eq!(error.message, "Invalid folder request.");
    }

    #[tokio::test]
    async fn a_stopped_macos_computer_is_not_started_by_the_access_methods() {
        let fixture = Fixture::new();
        let id = silo_engine::macos_computers_engine::install_test_computer(
            &fixture.engine.paths().app_data,
            "mac-stopped",
            silo_engine::macos_computers_engine::TestComputerState::Stopped,
            silo_engine::computer_use::Approval::Ask,
        );
        for (method, params) in [
            (MethodName::TerminalCommand, json!({"computerId": id})),
            (MethodName::GuestSshAliasEnsure, json!({"computerId": id})),
        ] {
            let error = fixture.call(method, params).await.unwrap_err();
            assert_eq!(error.message, "Start this computer first.", "{method:?}");
            assert_eq!(error.code, ErrorCode::NotRunning);
        }
    }

    #[tokio::test]
    async fn guest_ssh_streams_are_not_served_by_the_framed_socket_yet() {
        let fixture = Fixture::new();
        assert_eq!(
            fixture
                .code(MethodName::GuestSsh, json!({"computerId": COMPUTER}))
                .await,
            ErrorCode::UnsupportedRemoteOperation
        );
    }
}
