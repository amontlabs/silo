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
    let payload = finished_event_payload(&finished)?;
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
        let payload = finished_event_payload(&pending.finished)?;
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
    let payload = finished_event_payload(&finished)?;
    if context
        .job_registry()?
        .reconcile_interrupted(&finished.job_id, &finished)?
    {
        context.publish(Event::new("operations.job.finished", payload), audience);
        context.job_queue_changed();
    }
    Ok(())
}

fn finished_event_payload(finished: &JobFinished) -> Result<serde_json::Value, RegistryError> {
    let outcome_bytes = serde_json::to_vec(finished).map_err(RegistryError::serialization)?;
    if outcome_bytes.len() <= MAX_INLINE_JOB_OUTCOME_BYTES {
        return serde_json::to_value(finished).map_err(RegistryError::serialization);
    }

    let mut projection = finished.clone();
    projection.result = None;
    projection.error = None;
    projection.outcome_chunked = true;
    serde_json::to_value(projection).map_err(RegistryError::serialization)
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
            outcome_chunked: false,
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
            outcome_chunked: false,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{
        EngineIdentity, EnginePaths, EngineRuntimeState, MainThread, PackageKind, ProcessControl,
    };
    use std::sync::{Arc, Mutex};

    struct JobEventContext {
        paths: EnginePaths,
        identity: EngineIdentity,
        runtime_state: Arc<EngineRuntimeState>,
        published: Mutex<Vec<Event>>,
    }

    impl JobEventContext {
        fn new(app_data: std::path::PathBuf) -> Self {
            Self {
                paths: EnginePaths {
                    app_data,
                    ..EnginePaths::default()
                },
                identity: EngineIdentity {
                    channel: crate::channel::Channel::Production,
                    version: "test".into(),
                    package_kind: PackageKind::Other,
                },
                runtime_state: Arc::new(EngineRuntimeState::default()),
                published: Mutex::new(Vec::new()),
            }
        }
    }

    impl EngineContext for JobEventContext {
        fn paths(&self) -> &EnginePaths {
            &self.paths
        }

        fn identity(&self) -> &EngineIdentity {
            &self.identity
        }

        fn runtime_state(&self) -> Arc<EngineRuntimeState> {
            self.runtime_state.clone()
        }

        fn publish(&self, event: Event, _audience: Audience) {
            self.published.lock().unwrap().push(event);
        }

        fn main_thread(&self) -> Option<&dyn MainThread> {
            None
        }

        fn process(&self) -> &dyn ProcessControl {
            self
        }
    }

    impl ProcessControl for JobEventContext {
        fn exit(&self, _code: i32) {}

        fn restart(&self) -> ! {
            panic!("job event test must not restart the process")
        }
    }

    #[test]
    fn oversized_job_completion_events_reference_the_chunked_durable_outcome() {
        let directory = tempfile::tempdir().unwrap();
        let context = JobEventContext::new(directory.path().to_path_buf());
        let job_id = JobId::new("job:1").unwrap();
        let registry = context.job_registry().unwrap();
        registry.accept(job_id.clone(), "guest.exec.start").unwrap();
        let result = serde_json::Value::String(
            "x".repeat(MAX_INLINE_JOB_OUTCOME_BYTES + 4096),
        );

        publish_finished(
            &context,
            JobFinished {
                job_id: job_id.clone(),
                result: Some(result.clone()),
                error: None,
                outcome_chunked: false,
            },
            Audience::All,
        )
        .unwrap();

        let published = context.published.lock().unwrap();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].name, "operations.job.finished");
        assert_eq!(published[0].payload["outcomeChunked"], true);
        assert!(published[0].payload.get("result").is_none());
        assert!(published[0].payload.get("error").is_none());
        assert!(serde_json::to_vec(&published[0].payload).unwrap().len() < 4 * 1024 * 1024);
        drop(published);

        let mut stored_outcome = Vec::new();
        let mut offset = 0;
        loop {
            let lookup = registry.get_at(&job_id, Some(offset)).unwrap().unwrap();
            let chunk = lookup.outcome_chunk.unwrap();
            assert_eq!(chunk.offset_bytes, offset);
            stored_outcome.extend_from_slice(chunk.data.as_bytes());
            match chunk.next_offset_bytes {
                Some(next) => offset = next,
                None => break,
            }
        }
        let outcome: silo_api::jobs::JobOutcome =
            serde_json::from_slice(&stored_outcome).unwrap();
        assert_eq!(outcome.result, Some(result));
        assert!(outcome.error.is_none());
    }
}
