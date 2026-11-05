//! `changes.*`: capture a computer's state and compare it with its fork-time base.

use std::sync::Arc;

use serde_json::Value;
use silo_api::methods::{
    changes::{ChangesBaselineParams, ChangesCaptureParams, ChangesDiffParams, ChangesRole},
    common::JobStartResult,
};
use silo_engine::changes::{
    collect::TargetRunner,
    service::{self, Work},
};

use crate::dispatcher::{DispatchError, HandlerOutput};

use super::{
    super::guest::{guest_error, resolve_target},
    blocking, engine_context, output, require_local_device, HandlerContext,
};

pub(super) async fn capture(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesCaptureParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    start(
        context,
        "changes.capture",
        params.computer_id,
        Work::Capture {
            role: params.role.unwrap_or(ChangesRole::Latest),
            source: params.source,
        },
    )
    .await
}

pub(super) async fn diff(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesDiffParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    start(
        context,
        "changes.diff",
        params.computer_id,
        Work::Diff {
            max_lines: params.max_lines,
            baseline: params.baseline.unwrap_or(true),
            refresh: params.refresh.unwrap_or(true),
        },
    )
    .await
}

pub(super) async fn baseline(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChangesBaselineParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    start(
        context,
        "changes.baseline",
        params.computer_id,
        Work::Baseline {
            source: params.source,
            idle_seconds: params.idle_seconds,
        },
    )
    .await
}

/// Refuses a computer that is not running before a job exists, then runs the work as a job.
async fn start(
    context: &HandlerContext,
    method: &'static str,
    computer_id: String,
    work: Work,
) -> Result<HandlerOutput, DispatchError> {
    let silod = Arc::new(context.silod_context().clone());
    let engine = engine_context(context);
    let job_id = blocking(move || {
        let target = resolve_target(&silod, &computer_id).map_err(guest_error)?;
        target.ensure_running().map_err(guest_error)?;
        service::start(
            engine,
            Arc::new(TargetRunner(target)),
            computer_id,
            method,
            work,
        )
        .map_err(guest_error)
    })
    .await?;
    output(JobStartResult { job_id })
}
