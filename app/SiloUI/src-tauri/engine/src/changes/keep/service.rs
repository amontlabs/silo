//! The `changes.keep` and `changes.undo` jobs.

use std::sync::Arc;

use silo_api::jobs::JobId;

use super::{
    env::{KeepEnv, ManifestStore},
    guest_env::GuestEnv,
    journal::Journal,
    Request, LANE_WAIT,
};
use crate::{
    checkpoints::{Failure, Service},
    context::{Audience, EngineContext},
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

/// `changes.keep`.
pub fn start_keep(
    host: Arc<dyn RuntimeHost>,
    service: Service,
    store: Arc<dyn ManifestStore>,
    request: Request,
) -> Result<JobId, Failure> {
    start(host, "changes.keep", move |host| {
        let journal = journal_for(host.engine_context().as_ref());
        let env = GuestEnv::new(host.clone(), service, journal.clone(), Box::new(|_| {}));
        super::keep(
            &env as &dyn KeepEnv,
            store.as_ref(),
            &journal,
            &request,
            LANE_WAIT,
        )
        .map(|outcome| outcome.to_json())
    })
}

/// `changes.requests.keep`: the keep of an open review request's fork, pinned to the request's
/// digest through `request.reviewed`. A kept outcome marks the request kept with the target's
/// before-keep checkpoint; a refusal leaves it open. The job's result is the keep's result
/// with the request after it.
pub fn start_request_keep(
    host: Arc<dyn RuntimeHost>,
    service: Service,
    store: Arc<dyn ManifestStore>,
    request: Request,
    request_id: String,
) -> Result<JobId, Failure> {
    start(host, "changes.requests.keep", move |host| {
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
        let requests = crate::fold_requests::Store::new(&context.paths().app_data);
        let after = match &outcome {
            super::Outcome::Kept(report) => requests
                .mark_kept(&request_id, report.checkpoint.clone())
                .map_err(|error| format!("The changes were kept, but {error}"))?,
            _ => requests.open_request(&request_id)?,
        };
        let mut result = outcome.to_json();
        result["request"] = serde_json::to_value(after).map_err(|error| error.to_string())?;
        Ok(result)
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
