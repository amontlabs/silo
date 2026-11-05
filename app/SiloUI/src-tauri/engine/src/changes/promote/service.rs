//! The `changes.promote` job, and the promote side of `changes.undo`.

use std::sync::Arc;

use silo_api::jobs::JobId;

use super::{engine_env::EngineEnv, Journal, Request, LANE_WAIT};
use crate::{
    changes::keep::{self, env::ManifestStore, service as keep_service},
    checkpoints::{Failure, Service},
    context::EngineContext,
    runtime::RuntimeHost,
};

pub fn journal_for(context: &dyn EngineContext) -> Journal {
    Journal::new(context.paths().app_data.join("changes").join("promote"))
}

/// `changes.promote`.
pub fn start_promote(
    host: Arc<dyn RuntimeHost>,
    service: Service,
    store: Arc<dyn ManifestStore>,
    request: Request,
) -> Result<JobId, Failure> {
    keep_service::start(host, "changes.promote", move |host| {
        let context = host.engine_context();
        let journal = journal_for(context.as_ref());
        let env = EngineEnv::new(
            host.clone(),
            service,
            keep_service::journal_for(context.as_ref()),
        );
        super::promote(&env, store.as_ref(), &journal, &request, LANE_WAIT)
            .map(|outcome| outcome.to_json())
    })
}

/// Swaps a promote back when `computer` is part of one; the keep undo runs otherwise.
pub fn start_undo(
    host: Arc<dyn RuntimeHost>,
    service: Service,
    store: Arc<dyn ManifestStore>,
    computer: String,
) -> Result<JobId, Failure> {
    keep_service::start(host, "changes.undo", move |host| {
        let context = host.engine_context();
        let journal = journal_for(context.as_ref());
        let keep_journal = keep_service::journal_for(context.as_ref());
        let env = EngineEnv::new(host.clone(), service.clone(), keep_journal.clone());
        if let Some(done) = super::undo(&env, &journal, &computer, LANE_WAIT)? {
            return Ok(done.to_json());
        }
        let guest = keep::guest_env::GuestEnv::new(
            host.clone(),
            service,
            keep_journal.clone(),
            Box::new(|_| {}),
        );
        keep::undo(
            &guest as &dyn keep::KeepEnv,
            store.as_ref(),
            &keep_journal,
            &computer,
            keep::LANE_WAIT,
        )
        .map(|outcome| outcome.to_json())
    })
}

/// Settles every promote or undo the engine stopped in the middle of; returns the failures.
pub fn recover_interrupted(host: Arc<dyn RuntimeHost>, service: Service) -> Vec<String> {
    let context = host.engine_context();
    let journal = journal_for(context.as_ref());
    let env = EngineEnv::new(host, service, keep_service::journal_for(context.as_ref()));
    super::recover_all(&env, &journal)
}
