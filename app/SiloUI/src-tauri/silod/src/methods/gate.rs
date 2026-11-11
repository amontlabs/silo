//! The one place a request passes before its handler runs, and again after. [`admit`] asks
//! the policy (`silo_engine::policy`) whether the caller may make the call, records what a
//! computer's agent asked for, and refuses or holds the call; [`finish`] cuts the answer to
//! what the caller may see, records the jobs and computers a computer's agent started, and
//! writes the audit log. No handler is reached any other way, so none can forget either.

use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    sync::{Arc, LazyLock},
};

use serde_json::{json, Value};
use silo_api::{
    jobs::{ErrorCode, JobId, JobState as ApiJobState},
    methods::{audit::AuditOutcome, method_registry, MethodKind, MethodName},
};
use silo_engine::{
    audit::{AuditLog, Row},
    context::EngineContext,
    policy::{
        self,
        store::{Document, JobState, Pending, PolicyStore},
        Access, Caller, Creation, Decision, Filter, Resources,
    },
    resources,
};

use super::HandlerContext;
use crate::dispatcher::{DispatchError, HandlerOutput, RequestContext};

static KINDS: LazyLock<HashMap<MethodName, MethodKind>> = LazyLock::new(|| {
    method_registry()
        .iter()
        .map(|descriptor| (descriptor.method, descriptor.kind))
        .collect()
});

fn kind_of(method: MethodName) -> MethodKind {
    KINDS.get(&method).copied().unwrap_or(MethodKind::Command)
}

/// A call the policy let through, and what to do with its answer.
pub(crate) struct Admitted {
    params: Option<Value>,
    filter: Filter,
    /// The caller and the computers it made, for a computer's call.
    visible: Option<Arc<BTreeSet<String>>>,
    creates: Option<Creation>,
    target: Option<String>,
    kind: MethodKind,
    /// The job an acknowledgement names.
    job: Option<String>,
    pub(crate) reservation_id: Option<u64>,
}

impl Admitted {
    /// The parameters to run: the caller's, or the policy's rewrite of them.
    pub(crate) fn params<'a>(&'a self, original: &'a Value) -> &'a Value {
        self.params.as_ref().unwrap_or(original)
    }

    pub(crate) fn visible(&self) -> Option<Arc<BTreeSet<String>>> {
        self.visible.clone()
    }
}

fn app_data(context: &HandlerContext) -> PathBuf {
    context.silod_context().paths().app_data.clone()
}

pub(crate) fn error_code(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "internal".into())
}

struct Record<'a> {
    method: MethodName,
    target: Option<String>,
    /// The computers a computer's agent may reach: a row names a target only inside this set.
    visible: Option<Arc<BTreeSet<String>>>,
    outcome: AuditOutcome,
    error: Option<&'a DispatchError>,
    job_id: Option<String>,
    /// The approval the call ran under.
    approval_id: Option<String>,
}

fn audit(context: &HandlerContext, record: Record<'_>) {
    let app_data = app_data(context);
    // An engine always has an absolute data folder; a test context without one keeps no log.
    if !app_data.is_absolute() {
        return;
    }
    let request = context.client_identity();
    let caller = &request.caller;
    let client = match caller {
        Caller::Owner { client } => Some(client.clone()),
        _ => request.client.as_ref().map(|client| client.name.clone()),
    };
    AuditLog::shared(&app_data).record(Row {
        at_ms: silo_engine::audit::now_ms(),
        caller_kind: caller.kind(),
        caller_id: caller.id().map(str::to_owned),
        client,
        method: record.method.name().to_owned(),
        // A computer's call names no computer outside the ones it made, so the log tells
        // nothing a refused probe guessed.
        computer_id: match (&caller, &record.visible) {
            (Caller::Computer { .. }, Some(visible)) => {
                record.target.filter(|target| visible.contains(target))
            }
            (Caller::Computer { .. }, None) => None,
            _ => record.target,
        },
        outcome: record.outcome,
        error_code: record.error.map(|error| error_code(error.code)),
        job_id: record.job_id,
        approval_id: record.approval_id.or_else(|| {
            record
                .error
                .and_then(|error| error.details.as_ref())
                .and_then(|details| details.approval_id.clone())
        }),
    });
}

/// Records a call the owner allowed, run by the engine under the approval `approval_id`.
pub(crate) fn record_approved(
    context: &HandlerContext,
    method: MethodName,
    params: &Value,
    asker: &str,
    result: &Result<HandlerOutput, DispatchError>,
    approval_id: &str,
) {
    let (outcome, error) = match result {
        Ok(_) => (AuditOutcome::Ok, None),
        Err(error) => (AuditOutcome::Error, Some(error)),
    };
    let job_id = result
        .as_ref()
        .ok()
        .and_then(|output| output.value.get("jobId"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    audit(
        context,
        Record {
            method,
            // A call that names no computer is recorded against the computer that asked.
            target: policy::target_computer(params).or_else(|| Some(asker.to_owned())),
            outcome,
            error,
            job_id,
            approval_id: Some(approval_id.to_owned()),
            visible: None,
        },
    );
}

/// Notes a computer `creator` asked for, recorded as its own once its job has finished well.
pub(crate) fn begin_creation(
    app_data: &std::path::Path,
    creator: &str,
    creation: &Creation,
) -> Result<(), String> {
    PolicyStore::new(app_data).begin(
        creator,
        Pending {
            created_by: creator.to_owned(),
            name: creation.name.clone(),
            id: creation.id.clone(),
            source: creation.source.clone(),
            job_id: None,
            at: 0,
        },
    )
}

/// Records what became of a computer `creator` asked for, from the call's answer.
pub(crate) fn settle_creation(
    store: &PolicyStore,
    creator: &str,
    creation: &Creation,
    result: &Result<HandlerOutput, DispatchError>,
    job: Option<&str>,
) -> Result<(), String> {
    match (result, job) {
        (Ok(output), _) if output.value.pointer("/computer/id").is_some() => {
            let made = output.value.pointer("/computer/id").and_then(Value::as_str);
            store.confirm(creator, &creation.name, made.unwrap_or_default())
        }
        (Ok(_), Some(job)) => store.attach_job(creator, &creation.name, job),
        (Ok(_), None) => Ok(()),
        (Err(_), _) => store.withdraw(creator, &creation.name),
    }
}

/// A computer's name, when it exists.
pub(crate) async fn computer_name(context: &HandlerContext, id: &str) -> Option<String> {
    let world = EngineWorld::load(context, MethodName::ComputersAccess, None).await;
    policy::World::name(&world, id)
}

/// Computers a test adds to the device the policy sees, where no runtime lists any.
#[cfg(test)]
pub(crate) mod test_nodes {
    use std::sync::Mutex;

    type Node = (String, String, Option<String>);
    static NODES: Mutex<Vec<Node>> = Mutex::new(Vec::new());

    fn nodes() -> std::sync::MutexGuard<'static, Vec<Node>> {
        NODES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn add(id: &str, name: &str, parent: Option<&str>) {
        nodes().push((id.into(), name.into(), parent.map(str::to_owned)));
    }

    pub(crate) fn remove(id: &str) {
        nodes().retain(|(node, _, _)| node != id);
    }

    pub(crate) fn all() -> Vec<Node> {
        nodes().clone()
    }
}

#[cfg(test)]
pub(crate) mod test_resources {
    use std::sync::Mutex;
    use super::Resources;

    static RESOURCES: Mutex<Vec<(String, Resources)>> = Mutex::new(Vec::new());

    fn res() -> std::sync::MutexGuard<'static, Vec<(String, Resources)>> {
        RESOURCES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn add(id: &str, resources: Resources) {
        res().push((id.into(), resources));
    }

    pub(crate) fn clear() {
        res().clear();
    }

    pub(crate) fn all() -> Vec<(String, Resources)> {
        res().clone()
    }
}

/// The device now and the floors that apply to an agent inside a computer.
pub(crate) fn host_and_floors(
    context: &HandlerContext,
) -> Result<(resources::Host, resources::Floors), DispatchError> {
    let host = resources::measure(&app_data(context)).ok_or_else(|| {
        DispatchError::internal("This device's memory and disk could not be measured.")
    })?;
    // Settings that cannot be read leave the default floors, never none.
    let (memory, disk) = silo_engine::settings::resource_floors_for(context.engine_context())
        .unwrap_or((None, None));
    let floors = resources::Floors::resolve(&host, memory, disk);
    Ok((host, floors))
}

/// A call that starts, creates or checkpoints a computer, which takes memory or disk.
pub(crate) fn takes_headroom(method: MethodName) -> bool {
    matches!(
        method,
        MethodName::ComputersStart
            | MethodName::ComputersRestart
            | MethodName::ComputersCreate
            | MethodName::CheckpointsFork
            | MethodName::CheckpointsCreate
            | MethodName::CheckpointsRestore
            | MethodName::MacosSetupRetry
    )
}

#[derive(Clone, Debug)]
struct Reservation {
    id: u64,
    computer_id: Option<String>,
    job_id: Option<String>,
    memory_bytes: u64,
    created_at: std::time::Instant,
}

static RESERVATIONS: std::sync::Mutex<Vec<Reservation>> = std::sync::Mutex::new(Vec::new());
static NEXT_RESERVATION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn current_reserved_memory(engine: &dyn EngineContext) -> u64 {
    let mut reservations = RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reservations.retain(|r| {
        if r.created_at.elapsed() > std::time::Duration::from_secs(300) {
            return false;
        }
        if let Some(job) = &r.job_id {
            match job_state(engine, job) {
                JobState::Succeeded | JobState::Failed | JobState::Unknown => false,
                JobState::Running => true,
            }
        } else {
            true
        }
    });
    reservations.iter().map(|r| r.memory_bytes).sum()
}

fn reserve_memory(memory_bytes: u64, computer_id: Option<String>) -> u64 {
    let id = NEXT_RESERVATION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut reservations = RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reservations.push(Reservation {
        id,
        computer_id,
        job_id: None,
        memory_bytes,
        created_at: std::time::Instant::now(),
    });
    id
}

fn attach_reservation_job(id: u64, job_id: &str) {
    let mut reservations = RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(r) = reservations.iter_mut().find(|r| r.id == id) {
        r.job_id = Some(job_id.to_owned());
    }
}

fn release_reservation(id: u64) {
    let mut reservations = RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reservations.retain(|r| r.id != id);
}

#[cfg(test)]
pub(crate) fn clear_reservations() {
    let mut reservations = RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reservations.clear();
}

fn requested_memory_bytes(
    method: MethodName,
    params: &Value,
    resources: &HashMap<String, Resources>,
) -> u64 {
    if !takes_headroom(method) {
        return 0;
    }
    let explicit_gib = params
        .pointer("/configuration/memoryGiB")
        .or_else(|| params.pointer("/request/memoryGiB"))
        .or_else(|| params.get("memoryGiB"))
        .and_then(Value::as_u64);
    if let Some(gib) = explicit_gib {
        return gib.saturating_mul(silo_engine::resources::GIB);
    }
    let id = params
        .get("computerId")
        .or_else(|| params.get("id"))
        .and_then(Value::as_str);
    if let Some(id) = id {
        if let Some(res) = resources.get(id) {
            return res.memory_gib.saturating_mul(silo_engine::resources::GIB);
        }
    }
    0
}

/// Why an agent's call that takes memory or disk must fail now: the device has less free than
/// the floor, the requested memory exceeds remaining headroom, or the device cannot be measured so nobody can tell.
pub(crate) fn short_of_resources(
    context: &HandlerContext,
    method: MethodName,
    params: &Value,
    resources: &HashMap<String, Resources>,
) -> Option<String> {
    match host_and_floors(context) {
        Ok((host, floors)) => {
            let reserved = current_reserved_memory(context.engine_context());
            let requested = requested_memory_bytes(method, params, resources);
            resources::refusal_for(&host, &floors, reserved, requested)
        }
        Err(_) => Some(
            "Silo cannot measure this device's memory and disk, so it cannot tell whether another computer fits. Ask the person who runs Silo."
                .to_owned(),
        ),
    }
}

/// Asks the policy again about a call `computer` made, as at the time it asked, and returns
/// its answer with the computers `computer` may reach (itself and the ones it made). `None`
/// when that computer no longer exists.
pub(crate) async fn judge(
    context: &HandlerContext,
    computer: &str,
    method: MethodName,
    params: &Value,
) -> Option<(Decision, BTreeSet<String>)> {
    let world = EngineWorld::load(context, method, Some((computer, params))).await;
    policy::World::name(&world, computer)?;
    let caller = Caller::Computer {
        id: computer.to_owned(),
    };
    let mut tree: BTreeSet<String> = policy::World::made(&world, computer).into_iter().collect();
    tree.insert(computer.to_owned());
    Some((policy::decide(&caller, method, params, &world), tree))
}

/// Asks the policy about one call. A refused or held call is recorded and answered here.
pub(crate) async fn admit(
    context: &HandlerContext,
    method: MethodName,
    params: &Value,
) -> Result<Admitted, DispatchError> {
    let caller = context.client_identity().caller.clone();
    let target = policy::target_computer(params);
    let (decision, visible, name, world_resources) = match &caller {
        Caller::Computer { id } => {
            let world = EngineWorld::load(context, method, Some((id, params))).await;
            let mut visible: BTreeSet<String> =
                policy::World::made(&world, id).into_iter().collect();
            visible.insert(id.clone());
            (
                policy::decide(&caller, method, params, &world),
                Some(Arc::new(visible)),
                policy::World::name(&world, id),
                world.resources,
            )
        }
        _ => (
            policy::decide(&caller, method, params, &NoWorld),
            None,
            None,
            HashMap::new(),
        ),
    };
    let seen = visible.clone();
    let refuse = |error: DispatchError, outcome: AuditOutcome| {
        audit(
            context,
            Record {
                method,
                target: target.clone(),
                visible: seen.clone(),
                outcome,
                error: Some(&error),
                job_id: None,
                approval_id: None,
            },
        );
        Err(error)
    };
    // A call that would be held for approval is refused too: allowing it later would start a
    // computer on a device that has nothing left for it.
    if matches!(caller, Caller::Computer { .. })
        && takes_headroom(method)
        && matches!(decision, Decision::Allow(_) | Decision::Ask { .. })
    {
        if let Some(message) = short_of_resources(context, method, params, &world_resources) {
            return refuse(
                DispatchError::new(ErrorCode::ResourcesLow, message),
                AuditOutcome::Refused,
            );
        }
    }
    // A computer may have only so many jobs it has not acknowledged: a job nobody follows still
    // takes a place in the engine's job admission.
    if let Caller::Computer { id } = &caller {
        if kind_of(method) == MethodKind::Job && matches!(decision, Decision::Allow(_)) {
            let open = PolicyStore::new(&app_data(context))
                .snapshot()
                .map(|document| document.jobs_of(id).len());
            if open.map_or(true, |open| open >= silo_engine::guest_client::MAX_JOBS) {
                return refuse(
                    DispatchError::new(
                        ErrorCode::PermissionDenied,
                        format!(
                            "{} jobs of this computer are not finished and acknowledged yet.",
                            silo_engine::guest_client::MAX_JOBS
                        ),
                    ),
                    AuditOutcome::Refused,
                );
            }
        }
    }
    match decision {
        Decision::Allow(allowed) => {
            if let (Some(creation), Caller::Computer { id }) = (&allowed.creates, &caller) {
                // The computer is the caller's once its job has finished well (`settle`).
                let begun = begin_creation(&app_data(context), id, creation);
                if let Err(message) = begun {
                    return refuse(
                        DispatchError::new(ErrorCode::PermissionDenied, message),
                        AuditOutcome::Refused,
                    );
                }
            }
            let job = (method == MethodName::OperationsJobAcknowledge)
                .then(|| {
                    params
                        .get("jobId")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .flatten();
            let reservation_id = if matches!(caller, Caller::Computer { .. })
                && takes_headroom(method)
            {
                let requested = requested_memory_bytes(method, params, &world_resources);
                if requested > 0 {
                    Some(reserve_memory(requested, target.clone()))
                } else {
                    None
                }
            } else {
                None
            };
            Ok(Admitted {
                params: allowed.params,
                filter: allowed.filter,
                visible: visible.or(allowed.visible.map(Arc::new)),
                creates: allowed.creates,
                target,
                kind: kind_of(method),
                job,
                reservation_id,
            })
        }
        Decision::Refuse { reason } => refuse(
            DispatchError::new(ErrorCode::PermissionDenied, reason),
            AuditOutcome::Refused,
        ),
        Decision::Ask {
            kind,
            summary,
            params: rewritten,
            creates,
        } => {
            let Caller::Computer { id } = &caller else {
                return refuse(
                    DispatchError::new(ErrorCode::PermissionDenied, summary),
                    AuditOutcome::Refused,
                );
            };
            let asked = super::approvals::Asked {
                computer: id,
                tree: visible.as_deref().cloned().unwrap_or_default(),
                computer_name: name.unwrap_or_else(|| id.clone()),
                method,
                asked: params,
                params: rewritten.unwrap_or_else(|| params.clone()),
                kind,
                summary,
                creates,
            };
            match super::approvals::request(context, asked).await {
                Ok(error) => refuse(error, AuditOutcome::Asked),
                Err(error) => refuse(error, AuditOutcome::Refused),
            }
        }
    }
}

/// Completes an admitted call: records what a computer's agent started, cuts the answer for a
/// computer, and writes the audit row of a call that can change something.
pub(crate) fn finish(
    context: &HandlerContext,
    method: MethodName,
    admitted: &Admitted,
    mut result: Result<HandlerOutput, DispatchError>,
) -> Result<HandlerOutput, DispatchError> {
    let job = result
        .as_ref()
        .ok()
        .and_then(|output| output.value.get("jobId"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(res_id) = admitted.reservation_id {
        if let Some(job_id) = &job {
            attach_reservation_job(res_id, job_id);
        } else {
            release_reservation(res_id);
        }
    }
    if let Caller::Computer { id } = &context.client_identity().caller {
        let store = PolicyStore::new(&app_data(context));
        let mut noted = Vec::new();
        if let Some(creation) = &admitted.creates {
            noted.push(settle_creation(
                &store,
                id,
                creation,
                &result,
                job.as_deref(),
            ));
        }
        if let Some(job) = &job {
            noted.push(store.note_job_about(id, job, admitted.target.as_deref()));
        }
        if let (Ok(_), Some(done)) = (&result, &admitted.job) {
            noted.push(store.drop_job(id, done));
        }
        for error in noted.into_iter().filter_map(Result::err) {
            eprintln!("silod: computer {id}: {error}");
        }
        if let (Ok(output), Some(visible)) = (&mut result, &admitted.visible) {
            policy::filter_result(admitted.filter, &mut output.value, visible);
            policy::cut_for_computer(&mut output.value, visible);
        }
    }
    if matches!(
        admitted.kind,
        MethodKind::Command | MethodKind::Job | MethodKind::Stream
    ) {
        let (outcome, error) = match &result {
            Ok(_) => (AuditOutcome::Ok, None),
            Err(error) => (AuditOutcome::Error, Some(error)),
        };
        audit(
            context,
            Record {
                method,
                target: admitted.target.clone(),
                visible: admitted.visible.clone(),
                outcome,
                error,
                job_id: job,
                approval_id: None,
            },
        );
    }
    result
}

/// The policy's view for a caller it never asks about the device.
struct NoWorld;

impl policy::World for NoWorld {
    fn made(&self, _creator: &str) -> std::collections::HashSet<String> {
        Default::default()
    }
    fn parent(&self, _computer: &str) -> Option<String> {
        None
    }
    fn name(&self, _computer: &str) -> Option<String> {
        None
    }
    fn owns_job(&self, _caller: &str, _job_id: &str) -> bool {
        false
    }
    fn request_fork(&self, _request_id: &str) -> Option<String> {
        None
    }
    fn request_target(&self, _request_id: &str) -> Option<String> {
        None
    }
    fn resources(&self, _computer: &str) -> Option<Resources> {
        None
    }
    fn ssh_settings(&self, _computer: &str) -> Option<(u16, String)> {
        None
    }
    fn access(&self, _computer: &str) -> Access {
        Access::default()
    }
    fn github_policies(&self) -> Option<Vec<Value>> {
        None
    }
    fn new_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

/// The device as the engine sees it now: its computers, who made which, and the jobs
/// computers started.
struct EngineWorld {
    names: HashMap<String, String>,
    parents: HashMap<String, Option<String>>,
    document: Document,
    app_data: PathBuf,
    github: Option<Vec<Value>>,
    /// Every computer's sizes, read only for a call that makes a computer.
    resources: HashMap<String, Resources>,
    /// The saved SSH port and bind address of each computer, read only for `sshAccess.save`.
    ssh: HashMap<String, (u16, String)>,
    /// The saved SSH public keys of each computer, read with the above.
    ssh_keys: HashMap<String, Vec<String>>,
    /// The network policy of the caller and of every computer the call names, read only for a
    /// call that compares network policies. A computer whose policy could not be read is absent.
    network: HashMap<String, silo_engine::network_policy::Held>,
}

impl EngineWorld {
    async fn load(
        context: &HandlerContext,
        method: MethodName,
        call: Option<(&str, &Value)>,
    ) -> Self {
        let internal = HandlerContext::for_owner_bridge(
            context.engine.clone(),
            RequestContext::owner("policy"),
        );
        let tree = super::route(&internal, MethodName::ComputersTree, &json!({}))
            .await
            .map(|output| output.value)
            .unwrap_or(Value::Null);
        let mut names = HashMap::new();
        let mut parents = HashMap::new();
        for node in tree
            .get("nodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = node.get("id").and_then(Value::as_str) else {
                continue;
            };
            if let Some(name) = node.get("name").and_then(Value::as_str) {
                names.insert(id.to_owned(), name.to_owned());
            }
            parents.insert(
                id.to_owned(),
                node.get("parent")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            );
        }
        #[cfg(test)]
        for (id, name, parent) in test_nodes::all() {
            names.insert(id.clone(), name);
            parents.insert(id, parent);
        }
        let github = if method == MethodName::GitHubConfigurationSave {
            super::route(&internal, MethodName::GitHubState, &json!({}))
                .await
                .ok()
                .and_then(|output| output.value.get("computers").cloned())
                .and_then(|computers| computers.as_array().cloned())
        } else {
            None
        };
        let mut resources = HashMap::new();
        if method == MethodName::ComputersCreate || takes_headroom(method) {
            let list = super::route(&internal, MethodName::ComputersList, &json!({}))
                .await
                .map(|output| output.value)
                .unwrap_or(Value::Null);
            let computers = |kind: &str| {
                list.pointer(&format!("/{kind}/computers"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
            };
            for computer in computers("linux") {
                let configuration = &computer["configuration"];
                let id = configuration.get("id").and_then(Value::as_str);
                if let (Some(id), Some(sizes)) = (id, Resources::of_linux(configuration)) {
                    resources.insert(id.to_owned(), sizes);
                }
            }
            for computer in computers("macos") {
                let id = computer.get("id").and_then(Value::as_str);
                if let (Some(id), Some(sizes)) = (id, Resources::of_macos(&computer)) {
                    resources.insert(id.to_owned(), sizes);
                }
            }
        }
        #[cfg(test)]
        for (id, r) in test_resources::all() {
            resources.insert(id, r);
        }
        let mut ssh = HashMap::new();
        let mut ssh_keys = HashMap::new();
        if method == MethodName::SshAccessSave {
            let state = super::route(&internal, MethodName::SshAccessState, &json!({}))
                .await
                .map(|output| output.value)
                .unwrap_or(Value::Null);
            for computer in state
                .get("computers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let saved = (
                    computer
                        .get("port")
                        .and_then(Value::as_u64)
                        .and_then(|port| u16::try_from(port).ok()),
                    computer.get("bindAddress").and_then(Value::as_str),
                    computer.get("computerId").and_then(Value::as_str),
                );
                if let (Some(port), Some(address), Some(id)) = saved {
                    ssh.insert(id.to_owned(), (port, address.to_owned()));
                    let keys = computer
                        .get("keys")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect();
                    ssh_keys.insert(id.to_owned(), keys);
                }
            }
        }
        let mut network = HashMap::new();
        if let (true, Some((caller, params))) = (compares_networks(method), call) {
            let mut ids = vec![caller.to_owned()];
            ids.extend(
                params
                    .as_object()
                    .into_iter()
                    .flat_map(|object| object.values())
                    .filter_map(Value::as_str)
                    .filter(|value| names.contains_key(*value))
                    .map(str::to_owned),
            );
            ids.sort();
            ids.dedup();
            network = tokio::task::spawn_blocking(move || {
                ids.into_iter()
                    .filter_map(|id| {
                        super::access::judged_network_policy(&id).map(|held| (id, held))
                    })
                    .collect::<HashMap<_, _>>()
            })
            .await
            .unwrap_or_default();
        }
        let app_data = app_data(context);
        let engine = context.engine.clone();
        let settled = {
            let (names, parents, app_data) = (names.clone(), parents.clone(), app_data.clone());
            tokio::task::spawn_blocking(move || {
                let job = |job: &str| job_state(engine.as_ref(), job);
                let resolve = |pending: &Pending| resolve(&names, &parents, pending);
                PolicyStore::new(&app_data).settle(&job, &resolve)
            })
            .await
        };
        let document = match settled {
            Ok(Ok(document)) => document,
            Ok(Err(error)) => {
                eprintln!("silod: {error}");
                Document::default()
            }
            Err(_) => Document::default(),
        };
        Self {
            names,
            parents,
            document,
            app_data,
            github,
            resources,
            ssh,
            ssh_keys,
            network,
        }
    }
}

/// Whether a call's decision compares network policies.
fn compares_networks(method: MethodName) -> bool {
    matches!(
        method,
        MethodName::ComputersCreate | MethodName::CheckpointsFork | MethodName::NetworkPolicySet
    )
}

fn job_state(engine: &dyn EngineContext, job: &str) -> JobState {
    let Ok(job) = JobId::new(job) else {
        return JobState::Unknown;
    };
    match silo_engine::jobs::get_at(engine, &job, None) {
        Ok(Some(found)) if found.state == ApiJobState::Finished => {
            let failed = found
                .outcome
                .as_ref()
                .is_some_and(|outcome| outcome.error.is_some());
            if failed {
                JobState::Failed
            } else {
                JobState::Succeeded
            }
        }
        Ok(Some(_)) => JobState::Running,
        Ok(None) | Err(_) => JobState::Unknown,
    }
}

/// The id of a computer that was pending: the one the engine chose, or the computer of that
/// name whose parent is the fork's source. A name alone proves nothing, since another
/// computer may already have it, so a pending computer with neither is never matched; the
/// call that made it answers with its id.
fn resolve(
    names: &HashMap<String, String>,
    parents: &HashMap<String, Option<String>>,
    pending: &Pending,
) -> Option<String> {
    if let Some(id) = &pending.id {
        return names.contains_key(id).then(|| id.clone());
    }
    let source = pending.source.as_ref()?;
    names
        .iter()
        .filter(|(_, name)| name.eq_ignore_ascii_case(&pending.name))
        .map(|(id, _)| id)
        .find(|id| parents.get(*id).cloned().flatten().as_ref() == Some(source))
        .cloned()
}

impl policy::World for EngineWorld {
    fn made(&self, creator: &str) -> std::collections::HashSet<String> {
        self.document.made_by(creator)
    }
    fn parent(&self, computer: &str) -> Option<String> {
        self.parents.get(computer).cloned().flatten()
    }
    fn name(&self, computer: &str) -> Option<String> {
        self.names.get(computer).cloned()
    }
    fn owns_job(&self, caller: &str, job_id: &str) -> bool {
        self.document.owns_job(caller, job_id)
    }
    fn request_fork(&self, request_id: &str) -> Option<String> {
        silo_engine::fold_requests::Store::new(&self.app_data)
            .get(request_id)
            .ok()
            .flatten()
            .map(|request| request.fork_id)
    }
    fn request_target(&self, request_id: &str) -> Option<String> {
        silo_engine::fold_requests::Store::new(&self.app_data)
            .get(request_id)
            .ok()
            .flatten()
            .map(|request| request.target)
    }
    fn resources(&self, computer: &str) -> Option<Resources> {
        self.resources.get(computer).copied()
    }
    fn ssh_settings(&self, computer: &str) -> Option<(u16, String)> {
        self.ssh.get(computer).cloned()
    }
    fn ssh_keys(&self, computer: &str) -> Vec<String> {
        self.ssh_keys.get(computer).cloned().unwrap_or_default()
    }
    fn network_held(&self, computer: &str) -> Option<silo_engine::network_policy::Held> {
        self.network.get(computer).cloned()
    }
    fn access(&self, computer: &str) -> Access {
        self.names
            .get(computer)
            .map(|name| policy::access::of_computer(name))
            .unwrap_or_default()
    }
    fn github_policies(&self) -> Option<Vec<Value>> {
        self.github.clone()
    }
    fn github_token_name(&self, id: &str) -> Option<String> {
        silo_engine::secrets::github_token_name(id)
    }
    fn new_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

#[cfg(test)]
mod tests;
