//! Request/response framing payloads and protocol negotiation types.

use serde::{Deserialize, Serialize};

/// A canonical multiplexed request. `operationId` and `startWithinMs` are
/// present only for methods that use change admission semantics.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestEnvelope<P> {
    pub id: String,
    pub method: String,
    pub params: P,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_within_ms: Option<u64>,
}

/// A method error. The canonical multiplexed envelope carries only a stable
/// code and message; version diagnostics belong to the handshake contract.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorObject {
    pub code: crate::jobs::ErrorCode,
    pub message: String,
}

/// A response echoes its request id and contains exactly one success or error.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
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
