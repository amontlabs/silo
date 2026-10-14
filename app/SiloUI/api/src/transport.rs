//! Request/response framing payloads and protocol negotiation types.

use serde::{Deserialize, Serialize};

use crate::methods::MethodName;

/// A canonical multiplexed request. `operationId` and `startWithinMs` are
/// present only for methods that use change admission semantics.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestEnvelope<P> {
    pub id: String,
    pub method: MethodName,
    pub params: P,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_within_ms: Option<u64>,
}

/// A method error with optional structured version diagnostics for handshake refusal.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorObject {
    pub code: crate::jobs::ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<IncompatibleVersionDiagnostics>,
}

/// A response echoes its request id and contains exactly one success or error.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum ResponseEnvelope<T> {
    Success { id: String, result: T },
    Error { id: String, error: ErrorObject },
}

/// The inclusive protocol range a client supports.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ProtocolRange {
    pub min: u32,
    pub max: u32,
}

/// Engine and client versions reported when handshake protocol negotiation fails.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IncompatibleVersionDiagnostics {
    pub engine_protocol_range: ProtocolRange,
    pub client_protocol_range: ProtocolRange,
    pub engine_app_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_app_version: Option<String>,
}

/// The first canonical multiplexed engine protocol. Legacy bridge protocol v5
/// is retired and is not accepted by `engine.sock`.
pub const PROTOCOL_VERSION: u32 = 6;

/// Client information is diagnostic metadata, not an authorization identity.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ClientMetadata {
    pub name: String,
    pub version: String,
}

/// `engine.handshake` parameters. `sshKey` is used by the Connections bridge
/// to install the restricted key; local engine clients omit it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_key: Option<String>,
    pub protocol: ProtocolRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<ClientMetadata>,
}

/// Result of protocol negotiation. `clientId` is supplied on `engine.sock`
/// and omitted when the handshake runs through the remote control socket.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeResult {
    pub device_id: String,
    pub name: String,
    pub protocol: u32,
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}
