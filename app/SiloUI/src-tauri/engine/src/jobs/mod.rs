//! Durable job identities and terminal outcomes.

mod registry;

pub use registry::{
    Acceptance, CancelRoute, JobRegistry, RegistryError, RegistryErrorKind,
    DISCOVERY_ID_BUDGET_BYTES, MAX_INLINE_JOB_OUTCOME_BYTES, MAX_JOB_OUTCOME_CHUNK_BYTES,
};

pub(crate) fn registry_for_engine_data(
    engine_data_dir: &std::path::Path,
) -> Result<std::sync::Arc<JobRegistry>, RegistryError> {
    registry::registry_for_path(engine_data_dir)
}

use crate::context::{Audience, EngineContext, Event};
use silo_api::{
    jobs::{ErrorCode, JobFinished, JobId, JobProgress},
    methods::operations::OperationKind,
    transport::ErrorObject,
};

pub fn accept_domain_id(
    context: &dyn EngineContext,
    space: &str,
    domain_id: &str,
    method: &str,
) -> Result<Acceptance, RegistryError> {
    let id = JobId::new(format!("{space}:{domain_id}"))
        .map_err(|error| RegistryError::invalid(error.to_string()))?;
    let registry = context.job_registry()?;
    retry_pending_outcomes(context, registry.as_ref())?;
    let accepted = registry.accept(id, method)?;
    if accepted.created {
        context.job_queue_changed();
    }
    Ok(accepted)
}

pub fn allocate(context: &dyn EngineContext, method: &str) -> Result<Acceptance, RegistryError> {
    let registry = context.job_registry()?;
    retry_pending_outcomes(context, registry.as_ref())?;
    let accepted = registry.allocate(method)?;
    if accepted.created {
        context.job_queue_changed();
    }
    Ok(accepted)
}

pub fn associate_active_key(
    context: &dyn EngineContext,
    key: impl Into<String>,
    job_id: JobId,
) -> Result<(), RegistryError> {
    context.job_registry()?.associate_active_key(key, job_id);
    Ok(())
}

pub fn active_job_for_key(
    context: &dyn EngineContext,
    key: &str,
) -> Result<Option<JobId>, RegistryError> {
    Ok(context.job_registry()?.active_job_for_key(key))
}

/// Converts existing source progress payloads into the canonical job event
/// while leaving the caller free to publish the legacy Tauri event as well.
pub fn publish_source_progress(
    context: &dyn EngineContext,
    source_event: &str,
    payload: &serde_json::Value,
    audience: Audience,
) -> Result<(), RegistryError> {
    match source_event {
        "silo://computer-configuration-progress" => {
            let data: silo_api::methods::computers::ComputerConfigurationProgress =
                serde_json::from_value(payload.clone()).map_err(RegistryError::serialization)?;
            let id = JobId::new(format!("setup:{}", data.request_id))
                .map_err(|error| RegistryError::invalid(error.to_string()))?;
            publish_progress(
                context,
                JobProgress::Setup {
                    job_id: id,
                    kind: Some(OperationKind::ComputerConfiguration),
                    computer_id: None,
                    data,
                },
                audience,
            )?;
        }
        "silo://lifecycle-progress" => {
            let data: silo_api::jobs::LifecycleProgress =
                serde_json::from_value(payload.clone()).map_err(RegistryError::serialization)?;
            let key = format!("lifecycle:{}:{}", data.computer_id, data.action);
            let Some(id) = active_job_for_key(context, &key)? else {
                return Ok(());
            };
            publish_progress(
                context,
                JobProgress::Lifecycle {
                    job_id: id,
                    kind: Some(OperationKind::Lifecycle),
                    computer_id: Some(data.computer_id.clone()),
                    data,
                },
                audience,
            )?;
        }
        "silo://transfer-progress" => {
            let data: silo_api::methods::files::TransferProgress =
                serde_json::from_value(payload.clone()).map_err(RegistryError::serialization)?;
            let id = JobId::new(format!("transfer:{}", data.id))
                .map_err(|error| RegistryError::invalid(error.to_string()))?;
            publish_progress(
                context,
                JobProgress::Transfer {
                    job_id: id,
                    kind: None,
                    computer_id: None,
                    data,
                },
                audience,
            )?;
        }
        _ => {}
    }
    Ok(())
}

pub fn publish_progress(
    context: &dyn EngineContext,
    progress: JobProgress,
    audience: Audience,
) -> Result<(), RegistryError> {
    let job_id = progress_job_id(&progress);
    let payload = serde_json::to_value(progress).map_err(RegistryError::serialization)?;
    let registry = context.job_registry()?;
    registry.record_progress(&job_id, &payload)?;
    context.publish(Event::new("operations.job.progress", payload), audience);
    Ok(())
}

pub fn publish_finished(
    context: &dyn EngineContext,
    finished: JobFinished,
    audience: Audience,
) -> Result<(), RegistryError> {
    let payload = serde_json::to_value(&finished).map_err(RegistryError::serialization)?;
    let registry = context.job_registry()?;
    if registry.finish_with_audience(&finished, audience.clone())? {
        context.publish(Event::new("operations.job.finished", payload), audience);
        context.job_queue_changed();
    }
    Ok(())
}

fn retry_pending_outcomes(
    context: &dyn EngineContext,
    registry: &JobRegistry,
) -> Result<(), RegistryError> {
    while let Some(pending) = registry.retry_pending_outcome()? {
        let payload =
            serde_json::to_value(&pending.finished).map_err(RegistryError::serialization)?;
        context.publish(
            Event::new("operations.job.finished", payload),
            pending.audience,
        );
        context.job_queue_changed();
    }
    Ok(())
}

/// Persists a terminal value from a domain journal during restart reconciliation,
/// replacing only the generic interrupted outcome if one was recorded at open.
pub fn reconcile_finished(
    context: &dyn EngineContext,
    finished: JobFinished,
    audience: Audience,
) -> Result<(), RegistryError> {
    let payload = serde_json::to_value(&finished).map_err(RegistryError::serialization)?;
    if context
        .job_registry()?
        .reconcile_interrupted(&finished.job_id, &finished)?
    {
        context.publish(Event::new("operations.job.finished", payload), audience);
        context.job_queue_changed();
    }
    Ok(())
}

pub fn finish_with_result(
    context: &dyn EngineContext,
    job_id: JobId,
    result: serde_json::Value,
    audience: Audience,
) -> Result<(), RegistryError> {
    publish_finished(
        context,
        JobFinished {
            job_id,
            result: Some(result),
            error: None,
        },
        audience,
    )
}

pub fn finish_with_error(
    context: &dyn EngineContext,
    job_id: JobId,
    code: ErrorCode,
    message: impl Into<String>,
    audience: Audience,
) -> Result<(), RegistryError> {
    publish_finished(
        context,
        JobFinished {
            job_id,
            result: None,
            error: Some(ErrorObject {
                code,
                message: message.into(),
                diagnostics: None,
            }),
        },
        audience,
    )
}

fn progress_job_id(progress: &JobProgress) -> JobId {
    match progress {
        JobProgress::Setup { job_id, .. }
        | JobProgress::Lifecycle { job_id, .. }
        | JobProgress::Transfer { job_id, .. }
        | JobProgress::Backup { job_id, .. }
        | JobProgress::Push { job_id, .. }
        | JobProgress::Macos { job_id, .. }
        | JobProgress::Update { job_id, .. } => job_id.clone(),
    }
}
