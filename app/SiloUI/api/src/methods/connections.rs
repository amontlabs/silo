//! Connections inventory and remote-device methods.

use serde::{Deserialize, Serialize};

wire_struct! {
    ConnectDeviceParams {
        address: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replace: Option<bool>,
    }
}

wire_struct! {
    #[doc = "Mirrors `app/SiloUI/src-tauri/engine/src/remote.rs::Device`."]
    Device {
        id: String,
        name: String,
        address: String,
    }
}

wire_struct! {
    DeviceIdParams {
        device_id: String,
    }
}

wire_struct! {
    RevokeKeyParams {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        public_key: String,
    }
}

wire_struct! {
    ConnectionsSetEnabledParams {
        enabled: bool,
    }
}

wire_struct! {
    SshAddressParams {
        address: String,
    }
}

wire_struct! {
    SshCommandResult {
        command: String,
    }
}

wire_struct! {
    #[doc = "Mirrors `app/SiloUI/src-tauri/engine/src/remote.rs::ConnectionsStatus`."]
    ConnectionsStatus {
        enabled: bool,
        device_id: String,
        name: String,
        address: String,
        addresses: Vec<DeviceAddress>,
        error: Option<String>,
    }
}

wire_struct! {
    DeviceAddress {
        address: String,
        kind: DeviceAddressKind,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DeviceAddressKind {
    Name,
    Tailscale,
    Network,
}

wire_struct! {
    DeviceHealthResult {
        devices: Vec<DeviceHealth>,
    }
}

pub type DeviceHealthParams = super::common::EmptyParams;

wire_struct! {
    DeviceHealth {
        device_id: String,
        connected: bool,
        busy: bool,
        error: Option<String>,
        last_seen: Option<String>,
    }
}
