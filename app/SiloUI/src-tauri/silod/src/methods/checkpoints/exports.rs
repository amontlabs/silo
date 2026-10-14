//! `exports.*`: export and import of computers, archive inspection and the remembered
//! destination, over the engine's export controller (`backup_controller`).

use std::{path::PathBuf, sync::Arc};

use serde_json::Value;
use silo_api::{
    jobs::{ErrorCode, JobId},
    methods::{
        common::JobStartResult,
        exports::{
            ExportAcknowledgeParams, ExportCreateParams, ExportDestinationResult,
            ExportDestinationSetParams, ExportDismissParams, ExportImportParams,
            ExportInspectParams, ExportStateParams,
        },
    },
};
use silo_engine::{backup_controller as engine, checkpoints::Platform};

use crate::dispatcher::{DispatchError, HandlerOutput};

use super::{blocking, message_error, output, require_local_device, service, HandlerContext};

/// The export controller, installed on first use. It settles an interrupted operation's
/// journal as it installs, before any new operation can begin.
fn controller(context: &HandlerContext) -> Result<Arc<engine::Controller>, DispatchError> {
    context
        .silod_context()
        .backup_controller()
        .map_err(message_error)
}

fn exports_job(operation_id: &str) -> Result<JobStartResult, DispatchError> {
    JobId::new(format!("exports:{operation_id}"))
        .map(|job_id| JobStartResult { job_id })
        .map_err(|error| DispatchError::internal(error.to_string()))
}

pub(super) async fn state(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ExportStateParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let controller = controller(context)?;
    let state = blocking(move || engine::api_state(&controller).map_err(message_error)).await?;
    output(state)
}

pub(super) async fn destination_set(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ExportDestinationSetParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let controller = controller(context)?;
    let result = blocking(move || {
        let refused = |reason: String| {
            DispatchError::internal(format!(
                "Silo could not use the selected destination: {reason}"
            ))
        };
        let path =
            std::fs::canonicalize(&params.path).map_err(|error| refused(error.to_string()))?;
        if !path.is_dir() {
            return Err(refused("it is not a folder.".to_owned()));
        }
        let destination = engine::selected_path_text(&path).map_err(message_error)?;
        engine::remember_destination(&controller, path);
        engine::publish(&controller);
        Ok(ExportDestinationResult { destination })
    })
    .await?;
    output(result)
}

pub(super) async fn inspect(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ExportInspectParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let controller = controller(context)?;
    let request_id = params
        .request_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let job_id = JobId::new(format!("inspect:{request_id}"))
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    // Repeating a request id returns the job it already has; nothing is started or cancelled.
    if silo_engine::jobs::get_at(context.engine_context(), &job_id, None)
        .map_err(HandlerContext::map_registry_error)?
        .is_some()
    {
        return output(JobStartResult { job_id });
    }
    // The worker accepts the durable job as it is built. When acceptance fails there is no
    // job, so the refusal is returned instead of an id that would never finish.
    let worker =
        engine::inspection_worker(controller, PathBuf::from(params.archive_path), request_id);
    if silo_engine::jobs::get_at(context.engine_context(), &job_id, None)
        .map_err(HandlerContext::map_registry_error)?
        .is_none()
    {
        let refusal = blocking(move || worker().map(|_| ()).map_err(message_error)).await;
        return Err(refusal.err().unwrap_or_else(|| {
            DispatchError::internal("The archive inspection could not be registered.")
        }));
    }
    tokio::task::spawn_blocking(worker);
    output(JobStartResult { job_id })
}

pub(super) async fn create(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ExportCreateParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let controller = controller(context)?;
    let service = service(context);
    let operation_id = blocking(move || {
        for computer_id in &params.computer_ids {
            // The archive format carries Linux snapshots only; a macOS computer has no
            // export mechanism yet (20 Q4), so it is refused by name, not mistaken for missing.
            if service.platform(computer_id) == Ok(Platform::Macos) {
                return Err(DispatchError {
                    code: ErrorCode::UnsupportedRemoteOperation,
                    message: "macOS computers cannot be exported yet.".to_owned(),
                });
            }
        }
        engine::begin_export_for_ids(
            controller,
            params.destination,
            params.computer_ids,
            params.checkpoint_id,
        )
        .map_err(message_error)
    })
    .await?;
    output(exports_job(&operation_id)?)
}

pub(super) async fn import(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ExportImportParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let controller = controller(context)?;
    let operation_id = blocking(move || {
        engine::begin_import(
            controller,
            params.archive_path,
            params.new_name,
            params.source_name,
        )
        .map_err(message_error)
    })
    .await?;
    output(exports_job(&operation_id)?)
}

pub(super) async fn dismiss(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let decoded: ExportDismissParams = context.decode_params(params)?;
    require_local_device(context, decoded.device_id.as_deref())?;
    // The comparison is against the exact result the client holds, so it is passed on as
    // received rather than re-encoded.
    let expected = params["expectedOperation"].clone();
    let expected_id = decoded.expected_operation_id;
    let controller = controller(context)?;
    let dismissed = blocking(move || {
        let dismissed = engine::dismiss_finished_operation(
            &controller,
            Some(&expected),
            expected_id.as_deref(),
        )
        .map_err(message_error)?;
        if dismissed {
            engine::publish(&controller);
        }
        Ok(dismissed)
    })
    .await?;
    output(dismissed)
}

pub(super) async fn acknowledge(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ExportAcknowledgeParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let controller = controller(context)?;
    let changed = blocking(move || {
        let changed = engine::acknowledge_result(&controller, &params.expected_operation_id)
            .map_err(message_error)?;
        if changed {
            engine::publish(&controller);
        }
        Ok(changed)
    })
    .await?;
    output(changed)
}
