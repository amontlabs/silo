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
/// ChatGPT app and LCU methods and fields; 8 adds the channel inside a computer and retypes
/// `guestClient.enable`, `.revoke` and `.status` as Silo's tools in a computer (a port and a
/// token the engine writes into the computer, no token in any answer). On that channel the
/// engine speaks first with a [`ChannelChallenge`] and the handshake answers it with a `proof`
/// ([`channel_proof`]); the token itself never crosses the connection. Version 8 had not shipped
/// when its handshake changed from carrying the token to this challenge, so the number stayed.
/// Legacy bridge protocol v5 is retired and is not accepted by `engine.sock`.
pub const PROTOCOL_VERSION: u32 = 8;

/// Client information is diagnostic metadata, not an authorization identity.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ClientMetadata {
    pub name: String,
    pub version: String,
}

/// `engine.handshake` parameters. `sshKey` is used by the Connections bridge
/// to install the restricted key; local engine clients omit it. `proof` answers the channel's
/// [`ChannelChallenge`] on the channel inside a computer, and is required there (lowercase hex of
/// [`channel_proof`]); `engine.sock` ignores it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_key: Option<String>,
    pub protocol: ProtocolRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<ClientMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<String>,
}

/// The first frame on a computer's channel, sent by the engine as soon as it accepts the
/// connection: a fresh random nonce, lowercase hex of [`CHANNEL_NONCE_BYTES`] bytes.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelChallenge {
    pub nonce: String,
}

/// The length of a channel nonce.
pub const CHANNEL_NONCE_BYTES: usize = 32;
/// Names what a channel proof is for, so it is useless as any other HMAC.
pub const CHANNEL_PROOF_CONTEXT: &[u8] = b"silo channel handshake v1\0";

/// The key a channel proof is made with: SHA-256 of the computer's token. The engine keeps
/// exactly this and never the token.
pub fn channel_key(token: &str) -> [u8; 32] {
    use sha2::Digest as _;
    sha2::Sha256::digest(token.as_bytes()).into()
}

fn channel_mac(key: &[u8; 32], nonce: &[u8], port: u16) -> hmac::Hmac<sha2::Sha256> {
    use hmac::Mac as _;
    let mut mac = <hmac::Hmac<sha2::Sha256> as hmac::Mac>::new_from_slice(key)
        .expect("HMAC takes a key of any length");
    mac.update(CHANNEL_PROOF_CONTEXT);
    mac.update(nonce);
    mac.update(&port.to_be_bytes());
    mac
}

/// HMAC-SHA-256 keyed by [`channel_key`] over the context, the nonce and the port the client
/// believes it is connecting to. A proof is good for one nonce on one port.
pub fn channel_proof(key: &[u8; 32], nonce: &[u8], port: u16) -> [u8; 32] {
    use hmac::Mac as _;
    channel_mac(key, nonce, port).finalize().into_bytes().into()
}

/// Whether `proof` is [`channel_proof`] of `key`, `nonce` and `port`, compared in constant time.
pub fn channel_proof_matches(key: &[u8; 32], nonce: &[u8], port: u16, proof: &[u8]) -> bool {
    use hmac::Mac as _;
    nonce.len() == CHANNEL_NONCE_BYTES && channel_mac(key, nonce, port).verify_slice(proof).is_ok()
}

/// Lowercase hex of `bytes`.
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    text
}

/// The `N` bytes lowercase or uppercase hex `text` spells; `None` for anything else.
pub fn from_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.as_bytes();
    if text.len() != N * 2 {
        return None;
    }
    let digit = |byte: u8| char::from(byte).to_digit(16).map(|value| value as u8);
    let mut bytes = [0_u8; N];
    for (index, pair) in text.chunks_exact(2).enumerate() {
        bytes[index] = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channel_proof_is_bound_to_its_key_nonce_and_port() {
        let key = channel_key("silo_gc_abc");
        let nonce = [7_u8; CHANNEL_NONCE_BYTES];
        let proof = channel_proof(&key, &nonce, 41234);
        assert!(channel_proof_matches(&key, &nonce, 41234, &proof));
        assert!(!channel_proof_matches(&key, &nonce, 41235, &proof));
        assert!(!channel_proof_matches(&key, &[8_u8; 32], 41234, &proof));
        assert!(!channel_proof_matches(
            &channel_key("silo_gc_abd"),
            &nonce,
            41234,
            &proof
        ));
        assert!(!channel_proof_matches(&key, &nonce[..31], 41234, &proof));
        assert!(!channel_proof_matches(&key, &nonce, 41234, &proof[..31]));
        assert_eq!(from_hex::<32>(&to_hex(&proof)), Some(proof));
        assert_eq!(from_hex::<2>("0aFf"), Some([0x0a, 0xff]));
        assert_eq!(from_hex::<2>("0aF"), None);
        assert_eq!(from_hex::<1>("zz"), None);
    }
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
