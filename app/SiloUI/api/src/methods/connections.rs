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
    #[doc = "A controller's Connections public key (`ssh-ed25519 <key> [comment]`) for this device to accept."]
    AuthorizeKeyParams {
        public_key: String,
    }
}

wire_struct! {
    AuthorizeKeyResult {
        #[doc = "False when the key was already authorized."]
        added: bool,
    }
}

wire_struct! {
    #[doc = "One line of `~/.ssh/authorized_keys` whose forced command is this channel's Connections bridge."]
    AuthorizedKey {
        #[doc = "`<type> <key> [comment]`."]
        public_key: String,
        #[doc = "`SHA256:...`, as `ssh-keygen -l` prints it; absent when the key is not base64."]
        fingerprint: Option<String>,
        #[doc = "The line is exactly the one Silo writes now (`restrict`, the bridge with `--key` and the key's fingerprint)."]
        canonical: bool,
        #[doc = "The forced command passes the bridge this line's own key fingerprint, so a request through it can revoke that key."]
        identified: bool,
    }
}

wire_struct! {
    #[doc = "The controller keys this device's account accepts for Connections: every line of `~/.ssh/authorized_keys` whose forced command is the bridge, whatever its other options, comment or the Silo version that wrote it."]
    AuthorizedKeysResult {
        #[doc = "The keys of `keys`, each once."]
        public_keys: Vec<String>,
        #[doc = "Each line, in file order."]
        #[serde(default)]
        keys: Vec<AuthorizedKey>,
    }
}

wire_struct! {
    #[doc = "This device's own Connections public key, created when it had none."]
    OwnKeyResult {
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
        #[doc = "Why the engine could not link the `silo` command on PATH when it started."]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cli_link_error: Option<String>,
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
