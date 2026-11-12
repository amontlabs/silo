//! `steps.record` and `steps.list`: the commands run on a computer through Silo.

use serde_json::Value;
use silo_api::{
    jobs::ErrorCode,
    methods::steps::{StepsListParams, StepsListResult, StepsRecordParams, StepsRecordResult},
};
use silo_engine::{
    changes::store::Store,
    steps::{NewStep, StepStore},
};

use crate::dispatcher::{DispatchError, HandlerOutput};

use super::{message_error, output, require_local_device, service, HandlerContext};

/// `steps.list`: the computer's steps, oldest first.
pub(super) async fn list(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: StepsListParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let steps = StepStore::for_engine(context.engine_context())
        .list(&params.computer_id)
        .map_err(DispatchError::internal)?;
    output(StepsListResult { steps })
}

/// `steps.record`: adds a finished command to the computer's steps.
pub(super) async fn record(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: StepsRecordParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let engine = context.engine_context();
    service(context)
        .platform(&params.computer_id)
        .map_err(super::failure)?;
    if params.command.is_empty() {
        return Err(DispatchError {
            code: ErrorCode::Internal,
            message: "A step needs the command that ran.".into(),
            details: None,
        });
    }
    // The first step of a fork starts from the checkpoint the fork was made from.
    let parent_checkpoint = Store::for_engine(engine)
        .fork_link(&params.computer_id, "")
        .and_then(|link| link.source);
    let step = StepStore::for_engine(engine)
        .record(NewStep {
            computer_id: params.computer_id,
            command: params.command,
            exit_code: params.exit_code,
            started_at: params.started_at,
            finished_at: params.finished_at,
            checkpoint_id: params.checkpoint_id,
            parent_checkpoint,
        })
        .map_err(message_error)?;
    output(StepsRecordResult { step })
}
