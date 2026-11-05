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

fn start<F>(host: Arc<dyn RuntimeHost>, method: &'static str, work: F) -> Result<JobId, Failure>
where
    F: FnOnce(&Arc<dyn RuntimeHost>) -> Result<serde_json::Value, String> + Send + 'static,
{
    let context = host.engine_context();
    let registry = context.job_registry().map_err(Failure::from_message)?;
    let accepted = jobs::allocate(context.as_ref(), method)?;
    let job_id = accepted.job_id;
    registry
        .mark_running(&job_id)
        .map_err(|error| Failure::from_message(error.to_string()))?;
    let id = job_id.clone();
    std::thread::spawn(move || {
        let finished = match work(&host) {
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
