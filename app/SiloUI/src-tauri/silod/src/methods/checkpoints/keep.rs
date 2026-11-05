//! `changes.keep` and `changes.undo`: keep a fork's changes into the computer it came from, or
//! put that computer back. Both are durable jobs; their outcome is the engine's report.

use serde_json::Value;
use silo_api::methods::{
    common::JobStartResult,
    keep::{ChangesKeepParams, ChangesUndoParams},
};
use silo_engine::changes::keep::{service, store, Hooks, Request};

use crate::dispatcher::{DispatchError, HandlerOutput};

use super::{failure, output, service as checkpoint_service, HandlerContext};

pub(super) async fn keep(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesKeepParams = context.decode_params(params)?;
    let host = context.silod_context().runtime_host();
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
        data_ok: params.data_ok.unwrap_or(false),
        reviewed: params.review_digest,
    };
    let checkpoints = checkpoint_service(context);
    let manifests = store::production(host.clone());
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

pub(super) async fn undo(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesUndoParams = context.decode_params(params)?;
    let host = context.silod_context().runtime_host();
    let checkpoints = checkpoint_service(context);
    let manifests = store::production(host.clone());
    let started = tokio::task::spawn_blocking(move || {
        service::start_undo(host, checkpoints, manifests, params.computer_id)
    })
    .await
    .map_err(|_| DispatchError::internal("The undo could not start."))?;
    started
        .map(|job_id| JobStartResult { job_id })
        .map_err(failure)
        .and_then(output)
}
