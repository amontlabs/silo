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
    transport::{ApprovalKind, ApprovalRequiredDetails},
};
use silo_engine::{
    audit::{AuditLog, Row},
    context::EngineContext,
    policy::{
        self,
        store::{Document, JobState, Pending, PolicyStore},
        Access, Caller, Creation, Decision, Filter, Resources,
    },
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
        approval_id: record
            .error
            .and_then(|error| error.details.as_ref())
            .and_then(|details| details.approval_id.clone()),
    });
}

/// Turns a call the owner must approve into the answer its caller gets. No approval is stored
/// yet, so the answer carries no approval id.
fn request_approval(
    _context: &HandlerContext,
    _method: MethodName,
    _params: &Value,
    kind: ApprovalKind,
    summary: String,
) -> DispatchError {
    DispatchError {
        code: ErrorCode::ApprovalRequired,
        message: format!("This needs the approval of the person who runs Silo: {summary}"),
        details: Some(ApprovalRequiredDetails {
            kind,
            summary,
            approval_id: None,
        }),
    }
}

/// Asks the policy about one call. A refused or held call is recorded and answered here.
pub(crate) async fn admit(
    context: &HandlerContext,
    method: MethodName,
    params: &Value,
) -> Result<Admitted, DispatchError> {
    let caller = context.client_identity().caller.clone();
    let target = policy::target_computer(params);
    let (decision, visible) = match &caller {
        Caller::Computer { id } => {
            let world = EngineWorld::load(context, method).await;
            let mut visible: BTreeSet<String> = policy::World::made(&world, id).into_iter().collect();
            visible.insert(id.clone());
            (
                policy::decide(&caller, method, params, &world),
                Some(Arc::new(visible)),
            )
        }
        _ => (policy::decide(&caller, method, params, &NoWorld), None),
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
            },
        );
        Err(error)
    };
    match decision {
        Decision::Allow(allowed) => {
            if let (Some(creation), Caller::Computer { id }) = (&allowed.creates, &caller) {
                // The computer is the caller's once its job has finished well (`settle`).
                let begun = PolicyStore::new(&app_data(context)).begin(
                    id,
                    Pending {
                        created_by: id.clone(),
                        name: creation.name.clone(),
                        id: creation.id.clone(),
                        source: creation.source.clone(),
                        job_id: None,
                        at: 0,
                    },
                );
                if let Err(message) = begun {
                    return refuse(
                        DispatchError::new(ErrorCode::PermissionDenied, message),
                        AuditOutcome::Refused,
                    );
                }
            }
            let job = (method == MethodName::OperationsJobAcknowledge)
                .then(|| params.get("jobId").and_then(Value::as_str).map(str::to_owned))
                .flatten();
            Ok(Admitted {
                params: allowed.params,
                filter: allowed.filter,
                visible: visible.or(allowed.visible.map(Arc::new)),
                creates: allowed.creates,
                target,
                kind: kind_of(method),
                job,
            })
        }
        Decision::Refuse { reason } => refuse(
            DispatchError::new(ErrorCode::PermissionDenied, reason),
            AuditOutcome::Refused,
        ),
        Decision::Ask { kind, summary } => refuse(
            request_approval(context, method, params, kind, summary),
            AuditOutcome::Asked,
        ),
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
    if let Caller::Computer { id } = &context.client_identity().caller {
        let store = PolicyStore::new(&app_data(context));
        let mut noted = Vec::new();
        if let Some(creation) = &admitted.creates {
            noted.push(match (&result, &job) {
                (Ok(output), _) if output.value.pointer("/computer/id").is_some() => {
                    let made = output.value.pointer("/computer/id").and_then(Value::as_str);
                    store.confirm(id, &creation.name, made.unwrap_or_default())
                }
                (Ok(_), Some(job)) => store.attach_job(id, &creation.name, job),
                (Ok(_), None) => Ok(()),
                (Err(_), _) => store.withdraw(id, &creation.name),
            });
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
}

impl EngineWorld {
    async fn load(context: &HandlerContext, method: MethodName) -> Self {
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
                node.get("parent").and_then(Value::as_str).map(str::to_owned),
            );
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
        if method == MethodName::ComputersCreate {
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
        let mut ssh = HashMap::new();
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
                }
            }
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
        }
    }
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
/// name whose parent is the fork's source.
fn resolve(
    names: &HashMap<String, String>,
    parents: &HashMap<String, Option<String>>,
    pending: &Pending,
) -> Option<String> {
    if let Some(id) = &pending.id {
        return names.contains_key(id).then(|| id.clone());
    }
    names
        .iter()
        .filter(|(_, name)| name.eq_ignore_ascii_case(&pending.name))
        .map(|(id, _)| id)
        .find(|id| match &pending.source {
            Some(source) => parents.get(*id).cloned().flatten().as_deref() == Some(source),
            None => true,
        })
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
    fn access(&self, computer: &str) -> Access {
        self.names
            .get(computer)
            .map(|name| policy::access::of_computer(name))
            .unwrap_or_default()
    }
    fn github_policies(&self) -> Option<Vec<Value>> {
        self.github.clone()
    }
    fn new_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

#[cfg(test)]
mod tests;
