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

/// The macOS secrets and GitHub host proxy. Each function resolves the computer by name in
/// the macOS store and calls the engine's `proxy_*` entry point. No computer is a macOS
/// proxy computer until the access runtime is installed.
mod macos_proxy {
    use super::{SilodMacosHost, RUNTIME_HOST};
    use serde_json::Value;
    use silo_engine::macos_computers as engine;
    use std::sync::Arc;

    const NOT_INSTALLED: &str = "The access runtime is not initialized.";

    fn context() -> Result<engine::MacosContext, String> {
        let host = RUNTIME_HOST.get().ok_or(NOT_INSTALLED)?;
        Ok(engine::MacosContext::new(
            host.context.clone(),
            Arc::new(SilodMacosHost(host.clone())),
        ))
    }

    /// The context for identity lookups, or `None` before the runtime is installed.
    pub(super) fn lookup_context() -> Option<engine::MacosContext> {
        context().ok()
    }

    /// Whether `computer` (a name) is a macOS computer served by the host proxy.
    pub(super) fn contains_name(computer: &str) -> bool {
        context().is_ok_and(|context| engine::proxy_contains_name(&context, computer))
    }

    pub(super) fn apply_secrets(computer: &str) -> Result<Vec<String>, String> {
        engine::proxy_apply_secrets(&context()?, computer)
    }

    pub(super) fn validate_computers(computers: &[String]) -> Result<(), String> {
        engine::proxy_validate_computers(&context()?, computers)
    }

    pub(super) fn revoke_secret(computer: &str, name: &str) -> Result<bool, String> {
        engine::proxy_revoke_secret(&context()?, computer, name)
    }

    pub(super) fn scoped_cached_tokens(
        computer: &str,
    ) -> Result<silo_engine::runtime::ScopedTokens, String> {
        let mut tokens = silo_engine::runtime::ScopedTokens::default();
        tokens.extend(engine::proxy_scoped_cached_tokens(&context()?, computer)?);
        Ok(tokens)
    }

    pub(super) fn github_policy_is_cached(computer: &str, profile: &Value) -> Result<bool, String> {
        engine::proxy_github_policy_is_cached(&context()?, computer, profile)
    }

    pub(super) fn apply_github_policy(
        computer: &str,
        revision: u64,
        profile: &Value,
    ) -> Result<(), String> {
        engine::proxy_apply_github_policy(&context()?, computer, revision, profile)
    }

    pub(super) fn apply_github_identity(computer: &str, identity: &Value) -> Result<(), String> {
        engine::proxy_apply_github_identity(&context()?, computer, identity)
    }

    pub(super) fn is_pending_restore(computer: &str) -> bool {
        context().is_ok_and(|context| engine::proxy_is_pending_restore(&context, computer))
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

    fn access_dispatch(&self, method: &str, params: &Value) -> Result<Value, BridgeError> {
        match method {
            "github.connect" => start_owner_connect(&self.0.context),
            "guest.exec.start" => {
                let typed = serde_json::from_value(params.clone())
                    .map_err(|error| BridgeError::from(format!("Invalid params: {error}")))?;
                super::guest::start_exec(self.0.context.clone(), typed)
                    .map_err(bridge_error)
                    .and_then(|accepted| {
                        serde_json::to_value(accepted)
                            .map_err(|error| BridgeError::from(error.to_string()))
                    })
            }
            "guest.exec.get" => {
                let typed = serde_json::from_value(params.clone())
                    .map_err(|error| BridgeError::from(format!("Invalid params: {error}")))?;
                super::guest::exec_get_output(self.0.context.as_ref(), &typed)
                    .map(|output| output.value)
                    .map_err(bridge_error)
            }
            "guest.exec.input" => {
                let typed = serde_json::from_value(params.clone())
                    .map_err(|error| BridgeError::from(format!("Invalid params: {error}")))?;
                super::guest::exec_input_owner(&typed)
                    .map(|_| Value::Null)
                    .map_err(bridge_error)
            }
            "desktop.job" => super::display::start_owner_desktop_job(&self.0.context, params)
                .map_err(bridge_error),
            "operations.list"
            | "operations.job.get"
            | "operations.cancel"
            | "operations.job.acknowledge" => {
                owner_job_operation(self.0.context.as_ref(), method, params)
            }
            name if silo_engine::remote::is_owner_method(name) => {
                super::remote::serve_owner(&self.0.context, name, params)
            }
            _ => run_local(self.0.context.as_ref(), method, params).map_err(BridgeError::from),
        }
    }

    fn open_event_stream(
        &self,
        selectors: Vec<String>,
    ) -> Result<Box<dyn silo_engine::remote_events::EventSource>, String> {
        let context = &self.0.context;
        silo_engine::remote_events::BusSource::open(
            context.event_bus(),
            &context.local_device().device_id,
            selectors,
        )
        .map(|source| Box::new(source) as Box<dyn silo_engine::remote_events::EventSource>)
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
    } else if message == silo_engine::github::CONNECTION_CANCELLED {
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

/// Every request to another device's engine goes through here. Tests answer for the
/// devices they stub, so no SSH connection is needed to exercise the controller side.
pub(crate) fn call_owner(device: &str, method: &str, params: Value) -> Result<Value, BridgeError> {
    #[cfg(test)]
    if let Some(reply) = tests::stubbed_owner(device, method, &params) {
        return reply;
    }
    silo_engine::remote::call_remote_typed(device, method, params)
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

/// The name of a computer by identity: a Linux computer from the runtime metadata, else a
/// macOS computer from the macOS store.
fn computer_name(paths: &RuntimePaths, id: &str) -> Result<String, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid computer identity.".to_owned())?;
    let linux = runtime::read_metadata(&paths.metadata)
        .map_err(|error| error.to_string())?
        .computers
        .into_iter()
        .find(|computer| computer.id() == id)
        .map(|computer| computer.name().to_owned());
    linux
        .or_else(|| {
            let context = macos_proxy::lookup_context()?;
            silo_engine::macos_computers::name_for_id(&context, id)
        })
        .ok_or_else(|| "This computer no longer exists.".to_owned())
}

/// The stable identity of a computer by name, or `None` when it no longer exists.
fn computer_id(paths: &RuntimePaths, name: &str) -> Option<String> {
    runtime::resolve_computer_id(paths, name).ok().or_else(|| {
        let context = macos_proxy::lookup_context()?;
        silo_engine::macos_computers::id_for_name(&context, name)
    })
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
                call_owner(&device, name, routed).map(|value| (device, value))
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

/// The sign-in this engine is running, if any. GitHub's flow is one at a time (05 §4.2):
/// admission takes the slot atomically, and the job, the generation and the callback
/// listener all belong to that one flow until it ends. Only the active job's loopback
/// listener may be reached by a `github.connect.callback` stream, and only the active
/// job may cancel the engine's shared cancellation state.
static CONNECT_SLOT: Mutex<ConnectSlot> = Mutex::new(ConnectSlot::Idle);

enum ConnectSlot {
    Idle,
    /// Admitted; the job is being allocated.
    Admitting,
    Active {
        job_id: JobId,
        generation: u64,
    },
}

fn connect_slot() -> std::sync::MutexGuard<'static, ConnectSlot> {
    CONNECT_SLOT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Holds the connect slot; dropping it frees the slot.
struct ConnectAdmission;

impl ConnectAdmission {
    fn admit() -> Result<Self, DispatchError> {
        let mut slot = connect_slot();
        if !matches!(*slot, ConnectSlot::Idle) {
            return Err(DispatchError {
                code: ErrorCode::Busy,
                message: "A GitHub sign-in is already in progress. Finish or cancel it first."
                    .to_owned(),
            });
        }
        *slot = ConnectSlot::Admitting;
        Ok(Self)
    }

    fn activate(&self, job_id: JobId, generation: u64) {
        *connect_slot() = ConnectSlot::Active { job_id, generation };
    }
}

impl Drop for ConnectAdmission {
    fn drop(&mut self) {
        *connect_slot() = ConnectSlot::Idle;
    }
}

/// Whether `job_id` is the flow that currently holds the slot.
fn is_active_connect(job_id: &str) -> bool {
    matches!(&*connect_slot(), ConnectSlot::Active { job_id: active, .. } if active.as_str() == job_id)
}

/// Cancels the engine's GitHub flow on behalf of `job`, which is a no-op unless `job` is
/// still the flow that owns it: a finished job must never cancel a later sign-in.
fn cancel_connect_for(job: &JobId) -> Result<(), String> {
    if is_active_connect(job.as_str()) {
        silo_engine::github::cancel_connection().map(|_| ())
    } else {
        Ok(())
    }
}

/// Starts a named worker thread. Tests make a chosen name fail to start, to prove the
/// caller unwinds everything it created.
fn spawn_named(name: &'static str, work: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    #[cfg(test)]
    if tests::spawn_fails(name) {
        return Err(std::io::Error::other("thread start refused by the test"));
    }
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(work)
        .map(|_| ())
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
        Some(device) => tokio::task::spawn_blocking(move || start_remote_connect(&device))
            .await
            .map_err(|_| DispatchError::internal("The remote operation could not finish."))??,
        // Allocation reads SQLite and the credential lock can span Keychain access.
        None => tokio::task::spawn_blocking(move || start_local_connect(&engine))
            .await
            .map_err(|_| DispatchError::internal("The access operation could not finish."))??,
    };
    to_output(JobStartResult { job_id })
}

/// Starts this engine's own GitHub sign-in as a durable job. The client opens
/// `pendingAuthorizationUrl`; the engine never launches a browser.
fn start_local_connect(ctx: &SilodContext) -> Result<JobId, DispatchError> {
    #[cfg(test)]
    tests::note_blocking_work("github.connect");
    let admission = ConnectAdmission::admit()?;
    let accepted = silo_engine::jobs::allocate(ctx, "github.connect")
        .map_err(HandlerContext::map_registry_error)?;
    let job_id = accepted.job_id;
    let registry = ctx.job_registry().map_err(DispatchError::internal)?;
    let generation = silo_engine::github::prepare_connect();
    admission.activate(job_id.clone(), generation);
    let cancel_job = job_id.clone();
    registry.register_cancel(
        job_id.clone(),
        silo_engine::jobs::CancelRoute::Github,
        move || cancel_connect_for(&cancel_job),
    );
    let engine = ctx.clone();
    let running = job_id.clone();
    let spawned = spawn_named("silod-github-connect", move || {
        let result = silo_engine::github::connect(generation, |_| Ok(()));
        // The flow is over: free the slot before the outcome is published, so a client
        // that reacts to the finished job can start the next sign-in at once.
        drop(admission);
        finish_connect(&engine, running, result);
    });
    if let Err(error) = spawned {
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

/// The wire error a controller receives for a failure this engine reports locally: the
/// canonical code survives the trip, so a controller sees `not_found` and `busy` as such.
pub(crate) fn bridge_error(error: DispatchError) -> BridgeError {
    use silo_engine::bridge_error::ErrorCode as Code;
    let code = match error.code {
        ErrorCode::UpdateInProgress => Code::UpdateInProgress,
        ErrorCode::UnsupportedRemoteOperation => Code::UnsupportedRemoteOperation,
        ErrorCode::Cancelled => Code::Cancelled,
        ErrorCode::AlreadyQueued => Code::AlreadyQueued,
        ErrorCode::Busy => Code::Busy,
        ErrorCode::NotFound => Code::NotFound,
        ErrorCode::IncompatibleVersion => Code::IncompatibleVersion,
        _ => Code::Internal,
    };
    let finer = matches!(code, Code::Internal) && error.code != ErrorCode::Internal;
    let bridge = BridgeError::new(code, error.message);
    if finer {
        // The owner's own code (`not_running`...) travels beside the coarse one.
        if let Some(name) = serde_json::to_value(error.code)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
        {
            return bridge.with_api_code(name);
        }
    }
    bridge
}

/// Owner side of a controller's job follow-up: reads, cancels or acknowledges a job in
/// this engine's own registry, with the same results and error codes as `operations.*`
/// on `engine.sock`. Jobs keep the owner's `jobId`.
fn owner_job_operation(
    ctx: &SilodContext,
    method: &str,
    params: &Value,
) -> Result<Value, BridgeError> {
    if method == "operations.list" {
        let queue = ctx.operation_queue().map_err(BridgeError::from)?;
        return serde_json::to_value(queue).map_err(|error| BridgeError::from(error.to_string()));
    }
    let job_id = params["jobId"]
        .as_str()
        .ok_or_else(|| BridgeError::from("Missing job identity."))
        .and_then(|id| {
            JobId::new(id.to_owned()).map_err(|_| BridgeError::from("Invalid job identity."))
        })?;
    let registry_error = |error| bridge_error(HandlerContext::map_registry_error(error));
    match method {
        "operations.job.get" => {
            let offset = params.get("offsetBytes").and_then(Value::as_u64);
            let found = silo_engine::jobs::get_at(ctx, &job_id, offset)
                .map_err(registry_error)?
                .ok_or_else(|| {
                    BridgeError::new(
                        silo_engine::bridge_error::ErrorCode::NotFound,
                        "The job was not found.",
                    )
                })?;
            serde_json::to_value(found).map_err(|error| BridgeError::from(error.to_string()))
        }
        "operations.cancel" => {
            ctx.job_registry()?
                .cancel(&job_id)
                .map_err(registry_error)?;
            Ok(Value::Null)
        }
        _ => {
            ctx.job_registry()?
                .acknowledge(&job_id)
                .map_err(registry_error)?;
            // The outcome is acknowledged: the replay of an exec job is no longer needed.
            silo_engine::guest::forget_finished(&job_id);
            ctx.job_queue_changed();
            Ok(Value::Null)
        }
    }
}

#[cfg(test)]
pub(crate) fn owner_job_operation_for_tests(
    ctx: &SilodContext,
    method: &str,
    params: &Value,
) -> Result<Value, BridgeError> {
    owner_job_operation(ctx, method, params)
}

/// Owner side of a controller's `github.connect`: starts the job and, once its browser
/// link exists, answers with the job and the link so the controller can reserve the
/// callback port before it shows the link to anyone.
fn start_owner_connect(ctx: &SilodContext) -> Result<Value, BridgeError> {
    let job_id = start_local_connect(ctx).map_err(bridge_error)?;
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
    if !is_active_connect(job_id) {
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
/// Tests poll at once; nothing in them depends on elapsed time.
const OWNER_POLL: Duration = if cfg!(test) {
    Duration::ZERO
} else {
    Duration::from_secs(2)
};
const OWNER_POLL_FAILURES: u32 = 5;

fn cancel_owner_job(device: &str, job: &str) {
    let _ = call_owner(device, "operations.cancel", json!({ "jobId": job }));
}

/// Controller side of `github.connect` for a device: the owner runs the sign-in as its
/// own job and keeps it, so the client follows it by `(deviceId, jobId)` through
/// `operations.*`. This engine reserves the callback port, carries the browser's callback
/// to the owner, and closes the route when the owner's job ends.
fn start_remote_connect(device: &str) -> Result<JobId, DispatchError> {
    let reply = call_owner(device, "github.connect", json!({}))
        .map_err(HandlerContext::map_engine_error)?;
    let owner_job = reply["jobId"]
        .as_str()
        .ok_or_else(|| DispatchError::internal("The other Mac returned no connection job."))?
        .to_owned();
    follow_owner_connect(device, owner_job, reply["url"].as_str())
}

/// Everything after the owner accepted its sign-in: reserve the callback port, carry the
/// browser's callback, and watch the owner's job so the route ends with it. Any step that
/// fails unwinds what the earlier ones created: the route and the owner's job.
fn follow_owner_connect(
    device: &str,
    owner_job: String,
    url: Option<&str>,
) -> Result<JobId, DispatchError> {
    let fail = |message: &str| {
        cancel_owner_job(device, &owner_job);
        DispatchError::internal(message)
    };
    let job = JobId::new(owner_job.clone())
        .map_err(|_| fail("The other Mac returned an invalid connection job."))?;
    let url = url.ok_or_else(|| fail("The other Mac did not produce a GitHub link. Try again."))?;
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
        spawn_named("silod-github-callback", move || {
            accept_callbacks(listener, device, owner_job, stop)
        })
    };
    if accepting.is_err() {
        close_callback_route(device);
        return Err(fail("Could not listen for GitHub's reply on this Mac."));
    }
    let watching = {
        let (device, owner_job, stop) = (device.to_owned(), owner_job.clone(), stop.clone());
        spawn_named("silod-github-route", move || {
            watch_owner_connect(&device, &owner_job, &stop)
        })
    };
    if watching.is_err() {
        // Nothing would end the route with the sign-in: close it and stop the owner's flow,
        // so no listener outlives the request.
        close_route_if_current(device, &stop);
        return Err(fail("Could not follow the other Mac's GitHub sign-in."));
    }
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

/// Follows the owner's job until it ends and then closes the callback route. The outcome
/// stays with the owner until a client acknowledges it by `(deviceId, jobId)`.
fn watch_owner_connect(device: &str, owner_job: &str, stop: &Arc<AtomicBool>) {
    let mut failures = 0u32;
    while !stop.load(Ordering::Acquire) {
        thread::sleep(OWNER_POLL);
        match call_owner(device, "operations.job.get", json!({ "jobId": owner_job })) {
            Err(error) if error.code == silo_engine::bridge_error::ErrorCode::NotFound => break,
            Ok(value) => {
                failures = 0;
                match serde_json::from_value::<silo_api::jobs::JobLookupResult>(value) {
                    Ok(found) if found.state == JobState::Finished => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            Err(_) => {
                failures += 1;
                if failures >= OWNER_POLL_FAILURES {
                    break;
                }
            }
        }
    }
    close_route_if_current(device, stop);
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
    // The operation the host answers with is canonical: a push that was already running
    // for this repository keeps its own identity, and a start that could not be prepared
    // has none at all.
    let (reply, accepted) = if let Some(device) = remote_target(context, typed.device_id.as_deref())
    {
        // The owner runs and records the push; its job keeps the owner's identity.
        let request = json!({
            "computerId": typed.computer_id,
            "path": typed.repository_path,
            "operationId": typed.operation_id,
            "target": typed.target,
        });
        blocking_bridge(move || {
            let reply = call_owner(&device, "repository.push.start", request)?;
            let job_id = push_job_name(&reply)?;
            let accepted =
                match call_owner(&device, "operations.job.get", json!({ "jobId": job_id })) {
                    Ok(_) => true,
                    Err(error) if error.code == silo_engine::bridge_error::ErrorCode::NotFound => {
                        false
                    }
                    Err(error) => return Err(error),
                };
            Ok((reply, accepted))
        })
        .await?
    } else {
        let target = serde_json::to_value(&typed.target).map_err(serialization)?;
        let engine = context.silod_context().clone();
        blocking(move || {
            let paths = local_paths()?;
            let name = computer_name(&paths, &typed.computer_id)?;
            let target = serde_json::from_value(target).map_err(|error| error.to_string())?;
            // The operation's state reaches clients as job progress and `push.changed`.
            let reply = silo_engine::host_push_operations::start_result(
                &paths,
                name,
                typed.repository_path,
                typed.operation_id,
                target,
            );
            let job_id = JobId::new(push_job_name(&reply).map_err(|error| error.message)?)
                .map_err(|error| error.to_string())?;
            let accepted = silo_engine::jobs::get_at(&engine, &job_id, None)
                .map_err(|error| error.to_string())?
                .is_some();
            Ok((reply, accepted))
        })
        .await?
    };
    if !accepted {
        // Preparation failures never create a job; refuse instead of naming one.
        let message = reply["message"]
            .as_str()
            .unwrap_or("The push could not be started.");
        return Err(failure(message));
    }
    let job_id = JobId::new(push_job_name(&reply).map_err(HandlerContext::map_engine_error)?)
        .map_err(serialization)?;
    to_output(JobStartResult { job_id })
}

/// The job identity of the push operation a host reported.
fn push_job_name(reply: &Value) -> Result<String, BridgeError> {
    reply["operationId"]
        .as_str()
        .map(|id| format!("push:{id}"))
        .ok_or_else(|| BridgeError::from("The push reply names no operation."))
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
            blocking_bridge(move || call_owner(&device, "repository.push.status", request)).await?
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
            blocking_bridge(move || call_owner(&device, "repository.dismiss", request)).await?;
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
    let (state, local) = match remote_target(context, device_id.as_deref()) {
        Some(device) => {
            let mut state = blocking_bridge(move || {
                owner_network_change(device, computer_id, port, host_port, scheme, publishing)
            })
            .await?;
            restore_owner_names(&mut state);
            (state, false)
        }
        None => {
            let state = blocking(move || {
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
            .await?;
            (state, true)
        }
    };
    let state = canonical::<network::NetworkState>(state, "network state").map_err(failure)?;
    announce_network_state(&engine, local, &state);
    Ok(output(state))
}

/// Publishes `network.changed` for state this engine owns. An event carries this engine's
/// device identity, so another device's state is only ever answered to the caller: it
/// reaches other clients through that device's own events, never under this identity.
fn announce_network_state(ctx: &SilodContext, local: bool, state: &Value) {
    if local {
        publish(ctx, "network.changed", state.clone());
    }
}

/// The owner's port change, carried over Connections.
fn owner_network_change(
    device: String,
    computer_id: String,
    port: u16,
    host_port: Option<u16>,
    scheme: Option<network::NetworkScheme>,
    publishing: bool,
) -> Result<Value, BridgeError> {
    #[cfg(test)]
    if let Some(reply) = tests::stubbed_owner(
        &device,
        if publishing {
            "network.publish"
        } else {
            "network.unpublish"
        },
        &json!({"computerId": computer_id, "port": port}),
    ) {
        return reply;
    }
    if publishing {
        silo_engine::remote_network::remote_save_network_port(
            device,
            computer_id,
            port,
            host_port,
            scheme_name(scheme),
        )
    } else {
        silo_engine::remote_network::remote_remove_network_port(device, computer_id, port)
    }
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
    let (value, local) = match remote_target(context, typed.device_id.as_deref()) {
        Some(device) => {
            let mut value = blocking_bridge(move || owner_ssh_access_save(device, typed)).await?;
            restore_owner_names(&mut value);
            (value, false)
        }
        None => {
            let value = blocking(move || {
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
            .await?;
            (value, true)
        }
    };
    let value =
        canonical::<ssh_access::SshAccessState>(value, "SSH access state").map_err(failure)?;
    // `network.changed` carries this device's published-port state; a listener change is
    // its hint to re-read `sshAccess.state` too. Another device's change says nothing
    // about this device's network, so it publishes nothing here.
    if local {
        if let Ok(network) = blocking(local_network_state).await {
            if let Ok(network) = canonical::<network::NetworkState>(network, "network state") {
                announce_network_state(&engine, true, &network);
            }
        }
    }
    Ok(output(value))
}

/// The owner's SSH access change, carried over Connections.
fn owner_ssh_access_save(
    device: String,
    typed: ssh_access::SshAccessSaveParams,
) -> Result<Value, BridgeError> {
    #[cfg(test)]
    if let Some(reply) = tests::stubbed_owner(
        &device,
        "ssh.access.save",
        &json!({"computerId": typed.computer_id}),
    ) {
        return reply;
    }
    silo_engine::remote_ssh_access::remote_save_ssh_access(
        &device,
        typed.computer_id,
        typed.enabled,
        typed.port,
        typed.bind_address,
        typed.keys,
    )
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

/// Removes a connected Mac (95 A-36). The engine records the removal, has the owner drop
/// every key this Mac may have installed there (including one whose private half was
/// deleted or rotated), records the owner's confirmation, and only then forgets the device
/// and closes everything it holds for it. An owner that cannot confirm leaves the device
/// saved and the removal pending; the key is never reinstalled to make a retry succeed.
async fn connections_devices_remove(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed = context.decode_params::<connections::DeviceIdParams>(params)?;
    let device = typed.device_id;
    silo_engine::remote::remove_device(device.clone())
        .await
        .map_err(HandlerContext::map_engine_error)?;
    close_callback_route(&device);
    // Sessions that listened to the removed device end; its relay stops with them.
    context.event_bus().raise_gap_for_device(&device);
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
                call_owner(
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
pub(crate) mod tests {
    use std::{
        os::unix::net::UnixStream,
        sync::OnceLock,
        time::{Duration, Instant},
    };

    use silo_engine::context::{
        EngineIdentity, EnginePaths, Event, MainThread, PackageKind, ProcessControl,
    };

    use super::*;
    use crate::{dispatcher::RequestContext, identity::LocalDeviceIdentity};

    pub(crate) const LOCAL_DEVICE: &str = "11111111-1111-4111-8111-111111111111";
    pub(crate) const COMPUTER: &str = "22222222-2222-4222-8222-222222222222";
    const MISSING_COMPUTER: &str = "33333333-3333-4333-8333-333333333333";
    const UNKNOWN_DEVICE: &str = "44444444-4444-4444-8444-444444444444";
    pub(crate) const NOT_CONNECTED: &str = "This device is no longer connected.";

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

    pub(crate) struct Fixture {
        _root: tempfile::TempDir,
        pub(crate) context: Arc<SilodContext>,
        home: PathBuf,
    }

    /// One engine context for the whole test binary: the engine's GitHub, secrets and
    /// Connections state are process-wide, so every test shares this temporary home.
    pub(crate) fn fixture() -> &'static Fixture {
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

    pub(crate) fn request_context() -> HandlerContext {
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

    type OwnerStub = Arc<dyn Fn(&str, &Value) -> Result<Value, BridgeError> + Send + Sync>;

    /// Stand-ins for other devices' engines, by device id.
    static OWNER_STUBS: Mutex<BTreeMap<String, OwnerStub>> = Mutex::new(BTreeMap::new());

    pub(crate) fn stubbed_owner(
        device: &str,
        method: &str,
        params: &Value,
    ) -> Option<Result<Value, BridgeError>> {
        let stub = OWNER_STUBS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(device)
            .cloned()?;
        Some(stub(method, params))
    }

    /// Answers requests to `device` with `answer` until the guard drops.
    pub(crate) struct StubOwner(String);

    pub(crate) fn stub_owner(
        device: &str,
        answer: impl Fn(&str, &Value) -> Result<Value, BridgeError> + Send + Sync + 'static,
    ) -> StubOwner {
        OWNER_STUBS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(device.to_owned(), Arc::new(answer));
        StubOwner(device.to_owned())
    }

    impl Drop for StubOwner {
        fn drop(&mut self) {
            OWNER_STUBS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&self.0);
        }
    }

    /// Worker threads that must fail to start, by name.
    static FAILING_SPAWNS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

    pub(super) fn spawn_fails(name: &str) -> bool {
        FAILING_SPAWNS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&name)
    }

    /// Makes `name` fail to start until the guard drops.
    struct FailSpawn(&'static str);

    impl FailSpawn {
        fn new(name: &'static str) -> Self {
            FAILING_SPAWNS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(name);
            Self(name)
        }
    }

    impl Drop for FailSpawn {
        fn drop(&mut self) {
            FAILING_SPAWNS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .retain(|name| *name != self.0);
        }
    }

    /// The threads that ran blocking work, by label.
    static BLOCKING_WORK: Mutex<Vec<(&'static str, std::thread::ThreadId)>> =
        Mutex::new(Vec::new());

    pub(super) fn note_blocking_work(label: &'static str) {
        BLOCKING_WORK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((label, std::thread::current().id()));
    }

    /// Tests that run or cancel this engine's GitHub sign-in must not overlap.
    static CONNECT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
        edit_connections_config(|config| {
            config["devices"]
                .as_array_mut()
                .unwrap()
                .retain(|device| device["id"] != id);
            config["devices"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": id, "name": "Remote Mac", "address": address}));
        });
    }

    /// Edits the settings file one test at a time and replaces it atomically, so neither
    /// a concurrent edit nor a concurrent read ever sees a lost or half-written file.
    fn edit_connections_config(edit: impl FnOnce(&mut Value)) {
        static EDIT: Mutex<()> = Mutex::new(());
        let _edit = EDIT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = connections_config_path();
        let mut config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        edit(&mut config);
        let temporary = path.with_extension("json.test");
        std::fs::write(&temporary, serde_json::to_vec(&config).unwrap()).unwrap();
        std::fs::rename(temporary, path).unwrap();
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
    async fn macos_computers_resolve_through_the_macos_proxy_store() {
        const MACOS_COMPUTER: &str = "55555555-5555-4555-8555-555555555555";
        let fixture = fixture();
        let folder = fixture
            .context
            .paths()
            .app_data
            .join("macos-computers")
            .join(MACOS_COMPUTER);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("computer.json"),
            json!({"name": "Mac Dev"}).to_string(),
        )
        .unwrap();
        let paths = local_paths().unwrap();

        assert_eq!(computer_name(&paths, MACOS_COMPUTER).unwrap(), "Mac Dev");
        assert_eq!(
            computer_id(&paths, "Mac Dev").as_deref(),
            Some(MACOS_COMPUTER)
        );
        assert_eq!(
            computer_id(&paths, "mac dev").as_deref(),
            Some(MACOS_COMPUTER)
        );
        assert!(macos_proxy::contains_name("Mac Dev"));
        assert!(!macos_proxy::contains_name("dev"));
        // Linux computers still resolve first, and unknown names stay unknown.
        assert_eq!(computer_id(&paths, "dev").as_deref(), Some(COMPUTER));
        assert_eq!(computer_id(&paths, "gone"), None);

        let mut state = json!({"computers": [{"computer": "Mac Dev"}, {"computer": "gone"}]});
        project_github_ids(&mut state, &paths);
        assert_eq!(state["computers"], json!([{"computerId": MACOS_COMPUTER}]));
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
        let _flow = CONNECT_LOCK.lock().await;
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
    async fn one_github_sign_in_runs_at_a_time_and_only_it_can_cancel_or_receive_callbacks() {
        let _flow = CONNECT_LOCK.lock().await;
        let first = JobId::new("job:7001".to_owned()).unwrap();
        let admission = ConnectAdmission::admit().unwrap();
        // While the first flow is being admitted, a second one is refused outright.
        let early = expect_error(MethodName::GitHubConnect, json!({})).await;
        assert_eq!(early.code, ErrorCode::Busy);
        admission.activate(first.clone(), 5);

        // It does not overwrite the active identity, allocate a job or reach the engine.
        let second = expect_error(MethodName::GitHubConnect, json!({})).await;
        assert_eq!(second.code, ErrorCode::Busy);
        assert!(second.message.contains("already in progress"));
        assert!(is_active_connect("job:7001"));
        assert!(!is_active_connect("job:7002"));
        assert!(callback_listener_for("job:7002").is_err());

        // A job that is not the active flow cannot cancel the shared flow; the active one can.
        let generation = silo_engine::github::prepare_connect();
        cancel_connect_for(&JobId::new("job:7002".to_owned()).unwrap()).unwrap();
        assert_eq!(silo_engine::github::prepare_connect(), generation);
        cancel_connect_for(&first).unwrap();
        assert_ne!(silo_engine::github::prepare_connect(), generation);

        // Ending the flow frees the slot for the next one.
        drop(admission);
        assert!(!is_active_connect("job:7001"));
        drop(ConnectAdmission::admit().unwrap());
    }

    #[tokio::test]
    async fn starting_a_local_sign_in_does_not_block_an_async_worker() {
        let _flow = CONNECT_LOCK.lock().await;
        let _refused = FailSpawn::new("silod-github-connect");
        BLOCKING_WORK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        // This runtime has one thread, this one: storage and credential access that ran
        // inline would run here and stall every other request.
        let _ = call(MethodName::GitHubConnect, json!({})).await;
        let ran_on: Vec<_> = BLOCKING_WORK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|(label, _)| *label == "github.connect")
            .map(|(_, thread)| *thread)
            .collect();
        assert!(!ran_on.is_empty());
        assert!(ran_on
            .iter()
            .all(|thread| *thread != std::thread::current().id()));
    }

    #[tokio::test]
    async fn a_connect_thread_that_cannot_start_frees_the_slot_and_finishes_its_job() {
        let _flow = CONNECT_LOCK.lock().await;
        let _refused = FailSpawn::new("silod-github-connect");
        let error = expect_error(MethodName::GitHubConnect, json!({})).await;
        assert!(error.message.contains("Could not start GitHub connect"));
        drop(ConnectAdmission::admit().unwrap());
    }

    /// A GitHub link whose callback is carried on `port`.
    fn link_with_callback(port: u16) -> String {
        format!(
            "https://github.com/login/oauth/authorize?client_id=x&redirect_uri=http%3A%2F%2F127.0.0.1%3A{port}%2Fgithub%2Fcallback&state=s"
        )
    }

    fn free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn owner_not_found() -> BridgeError {
        BridgeError::new(
            silo_engine::bridge_error::ErrorCode::NotFound,
            "The job was not found.",
        )
    }

    fn finished_owner_job(owner_job: &str) -> Value {
        json!({
            "jobId": owner_job,
            "state": "finished",
            "outcome": {"result": {"connected": true}},
        })
    }

    #[tokio::test]
    async fn a_route_watcher_that_cannot_start_unwinds_the_route_and_the_owner_job() {
        let _flow = CONNECT_LOCK.lock().await;
        let _push = PUSH_LOCK.lock().await;
        let owner = "77777777-7777-4777-8777-77777777777b";
        let calls = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
        let seen = calls.clone();
        let _stub = stub_owner(owner, move |method, params| {
            seen.lock()
                .unwrap()
                .push((method.to_owned(), params.clone()));
            Ok(Value::Null)
        });
        let _refused = FailSpawn::new("silod-github-route");
        let link = link_with_callback(free_port());

        let error = follow_owner_connect(owner, "job:5003".to_owned(), Some(&link)).unwrap_err();
        assert!(error.message.contains("Could not follow"));

        // The callback route is closed and its listener told to stop.
        assert!(callback_routes().get(owner).is_none());
        // The owner's sign-in was cancelled, not left running.
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "operations.cancel");
        assert_eq!(calls[0].1["jobId"], "job:5003");
    }

    #[tokio::test]
    async fn a_remote_sign_in_is_the_owners_job_and_its_route_ends_with_it() {
        let _flow = CONNECT_LOCK.lock().await;
        let _push = PUSH_LOCK.lock().await;
        let owner = "77777777-7777-4777-8777-77777777777c";
        let link = link_with_callback(free_port());
        let finished = Arc::new(AtomicBool::new(false));
        let (done, reply_url) = (finished.clone(), link.clone());
        let _stub = stub_owner(owner, move |method, params| match method {
            "github.connect" => Ok(json!({"jobId": "job:5001", "url": reply_url})),
            "operations.job.get" if done.load(Ordering::SeqCst) => {
                Ok(finished_owner_job(params["jobId"].as_str().unwrap()))
            }
            "operations.job.get" => Ok(json!({"jobId": params["jobId"], "state": "running"})),
            other => panic!("the controller must not send {other}"),
        });

        let started = call(MethodName::GitHubConnect, json!({"deviceId": owner}))
            .await
            .unwrap();
        // The client follows the owner's own job: no job of this engine stands in for it.
        assert_eq!(started["jobId"], "job:5001");
        assert!(callback_routes().get(owner).is_some());

        // The owner's job ends; the controller closes the route and acknowledges nothing:
        // the owner keeps the outcome until a client acknowledges it by (deviceId, jobId).
        finished.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(10);
        while callback_routes().get(owner).is_some() {
            assert!(Instant::now() < deadline, "the route outlived the sign-in");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
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

    /// Push tests share the saved push journal of the fixture's home.
    static PUSH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn push_journal() -> PathBuf {
        fixture();
        local_paths()
            .unwrap()
            .home
            .join("repository-push-operations.json")
    }

    fn start_params(operation: &str, branch: &str) -> Value {
        json!({
            "computerId": COMPUTER,
            "repositoryPath": "/work/repo",
            "operationId": operation,
            "target": {"repository": "owner/repo", "branch": branch, "commit": "0".repeat(40)},
        })
    }

    fn job_exists(job: &str) -> bool {
        let engine = fixture().context.clone();
        silo_engine::jobs::get_at(engine.as_ref(), &JobId::new(job.to_owned()).unwrap(), None)
            .unwrap()
            .is_some()
    }

    #[tokio::test]
    async fn push_start_refuses_a_start_that_was_never_prepared() {
        let _push = PUSH_LOCK.lock().await;
        let operation = "66666666-6666-4666-8666-666666666666";
        let error = expect_error(MethodName::PushStart, start_params(operation, "-bad")).await;
        assert_eq!(error.message, "Invalid branch for this push.");
        assert!(!job_exists(&format!("push:{operation}")));

        // A journal that cannot be read fails the start before any job exists.
        std::fs::create_dir_all(push_journal()).unwrap();
        let operation = "66666666-6666-4666-8666-666666666667";
        let error = expect_error(MethodName::PushStart, start_params(operation, "main")).await;
        std::fs::remove_dir_all(push_journal()).unwrap();
        assert!(
            error.message.contains("saved push operations"),
            "{}",
            error.message
        );
        assert!(!job_exists(&format!("push:{operation}")));
    }

    #[tokio::test]
    async fn push_start_returns_the_job_of_the_push_already_running() {
        let _push = PUSH_LOCK.lock().await;
        let running = "66666666-6666-4666-8666-666666666668";
        let requested = "66666666-6666-4666-8666-666666666669";
        let journal = json!({
            running: {
                "session": "an earlier session",
                "updated": 1,
                "dismissed": false,
                "operation": {
                    "operationId": running,
                    "computer": "dev",
                    "repositoryPath": "/work/repo",
                    "status": "pushing",
                    "commitCount": 1,
                    "target": {"repository": "owner/repo", "branch": "main", "commit": "0".repeat(40)},
                },
            },
        });
        std::fs::write(push_journal(), serde_json::to_vec(&journal).unwrap())
            .unwrap_or_else(|error| panic!("{error}: {}", push_journal().display()));
        let started = call(MethodName::PushStart, start_params(requested, "main")).await;
        std::fs::remove_file(push_journal()).unwrap();
        let started = started.unwrap();
        assert_eq!(started["jobId"], format!("push:{running}"));
        assert!(job_exists(&format!("push:{running}")));
        assert!(!job_exists(&format!("push:{requested}")));
    }

    #[tokio::test]
    async fn a_remote_push_start_names_the_owners_job_or_refuses() {
        let owner = "77777777-7777-4777-8777-77777777777a";
        let running = "66666666-6666-4666-8666-66666666666a";
        let requested = "66666666-6666-4666-8666-66666666666b";
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen = calls.clone();
        let _owner = stub_owner(owner, move |method, params| {
            seen.lock().unwrap().push(method.to_owned());
            match method {
                "repository.push.start" => Ok(json!({
                    "operationId": if params["operationId"] == requested { running } else { requested },
                    "status": "pushing", "computer": "dev", "repositoryPath": "/work/repo",
                    "commitCount": 0,
                })),
                // Only the push that is already running has a job on the owner.
                "operations.job.get" if params["jobId"] == format!("push:{running}") => {
                    Ok(json!({"state": "waiting"}))
                }
                "operations.job.get" => Err(owner_not_found()),
                other => panic!("unexpected owner request {other}"),
            }
        });
        let mut params = start_params(requested, "main");
        params["deviceId"] = json!(owner);
        let started = call(MethodName::PushStart, params.clone()).await.unwrap();
        assert_eq!(started["jobId"], format!("push:{running}"));

        // The owner could not prepare it: the reply is a failed operation without a job.
        let _refusing = stub_owner(owner, |method, _| match method {
            "repository.push.start" => Ok(json!({
                "operationId": "66666666-6666-4666-8666-66666666666c",
                "status": "failed", "message": "Invalid commit for this push.",
            })),
            _ => Err(owner_not_found()),
        });
        let error = expect_error(MethodName::PushStart, params).await;
        assert_eq!(error.message, "Invalid commit for this push.");
    }

    fn published_network_events(
        receiver: &mut tokio::sync::mpsc::Receiver<Event>,
    ) -> Vec<(String, Value)> {
        std::iter::from_fn(|| receiver.try_recv().ok())
            .filter(|event| event.name == "network.changed")
            .map(|event| (event.name, event.payload))
            .collect()
    }

    #[tokio::test]
    async fn another_devices_network_change_is_never_published_as_this_devices_state() {
        let owner = "77777777-7777-4777-8777-77777777777d";
        let engine = fixture().context.clone();
        let mut events = engine.subscribe(silo_engine::context::ClientId::new("network-events"));
        let owner_network = json!({"computers": [{
            "computer": "owner-box", "error": null, "ports": [],
        }]});
        let owner_ssh = json!({"computers": [{
            "computer": "owner-box", "computerId": COMPUTER, "enabled": false, "port": 2222,
            "bindAddress": "127.0.0.1", "keys": [], "state": "disabled", "message": null,
            "fingerprint": null, "deviceName": "Owner", "addresses": [], "user": "owner",
        }]});
        let (network, ssh) = (owner_network.clone(), owner_ssh.clone());
        let _stub = stub_owner(owner, move |method, _| match method {
            "network.publish" | "network.unpublish" => Ok(network.clone()),
            "ssh.access.save" => Ok(ssh.clone()),
            other => panic!("unexpected owner request {other}"),
        });

        // The caller gets the owner's state in the reply.
        let published = call(
            MethodName::NetworkPublish,
            json!({"computerId": COMPUTER, "deviceId": owner, "port": 8080}),
        )
        .await
        .unwrap();
        assert_eq!(published["computers"][0]["computer"], "owner-box");
        call(
            MethodName::NetworkUnpublish,
            json!({"computerId": COMPUTER, "deviceId": owner, "port": 8080}),
        )
        .await
        .unwrap();
        let saved = call(
            MethodName::SshAccessSave,
            json!({
                "computerId": COMPUTER, "deviceId": owner, "enabled": false, "port": 2222,
                "bindAddress": "127.0.0.1",
            }),
        )
        .await
        .unwrap();
        assert_eq!(saved["computers"][0]["deviceName"], "Owner");

        // But this engine's event stream carries only this engine's own state: neither
        // the owner's rows nor this device's network read as a consequence of its change.
        let leaked: Vec<_> = published_network_events(&mut events)
            .into_iter()
            .filter(|(_, payload)| payload.to_string().contains("owner-box"))
            .collect();
        assert!(leaked.is_empty(), "{leaked:?}");

        // State this engine owns is announced, with its own rows.
        let controller = canonical::<network::NetworkState>(
            json!({"computers": [{"computer": "controller-box", "error": null, "ports": []}]}),
            "network state",
        )
        .unwrap();
        announce_network_state(&engine, false, &controller);
        announce_network_state(&engine, true, &controller);
        let announced: Vec<_> = published_network_events(&mut events)
            .into_iter()
            .filter(|(_, payload)| payload.to_string().contains("controller-box"))
            .collect();
        assert_eq!(announced.len(), 1);
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
        // Turning Connections off rewrites the settings file other tests edit.
        let _key = KEY_LOCK.lock().await;
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
        // Removals rewrite the settings file; a save that overlaps one could be lost.
        let _key = KEY_LOCK.lock().await;
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
        use silo_engine::bridge_error::ErrorCode as Code;
        let engine = fixture().context.clone();
        let unknown = json!({"jobId": "job:424242"});
        // The codes are the ones `operations.*` answers on `engine.sock`, not text.
        for method in ["operations.job.get", "operations.cancel"] {
            let error = owner_job_operation(engine.as_ref(), method, &unknown).unwrap_err();
            assert_eq!(error.code, Code::NotFound, "{method}: {}", error.message);
        }
        // Acknowledging a job that is already gone is a successful retry.
        assert_eq!(
            owner_job_operation(engine.as_ref(), "operations.job.acknowledge", &unknown).unwrap(),
            Value::Null
        );
        assert_eq!(
            owner_job_operation(engine.as_ref(), "operations.job.get", &json!({}))
                .unwrap_err()
                .code,
            Code::Internal
        );
        let accepted = silo_engine::jobs::allocate(engine.as_ref(), "github.connect").unwrap();
        let params = json!({"jobId": accepted.job_id.as_str()});
        let found = owner_job_operation(engine.as_ref(), "operations.job.get", &params).unwrap();
        assert_eq!(found["jobId"], accepted.job_id.as_str());
        assert_ne!(found["state"], "finished");
        // A running job cannot be acknowledged yet.
        let running = owner_job_operation(engine.as_ref(), "operations.job.acknowledge", &params)
            .unwrap_err();
        assert_eq!(running.code, Code::Busy, "{}", running.message);
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
    async fn the_owners_codes_survive_the_local_access_adapter() {
        use silo_engine::{bridge_error::ErrorCode as Code, remote_access::LocalAccess};
        let _flow = CONNECT_LOCK.lock().await;
        fixture();
        let adapter = SilodLocalAccess(runtime_host().unwrap().clone());
        let unknown = json!({"jobId": "job:424243"});
        let error = adapter
            .access_dispatch("operations.cancel", &unknown)
            .unwrap_err();
        assert_eq!(error.code, Code::NotFound);
        // A sign-in already running on the owner is refused as busy, not as `internal`.
        let admission = ConnectAdmission::admit().unwrap();
        let error = adapter
            .access_dispatch("github.connect", &json!({}))
            .unwrap_err();
        assert_eq!(error.code, Code::Busy, "{}", error.message);
        drop(admission);
    }

    #[test]
    fn only_the_sign_in_the_engine_reports_as_cancelled_is_classified_as_cancelled() {
        assert_eq!(
            failure(silo_engine::github::CONNECTION_CANCELLED).code,
            ErrorCode::Cancelled
        );
        // Text that merely mentions cancellation is not a discriminator.
        assert_eq!(
            failure("The sign-in was not cancelled by anyone.").code,
            ErrorCode::Internal
        );
    }

    /// Counts the opens of an owner stream and never connects.
    struct UnreachableOwner(std::sync::atomic::AtomicUsize);

    impl crate::relay::OwnerLink for UnreachableOwner {
        fn open(
            &self,
            _device: &str,
            _selectors: &[String],
            _cancelled: &dyn Fn() -> bool,
        ) -> Result<crate::relay::OwnerStream, String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err("The owner is unreachable in this test.".to_owned())
        }
    }

    #[tokio::test]
    async fn events_subscribe_relays_saved_devices_only_and_ends_with_the_device() {
        use crate::dispatcher::{CoreMethodRegistry, MethodRegistry};
        let _key = KEY_LOCK.lock().await;
        let device = "77777777-7777-4777-8777-777777777771";
        save_device(device, "owner.example").await;
        let link = Arc::new(UnreachableOwner(Default::default()));
        let engine = Arc::new((*fixture().context).clone().with_relay_link(
            link.clone(),
            crate::relay::RelayTiming {
                tick: Duration::from_millis(5),
                stall: Duration::from_millis(100),
                backoff_initial: Duration::from_millis(5),
                backoff_max: Duration::from_millis(10),
                stable_after: Duration::from_secs(60),
            },
        ));
        let local = engine.local_device().device_id.clone();
        let client = silo_engine::context::ClientId::new("subscriber");
        let mut events = engine.connect_event_stream(client.clone());
        let registry = CoreMethodRegistry::default();
        let subscribe = |params: Value| {
            let request = RequestContext {
                client_id: "subscriber".to_owned(),
                protocol: 6,
                client: None,
            };
            registry.dispatch(MethodName::EventsSubscribe, params, engine.clone(), request)
        };

        // Without deviceIds, with this engine's own id, and with an empty list: no relay.
        for params in [
            json!({"events": ["computers.changed"]}),
            json!({"events": ["computers.changed"], "deviceIds": [local]}),
            json!({"events": ["computers.changed"], "deviceIds": []}),
        ] {
            subscribe(params).await.unwrap();
        }
        assert!(engine.relays().active().is_empty());

        // A device this engine is not connected to cannot be relayed.
        let error = subscribe(json!({"events": ["computers.changed"], "deviceIds": ["44444444-4444-4444-8444-444444444444"]}))
            .await
            .err()
            .expect("an unknown device is refused");
        assert_eq!(error.code, ErrorCode::NotFound);
        assert!(engine.relays().active().is_empty());

        // A saved device is relayed: its stream is tried (and retried while unreachable).
        let result =
            subscribe(json!({"events": ["computers.changed"], "deviceIds": [device, local]}))
                .await
                .unwrap();
        assert!(result.value["subscription"].is_string());
        let deadline = Instant::now() + Duration::from_secs(10);
        while link.0.load(Ordering::SeqCst) < 3 {
            assert!(Instant::now() < deadline, "the relay never retried");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(engine.relays().active(), [device]);

        // Removing the device ends the sessions that listened to it, and the relay stops.
        engine.event_bus().raise_gap_for_device(device);
        assert!(matches!(
            events.recv().await,
            Some(silo_engine::events::EventDelivery::Gap(_))
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !engine.relays().active().is_empty() {
            assert!(
                Instant::now() < deadline,
                "the relay outlived its subscribers"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        edit_connections_config(|config| {
            config["devices"]
                .as_array_mut()
                .unwrap()
                .retain(|saved| saved["id"] != device);
        });
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
    async fn removing_a_device_after_its_private_key_was_deleted_still_needs_the_owner() {
        let _key = KEY_LOCK.lock().await;
        let device = "99999999-9999-4999-8999-999999999999";
        // The owner holds the public key recorded for this device; the private key is gone.
        save_device(device, "not an address").await;
        let path = connections_config_path();
        edit_connections_config(|config| {
            config["controllerKeys"] =
                json!({device: ["ssh-ed25519 AAAAdeleted Silo remote management"]});
        });
        let directory = path.parent().unwrap().to_path_buf();
        let _ = std::fs::remove_file(directory.join("id_ed25519"));
        let _ = std::fs::remove_file(directory.join("id_ed25519.pub"));
        assert_eq!(silo_engine::remote::connections_public_key().unwrap(), None);
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
        // The owner could not confirm: the device stays saved, with the removal pending.
        assert!(saved_device_ids().iter().any(|id| id == device));
        let config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(config["removals"][0]["deviceId"], device);
        assert_eq!(config["removals"][0]["phase"], "revoke_pending");
        // Clean up for the other tests that share this engine home.
        config_without_device(device);
    }

    fn config_without_device(device: &str) {
        edit_connections_config(|config| {
            config["devices"]
                .as_array_mut()
                .unwrap()
                .retain(|saved| saved["id"] != device);
            config["removals"] = json!([]);
            config["controllerKeys"] = json!({});
        });
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
