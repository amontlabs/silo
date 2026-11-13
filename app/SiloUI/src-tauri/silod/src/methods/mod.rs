//! Per-domain request handlers for canonical `silo-api` methods.

mod ownership;

pub mod access;
pub mod approvals;
pub mod audit;
pub mod checkpoints;
pub mod computers;
pub mod display;
pub(crate) mod gate;
pub mod guest;
pub mod guest_client;
pub mod guest_remote;
pub mod operations;
pub mod remote;
pub mod services;

/// The macOS computer inventory is process-global. Tests that seed or reset it hold this
/// lock for their whole run, so one module's reset never removes another module's computer.
#[cfg(test)]
pub(crate) static MACOS_REGISTRY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

use std::{collections::BTreeSet, sync::Arc};

use serde::de::DeserializeOwned;
use serde_json::Value;
use silo_api::{
    jobs::{ErrorCode as ApiErrorCode, JobId},
    methods::{common::JobStartResult, MethodName},
};
use silo_engine::{
    context::{ClientId, EngineContext},
    events::EventBus,
    jobs::{JobRegistry, RegistryError, RegistryErrorKind},
};

use crate::{
    context::SilodContext,
    dispatcher::{self, DispatchError, HandlerOutput, RequestContext},
};

/// State and common operations available to every domain handler.
pub struct HandlerContext {
    engine: Arc<SilodContext>,
    client: RequestContext,
    client_id: ClientId,
    /// Set when the request is another device's, run here by `remote::serve_owner`.
    owner_bridge: bool,
    /// For a computer's call: that computer and the computers it made.
    visible: Option<Arc<BTreeSet<String>>>,
}

impl HandlerContext {
    pub(crate) fn new(engine: Arc<SilodContext>, client: RequestContext) -> Self {
        let client_id = ClientId::new(client.client_id.clone());
        Self {
            engine,
            client,
            client_id,
            owner_bridge: false,
            visible: None,
        }
    }

    /// The same request, limited to `visible` computers for a handler that cuts its own answer.
    fn with_visible(&self, visible: Option<Arc<BTreeSet<String>>>) -> Self {
        Self {
            engine: self.engine.clone(),
            client: self.client.clone(),
            client_id: self.client_id.clone(),
            owner_bridge: self.owner_bridge,
            visible,
        }
    }

    /// Who is calling.
    pub(crate) fn caller(&self) -> &silo_engine::policy::Caller {
        &self.client.caller
    }

    /// For a computer's call: that computer and the computers it made.
    pub(crate) fn visible(&self) -> Option<&BTreeSet<String>> {
        self.visible.as_deref()
    }

    /// A request another device's engine sent over Connections, served as a local one. Its
    /// client is the bridge itself, which no socket event reaches, so work that would answer
    /// only "the client that asked" is announced to every subscriber instead, and the
    /// controller reads it through its event relay.
    pub(crate) fn for_owner_bridge(engine: Arc<SilodContext>, client: RequestContext) -> Self {
        Self {
            owner_bridge: true,
            ..Self::new(engine, client)
        }
    }

    pub(crate) fn is_owner_bridge(&self) -> bool {
        self.owner_bridge
    }

    /// The engine interface implemented by silod's host context.
    pub fn engine_context(&self) -> &dyn EngineContext {
        self.engine.as_ref()
    }

    /// The silod-specific context for snapshots and socket event operations.
    pub fn silod_context(&self) -> &SilodContext {
        self.engine.as_ref()
    }

    /// The durable job registry, opened only when a handler needs it.
    pub fn job_registry(&self) -> Result<Arc<JobRegistry>, DispatchError> {
        self.engine_context()
            .job_registry()
            .map_err(DispatchError::internal)
    }

    /// The event bus used to publish sequenced wire events.
    pub fn event_bus(&self) -> &EventBus {
        self.engine.event_bus()
    }

    /// The negotiated client identity and metadata for this request.
    pub fn client_identity(&self) -> &RequestContext {
        &self.client
    }

    /// The event and audience identity used by engine event APIs.
    pub fn client_id(&self) -> &ClientId {
        &self.client_id
    }

    /// Decodes a request into the canonical parameter type from `silo-api`.
    pub fn decode_params<T: DeserializeOwned>(&self, params: &Value) -> Result<T, DispatchError> {
        serde_json::from_value(params.clone())
            .map_err(|error| DispatchError::internal(format!("Invalid params: {error}")))
    }

    /// Allocates a durable `job:` id and returns the API's canonical `{jobId}` result.
    pub fn start_job(&self, method: &str) -> Result<JobStartResult, DispatchError> {
        let accepted = silo_engine::jobs::allocate(self.engine_context(), method)
            .map_err(Self::map_registry_error)?;
        Ok(JobStartResult {
            job_id: accepted.job_id,
        })
    }

    /// Accepts a job whose identity is assigned by its domain, then returns `{jobId}`.
    pub fn start_job_with_id(
        &self,
        method: &str,
        job_id: JobId,
    ) -> Result<JobStartResult, DispatchError> {
        let (space, domain_id) = job_id
            .as_str()
            .split_once(':')
            .expect("JobId validation guarantees a space separator");
        let accepted =
            silo_engine::jobs::accept_domain_id(self.engine_context(), space, domain_id, method)
                .map_err(Self::map_registry_error)?;
        Ok(JobStartResult {
            job_id: accepted.job_id,
        })
    }

    /// Maps stable engine bridge errors onto the canonical API error catalogue.
    pub fn map_engine_error(error: silo_engine::bridge_error::BridgeError) -> DispatchError {
        use silo_engine::bridge_error::ErrorCode as EngineErrorCode;

        // The owner's own, finer code (`not_running`, `needs_personalizing`...) survives.
        if let Some(code) = error
            .api_code
            .as_deref()
            .and_then(|code| serde_json::from_value::<ApiErrorCode>(Value::from(code)).ok())
        {
            return DispatchError {
                code,
                message: error.message,
                details: None,
            };
        }
        let code = match error.code {
            EngineErrorCode::UpdateInProgress => ApiErrorCode::UpdateInProgress,
            EngineErrorCode::UnsupportedRemoteOperation => ApiErrorCode::UnsupportedRemoteOperation,
            EngineErrorCode::Cancelled => ApiErrorCode::Cancelled,
            EngineErrorCode::AlreadyQueued => ApiErrorCode::AlreadyQueued,
            EngineErrorCode::Busy => ApiErrorCode::Busy,
            EngineErrorCode::NotFound => ApiErrorCode::NotFound,
            EngineErrorCode::IncompatibleVersion => ApiErrorCode::IncompatibleVersion,
            EngineErrorCode::Internal => ApiErrorCode::Internal,
        };
        DispatchError {
            code,
            message: error.message,
            details: None,
        }
    }

    /// Maps durable registry failures using the dispatcher's existing error policy.
    pub fn map_registry_error(error: RegistryError) -> DispatchError {
        let message = error.to_string();
        let code = match error.kind() {
            RegistryErrorKind::NotCancellable | RegistryErrorKind::Conflict => {
                ApiErrorCode::Internal
            }
            RegistryErrorKind::Storage | RegistryErrorKind::Internal => {
                if message == "not_found" {
                    ApiErrorCode::NotFound
                } else if message.starts_with("busy:") || message.contains("cannot be acknowledged")
                {
                    ApiErrorCode::Busy
                } else {
                    ApiErrorCode::Internal
                }
            }
        };
        DispatchError {
            code,
            message,
            details: None,
        }
    }
}

/// Methods the engine answers itself, outside any domain module.
fn is_built_in(method: MethodName) -> bool {
    matches!(
        method,
        MethodName::EngineHandshake
            | MethodName::EngineState
            | MethodName::EventsSubscribe
            | MethodName::EventsUnsubscribe
            | MethodName::NotificationsList
            | MethodName::OperationsCancel
            | MethodName::OperationsJobAcknowledge
            | MethodName::OperationsJobGet
            | MethodName::OperationsList
            | MethodName::GuestSsh
    )
}

/// Owned by a domain module but answered with a typed refusal for every `engine.sock`
/// caller, so the handshake must not advertise them. `desktop.connect` is engine-to-engine
/// only: a peer reaches it through the owner-side bridge, never through this socket.
fn is_never_served(method: MethodName) -> bool {
    matches!(
        method,
        MethodName::MacosDisplayStream
            | MethodName::DisplayNativeAttach
            | MethodName::DesktopConnect
    )
}

/// Whether this engine serves the method: a built-in or one a domain router owns. The
/// handshake's capability list is derived from this, so it cannot drift from the routers.
pub fn serves(method: MethodName) -> bool {
    if is_never_served(method) {
        return false;
    }
    is_built_in(method)
        || computers::owns(method)
        || checkpoints::owns(method)
        || guest::owns(method)
        || guest_client::owns(method)
        || audit::owns(method)
        || approvals::owns(method)
        || access::owns(method)
        || display::owns(method)
        || services::owns(method)
}

/// The methods that can change a computer's state, which move its generation
/// (`silo_engine::generation`). Network and port changes are not state.
fn changes_computer(method: MethodName) -> bool {
    matches!(
        method,
        MethodName::GuestExec
            | MethodName::GuestExecStart
            | MethodName::GuestExecInput
            | MethodName::GuestFilesWrite
            | MethodName::GuestFilesRemove
            | MethodName::GuestPrepare
            | MethodName::FilesUpload
            | MethodName::ChangesKeep
            | MethodName::ChangesRequestsKeep
            | MethodName::ChangesPromote
            | MethodName::ChangesUndo
            | MethodName::CheckpointsRestore
            | MethodName::CheckpointsAbandonRestore
            | MethodName::ComputersStart
            | MethodName::ComputersStop
            | MethodName::ComputersRestart
            | MethodName::ComputersForceStop
            | MethodName::ComputersLaunch
            | MethodName::ComputersDelete
            | MethodName::SshAccessConnection
            | MethodName::TerminalCommand
            | MethodName::EditorCommand
            | MethodName::DesktopAction
            | MethodName::AgentsInstall
            | MethodName::DisplayViewerOpen
            | MethodName::DisplayShow
            | MethodName::MacosDisplayConnect
            | MethodName::MacosDisplaySession
            | MethodName::ClipboardWrite
            | MethodName::ClipboardTransfer
            | MethodName::ComputerUseSetup
            | MethodName::ComputerUseApproval
    )
}

/// The computer a state-changing request names. A keep or promote into the fork's default
/// target, and any request that names no computer, moves every computer's generation.
fn changed_computer(method: MethodName, params: &Value) -> Option<String> {
    if matches!(method, MethodName::ChangesKeep | MethodName::ChangesPromote) {
        return None;
    }
    params
        .get("computerId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

/// Marks the work a request does on its computer while its handler runs. A promote is not
/// marked: its job moves the generation itself once it holds its lanes, so the generation an
/// approval pinned for the fork still matches when the job checks it.
fn begin_work(
    method: MethodName,
    computer: Option<&str>,
) -> Option<silo_engine::generation::Touch> {
    (method != MethodName::ChangesPromote).then(|| silo_engine::generation::touch(computer))
}

/// Runs the request once the policy admits it (`gate`), moving the generation of the computer
/// it can change.
pub(crate) async fn dispatch(
    context: &HandlerContext,
    method: MethodName,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let admitted = gate::admit(context, method, params).await?;
    let scoped = context.with_visible(admitted.visible());
    let result = run(&scoped, method, admitted.params(params)).await;
    gate::finish(context, method, &admitted, result)
}

async fn run(
    context: &HandlerContext,
    method: MethodName,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    if !changes_computer(method) {
        return route(context, method, params).await;
    }
    let computer = changed_computer(method, params);
    let _touch = begin_work(method, computer.as_deref());
    let output = route(context, method, params).await?;
    if let Some(job) = output.value.get("jobId").and_then(Value::as_str) {
        silo_engine::generation::note_job(job, computer.as_deref());
    }
    Ok(output)
}

/// Tries domain handlers in stable order and returns the unsupported error if none handles it.
async fn route(
    context: &HandlerContext,
    method: MethodName,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    if let Some(result) = remote::route(method, params, context).await {
        return result;
    }
    if let Some(result) = operations::handle(method, params, context).await {
        return result;
    }
    if let Some(result) = computers::handle(method, params, context).await {
        return result;
    }
    if let Some(result) = checkpoints::handle(method, params, context).await {
        return result;
    }
    if let Some(result) = guest::handle(method, params, context).await {
        return result;
    }
    if let Some(result) = guest_client::handle(method, params, context).await {
        return result;
    }
    if let Some(result) = audit::handle(method, params, context).await {
        return result;
    }
    if let Some(answer) = approvals::handle(method, params, context) {
        return answer.await;
    }
    if let Some(result) = access::handle(method, params, context).await {
        return result;
    }
    if let Some(result) = display::handle(method, params, context).await {
        return result;
    }
    if let Some(result) = services::handle(method, params, context).await {
        return result;
    }
    Err(DispatchError::unsupported(dispatcher::method_name(method)))
}

#[cfg(test)]
mod tests {
    use silo_api::methods::{method_registry, MethodName};

    use super::{
        access, approvals, audit, changed_computer, changes_computer, checkpoints, computers,
        display, guest, guest_client, serves, services,
    };

    #[test]
    fn a_promote_approved_for_a_fork_passes_its_own_check_after_the_dispatcher_ran_it() {
        use silo_engine::changes::promote::service::{promote_guard, promote_pin};
        let fork = "dispatcher-pin-fork";
        let params = serde_json::json!({"forkId": fork});
        let guard = promote_guard(Some(promote_pin(fork)), fork.to_owned());
        // The dispatcher wraps the handler that starts the job, then the job checks.
        drop(super::begin_work(
            MethodName::ChangesPromote,
            changed_computer(MethodName::ChangesPromote, &params).as_deref(),
        ));
        assert_eq!(guard(), Ok(()));
        // Anything else that happened to the fork in between is seen.
        let guard = promote_guard(Some(promote_pin(fork)), fork.to_owned());
        drop(super::begin_work(MethodName::GuestExec, Some(fork)));
        assert!(guard().unwrap_err().contains("fork changed"));
        // So is a change no computer is named for.
        let guard = promote_guard(Some(promote_pin(fork)), fork.to_owned());
        drop(super::begin_work(MethodName::ChangesKeep, None));
        assert!(guard().is_err());
        // A promote nobody pinned runs whatever happened.
        assert_eq!(promote_guard(None, fork.to_owned())(), Ok(()));
    }

    #[test]
    fn commands_and_file_writes_change_a_computer_but_ports_do_not() {
        let params = serde_json::json!({"computerId": "c1"});
        assert!(changes_computer(MethodName::GuestExec));
        assert!(changes_computer(MethodName::GuestFilesWrite));
        assert!(changes_computer(MethodName::ComputersStart));
        assert!(changes_computer(MethodName::AgentsInstall));
        assert!(!changes_computer(MethodName::AgentsList));
        assert!(!changes_computer(MethodName::NetworkPublish));
        assert!(!changes_computer(MethodName::NetworkUnpublish));
        assert!(!changes_computer(MethodName::GuestFilesRead));
        assert_eq!(
            changed_computer(MethodName::GuestExec, &params).as_deref(),
            Some("c1")
        );
        // A keep into a default target cannot name the computer it changes.
        assert_eq!(changed_computer(MethodName::ChangesKeep, &params), None);
    }

    #[test]
    fn the_capability_set_is_every_routed_method_and_nothing_unserved() {
        let served: Vec<_> = method_registry()
            .iter()
            .filter(|descriptor| serves(descriptor.method))
            .map(|descriptor| descriptor.name)
            .collect();
        for name in [
            "engine.handshake",
            "guest.ssh",
            "computers.list",
            "computers.launch",
            "checkpoints.create",
            "exports.state",
            "migration.state",
            "display.show",
            "agents.list",
            "agents.install",
            "settings.read",
            "engine.shutdown",
        ] {
            assert!(served.contains(&name), "{name} must be advertised");
        }
        assert!(!served.contains(&"macos.display.stream"));
        assert!(!served.contains(&"display.native.attach"));
        assert!(!served.contains(&"desktop.connect"));
        // Everything in the catalogue is either served or deliberately withheld.
        let withheld: Vec<_> = method_registry()
            .iter()
            .filter(|descriptor| !serves(descriptor.method))
            .map(|descriptor| descriptor.name)
            .collect();
        assert_eq!(
            withheld,
            [
                "desktop.connect",
                "display.native.attach",
                "macos.display.stream"
            ]
        );
    }

    fn core_method(method: MethodName) -> bool {
        matches!(
            method,
            MethodName::EngineHandshake
                | MethodName::EngineState
                | MethodName::EventsSubscribe
                | MethodName::EventsUnsubscribe
                | MethodName::NotificationsList
                | MethodName::OperationsCancel
                | MethodName::OperationsJobAcknowledge
                | MethodName::OperationsJobGet
                | MethodName::OperationsList
        )
    }

    #[test]
    fn every_domain_method_routes_to_exactly_one_domain_module() {
        let domains = [
            computers::owns as fn(MethodName) -> bool,
            checkpoints::owns,
            guest::owns,
            guest_client::owns,
            audit::owns,
            approvals::owns,
            access::owns,
            display::owns,
            services::owns,
        ];

        for descriptor in method_registry() {
            let domain_owners = domains
                .iter()
                .filter(|owns| owns(descriptor.method))
                .count();
            if core_method(descriptor.method) {
                assert_eq!(
                    domain_owners, 0,
                    "{} is built-in and must not overlap a domain module",
                    descriptor.name
                );
            } else {
                assert_eq!(
                    domain_owners, 1,
                    "{} must route to exactly one domain module",
                    descriptor.name
                );
            }
        }
    }
}

/// Tests of different modules share the process-wide operation gate, whose job ids repeat in
/// every engine context (`job:1`). A test that starts or holds gate work takes this lease for
/// its whole fixture lifetime so another module's jobs are neither confused with nor queued
/// behind its own. (A flag and a condition variable rather than a `MutexGuard`, so a fixture
/// holding it stays `Send`.)
#[cfg(test)]
pub(crate) fn process_gate_lock() -> ProcessGateLease {
    let (held, released) = &*PROCESS_GATE;
    let mut held = held.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    while *held {
        held = released
            .wait(held)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
    *held = true;
    ProcessGateLease(())
}

#[cfg(test)]
static PROCESS_GATE: std::sync::LazyLock<(std::sync::Mutex<bool>, std::sync::Condvar)> =
    std::sync::LazyLock::new(Default::default);

#[cfg(test)]
pub(crate) struct ProcessGateLease(());

#[cfg(test)]
impl Drop for ProcessGateLease {
    fn drop(&mut self) {
        let (held, released) = &*PROCESS_GATE;
        *held.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = false;
        released.notify_one();
    }
}
