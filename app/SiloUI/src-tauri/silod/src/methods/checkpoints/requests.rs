//! `changes.requests.*`: list, show, keep, throw away and open the review requests that
//! `changes.request` records. The store and the rules live in `silo_engine::fold_requests`;
//! the diff, keep, stop, delete and start work is the engine's existing code.

use std::{sync::Arc, time::Duration};

use serde::Serialize;
use serde_json::{json, Value};
use silo_api::{
    jobs::JobId,
    methods::{
        common::JobStartResult,
        keep::{FoldRequest, FoldRequestStatus},
        review_requests::{
            bucket_counts, ChangesRequestsKeepParams, ChangesRequestsListParams,
            ChangesRequestsListResult, ChangesRequestsMarkKeptParams,
            ChangesRequestsMarkKeptResult, ChangesRequestsOpenResult, ChangesRequestsShowResult,
            ChangesRequestsThrowAwayResult, RequestEntry, RequestIdParams,
        },
        MethodName,
    },
};
use silo_engine::{
    changes::{
        collect::TargetRunner,
        keep::{service as keep_service, store, Hooks, Request},
        service::{self as changes_service, Work},
    },
    checkpoints::Failure,
    context::{Audience, EngineContext},
    fold_requests::{self, Store},
};

use crate::{
    context::SilodContext,
    dispatcher::{DispatchError, HandlerOutput},
};

use super::{
    super::{
        computers,
        guest::{guest_error, resolve_target},
    },
    blocking, engine_context, failure, message_error, output, require_local_device,
    service as checkpoint_service, HandlerContext,
};

/// A computer as `computers.list` reports it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub id: String,
    pub name: String,
    pub state: String,
    /// What `computers.delete` needs for a Linux computer.
    pub configuration: Option<Value>,
    /// The ids of the computer's checkpoints: a fork's source may be one of them.
    pub checkpoints: Vec<String>,
}

fn checkpoint_ids(computer: &Value) -> Vec<String> {
    computer
        .get("checkpoints")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|checkpoint| checkpoint.get("id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

/// The computer a fork's recorded source stands for: the source itself when it is a
/// computer, or the computer that owns it when it is one of its checkpoints.
pub(super) fn source_computer(rows: &[Row], source: &str) -> String {
    if rows.iter().any(|row| row.id == source) {
        return source.to_owned();
    }
    rows.iter()
        .find(|row| row.checkpoints.iter().any(|id| id == source))
        .map_or_else(|| source.to_owned(), |row| row.id.clone())
}

/// The computers out of a `computers.list` reply.
pub(super) fn rows(listed: &Value) -> Vec<Row> {
    let text = |value: &Value, key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let mut found = Vec::new();
    for computer in listed
        .pointer("/linux/computers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(configuration) = computer.get("configuration") {
            found.push(Row {
                id: text(configuration, "id"),
                name: text(configuration, "name"),
                state: text(computer, "state"),
                configuration: Some(configuration.clone()),
                checkpoints: checkpoint_ids(computer),
            });
        }
    }
    for computer in listed
        .pointer("/macos/computers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        found.push(Row {
            id: text(computer, "id"),
            name: text(computer, "name"),
            state: text(computer, "state"),
            configuration: None,
            checkpoints: checkpoint_ids(computer),
        });
    }
    found
}

async fn call_computers(
    context: &HandlerContext,
    method: MethodName,
    params: Value,
) -> Result<Value, DispatchError> {
    computers::handle(method, &params, context)
        .await
        .unwrap_or_else(|| Err(DispatchError::unsupported(method.name())))
        .map(|output| output.value)
}

/// The computers and whether the list is complete: both platforms were read and the Linux
/// runtime reported no repair that hides computers.
struct Listing {
    rows: Vec<Row>,
    complete: bool,
}

fn parse_listing(listed: &Value) -> Listing {
    Listing {
        rows: rows(listed),
        complete: listed
            .pointer("/linux/runtimeRepair")
            .is_none_or(Value::is_null)
            && listed.pointer("/macos/computers").is_some(),
    }
}

/// Every computer. The read has no event barrier of its own: it is not a subscription's
/// snapshot, so a client that has not subscribed to `computers.*` can still ask.
async fn listing(context: &HandlerContext) -> Result<Listing, DispatchError> {
    #[cfg(test)]
    if let Some((rows, complete)) = tests_rows::get() {
        return Ok(Listing { rows, complete });
    }
    let reader = HandlerContext::for_owner_bridge(context.engine.clone(), context.client.clone());
    Ok(parse_listing(
        &call_computers(&reader, MethodName::ComputersList, json!({})).await?,
    ))
}

/// Every computer, refusing a list that may leave some out: a fork missing from it is not
/// known to be gone.
async fn all_rows(context: &HandlerContext) -> Result<Vec<Row>, DispatchError> {
    let found = listing(context).await?;
    if !found.complete {
        return Err(message_error(
            "The list of computers is incomplete (the runtime needs repair), so nothing is changed.",
        ));
    }
    Ok(found.rows)
}

fn store_for(context: &HandlerContext) -> Store {
    Store::new(&context.silod_context().paths().app_data)
}

/// What a handler needs a request to be.
#[derive(Clone, Copy)]
enum Need {
    Any,
    Open,
    Keepable,
}

async fn open_request(
    context: &HandlerContext,
    id: String,
    need: Need,
) -> Result<FoldRequest, DispatchError> {
    let store = store_for(context);
    blocking(move || {
        match need {
            Need::Open => store.open_request(&id),
            Need::Keepable => store.keepable_request(&id),
            Need::Any => store
                .get(&id)
                .and_then(|found| found.ok_or_else(|| format!("There is no review request {id}."))),
        }
        .map_err(message_error)
    })
    .await
}

/// The computer a fork was forked from, from its fork-time record.
pub(super) async fn fork_source(
    context: &HandlerContext,
    fork_id: &str,
) -> Result<Option<String>, DispatchError> {
    #[cfg(test)]
    {
        let _ = context;
        return Ok(tests_rows::source(fork_id));
    }
    #[cfg(not(test))]
    {
        let manifests = store::production(context.silod_context().runtime_host());
        let fork = fork_id.to_owned();
        blocking(move || manifests.source(&fork).map_err(message_error)).await
    }
}

/// Refuses a fork that is not a fork of `target` (or, for a folder, the CLI's fork of it):
/// a request names only computers it may keep into or drop.
pub(super) async fn verify_fork(
    context: &HandlerContext,
    fork_id: &str,
    target: &str,
) -> Result<(), DispatchError> {
    let source = if fold_requests::folder_path(target).is_some() {
        None
    } else {
        match fork_source(context, fork_id).await? {
            // A record without an origin names a checkpoint: it counts as its computer.
            Some(source) => Some(source_computer(&listing(context).await?.rows, &source)),
            None => None,
        }
    };
    let state_dir = context
        .silod_context()
        .identity()
        .channel
        .state_dir(&context.silod_context().paths().home);
    fold_requests::check_fork_belongs(fork_id, target, source.as_deref(), &state_dir)
        .map_err(message_error)
}

/// Keeps the claim for a job's whole run.
fn claim(id: &str) -> Result<fold_requests::Claim, DispatchError> {
    fold_requests::claim(id).map_err(message_error)
}

/// Marks requests whose keep started and finished kept (see `settle_request`).
async fn settle(context: &HandlerContext) {
    let engine = engine_context(context);
    let _ = blocking(move || {
        keep_service::settle_all(engine.as_ref());
        Ok(())
    })
    .await;
}

fn fork_row<'a>(rows: &'a [Row], request: &FoldRequest) -> Option<&'a Row> {
    rows.iter().find(|row| row.id == request.fork_id)
}

fn target_name(rows: &[Row], request: &FoldRequest) -> Option<String> {
    rows.iter()
        .find(|row| row.id == request.target)
        .map(|row| row.name.clone())
}

pub(super) async fn list(
    context: &HandlerContext,
    raw: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesRequestsListParams = context.decode_params(raw)?;
    require_local_device(context, params.device_id.as_deref())?;
    let filter = params.status.unwrap_or_default();
    settle(context).await;
    let found = listing(context).await?;
    let store = store_for(context);
    // Only a list that has every computer shows that a fork or target is gone.
    let present: Option<Vec<String>> = found
        .complete
        .then(|| found.rows.iter().map(|row| row.id.clone()).collect());
    let requests = blocking(move || {
        if let Some(present) = present {
            store.close_missing(&present);
        }
        Ok(store.list_matching(filter))
    })
    .await?;
    let computers = found.rows;
    let requests = requests
        .into_iter()
        .map(|request| {
            let fork = fork_row(&computers, &request);
            RequestEntry {
                fork_name: fork.map(|row| row.name.clone()),
                fork_state: fork.map(|row| row.state.clone()),
                target_name: target_name(&computers, &request),
                device_id: None,
                device_name: None,
                request,
            }
        })
        .collect();
    let local = ChangesRequestsListResult {
        requests,
        unreachable: Vec::new(),
    };
    if params.all_devices == Some(true) {
        return output(super::all_devices::merge(context, raw, local).await);
    }
    output(local)
}

/// `changes.requests.markKept`: closes the open requests for a fork whose changes were kept
/// outside the engine (the CLI folding a folder fork into its folder).
pub(super) async fn mark_kept(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesRequestsMarkKeptParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    verify_fork(context, &params.fork_id, &params.target).await?;
    let store = store_for(context);
    let requests = blocking(move || {
        Ok(store.mark_kept_for(&params.fork_id, &params.target, params.keep_id.as_deref()))
    })
    .await?;
    output(ChangesRequestsMarkKeptResult { requests })
}

/// Accepts a job that this module finishes itself.
fn begin(context: &HandlerContext, method: &str) -> Result<JobId, DispatchError> {
    let started = context.start_job(method)?;
    context
        .job_registry()?
        .mark_running(&started.job_id)
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    Ok(started.job_id)
}

fn finish(silod: &SilodContext, job_id: JobId, outcome: Result<Value, String>) {
    let _ = match outcome {
        Ok(value) => silo_engine::jobs::finish_with_result(silod, job_id, value, Audience::All),
        Err(message) => {
            let failure = Failure::from_message(message);
            silo_engine::jobs::finish_with_error(
                silod,
                job_id,
                failure.code,
                failure.message,
                Audience::All,
            )
        }
    };
}

fn encode(value: impl Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

fn started(job_id: JobId) -> Result<HandlerOutput, DispatchError> {
    output(JobStartResult { job_id })
}

/// The show result for a request: the fresh diff document and what the credentials line says.
fn show_result(
    request: FoldRequest,
    fork: &Row,
    target_name: Option<String>,
    diff: &Value,
    fresh: bool,
) -> Result<ChangesRequestsShowResult, String> {
    let review_digest = diff["reviewDigest"].as_str().unwrap_or_default().to_owned();
    let document = diff["diff"].clone();
    Ok(ChangesRequestsShowResult {
        stale: Some(fold_requests::is_stale(&request, &review_digest)),
        review_digest,
        fresh,
        counts: bucket_counts(&document),
        diff: document,
        text: diff["text"].as_str().unwrap_or_default().to_owned(),
        credentials: fold_requests::credentials_for(&fork.id),
        fork_name: fork.name.clone(),
        fork_state: fork.state.clone(),
        target_name,
        request,
    })
}

pub(super) async fn show(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: RequestIdParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let request = open_request(context, params.request_id, Need::Any).await?;
    let computers = all_rows(context).await?;
    let Some(fork) = fork_row(&computers, &request).cloned() else {
        // A request that was answered stays readable after its fork is gone.
        if matches!(
            request.status,
            FoldRequestStatus::Kept | FoldRequestStatus::ThrownAway
        ) {
            let result = ChangesRequestsShowResult {
                stale: None,
                review_digest: request.review_digest.clone(),
                fresh: false,
                counts: request.counts.clone(),
                diff: Value::Null,
                text: request.summary.clone(),
                credentials: fold_requests::credentials_for(&request.fork_id),
                fork_name: String::new(),
                fork_state: "gone".to_owned(),
                target_name: target_name(&computers, &request),
                request,
            };
            let job_id = begin(context, "changes.requests.show")?;
            finish(context.silod_context(), job_id.clone(), encode(result));
            return started(job_id);
        }
        return Err(message_error("The fork of this request no longer exists."));
    };
    if fold_requests::folder_path(&request.target).is_some() {
        // A folder fork's changes are its git commits, which only the CLI reads.
        let result = ChangesRequestsShowResult {
            stale: None,
            review_digest: request.review_digest.clone(),
            fresh: false,
            counts: request.counts.clone(),
            diff: Value::Null,
            text: request.summary.clone(),
            credentials: fold_requests::credentials_for(&fork.id),
            fork_name: fork.name.clone(),
            fork_state: fork.state.clone(),
            target_name: target_name(&computers, &request),
            request,
        };
        let job_id = begin(context, "changes.requests.show")?;
        finish(context.silod_context(), job_id.clone(), encode(result));
        return started(job_id);
    }
    let silod = Arc::new(context.silod_context().clone());
    let engine = engine_context(context);
    let running = fork.state == "running";
    let finisher_fork = fork.clone();
    let finisher_target = target_name(&computers, &request);
    let job_id = blocking(move || {
        let target = resolve_target(&silod, &finisher_fork.id).map_err(guest_error)?;
        if running {
            target.ensure_running().map_err(guest_error)?;
        }
        let finish: changes_service::Finish = Box::new(move |diff| {
            show_result(request, &finisher_fork, finisher_target, &diff, running)
                .and_then(encode)
                .map_err(silo_engine::guest::GuestError::invalid)
        });
        changes_service::start_with(
            engine,
            Arc::new(TargetRunner(target)),
            fork.id,
            "changes.requests.show",
            Work::Diff {
                max_lines: None,
                baseline: true,
                refresh: running,
            },
            finish,
        )
        .map_err(guest_error)
    })
    .await?;
    started(job_id)
}

pub(super) async fn keep(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesRequestsKeepParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    // One job per request: taken before anything is read, held until the keep job ends.
    let hold = claim(&params.request_id)?;
    let request = open_request(context, params.request_id, Need::Keepable).await?;
    fold_requests::check_keepable(&request).map_err(message_error)?;
    let approved = crate::methods::approvals::expected_review_digest();
    check_approved_changes(&request.review_digest, approved.as_deref())?;
    let computers = all_rows(context).await?;
    if fork_row(&computers, &request).is_none() {
        return Err(message_error(
            "The fork of this request no longer exists, so there is nothing to keep.",
        ));
    }
    verify_fork(context, &request.fork_id, &request.target).await?;
    let host = context.silod_context().runtime_host();
    let hooks = params.keep.map(|h| Hooks {
        restart: h.restart,
        migrate: h.migrate,
        after: h.after,
    });
    let save_recipe = matches!(
        context.caller(),
        silo_engine::policy::Caller::Owner { .. } | silo_engine::policy::Caller::Device { .. }
    );
    // What the computer's ask recorded applies unless the caller answers the same item.
    let (mut choices, mut skip) = crate::methods::approvals::recorded_answers(context, &request);
    choices.extend(params.choices.unwrap_or_default());
    skip.extend(params.skip.unwrap_or_default());
    let keep_request = Request {
        fork: request.fork_id.clone(),
        target: Some(request.target.clone()),
        hooks: hooks.unwrap_or_default(),
        dry_run: false,
        data_ok: params.data_ok.unwrap_or(false),
        reviewed: Some(approved.unwrap_or_else(|| request.review_digest.clone())),
        choices: super::keep::choices_of(Some(choices), Some(skip)),
        save_recipe,
        hide_recipe_commands: crate::methods::approvals::result_goes_to_computer(context),
        ..Request::default()
    };
    let checkpoints = checkpoint_service(context);
    let manifests = store::production(host.clone());
    let id = request.id;
    let request_id = id.clone();
    let job_id = tokio::task::spawn_blocking(move || {
        keep_service::start_request_keep(host, checkpoints, manifests, keep_request, id, hold)
    })
    .await
    .map_err(|_| DispatchError::internal("The keep could not start."))?
    .map_err(failure)?;
    crate::methods::approvals::request_answered(context, &request_id, Some(job_id.as_str()));
    started(job_id)
}

/// Refuses a keep whose request shows other changes than the ones the owner approved: the
/// computer asked for the review again after it was approved.
fn check_approved_changes(shown: &str, approved: Option<&str>) -> Result<(), DispatchError> {
    match approved {
        Some(approved) if approved != shown => Err(message_error(
            "The review request was asked for again with other changes after you were asked, so nothing was merged. The computer can ask again.",
        )),
        _ => Ok(()),
    }
}

/// The same handler context, owned, for work that outlives the request.
fn detached(context: &HandlerContext) -> HandlerContext {
    HandlerContext::new(context.engine.clone(), context.client.clone())
}

/// Waits for a job another handler started and returns its result.
pub(super) async fn wait_for(
    silod: &SilodContext,
    job_id: &str,
    limit: Duration,
) -> Result<Value, String> {
    let deadline = std::time::Instant::now() + limit;
    let id = JobId::new(job_id).map_err(|_| "The engine returned an invalid job id.".to_owned())?;
    let registry = silo_engine::context::EngineContext::job_registry(silod)?;
    loop {
        let found = registry
            .get(&id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("The record of the job {job_id} is gone."))?;
        if found.state == silo_api::jobs::JobState::Finished {
            let _ = registry.acknowledge(&id);
            let outcome = found.outcome.unwrap_or(silo_api::jobs::JobOutcome {
                result: None,
                error: None,
            });
            return match outcome.error {
                Some(error) => Err(error.message),
                None => Ok(outcome.result.unwrap_or(Value::Null)),
            };
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "The job {job_id} did not finish within {} seconds.",
                limit.as_secs()
            ));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn run_sub_job(
    context: &HandlerContext,
    method: MethodName,
    params: Value,
) -> Result<Value, String> {
    let started = call_computers(context, method, params)
        .await
        .map_err(|error| error.message)?;
    let job_id = started["jobId"]
        .as_str()
        .ok_or_else(|| "The engine did not accept the job.".to_owned())?
        .to_owned();
    wait_for(context.silod_context(), &job_id, JOB_WAIT).await
}

const STOP_WAIT: Duration = Duration::from_secs(120);
/// The longest a stop, delete or start job is waited for.
const JOB_WAIT: Duration = Duration::from_secs(600);
/// The longest a started fork is waited for to report running.
#[cfg(not(test))]
const RUNNING_WAIT: Duration = Duration::from_secs(120);
#[cfg(test)]
const RUNNING_WAIT: Duration = Duration::from_secs(2);
const DROP_RETRIES: u32 = 10;

/// Stops the fork when it is not stopped, then deletes it, as `silo drop` does.
async fn drop_fork(context: &HandlerContext, fork: &Row) -> Result<(), String> {
    if fork.state != "stopped" {
        let stopped = run_sub_job(
            context,
            MethodName::ComputersStop,
            json!({ "computerId": fork.id }),
        )
        .await;
        if stopped.is_ok() {
            let begun = std::time::Instant::now();
            while begun.elapsed() < STOP_WAIT {
                let listed = all_rows(context).await.map_err(|error| error.message)?;
                match listed.iter().find(|row| row.id == fork.id) {
                    Some(now) if now.state != "stopped" => {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    _ => break,
                }
            }
        }
    }
    let mut attempt = 0;
    loop {
        let mut params = json!({ "computerId": fork.id });
        if let Some(configuration) = &fork.configuration {
            params["expected"] = configuration.clone();
        }
        match run_sub_job(context, MethodName::ComputersDelete, params).await {
            Err(message)
                if message.contains("Stop the computer first") && attempt < DROP_RETRIES =>
            {
                attempt += 1;
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            other => return other.map(|_| ()),
        }
    }
}

pub(super) async fn throw_away(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: RequestIdParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    // One job per request: held until the drop job ends, so a keep cannot start meanwhile.
    let hold = claim(&params.request_id)?;
    let request = open_request(context, params.request_id, Need::Open).await?;
    // A listing that may leave computers out does not show that a fork is gone.
    let computers = all_rows(context).await?;
    let job_id = begin(context, "changes.requests.throwAway")?;
    let Some(fork) = fork_row(&computers, &request).cloned() else {
        // The fork is already gone; only the request is left to close.
        let store = store_for(context);
        let outcome = blocking(move || store.mark_thrown_away(&request.id).map_err(message_error))
            .await
            .map_err(|error| error.message)
            .inspect(|request| {
                crate::methods::approvals::request_answered(context, &request.id, None)
            })
            .and_then(|request| {
                encode(ChangesRequestsThrowAwayResult {
                    request,
                    dropped: String::new(),
                })
            });
        finish(context.silod_context(), job_id.clone(), outcome);
        return started(job_id);
    };
    // Only a fork of the request's target is dropped, however the request was made.
    let allowed = match verify_fork(context, &fork.id, &request.target).await {
        Ok(()) => fold_requests::check_throwable(&request, &fork.id, &fork.name),
        Err(error) => Err(error.message),
    };
    if let Err(message) = allowed {
        finish(context.silod_context(), job_id.clone(), Err(message));
        return started(job_id);
    }
    let owned = detached(context);
    let finishing = job_id.clone();
    tokio::spawn(async move {
        let _hold = hold;
        let _touch = silo_engine::generation::touch(Some(&fork.id));
        let outcome = match drop_fork(&owned, &fork).await {
            Ok(()) => store_for(&owned)
                .mark_thrown_away(&request.id)
                .map_err(|error| format!("{} was dropped, but {error}", fork.name))
                .inspect(|request| {
                    crate::methods::approvals::request_answered(&owned, &request.id, None)
                })
                .and_then(|request| {
                    encode(ChangesRequestsThrowAwayResult {
                        request,
                        dropped: fork.name.clone(),
                    })
                }),
            Err(message) => Err(message),
        };
        finish(owned.silod_context(), finishing, outcome);
    });
    started(job_id)
}

/// The name `ssh` knows the fork by once `silo ssh-setup` has added it.
fn ssh_alias(context: &HandlerContext, name: &str) -> Option<String> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    let suffix = if silo_engine::context::EngineContext::identity(context.silod_context())
        .channel
        .is_development()
    {
        "silo-dev"
    } else {
        "silo"
    };
    valid.then(|| format!("{name}.{suffix}"))
}

pub(super) async fn open(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: RequestIdParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let request = open_request(context, params.request_id, Need::Open).await?;
    let computers = all_rows(context).await?;
    let fork = fork_row(&computers, &request)
        .cloned()
        .ok_or_else(|| message_error("The fork of this request no longer exists."))?;
    let job_id = begin(context, "changes.requests.open")?;
    let owned = detached(context);
    let finishing = job_id.clone();
    tokio::spawn(async move {
        let outcome = reach(&owned, request, fork).await;
        finish(owned.silod_context(), finishing, outcome);
    });
    started(job_id)
}

/// Starts the fork when it is stopped and says how to reach it.
async fn reach(context: &HandlerContext, request: FoldRequest, fork: Row) -> Result<Value, String> {
    let _touch = silo_engine::generation::touch(Some(&fork.id));
    let mut state = fork.state.clone();
    let mut started = false;
    // Only a stopped fork is started; one that is starting is waited for.
    if state == "stopped" {
        run_sub_job(
            context,
            MethodName::ComputersStart,
            json!({ "computerId": fork.id }),
        )
        .await?;
        started = true;
    }
    let begun = std::time::Instant::now();
    while state != "running" {
        let listed = all_rows(context).await.map_err(|error| error.message)?;
        state = listed
            .iter()
            .find(|row| row.id == fork.id)
            .map(|row| row.state.clone())
            .ok_or_else(|| format!("{} is gone.", fork.name))?;
        if state != "running" {
            if begun.elapsed() >= RUNNING_WAIT {
                return Err(format!(
                    "{} did not reach running within {} seconds (it is {state}).",
                    fork.name,
                    RUNNING_WAIT.as_secs()
                ));
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    let id = fork.id.clone();
    let urls = silo_engine::url_proxy::shared()
        .map(|proxy| proxy.urls(&id))
        .unwrap_or_default()
        .into_iter()
        .map(|url| url.url)
        .collect();
    encode(ChangesRequestsOpenResult {
        request,
        computer_id: fork.id,
        ssh_alias: ssh_alias(context, &fork.name),
        name: fork.name,
        state,
        started,
        urls,
    })
}

/// The computers the handlers see in a test, where no Linux runtime can list them.
#[cfg(test)]
pub(crate) mod tests_rows {
    use super::Row;
    use std::sync::Mutex;

    struct State {
        rows: Option<Vec<Row>>,
        complete: bool,
        sources: Vec<(String, String)>,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        rows: None,
        complete: true,
        sources: Vec::new(),
    });

    fn state() -> std::sync::MutexGuard<'static, State> {
        STATE.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The computers, a complete list, and no fork-time records.
    pub(in crate::methods::checkpoints) fn set(rows: Vec<Row>) {
        let mut state = state();
        state.rows = Some(rows);
        state.complete = true;
        state.sources.clear();
    }

    /// No computers, a complete list, and no fork-time records.
    pub fn set_empty() {
        set(Vec::new());
    }

    /// The fork-time source of each `(fork, target)` pair.
    pub fn set_sources(sources: &[(&str, &str)]) {
        state().sources = sources
            .iter()
            .map(|(fork, target)| ((*fork).to_owned(), (*target).to_owned()))
            .collect();
    }

    pub fn set_complete(complete: bool) {
        state().complete = complete;
    }

    pub(in crate::methods::checkpoints) fn get() -> Option<(Vec<Row>, bool)> {
        let state = state();
        state.rows.clone().map(|rows| (rows, state.complete))
    }

    pub fn source(fork: &str) -> Option<String> {
        state()
            .sources
            .iter()
            .find(|(candidate, _)| candidate == fork)
            .map(|(_, target)| target.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keep_approved_for_other_changes_than_the_request_shows_is_refused() {
        assert!(check_approved_changes("d1", None).is_ok());
        assert!(check_approved_changes("d1", Some("d1")).is_ok());
        let error = check_approved_changes("d2", Some("d1")).unwrap_err();
        assert!(
            error.message.contains("asked for again"),
            "{}",
            error.message
        );
    }

    #[test]
    fn computers_are_read_from_both_platforms() {
        let listed = json!({
            "linux": {"computers": [
                {"state": "running", "configuration": {"id": "c1", "name": "main"}},
                {"state": "stopped", "configuration": {"id": "c2", "name": "try"}}
            ]},
            "macos": {"computers": [{"id": "m1", "name": "mac", "state": "stopped"}]}
        });
        let found = rows(&listed);
        assert_eq!(
            found
                .iter()
                .map(|row| (row.name.as_str(), row.state.as_str()))
                .collect::<Vec<_>>(),
            [("main", "running"), ("try", "stopped"), ("mac", "stopped")]
        );
        assert!(found[0].configuration.is_some() && found[2].configuration.is_none());
    }
}
