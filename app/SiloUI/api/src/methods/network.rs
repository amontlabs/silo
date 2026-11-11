//! Published-port state and operations.

use super::common::{ComputerTargetParams, DeviceParams};
use serde::{Deserialize, Serialize};

pub type NetworkStateParams = DeviceParams;

wire_struct! {
    NetworkState {
        computers: Vec<NetworkComputer>,
    }
}

wire_struct! {
    NetworkComputer {
        computer: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host: Option<String>,
        error: Option<String>,
        ports: Vec<PublishedPort>,
    }
}

wire_struct! {
    PublishedPort {
        port: u16,
        host_port: Option<u16>,
        configured_host_port: Option<u16>,
        scheme: Option<NetworkScheme>,
        state: PublishedPortState,
        configured: bool,
        message: Option<String>,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NetworkScheme {
    Http,
    Https,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PublishedPortState {
    Reachable,
    Waiting,
    Unpublished,
    Unknown,
}

wire_struct! {
    NetworkPublishParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        port: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host_port: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scheme: Option<NetworkScheme>,
    }
}

wire_struct! { NetworkUnpublishParams { computer_id: String, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String>, port: u16 } }
wire_struct! { NetworkOpenParams { computer_id: String, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String>, port: u16 } }
wire_struct! { NetworkOpenResult { url: String } }
pub type NetworkChangeResult = ();
pub type NetworkTargetParams = ComputerTargetParams;

wire_struct! {
    NetworkUrl {
        port: u16,
        host: String,
        url: String,
    }
}
wire_struct! {
    NetworkUrlsResult {
        computer_id: String,
        urls: Vec<NetworkUrl>,
    }
}

/// How much of the network a computer may reach.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum NetworkPolicyMode {
    /// Any public address, as every computer had before policies existed.
    #[default]
    Open,
    /// Only the hosts on its list, the hosts of its presets and the hosts its grants need.
    Allowlist,
    /// No outbound traffic, not even name lookups.
    Closed,
}

wire_struct! {
    /// A computer's network policy, as the owner sets it.
    NetworkPolicy {
        mode: NetworkPolicyMode,
        /// Host patterns: `example.com` or `*.example.com` (which also covers
        /// `example.com`). Lowercase names only; no ports, no addresses. Used in allowlist mode.
        #[serde(default)]
        hosts: Vec<String>,
        /// Named host lists kept by the engine (`network.policy.get` lists them). Used in
        /// allowlist mode.
        #[serde(default)]
        presets: Vec<String>,
    }
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            mode: NetworkPolicyMode::Open,
            hosts: Vec::new(),
            presets: Vec::new(),
        }
    }
}

wire_struct! {
    NetworkPolicySetParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        policy: NetworkPolicy,
        /// The policy the caller judged the change against. When it is given and the computer's
        /// policy is now another, nothing is saved. The engine fills it in for a computer's
        /// agent narrowing its own network, so a narrowing cannot undo a change the owner made
        /// meanwhile.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected: Option<NetworkPolicy>,
        /// When true, re-pins presets to their latest versions, accepting pending updates.
        /// A normal save preserves existing pins.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        accept_presets: Option<bool>,
    }
}

/// When a saved policy is in force.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NetworkPolicyTiming {
    /// The computer runs with it now.
    Applied,
    /// The computer is stopped; it starts with this policy.
    NextStart,
    /// The computer is running with its previous rules and keeps them until it restarts.
    NextRestart,
}

wire_struct! {
    /// One host a computer in allowlist mode may reach, and why.
    NetworkPolicyHost {
        /// A host pattern, as in `NetworkPolicy::hosts`.
        host: String,
        /// The TCP ports allowed to it.
        ports: Vec<u16>,
        /// `list`, `preset:<name>`, `github` or `secret:<name>`.
        from: String,
    }
}

wire_struct! {
    /// A named host list the engine keeps.
    NetworkPreset {
        name: String,
        title: String,
        hosts: Vec<String>,
    }
}

wire_struct! {
    /// A preset whose list in this Silo has hosts the computer's policy was not approved with.
    /// The computer keeps its approved list until the owner sets the policy again with the
    /// same presets.
    NetworkPresetUpdate {
        name: String,
        title: String,
        /// The hosts the update would add.
        added_hosts: Vec<String>,
    }
}

wire_struct! {
    NetworkPolicyState {
        computer_id: String,
        policy: NetworkPolicy,
        /// Every host the computer may reach in allowlist mode, including those added for its
        /// GitHub access and its secrets. Empty in the other modes.
        allowed_hosts: Vec<NetworkPolicyHost>,
        timing: NetworkPolicyTiming,
        presets: Vec<NetworkPreset>,
        /// Presets of the policy that have an update to accept.
        #[serde(default)]
        preset_updates: Vec<NetworkPresetUpdate>,
        /// Things to tell the owner about this policy, in plain words: a secret wildcard that is
        /// not added to the list, or a policy that could not be read.
        #[serde(default)]
        notices: Vec<String>,
    }
}
