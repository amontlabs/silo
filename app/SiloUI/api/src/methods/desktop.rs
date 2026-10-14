//! Linux desktop service methods shared with macOS computer-use status.

use super::common::ComputerTargetParams;
use serde::{Deserialize, Serialize};

pub type DesktopStatusResult = DesktopStatus;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopAction {
    Start,
    Stop,
    Restart,
    RestartStreamer,
    UpdateStreamer,
}

wire_struct! {
    DesktopActionParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        action: DesktopAction,
    }
}

pub type DesktopStatusParams = ComputerTargetParams;
pub type DesktopActionResult = super::common::JobStartResult;
pub type DesktopActionFinishedResult = DesktopStatus;

wire_struct! {
    DesktopConnectParams {
        computer_id: String,
    }
}

wire_struct! {
    DesktopConnectResult {
        port: u16,
        username: String,
        password: String,
    }
}

wire_struct! {
    DesktopStatus {
        installed: bool,
        state: String,
        auto_start: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        backend: Option<String>,
        session_state: String,
        stream_state: String,
        update_required: bool,
        update_available: bool,
        streamer_version: Option<String>,
        computer_use: Option<super::computer_use::ComputerUseState>,
        name: Option<String>,
    }
}
