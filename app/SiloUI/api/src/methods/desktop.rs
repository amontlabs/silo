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
    /// Install or update the Node.js runtime and Claude Code CLI (Linux computers).
    SetupAgentTools,
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
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        user: Option<String>,
        #[serde(default)]
        display: Option<String>,
        #[serde(default)]
        backend: Option<String>,
        session_state: Option<String>,
        stream_state: Option<String>,
        update_required: bool,
        update_available: bool,
        streamer_version: Option<String>,
        computer_use: Option<super::computer_use::ComputerUseState>,
        name: Option<String>,
        /// The agent tools (Node.js and the Claude Code CLI) of the computer; null when the
        /// computer cannot be inspected (stopped Linux computers).
        #[serde(default)]
        agent_tools: Option<AgentToolsStatus>,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AgentToolsState {
    /// Both tools are installed and the setup recipe is current.
    Ready,
    /// At least one tool is not installed.
    Missing,
    /// Everything is installed, but by an older recipe than this Silo ships.
    Outdated,
}

wire_struct! {
    /// What `claude -p` runs and Fjord's guest scripts need in a computer. Silo never signs in
    /// to Claude Code; authentication comes from the client.
    AgentToolsStatus {
        state: AgentToolsState,
        /// Any of `node`, `claude`.
        missing: Vec<String>,
        node_version: Option<String>,
        claude_version: Option<String>,
        expected_node_version: String,
        expected_claude_version: String,
        /// The recipe that installed the tools; null when none did.
        recipe_version: Option<u32>,
        expected_recipe_version: u32,
    }
}
