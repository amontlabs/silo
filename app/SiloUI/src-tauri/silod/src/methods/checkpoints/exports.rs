//! `exports.*`: export and import of computers, archive inspection and the remembered
//! destination, over the engine's export controller (`backup_controller`).

use std::{path::PathBuf, sync::Arc};

use serde_json::Value;
use silo_api::{
    jobs::JobId,
    methods::{
        common::JobStartResult,
        exports::{
            ExportAcknowledgeParams, ExportCheckParams, ExportCreateParams,
            ExportDestinationResult, ExportDestinationSetParams, ExportDiscardParams,
            ExportDismissParams, ExportImportParams, ExportInspectParams, ExportStateParams,
        },
    },
};
use silo_engine::backup_controller as engine;

use crate::dispatcher::{DispatchError, HandlerOutput};

use super::{blocking, message_error, output, require_local_device, HandlerContext};

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

/// `exports.check`: the logins on a computer, before it is shared. Starts a job; the result
/// is the list. The computer is resolved here so an unknown id is refused at once.
pub(super) async fn check(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ExportCheckParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let host = context.silod_context().runtime_host();
    let from_checkpoint = params.checkpoint_id.is_some();
    let started = tokio::task::spawn_blocking(move || {
        silo_engine::changes::credentials::start_check(
            host,
            params.computer_id,
            from_checkpoint,
            params.include_logins == Some(true),
        )
    })
    .await
    .map_err(|_| DispatchError::internal("The check could not start."))?;
    started
        .map(|job_id| JobStartResult { job_id })
        .map_err(super::failure)
        .and_then(output)
}

pub(super) async fn create(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ExportCreateParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let controller = controller(context)?;
    let (destination, archive_path) = if params.transfer_area == Some(true) {
        // The machine picks the name, in its own transfer area; the caller's paths are not used.
        let area = transfer_area(context)?;
        let (_, path) = area.export_target();
        (
            area.dir().to_string_lossy().into_owned(),
            Some(path.to_string_lossy().into_owned()),
        )
    } else {
        // A name in the area is the machine's to give; a caller's own file there would be
        // read like an export and removed by the sweep.
        let area = transfer_area(context)?;
        for path in [
            Some(params.destination.as_str()),
            params.archive_path.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if area.contains(std::path::Path::new(path)) {
                return Err(DispatchError::internal(
                    "That folder is Silo's transfer folder. Choose another place for the export.",
                ));
            }
        }
        (params.destination, params.archive_path)
    };
    let operation_id = blocking(move || {
        engine::begin_export_for_ids(
            controller,
            destination,
            params.computer_ids,
            params.checkpoint_id,
            archive_path,
            params.include_logins == Some(true),
        )
        .map_err(message_error)
    })
    .await?;
    output(exports_job(&operation_id)?)
}

fn transfer_area(
    context: &HandlerContext,
) -> Result<silo_engine::transfer_area::Area, DispatchError> {
    silo_engine::transfer_area::Area::of_engine(context.engine_context()).map_err(message_error)
}

/// `exports.discard`: removes a file of this machine's transfer area, once the other machine is
/// done with it. A name that is not one of the area's is refused.
pub(super) async fn discard(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ExportDiscardParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let area = transfer_area(context)?;
    let removed = blocking(move || area.remove(&params.name).map_err(message_error)).await?;
    output(removed)
}

pub(super) async fn import(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ExportImportParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let controller = controller(context)?;
    let archive_path = match params.transfer_name.as_deref() {
        // A file this machine received: the archive checks are the only trust it gets, as for
        // any file of unknown origin.
        Some(name) => {
            if !params.archive_path.is_empty() {
                return Err(DispatchError::internal(
                    "Invalid params: pass either archivePath or transferName.",
                ));
            }
            let area = transfer_area(context)?;
            area.import_path(name)
                .map_err(message_error)?
                .to_string_lossy()
                .into_owned()
        }
        None => params.archive_path,
    };
    let operation_id = blocking(move || {
        engine::begin_import(
            controller,
            archive_path,
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
