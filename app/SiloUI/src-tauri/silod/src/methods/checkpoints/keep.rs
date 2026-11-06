//! `changes.keep` and `changes.undo`: keep a fork's changes into the computer it came from, or
//! put that computer back. Both are durable jobs; their outcome is the engine's report.

use serde_json::Value;
use silo_api::methods::{
    common::JobStartResult,
    keep::{
        ChangesKeepParams, ChangesPromoteParams, ChangesRequestParams, ChangesRequestResult,
        ChangesUndoParams,
    },
};
use silo_engine::{
    changes::{
        keep::{service, store, Hooks, Request},
        promote,
    },
    context::EngineContext,
    fold_requests,
};

use crate::dispatcher::{DispatchError, HandlerOutput};

use super::{
    failure, message_error, output, require_local_device, service as checkpoint_service,
    HandlerContext,
};

pub(super) async fn keep(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesKeepParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
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
    require_local_device(context, params.device_id.as_deref())?;
    // An undo swaps two computers, so both lose their tokens.
    let swapped = crate::methods::guest_client::swap_participants(context, &params.computer_id);
    let swapped: Vec<&str> = swapped.iter().map(String::as_str).collect();
    crate::methods::guest_client::revoke_for(context, &swapped)?;
    let host = context.silod_context().runtime_host();
    let checkpoints = checkpoint_service(context);
    let manifests = store::production(host.clone());
    let started = tokio::task::spawn_blocking(move || {
        promote::service::start_undo(host, checkpoints, manifests, params.computer_id)
    })
    .await
    .map_err(|_| DispatchError::internal("The undo could not start."))?;
    started
        .map(|job_id| JobStartResult { job_id })
        .map_err(failure)
        .and_then(output)
}

pub(super) async fn promote(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesPromoteParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    // Which computer is main changes, so the agents of both computers get new tokens on purpose.
    let source = super::requests::fork_source(context, &params.fork_id).await?;
    let target = params.target_id.clone().or(source);
    let mut swapped = vec![params.fork_id.as_str()];
    swapped.extend(target.as_deref());
    crate::methods::guest_client::revoke_for(context, &swapped)?;
    let host = context.silod_context().runtime_host();
    let request = promote::Request {
        fork: params.fork_id,
        target: params.target_id,
        dry_run: params.dry_run.unwrap_or(false),
    };
    let checkpoints = checkpoint_service(context);
    let manifests = store::production(host.clone());
    let started = tokio::task::spawn_blocking(move || {
        promote::service::start_promote(host, checkpoints, manifests, request)
    })
    .await
    .map_err(|_| DispatchError::internal("The promote could not start."))?;
    started
        .map(|job_id| JobStartResult { job_id })
        .map_err(failure)
        .and_then(output)
}

/// `changes.request`: records that a fork waits for a review. Nothing is kept or changed.
pub(super) async fn request(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesRequestParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let target = match params.target_id.filter(|id| !id.is_empty()) {
        Some(target) => target,
        None => super::requests::fork_source(context, &params.fork_id)
            .await?
            .ok_or_else(|| {
                message_error(format!(
                    "{} has no recorded source; name the computer it would be kept into.",
                    params.fork_id
                ))
            })?,
    };
    // A request is made only for a fork of its target: a later keep or throw away acts on it.
    super::requests::verify_fork(context, &params.fork_id, &target).await?;
    let app_data = context.silod_context().paths().app_data.clone();
    let request = tokio::task::spawn_blocking(move || {
        fold_requests::Store::new(&app_data).create(fold_requests::NewRequest {
            fork_id: params.fork_id,
            target,
            review_digest: params.review_digest.unwrap_or_default(),
            summary: params.summary.unwrap_or_default(),
            counts: params.counts.unwrap_or_default(),
        })
    })
    .await
    .map_err(|_| DispatchError::internal("The request could not be recorded."))?
    .map_err(DispatchError::internal)?;
    output(ChangesRequestResult { request })
}
