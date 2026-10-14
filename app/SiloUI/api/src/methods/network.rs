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
