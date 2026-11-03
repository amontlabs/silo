//! `preparation.*`: the Linux guest image preparation. Retry returns the status at once;
//! progress follows as `preparation.changed`.

use serde_json::Value;
use silo_api::methods::preparation::{
    PreparationRetryParams, PreparationStatus, PreparationStatusParams,
};
use silo_engine::preparation;

use crate::dispatcher::{DispatchError, HandlerOutput};

use super::{output, require_local_device, HandlerContext};

fn to_api<T: serde::de::DeserializeOwned>(
    value: impl serde::Serialize,
) -> Result<T, DispatchError> {
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .map_err(|error| DispatchError::internal(error.to_string()))
}

pub(super) async fn status(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: PreparationStatusParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    output(to_api::<PreparationStatus>(preparation::snapshot())?)
}

pub(super) async fn retry(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: PreparationRetryParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    output(to_api::<PreparationStatus>(preparation::retry())?)
}
