//! Handlers for `checkpoints.*`, `exports.*`, `migration.*`, `preparation.*` and `device.*`.
//!
//! Each method decodes its `silo-api` params, runs the engine behavior and returns the
//! `silo-api` result. Long operations are durable jobs (`{jobId}`) whose outcome and
//! progress the engine publishes on the `operations.job.*` topics; state changes publish
//! the domain's catalogue topic (`checkpoints.changed`, `exports.changed`, ...).
//! Requests naming another device belong to the Connections router and are refused here.

mod device;
mod exports;
mod migration;
mod preparation;

use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use silo_api::{
    jobs::ErrorCode,
    methods::{
        checkpoints::{
            CheckpointCreateParams, CheckpointForkParams, CheckpointTargetParams,
            CheckpointUsageParams,
        },
        common::{ComputerTargetParams, JobStartResult},
        MethodName,
    },
};
use silo_engine::{
    checkpoints::{Failure, Service},
    context::EngineContext,
    macos_computers_engine::{MacosContext, MacosHost},
};

use crate::dispatcher::{method_name, DispatchError, HandlerOutput};

use super::{ownership::owns_prefix, HandlerContext};

pub(super) fn owns(method: MethodName) -> bool {
    owns_prefix(
        method,
        &[
            "checkpoints.",
            "exports.",
            "migration.",
            "preparation.",
            "device.",
        ],
    )
}

pub async fn handle(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Option<Result<HandlerOutput, DispatchError>> {
    if owns(method) {
        Some(dispatch(method, params, context).await)
    } else {
        None
    }
}

async fn dispatch(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    match method {
        MethodName::CheckpointsCreate => create(context, params).await,
        MethodName::CheckpointsRestore => restore(context, params).await,
        MethodName::CheckpointsFork => fork(context, params).await,
        MethodName::CheckpointsDelete => delete(context, params).await,
        MethodName::CheckpointsAbandonRestore => abandon_restore(context, params).await,
        MethodName::CheckpointsUsage => usage(context, params).await,
        MethodName::ExportsState => exports::state(context, params).await,
        MethodName::ExportsDestinationSet => exports::destination_set(context, params).await,
        MethodName::ExportsInspect => exports::inspect(context, params).await,
        MethodName::ExportsCreate => exports::create(context, params).await,
        MethodName::ExportsImport => exports::import(context, params).await,
        MethodName::ExportsDismiss => exports::dismiss(context, params).await,
        MethodName::ExportsAcknowledge => exports::acknowledge(context, params).await,
        MethodName::MigrationState => migration::state(context, params).await,
        MethodName::MigrationPreUpgradeBackupGet => migration::backup_get(context, params).await,
        MethodName::MigrationPreUpgradeBackupMeasure => {
            migration::backup_measure(context, params).await
        }
        MethodName::MigrationPreUpgradeBackupDelete => {
            migration::backup_delete(context, params).await
        }
        MethodName::MigrationPreUpgradeBackupAcknowledgeNotice => {
            migration::backup_acknowledge_notice(context, params).await
        }
        MethodName::PreparationStatus => preparation::status(context, params).await,
        MethodName::PreparationRetry => preparation::retry(context, params).await,
        MethodName::PreparationChatgptStatus => preparation::chatgpt_status(context, params).await,
        MethodName::PreparationChatgptRetry => preparation::chatgpt_retry(context, params).await,
        MethodName::DeviceDependencies => device::dependencies(context, params).await,
        MethodName::DeviceFacts => device::facts(context, params).await,
        _ => Err(DispatchError::unsupported(method_name(method))),
    }
}

// MARK: Shared helpers

pub(super) fn output<T: Serialize>(value: T) -> Result<HandlerOutput, DispatchError> {
    serde_json::to_value(value)
        .map(|value| HandlerOutput {
            value,
            event_barrier: None,
        })
        .map_err(|error| DispatchError::internal(error.to_string()))
}

pub(super) fn failure(failure: Failure) -> DispatchError {
    DispatchError {
        code: failure.code,
        message: failure.message,
    }
}

/// Maps an engine message onto the canonical codes the workflows are documented to return.
pub(super) fn message_error(message: impl Into<String>) -> DispatchError {
    failure(Failure::from_message(message))
}

/// Routing a request to another device is the Connections router's job (P3-connections).
pub(super) fn require_local_device(
    context: &HandlerContext,
    device_id: Option<&str>,
) -> Result<(), DispatchError> {
    if device_id.is_some_and(|device| device != context.silod_context().local_device().device_id) {
        return Err(DispatchError {
            code: ErrorCode::UnsupportedRemoteOperation,
            message: "This engine does not route requests to remote devices.".to_owned(),
        });
    }
    Ok(())
}

/// Runs blocking engine work off the async workers.
pub(super) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, DispatchError> + Send + 'static,
) -> Result<T, DispatchError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| DispatchError::internal("The engine task stopped unexpectedly."))?
}

pub(super) fn engine_context(context: &HandlerContext) -> Arc<dyn EngineContext> {
    Arc::new(context.silod_context().clone())
}

/// The macOS side reaches no display window from checkpoint work; the Linux names it
/// reserves fork names against come from this device's inventory.
struct CheckpointMacosHost {
    host: Arc<dyn silo_engine::runtime::RuntimeHost>,
}

impl MacosHost for CheckpointMacosHost {
    fn show_display(
        &self,
        _context: &MacosContext,
        _id: &str,
        _title: &str,
        _toolbar: bool,
    ) -> Result<(), String> {
        Err("This operation does not open a display.".to_owned())
    }

    fn close_display(&self, _id: &str) {}

    fn linux_computer_names(&self) -> Vec<String> {
        silo_engine::runtime::computer_names(self.host.as_ref())
    }
}

pub(super) fn macos_context(context: &HandlerContext) -> MacosContext {
    MacosContext::new(
        engine_context(context),
        Arc::new(CheckpointMacosHost {
            host: context.silod_context().runtime_host(),
        }),
    )
}

/// The two-platform checkpoint service, reading the owner's retention setting from the
/// engine's settings each time it is asked.
pub(super) fn service(context: &HandlerContext) -> Service {
    let engine = engine_context(context);
    Service::new(
        context.silod_context().runtime_host(),
        macos_context(context),
        Arc::new(move || {
            silo_engine::settings::automatic_checkpoint_retention_count_for(engine.as_ref())
        }),
    )
}

fn job_started(
    result: Result<silo_api::jobs::JobId, Failure>,
) -> Result<HandlerOutput, DispatchError> {
    result
        .map(|job_id| JobStartResult { job_id })
        .map_err(failure)
        .and_then(output)
}

// MARK: checkpoints.*

async fn create(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let params: CheckpointCreateParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let service = service(context);
    job_started(
        blocking_failure(move || {
            service.start_create(&params.computer_id, &params.name, params.kind)
        })
        .await,
    )
}

async fn restore(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let params: CheckpointTargetParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let service = service(context);
    job_started(
        blocking_failure(move || service.start_restore(&params.computer_id, &params.checkpoint_id))
            .await,
    )
}

async fn fork(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let params: CheckpointForkParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let service = service(context);
    job_started(
        blocking_failure(move || {
            service.start_fork(
                &params.computer_id,
                params.checkpoint_id.as_deref(),
                &params.new_name,
            )
        })
        .await,
    )
}

async fn delete(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let params: CheckpointTargetParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let service = service(context);
    job_started(
        blocking_failure(move || service.start_delete(&params.computer_id, &params.checkpoint_id))
            .await,
    )
}

async fn abandon_restore(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ComputerTargetParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let service = service(context);
    job_started(blocking_failure(move || service.start_abandon_restore(&params.computer_id)).await)
}

async fn usage(context: &HandlerContext, params: &Value) -> Result<HandlerOutput, DispatchError> {
    let params: CheckpointUsageParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let service = service(context);
    blocking_failure(move || service.usage(&params.computer_id))
        .await
        .map_err(failure)
        .and_then(output)
}

/// Starting a job accepts it durably and spawns its worker, which needs the async
/// runtime's handle; the blocking section also keeps inventory reads off the workers.
async fn blocking_failure<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, Failure> {
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let _entered = handle.enter();
        work()
    })
    .await
    .map_err(|_| Failure::new(ErrorCode::Internal, "The engine task stopped unexpectedly."))?
}

#[cfg(test)]
mod tests;
