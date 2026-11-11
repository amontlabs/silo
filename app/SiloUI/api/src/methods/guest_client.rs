//! Silo's tools in a computer: the channel a computer's own agent reaches the engine on. The
//! engine listens on one loopback port per computer, the computer's network allows exactly that
//! port on its gateway address, and the handshake carries a token minted for that computer only.
//! The engine writes the address and the token into the computer itself; no answer carries the
//! token. See `docs/SiloUI-IN-GUEST-CLIENT.md`.

use crate::methods::network::NetworkPolicyTiming;

/// Where the engine writes the channel's address and token in a computer (root-owned, readable
/// by the `silo` account's group).
pub const GUEST_ENDPOINT_PATH: &str = "/etc/silo/endpoint.json";
/// The token file of the earlier in-guest client; a fork still drops it.
pub const LEGACY_GUEST_TOKEN_PATH: &str = "/etc/silo/guest-client.json";
/// The name a computer reaches the device's loopback by (its gateway address).
pub const GUEST_ENDPOINT_HOST: &str = "host.microsandbox.internal";

wire_struct! {
    GuestClientComputerParams {
        computer_id: String,
    }
}

wire_struct! {
    GuestClientEnableParams {
        computer_id: String,
        /// A `silo` program for Linux on this device to send into the computer instead of the one
        /// this build ships (for development). It is checked inside the computer by its SHA-256
        /// like the shipped one, and replaced by the shipped one at the computer's next start.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        binary_path: Option<String>,
    }
}

/// How one piece Silo keeps in a computer ended its last reconcile.
#[derive(
    Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum GuestComponentState {
    /// In place and current.
    Ok,
    /// The step failed; the detail says why.
    Failed,
    /// This build of Silo cannot provide the piece.
    Unavailable,
    /// Waiting for something else, such as an agent that is not installed yet.
    Waiting,
    /// Taken out on purpose because the tools were turned off.
    Removed,
}

wire_struct! {
    /// A piece Silo puts into a computer and keeps current, with the result of its last reconcile
    /// (at every start of the computer, and when the tools are turned on or off).
    GuestComponentStatus {
        /// `silo` (the program at `/usr/local/bin/silo`) or `silo-mcp` (its registration as an
        /// MCP server in Claude Code and Codex).
        id: String,
        version: u32,
        state: GuestComponentState,
        /// Why it failed or is waiting, in words for a person.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        /// Seconds since the epoch of that reconcile.
        at: u64,
    }
}

wire_struct! {
    /// Whether a computer has Silo's tools, and where they answer.
    GuestClientStatus {
        computer_id: String,
        enabled: bool,
        /// The host name the computer reaches the channel by.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host: Option<String>,
        /// The channel's port on the device's loopback.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<u16>,
        /// Where the computer finds the address and its token.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        endpoint_path: Option<String>,
        /// When the tools were turned on.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        enabled_at: Option<String>,
        /// When the computer's network lets it reach the channel: now, at its next start, or
        /// after a restart.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        network: Option<NetworkPolicyTiming>,
        /// Whether the endpoint file was written into the computer by this call. A computer
        /// that is not running gets it when it next starts.
        #[serde(default)]
        written: bool,
        /// What Silo keeps in the computer and how each piece fared at its last reconcile.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        components: Vec<GuestComponentStatus>,
    }
}
