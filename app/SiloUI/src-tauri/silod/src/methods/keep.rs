//! Handlers for `changes.keep` and `changes.undo`: keep a fork's changes into the computer it
//! came from, or put that computer back. Both are durable jobs; their outcome is the engine's
//! keep or undo report.

use serde_json::Value;
use silo_api::methods::{
    common::JobStartResult,
    keep::{ChangesKeepParams, ChangesUndoParams},
    MethodName,
};
use silo_engine::changes::keep::{self, service, store, Hooks, Request};

use super::{
    checkpoints::{failure, output, service as checkpoint_service},
    ownership::owns_prefix,
    HandlerContext,
};
use crate::dispatcher::{DispatchError, HandlerOutput};

pub(super) fn owns(method: MethodName) -> bool {
    matches!(method, MethodName::ChangesKeep | MethodName::ChangesUndo)
        && owns_prefix(method, &["changes."])
}

pub async fn handle(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Option<Result<HandlerOutput, DispatchError>> {
    match method {
        MethodName::ChangesKeep => Some(keep_job(params, context).await),
        MethodName::ChangesUndo => Some(undo_job(params, context).await),
        _ => None,
    }
}

async fn keep_job(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesKeepParams = context.decode_params(params)?;
    let host = context.silod_context().runtime_host();
    let engine = host.engine_context();
    let hooks = params.keep.map(|h| Hooks {
        restart: h.restart,
        migrate: h.migrate,
        after: h.after,
    });
    let request = Request {
        fork: params.fork_id,
        target: params.target_id,
        hooks: hooks.unwrap_or_default(),
        dry_run: params.dry_run.unwrap_or(false),
    };
    let checkpoints = checkpoint_service(context);
    let manifests = store::production(engine.as_ref());
    let started = tokio::task::spawn_blocking(move || {
        service::start_keep(host, checkpoints, manifests, request)
    })
    .await
    .map_err(|_| DispatchError::internal("The keep could not start."))?;
    started
        .map(|job_id| JobStartResult { job_id })
        .map_err(failure)
        .and_then(output)
}

async fn undo_job(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesUndoParams = context.decode_params(params)?;
    let host = context.silod_context().runtime_host();
    let checkpoints = checkpoint_service(context);
    let started = tokio::task::spawn_blocking(move || {
        service::start_undo(host, checkpoints, params.computer_id)
    })
    .await
    .map_err(|_| DispatchError::internal("The undo could not start."))?;
    started
        .map(|job_id| JobStartResult { job_id })
        .map_err(failure)
        .and_then(output)
}

/// Puts back any main a keep left half-changed when the engine last stopped.
#[allow(dead_code)]
pub fn recover(context: &HandlerContext) -> Vec<String> {
    let host = context.silod_context().runtime_host();
    let journal = service::journal_for(host.engine_context().as_ref());
    let env = keep::guest_env::GuestEnv::new(
        host,
        checkpoint_service(context),
        journal.clone(),
        Box::new(|_| {}),
    );
    keep::recover_all(&env, &journal)
}
