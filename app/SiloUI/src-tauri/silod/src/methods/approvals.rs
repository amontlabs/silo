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
        keep::{ChangesKeepParams, FoldRequest, FoldRequestStatus},
        network::{NetworkPolicyState, NetworkState},
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

/// The approval as the caller may read it: a computer's own summary leaves out what only the
/// owner may see.
fn view_for(context: &HandlerContext, record: &Record) -> Approval {
    match context.caller() {
        Caller::Computer { .. } => record.view_for_computer(),
        _ => record.view(),
    }
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
    /// The computer and the computers it made.
    pub tree: BTreeSet<String>,
    pub computer_name: String,
    pub method: MethodName,
    /// The parameters as the computer sent them.
    pub asked: &'a Value,
    /// The parameters that run on allow.
    pub params: Value,
    pub kind: ApprovalKind,
    pub summary: String,
    /// What the computer may read of `summary`, when it differs.
    pub computer_summary: Option<String>,
    pub creates: Option<policy::Creation>,
}

fn approval_required(record: &Record) -> DispatchError {
    let approval = &record.approval;
    DispatchError {
        code: ErrorCode::ApprovalRequired,
        message: format!(
            "This needs the approval of the person who runs Silo: {} The request is {}; read its answer with `silo approvals status {}`.",
            record.summary_for_computer(), approval.id, approval.id
        ),
        details: Some(ApprovalRequiredDetails {
            kind: approval.kind,
            summary: record.summary_for_computer().to_owned(),
            approval_id: Some(approval.id.clone()),
        }),
    }
}

/// A computer's name as a message of Silo's quotes it.
fn shown_name(name: &str) -> String {
    format!("“{}”", silo_api::plain_text_within(name, 100))
}

/// What a call acts on as it is now, so an allowed call is refused when it moved after the
/// ask: the last merge or replacement an undo would take back, or the state of the fork a
/// promote would put in place.
fn pin_of(context: &HandlerContext, method: MethodName, params: &Value) -> Option<String> {
    match method {
        MethodName::ChangesUndo => {
            let computer = params.get("computerId").and_then(Value::as_str)?;
            Some(silo_engine::changes::promote::service::undo_pin(
                context.engine_context(),
                computer,
            ))
        }
        MethodName::ChangesPromote => {
            let fork = params.get("forkId").and_then(Value::as_str)?;
            Some(silo_engine::changes::promote::service::promote_pin(fork))
        }
        _ => None,
    }
}

/// Why an allowed call that acts on `method`'s pinned state does not run.
fn pin_moved_message(method: MethodName) -> &'static str {
    match method {
        MethodName::ChangesUndo => {
            "Nothing was undone: another merge or replacement happened after you were asked. The computer can ask again."
        }
        _ => {
            "Nothing was replaced: the fork changed after you were asked. The computer can ask again."
        }
    }
}

fn refused(message: impl Into<String>) -> DispatchError {
    DispatchError::new(ErrorCode::PermissionDenied, message)
}

/// The review request a merge approval shows its changes with, and the digest it is pinned to.
/// The number of changes in each bucket of a reviewed diff.
type Counts = std::collections::BTreeMap<String, u64>;

/// The changes of a fork the engine counts from the captures its last diff stored, when they
/// still give `digest`; what the computer wrote about them is never used.
async fn engine_counts(context: &HandlerContext, fork: &str, digest: &str) -> Counts {
    let engine = context.silod_context().clone();
    let (fork, digest) = (fork.to_owned(), digest.to_owned());
    tokio::task::spawn_blocking(move || {
        let store = silo_engine::changes::store::Store::for_engine(&engine);
        silo_engine::changes::service::review_counts(&store, &fork, &digest)
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_default()
}

async fn review_request(
    context: &HandlerContext,
    asked: &Asked<'_>,
) -> Result<Option<(String, String, Counts)>, DispatchError> {
    match asked.method {
        MethodName::ChangesKeep => {
            let keep: ChangesKeepParams = serde_json::from_value(asked.params.clone())
                .map_err(|error| refused(format!("Invalid params: {error}")))?;
            if keep.dry_run == Some(true) {
                return Err(refused(
                    "A dry run of a merge into this computer is not offered; `silo diff` shows what the merge would bring.",
                ));
            }
            let digest = keep
                .review_digest
                .filter(|digest| !digest.is_empty())
                .ok_or_else(|| {
                    refused(format!(
                        "Run the diff first: `silo diff {}` gives a review digest. Merge with that digest, so the person who runs Silo sees exactly those changes.",
                        keep.fork_id
                    ))
                })?;
            let counts = engine_counts(context, &keep.fork_id, &digest).await;
            let mut request = json!({
                "forkId": keep.fork_id,
                "reviewDigest": digest,
                "summary": asked.computer_summary.as_ref().unwrap_or(&asked.summary),
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
            Ok(Some((id.to_owned(), digest, counts)))
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
            if !asked.tree.contains(&request.target) {
                return Err(refused(
                    "A merge can go only into this computer or one it made.",
                ));
            }
            let counts = engine_counts(context, &request.fork_id, &request.review_digest).await;
            Ok(Some((request.id, request.review_digest, counts)))
        }
        _ => Ok(None),
    }
}

/// Whether a merge approval's review request now holds other changes than the approval.
fn merge_moved_on(context: &HandlerContext, record: &Record) -> bool {
    let Some(request_id) = &record.approval.review_request_id else {
        return false;
    };
    fold_requests::Store::new(&app_data(context))
        .get(request_id)
        .ok()
        .flatten()
        .is_none_or(|request| {
            request.status != FoldRequestStatus::Open
                || Some(&request.review_digest) != record.approval.review_digest.as_ref()
        })
}

/// Stores the approval for a held call, or finds the one the same computer already asked for
/// the same call. `Ok` is the `approval_required` answer; `Err` refuses the call.
pub(crate) async fn request(
    context: &HandlerContext,
    asked: Asked<'_>,
) -> Result<DispatchError, DispatchError> {
    let store = store(context);
    for expired in store.sweep(approvals::now_ms()) {
        announce(context, &expired);
    }
    let key = approvals::request_key(asked.method.name(), asked.asked);
    let pin = pin_of(context, asked.method, &asked.params);
    if let Some(same) = store.find_pending(asked.computer, &key) {
        if !merge_moved_on(context, &same) && same.pin == pin {
            return Ok(approval_required(&same));
        }
        // The review request was asked for again with other changes, or what the call acts
        // on moved, since: the earlier ask can no longer be allowed as asked.
        if let Ok(cancelled) = store.cancel(
            same.id(),
            None,
            "What the call acts on changed after it was asked; the computer asked again.",
        ) {
            announce(context, &cancelled);
        }
    }
    let linked = review_request(context, &asked).await?;
    if let Some((request_id, digest, _)) = &linked {
        // The review request now shows these changes, so an earlier ask about other changes
        // can no longer be kept as asked; it is withdrawn before the new one counts.
        for older in store.pending_for_request(request_id) {
            if older.approval.review_digest.as_ref() == Some(digest) {
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
    let made = store.ask(NewApproval {
        kind: asked.kind,
        computer_id: asked.computer.to_owned(),
        computer_name: asked.computer_name.clone(),
        method: asked.method.name().to_owned(),
        params: asked.params.clone(),
        asked: asked.asked.clone(),
        summary: asked.summary.clone(),
        computer_summary: asked.computer_summary.clone(),
        creates: asked.creates.as_ref().map(|creation| approvals::Creation {
            name: creation.name.clone(),
            id: creation.id.clone(),
            source: creation.source.clone(),
        }),
        pin,
        review_request_id: linked.as_ref().map(|(id, _, _)| id.clone()),
        review_digest: linked.as_ref().map(|(_, digest, _)| digest.clone()),
    });
    let (record, created) = match made {
        Ok(made) => made,
        Err(AskError::Limit) => {
            return Err(refused(format!(
                "{} already has {} requests waiting for an answer. Wait for an answer, or withdraw one, before asking again.",
                shown_name(&asked.computer_name),
                approvals::PENDING_LIMIT
            )))
        }
        Err(AskError::Rate) => {
            return Err(refused(format!(
                "{} has already made {} requests in the last hour, which is the most it can. Wait before asking again.",
                shown_name(&asked.computer_name),
                approvals::ASKS_PER_HOUR
            )))
        }
        Err(AskError::Store(message)) => return Err(DispatchError::internal(message)),
    };
    if created {
        if let Some((request_id, _, counts)) = &linked {
            if let Err(message) = fold_requests::Store::new(&app_data(context)).link_approval(
                request_id,
                record.id(),
                &record.approval.summary,
                counts.clone(),
            ) {
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
        .map(|record| view_for(context, record))
        .collect();
    output(ApprovalsListResult { approvals })
}

async fn get(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let params: ApprovalIdParams = context.decode_params(params)?;
    for record in store(context).sweep(approvals::now_ms()) {
        announce(context, &record);
    }
    let mut record = [visible_record(context, &params.id)?];
    settle_links(context, &mut record);
    let [record] = record;
    output(view_for(context, &record))
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
    let code =
        if message.starts_with("There is no approval") || message.contains("not an approval id") {
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
    let note = bounded_note(params.note, DECISION_NOTE_MAX_CHARS)?;
    let store = store(context);
    let found = visible_record(context, &params.id)?;
    match params.decision {
        ApprovalDecision::Deny => {
            let denied = store
                .deny(found.id(), &by, note.as_deref())
                .map_err(unchangeable)?;
            announce(context, &denied);
            if let Some(request_id) = &found.approval.review_request_id {
                let _ = fold_requests::Store::new(&app_data(context)).mark_thrown_away(request_id);
            }
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
fn merge_call(
    context: &HandlerContext,
    record: &Record,
    tree: &BTreeSet<String>,
) -> Result<(MethodName, Value), String> {
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
    if !tree.contains(&request.target) || !tree.contains(&request.fork_id) {
        return Err(
            "The merge no longer goes between computers the one that asked made, so nothing was merged."
                .into(),
        );
    }
    if !matches!(request.status, FoldRequestStatus::Open) {
        return Err(
            "Its review request was already answered or closed, so nothing was merged.".into(),
        );
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

/// The answers a computer recorded when it asked for the merge of `request`, which the owner
/// is shown with the ask: the choices and the items to skip of the pending approval that wraps
/// the request as it stands now. Merging from the review page applies them, so the page does
/// not ask again what the ask already answered.
pub(crate) fn recorded_answers(
    context: &HandlerContext,
    request: &FoldRequest,
) -> (
    std::collections::BTreeMap<String, silo_api::methods::keep::KeepAnswer>,
    Vec<String>,
) {
    if matches!(context.caller(), Caller::Computer { .. }) {
        return Default::default();
    }
    let Some(record) = store(context)
        .pending_for_request(&request.id)
        .into_iter()
        .find(|record| record.approval.review_digest.as_ref() == Some(&request.review_digest))
    else {
        return Default::default();
    };
    let choices = record
        .approval
        .params
        .get("choices")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    let skip = record
        .approval
        .params
        .get("skip")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    (choices, skip)
}

/// The call that runs: a merge runs the review request it was shown; any other call runs as
/// the second check now rewrites it, when it does.
fn call_to_run(
    stored: Result<(MethodName, Value), String>,
    rewritten: Option<Value>,
    method: MethodName,
    linked: bool,
) -> Result<(MethodName, Value), String> {
    match (stored?, rewritten) {
        (_, Some(rewritten)) if !linked => Ok((method, rewritten)),
        (call, _) => Ok(call),
    }
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
        let (tree, rewritten) = match gate::judge(context, asker, method, &approval.params).await {
            None => {
                return failed(format!(
                    "{} no longer exists, so nothing was run.",
                    approval.computer_name
                ))
            }
            Some((Decision::Refuse { reason }, _)) => {
                return failed(format!("Nothing was run: {reason}"));
            }
            // Asked again now, the call must ask for exactly what you were shown.
            Some((
                Decision::Ask {
                    kind,
                    summary,
                    computer_summary,
                    ..
                },
                _,
            )) if kind != approval.kind
                || summary != approval.summary
                || record
                    .computer_summary
                    .as_ref()
                    .is_some_and(|stored| computer_summary.as_ref() != Some(stored)) =>
            {
                return failed(
                    "Nothing was run: the call would now ask for something other than what you allowed. The computer can ask again.",
                );
            }
            Some((Decision::Ask { .. }, tree)) => (tree, None),
            // The computer may do it now without asking; it runs as the policy now rewrites it.
            Some((Decision::Allow(allowed), tree)) => (tree, allowed.params),
        };
        if gate::takes_headroom(method) {
            if let Some(message) =
                gate::short_of_resources(context, method, &approval.params, &std::collections::HashMap::new())
            {
                return failed(format!("Nothing was run: {message}"));
            }
        }
        if let Some(pinned) = &record.pin {
            if pin_of(context, method, &approval.params).as_ref() != Some(pinned) {
                return failed(pin_moved_message(method));
            }
        }
        let linked = approval.review_request_id.is_some();
        let (run_method, run_params) = match call_to_run(
            merge_call(context, record, &tree),
            rewritten,
            method,
            linked,
        ) {
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
        let review_digest = approval
            .review_digest
            .clone()
            .filter(|_| run_method == MethodName::ChangesRequestsKeep);
        let result = run_call(
            context,
            run_method,
            &run_params,
            record.pin.clone(),
            review_digest,
        )
        .await;
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
        gate::record_approved(
            context,
            run_method,
            &run_params,
            asker,
            &result,
            record.id(),
        );
        match result {
            Ok(_) if job.is_some() => ApprovalOutcome {
                result: None,
                job_id: job,
                error: None,
            },
            Ok(output) => ApprovalOutcome {
                result: Some(outcome_result(run_method, &run_params, &output.value)),
                job_id: None,
                error: None,
            },
            Err(error) => failed(error.message),
        }
    })
}

/// What an allowed call tells the computer that asked: a small answer written here for each
/// method, never what the call returned, which can hold the device's secrets, GitHub accounts
/// and computers. A method this does not know answers with nothing.
fn outcome_result(method: MethodName, params: &Value, returned: &Value) -> Value {
    let text =
        |value: &Value, pointer: &str| value.pointer(pointer).cloned().unwrap_or(Value::Null);
    match method {
        MethodName::SecretsSave => json!({ "name": text(params, "/request/name") }),
        MethodName::NetworkPublish => {
            let computer = params.get("computerId").and_then(Value::as_str);
            let port = params.get("port").and_then(Value::as_u64);
            let published = returned
                .get("computers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|entry| entry.get("computerId").and_then(Value::as_str) == computer)
                .and_then(|entry| entry.get("ports").and_then(Value::as_array))
                .into_iter()
                .flatten()
                .find(|entry| entry.get("port").and_then(Value::as_u64) == port);
            let from =
                |key: &str| published.map_or(Value::Null, |entry| text(entry, &format!("/{key}")));
            json!({
                "computerId": computer,
                "port": port,
                "hostPort": from("hostPort"),
                "scheme": from("scheme"),
                "state": from("state"),
            })
        }
        MethodName::SshAccessSave => {
            let computer = params.get("computerId").and_then(Value::as_str);
            let saved = returned
                .get("computers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|entry| entry.get("computerId").and_then(Value::as_str) == computer);
            let from =
                |key: &str| saved.map_or(Value::Null, |entry| text(entry, &format!("/{key}")));
            json!({
                "computerId": computer,
                "enabled": from("enabled"),
                "port": from("port"),
                "bindAddress": from("bindAddress"),
                "state": from("state"),
            })
        }
        MethodName::NetworkPolicySet => json!({
            "computerId": text(returned, "/computerId"),
            "policy": text(returned, "/policy"),
            "timing": text(returned, "/timing"),
        }),
        MethodName::ComputersCreate | MethodName::CheckpointsFork => {
            match returned.pointer("/computer/id").and_then(Value::as_str) {
                Some(id) => {
                    json!({ "computer": { "id": id, "name": text(returned, "/computer/name") } })
                }
                None => json!({}),
            }
        }
        _ => json!({}),
    }
}

#[cfg(test)]
pub(crate) type TestRun =
    std::sync::Arc<dyn Fn(MethodName, &Value) -> Result<Value, DispatchError> + Send + Sync>;

#[cfg(test)]
tokio::task_local! {
    /// Stands in for the engine running an allowed call, in a test.
    pub(crate) static TEST_RUN: TestRun;
}

tokio::task_local! {
    /// Set while an allowed call runs for the computer that asked, so a merge hides the
    /// recipe's command lines from the job that computer reads.
    static FOR_COMPUTER: bool;
    /// The state an allowed undo or promote was approved for; the job compares it while it
    /// holds the lanes, since another approved call may run between this check and the job.
    static EXPECTED_PIN: Option<String>;
    /// The review digest an allowed merge was approved for; the keep compares it while it
    /// holds the request, since the computer may ask again between the check and the keep.
    static EXPECTED_REVIEW: Option<String>;
}

/// The review digest the allowed merge that is running was approved for, when it is one.
pub(crate) fn expected_review_digest() -> Option<String> {
    EXPECTED_REVIEW.try_with(Clone::clone).ok().flatten()
}

/// The state the allowed call that is running was approved for, when it acts on pinned state.
pub(crate) fn expected_pin() -> Option<String> {
    EXPECTED_PIN.try_with(Clone::clone).ok().flatten()
}

/// Whether a merge's result goes to a computer: it is the caller, or it asked for the call an
/// owner allowed.
pub(crate) fn result_goes_to_computer(context: &HandlerContext) -> bool {
    matches!(context.caller(), Caller::Computer { .. })
        || FOR_COMPUTER.try_with(|asked| *asked).unwrap_or(false)
}

/// Runs an allowed call as the owner.
async fn run_call(
    context: &HandlerContext,
    method: MethodName,
    params: &Value,
    pin: Option<String>,
    review_digest: Option<String>,
) -> Result<HandlerOutput, DispatchError> {
    EXPECTED_REVIEW
        .scope(review_digest, async {
            EXPECTED_PIN
                .scope(pin, async {
                    FOR_COMPUTER
                        .scope(true, async {
                            #[cfg(test)]
                            if let Ok(run) = TEST_RUN.try_with(Clone::clone) {
                                return run(method, params).map(|value| HandlerOutput {
                                    value,
                                    event_barrier: None,
                                });
                            }
                            super::run(context, method, params).await
                        })
                        .await
                })
                .await
        })
        .await
}

async fn cancel(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let params: ApprovalIdParams = context.decode_params(params)?;
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
    output(view_for(context, &cancelled))
}

async fn note(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let Caller::Computer { id: caller } = context.caller() else {
        return Err(refused(
            "Only the computer that asked writes its note; your own note goes with approvals.decide.",
        ));
    };
    let params: ApprovalsNoteParams = context.decode_params(params)?;
    let found = visible_record(context, &params.id)?;
    let noted = store(context)
        .note(found.id(), caller, &params.note)
        .map_err(unchangeable)?;
    announce(context, &noted);
    output(view_for(context, &noted))
}

// MARK: What a computer holds

async fn access(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let params: ComputersAccessParams = context.decode_params(params)?;
    let id = params.computer_id;
    if seen(context).is_some_and(|seen| !seen.contains(&id)) {
        return Err(refused("That computer is not this one or one it made."));
    }
    let reader = internal(context);
    let name = gate::computer_name(context, &id).await.ok_or_else(|| {
        DispatchError::new(ErrorCode::NotFound, format!("There is no computer {id}."))
    })?;
    let held_name = name.clone();
    let (secrets, github) = tokio::task::spawn_blocking(move || policy::access::held(&held_name))
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
    let network_policy = super::route(
        &reader,
        MethodName::NetworkPolicyGet,
        &json!({ "computerId": id }),
    )
    .await
    .ok()
    .and_then(|output| serde_json::from_value::<NetworkPolicyState>(output.value).ok())
    .map(|state| state.policy);
    output(ComputerAccess {
        computer_id: id,
        name,
        secrets,
        github,
        ports,
        ssh,
        network_policy,
    })
}

// MARK: Review requests answered from the app

/// A review request kept or thrown away directly closes the approval that wraps it: kept is
/// allowed with the keep's job, thrown away by the owner is denied, by a computer cancelled.
pub(crate) fn request_answered(context: &HandlerContext, request_id: &str, kept_job: Option<&str>) {
    let store = store(context);
    let by = match context.caller() {
        Caller::Owner { client } => Some(client.clone()),
        Caller::Device { .. } => Some("another device".to_owned()),
        Caller::Computer { .. } => None,
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
