//! The `computers.rebuild` job and the recovery of an interrupted rebuild.

use std::{path::PathBuf, sync::Arc};

use silo_api::jobs::JobId;

use super::{engine_env::EngineRebuildEnv, rebuild, Journal, LANE_WAIT};
use crate::{
    changes::keep::service as keep_service,
    checkpoints::{Failure, Service},
    context::EngineContext,
    runtime::RuntimeHost,
};

fn folder(context: &dyn EngineContext) -> PathBuf {
    context.paths().app_data.join("rebuild")
}

pub fn journal_for(context: &dyn EngineContext) -> Journal {
    Journal::new(folder(context))
}

/// The folder the home folder waits in on the host between two computers.
pub fn stage_folder(context: &dyn EngineContext) -> PathBuf {
    folder(context).join("stage")
}

/// Removes the staged home folders no interrupted rebuild waits for.
pub fn sweep_stage(context: &dyn EngineContext) {
    super::sweep_stage(&journal_for(context), &stage_folder(context));
}

/// The environment a rebuild or its recovery runs in.
pub fn env_for(host: Arc<dyn RuntimeHost>, service: Service) -> EngineRebuildEnv {
    let context = host.engine_context();
    EngineRebuildEnv::new(
        host,
        service,
        keep_service::journal_for(context.as_ref()),
        stage_folder(context.as_ref()),
    )
}

/// `computers.rebuild`: rebuilds the Linux computer `computer_id` on the newest guest image.
pub fn start_rebuild(
    host: Arc<dyn RuntimeHost>,
    service: Service,
    computer_id: String,
    force: bool,
) -> Result<JobId, Failure> {
    keep_service::start(host, "computers.rebuild", move |host| {
        let journal = journal_for(host.engine_context().as_ref());
        let env = env_for(host.clone(), service);
        let result = rebuild(&env, &journal, &computer_id, force, LANE_WAIT)?;
        serde_json::to_value(result).map_err(|error| error.to_string())
    })
}
