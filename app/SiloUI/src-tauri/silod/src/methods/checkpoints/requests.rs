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
        keep::FoldRequest,
        review_requests::{
            bucket_counts, ChangesRequestsKeepParams, ChangesRequestsListParams,
            ChangesRequestsListResult, ChangesRequestsOpenResult, ChangesRequestsShowResult,
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
    blocking, engine_context, failure, message_error, output, service as checkpoint_service,
    HandlerContext,
};

/// A computer as `computers.list` reports it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub id: String,
    pub name: String,
    pub state: String,
    /// What `computers.delete` needs for a Linux computer.
    pub configuration: Option<Value>,
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

/// Every computer. The read has no event barrier of its own: it is not a subscription's
/// snapshot, so a client that has not subscribed to `computers.*` can still ask.
async fn all_rows(context: &HandlerContext) -> Result<Vec<Row>, DispatchError> {
    #[cfg(test)]
    if let Some(rows) = tests_rows::get() {
        return Ok(rows);
    }
    let reader = HandlerContext::for_owner_bridge(context.engine.clone(), context.client.clone());
    Ok(rows(
        &call_computers(&reader, MethodName::ComputersList, json!({})).await?,
    ))
}

fn store_for(context: &HandlerContext) -> Store {
    Store::new(&context.silod_context().paths().app_data)
}

async fn open_request(
    context: &HandlerContext,
    id: String,
    open_only: bool,
) -> Result<FoldRequest, DispatchError> {
    let store = store_for(context);
    blocking(move || {
        if open_only {
            store.open_request(&id)
        } else {
            store
                .get(&id)
                .and_then(|found| found.ok_or_else(|| format!("There is no review request {id}.")))
        }
        .map_err(message_error)
    })
    .await
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
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesRequestsListParams = context.decode_params(params)?;
    let filter = params.status.unwrap_or_default();
    let store = store_for(context);
    let requests = blocking(move || Ok(store.list_matching(filter))).await?;
    let computers = all_rows(context).await?;
    let requests = requests
        .into_iter()
        .map(|request| {
            let fork = fork_row(&computers, &request);
            RequestEntry {
                fork_name: fork.map(|row| row.name.clone()),
                fork_state: fork.map(|row| row.state.clone()),
                target_name: target_name(&computers, &request),
                request,
            }
        })
        .collect();
    output(ChangesRequestsListResult { requests })
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
        request,
    })
}

pub(super) async fn show(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: RequestIdParams = context.decode_params(params)?;
    let request = open_request(context, params.request_id, false).await?;
    let computers = all_rows(context).await?;
    let fork = fork_row(&computers, &request)
        .cloned()
        .ok_or_else(|| message_error("The fork of this request no longer exists."))?;
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
    let job_id = blocking(move || {
        let target = resolve_target(&silod, &finisher_fork.id).map_err(guest_error)?;
        if running {
            target.ensure_running().map_err(guest_error)?;
        }
        let finish: changes_service::Finish = Box::new(move |diff| {
            show_result(request, &finisher_fork, &diff, running)
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
    let request = open_request(context, params.request_id, true).await?;
    fold_requests::check_keepable(&request).map_err(message_error)?;
    let computers = all_rows(context).await?;
    if fork_row(&computers, &request).is_none() {
        return Err(message_error(
            "The fork of this request no longer exists, so there is nothing to keep.",
        ));
    }
    let host = context.silod_context().runtime_host();
    let hooks = params.keep.map(|h| Hooks {
        restart: h.restart,
        migrate: h.migrate,
        after: h.after,
    });
    let keep_request = Request {
        fork: request.fork_id.clone(),
        target: Some(request.target.clone()),
        hooks: hooks.unwrap_or_default(),
        dry_run: false,
        data_ok: params.data_ok.unwrap_or(false),
        reviewed: Some(request.review_digest.clone()),
    };
    let checkpoints = checkpoint_service(context);
    let manifests = store::production(host.clone());
    let id = request.id;
    let job_id = tokio::task::spawn_blocking(move || {
        keep_service::start_request_keep(host, checkpoints, manifests, keep_request, id)
    })
    .await
    .map_err(|_| DispatchError::internal("The keep could not start."))?
    .map_err(failure)?;
    started(job_id)
}

/// The same handler context, owned, for work that outlives the request.
fn detached(context: &HandlerContext) -> HandlerContext {
    HandlerContext::new(context.engine.clone(), context.client.clone())
}

/// Waits for a job another handler started and returns its result.
async fn wait_for(silod: &SilodContext, job_id: &str) -> Result<Value, String> {
    let id = JobId::new(job_id).map_err(|_| "The engine returned an invalid job id.".to_owned())?;
    let registry = silo_engine::context::EngineContext::job_registry(silod)?;
    loop {
        let found = registry.get(&id).map_err(|error| error.to_string())?;
        if let Some(found) = found {
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
    wait_for(context.silod_context(), &job_id).await
}

const STOP_WAIT: Duration = Duration::from_secs(120);
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
    let request = open_request(context, params.request_id, true).await?;
    let computers = all_rows(context).await?;
    let job_id = begin(context, "changes.requests.throwAway")?;
    let Some(fork) = fork_row(&computers, &request).cloned() else {
        // The fork is already gone; only the request is left to close.
        let store = store_for(context);
        let outcome = blocking(move || store.mark_thrown_away(&request.id).map_err(message_error))
            .await
            .map_err(|error| error.message)
            .and_then(|request| {
                encode(ChangesRequestsThrowAwayResult {
                    request,
                    dropped: String::new(),
                })
            });
        finish(context.silod_context(), job_id.clone(), outcome);
        return started(job_id);
    };
    if let Err(message) = fold_requests::check_throwable(&request, &fork.id, &fork.name) {
        finish(context.silod_context(), job_id.clone(), Err(message));
        return started(job_id);
    }
    let owned = detached(context);
    let finishing = job_id.clone();
    tokio::spawn(async move {
        let outcome = match drop_fork(&owned, &fork).await {
            Ok(()) => store_for(&owned)
                .mark_thrown_away(&request.id)
                .map_err(|error| format!("{} was dropped, but {error}", fork.name))
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
    let request = open_request(context, params.request_id, true).await?;
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
    let mut state = fork.state.clone();
    let mut started = false;
    if state != "running" {
        run_sub_job(
            context,
            MethodName::ComputersStart,
            json!({ "computerId": fork.id }),
        )
        .await?;
        started = true;
        let listed = all_rows(context).await.map_err(|error| error.message)?;
        state = listed
            .iter()
            .find(|row| row.id == fork.id)
            .map_or_else(|| "running".to_owned(), |row| row.state.clone());
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

/// The computers the handlers see in a test, where no MicroSandbox runtime can list them.
#[cfg(test)]
pub(super) mod tests_rows {
    use super::Row;
    use std::sync::Mutex;

    static ROWS: Mutex<Option<Vec<Row>>> = Mutex::new(None);

    pub fn set(rows: Vec<Row>) {
        *ROWS.lock().unwrap_or_else(|p| p.into_inner()) = Some(rows);
    }

    pub fn get() -> Option<Vec<Row>> {
        ROWS.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
