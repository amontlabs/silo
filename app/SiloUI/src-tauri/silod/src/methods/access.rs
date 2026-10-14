//! Access, network, push, SSH access, and Connections methods.

use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use silo_api::{
    jobs::{ErrorCode, JobId, JobState},
    methods::{
        common::JobStartResult, connections, github, network, push, secrets, ssh_access, MethodName,
    },
};
use silo_engine::{
    bridge_error::BridgeError,
    context::{Audience, EngineContext, Event},
    runtime::{self, RuntimeHost, RuntimePackageKind, RuntimePaths},
};

use crate::{
    context::SilodContext,
    dispatcher::{method_name, DispatchError, HandlerOutput},
};

use super::{ownership::owns_prefix, HandlerContext};

pub(super) fn owns(method: MethodName) -> bool {
    owns_prefix(
        method,
        &[
            "github.",
            "secrets.",
            "push.",
            "network.",
            "sshAccess.",
            "connections.",
        ],
    )
}

/// Installs the adapters needed by the engine's access and remote domains and starts
/// their background workers. Called once during silod startup, after the process
/// context is installed.
pub fn install(context: Arc<crate::context::SilodContext>) -> Result<(), String> {
    install_host(context)?;
    silo_engine::github::install()?;
    silo_engine::secrets::install()
}

/// Registers the runtime adapters without starting the GitHub and secrets workers, which
/// read the system credential store. Socket and unit tests install this much.
pub fn install_host(context: Arc<crate::context::SilodContext>) -> Result<(), String> {
    let host = Arc::new(AccessRuntimeHost::new(context));
    silo_engine::github::install_runtime(Arc::new(GitHubRuntimeAccess(host.clone())))?;
    silo_engine::secrets::install_runtime(Arc::new(SecretsRuntimeAccess(host.clone())))?;
    RUNTIME_HOST
        .set(host)
        .map_err(|_| "Access runtime host is already installed".to_owned())
}

static RUNTIME_HOST: std::sync::OnceLock<Arc<AccessRuntimeHost>> = std::sync::OnceLock::new();

struct AccessRuntimeHost {
    context: Arc<crate::context::SilodContext>,
    migration: Arc<silo_engine::runtime_migration::Controller>,
}

impl AccessRuntimeHost {
    fn new(context: Arc<crate::context::SilodContext>) -> Self {
        let migration = Arc::new(silo_engine::runtime_migration::Controller::new(
            context.paths().app_data.clone(),
        ));
        Self { context, migration }
    }

    fn paths(&self) -> Result<RuntimePaths, String> {
        runtime::runtime_paths(self)
    }
}

impl RuntimeHost for AccessRuntimeHost {
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

    fn schedule_secret_revocations(&self) {
        silo_engine::secrets::schedule_revocations();
    }

    fn repository_push_operations(&self, local: Vec<Value>) -> Vec<Value> {
        self.paths()
            .and_then(|paths| silo_engine::host_push_operations::merge(&paths, local.clone()))
            .unwrap_or(local)
    }

    fn github_snapshot(&self) -> Result<Value, String> {
        silo_engine::github::snapshot()
    }
}

struct GitHubRuntimeAccess(Arc<AccessRuntimeHost>);

impl silo_engine::github::RuntimeAccess for GitHubRuntimeAccess {
    fn scoped_cached_tokens(&self, computer: &str) -> Result<runtime::ScopedTokens, String> {
        if macos_proxy::contains_name(computer) {
            return macos_proxy::scoped_cached_tokens(computer);
        }
        runtime::scoped_cached_tokens(self.0.as_ref(), computer)
    }

    fn github_policy_is_cached(&self, computer: &str, profile: &Value) -> Result<bool, String> {
        if macos_proxy::contains_name(computer) {
            return macos_proxy::github_policy_is_cached(computer, profile);
        }
        runtime::github_policy_is_cached(self.0.as_ref(), computer, profile)
    }

    fn apply_github_policy(
        &self,
        computer: &str,
        revision: u64,
        profile: &Value,
    ) -> Result<(), String> {
        if macos_proxy::contains_name(computer) {
            return macos_proxy::apply_github_policy(computer, revision, profile);
        }
        runtime::apply_github_policy(self.0.as_ref(), computer, revision, profile)
    }

    fn apply_github_identity(&self, computer: &str, identity: &Value) -> Result<(), String> {
        if macos_proxy::contains_name(computer) {
            return macos_proxy::apply_github_identity(computer, identity);
        }
        runtime::apply_github_identity(self.0.as_ref(), computer, identity)
    }

    fn is_pending_restore(&self, computer: &str) -> bool {
        if macos_proxy::contains_name(computer) {
            return macos_proxy::is_pending_restore(computer);
        }
        self.0
            .paths()
            .is_ok_and(|paths| runtime::is_pending_restore(&paths, computer))
    }

    fn is_device_removed(&self, computer: &str) -> bool {
        if macos_proxy::contains_name(computer) {
            return false;
        }
        self.0.paths().is_ok_and(|paths| {
            matches!(
                runtime::resolve_computer_id(&paths, computer),
                Err(runtime::RuntimeError::Invalid(_))
            )
        })
    }
}

struct SecretsRuntimeAccess(Arc<AccessRuntimeHost>);

impl silo_engine::secrets::RuntimeAccess for SecretsRuntimeAccess {
    fn apply_secrets(&self, computer: &str) -> Result<Vec<String>, String> {
        if macos_proxy::contains_name(computer) {
            return macos_proxy::apply_secrets(computer);
        }
        runtime::apply_secrets(self.0.as_ref(), computer)
    }

    fn validate_computers(&self, computers: &[String]) -> Result<(), String> {
        let (macos, linux): (Vec<_>, Vec<_>) = computers
            .iter()
            .cloned()
            .partition(|computer| macos_proxy::contains_name(computer));
        if !macos.is_empty() {
            macos_proxy::validate_computers(&macos)?;
        }
        if linux.is_empty() {
            return Ok(());
        }
        runtime::validate_secret_computers(self.0.as_ref(), &linux)
    }

    fn revoke_secret(
        &self,
        record: &silo_engine::secrets::PendingRevocation,
    ) -> Result<bool, String> {
        if macos_proxy::contains_name(record.computer()) {
            return macos_proxy::revoke_secret(record.computer(), record.name());
        }
        runtime::revoke_secret(self.0.as_ref(), record)
    }
}

/// MACOS-PROXY: the call site for the macOS secrets and GitHub host proxy.
///
/// The proxy lives on branch `engine/p3-macos-proxy` and is merged separately. When it
/// lands, replace the bodies below with its `proxy_*` entry points (its Tauri commands
/// show the same shape: `proxy_contains_name`, `proxy_apply_secrets`,
/// `proxy_validate_computers`, `proxy_revoke_secret`, `proxy_scoped_cached_tokens`,
/// `proxy_github_policy_is_cached`, `proxy_apply_github_policy`,
/// `proxy_apply_github_identity`, `proxy_is_pending_restore`). Every Linux path above is
/// complete; until then no computer is a macOS proxy computer, so none of this runs.
mod macos_proxy {
    use serde_json::Value;

    const UNAVAILABLE: &str =
        "Secrets and GitHub access for macOS computers are not available in this build yet.";

    /// Whether `computer` (a name) is a macOS computer served by the host proxy.
    pub(super) fn contains_name(_computer: &str) -> bool {
        false
    }

    pub(super) fn apply_secrets(_computer: &str) -> Result<Vec<String>, String> {
        Err(UNAVAILABLE.into())
    }

    pub(super) fn validate_computers(_computers: &[String]) -> Result<(), String> {
        Err(UNAVAILABLE.into())
    }

    pub(super) fn revoke_secret(_computer: &str, _name: &str) -> Result<bool, String> {
        Err(UNAVAILABLE.into())
    }

    pub(super) fn scoped_cached_tokens(
        _computer: &str,
    ) -> Result<silo_engine::runtime::ScopedTokens, String> {
        Err(UNAVAILABLE.into())
    }

    pub(super) fn github_policy_is_cached(
        _computer: &str,
        _profile: &Value,
    ) -> Result<bool, String> {
        Err(UNAVAILABLE.into())
    }

    pub(super) fn apply_github_policy(
        _computer: &str,
        _revision: u64,
        _profile: &Value,
    ) -> Result<(), String> {
        Err(UNAVAILABLE.into())
    }

    pub(super) fn apply_github_identity(_computer: &str, _identity: &Value) -> Result<(), String> {
        Err(UNAVAILABLE.into())
    }

    pub(super) fn is_pending_restore(_computer: &str) -> bool {
        false
    }
}

struct SilodMacosHost(Arc<AccessRuntimeHost>);

impl silo_engine::macos_computers::MacosHost for SilodMacosHost {
    fn show_display(
        &self,
        _context: &silo_engine::macos_computers::MacosContext,
        _id: &str,
        _title: &str,
        _toolbar: bool,
    ) -> Result<(), String> {
        Err("A remote display cannot be opened by the headless engine process.".into())
    }

    fn close_display(&self, _id: &str) {}

    fn linux_computer_names(&self) -> Vec<String> {
        runtime::computer_names(self.0.as_ref())
    }
}

struct SilodLocalAccess(Arc<AccessRuntimeHost>);

impl silo_engine::remote_access::LocalAccess for SilodLocalAccess {
    fn desktop_connection(&self, computer: &str, computer_id: &str) -> Result<Value, String> {
        let paths = self.0.paths()?;
        silo_engine::desktop_viewer::local_connection(&paths, computer, Some(computer_id))
    }

    fn desktop_dispatch(&self, method: &str, params: &Value) -> Result<Value, String> {
        let paths = self.0.paths()?;
        silo_engine::desktop::dispatch(&paths, method, params)
    }

    fn chatgpt_status(&self) -> Result<Value, String> {
        let root = self.0.context.paths().app_data.join("chatgpt");
        let status = silo_engine::chatgpt_app::local_status(&root)?;
        silo_engine::chatgpt_app::to_value(status)
    }

    fn chatgpt_retry(&self) -> Result<Value, String> {
        let root = self.0.context.paths().app_data.join("chatgpt");
        let status = silo_engine::chatgpt_app::retry_now(
            &root,
            Arc::new(|status| {
                if let Some(context) = silo_engine::context::current() {
                    if let Ok(payload) = silo_engine::chatgpt_app::to_value(status) {
                        context.publish(Event::new("chatgpt.status", payload), Audience::All);
                    }
                }
            }),
            Arc::new(|| {}),
        )?;
        silo_engine::chatgpt_app::to_value(status)
    }

    fn access_dispatch(&self, method: &str, params: &Value) -> Result<Value, String> {
        match method {
            "github.connect" => start_owner_connect(&self.0.context),
            "operations.job.get" | "operations.cancel" | "operations.job.acknowledge" => {
                owner_job_operation(self.0.context.as_ref(), method, params)
            }
            _ => run_local(self.0.context.as_ref(), method, params),
        }
    }

    fn github_callback_stream(&self, params: &Value) -> Result<TcpStream, String> {
        let job_id = params["jobId"]
            .as_str()
            .ok_or("Missing GitHub connection.")?;
        callback_listener_for(job_id)
    }
}

/// Starts the owner-side bridge with silod's engine-only host adapters.
pub fn start_connections(context: Arc<crate::context::SilodContext>) -> Result<(), String> {
    let host = RUNTIME_HOST
        .get()
        .cloned()
        .ok_or_else(|| "Access runtime is not installed.".to_owned())?;
    let resolver_host = host.clone();
    let paths: Arc<silo_engine::remote::RuntimePathsResolver> =
        Arc::new(move || resolver_host.paths());
    let checkpoint_host = host.clone();
    let checkpoints: Arc<silo_engine::remote_access::CheckpointHandler> =
        Arc::new(move |method, params| {
            let paths = checkpoint_host.paths()?;
            let computer_id = params["computerId"]
                .as_str()
                .ok_or("Missing computer identity.")?;
            match method {
                "checkpoint.create" => silo_engine::runtime::checkpoints::create(
                    &paths,
                    computer_id,
                    params["name"].as_str().ok_or("Missing checkpoint name.")?,
                )?,
                "checkpoint.restore" => silo_engine::runtime::checkpoints::restore(
                    &paths,
                    computer_id,
                    params["checkpointId"]
                        .as_str()
                        .ok_or("Missing checkpoint identity.")?,
                )?,
                _ => return Err("This checkpoint operation is not available in silod.".into()),
            }
            Ok(Value::Null)
        });
    let macos = Arc::new(silo_engine::macos_computers::MacosContext::new(
        context.clone(),
        Arc::new(SilodMacosHost(host.clone())),
    ));
    silo_engine::remote::start(
        paths,
        checkpoints,
        host.clone(),
        macos,
        Arc::new(SilodLocalAccess(host)),
    );
    Ok(())
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
        MethodName::GitHubState
        | MethodName::GitHubConnectReopen
        | MethodName::GitHubRepositoriesManage
        | MethodName::GitHubRepositoriesRefresh
        | MethodName::GitHubDisconnect
        | MethodName::GitHubAccessSet
        | MethodName::GitHubConfigurationSave
        | MethodName::GitHubConfigurationRetry
        | MethodName::GitHubPersonalTokenSave
        | MethodName::GitHubPersonalTokenRemove
        | MethodName::SecretsList
        | MethodName::SecretsSave
        | MethodName::SecretsRemove
        | MethodName::SecretsRetry => owner_method(method, params, context).await,
        MethodName::GitHubConnect => github_connect(params, context).await,
        // The callback is an engine-to-engine stream on the Connections bridge
        // (`silo_engine::remote`, `github.connect.callback`); `engine.sock` carries no raw streams.
        MethodName::GitHubConnectCallback => Err(DispatchError {
            code: ErrorCode::UnsupportedRemoteOperation,
            message: "github.connect.callback is served between engines over Connections, not on the local socket.".to_owned(),
        }),
        MethodName::PushRepositories => push_repositories(params, context).await,
        MethodName::PushStart => push_start(params, context).await,
        MethodName::PushStatus => push_status(params, context).await,
        MethodName::PushDismiss => push_dismiss(params, context).await,
        MethodName::NetworkState => network_state(params, context).await,
        MethodName::NetworkPublish => network_change(params, context, true).await,
        MethodName::NetworkUnpublish => network_change(params, context, false).await,
        MethodName::NetworkOpen => network_open(params, context).await,
        MethodName::SshAccessState => ssh_access_state(params, context).await,
        MethodName::SshAccessSave => ssh_access_save(params, context).await,
        MethodName::SshAccessConnection => ssh_access_connection(params, context).await,
        MethodName::ConnectionsStatus => connections_status(context).await,
        MethodName::ConnectionsSetEnabled => connections_set_enabled(params, context).await,
        MethodName::ConnectionsDevicesList => connections_devices_list(context).await,
        MethodName::ConnectionsDevicesConnect => connections_devices_connect(params, context).await,
        MethodName::ConnectionsDevicesRemove => connections_devices_remove(params, context).await,
        MethodName::ConnectionsKeysRevoke => connections_keys_revoke(params, context).await,
        MethodName::ConnectionsDevicesHealth => connections_devices_health(context).await,
        MethodName::ConnectionsSshAuthorizationCommand => {
            connections_ssh_command(params, context, false).await
        }
        MethodName::ConnectionsSshKeySetupCommand => {
            connections_ssh_command(params, context, true).await
        }
        _ => Err(DispatchError::unsupported(method_name(method))),
    })
}

// ---------------------------------------------------------------------------
// Shared plumbing
// ---------------------------------------------------------------------------

/// Maps a plain engine failure message to the canonical error code it deserves.
fn failure(message: impl Into<String>) -> DispatchError {
    let message = message.into();
    let code = if message.starts_with("Silo is installing an update") {
        ErrorCode::UpdateInProgress
    } else if message.to_ascii_lowercase().contains("cancelled") {
        ErrorCode::Cancelled
    } else {
        ErrorCode::Internal
    };
    DispatchError { code, message }
}

fn serialization(error: impl std::fmt::Display) -> DispatchError {
    DispatchError::internal(error.to_string())
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, DispatchError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| DispatchError::internal("The access operation could not finish."))?
        .map_err(failure)
}

/// Runs a blocking call to another device without holding a runtime worker.
async fn blocking_bridge<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, BridgeError> + Send + 'static,
) -> Result<T, DispatchError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| DispatchError::internal("The remote operation could not finish."))?
        .map_err(HandlerContext::map_engine_error)
}

fn runtime_host() -> Result<&'static Arc<AccessRuntimeHost>, String> {
    RUNTIME_HOST
        .get()
        .ok_or_else(|| "The access runtime is not initialized.".to_owned())
}

fn local_paths() -> Result<RuntimePaths, String> {
    runtime_host()?.paths()
}

/// `Some(device)` when a request names another device. Naming this engine's own device
/// is the same as naming none.
fn remote_target(context: &HandlerContext, device_id: Option<&str>) -> Option<String> {
    device_id
        .filter(|id| *id != context.silod_context().local_device().device_id)
        .map(str::to_owned)
}

fn without_device(params: &Value) -> Value {
    let mut params = params.clone();
    if let Some(object) = params.as_object_mut() {
        object.remove("deviceId");
    }
    params
}

/// The name of a Linux computer by identity.
///
/// MACOS-PROXY: macOS computers live in the macOS store, not in this metadata. When the
/// proxy branch is merged, resolve their names here (or ask `macos_proxy`) so secrets and
/// GitHub policies can target them.
fn computer_name(paths: &RuntimePaths, id: &str) -> Result<String, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid computer identity.".to_owned())?;
    runtime::read_metadata(&paths.metadata)
        .map_err(|error| error.to_string())?
        .computers
        .into_iter()
        .find(|computer| computer.id() == id)
        .map(|computer| computer.name().to_owned())
        .ok_or_else(|| "This computer no longer exists.".to_owned())
}

/// The stable identity of a computer by name, or `None` when it no longer exists.
fn computer_id(paths: &RuntimePaths, name: &str) -> Option<String> {
    runtime::resolve_computer_id(paths, name).ok()
}

/// Rows of `field` keyed by a computer `name` gain `computerId`. Rows for computers that
/// no longer exist are dropped.
fn project_name_rows_to_ids(value: &mut Value, paths: &RuntimePaths, fields: &[&str]) {
    for field in fields {
        let Some(rows) = value.get_mut(*field).and_then(Value::as_array_mut) else {
            continue;
        };
        rows.retain_mut(|row| {
            let Some(name) = row.get("computer").and_then(Value::as_str) else {
                return true;
            };
            match computer_id(paths, name) {
                Some(id) => {
                    row["computerId"] = json!(id);
                    true
                }
                None => false,
            }
        });
    }
}

/// A remote owner's rows already carry `computerId`; show the owner's name for `computer`.
fn restore_owner_names(value: &mut Value) {
    let Some(rows) = value.get_mut("computers").and_then(Value::as_array_mut) else {
        return;
    };
    for row in rows {
        let Some(object) = row.as_object_mut() else {
            continue;
        };
        if let Some(name) = object.remove("computerName") {
            object.insert("computer".to_owned(), name);
        }
    }
}

fn project_secret_ids(rows: &mut [Value], paths: &RuntimePaths) -> Result<(), String> {
    for row in rows {
        for (source, destination) in [
            ("computers", "computerIds"),
            ("pendingComputers", "pendingComputerIds"),
        ] {
            let names = row
                .get(source)
                .and_then(Value::as_array)
                .ok_or("Invalid secret assignment state.")?;
            let ids = names
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|name| computer_id(paths, name))
                .collect::<Vec<_>>();
            row[destination] = json!(ids);
            if let Some(object) = row.as_object_mut() {
                object.remove(source);
            }
        }
    }
    Ok(())
}

fn project_github_ids(value: &mut Value, paths: &RuntimePaths) {
    for field in ["computers", "computerOperations"] {
        let Some(rows) = value.get_mut(field).and_then(Value::as_array_mut) else {
            continue;
        };
        rows.retain_mut(|row| {
            let Some(name) = row.get("computer").and_then(Value::as_str) else {
                return true;
            };
            let Some(id) = computer_id(paths, name) else {
                return false;
            };
            row["computerId"] = json!(id);
            if let Some(object) = row.as_object_mut() {
                object.remove("computer");
            }
            true
        });
    }
}

fn api_configuration_to_engine(
    configuration: github::GitHubConfiguration,
    paths: &RuntimePaths,
) -> Result<Value, String> {
    let mut value = serde_json::to_value(configuration).map_err(|error| error.to_string())?;
    for row in value
        .get_mut("computers")
        .and_then(Value::as_array_mut)
        .ok_or("Missing computer policies.")?
    {
        let id = row
            .get("computerId")
            .and_then(Value::as_str)
            .ok_or("Missing computer identity.")?;
        row["computer"] = json!(computer_name(paths, id)?);
        if let Some(object) = row.as_object_mut() {
            object.remove("computerId");
        }
    }
    Ok(value)
}

fn publish(context: &dyn EngineContext, topic: &str, payload: Value) {
    context.publish(Event::new(topic, payload), Audience::All);
}

fn output(value: Value) -> HandlerOutput {
    HandlerOutput {
        value,
        event_barrier: None,
    }
}

fn to_output<T: Serialize>(value: T) -> Result<HandlerOutput, DispatchError> {
    serde_json::to_value(value)
        .map(output)
        .map_err(serialization)
}

/// The latest event sequence of `domain` already delivered to this client. It is read
/// before the state, so any event newer than the state is delivered again, never skipped.
fn read_barrier(
    context: &HandlerContext,
    domain: &str,
) -> Option<std::collections::HashMap<String, u64>> {
    context
        .event_bus()
        .domain_barrier(context.client_id(), domain)
        .ok()
}

/// Round-trips `value` through its canonical type, so a reply that does not match the
/// schema is an error here rather than a malformed answer to the client.
fn canonical<T: DeserializeOwned + Serialize>(value: Value, what: &str) -> Result<Value, String> {
    let typed: T =
        serde_json::from_value(value).map_err(|error| format!("Invalid {what}: {error}"))?;
    serde_json::to_value(typed).map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------
// GitHub and secrets: owner methods
// ---------------------------------------------------------------------------

/// Checks the typed parameters of an owner method before it runs or is routed.
fn validate_owner_params(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Result<(), DispatchError> {
    match method {
        MethodName::GitHubState => {
            context.decode_params::<github::GitHubStateParams>(params)?;
        }
        MethodName::GitHubConnectReopen => {
            context.decode_params::<github::GitHubConnectReopenParams>(params)?;
        }
        MethodName::GitHubRepositoriesManage => {
            context.decode_params::<github::GitHubRepositoriesManageParams>(params)?;
        }
        MethodName::GitHubRepositoriesRefresh => {
            context.decode_params::<github::GitHubRepositoriesRefreshParams>(params)?;
        }
        MethodName::GitHubDisconnect => {
            context.decode_params::<github::GitHubDisconnectParams>(params)?;
        }
        MethodName::GitHubAccessSet => {
            context.decode_params::<github::GitHubAccessSetParams>(params)?;
        }
        MethodName::GitHubConfigurationSave => {
            context.decode_params::<github::GitHubConfigurationSaveParams>(params)?;
        }
        MethodName::GitHubConfigurationRetry => {
            context.decode_params::<github::GitHubConfigurationRetryParams>(params)?;
        }
        MethodName::GitHubPersonalTokenSave => {
            context.decode_params::<github::GitHubPersonalTokenSaveParams>(params)?;
        }
        MethodName::GitHubPersonalTokenRemove => {
            context.decode_params::<github::GitHubPersonalTokenRemoveParams>(params)?;
        }
        MethodName::SecretsList => {
            context.decode_params::<secrets::SecretsListParams>(params)?;
        }
        MethodName::SecretsSave => {
            let typed = context.decode_params::<secrets::SecretsSaveParams>(params)?;
            require_wildcard_acknowledgement(&typed.request).map_err(failure)?;
        }
        MethodName::SecretsRemove => {
            context.decode_params::<secrets::SecretsRemoveParams>(params)?;
        }
        MethodName::SecretsRetry => {
            context.decode_params::<secrets::SecretsRetryParams>(params)?;
        }
        _ => unreachable!("owner_method only receives GitHub and secrets methods"),
    }
    Ok(())
}

/// A secret that may go to any HTTPS destination needs the caller's explicit say-so
/// (95 A-16), checked before anything is stored or sent to another device.
fn require_wildcard_acknowledgement(request: &secrets::SecretSaveRequest) -> Result<(), String> {
    if request
        .allowed_domains
        .iter()
        .any(|domain| domain.trim() == "*")
        && request.allow_any_destination != Some(true)
    {
        return Err(WILDCARD_UNACKNOWLEDGED.to_owned());
    }
    Ok(())
}

const WILDCARD_UNACKNOWLEDGED: &str =
    "Confirm that this secret may go to any HTTPS destination (allowAnyDestination) before saving a wildcard secret.";

async fn owner_method(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    validate_owner_params(method, params, context)?;
    let name = method_name(method);
    let barrier = match method {
        MethodName::GitHubState => read_barrier(context, "github"),
        MethodName::SecretsList => read_barrier(context, "secrets"),
        _ => None,
    };
    let device = remote_target(context, params.get("deviceId").and_then(Value::as_str));
    let value = match device {
        Some(device) => {
            let routed = without_device(params);
            let reply = blocking_bridge(move || {
                silo_engine::remote::call_remote_typed(&device, name, routed)
                    .map(|value| (device, value))
            })
            .await?;
            controller_view(method, &reply.0, reply.1).map_err(failure)?
        }
        None => {
            let engine = context.silod_context().clone();
            let params = params.clone();
            blocking(move || run_local(&engine, name, &params)).await?
        }
    };
    Ok(HandlerOutput {
        value,
        event_barrier: barrier,
    })
}

/// What this controller shows for an owner's reply: the canonical value, and never a
/// loopback authorization link whose callback this controller cannot carry yet.
fn controller_view(method: MethodName, device: &str, value: Value) -> Result<Value, String> {
    match method {
        MethodName::SecretsList
        | MethodName::SecretsSave
        | MethodName::SecretsRemove
        | MethodName::SecretsRetry => canonical::<Vec<secrets::Secret>>(value, "secrets state"),
        MethodName::GitHubConnectReopen => {
            let result: github::GitHubRepositoriesManageResult = serde_json::from_value(value)
                .map_err(|error| format!("Invalid GitHub link: {error}"))?;
            if !link_is_safe(device, &result.url) {
                return Err(ROUTE_UNAVAILABLE.to_owned());
            }
            serde_json::to_value(result).map_err(|error| error.to_string())
        }
        MethodName::GitHubRepositoriesManage => {
            canonical::<github::GitHubRepositoriesManageResult>(value, "GitHub link")
        }
        _ => {
            let mut state: github::GitHubState = serde_json::from_value(value)
                .map_err(|error| format!("Invalid GitHub state: {error}"))?;
            if let Some(url) = state.pending_authorization_url.as_deref() {
                if !link_is_safe(device, url) {
                    state.pending_authorization_url = None;
                }
            }
            serde_json::to_value(state).map_err(|error| error.to_string())
        }
    }
}

const ROUTE_UNAVAILABLE: &str =
    "This Mac is not carrying that GitHub sign-in back to the other Mac. Cancel it and connect again.";

/// Runs one GitHub or secrets method against this engine's own state and returns the
/// canonical result. Mutations publish the canonical changed event. The owner calls this
/// for its own clients and for another device's controller alike.
fn run_local(ctx: &SilodContext, method: &str, params: &Value) -> Result<Value, String> {
    let paths = local_paths()?;
    match method {
        "github.state" => {
            let mut state = silo_engine::github::snapshot()?;
            project_github_ids(&mut state, &paths);
            canonical::<github::GitHubState>(state, "GitHub state")
        }
        "github.connect.reopen" => {
            let url = silo_engine::github::authorization_url()?;
            Ok(json!({ "url": url }))
        }
        "github.repositories.manage" => {
            let url = silo_engine::github::repository_authorization_url()?;
            Ok(json!({ "url": url }))
        }
        "github.repositories.refresh"
        | "github.disconnect"
        | "github.access.set"
        | "github.configuration.save"
        | "github.configuration.retry"
        | "github.personalToken.save"
        | "github.personalToken.remove" => {
            let mut state = github_mutation(method, params, &paths)?;
            project_github_ids(&mut state, &paths);
            let state = canonical::<github::GitHubState>(state, "GitHub state")?;
            publish(ctx, "github.changed", state.clone());
            Ok(state)
        }
        "secrets.list" => {
            let mut rows = silo_engine::secrets::read_secrets_state_sync()?;
            project_secret_ids(&mut rows, &paths)?;
            canonical::<Vec<secrets::Secret>>(json!(rows), "secrets state")
        }
        "secrets.save" => {
            let typed: secrets::SecretsSaveParams = serde_json::from_value(params.clone())
                .map_err(|error| format!("Invalid params: {error}"))?;
            require_wildcard_acknowledgement(&typed.request)?;
            let names = typed
                .request
                .computer_ids
                .iter()
                .map(|id| computer_name(&paths, id))
                .collect::<Result<Vec<_>, _>>()?;
            let mut request =
                serde_json::to_value(typed.request).map_err(|error| error.to_string())?;
            if let Some(object) = request.as_object_mut() {
                object.insert("computers".to_owned(), json!(names));
                object.remove("computerIds");
                object.remove("allowAnyDestination");
            }
            let request: silo_engine::secrets::Request = serde_json::from_value(request)
                .map_err(|error| format!("Invalid secret request: {error}"))?;
            let rows =
                silo_engine::secrets::save_secret(request, silo_engine::updates::operation_guard)?;
            secrets_changed(ctx, rows, &paths)
        }
        "secrets.remove" | "secrets.retry" => {
            let typed: secrets::SecretIdParams = serde_json::from_value(params.clone())
                .map_err(|error| format!("Invalid params: {error}"))?;
            let rows = if method == "secrets.remove" {
                silo_engine::secrets::remove_secret(
                    typed.id,
                    silo_engine::updates::operation_guard,
                )?
            } else {
                silo_engine::secrets::retry_secret(typed.id, silo_engine::updates::operation_guard)?
            };
            secrets_changed(ctx, rows, &paths)
        }
        _ => Err("This Silo version does not support that remote operation.".to_owned()),
    }
}

fn secrets_changed(
    ctx: &SilodContext,
    mut rows: Vec<Value>,
    paths: &RuntimePaths,
) -> Result<Value, String> {
    project_secret_ids(&mut rows, paths)?;
    let rows = canonical::<Vec<secrets::Secret>>(json!(rows), "secrets state")?;
    publish(ctx, "secrets.changed", rows.clone());
    Ok(rows)
}

fn github_mutation(method: &str, params: &Value, paths: &RuntimePaths) -> Result<Value, String> {
    fn typed<T: DeserializeOwned>(params: &Value) -> Result<T, String> {
        serde_json::from_value(params.clone()).map_err(|error| format!("Invalid params: {error}"))
    }
    match method {
        "github.repositories.refresh" => {
            silo_engine::github::prepare_refresh();
            silo_engine::github::run_operation(silo_engine::github::refresh_repositories)
        }
        "github.disconnect" => {
            let ticket = silo_engine::github::prepare_disconnect();
            silo_engine::github::disconnect(ticket, silo_engine::updates::operation_guard)
        }
        "github.access.set" => {
            let typed: github::GitHubAccessSetParams = typed(params)?;
            let ticket = silo_engine::github::prepare_access_change(typed.enabled);
            silo_engine::github::set_access_enabled(
                typed.enabled,
                ticket,
                silo_engine::updates::operation_guard,
            )
        }
        "github.configuration.save" => {
            let typed: github::GitHubConfigurationSaveParams = typed(params)?;
            let configuration = api_configuration_to_engine(typed.configuration, paths)?;
            silo_engine::github::validate_configuration(&configuration)?;
            let ticket = silo_engine::github::intent_ticket();
            silo_engine::github::save_configuration(
                configuration,
                ticket,
                silo_engine::updates::operation_guard,
            )
        }
        "github.configuration.retry" => {
            let typed: github::GitHubConfigurationRetryParams = typed(params)?;
            let computer = typed
                .computer_id
                .as_deref()
                .map(|id| computer_name(paths, id))
                .transpose()?;
            let ticket = silo_engine::github::prepare_retry_configuration();
            silo_engine::github::retry_configuration(
                computer,
                ticket,
                silo_engine::updates::operation_guard,
            )
        }
        "github.personalToken.save" => {
            let typed: github::GitHubPersonalTokenSaveParams = typed(params)?;
            let _update = silo_engine::updates::operation_guard()?;
            silo_engine::github::personal_token::save_github_personal_token(typed.token)
        }
        "github.personalToken.remove" => {
            let _update = silo_engine::updates::operation_guard()?;
            silo_engine::github::personal_token::remove_github_personal_token()
        }
        _ => Err("Unsupported GitHub operation.".to_owned()),
    }
}

// ---------------------------------------------------------------------------
// GitHub sign-in: the owner's job, and the controller's callback route
// ---------------------------------------------------------------------------

/// The connect job this engine is running as an owner, if any. It is the only job whose
/// loopback listener a `github.connect.callback` stream may reach.
static ACTIVE_CONNECT: Mutex<Option<JobId>> = Mutex::new(None);

fn active_connect() -> std::sync::MutexGuard<'static, Option<JobId>> {
    ACTIVE_CONNECT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// How long the owner waits for its browser link before it reports the job without one.
const LINK_READY_TIMEOUT: Duration = Duration::from_secs(20);

async fn github_connect(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<github::GitHubConnectParams>(params)?;
    let engine = context.silod_context().clone();
    let job_id = match remote_target(context, typed.device_id.as_deref()) {
        Some(device) => tokio::task::spawn_blocking(move || start_remote_connect(&engine, &device))
            .await
            .map_err(|_| DispatchError::internal("The remote operation could not finish."))??,
        None => start_local_connect(&engine)?,
    };
    to_output(JobStartResult { job_id })
}

/// Starts this engine's own GitHub sign-in as a durable job. The client opens
/// `pendingAuthorizationUrl`; the engine never launches a browser.
fn start_local_connect(ctx: &SilodContext) -> Result<JobId, DispatchError> {
    let accepted = silo_engine::jobs::allocate(ctx, "github.connect")
        .map_err(HandlerContext::map_registry_error)?;
    let job_id = accepted.job_id;
    let registry = ctx.job_registry().map_err(DispatchError::internal)?;
    let generation = silo_engine::github::prepare_connect();
    registry.register_cancel(
        job_id.clone(),
        silo_engine::jobs::CancelRoute::Github,
        || silo_engine::github::cancel_connection().map(|_| ()),
    );
    *active_connect() = Some(job_id.clone());
    let engine = ctx.clone();
    let running = job_id.clone();
    let spawned = thread::Builder::new()
        .name("silod-github-connect".to_owned())
        .spawn(move || {
            let result = silo_engine::github::connect(generation, |_| Ok(()));
            finish_connect(&engine, running.clone(), result);
            let mut active = active_connect();
            if active.as_ref() == Some(&running) {
                *active = None;
            }
        });
    if let Err(error) = spawned {
        *active_connect() = None;
        let _ = silo_engine::jobs::finish_with_error(
            ctx,
            job_id,
            ErrorCode::Internal,
            "Could not start the GitHub connection.",
            Audience::All,
        );
        return Err(DispatchError::internal(format!(
            "Could not start GitHub connect: {error}"
        )));
    }
    Ok(job_id)
}

fn finish_connect(ctx: &SilodContext, job_id: JobId, result: Result<Value, String>) {
    let paths = local_paths();
    let project = |mut state: Value| -> Option<Value> {
        if let Ok(paths) = &paths {
            project_github_ids(&mut state, paths);
        }
        canonical::<github::GitHubState>(state, "GitHub state").ok()
    };
    match result {
        Ok(state) => match project(state) {
            Some(state) => {
                publish(ctx, "github.changed", state.clone());
                let _ = silo_engine::jobs::finish_with_result(ctx, job_id, state, Audience::All);
            }
            None => {
                let _ = silo_engine::jobs::finish_with_error(
                    ctx,
                    job_id,
                    ErrorCode::Internal,
                    "GitHub returned a state this engine could not read.",
                    Audience::All,
                );
            }
        },
        Err(message) => {
            if let Some(state) = silo_engine::github::snapshot().ok().and_then(project) {
                publish(ctx, "github.changed", state);
            }
            let error = failure(message);
            let _ = silo_engine::jobs::finish_with_error(
                ctx,
                job_id,
                error.code,
                error.message,
                Audience::All,
            );
        }
    }
}

/// Owner side of a controller's job follow-up: reads, cancels or acknowledges a job in
/// this engine's own registry. Jobs keep the owner's `jobId`; a job this engine does not
/// know reads as `null`.
fn owner_job_operation(ctx: &SilodContext, method: &str, params: &Value) -> Result<Value, String> {
    let job_id = params["jobId"]
        .as_str()
        .ok_or_else(|| "Missing job identity.".to_owned())
        .and_then(|id| JobId::new(id.to_owned()).map_err(|_| "Invalid job identity.".to_owned()))?;
    match method {
        "operations.job.get" => {
            let offset = params.get("offsetBytes").and_then(Value::as_u64);
            match silo_engine::jobs::get_at(ctx, &job_id, offset)
                .map_err(|error| error.to_string())?
            {
                Some(found) => serde_json::to_value(found).map_err(|error| error.to_string()),
                None => Ok(Value::Null),
            }
        }
        "operations.cancel" => {
            ctx.job_registry()?
                .cancel(&job_id)
                .map_err(|error| error.to_string())?;
            Ok(Value::Null)
        }
        _ => {
            ctx.job_registry()?
                .acknowledge(&job_id)
                .map_err(|error| error.to_string())?;
            ctx.job_queue_changed();
            Ok(Value::Null)
        }
    }
}

/// Owner side of a controller's `github.connect`: starts the job and, once its browser
/// link exists, answers with the job and the link so the controller can reserve the
/// callback port before it shows the link to anyone.
fn start_owner_connect(ctx: &SilodContext) -> Result<Value, String> {
    let job_id = start_local_connect(ctx).map_err(|error| error.message)?;
    let deadline = Instant::now() + LINK_READY_TIMEOUT;
    let url = loop {
        if let Ok(url) = silo_engine::github::authorization_url() {
            break Some(url);
        }
        let finished = silo_engine::jobs::get_at(ctx, &job_id, None)
            .ok()
            .flatten()
            .is_some_and(|job| job.state == JobState::Finished);
        if finished || Instant::now() >= deadline {
            break None;
        }
        thread::sleep(Duration::from_millis(25));
    };
    Ok(json!({ "jobId": job_id.as_str(), "url": url }))
}

/// Opens the TCP connection a `github.connect.callback` stream carries: this engine's
/// own listener for `job_id`, and only while that job is the active connect.
fn callback_listener_for(job_id: &str) -> Result<TcpStream, String> {
    let permitted = active_connect()
        .as_ref()
        .is_some_and(|active| active.as_str() == job_id);
    if !permitted {
        return Err("No GitHub connection is waiting for this callback.".to_owned());
    }
    let url = silo_engine::github::authorization_url()?;
    let port = silo_engine::github::callback_port(&url)?
        .ok_or("This GitHub step has no callback to carry.")?;
    TcpStream::connect(("127.0.0.1", port))
        .map_err(|_| "The GitHub callback listener is closed.".to_owned())
}

/// The callback this controller carries for one owner: it listens on the same loopback
/// port as the owner's listener, so the authorization link works unchanged in a browser
/// here, and hands each browser request to the owner over Connections.
struct CallbackRoute {
    port: u16,
    stop: Arc<AtomicBool>,
}

static CALLBACK_ROUTES: Mutex<BTreeMap<String, CallbackRoute>> = Mutex::new(BTreeMap::new());

fn callback_routes() -> std::sync::MutexGuard<'static, BTreeMap<String, CallbackRoute>> {
    CALLBACK_ROUTES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether a link the owner reported may be shown here: it is either a page without a
/// loopback callback, or its callback port is one this controller is carrying.
fn link_is_safe(device: &str, url: &str) -> bool {
    match silo_engine::github::callback_port(url) {
        Ok(None) => true,
        Ok(Some(port)) => callback_routes()
            .get(device)
            .is_some_and(|route| route.port == port && !route.stop.load(Ordering::Acquire)),
        Err(_) => false,
    }
}

/// Ends the callback route for `device`. Called when its flow ends and when the device is
/// removed.
fn close_callback_route(device: &str) {
    if let Some(route) = callback_routes().remove(device) {
        route.stop.store(true, Ordering::Release);
    }
}

fn close_route_if_current(device: &str, stop: &Arc<AtomicBool>) {
    let mut routes = callback_routes();
    if routes
        .get(device)
        .is_some_and(|route| Arc::ptr_eq(&route.stop, stop))
    {
        routes.remove(device);
    }
    stop.store(true, Ordering::Release);
}

const MAX_CALLBACK_RELAYS: usize = 4;
const CALLBACK_HEADER_LIMIT: usize = 8192;
const CALLBACK_REPLY_LIMIT: u64 = 65536;
const OWNER_POLL: Duration = Duration::from_secs(2);
const OWNER_POLL_FAILURES: u32 = 5;

fn cancel_owner_job(device: &str, job: &str) {
    let _ = silo_engine::remote::call_remote_typed(
        device,
        "operations.cancel",
        json!({ "jobId": job }),
    );
}

/// Controller side of `github.connect` for a device: the owner runs the sign-in as its
/// own job; this engine reserves the callback port, carries the browser's callback to
/// the owner, and mirrors the owner's outcome in a job of its own.
fn start_remote_connect(ctx: &SilodContext, device: &str) -> Result<JobId, DispatchError> {
    let reply = silo_engine::remote::call_remote_typed(device, "github.connect", json!({}))
        .map_err(HandlerContext::map_engine_error)?;
    let owner_job = reply["jobId"]
        .as_str()
        .ok_or_else(|| DispatchError::internal("The other Mac returned no connection job."))?
        .to_owned();
    let fail = |message: &str| {
        cancel_owner_job(device, &owner_job);
        DispatchError::internal(message)
    };
    let url = reply["url"]
        .as_str()
        .ok_or_else(|| fail("The other Mac did not produce a GitHub link. Try again."))?;
    let port = silo_engine::github::callback_port(url)
        .map_err(|message| fail(&message))?
        .ok_or_else(|| fail("The other Mac's GitHub link has no callback to carry."))?;
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|_| {
        fail(&format!(
            "Port {port} is in use on this Mac, so GitHub's reply cannot be carried to the other Mac. Close what uses it and connect again."
        ))
    })?;
    listener
        .set_nonblocking(true)
        .map_err(|_| fail("Could not listen for GitHub's reply on this Mac."))?;
    let stop = Arc::new(AtomicBool::new(false));
    let previous = callback_routes().insert(
        device.to_owned(),
        CallbackRoute {
            port,
            stop: stop.clone(),
        },
    );
    if let Some(previous) = previous {
        previous.stop.store(true, Ordering::Release);
    }
    let accepting = {
        let (device, owner_job, stop) = (device.to_owned(), owner_job.clone(), stop.clone());
        thread::Builder::new()
            .name("silod-github-callback".to_owned())
            .spawn(move || accept_callbacks(listener, device, owner_job, stop))
    };
    if accepting.is_err() {
        close_callback_route(device);
        return Err(fail("Could not listen for GitHub's reply on this Mac."));
    }
    let job = match silo_engine::jobs::allocate(ctx, "github.connect") {
        Ok(accepted) => accepted.job_id,
        Err(error) => {
            close_route_if_current(device, &stop);
            cancel_owner_job(device, &owner_job);
            return Err(HandlerContext::map_registry_error(error));
        }
    };
    if let Ok(registry) = ctx.job_registry() {
        let (device, owner_job) = (device.to_owned(), owner_job.clone());
        registry.register_cancel(
            job.clone(),
            silo_engine::jobs::CancelRoute::Github,
            move || {
                silo_engine::remote::call_remote_typed(
                    &device,
                    "operations.cancel",
                    json!({ "jobId": owner_job }),
                )
                .map(|_| ())
                .map_err(|error| error.message)
            },
        );
    }
    let (ctx, device, mirror) = (ctx.clone(), device.to_owned(), job.clone());
    thread::Builder::new()
        .name("silod-github-mirror".to_owned())
        .spawn(move || mirror_owner_connect(&ctx, &device, &owner_job, mirror, &stop))
        .map_err(|_| DispatchError::internal("Could not follow the other Mac's GitHub sign-in."))?;
    Ok(job)
}

fn accept_callbacks(
    listener: TcpListener,
    device: String,
    owner_job: String,
    stop: Arc<AtomicBool>,
) {
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((browser, _)) => {
                if active.fetch_add(1, Ordering::AcqRel) >= MAX_CALLBACK_RELAYS {
                    active.fetch_sub(1, Ordering::AcqRel);
                    continue;
                }
                let (device, owner_job, stop, active) = (
                    device.clone(),
                    owner_job.clone(),
                    stop.clone(),
                    active.clone(),
                );
                let spawned = thread::Builder::new()
                    .name("silod-github-relay".to_owned())
                    .spawn(move || {
                        relay_browser_callback(browser, &device, &owner_job, &stop);
                        active.fetch_sub(1, Ordering::AcqRel);
                    });
                if spawned.is_err() {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
}

fn relay_browser_callback(browser: TcpStream, device: &str, owner_job: &str, stop: &AtomicBool) {
    let cancelled = || stop.load(Ordering::Acquire);
    let Ok(opened) = silo_engine::remote::open_bridge_stream(
        device,
        "github.connect.callback",
        json!({ "jobId": owner_job }),
        &cancelled,
    ) else {
        return;
    };
    let silo_engine::remote::BridgeStream {
        child,
        input,
        output,
    } = opened;
    exchange_callback(browser, input, output);
    drop(child);
}

/// Carries one bounded HTTP callback: the browser's request header to the owner, and the
/// owner's own response back. Nothing else crosses.
fn exchange_callback(
    mut browser: TcpStream,
    mut to_owner: impl Write,
    from_owner: impl Read + Send + 'static,
) {
    let Some(request) = read_callback_header(&mut browser) else {
        return;
    };
    if to_owner
        .write_all(&request)
        .and_then(|()| to_owner.flush())
        .is_err()
    {
        return;
    }
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reply = Vec::new();
        let _ = from_owner
            .take(CALLBACK_REPLY_LIMIT)
            .read_to_end(&mut reply);
        let _ = sender.send(reply);
    });
    if let Ok(reply) = receiver.recv_timeout(Duration::from_secs(10)) {
        let _ = browser.write_all(&reply);
        let _ = browser.flush();
    }
}

/// The complete request header, within 8 KiB and two seconds, as the owner reads it.
fn read_callback_header(browser: &mut TcpStream) -> Option<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    browser.set_nonblocking(false).ok()?;
    browser
        .set_read_timeout(Some(Duration::from_millis(200)))
        .ok()?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 1024];
    while bytes.len() < CALLBACK_HEADER_LIMIT && Instant::now() < deadline {
        let room = (CALLBACK_HEADER_LIMIT - bytes.len()).min(chunk.len());
        match browser.read(&mut chunk[..room]) {
            Ok(0) => return None,
            Ok(length) => bytes.extend_from_slice(&chunk[..length]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(_) => return None,
        }
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            bytes.truncate(end + 4);
            return Some(bytes);
        }
    }
    None
}

/// Follows the owner's job until it ends, then closes the callback route and finishes
/// this engine's job with the same outcome.
fn mirror_owner_connect(
    ctx: &SilodContext,
    device: &str,
    owner_job: &str,
    job: JobId,
    stop: &Arc<AtomicBool>,
) {
    let mut failures = 0u32;
    let outcome = loop {
        thread::sleep(OWNER_POLL);
        match silo_engine::remote::call_remote_typed(
            device,
            "operations.job.get",
            json!({ "jobId": owner_job }),
        ) {
            Ok(Value::Null) => {
                break Err((
                    ErrorCode::Internal,
                    "The other Mac no longer has this sign-in. Connect again.".to_owned(),
                ));
            }
            Ok(value) => {
                failures = 0;
                match serde_json::from_value::<silo_api::jobs::JobLookupResult>(value) {
                    Ok(found) if found.state == JobState::Finished => {
                        break found.outcome.ok_or_else(|| {
                            (
                                ErrorCode::Internal,
                                "The other Mac finished without a result.".to_owned(),
                            )
                        });
                    }
                    Ok(_) => {}
                    Err(_) => {
                        break Err((
                            ErrorCode::Internal,
                            "The other Mac sent an unreadable job.".to_owned(),
                        ))
                    }
                }
            }
            Err(error) => {
                failures += 1;
                if failures >= OWNER_POLL_FAILURES {
                    break Err((
                        ErrorCode::Internal,
                        format!(
                            "Lost contact with the other Mac during GitHub sign-in: {}",
                            error.message
                        ),
                    ));
                }
            }
        }
    };
    close_route_if_current(device, stop);
    let acknowledge = outcome.is_ok();
    match outcome {
        Ok(outcome) => match (outcome.result, outcome.error) {
            (_, Some(error)) => {
                let _ = silo_engine::jobs::finish_with_error(
                    ctx,
                    job,
                    error.code,
                    error.message,
                    Audience::All,
                );
            }
            (Some(result), None) => {
                let _ = silo_engine::jobs::finish_with_result(ctx, job, result, Audience::All);
            }
            (None, None) => {
                let _ = silo_engine::jobs::finish_with_error(
                    ctx,
                    job,
                    ErrorCode::Internal,
                    "The other Mac finished without a result.",
                    Audience::All,
                );
            }
        },
        Err((code, message)) => {
            let _ = silo_engine::jobs::finish_with_error(ctx, job, code, message, Audience::All);
        }
    }
    if acknowledge {
        let _ = silo_engine::remote::call_remote_typed(
            device,
            "operations.job.acknowledge",
            json!({ "jobId": owner_job }),
        );
    }
}

// ---------------------------------------------------------------------------
// Host Push
// ---------------------------------------------------------------------------

async fn push_repositories(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<push::PushRepositoriesParams>(params)?;
    let barrier = read_barrier(context, "push");
    let refresh = typed.refresh.unwrap_or(false);
    let value = match remote_target(context, typed.device_id.as_deref()) {
        Some(device) => {
            let computer_id = typed.computer_id;
            let snapshot = silo_engine::remote::device_snapshot(device, Some(refresh))
                .await
                .map_err(HandlerContext::map_engine_error)?;
            snapshot["computers"]
                .as_array()
                .and_then(|rows| {
                    rows.iter().find(|row| {
                        row.pointer("/configuration/id").and_then(Value::as_str)
                            == Some(&computer_id)
                    })
                })
                .and_then(|row| row.get("repositories"))
                .cloned()
                .unwrap_or_else(|| json!([]))
        }
        None => {
            let computer_id = typed.computer_id;
            blocking(move || {
                let paths = local_paths()?;
                silo_engine::host_push::discover_repositories(&paths, &computer_id, refresh)
                    .map(Value::from)
            })
            .await?
        }
    };
    let rows = canonical::<Vec<push::PushRepository>>(value, "repository list").map_err(failure)?;
    Ok(HandlerOutput {
        value: rows,
        event_barrier: barrier,
    })
}

async fn push_start(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<push::PushStartParams>(params)?;
    uuid::Uuid::parse_str(&typed.operation_id)
        .map_err(|_| DispatchError::internal("Invalid push operation identity."))?;
    let job_id = JobId::new(format!("push:{}", typed.operation_id)).map_err(serialization)?;
    if let Some(device) = remote_target(context, typed.device_id.as_deref()) {
        // The owner runs and records the push; its job keeps the owner's identity.
        let request = json!({
            "computerId": typed.computer_id,
            "path": typed.repository_path,
            "operationId": typed.operation_id,
            "target": typed.target,
        });
        blocking_bridge(move || {
            silo_engine::remote::call_remote_typed(&device, "repository.push.start", request)
        })
        .await?;
        return to_output(JobStartResult { job_id });
    }
    let target = serde_json::to_value(&typed.target).map_err(serialization)?;
    blocking(move || {
        let paths = local_paths()?;
        let name = computer_name(&paths, &typed.computer_id)?;
        let target = serde_json::from_value(target).map_err(|error| error.to_string())?;
        // The operation's state reaches clients as job progress and `push.changed`.
        Ok(silo_engine::host_push_operations::start_result(
            &paths,
            name,
            typed.repository_path,
            typed.operation_id,
            target,
        ))
    })
    .await?;
    to_output(JobStartResult { job_id })
}

async fn push_status(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<push::PushStatusParams>(params)?;
    let barrier = read_barrier(context, "push");
    let value = match remote_target(context, typed.device_id.as_deref()) {
        Some(device) => {
            let request = json!({
                "computerId": typed.computer_id,
                "path": typed.repository_path,
                "operationId": typed.operation_id,
            });
            blocking_bridge(move || {
                silo_engine::remote::call_remote_typed(&device, "repository.push.status", request)
            })
            .await?
        }
        None => {
            blocking(move || {
                let paths = local_paths()?;
                let name = computer_name(&paths, &typed.computer_id)?;
                silo_engine::host_push_operations::status(
                    &paths,
                    &name,
                    &typed.repository_path,
                    &typed.operation_id,
                )
            })
            .await?
        }
    };
    let value = canonical::<Option<push::PushOperation>>(value, "push status").map_err(failure)?;
    Ok(HandlerOutput {
        value,
        event_barrier: barrier,
    })
}

async fn push_dismiss(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<push::PushDismissParams>(params)?;
    match remote_target(context, typed.device_id.as_deref()) {
        Some(device) => {
            let request = json!({ "computerId": typed.computer_id, "path": typed.repository_path });
            blocking_bridge(move || {
                silo_engine::remote::call_remote_typed(&device, "repository.dismiss", request)
            })
            .await?;
        }
        None => {
            blocking(move || {
                let paths = local_paths()?;
                let name = computer_name(&paths, &typed.computer_id)?;
                silo_engine::host_push::dismiss_repository_push(
                    &paths,
                    &name,
                    &typed.repository_path,
                )
            })
            .await?;
        }
    }
    Ok(output(Value::Null))
}

// ---------------------------------------------------------------------------
// Network and SSH access
// ---------------------------------------------------------------------------

fn scheme_name(scheme: Option<network::NetworkScheme>) -> Option<String> {
    scheme.map(|scheme| match scheme {
        network::NetworkScheme::Http => "http".to_owned(),
        network::NetworkScheme::Https => "https".to_owned(),
    })
}

async fn network_state(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<network::NetworkStateParams>(params)?;
    let barrier = read_barrier(context, "network");
    let value = match remote_target(context, typed.device_id.as_deref()) {
        Some(device) => {
            let mut value =
                blocking_bridge(move || silo_engine::remote_network::remote_network_state(&device))
                    .await?;
            restore_owner_names(&mut value);
            value
        }
        None => blocking(local_network_state).await?,
    };
    let value = canonical::<network::NetworkState>(value, "network state").map_err(failure)?;
    Ok(HandlerOutput {
        value,
        event_barrier: barrier,
    })
}

fn local_network_state() -> Result<Value, String> {
    let paths = local_paths()?;
    let state = silo_engine::network::read_network_state(&paths)?;
    let mut value = serde_json::to_value(state).map_err(|error| error.to_string())?;
    project_name_rows_to_ids(&mut value, &paths, &["computers"]);
    Ok(value)
}

async fn network_change(
    params: &Value,
    context: &HandlerContext,
    publishing: bool,
) -> Result<HandlerOutput, DispatchError> {
    let (computer_id, device_id, port, host_port, scheme) = if publishing {
        let typed = context.decode_params::<network::NetworkPublishParams>(params)?;
        (
            typed.computer_id,
            typed.device_id,
            typed.port,
            typed.host_port,
            typed.scheme,
        )
    } else {
        let typed = context.decode_params::<network::NetworkUnpublishParams>(params)?;
        (typed.computer_id, typed.device_id, typed.port, None, None)
    };
    let engine = context.silod_context().clone();
    let state = match remote_target(context, device_id.as_deref()) {
        Some(device) => {
            let mut state = blocking_bridge(move || {
                if publishing {
                    silo_engine::remote_network::remote_save_network_port(
                        device,
                        computer_id,
                        port,
                        host_port,
                        scheme_name(scheme),
                    )
                } else {
                    silo_engine::remote_network::remote_remove_network_port(
                        device,
                        computer_id,
                        port,
                    )
                }
            })
            .await?;
            restore_owner_names(&mut state);
            state
        }
        None => {
            blocking(move || {
                let paths = local_paths()?;
                let name = computer_name(&paths, &computer_id)?;
                let state = if publishing {
                    silo_engine::network::save_network_port(
                        &paths,
                        name,
                        port,
                        host_port,
                        scheme_name(scheme),
                    )?
                } else {
                    silo_engine::network::remove_network_port(&paths, name, port)?
                };
                let mut value = serde_json::to_value(state).map_err(|error| error.to_string())?;
                project_name_rows_to_ids(&mut value, &paths, &["computers"]);
                Ok(value)
            })
            .await?
        }
    };
    let state = canonical::<network::NetworkState>(state, "network state").map_err(failure)?;
    publish(&engine, "network.changed", state.clone());
    Ok(output(state))
}

async fn network_open(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<network::NetworkOpenParams>(params)?;
    let url = match remote_target(context, typed.device_id.as_deref()) {
        Some(device) => {
            blocking(move || {
                silo_engine::remote_network::remote_open_network_port(
                    device,
                    typed.computer_id,
                    typed.port,
                )
            })
            .await?
        }
        None => {
            blocking(move || {
                let paths = local_paths()?;
                let name = computer_name(&paths, &typed.computer_id)?;
                silo_engine::network::network_port_url(&paths, name, typed.port)
            })
            .await?
        }
    };
    to_output(network::NetworkOpenResult { url })
}

async fn ssh_access_state(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<ssh_access::SshAccessStateParams>(params)?;
    let barrier = read_barrier(context, "network");
    let value = match remote_target(context, typed.device_id.as_deref()) {
        Some(device) => {
            let mut value = blocking_bridge(move || {
                silo_engine::remote_ssh_access::remote_ssh_access_state(&device)
            })
            .await?;
            restore_owner_names(&mut value);
            value
        }
        None => blocking(local_ssh_access_state).await?,
    };
    let value =
        canonical::<ssh_access::SshAccessState>(value, "SSH access state").map_err(failure)?;
    Ok(HandlerOutput {
        value,
        event_barrier: barrier,
    })
}

fn local_ssh_access_state() -> Result<Value, String> {
    let paths = local_paths()?;
    let state = silo_engine::ssh_access::read_state(&paths)?;
    let mut value = serde_json::to_value(state).map_err(|error| error.to_string())?;
    project_name_rows_to_ids(&mut value, &paths, &["computers"]);
    Ok(value)
}

async fn ssh_access_save(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<ssh_access::SshAccessSaveParams>(params)?;
    let engine = context.silod_context().clone();
    let value = match remote_target(context, typed.device_id.as_deref()) {
        Some(device) => {
            let mut value = blocking_bridge(move || {
                silo_engine::remote_ssh_access::remote_save_ssh_access(
                    &device,
                    typed.computer_id,
                    typed.enabled,
                    typed.port,
                    typed.bind_address,
                    typed.keys,
                )
            })
            .await?;
            restore_owner_names(&mut value);
            value
        }
        None => {
            blocking(move || {
                let paths = local_paths()?;
                let name = computer_name(&paths, &typed.computer_id)?;
                let state = silo_engine::ssh_access::save(
                    &paths,
                    name,
                    typed.enabled,
                    typed.port,
                    typed.bind_address,
                    typed.keys,
                )?;
                let mut value = serde_json::to_value(state).map_err(|error| error.to_string())?;
                project_name_rows_to_ids(&mut value, &paths, &["computers"]);
                Ok(value)
            })
            .await?
        }
    };
    let value =
        canonical::<ssh_access::SshAccessState>(value, "SSH access state").map_err(failure)?;
    // `network.changed` carries the published-port state; a listener change is its hint
    // to re-read `sshAccess.state` too.
    if let Ok(network) = blocking(local_network_state).await {
        if let Ok(network) = canonical::<network::NetworkState>(network, "network state") {
            publish(&engine, "network.changed", network);
        }
    }
    Ok(output(value))
}

async fn ssh_access_connection(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<ssh_access::SshAccessConnectionParams>(params)?;
    let device = remote_target(context, typed.device_id.as_deref());
    let command = blocking(move || {
        let paths = local_paths()?;
        let computer = match device {
            Some(_) => None,
            None => Some(computer_name(&paths, &typed.computer_id)?),
        };
        let prepared = silo_engine::ssh_connection::prepare(
            &paths,
            computer,
            device,
            Some(typed.computer_id),
            typed.download,
            typed.network,
        )?;
        match prepared {
            silo_engine::ssh_connection::ConnectionResult::Command(command) => Ok(Some(command)),
            silo_engine::ssh_connection::ConnectionResult::KeyExport { source, .. } => {
                let destination = typed
                    .destination
                    .ok_or("Choose a destination for the private key.")?;
                let destination = PathBuf::from(destination);
                if !destination.is_absolute() {
                    return Err("Choose an absolute destination path.".to_owned());
                }
                let private = silo_engine::editor::read_regular(&source)?;
                let parent = destination.parent().ok_or("Choose a destination file.")?;
                std::fs::create_dir_all(parent)
                    .map_err(|_| "Could not write the SSH key destination.".to_owned())?;
                silo_engine::editor::write_private(&destination, &private)?;
                Ok(None)
            }
        }
    })
    .await?;
    to_output(command)
}

// ---------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------

async fn connections_status(context: &HandlerContext) -> Result<HandlerOutput, DispatchError> {
    let barrier = read_barrier(context, "connections");
    let status = silo_engine::remote::connections_status()
        .await
        .map_err(failure)?;
    let value = canonical::<connections::ConnectionsStatus>(
        serde_json::to_value(status).map_err(serialization)?,
        "Connections status",
    )
    .map_err(failure)?;
    Ok(HandlerOutput {
        value,
        event_barrier: barrier,
    })
}

async fn connections_set_enabled(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<connections::ConnectionsSetEnabledParams>(params)?;
    let status = silo_engine::remote::set_connections_enabled(typed.enabled)
        .await
        .map_err(failure)?;
    let value = canonical::<connections::ConnectionsStatus>(
        serde_json::to_value(status).map_err(serialization)?,
        "Connections status",
    )
    .map_err(failure)?;
    Ok(output(value))
}

async fn connections_devices_list(
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let barrier = read_barrier(context, "connections");
    let devices = silo_engine::remote::device_list().await.map_err(failure)?;
    let value = canonical::<Vec<connections::Device>>(
        serde_json::to_value(devices).map_err(serialization)?,
        "device list",
    )
    .map_err(failure)?;
    Ok(HandlerOutput {
        value,
        event_barrier: barrier,
    })
}

async fn connections_devices_connect(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<connections::ConnectDeviceParams>(params)?;
    let device = silo_engine::remote::connect_device(typed.address, typed.replace)
        .await
        .map_err(failure)?;
    silo_engine::remote::publish_device_health();
    to_output(device)
}

/// Removes a connected Mac: the owner first drops this Mac's management key (95 A-36),
/// then this engine forgets the device and closes everything it holds for it. An owner
/// that cannot be reached, or a key that cannot be removed, leaves the device saved so
/// the removal can be retried; the key is never reinstalled to make a retry succeed.
async fn connections_devices_remove(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<connections::DeviceIdParams>(params)?;
    let device = typed.device_id;
    let key = silo_engine::remote::connections_public_key().map_err(failure)?;
    if let Some(public_key) = key {
        let owner = device.clone();
        blocking_bridge(move || {
            silo_engine::remote::call_remote_typed(
                &owner,
                "connections.keys.revoke",
                json!({ "publicKey": public_key }),
            )
        })
        .await?;
    }
    close_callback_route(&device);
    silo_engine::remote::remove_device(device)
        .await
        .map_err(failure)?;
    Ok(output(Value::Null))
}

async fn connections_keys_revoke(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<connections::RevokeKeyParams>(params)?;
    match remote_target(context, typed.device_id.as_deref()) {
        Some(device) => {
            let public_key = typed.public_key;
            blocking_bridge(move || {
                silo_engine::remote::call_remote_typed(
                    &device,
                    "connections.keys.revoke",
                    json!({ "publicKey": public_key }),
                )
            })
            .await?;
        }
        None => {
            blocking(move || silo_engine::remote::revoke_connections_key(&typed.public_key))
                .await?;
        }
    }
    Ok(output(Value::Null))
}

/// The recorded health of each saved device. It reads memory only; it opens no SSH
/// connection and probes nothing.
async fn connections_devices_health(
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let barrier = read_barrier(context, "connections");
    let health = silo_engine::remote::device_health().map_err(failure)?;
    Ok(HandlerOutput {
        value: serde_json::to_value(health).map_err(serialization)?,
        event_barrier: barrier,
    })
}

async fn connections_ssh_command(
    params: &Value,
    context: &HandlerContext,
    setup: bool,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<connections::SshAddressParams>(params)?;
    let command = blocking(move || {
        if setup {
            let (address, command) = silo_engine::remote::prepare_device_key_setup(&typed.address)?;
            silo_engine::remote::remember_device_key_setup(&address);
            Ok(command)
        } else {
            silo_engine::remote::authorization_command(&typed.address)
        }
    })
    .await?;
    to_output(connections::SshCommandResult { command })
}

#[cfg(test)]
mod tests {
    use std::{
        os::unix::net::UnixStream,
        sync::OnceLock,
        time::{Duration, Instant},
    };

    use silo_engine::context::{
        EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl,
    };

    use super::*;
    use crate::{dispatcher::RequestContext, identity::LocalDeviceIdentity};

    const LOCAL_DEVICE: &str = "11111111-1111-4111-8111-111111111111";
    const COMPUTER: &str = "22222222-2222-4222-8222-222222222222";
    const MISSING_COMPUTER: &str = "33333333-3333-4333-8333-333333333333";
    const UNKNOWN_DEVICE: &str = "44444444-4444-4444-8444-444444444444";
    const NOT_CONNECTED: &str = "This device is no longer connected.";

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
            panic!("restart is not expected in an access test")
        }
    }

    struct Fixture {
        _root: tempfile::TempDir,
        context: Arc<SilodContext>,
        home: PathBuf,
    }

    /// One engine context for the whole test binary: the engine's GitHub, secrets and
    /// Connections state are process-wide, so every test shares this temporary home.
    fn fixture() -> &'static Fixture {
        static FIXTURE: OnceLock<Fixture> = OnceLock::new();
        FIXTURE.get_or_init(|| {
            let root = tempfile::tempdir().unwrap();
            let base = root.path().to_path_buf();
            let paths = EnginePaths {
                app_data: base.join("data"),
                app_config: base.join("config"),
                app_cache: base.join("cache"),
                app_log: base.join("log"),
                resources: base.join("resources"),
                home: base.join("home"),
            };
            for directory in [
                &paths.app_data,
                &paths.app_config,
                &paths.app_cache,
                &paths.app_log,
                &paths.resources,
                &paths.home,
            ] {
                std::fs::create_dir_all(directory).unwrap();
            }
            let home = paths.home.clone();
            let context = Arc::new(SilodContext::new(
                paths,
                EngineIdentity {
                    channel: silo_engine::channel::Channel::Development,
                    version: "0.1.0-test".to_owned(),
                    package_kind: PackageKind::MacApp,
                },
                LocalDeviceIdentity {
                    device_id: LOCAL_DEVICE.to_owned(),
                    name: "Test Mac".to_owned(),
                },
                Arc::new(NoopMainThread),
                Arc::new(NoopProcess),
            ));
            silo_engine::context::install(context.clone()).unwrap();
            // Not `install`: its GitHub worker would read the real credential store.
            install_host(context.clone()).unwrap();
            silo_engine::secrets::install().unwrap();
            let storage = runtime_host().unwrap().runtime_storage().unwrap();
            std::fs::create_dir_all(&storage).unwrap();
            std::fs::write(
                storage.join("computers.json"),
                json!({
                    "schemaVersion": 1,
                    "computers": [{
                        "id": COMPUTER,
                        "name": "dev",
                        "cpus": 2,
                        "maxCPUs": 4,
                        "memoryGiB": 4,
                        "maxMemoryGiB": 8,
                        "workspaceStorageGiB": 20,
                        "runtimeStorageGiB": 20
                    }]
                })
                .to_string(),
            )
            .unwrap();
            Fixture {
                _root: root,
                context,
                home,
            }
        })
    }

    fn request_context() -> HandlerContext {
        HandlerContext::new(
            fixture().context.clone(),
            RequestContext {
                client_id: "access-test-client".to_owned(),
                protocol: 6,
                client: None,
            },
        )
    }

    async fn call(method: MethodName, params: Value) -> Result<Value, DispatchError> {
        handle(method, &params, &request_context())
            .await
            .expect("access owns this method")
            .map(|output| output.value)
    }

    async fn expect_error(method: MethodName, params: Value) -> DispatchError {
        call(method, params)
            .await
            .expect_err("the request should be refused")
    }

    async fn expect_routed_to_unknown_device(method: MethodName, mut params: Value) {
        params["deviceId"] = json!(UNKNOWN_DEVICE);
        let error = expect_error(method, params).await;
        assert!(
            error.message.contains(NOT_CONNECTED),
            "{} should route to the device: {}",
            method_name(method),
            error.message
        );
    }

    async fn expect_invalid_params(method: MethodName, params: Value) {
        let error = expect_error(method, params).await;
        assert!(
            error.message.starts_with("Invalid params"),
            "{}: {}",
            method_name(method),
            error.message
        );
        assert_eq!(error.code, ErrorCode::Internal);
    }

    /// Tests that create or remove this engine's Connections key must not overlap.
    static KEY_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn connections_config_path() -> PathBuf {
        silo_engine::channel::current()
            .state_dir(&fixture().home)
            .join("desktop-remote/config.json")
    }

    /// Saves a device in this engine's Connections settings without contacting it.
    async fn save_device(id: &str, address: &str) {
        call(MethodName::ConnectionsStatus, json!({}))
            .await
            .unwrap();
        let path = connections_config_path();
        let mut config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        config["devices"]
            .as_array_mut()
            .unwrap()
            .retain(|device| device["id"] != id);
        config["devices"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": id, "name": "Remote Mac", "address": address}));
        std::fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
    }

    fn saved_device_ids() -> Vec<String> {
        let config: Value =
            serde_json::from_slice(&std::fs::read(connections_config_path()).unwrap()).unwrap();
        config["devices"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|device| device["id"].as_str().map(str::to_owned))
            .collect()
    }

    #[test]
    fn ownership_covers_exactly_the_access_prefixes() {
        for method in [
            MethodName::GitHubState,
            MethodName::SecretsList,
            MethodName::PushStart,
            MethodName::NetworkState,
            MethodName::SshAccessSave,
            MethodName::ConnectionsDevicesHealth,
        ] {
            assert!(owns(method), "{}", method_name(method));
        }
        assert!(!owns(MethodName::ComputersList));
        assert!(!owns(MethodName::OperationsJobGet));
    }

    #[test]
    fn failures_map_to_the_canonical_codes() {
        assert_eq!(
            failure("Silo is installing an update. Try again after it restarts.").code,
            ErrorCode::UpdateInProgress
        );
        assert_eq!(
            failure("GitHub connection cancelled.").code,
            ErrorCode::Cancelled
        );
        assert_eq!(failure("Anything else").code, ErrorCode::Internal);
        assert_eq!(
            without_device(&json!({"deviceId": "d", "id": "s"})),
            json!({"id": "s"})
        );
    }

    #[tokio::test]
    async fn own_device_id_is_local_and_other_ids_are_remote() {
        let context = request_context();
        assert_eq!(remote_target(&context, None), None);
        assert_eq!(remote_target(&context, Some(LOCAL_DEVICE)), None);
        assert_eq!(
            remote_target(&context, Some(UNKNOWN_DEVICE)).as_deref(),
            Some(UNKNOWN_DEVICE)
        );
    }

    #[tokio::test]
    async fn projections_replace_computer_names_with_identities() {
        fixture();
        let paths = local_paths().unwrap();
        assert_eq!(computer_name(&paths, COMPUTER).unwrap(), "dev");
        assert_eq!(
            computer_name(&paths, MISSING_COMPUTER).unwrap_err(),
            "This computer no longer exists."
        );
        assert!(computer_name(&paths, "dev").is_err());

        let mut state = json!({
            "computers": [{"computer": "dev"}, {"computer": "gone"}],
            "computerOperations": [{"computer": "dev", "status": "succeeded"}],
        });
        project_github_ids(&mut state, &paths);
        assert_eq!(state["computers"], json!([{"computerId": COMPUTER}]));
        assert_eq!(
            state["computerOperations"],
            json!([{"computerId": COMPUTER, "status": "succeeded"}])
        );

        let mut rows = vec![json!({"computers": ["dev", "gone"], "pendingComputers": []})];
        project_secret_ids(&mut rows, &paths).unwrap();
        assert_eq!(
            rows[0],
            json!({"computerIds": [COMPUTER], "pendingComputerIds": []})
        );

        let mut network = json!({"computers": [{"computer": "dev", "ports": []}]});
        project_name_rows_to_ids(&mut network, &paths, &["computers"]);
        assert_eq!(network["computers"][0]["computerId"], COMPUTER);

        let mut owner = json!({"computers": [{
            "computer": "silo-remote:device:id",
            "computerName": "their-name",
            "computerId": COMPUTER,
        }]});
        restore_owner_names(&mut owner);
        assert_eq!(
            owner["computers"][0],
            json!({"computer": "their-name", "computerId": COMPUTER})
        );
    }

    #[tokio::test]
    async fn github_state_is_canonical_and_never_exposes_a_link_without_a_flow() {
        let state = call(MethodName::GitHubState, json!({})).await.unwrap();
        let typed: github::GitHubState = serde_json::from_value(state.clone()).unwrap();
        assert_eq!(typed.pending_authorization_url, None);
        assert_eq!(typed.state, github::GitHubConnectionState::Disconnected);
        assert!(state.get("computers").is_some_and(Value::is_array));
        // Naming this engine's own device is the same as naming none.
        let own = call(MethodName::GitHubState, json!({"deviceId": LOCAL_DEVICE}))
            .await
            .unwrap();
        assert_eq!(own, state);
        expect_invalid_params(MethodName::GitHubState, json!({"deviceId": 7})).await;
        expect_routed_to_unknown_device(MethodName::GitHubState, json!({})).await;
    }

    #[tokio::test]
    async fn github_connect_runs_as_a_job_whose_callback_only_the_active_job_reaches() {
        expect_invalid_params(MethodName::GitHubConnect, json!({"deviceId": []})).await;
        expect_routed_to_unknown_device(MethodName::GitHubConnect, json!({})).await;
        assert!(callback_listener_for("job:999999").is_err());

        let started = call(MethodName::GitHubConnect, json!({})).await.unwrap();
        let job_id = started["jobId"].as_str().unwrap().to_owned();
        assert!(job_id.starts_with("job:"));
        let engine = fixture().context.clone();
        let parsed = JobId::new(job_id.clone()).unwrap();

        // The flow either reaches its browser step or finishes at once because this
        // build has no GitHub App configured. Neither contacts GitHub.
        let deadline = Instant::now() + Duration::from_secs(10);
        let url = loop {
            if let Ok(url) = silo_engine::github::authorization_url() {
                break Some(url);
            }
            let finished = silo_engine::jobs::get_at(engine.as_ref(), &parsed, None)
                .unwrap()
                .is_some_and(|job| job.state == JobState::Finished);
            if finished || Instant::now() > deadline {
                break None;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        match url {
            Some(url) => {
                let port = silo_engine::github::callback_port(&url).unwrap().unwrap();
                let state = call(MethodName::GitHubState, json!({})).await.unwrap();
                assert_eq!(state["pendingAuthorizationUrl"], url.as_str());
                let reopened = call(MethodName::GitHubConnectReopen, json!({}))
                    .await
                    .unwrap();
                assert_eq!(reopened["url"], url.as_str());
                assert!(callback_listener_for("job:999999").is_err());
                let mut callback = callback_listener_for(&job_id).unwrap();
                assert_eq!(callback.peer_addr().unwrap().port(), port);
                callback
                    .write_all(b"GET /github/callback?state=wrong&code=x HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                    .unwrap();
                let mut reply = String::new();
                callback.read_to_string(&mut reply).unwrap();
                assert!(reply.starts_with("HTTP/1.1 400"), "{reply}");
                engine.job_registry().unwrap().cancel(&parsed).unwrap();
            }
            None => {}
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        let outcome = loop {
            let found = silo_engine::jobs::get_at(engine.as_ref(), &parsed, None)
                .unwrap()
                .unwrap();
            if found.state == JobState::Finished {
                break found.outcome.unwrap();
            }
            assert!(Instant::now() < deadline, "the connect job never finished");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let error = outcome.error.expect("no one authorized this flow");
        assert!(
            error.code == ErrorCode::Cancelled || error.code == ErrorCode::Internal,
            "{error:?}"
        );
        assert!(callback_listener_for(&job_id).is_err());
        engine.job_registry().unwrap().acknowledge(&parsed).unwrap();
    }

    #[tokio::test]
    async fn github_connect_callback_is_served_between_engines_only() {
        let error =
            expect_error(MethodName::GitHubConnectCallback, json!({"jobId": "job:1"})).await;
        assert_eq!(error.code, ErrorCode::UnsupportedRemoteOperation);
        assert!(error.message.contains("between engines"));
    }

    #[tokio::test]
    async fn github_links_come_from_the_owner() {
        let error = expect_error(MethodName::GitHubConnectReopen, json!({})).await;
        assert!(error
            .message
            .contains("No browser authorization is waiting"));
        expect_routed_to_unknown_device(MethodName::GitHubConnectReopen, json!({})).await;

        match call(MethodName::GitHubRepositoriesManage, json!({})).await {
            Ok(link) => assert!(link["url"]
                .as_str()
                .unwrap()
                .starts_with("https://github.com/apps/")),
            Err(error) => assert!(error.message.contains("GitHub App is not configured")),
        }
        expect_routed_to_unknown_device(MethodName::GitHubRepositoriesManage, json!({})).await;
    }

    #[tokio::test]
    async fn github_mutations_check_params_and_route_to_the_owner() {
        for method in [
            MethodName::GitHubRepositoriesRefresh,
            MethodName::GitHubDisconnect,
            MethodName::GitHubPersonalTokenRemove,
            MethodName::GitHubConfigurationRetry,
        ] {
            expect_invalid_params(method, json!({"deviceId": false})).await;
            expect_routed_to_unknown_device(method, json!({})).await;
        }
        expect_invalid_params(MethodName::GitHubAccessSet, json!({})).await;
        expect_routed_to_unknown_device(MethodName::GitHubAccessSet, json!({"enabled": true}))
            .await;
        expect_invalid_params(MethodName::GitHubPersonalTokenSave, json!({})).await;
        expect_routed_to_unknown_device(
            MethodName::GitHubPersonalTokenSave,
            json!({"token": "synthetic-token"}),
        )
        .await;
        expect_invalid_params(MethodName::GitHubConfigurationSave, json!({})).await;
        let configuration = json!({"configuration": {"computers": [], "baseRevision": null}});
        expect_routed_to_unknown_device(MethodName::GitHubConfigurationSave, configuration).await;
    }

    #[tokio::test]
    async fn github_configuration_names_computers_by_identity() {
        let policy = |computer_id: &str| {
            json!({
                "computerId": computer_id,
                "identity": {"name": "A", "email": "a@example.com", "apply": false},
                "repositoryMode": "selected",
                "repositories": [],
                "allRepositoriesAllowChanges": false,
                "authenticationMethod": "oauth",
            })
        };
        let error = expect_error(
            MethodName::GitHubConfigurationSave,
            json!({"configuration": {"computers": [policy(MISSING_COMPUTER)], "baseRevision": null}}),
        )
        .await;
        assert_eq!(error.message, "This computer no longer exists.");
        let error = expect_error(
            MethodName::GitHubConfigurationRetry,
            json!({"computerId": MISSING_COMPUTER}),
        )
        .await;
        assert_eq!(error.message, "This computer no longer exists.");

        let typed: github::GitHubConfiguration =
            serde_json::from_value(json!({"computers": [policy(COMPUTER)], "baseRevision": null}))
                .unwrap();
        let engine = api_configuration_to_engine(typed, &local_paths().unwrap()).unwrap();
        assert_eq!(engine["computers"][0]["computer"], "dev");
        assert!(engine["computers"][0].get("computerId").is_none());
    }

    #[tokio::test]
    async fn secrets_list_is_canonical_and_routes_by_device() {
        let rows = call(MethodName::SecretsList, json!({})).await.unwrap();
        let typed: Vec<secrets::Secret> = serde_json::from_value(rows).unwrap();
        assert!(typed.is_empty());
        expect_invalid_params(MethodName::SecretsList, json!({"deviceId": 3})).await;
        expect_routed_to_unknown_device(MethodName::SecretsList, json!({})).await;
    }

    #[tokio::test]
    async fn secrets_save_requires_an_explicit_wildcard_acknowledgement() {
        let request = |domains: Value, acknowledged: Option<bool>| {
            let mut request = json!({
                "operation": "add",
                "name": "SYNTHETIC_TOKEN",
                "value": "synthetic-value",
                "computerIds": [COMPUTER],
                "allowedDomains": domains,
            });
            if let Some(acknowledged) = acknowledged {
                request["allowAnyDestination"] = json!(acknowledged);
            }
            json!({"request": request})
        };
        for acknowledged in [None, Some(false)] {
            // Refused before anything is stored, routed, or even resolved to a computer.
            let error =
                expect_error(MethodName::SecretsSave, request(json!(["*"]), acknowledged)).await;
            assert!(
                error.message.contains("allowAnyDestination"),
                "{}",
                error.message
            );
            let mut remote = request(json!(["api.example.com", "*"]), acknowledged);
            remote["deviceId"] = json!(UNKNOWN_DEVICE);
            let error = expect_error(MethodName::SecretsSave, remote).await;
            assert!(
                error.message.contains("allowAnyDestination"),
                "{}",
                error.message
            );
        }
        // Acknowledged, or not a wildcard: the request goes on to its owner.
        let mut acknowledged = request(json!(["*"]), Some(true));
        acknowledged["deviceId"] = json!(UNKNOWN_DEVICE);
        let error = expect_error(MethodName::SecretsSave, acknowledged).await;
        assert!(error.message.contains(NOT_CONNECTED), "{}", error.message);
        let mut specific = request(json!(["api.example.com"]), None);
        specific["deviceId"] = json!(UNKNOWN_DEVICE);
        let error = expect_error(MethodName::SecretsSave, specific).await;
        assert!(error.message.contains(NOT_CONNECTED), "{}", error.message);
        expect_invalid_params(MethodName::SecretsSave, json!({})).await;
    }

    #[tokio::test]
    async fn secrets_remove_and_retry_check_params_and_route_to_the_owner() {
        for method in [MethodName::SecretsRemove, MethodName::SecretsRetry] {
            expect_invalid_params(method, json!({})).await;
            expect_routed_to_unknown_device(method, json!({"id": "secret-1"})).await;
        }
        let error = expect_error(MethodName::SecretsRetry, json!({"id": "missing"})).await;
        assert_eq!(error.code, ErrorCode::Internal);
    }

    #[tokio::test]
    async fn push_methods_resolve_computers_by_identity_and_route_by_device() {
        let operation = "55555555-5555-4555-8555-555555555555";
        let target =
            json!({"repository": "owner/repo", "branch": "main", "commit": "0".repeat(40)});
        let missing = |extra: Value| {
            let mut params =
                json!({"computerId": MISSING_COMPUTER, "repositoryPath": "/work/repo"});
            for (key, value) in extra.as_object().unwrap() {
                params[key] = value.clone();
            }
            params
        };

        let error = expect_error(MethodName::PushRepositories, missing(json!({}))).await;
        assert!(
            error.message.contains("no longer exists"),
            "{}",
            error.message
        );
        expect_routed_to_unknown_device(MethodName::PushRepositories, missing(json!({}))).await;

        let start = missing(json!({"operationId": operation, "target": target}));
        let error = expect_error(MethodName::PushStart, start.clone()).await;
        assert_eq!(error.message, "This computer no longer exists.");
        expect_routed_to_unknown_device(MethodName::PushStart, start).await;
        let error = expect_error(
            MethodName::PushStart,
            missing(json!({"operationId": "not-a-uuid", "target": target})),
        )
        .await;
        assert_eq!(error.message, "Invalid push operation identity.");

        let status = missing(json!({"operationId": operation}));
        let error = expect_error(MethodName::PushStatus, status.clone()).await;
        assert_eq!(error.message, "This computer no longer exists.");
        expect_routed_to_unknown_device(MethodName::PushStatus, status).await;

        let error = expect_error(MethodName::PushDismiss, missing(json!({}))).await;
        assert_eq!(error.message, "This computer no longer exists.");
        expect_routed_to_unknown_device(MethodName::PushDismiss, missing(json!({}))).await;
        expect_invalid_params(MethodName::PushStart, json!({})).await;
    }

    #[tokio::test]
    async fn network_methods_use_computer_identities() {
        let state = call(MethodName::NetworkState, json!({})).await.unwrap();
        let typed: network::NetworkState = serde_json::from_value(state).unwrap();
        assert!(typed.computers.iter().all(|row| row.computer_id.is_some()));
        expect_routed_to_unknown_device(MethodName::NetworkState, json!({})).await;

        for method in [MethodName::NetworkPublish, MethodName::NetworkUnpublish] {
            let params = json!({"computerId": MISSING_COMPUTER, "port": 8080});
            let error = expect_error(method, params.clone()).await;
            assert_eq!(error.message, "This computer no longer exists.");
            expect_routed_to_unknown_device(method, params).await;
            expect_invalid_params(method, json!({"port": 8080})).await;
        }
        let open = json!({"computerId": MISSING_COMPUTER, "port": 8080});
        let error = expect_error(MethodName::NetworkOpen, open.clone()).await;
        assert_eq!(error.message, "This computer no longer exists.");
        let mut remote = open;
        remote["deviceId"] = json!(UNKNOWN_DEVICE);
        let error = expect_error(MethodName::NetworkOpen, remote).await;
        assert!(error.message.contains(NOT_CONNECTED), "{}", error.message);
    }

    #[tokio::test]
    async fn ssh_access_methods_use_computer_identities() {
        let state = call(MethodName::SshAccessState, json!({})).await.unwrap();
        let typed: ssh_access::SshAccessState = serde_json::from_value(state).unwrap();
        assert!(typed
            .computers
            .iter()
            .all(|row| !row.computer_id.is_empty()));
        expect_routed_to_unknown_device(MethodName::SshAccessState, json!({})).await;

        let save = json!({
            "computerId": MISSING_COMPUTER,
            "enabled": true,
            "port": 2222,
            "bindAddress": "127.0.0.1",
        });
        let error = expect_error(MethodName::SshAccessSave, save.clone()).await;
        assert_eq!(error.message, "This computer no longer exists.");
        expect_routed_to_unknown_device(MethodName::SshAccessSave, save).await;

        let connection = json!({"computerId": MISSING_COMPUTER, "download": false});
        let error = expect_error(MethodName::SshAccessConnection, connection.clone()).await;
        assert_eq!(error.message, "This computer no longer exists.");
        let mut remote = connection;
        remote["deviceId"] = json!(UNKNOWN_DEVICE);
        let error = expect_error(MethodName::SshAccessConnection, remote).await;
        assert!(error.message.contains(NOT_CONNECTED), "{}", error.message);
        expect_invalid_params(MethodName::SshAccessConnection, json!({})).await;
    }

    #[tokio::test]
    async fn connections_status_list_and_enabled_state() {
        let status = call(MethodName::ConnectionsStatus, json!({}))
            .await
            .unwrap();
        let typed: connections::ConnectionsStatus = serde_json::from_value(status).unwrap();
        assert!(!typed.device_id.is_empty());
        let disabled = call(MethodName::ConnectionsSetEnabled, json!({"enabled": false}))
            .await
            .unwrap();
        assert_eq!(disabled["enabled"], false);
        assert_eq!(disabled["deviceId"], typed.device_id.as_str());
        expect_invalid_params(MethodName::ConnectionsSetEnabled, json!({})).await;

        let devices = call(MethodName::ConnectionsDevicesList, json!({}))
            .await
            .unwrap();
        assert!(devices.is_array());
    }

    #[tokio::test]
    async fn connections_health_is_a_side_effect_free_projection() {
        let device = "66666666-6666-4666-8666-666666666666";
        save_device(device, "ssh://user@127.0.0.1:9").await;
        let health = call(MethodName::ConnectionsDevicesHealth, json!({}))
            .await
            .unwrap();
        let typed: connections::DeviceHealthResult = serde_json::from_value(health).unwrap();
        let row = typed
            .devices
            .iter()
            .find(|row| row.device_id == device)
            .unwrap();
        assert!(!row.connected && !row.busy);
        assert_eq!(row.error, None);
        assert_eq!(row.last_seen, None);

        silo_engine::remote::poll_busy(device);
        let health = call(MethodName::ConnectionsDevicesHealth, json!({}))
            .await
            .unwrap();
        let typed: connections::DeviceHealthResult = serde_json::from_value(health).unwrap();
        let row = typed
            .devices
            .iter()
            .find(|row| row.device_id == device)
            .unwrap();
        assert!(row.busy && !row.connected);
        assert_eq!(row.error.as_deref(), Some("The other device is busy."));
    }

    #[tokio::test]
    async fn connections_ssh_commands_are_local_text() {
        let _key = KEY_LOCK.lock().await;
        let authorization = call(
            MethodName::ConnectionsSshAuthorizationCommand,
            json!({"address": "user@host.example"}),
        )
        .await
        .unwrap();
        assert!(authorization["command"]
            .as_str()
            .unwrap()
            .contains("user@host.example"));
        let setup = call(
            MethodName::ConnectionsSshKeySetupCommand,
            json!({"address": "user@host.example"}),
        )
        .await
        .unwrap();
        assert!(setup["command"]
            .as_str()
            .unwrap()
            .contains("user@host.example"));
        let error = expect_error(
            MethodName::ConnectionsSshAuthorizationCommand,
            json!({"address": "bad address; rm -rf /"}),
        )
        .await;
        assert!(
            error.message.contains("Enter an SSH alias"),
            "{}",
            error.message
        );
        expect_invalid_params(MethodName::ConnectionsSshKeySetupCommand, json!({})).await;
    }

    #[tokio::test]
    async fn connections_connect_refuses_an_invalid_address() {
        let error = expect_error(
            MethodName::ConnectionsDevicesConnect,
            json!({"address": "not an address"}),
        )
        .await;
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(
            error.message.contains("Enter an SSH alias"),
            "{}",
            error.message
        );
        expect_invalid_params(MethodName::ConnectionsDevicesConnect, json!({})).await;
    }

    #[tokio::test]
    async fn keys_revoke_leaves_unrelated_keys_alone_and_succeeds_when_absent() {
        let path = fixture().home.join(".ssh/authorized_keys");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let blob = "AAAAC3NzaC1lZDI1NTE5AAAAIFakeFakeFakeFakeFakeFakeFakeFakeFakeFakeFake";
        let public_key = format!("ssh-ed25519 {blob} Silo remote management");
        let original = format!(
            "ssh-ed25519 AAAAother user@laptop\nssh-ed25519 {blob} someone@elsewhere\n# comment\n"
        );
        std::fs::write(&path, &original).unwrap();
        for _ in 0..2 {
            let revoked = call(
                MethodName::ConnectionsKeysRevoke,
                json!({"publicKey": public_key}),
            )
            .await
            .unwrap();
            assert_eq!(revoked, Value::Null);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
        expect_routed_to_unknown_device(
            MethodName::ConnectionsKeysRevoke,
            json!({"publicKey": public_key}),
        )
        .await;
        let error = expect_error(
            MethodName::ConnectionsKeysRevoke,
            json!({"publicKey": "garbage"}),
        )
        .await;
        assert_eq!(error.message, "Invalid Silo SSH public key.");
        expect_invalid_params(MethodName::ConnectionsKeysRevoke, json!({})).await;
    }

    #[tokio::test]
    async fn an_owner_serves_its_own_jobs_to_a_controller() {
        let engine = fixture().context.clone();
        let unknown = json!({"jobId": "job:424242"});
        assert_eq!(
            owner_job_operation(engine.as_ref(), "operations.job.get", &unknown).unwrap(),
            Value::Null
        );
        assert!(owner_job_operation(engine.as_ref(), "operations.cancel", &unknown).is_err());
        assert!(owner_job_operation(engine.as_ref(), "operations.job.get", &json!({})).is_err());
        let accepted = silo_engine::jobs::allocate(engine.as_ref(), "github.connect").unwrap();
        let params = json!({"jobId": accepted.job_id.as_str()});
        let found = owner_job_operation(engine.as_ref(), "operations.job.get", &params).unwrap();
        assert_eq!(found["jobId"], accepted.job_id.as_str());
        assert_ne!(found["state"], "finished");
        silo_engine::jobs::finish_with_error(
            engine.as_ref(),
            accepted.job_id.clone(),
            ErrorCode::Cancelled,
            "GitHub connection cancelled.",
            Audience::All,
        )
        .unwrap();
        let found = owner_job_operation(engine.as_ref(), "operations.job.get", &params).unwrap();
        assert_eq!(found["state"], "finished");
        assert_eq!(found["outcome"]["error"]["code"], "cancelled");
        assert_eq!(
            owner_job_operation(engine.as_ref(), "operations.job.acknowledge", &params).unwrap(),
            Value::Null
        );
    }

    #[tokio::test]
    async fn removing_a_device_revokes_its_key_first_and_keeps_it_when_that_fails() {
        let _key = KEY_LOCK.lock().await;
        let device = "77777777-7777-4777-8777-777777777777";
        // The address cannot be used, so the owner can never confirm the revocation.
        save_device(device, "not an address").await;
        call(
            MethodName::ConnectionsSshKeySetupCommand,
            json!({"address": "user@host.example"}),
        )
        .await
        .unwrap();
        let error = expect_error(
            MethodName::ConnectionsDevicesRemove,
            json!({"deviceId": device}),
        )
        .await;
        assert!(
            error.message.contains("Enter an SSH alias"),
            "{}",
            error.message
        );
        assert!(saved_device_ids().iter().any(|id| id == device));
        expect_invalid_params(MethodName::ConnectionsDevicesRemove, json!({})).await;
    }

    #[tokio::test]
    async fn removing_a_device_without_a_connections_key_forgets_it() {
        let _key = KEY_LOCK.lock().await;
        let device = "88888888-8888-4888-8888-888888888888";
        save_device(device, "ssh://user@127.0.0.1:9").await;
        let key = silo_engine::remote::connections_public_key().unwrap();
        if key.is_some() {
            // Another test created this engine's key; remove it to model "never set up".
            let directory = connections_config_path().parent().unwrap().to_path_buf();
            let _ = std::fs::remove_file(directory.join("id_ed25519"));
            let _ = std::fs::remove_file(directory.join("id_ed25519.pub"));
        }
        let removed = call(
            MethodName::ConnectionsDevicesRemove,
            json!({"deviceId": device}),
        )
        .await
        .unwrap();
        assert_eq!(removed, Value::Null);
        assert!(!saved_device_ids().iter().any(|id| id == device));
    }

    fn loopback_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    }

    #[test]
    fn a_browser_callback_reaches_the_owner_and_only_its_reply_comes_back() {
        let (mut browser, relayed) = loopback_pair();
        let (controller_write, mut owner_read) = UnixStream::pair().unwrap();
        let (mut owner_write, controller_read) = UnixStream::pair().unwrap();
        let owner = thread::spawn(move || {
            let mut request = vec![0u8; 4096];
            let mut length = 0;
            while !request[..length]
                .windows(4)
                .any(|window| window == b"\r\n\r\n")
            {
                length += owner_read.read(&mut request[length..]).unwrap();
            }
            owner_write
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK")
                .unwrap();
            request.truncate(length);
            request
        });
        let relay =
            thread::spawn(move || exchange_callback(relayed, controller_write, controller_read));
        browser
            .write_all(b"GET /github/callback?state=s&code=c HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .unwrap();
        let mut reply = String::new();
        browser.read_to_string(&mut reply).unwrap();
        assert_eq!(reply, "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
        relay.join().unwrap();
        let seen = owner.join().unwrap();
        assert!(seen.starts_with(b"GET /github/callback?state=s&code=c HTTP/1.1"));
    }

    #[test]
    fn an_oversized_or_incomplete_callback_is_never_relayed() {
        let (mut browser, mut relayed) = loopback_pair();
        browser
            .write_all(&vec![b'a'; CALLBACK_HEADER_LIMIT + 1])
            .unwrap();
        assert_eq!(read_callback_header(&mut relayed), None);

        let (mut browser, mut relayed) = loopback_pair();
        browser
            .write_all(b"GET /github/callback HTTP/1.1\r\n")
            .unwrap();
        let started = Instant::now();
        assert_eq!(read_callback_header(&mut relayed), None);
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn a_loopback_link_is_shown_only_while_its_route_is_carried() {
        let device = "99999999-9999-4999-8999-999999999999";
        let authorize = |port: u16| {
            format!(
                "https://github.com/login/oauth/authorize?redirect_uri=http%3A%2F%2F127.0.0.1%3A{port}%2Fgithub%2Fcallback&state=s"
            )
        };
        let install = "https://github.com/apps/example/installations/new";
        assert!(link_is_safe(device, install));
        assert!(!link_is_safe(device, &authorize(45001)));
        let stop = Arc::new(AtomicBool::new(false));
        callback_routes().insert(
            device.to_owned(),
            CallbackRoute {
                port: 45001,
                stop: stop.clone(),
            },
        );
        assert!(link_is_safe(device, &authorize(45001)));
        assert!(!link_is_safe(device, &authorize(45002)));
        assert!(!link_is_safe(
            device,
            "https://github.com/login/oauth/authorize?redirect_uri=http%3A%2F%2Fevil.example%2Fcb"
        ));
        let mut state = serde_json::to_value(github_state_with_link(&authorize(45002))).unwrap();
        state = controller_view(MethodName::GitHubState, device, state).unwrap();
        assert_eq!(state["pendingAuthorizationUrl"], Value::Null);
        let mut state = serde_json::to_value(github_state_with_link(&authorize(45001))).unwrap();
        state = controller_view(MethodName::GitHubState, device, state).unwrap();
        assert_eq!(state["pendingAuthorizationUrl"], authorize(45001).as_str());
        let reopen = controller_view(
            MethodName::GitHubConnectReopen,
            device,
            json!({"url": authorize(45002)}),
        );
        assert_eq!(reopen.unwrap_err(), ROUTE_UNAVAILABLE);
        close_callback_route(device);
        assert!(stop.load(Ordering::Acquire));
        assert!(!link_is_safe(device, &authorize(45001)));
    }

    fn github_state_with_link(url: &str) -> github::GitHubState {
        github::GitHubState {
            policy_revision: 1,
            personal_token: github::GitHubPersonalToken {
                state: github::GitHubPersonalTokenState::Disconnected,
                saved: false,
                account: None,
                message: None,
            },
            state: github::GitHubConnectionState::Connecting,
            account: None,
            access_enabled: false,
            device_identity: None,
            repository_catalog: Vec::new(),
            repository_catalog_status: github::GitHubRepositoryCatalogStatus::Available,
            computers: Vec::new(),
            computer_operations: Vec::new(),
            pending_authorization_url: Some(url.to_owned()),
        }
    }
}
