//! Which computer a command addresses. The UI names a local computer by its display name
//! and a computer on another device as `silo-remote:<deviceId>:<computerId>`; the engine
//! wants a stable computer id and, for another device, its `deviceId`.

use silo_api::methods::common::ComputerTargetParams;

use super::{EngineClient, EngineError};

const REMOTE_PREFIX: &str = "silo-remote:";

/// A computer in the engine's terms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub device_id: Option<String>,
    pub computer_id: String,
}

impl Target {
    pub(crate) fn computer(&self) -> ComputerTargetParams {
        ComputerTargetParams {
            computer_id: self.computer_id.clone(),
            device_id: self.device_id.clone(),
        }
    }
}

/// `Some((deviceId, computerId))` for a remote target string, `None` for a local name.
pub(crate) fn parse_remote(value: &str) -> Result<Option<(String, String)>, EngineError> {
    let Some(rest) = value.strip_prefix(REMOTE_PREFIX) else {
        return Ok(None);
    };
    match rest.split_once(':') {
        Some((device, computer)) if !device.is_empty() && !computer.is_empty() => {
            Ok(Some((device.to_owned(), computer.to_owned())))
        }
        _ => Err(EngineError::internal("Invalid remote computer target.")),
    }
}

/// Resolves a UI target. A local name goes through the engine's inventory.
pub(crate) async fn resolve(engine: &EngineClient, value: &str) -> Result<Target, EngineError> {
    match parse_remote(value)? {
        Some((device, computer)) => Ok(Target {
            device_id: Some(device),
            computer_id: computer,
        }),
        None => Ok(Target {
            device_id: None,
            computer_id: engine.computer_id(value).await?,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_targets_split_into_device_and_computer() {
        assert_eq!(parse_remote("dev").unwrap(), None);
        assert_eq!(
            parse_remote("silo-remote:d1:c1").unwrap(),
            Some(("d1".to_owned(), "c1".to_owned()))
        );
        assert!(parse_remote("silo-remote:d1").is_err());
        assert!(parse_remote("silo-remote::c1").is_err());
    }

    #[test]
    fn params_carry_the_device_only_for_another_device() {
        let local = Target {
            device_id: None,
            computer_id: "c".into(),
        };
        assert_eq!(
            serde_json::to_value(local.computer()).unwrap(),
            serde_json::json!({"computerId": "c"})
        );
        let remote = Target {
            device_id: Some("d".into()),
            computer_id: "c".into(),
        };
        assert_eq!(
            serde_json::to_value(remote.computer()).unwrap(),
            serde_json::json!({"computerId": "c", "deviceId": "d"})
        );
    }
}
