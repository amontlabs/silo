//! Stable native/remote error classification; display text is never a discriminator.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    UpdateInProgress,
    UnsupportedRemoteOperation,
    Cancelled,
    AlreadyQueued,
    Busy,
    NotFound,
    IncompatibleVersion,
    Internal,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct BridgeError {
    pub code: ErrorCode,
    pub message: String,
    /// The remote protocol version the other device reported; never serialized to the UI.
    #[serde(skip)]
    pub peer_version: Option<u32>,
    /// The canonical API error code (`silo-api`) the owner reported when it is finer than
    /// `code`, such as `not_running`. The controller keeps it so the user sees the owner's
    /// reason; a device that predates it sends none and `code` stands alone.
    #[serde(skip)]
    pub api_code: Option<String>,
}

/// The message an unreleased-protocol (version 3) device sends for any version mismatch.
pub const LEGACY_INCOMPATIBLE: &str =
    "Silo versions are incompatible. Update Silo on both computers.";

impl BridgeError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            peer_version: None,
            api_code: None,
        }
    }

    /// Carries the canonical API code of an owner-side failure across the bridge.
    pub fn with_api_code(mut self, api_code: impl Into<String>) -> Self {
        self.api_code = Some(api_code.into());
        self
    }

    pub fn incompatible_version(message: impl Into<String>, peer_version: Option<u32>) -> Self {
        Self {
            peer_version,
            ..Self::new(ErrorCode::IncompatibleVersion, message)
        }
    }

    pub fn updating() -> Self {
        Self::new(
            ErrorCode::UpdateInProgress,
            "Computer configuration is being updated.",
        )
    }

    pub fn unsupported() -> Self {
        Self::new(
            ErrorCode::UnsupportedRemoteOperation,
            "This Silo version does not support that remote operation.",
        )
    }

    /// Compatibility is confined to the wire decoder for devices predating codes.
    /// Exact legacy values only: unrelated text must never change classification.
    pub fn from_remote_reply(reply: &serde_json::Value) -> Option<Self> {
        let legacy = reply.get("error")?.as_str()?;
        let message = reply
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(legacy);
        let peer_version = reply
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u32::try_from(version).ok());
        let code = match reply.get("code") {
            Some(code) => serde_json::from_value(code.clone()).unwrap_or(ErrorCode::Internal),
            None => match legacy {
                "SILO_SANDBOX_UPDATE_IN_PROGRESS" => ErrorCode::UpdateInProgress,
                "Unsupported remote request."
                | "This Silo version does not support that remote operation." => {
                    ErrorCode::UnsupportedRemoteOperation
                }
                LEGACY_INCOMPATIBLE => ErrorCode::IncompatibleVersion,
                _ => ErrorCode::Internal,
            },
        };
        // A version 3 device sends its mismatch text with the code `internal`.
        let code = if code == ErrorCode::Internal && legacy == LEGACY_INCOMPATIBLE {
            ErrorCode::IncompatibleVersion
        } else {
            code
        };
        let mut error = Self::new(code, message);
        error.api_code = reply
            .get("apiCode")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        if code == ErrorCode::IncompatibleVersion {
            error.peer_version = peer_version;
        }
        Some(error)
    }
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for BridgeError {}
impl From<String> for BridgeError {
    fn from(message: String) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
}
impl From<&str> for BridgeError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

impl From<crate::runtime::operation_gate::GateError> for BridgeError {
    fn from(error: crate::runtime::operation_gate::GateError) -> Self {
        use crate::runtime::operation_gate::GateError;
        let code = match error {
            GateError::AlreadyQueued => ErrorCode::AlreadyQueued,
            GateError::Busy => ErrorCode::Busy,
            GateError::Cancelled => ErrorCode::Cancelled,
            _ => ErrorCode::Internal,
        };
        Self::new(code, error.to_string())
    }
}
impl From<crate::runtime::RuntimeError> for BridgeError {
    fn from(error: crate::runtime::RuntimeError) -> Self {
        use crate::runtime::RuntimeError;
        let code = match &error {
            RuntimeError::Busy => ErrorCode::Busy,
            RuntimeError::Cancelled { .. } => ErrorCode::Cancelled,
            RuntimeError::Admission(error) => return error.clone().into(),
            RuntimeError::Partial(_) => ErrorCode::Internal,
            _ => ErrorCode::Internal,
        };
        Self::new(code, crate::runtime::safe_activity_error(&error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_error_contract_round_trips_every_code() {
        let codes = [
            ErrorCode::UpdateInProgress,
            ErrorCode::UnsupportedRemoteOperation,
            ErrorCode::Cancelled,
            ErrorCode::AlreadyQueued,
            ErrorCode::Busy,
            ErrorCode::NotFound,
            ErrorCode::IncompatibleVersion,
            ErrorCode::Internal,
        ];
        let errors: Vec<_> = codes
            .into_iter()
            .map(|code| BridgeError::new(code, "Display text may change."))
            .collect();
        let value = serde_json::to_value(&errors).unwrap();
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../src/test/contracts/bridge-errors.json"
        ))
        .unwrap();
        assert_eq!(value, expected);
        assert_eq!(
            serde_json::from_value::<Vec<BridgeError>>(value).unwrap(),
            errors
        );
    }
    #[test]
    fn remote_decoder_preserves_codes_and_only_classifies_exact_legacy_errors() {
        let error = BridgeError::from_remote_reply(
            &serde_json::json!({"code":"cancelled","error":"Stopped at your request."}),
        )
        .unwrap();
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert_eq!(
            BridgeError::from_remote_reply(
                &serde_json::json!({"error":"Unsupported remote request."})
            )
            .unwrap()
            .code,
            ErrorCode::UnsupportedRemoteOperation
        );
        assert_eq!(
            BridgeError::from_remote_reply(
                &serde_json::json!({"error":"log contains SILO_SANDBOX_UPDATE_IN_PROGRESS"})
            )
            .unwrap()
            .code,
            ErrorCode::Internal
        );
        assert_eq!(
            BridgeError::from_remote_reply(
                &serde_json::json!({"code":"future_error","error":"Update the owner."})
            )
            .unwrap()
            .code,
            ErrorCode::Internal
        );
        assert_eq!(
            BridgeError::from("The operation was cancelled.").code,
            ErrorCode::Internal
        );
    }
    #[test]
    fn admission_errors_keep_their_kind_across_the_runtime_boundary() {
        use crate::runtime::{operation_gate::GateError, RuntimeError};
        for (gate, code) in [
            (GateError::AlreadyQueued, ErrorCode::AlreadyQueued),
            (GateError::Cancelled, ErrorCode::Cancelled),
            (GateError::Busy, ErrorCode::Busy),
        ] {
            let error = BridgeError::from(RuntimeError::from(gate));
            assert_eq!(error.code, code);
        }
    }
}
