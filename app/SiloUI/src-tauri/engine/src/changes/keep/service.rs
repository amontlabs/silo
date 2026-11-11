//! The `changes.keep` and `changes.undo` jobs.

use std::sync::Arc;

use silo_api::{
    jobs::JobId,
    methods::keep::{FoldRequest, FoldRequestStatus},
};

use super::{
    env::{KeepEnv, ManifestStore},
    guest_env::GuestEnv,
    journal::Journal,
    Request, LANE_WAIT,
};
use crate::{
    checkpoints::{Failure, Service},
    context::{Audience, EngineContext},
    fold_requests::{check_keepable, claim, Claim, Store},
    jobs,
    runtime::RuntimeHost,
};

pub fn journal_for(context: &dyn EngineContext) -> Journal {
    Journal::new(context.paths().app_data.join("changes").join("keep"))
}

pub(crate) fn start<F>(
    host: Arc<dyn RuntimeHost>,
    method: &'static str,
    work: F,
) -> Result<JobId, Failure>
where
    F: FnOnce(&Arc<dyn RuntimeHost>) -> Result<serde_json::Value, String> + Send + 'static,
{
    // The checkpoint code a keep calls needs a Tokio runtime, so the job runs on the engine's
    // own blocking pool rather than a bare thread.
    let runtime = tokio::runtime::Handle::try_current()
        .map_err(|_| Failure::from_message("The engine has no runtime to run the keep on."))?;
    let context = host.engine_context();
    let registry = context.job_registry().map_err(Failure::from_message)?;
    let accepted = jobs::allocate(context.as_ref(), method)?;
    let job_id = accepted.job_id;
    registry
        .mark_running(&job_id)
        .map_err(|error| Failure::from_message(error.to_string()))?;
    let id = job_id.clone();
    runtime.spawn_blocking(move || {
        // A panic fails the job with its message instead of leaving it running.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&host)))
            .unwrap_or_else(|panic| {
                let detail = panic
                    .downcast_ref::<&str>()
                    .map(|text| (*text).to_owned())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                Err(format!("The {method} stopped unexpectedly: {detail}"))
            });
        let finished = match outcome {
            Ok(value) => jobs::finish_with_result(context.as_ref(), id, value, Audience::All),
            Err(message) => {
                let failure = Failure::from_message(message);
                jobs::finish_with_error(
                    context.as_ref(),
                    id,
                    failure.code,
                    failure.message,
                    Audience::All,
                )
            }
        };
        if let Err(error) = finished {
            eprintln!("silo-engine: could not persist the {method} outcome: {error}");
        }
    });
    Ok(job_id)
}

/// Appends the packages and settings files a keep brought to the recipe of the computer it
/// merged into, and says so in the report. A failure is a note: the merge is done.
fn note_recipe(context: &dyn EngineContext, report: &mut super::Report) {
    let library = crate::recipes::Library::new(&context.paths().app_data);
    let target = report.target.clone();
    match crate::recipes::record_merge(&library, &target, &report.additions) {
        Ok(Some((name, added))) => {
            report.recipe = Some(super::RecipeUpdate {
                name,
                packages: added.packages,
                files: added.files,
            });
        }
        Ok(None) => {}
        Err(error) => report.notes.push(format!(
            "The recipe of {target} could not be updated: {error}"
        )),
    }
}

/// `changes.keep`.
pub fn start_keep(
    host: Arc<dyn RuntimeHost>,
    service: Service,
    store: Arc<dyn ManifestStore>,
    request: Request,
) -> Result<JobId, Failure> {
    start(host, "changes.keep", move |host| {
        let context = host.engine_context();
        let journal = journal_for(context.as_ref());
        let env = GuestEnv::new(host.clone(), service, journal.clone(), Box::new(|_| {}));
        let outcome = super::keep(
            &env as &dyn KeepEnv,
            store.as_ref(),
            &journal,
            &request,
            LANE_WAIT,
        )?;
        let mut outcome = outcome;
        if let super::Outcome::Kept(report) = &mut outcome {
            note_recipe(context.as_ref(), report);
            // The changes the open requests for this fork asked to review are in.
            Store::new(&context.paths().app_data).mark_kept_for(
                &report.fork,
                &report.target,
                report.checkpoint.as_deref(),
            );
        }
        Ok(outcome.to_json())
    })
}

/// The result of `changes.requests.keep`: the keep's own result and the request after it.
pub fn request_keep_result(
    outcome: &super::Outcome,
    request: &FoldRequest,
) -> Result<serde_json::Value, String> {
    let mut result = outcome.to_json();
    result["request"] = serde_json::to_value(request).map_err(|error| error.to_string())?;
    Ok(result)
}

/// Settles a request whose keep was started, for the job that already holds the request.
/// When the keep journal shows that keep finished (a keep of its fork into its target, pinned
/// to its digest, that succeeded after the request's keep began, even if it was undone since),
/// the request is marked kept and returned. When no such keep exists and none is pending for
/// the target, the keep was rolled back or never got going, so the request is open again.
pub fn settle_held(
    requests: &Store,
    journal: &Journal,
    request: &FoldRequest,
) -> Option<FoldRequest> {
    if request.status != FoldRequestStatus::Keeping {
        return None;
    }
    let since = u128::from(request.keeping_since_ms.unwrap_or(0));
    match journal.finished_keep(
        &request.target,
        &request.fork_id,
        &request.review_digest,
        since,
    ) {
        Some(checkpoint) => requests.mark_kept(&request.id, Some(checkpoint.id)).ok(),
        None => {
            if !journal.has_pending(&request.target) {
                let _ = requests.reopen(&request.id);
            }
            None
        }
    }
}

/// [`settle_held`] for a request no job holds: it is taken for the duration, and left as it
/// is when another job has it.
pub fn settle_request(
    requests: &Store,
    journal: &Journal,
    request: &FoldRequest,
) -> Option<FoldRequest> {
    if request.status != FoldRequestStatus::Keeping {
        return None;
    }
    let _claim = claim(&request.id).ok()?;
    settle_held(requests, journal, request)
}

/// What a keep of a request starts from.
#[derive(Debug, PartialEq)]
pub enum Start {
    /// The request's keep had finished; it is now kept and nothing is kept again.
    Settled(FoldRequest),
    /// Keep now.
    Keep,
}

/// The first step of a request's keep job, which holds the request: it must be keepable, and
/// a keep of it that already finished is settled instead of being run a second time.
pub fn begin_request_keep(
    requests: &Store,
    journal: &Journal,
    request_id: &str,
) -> Result<Start, String> {
    let current = requests.keepable_request(request_id)?;
    check_keepable(&current)?;
    Ok(match settle_held(requests, journal, &current) {
        Some(settled) => Start::Settled(settled),
        None => Start::Keep,
    })
}

/// [`settle_request`] for every request whose keep was started.
pub fn settle_all(context: &dyn EngineContext) -> Vec<FoldRequest> {
    let requests = Store::new(&context.paths().app_data);
    let journal = journal_for(context);
    requests
        .list()
        .iter()
        .filter_map(|request| settle_request(&requests, &journal, request))
        .collect()
}

/// `changes.requests.keep`: the keep of an open review request's fork, pinned to the request's
/// digest through `request.reviewed`, run while `hold` keeps any other job off the request.
/// The request is marked `keeping` before the keep starts, so a crash between the keep and
/// the mark leaves a record the next keep settles from the keep journal instead of keeping
/// twice. A kept outcome marks the request kept with the target's before-keep checkpoint; any
/// other outcome opens it again. The job's result is the keep's result with the request.
pub fn start_request_keep(
    host: Arc<dyn RuntimeHost>,
    service: Service,
    store: Arc<dyn ManifestStore>,
    request: Request,
    request_id: String,
    hold: Claim,
) -> Result<JobId, Failure> {
    start(host, "changes.requests.keep", move |host| {
        let _hold = hold;
        let context = host.engine_context();
        let journal = journal_for(context.as_ref());
        let requests = Store::new(&context.paths().app_data);
        let current = requests.keepable_request(&request_id)?;
        let target = current.target.clone();
        if let Start::Settled(settled) = begin_request_keep(&requests, &journal, &request_id)? {
            let report = super::Report {
                fork: settled.fork_id.clone(),
                target,
                notes: vec![
                    "A keep of this request had already finished; the request is now marked kept."
                        .to_owned(),
                ],
                checkpoint: settled.keep_id.clone(),
                review_digest: Some(settled.review_digest.clone()),
                ..Default::default()
            };
            return request_keep_result(&super::Outcome::Kept(report), &settled);
        }
        requests.mark_keeping(
            &request_id,
            super::super::promote::journal::now_millis() as u64,
        )?;
        let env = GuestEnv::new(host.clone(), service, journal.clone(), Box::new(|_| {}));
        let outcome = super::keep(
            &env as &dyn KeepEnv,
            store.as_ref(),
            &journal,
            &request,
            LANE_WAIT,
        );
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                // A keep that stopped midway is put back by recovery; only a failure that
                // left no journal changed nothing.
                if !journal.has_pending(&target) {
                    let _ = requests.reopen(&request_id);
                }
                return Err(error);
            }
        };
        let mut outcome = outcome;
        if let super::Outcome::Kept(report) = &mut outcome {
            note_recipe(context.as_ref(), report);
        }
        let after = match &outcome {
            super::Outcome::Kept(report) => requests
                .mark_kept(&request_id, report.checkpoint.clone())
                .map_err(|error| format!("The changes were kept, but {error}"))?,
            _ => requests.reopen(&request_id)?,
        };
        request_keep_result(&outcome, &after)
    })
}
/// `changes.undo`.
pub fn start_undo(
    host: Arc<dyn RuntimeHost>,
    service: Service,
    store: Arc<dyn ManifestStore>,
    computer: String,
) -> Result<JobId, Failure> {
    start(host, "changes.undo", move |host| {
        let journal = journal_for(host.engine_context().as_ref());
        let env = GuestEnv::new(host.clone(), service, journal.clone(), Box::new(|_| {}));
        super::undo(
            &env as &dyn KeepEnv,
            store.as_ref(),
            &journal,
            &computer,
            LANE_WAIT,
        )
        .map(|outcome| outcome.to_json())
    })
}
