//! `approvals.*` and `computers.access`. A call the policy holds for the owner becomes an
//! approval here ([`request`]); `approvals.decide` answers it, and on allow the engine checks
//! the stored call again and runs it as the owner ([`execute`]). The store is
//! `silo_engine::approvals`; a merge is shown and kept through the review-request machinery
//! (`silo_engine::fold_requests`, `changes.requests.*`).

use std::{collections::BTreeSet, future::Future, pin::Pin};

use serde_json::{json, Value};
use silo_api::{
    jobs::ErrorCode,
    methods::{
        approvals::{
            Approval, ApprovalDecision, ApprovalIdParams, ApprovalOutcome, ApprovalState,
            ApprovalsDecideParams, ApprovalsListParams, ApprovalsListResult, ApprovalsNoteParams,
            ComputerAccess, ComputerAccessSsh, ComputersAccessParams,
        },
        keep::{ChangesKeepParams, FoldRequestStatus},
        network::NetworkState,
        ssh_access::SshAccessState,
        MethodName,
    },
    transport::{ApprovalKind, ApprovalRequiredDetails},
};
use silo_engine::{
    approvals::{self, AskError, NewApproval, Record, Store},
    context::EngineContext,
    fold_requests,
    policy::{self, store::PolicyStore, Caller, Decision},
};

use super::{gate, ownership::owns_prefix, HandlerContext};
use crate::dispatcher::{DispatchError, HandlerOutput, RequestContext};

/// The longest note the owner gives with an answer.
const DECISION_NOTE_MAX_CHARS: usize = 2000;

pub(super) fn owns(method: MethodName) -> bool {
    owns_prefix(method, &["approvals."]) || method == MethodName::ComputersAccess
}

type Answer<'a> = Pin<Box<dyn Future<Output = Result<HandlerOutput, DispatchError>> + Send + 'a>>;

pub(super) fn handle<'a>(
    method: MethodName,
    params: &'a Value,
    context: &'a HandlerContext,
) -> Option<Answer<'a>> {
    owns(method).then_some(())?;
    // Boxed: allowing a call runs it through the router that reaches this handler.
    Some(Box::pin(async move {
        match method {
            MethodName::ApprovalsList => list(context, params).await,
            MethodName::ApprovalsGet => get(context, params).await,
            MethodName::ApprovalsDecide => decide(context, params).await,
            MethodName::ApprovalsCancel => cancel(context, params).await,
            MethodName::ApprovalsNote => note(context, params).await,
            MethodName::ComputersAccess => access(context, params).await,
            other => Err(DispatchError::unsupported(other.name())),
        }
    }))
}

fn app_data(context: &HandlerContext) -> std::path::PathBuf {
    context.silod_context().paths().app_data.clone()
}

fn store(context: &HandlerContext) -> Store {
    Store::new(&app_data(context))
}

fn output(value: impl serde::Serialize) -> Result<HandlerOutput, DispatchError> {
    serde_json::to_value(value)
        .map(|value| HandlerOutput {
            value,
            event_barrier: None,
        })
        .map_err(|error| DispatchError::internal(error.to_string()))
}

fn local_only(context: &HandlerContext, device_id: Option<&str>) -> Result<(), DispatchError> {
    match device_id {
        Some(device)
            if device != "local"
                && device != context.silod_context().local_device().device_id =>
        {
            Err(DispatchError::new(
                ErrorCode::UnsupportedRemoteOperation,
                "Approvals are answered on the device whose computer asked.",
            ))
        }
        _ => Ok(()),
    }
}

/// The engine itself, reading the device as the owner.
fn internal(context: &HandlerContext) -> HandlerContext {
    HandlerContext::for_owner_bridge(context.engine.clone(), RequestContext::owner("approvals"))
}

/// The computers whose approvals the caller sees: every one for the owner; for a computer,
/// itself and the computers it made.
fn seen(context: &HandlerContext) -> Option<BTreeSet<String>> {
    match context.caller() {
        Caller::Owner { .. } => None,
        _ => Some(context.visible().cloned().unwrap_or_default()),
    }
}

fn may_see(seen: &Option<BTreeSet<String>>, record: &Record) -> bool {
    seen.as_ref()
        .is_none_or(|seen| seen.contains(record.computer()))
}

fn announce(context: &HandlerContext, record: &Record) {
    approvals::announce(context.silod_context(), record);
}

fn not_found(id: &str) -> DispatchError {
    DispatchError::new(ErrorCode::NotFound, format!("There is no approval {id}."))
}

/// The approval `id` as the caller may see it.
fn visible_record(context: &HandlerContext, id: &str) -> Result<Record, DispatchError> {
    let found = store(context)
        .get(id)
        .map_err(|message| DispatchError::new(ErrorCode::NotFound, message))?
        .ok_or_else(|| not_found(id))?;
    if !may_see(&seen(context), &found) {
        return Err(not_found(id));
    }
    Ok(found)
}

// MARK: Asking

/// A call the policy holds for the owner.
pub(crate) struct Asked<'a> {
    pub computer: &'a str,
    pub computer_name: String,
    pub method: MethodName,
    /// The parameters as the computer sent them.
    pub asked: &'a Value,
    /// The parameters that run on allow.
    pub params: Value,
    pub kind: ApprovalKind,
    pub summary: String,
    pub creates: Option<policy::Creation>,
}

fn approval_required(record: &Record) -> DispatchError {
    let approval = &record.approval;
    DispatchError {
        code: ErrorCode::ApprovalRequired,
        message: format!(
            "This needs the approval of the person who runs Silo: {} The request is {}; read its answer with approvals.get.",
            approval.summary, approval.id
        ),
        details: Some(ApprovalRequiredDetails {
            kind: approval.kind,
            summary: approval.summary.clone(),
            approval_id: Some(approval.id.clone()),
        }),
    }
}

fn refused(message: impl Into<String>) -> DispatchError {
    DispatchError::new(ErrorCode::PermissionDenied, message)
}

/// The review request a merge approval shows its changes with, and the digest it is pinned to.
async fn review_request(
    context: &HandlerContext,
    asked: &Asked<'_>,
) -> Result<Option<(String, String)>, DispatchError> {
    match asked.method {
        MethodName::ChangesKeep => {
            let keep: ChangesKeepParams = serde_json::from_value(asked.params.clone())
                .map_err(|error| refused(format!("Invalid params: {error}")))?;
            if keep.dry_run == Some(true) {
                return Err(refused(
                    "A dry run of a merge into this computer is not offered; changes.diff shows what the merge would bring.",
                ));
            }
            let digest = keep
                .review_digest
                .filter(|digest| !digest.is_empty())
                .ok_or_else(|| {
                    refused(format!(
                        "Run the diff first: changes.diff on {} gives a reviewDigest. Pass it as reviewDigest, so the person who runs Silo sees exactly those changes.",
                        keep.fork_id
                    ))
                })?;
            let mut request = json!({
                "forkId": keep.fork_id,
                "reviewDigest": digest,
                "summary": asked.summary,
            });
            if let Some(target) = keep.target_id.filter(|target| !target.is_empty()) {
                request["targetId"] = json!(target);
            }
            let made = super::route(&internal(context), MethodName::ChangesRequest, &request)
                .await
                .map_err(|error| refused(error.message))?;
            let id = made
                .value
                .pointer("/request/id")
                .and_then(Value::as_str)
                .ok_or_else(|| DispatchError::internal("The review request was not recorded."))?;
            Ok(Some((id.to_owned(), digest)))
        }
        MethodName::ChangesRequestsKeep => {
            let id = asked
                .params
                .get("requestId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let request = fold_requests::Store::new(&app_data(context))
                .open_request(&id)
                .map_err(refused)?;
            fold_requests::check_keepable(&request).map_err(refused)?;
            Ok(Some((request.id, request.review_digest)))
        }
        _ => Ok(None),
    }
}

/// Stores the approval for a held call, or finds the one the same computer already asked for
/// the same call. `Ok` is the `approval_required` answer; `Err` refuses the call.
pub(crate) async fn request(
    context: &HandlerContext,
    asked: Asked<'_>,
) -> Result<DispatchError, DispatchError> {
    let store = store(context);
    let key = approvals::request_key(asked.method.name(), asked.asked);
    if let Some(same) = store.find_pending(asked.computer, &key) {
        return Ok(approval_required(&same));
    }
    let linked = review_request(context, &asked).await?;
    let made = store.ask(NewApproval {
        kind: asked.kind,
        computer_id: asked.computer.to_owned(),
        computer_name: asked.computer_name.clone(),
        method: asked.method.name().to_owned(),
        params: asked.params.clone(),
        asked: asked.asked.clone(),
        summary: asked.summary.clone(),
        creates: asked.creates.as_ref().map(|creation| approvals::Creation {
            name: creation.name.clone(),
            id: creation.id.clone(),
            source: creation.source.clone(),
        }),
        review_request_id: linked.as_ref().map(|(id, _)| id.clone()),
        review_digest: linked.as_ref().map(|(_, digest)| digest.clone()),
    });
    let (record, created) = match made {
        Ok(made) => made,
        Err(AskError::Limit) => {
            return Err(refused(format!(
                "{} already has {} requests waiting for an answer. Wait for an answer, or withdraw one with approvals.cancel, before asking again.",
                asked.computer_name,
                approvals::PENDING_LIMIT
            )))
        }
        Err(AskError::Store(message)) => return Err(DispatchError::internal(message)),
    };
    if created {
        if let Some((request_id, _)) = &linked {
            if let Err(message) =
                fold_requests::Store::new(&app_data(context)).link_approval(request_id, record.id())
            {
                eprintln!("silod: approval {}: {message}", record.id());
            }
            // A merge asked again with other changes replaces the earlier ask.
            for older in store.pending_for_request(request_id) {
                if older.id() == record.id() {
                    continue;
                }
                if let Ok(cancelled) = store.cancel(
                    older.id(),
                    None,
                    "The merge was asked for again with other changes.",
                ) {
                    announce(context, &cancelled);
                }
            }
        }
        announce(context, &record);
    }
    Ok(approval_required(&record))
}

// MARK: Reading

/// Closes pending merge approvals whose review request was answered or closed outside them.
fn settle_links(context: &HandlerContext, records: &mut [Record]) {
    let requests = fold_requests::Store::new(&app_data(context));
    let store = store(context);
    for record in records.iter_mut() {
        if record.state() != ApprovalState::Pending {
            continue;
        }
        let Some(request_id) = record.approval.review_request_id.clone() else {
            continue;
        };
        let status = requests
            .get(&request_id)
            .ok()
            .flatten()
            .map(|request| request.status);
        let changed = match status {
            Some(FoldRequestStatus::Open | FoldRequestStatus::Keeping) => continue,
            Some(FoldRequestStatus::Kept) => {
                store.answered_outside(record.id(), true, "review request", None)
            }
            Some(FoldRequestStatus::ThrownAway) | None => store.cancel(
                record.id(),
                None,
                "Its review request was closed: the fork was thrown away or a computer was deleted.",
            ),
        };
        if let Ok(changed) = changed {
            announce(context, &changed);
            *record = changed;
        }
    }
}

async fn list(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let params: ApprovalsListParams = context.decode_params(params)?;
    local_only(context, params.device_id.as_deref())?;
    let (mut records, expired) = store(context).list();
    for record in &expired {
        announce(context, record);
    }
    settle_links(context, &mut records);
    let seen = seen(context);
    let approvals: Vec<Approval> = records
        .iter()
        .filter(|record| may_see(&seen, record))
        .filter(|record| params.state.is_none_or(|state| record.state() == state))
        .filter(|record| {
            params
                .computer_id
                .as_deref()
                .is_none_or(|computer| record.computer() == computer)
        })
        .map(Record::view)
        .collect();
    output(ApprovalsListResult { approvals })
}

async fn get(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let params: ApprovalIdParams = context.decode_params(params)?;
    local_only(context, params.device_id.as_deref())?;
    for record in store(context).sweep(approvals::now_ms()) {
        announce(context, &record);
    }
    let mut record = [visible_record(context, &params.id)?];
    settle_links(context, &mut record);
    let [record] = record;
    output(record.view())
}

// MARK: Answering

fn bounded_note(note: Option<String>, max: usize) -> Result<Option<String>, DispatchError> {
    let note = note
        .map(|note| note.trim().to_owned())
        .filter(|note| !note.is_empty());
    if note.as_ref().is_some_and(|note| note.chars().count() > max) {
        return Err(DispatchError::new(
            ErrorCode::PermissionDenied,
            format!("A note is at most {max} characters."),
        ));
    }
    Ok(note)
}

/// The error for an approval that cannot change now.
fn unchangeable(message: String) -> DispatchError {
    let code = if message.starts_with("There is no approval") || message.contains("not an approval id") {
        ErrorCode::NotFound
    } else {
        ErrorCode::Changed
    };
    DispatchError::new(code, message)
}

async fn decide(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let Caller::Owner { client } = context.caller() else {
        return Err(refused(
            "Only the person who runs Silo answers an approval.",
        ));
    };
    let by = client.clone();
    let params: ApprovalsDecideParams = context.decode_params(params)?;
    local_only(context, params.device_id.as_deref())?;
    let note = bounded_note(params.note, DECISION_NOTE_MAX_CHARS)?;
    let store = store(context);
    let found = visible_record(context, &params.id)?;
    match params.decision {
        ApprovalDecision::Deny => {
            let denied = store
                .deny(found.id(), &by, note.as_deref())
                .map_err(unchangeable)?;
            announce(context, &denied);
            output(denied.view())
        }
        ApprovalDecision::Allow => {
            let allowed = store
                .allow(found.id(), &by, note.as_deref())
                .map_err(unchangeable)?;
            announce(context, &allowed);
            let outcome = execute(context, &allowed).await;
            let finished = store
                .finish(allowed.id(), outcome)
                .map_err(DispatchError::internal)?;
            announce(context, &finished);
            output(finished.view())
        }
    }
}

fn failed(message: impl Into<String>) -> ApprovalOutcome {
    ApprovalOutcome {
        result: None,
        job_id: None,
        error: Some(message.into()),
    }
}

/// The `changes.requests.keep` an allowed merge runs, after checking that its review request
/// still shows the changes the owner was asked about.
fn merge_call(context: &HandlerContext, record: &Record) -> Result<(MethodName, Value), String> {
    let approval = &record.approval;
    let Some(request_id) = &approval.review_request_id else {
        return Ok((
            MethodName::from_name(&approval.method).ok_or("The stored call is unknown.")?,
            approval.params.clone(),
        ));
    };
    let request = fold_requests::Store::new(&app_data(context))
        .get(request_id)?
        .ok_or("Its review request no longer exists, so nothing was merged.")?;
    if !matches!(request.status, FoldRequestStatus::Open) {
        return Err("Its review request was already answered or closed, so nothing was merged.".into());
    }
    if Some(&request.review_digest) != approval.review_digest.as_ref() {
        return Err(
            "The review request was asked for again with other changes after you were asked, so nothing was merged. The computer can ask again."
                .into(),
        );
    }
    let mut keep = json!({ "requestId": request_id });
    for key in ["choices", "skip", "keep", "dataOk"] {
        if let Some(value) = approval.params.get(key).filter(|value| !value.is_null()) {
            keep[key] = value.clone();
        }
    }
    Ok((MethodName::ChangesRequestsKeep, keep))
}

/// Checks an allowed call again and runs it as the owner. Never runs a call the check
/// refuses; the outcome says why in plain words.
fn execute<'a>(
    context: &'a HandlerContext,
    record: &'a Record,
) -> Pin<Box<dyn Future<Output = ApprovalOutcome> + Send + 'a>> {
    Box::pin(async move {
        let approval = &record.approval;
        let asker = record.computer();
        let Some(method) = MethodName::from_name(&approval.method) else {
            return failed("The stored call is not one this engine knows, so nothing was run.");
        };
        match gate::judge(context, asker, method, &approval.params).await {
            None => {
                return failed(format!(
                    "{} no longer exists, so nothing was run.",
                    approval.computer_name
                ))
            }
            Some(Decision::Refuse { reason }) => {
                return failed(format!("Nothing was run: {reason}"));
            }
            Some(Decision::Ask { kind, .. }) if kind != approval.kind => {
                return failed(
                    "Nothing was run: the call would now ask for something other than what you allowed.",
                );
            }
            Some(_) => {}
        }
        let (run_method, run_params) = match merge_call(context, record) {
            Ok(call) => call,
            Err(message) => return failed(message),
        };
        let app_data = app_data(context);
        let creation = record.creates.as_ref().map(|creation| policy::Creation {
            name: creation.name.clone(),
            id: creation.id.clone(),
            source: creation.source.clone(),
        });
        if let Some(creation) = &creation {
            if let Err(message) = gate::begin_creation(&app_data, asker, creation) {
                return failed(format!("Nothing was run: {message}"));
            }
        }
        let result = super::run(context, run_method, &run_params).await;
        let job = result
            .as_ref()
            .ok()
            .and_then(|output| output.value.get("jobId"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let policy_store = PolicyStore::new(&app_data);
        let mut noted = Vec::new();
        if let Some(creation) = &creation {
            noted.push(gate::settle_creation(
                &policy_store,
                asker,
                creation,
                &result,
                job.as_deref(),
            ));
        }
        // The computer that asked reads the job as one it started.
        if let Some(job) = &job {
            noted.push(policy_store.note_job(asker, job));
        }
        for error in noted.into_iter().filter_map(Result::err) {
            eprintln!("silod: approval {}: {error}", record.id());
        }
        gate::record_approved(context, run_method, &run_params, &result, record.id());
        match result {
            Ok(_) if job.is_some() => ApprovalOutcome {
                result: None,
                job_id: job,
                error: None,
            },
            Ok(output) => ApprovalOutcome {
                result: Some(output.value),
                job_id: None,
                error: None,
            },
            Err(error) => failed(error.message),
        }
    })
}

async fn cancel(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let params: ApprovalIdParams = context.decode_params(params)?;
    local_only(context, params.device_id.as_deref())?;
    let found = visible_record(context, &params.id)?;
    let (by, reason) = match context.caller() {
        Caller::Owner { .. } => (None, "Withdrawn by the person who runs Silo."),
        Caller::Computer { id } => (Some(id.as_str()), "Withdrawn by the computer that asked."),
        Caller::Device { .. } => return Err(refused("Another device cannot withdraw it.")),
    };
    let cancelled = store(context)
        .cancel(found.id(), by, reason)
        .map_err(unchangeable)?;
    announce(context, &cancelled);
    output(cancelled.view())
}

async fn note(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let Caller::Computer { id: caller } = context.caller() else {
        return Err(refused(
            "Only the computer that asked writes its note; your own note goes with approvals.decide.",
        ));
    };
    let params: ApprovalsNoteParams = context.decode_params(params)?;
    local_only(context, params.device_id.as_deref())?;
    let found = visible_record(context, &params.id)?;
    let noted = store(context)
        .note(found.id(), caller, &params.note)
        .map_err(unchangeable)?;
    announce(context, &noted);
    output(noted.view())
}

// MARK: What a computer holds

async fn access(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let params: ComputersAccessParams = context.decode_params(params)?;
    local_only(context, params.device_id.as_deref())?;
    let id = params.computer_id;
    if seen(context).is_some_and(|seen| !seen.contains(&id)) {
        return Err(refused("That computer is not this one or one it made."));
    }
    let reader = internal(context);
    let tree = super::route(&reader, MethodName::ComputersTree, &json!({}))
        .await
        .map(|output| output.value)
        .unwrap_or(Value::Null);
    let name = tree
        .get("nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|node| node.get("id").and_then(Value::as_str) == Some(id.as_str()))
        .and_then(|node| node.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .ok_or_else(|| {
            DispatchError::new(ErrorCode::NotFound, format!("There is no computer {id}."))
        })?;
    let held_name = name.clone();
    let (secrets, github) = tokio::task::spawn_blocking(move || {
        policy::access::held(&held_name)
    })
    .await
    .map_err(|_| DispatchError::internal("The computer's access could not be read."))?;
    let ports = super::route(&reader, MethodName::NetworkState, &json!({}))
        .await
        .ok()
        .and_then(|output| serde_json::from_value::<NetworkState>(output.value).ok())
        .and_then(|state| {
            state.computers.into_iter().find(|computer| {
                computer.computer_id.as_deref() == Some(id.as_str())
                    || (computer.computer_id.is_none() && computer.computer == name)
            })
        })
        .map(|computer| computer.ports)
        .unwrap_or_default();
    let ssh = super::route(&reader, MethodName::SshAccessState, &json!({}))
        .await
        .ok()
        .and_then(|output| serde_json::from_value::<SshAccessState>(output.value).ok())
        .and_then(|state| {
            state
                .computers
                .into_iter()
                .find(|computer| computer.computer_id == id)
        })
        .map(|computer| ComputerAccessSsh {
            enabled: computer.enabled,
            port: computer.port,
            state: serde_json::to_value(computer.state)
                .ok()
                .and_then(|state| state.as_str().map(str::to_owned))
                .unwrap_or_default(),
        });
    output(ComputerAccess {
        computer_id: id,
        name,
        secrets,
        github,
        ports,
        ssh,
    })
}

// MARK: Review requests answered from the app

/// A review request kept or thrown away directly closes the approval that wraps it: kept is
/// allowed with the keep's job, thrown away by the owner is denied, by a computer cancelled.
pub(crate) fn request_answered(
    context: &HandlerContext,
    request_id: &str,
    kept_job: Option<&str>,
) {
    let store = store(context);
    let by = match context.caller() {
        Caller::Owner { client } => Some(client.clone()),
        _ => None,
    };
    for record in store.pending_for_request(request_id) {
        let changed = match (kept_job, &by) {
            (Some(job), Some(by)) => {
                let changed = store.answered_outside(
                    record.id(),
                    true,
                    by,
                    Some(ApprovalOutcome {
                        result: None,
                        job_id: Some(job.to_owned()),
                        error: None,
                    }),
                );
                if changed.is_ok() {
                    if let Err(error) =
                        PolicyStore::new(&app_data(context)).note_job(record.computer(), job)
                    {
                        eprintln!("silod: approval {}: {error}", record.id());
                    }
                }
                changed
            }
            (None, Some(by)) => store.answered_outside(record.id(), false, by, None),
            (_, None) => store.cancel(
                record.id(),
                None,
                "Its review request was answered by a computer's agent.",
            ),
        };
        if let Ok(changed) = changed {
            announce(context, &changed);
        }
    }
}

/// At engine start: an allowed call whose outcome was never stored is ended, not run again.
pub(crate) fn recover(context: &crate::context::SilodContext) {
    let store = Store::new(&context.paths().app_data);
    for record in store.recover() {
        approvals::announce(context, &record);
    }
    for record in store.sweep(approvals::now_ms()) {
        approvals::announce(context, &record);
    }
}

#[cfg(test)]
mod tests;
