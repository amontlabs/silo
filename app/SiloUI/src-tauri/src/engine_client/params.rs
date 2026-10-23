//! Small builders for the parameter shapes most engine methods share, and the one place
//! UI-provided JSON becomes a typed request parameter.

use serde::de::DeserializeOwned;
use serde_json::Value;
use silo_api::methods::common::{ComputerTargetParams, DeviceParams};

use super::EngineError;

pub(crate) fn target(computer_id: &str, device_id: Option<&str>) -> ComputerTargetParams {
    ComputerTargetParams {
        computer_id: computer_id.to_owned(),
        device_id: device_id.map(str::to_owned),
    }
}

pub(crate) fn device(device_id: Option<&str>) -> DeviceParams {
    DeviceParams {
        device_id: device_id.map(str::to_owned),
    }
}

/// An engine job id, as the engine returned it.
pub(crate) fn job_id(job_id: &str) -> Result<silo_api::JobId, EngineError> {
    silo_api::JobId::new(job_id)
        .map_err(|_| EngineError::internal("The engine returned an invalid job id."))
}

/// Reads a UI-provided value as the registry's type for it.
pub(crate) fn decode<T: DeserializeOwned>(value: Value, what: &str) -> Result<T, EngineError> {
    serde_json::from_value(value)
        .map_err(|error| EngineError::internal(format!("Invalid {what}: {error}")))
}
