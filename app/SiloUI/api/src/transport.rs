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
    /// For `approval_required`: what the call asks the owner for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<ApprovalRequiredDetails>,
}

/// What a call that widens a computer's access asks the owner for.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    /// Bring a computer's changes into one the caller did not make.
    Merge,
    /// Replace main with a fork.
    ReplaceMain,
    /// GitHub access: a repository, or write access.
    Github,
    /// A secret, or a secret's assignment to a computer.
    Secret,
    /// A wider network policy.
    Network,
    /// A port published on the device.
    Port,
    /// SSH access to a computer from outside.
    Ssh,
    /// Access the caller does not have itself, handed to a computer it makes or forks.
    Access,
    /// A container computer, which shares the device's kernel.
    Container,
    /// A new computer with more CPUs, memory or storage than the computer making it has.
    Resources,
}

/// The details of an `approval_required` error.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequiredDetails {
    pub kind: ApprovalKind,
    /// What is asked, written by the engine from the call's parameters.
    pub summary: String,
    /// The request the owner answers, once approvals are recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
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

/// The engine protocol. 6 was the first canonical multiplexed protocol; 7 removes the
/// ChatGPT app and LCU methods and fields; 8 adds the handshake's `token` (the channel inside a
/// computer) and retypes `guestClient.enable`, `.revoke` and `.status` as Silo's tools in a
/// computer (a port and a token the engine writes into the computer, no token in any answer).
/// Legacy bridge protocol v5 is retired and is not accepted by `engine.sock`.
pub const PROTOCOL_VERSION: u32 = 8;

/// Client information is diagnostic metadata, not an authorization identity.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ClientMetadata {
    pub name: String,
    pub version: String,
}

/// `engine.handshake` parameters. `sshKey` is used by the Connections bridge
/// to install the restricted key; local engine clients omit it. `token` is the computer's
/// token on the channel inside a computer, and is required there; `engine.sock` ignores it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_key: Option<String>,
    pub protocol: ProtocolRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<ClientMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
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
