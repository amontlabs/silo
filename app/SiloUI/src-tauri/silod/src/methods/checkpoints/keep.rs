//! `changes.keep` and `changes.undo`: keep a fork's changes into the computer it came from, or
//! put that computer back. Both are durable jobs; their outcome is the engine's report.

use serde_json::Value;
use silo_api::methods::{
    common::JobStartResult,
    keep::{
        ChangesKeepParams, ChangesPromoteParams, ChangesRequestParams, ChangesRequestResult,
        ChangesUndoParams, KeepAnswer,
    },
};
use silo_engine::{
    changes::{
        keep::{service, store, Answer, Choices, Hooks, Request},
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

/// The caller's answers and skips, as the engine reads them.
pub(super) fn choices_of(
    answers: Option<std::collections::BTreeMap<String, KeepAnswer>>,
    skip: Option<Vec<String>>,
) -> Choices {
    Choices {
        answers: answers
            .unwrap_or_default()
            .into_iter()
            .map(|(id, answer)| {
                let answer = match answer {
                    KeepAnswer::Fork => Answer::Fork,
                    KeepAnswer::Main => Answer::Main,
                    KeepAnswer::Branch => Answer::Branch,
                };
                (id, answer)
            })
            .collect(),
        skip: skip.unwrap_or_default().into_iter().collect(),
    }
}

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
    let save_recipe = matches!(
        context.caller(),
        silo_engine::policy::Caller::Owner { .. } | silo_engine::policy::Caller::Device { .. }
    );
    let request = Request {
        fork: params.fork_id,
        target: params.target_id,
        hooks: hooks.unwrap_or_default(),
        dry_run: params.dry_run.unwrap_or(false),
        data_ok: params.data_ok.unwrap_or(false),
        reviewed: params.review_digest,
        choices: choices_of(params.choices, params.skip),
        save_recipe,
        hide_recipe_commands: crate::methods::approvals::result_goes_to_computer(context),
        ..Request::default()
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
    let pin = crate::methods::approvals::expected_pin();
    let started = tokio::task::spawn_blocking(move || {
        promote::service::start_undo(host, checkpoints, manifests, params.computer_id, pin)
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
    let pin = crate::methods::approvals::expected_pin();
    let started = tokio::task::spawn_blocking(move || {
        promote::service::start_promote(host, checkpoints, manifests, request, pin)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_and_skips_reach_the_engine_as_the_caller_wrote_them() {
        let choices = choices_of(
            Some(
                [
                    ("repo:/root/app".to_owned(), KeepAnswer::Branch),
                    ("file:/etc/x".to_owned(), KeepAnswer::Fork),
                    ("package:dpkg:jq".to_owned(), KeepAnswer::Main),
                ]
                .into(),
            ),
            Some(vec!["cron:root".to_owned()]),
        );
        assert_eq!(choices.answers["repo:/root/app"], Answer::Branch);
        assert_eq!(choices.answers["file:/etc/x"], Answer::Fork);
        assert_eq!(choices.answers["package:dpkg:jq"], Answer::Main);
        assert!(choices.skip.contains("cron:root"));
        assert!(choices_of(None, None).is_empty());
    }
}
