//! The agents a computer has, and installing one.
//!
//! The same two methods serve Linux virtual machines, container computers and macOS computers.
//! Silo installs an agent but never signs in to it.

use super::common::ComputerTargetParams;
use serde::{Deserialize, Serialize};

/// An agent Silo can install in a computer.
#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum AgentId {
    ClaudeCode,
    Codex,
}

impl AgentId {
    /// Every agent, in the order a list shows them.
    pub const ALL: [AgentId; 2] = [AgentId::ClaudeCode, AgentId::Codex];

    /// The wire name, also used in guest scripts and receipts.
    pub const fn as_str(self) -> &'static str {
        match self {
            AgentId::ClaudeCode => "claude-code",
            AgentId::Codex => "codex",
        }
    }

    /// The name a person reads.
    pub const fn display_name(self) -> &'static str {
        match self {
            AgentId::ClaudeCode => "Claude Code",
            AgentId::Codex => "Codex",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AgentState {
    /// Installed at the version this Silo ships.
    Ready,
    /// Not installed.
    Missing,
    /// Installed, but not at the version this Silo ships.
    Outdated,
    /// An install is running in the computer.
    Installing,
    /// Not known: the computer is stopped or cannot be reached.
    Unknown,
}

wire_struct! {
    AgentInfo {
        id: AgentId,
        /// The name to show, for example `Claude Code`.
        name: String,
        state: AgentState,
        /// The installed version; null when not installed or not known.
        version: Option<String>,
        /// The version an install puts in the computer.
        expected_version: String,
    }
}

pub type AgentsListParams = ComputerTargetParams;

wire_struct! {
    AgentsListResult {
        agents: Vec<AgentInfo>,
    }
}

wire_struct! {
    AgentsInstallParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        agent: AgentId,
    }
}

/// Where an agent install is. The first step of a macOS install downloads the pinned files on
/// this device; the last registers computer use with the agent.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AgentInstallStep {
    Waiting,
    Downloading,
    Installing,
    Registering,
    Done,
}

wire_struct! {
    /// The `data` of an `agent` job progress event.
    AgentInstallProgress {
        computer_id: String,
        agent: AgentId,
        step: AgentInstallStep,
        /// A short sentence for the step, such as `Downloading Codex (42%)`.
        detail: Option<String>,
    }
}

pub type AgentsInstallResult = super::common::JobStartResult;
pub type AgentsInstallFinishedResult = AgentsListResult;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_use_kebab_case_names() {
        assert_eq!(
            serde_json::to_value(AgentId::ClaudeCode).unwrap(),
            "claude-code"
        );
        assert_eq!(serde_json::to_value(AgentId::Codex).unwrap(), "codex");
        for agent in AgentId::ALL {
            assert_eq!(serde_json::to_value(agent).unwrap(), agent.as_str());
        }
        assert!(serde_json::from_value::<AgentId>(serde_json::json!("pi")).is_err());
    }

    #[test]
    fn states_use_kebab_case_names() {
        for (state, name) in [
            (AgentState::Ready, "ready"),
            (AgentState::Missing, "missing"),
            (AgentState::Outdated, "outdated"),
            (AgentState::Installing, "installing"),
            (AgentState::Unknown, "unknown"),
        ] {
            assert_eq!(serde_json::to_value(state).unwrap(), name);
        }
    }

    #[test]
    fn list_and_install_params_round_trip_with_and_without_a_device() {
        let list: AgentsListParams =
            serde_json::from_value(serde_json::json!({"computerId": "c"})).unwrap();
        assert_eq!(list.device_id, None);
        let install = serde_json::json!({"computerId": "c", "deviceId": "d", "agent": "codex"});
        let parsed: AgentsInstallParams = serde_json::from_value(install.clone()).unwrap();
        assert_eq!(parsed.agent, AgentId::Codex);
        assert_eq!(serde_json::to_value(parsed).unwrap(), install);
        let local: AgentsInstallParams =
            serde_json::from_value(serde_json::json!({"computerId": "c", "agent": "claude-code"}))
                .unwrap();
        assert!(serde_json::to_value(local)
            .unwrap()
            .get("deviceId")
            .is_none());
    }

    #[test]
    fn a_list_result_round_trips() {
        let payload = serde_json::json!({"agents": [
            {"id": "claude-code", "name": "Claude Code", "state": "ready",
             "version": "2.1.287", "expectedVersion": "2.1.287"},
            {"id": "codex", "name": "Codex", "state": "unknown",
             "version": null, "expectedVersion": "0.167.0"},
        ]});
        let result: AgentsListResult = serde_json::from_value(payload.clone()).unwrap();
        assert_eq!(result.agents[1].state, AgentState::Unknown);
        assert_eq!(serde_json::to_value(result).unwrap(), payload);
    }
}
