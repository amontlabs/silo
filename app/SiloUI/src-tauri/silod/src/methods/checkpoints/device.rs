//! `device.*`: facts about this Mac and the dependency report.

use serde_json::Value;
use silo_api::methods::device::{DependencyReportParams, DeviceFactsParams};
use silo_engine::{
    dependencies::{self, ProbePaths},
    device_facts,
};

use crate::dispatcher::{DispatchError, HandlerOutput};

use super::{blocking, message_error, output, require_local_device, HandlerContext};

pub(super) async fn facts(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: DeviceFactsParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    // The Remote Login probe is a short network connect, so it runs off the async workers.
    let facts = blocking(|| Ok(device_facts::read())).await?;
    output(facts)
}

pub(super) async fn dependencies(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: DependencyReportParams = context.decode_params(params)?;
    dependencies::validate_request_id(&params.request_id).map_err(message_error)?;
    let executable_dir = context
        .silod_context()
        .runtime_host()
        .bundled_tools_directory()
        .map_err(message_error)?;
    let resource_dir = context.engine_context().paths().resources.clone();
    let frameworks_dir = executable_dir
        .parent()
        .map(|contents| contents.join("Frameworks"));
    // Three probe groups run one after another, each under its own budget, so a report can
    // take close to a minute.
    let report = blocking(move || {
        Ok(dependencies::read_report(
            params.request_id,
            ProbePaths {
                executable_dir,
                resource_dir,
                frameworks_dir,
            },
        ))
    })
    .await?;
    let report: silo_api::methods::device::DependencyReport = serde_json::to_value(report)
        .and_then(serde_json::from_value)
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    output(report)
}
