//! The agents a computer has: one interface over Linux and macOS computers.
//!
//! An agent is a program a person works with inside a computer (Claude Code, Codex). This
//! module holds what does not depend on the system: the pinned versions, what a computer's
//! report means for each agent, and which installs are running. Linux computers are inspected
//! and installed by `desktop` (the guest script in `agent_tools`); macOS computers by
//! `macos_computers::agent_tools`. Silo installs an agent but never signs in to it.
use crate::agent_tools;
use serde_json::Value;
use silo_api::methods::agents::{AgentId, AgentInfo, AgentState};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, MutexGuard},
};

fn info(agent: AgentId, state: AgentState, version: Option<String>) -> AgentInfo {
    AgentInfo {
        id: agent,
        name: agent.display_name().to_owned(),
        state,
        version,
        expected_version: agent_tools::expected_agent_version(agent),
    }
}

/// Every agent as `unknown`: the computer is stopped or cannot be reached.
pub fn unknown() -> Vec<AgentInfo> {
    AgentId::ALL
        .into_iter()
        .map(|agent| info(agent, AgentState::Unknown, None))
        .collect()
}

/// The state of an agent whose installed version is `installed`.
fn state_of_version(agent: AgentId, installed: Option<&str>) -> AgentState {
    match installed {
        None => AgentState::Missing,
        Some(version) if version == agent_tools::expected_agent_version(agent) => AgentState::Ready,
        Some(_) => AgentState::Outdated,
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value[key]
        .as_str()
        .filter(|text| !text.is_empty() && text.len() <= 64)
        .map(str::to_owned)
}

/// The agents of a running Linux computer from the line `agent_tools::status_command` printed.
/// An empty or unreadable line means nothing is installed. Claude Code follows the agent
/// tools projection (it needs the Node.js runtime, and an older setup recipe is outdated).
pub fn from_linux_status(line: Option<&str>) -> Vec<AgentInfo> {
    let raw: Value = line
        .and_then(|line| serde_json::from_str(line.trim()).ok())
        .unwrap_or(Value::Null);
    let tools = agent_tools::linux_state(line);
    let claude_state = match tools["state"].as_str() {
        Some("ready") => AgentState::Ready,
        Some("outdated") => AgentState::Outdated,
        _ => AgentState::Missing,
    };
    let codex = text(&raw, "codexVersion");
    vec![
        info(
            AgentId::ClaudeCode,
            claude_state,
            text(&raw, "claudeVersion"),
        ),
        info(
            AgentId::Codex,
            state_of_version(AgentId::Codex, codex.as_deref()),
            codex,
        ),
    ]
}

/// The agents of a macOS computer. Claude Code comes with the computer's developer tools step,
/// so it is ready when that step is current, `outdated` when an older step installed it, and
/// missing before setup; a receipt written by `agents.install` for the current version is
/// ready whatever the step says. Codex is known only from its receipt, which needs the
/// running computer: `codex_receipt` is `None` while it is stopped and `Some(None)` when it
/// is running without one.
pub fn from_macos(
    developer_tools_installed: bool,
    developer_tools_current: bool,
    claude_receipt: Option<&Value>,
    codex_receipt: Option<Option<&Value>>,
) -> Vec<AgentInfo> {
    let receipt_version = |receipt: Option<&Value>| receipt.and_then(|r| text(r, "version"));
    let claude_installed = receipt_version(claude_receipt);
    let claude_expected = agent_tools::expected_agent_version(AgentId::ClaudeCode);
    let claude = if claude_installed.as_deref() == Some(claude_expected.as_str()) {
        info(AgentId::ClaudeCode, AgentState::Ready, claude_installed)
    } else if !developer_tools_installed {
        info(AgentId::ClaudeCode, AgentState::Missing, None)
    } else if developer_tools_current {
        info(
            AgentId::ClaudeCode,
            AgentState::Ready,
            Some(claude_expected),
        )
    } else {
        info(AgentId::ClaudeCode, AgentState::Outdated, claude_installed)
    };
    let codex = match codex_receipt {
        None => info(AgentId::Codex, AgentState::Unknown, None),
        Some(receipt) => {
            let version = receipt_version(receipt);
            info(
                AgentId::Codex,
                state_of_version(AgentId::Codex, version.as_deref()),
                version,
            )
        }
    };
    vec![claude, codex]
}

static INSTALLING: Mutex<Option<HashSet<(String, AgentId)>>> = Mutex::new(None);

fn installing() -> MutexGuard<'static, Option<HashSet<(String, AgentId)>>> {
    INSTALLING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// An install that is running; the agent shows as `installing` until this is dropped.
pub struct InstallGuard {
    computer_id: String,
    agent: AgentId,
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        if let Some(set) = installing().as_mut() {
            set.remove(&(self.computer_id.clone(), self.agent));
        }
    }
}

/// Records that `agent` is being installed in the computer, or refuses a second install of the
/// same agent in the same computer.
pub fn begin_install(computer_id: &str, agent: AgentId) -> Result<InstallGuard, String> {
    let mut set = installing();
    let running = set.get_or_insert_with(HashSet::new);
    if !running.insert((computer_id.to_owned(), agent)) {
        return Err(format!(
            "{} is already being installed in this computer.",
            agent.display_name()
        ));
    }
    Ok(InstallGuard {
        computer_id: computer_id.to_owned(),
        agent,
    })
}

/// Marks the agents being installed in the computer as `installing`.
pub fn mark_installing(computer_id: &str, agents: &mut [AgentInfo]) {
    let set = installing();
    let Some(running) = set.as_ref() else {
        return;
    };
    for entry in agents {
        if running.contains(&(computer_id.to_owned(), entry.id)) {
            entry.state = AgentState::Installing;
        }
    }
}

static REQUESTED: Mutex<Option<HashMap<String, Vec<AgentId>>>> = Mutex::new(None);

fn requested() -> MutexGuard<'static, Option<HashMap<String, Vec<AgentId>>>> {
    REQUESTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Remembers the agents a create request asked for until the new Linux computer is ready.
pub fn request_for_creation(computer_id: &str, agents: &[AgentId]) {
    let mut unique = Vec::new();
    for agent in agents {
        if !unique.contains(agent) {
            unique.push(*agent);
        }
    }
    let mut map = requested();
    let map = map.get_or_insert_with(HashMap::new);
    if unique.is_empty() {
        map.remove(computer_id);
    } else {
        map.insert(computer_id.to_owned(), unique);
    }
}

/// The agents a create request asked for, once; none when it asked for none.
pub fn take_requested(computer_id: &str) -> Vec<AgentId> {
    requested()
        .as_mut()
        .and_then(|map| map.remove(computer_id))
        .unwrap_or_default()
}

/// The guest script that installs `agents` in a Linux computer and then registers computer use
/// with them. An install that fails does not stop the others.
pub(crate) fn linux_install_all_script(agents: &[AgentId]) -> String {
    let mut script = String::from("status=0\n");
    for agent in agents {
        script.push_str(&format!(
            "({}) || status=1\n",
            agent_tools::setup_script_for(*agent)
        ));
    }
    script.push_str(&crate::computer_use::register_installed_agents_command());
    script.push_str("\nexit $status\n");
    script
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn by_id(agents: &[AgentInfo], id: AgentId) -> &AgentInfo {
        agents.iter().find(|agent| agent.id == id).unwrap()
    }

    #[test]
    fn a_stopped_computer_has_unknown_agents_with_the_versions_an_install_gives() {
        let agents = unknown();
        assert_eq!(agents.len(), 2);
        assert!(agents
            .iter()
            .all(|agent| agent.state == AgentState::Unknown));
        assert!(agents.iter().all(|agent| agent.version.is_none()));
        assert_eq!(by_id(&agents, AgentId::Codex).name, "Codex");
        assert_eq!(by_id(&agents, AgentId::ClaudeCode).name, "Claude Code");
        assert_eq!(by_id(&agents, AgentId::Codex).expected_version, "0.167.0");
    }

    #[test]
    fn nothing_printed_by_a_running_linux_computer_means_both_agents_are_missing() {
        for line in [None, Some(""), Some("not json"), Some("{}")] {
            let agents = from_linux_status(line);
            assert!(agents
                .iter()
                .all(|agent| agent.state == AgentState::Missing));
            assert!(agents.iter().all(|agent| agent.version.is_none()));
        }
    }

    #[test]
    fn linux_states_follow_the_versions_in_the_status_line() {
        let claude = agent_tools::expected_agent_version(AgentId::ClaudeCode);
        let codex = agent_tools::expected_agent_version(AgentId::Codex);
        let recipe = 1;
        let line = |claude: &str, codex: &str, recipe: u32| {
            format!(
                r#"{{"nodeVersion":"24.21.0","claudeVersion":{claude},"codexVersion":{codex},"recipeVersion":{recipe}}}"#
            )
        };
        let quote = |value: &str| format!("\"{value}\"");
        let ready = from_linux_status(Some(&line(&quote(&claude), &quote(&codex), recipe)));
        assert!(ready.iter().all(|agent| agent.state == AgentState::Ready));
        assert_eq!(
            by_id(&ready, AgentId::Codex).version.as_deref(),
            Some(codex.as_str())
        );

        let mixed = from_linux_status(Some(&line(&quote(&claude), &quote("0.100.0"), recipe)));
        assert_eq!(by_id(&mixed, AgentId::ClaudeCode).state, AgentState::Ready);
        assert_eq!(by_id(&mixed, AgentId::Codex).state, AgentState::Outdated);
        assert_eq!(
            by_id(&mixed, AgentId::Codex).version.as_deref(),
            Some("0.100.0")
        );

        let old_recipe = from_linux_status(Some(&line(&quote(&claude), "null", 0)));
        assert_eq!(
            by_id(&old_recipe, AgentId::ClaudeCode).state,
            AgentState::Outdated
        );
        assert_eq!(
            by_id(&old_recipe, AgentId::Codex).state,
            AgentState::Missing
        );

        let half = from_linux_status(Some(
            r#"{"nodeVersion":"24.21.0","claudeVersion":null,"codexVersion":"0.167.0","recipeVersion":null}"#,
        ));
        assert_eq!(by_id(&half, AgentId::ClaudeCode).state, AgentState::Missing);
        assert_eq!(by_id(&half, AgentId::Codex).state, AgentState::Ready);
    }

    #[test]
    fn macos_reports_claude_code_ready_with_the_current_template_and_codex_by_its_receipt() {
        let stopped = from_macos(true, true, None, None);
        assert_eq!(
            by_id(&stopped, AgentId::ClaudeCode).state,
            AgentState::Ready
        );
        assert_eq!(
            by_id(&stopped, AgentId::ClaudeCode).version.as_deref(),
            Some("2.1.287")
        );
        assert_eq!(by_id(&stopped, AgentId::Codex).state, AgentState::Unknown);

        let none = from_macos(true, true, None, Some(None));
        assert_eq!(by_id(&none, AgentId::Codex).state, AgentState::Missing);

        let current = json!({"version": "0.167.0"});
        let ready = from_macos(true, true, None, Some(Some(&current)));
        assert_eq!(by_id(&ready, AgentId::Codex).state, AgentState::Ready);

        let older = json!({"version": "0.100.0"});
        let outdated = from_macos(true, true, None, Some(Some(&older)));
        assert_eq!(by_id(&outdated, AgentId::Codex).state, AgentState::Outdated);
    }

    #[test]
    fn macos_claude_code_is_outdated_or_missing_without_a_current_template_and_repair_fixes_it() {
        let before_setup = from_macos(false, false, None, None);
        assert_eq!(
            by_id(&before_setup, AgentId::ClaudeCode).state,
            AgentState::Missing
        );
        let old_step = from_macos(true, false, None, None);
        assert_eq!(
            by_id(&old_step, AgentId::ClaudeCode).state,
            AgentState::Outdated
        );
        let repaired = json!({"version": "2.1.287"});
        let fixed = from_macos(true, false, Some(&repaired), None);
        assert_eq!(by_id(&fixed, AgentId::ClaudeCode).state, AgentState::Ready);
        let stale_receipt = json!({"version": "2.0.0"});
        let still_old = from_macos(true, false, Some(&stale_receipt), None);
        assert_eq!(
            by_id(&still_old, AgentId::ClaudeCode).state,
            AgentState::Outdated
        );
        assert_eq!(
            by_id(&still_old, AgentId::ClaudeCode).version.as_deref(),
            Some("2.0.0")
        );
    }

    #[test]
    fn a_running_install_shows_as_installing_and_a_second_one_is_refused() {
        let computer = "agents-test-installing";
        let guard = begin_install(computer, AgentId::Codex).unwrap();
        let again = begin_install(computer, AgentId::Codex).err().unwrap();
        assert!(
            again.contains("Codex is already being installed"),
            "{again}"
        );
        // Another agent, and the same agent in another computer, are separate installs.
        drop(begin_install(computer, AgentId::ClaudeCode).unwrap());
        drop(begin_install("agents-test-other", AgentId::Codex).unwrap());

        let mut agents = from_linux_status(None);
        mark_installing(computer, &mut agents);
        assert_eq!(by_id(&agents, AgentId::Codex).state, AgentState::Installing);
        assert_eq!(
            by_id(&agents, AgentId::ClaudeCode).state,
            AgentState::Missing
        );
        drop(guard);
        let mut agents = from_linux_status(None);
        mark_installing(computer, &mut agents);
        assert_eq!(by_id(&agents, AgentId::Codex).state, AgentState::Missing);
        drop(begin_install(computer, AgentId::Codex).unwrap());
    }

    #[test]
    fn requested_agents_are_taken_once_and_deduplicated() {
        let computer = "agents-test-requested";
        assert!(take_requested(computer).is_empty());
        request_for_creation(
            computer,
            &[AgentId::Codex, AgentId::Codex, AgentId::ClaudeCode],
        );
        assert_eq!(
            take_requested(computer),
            vec![AgentId::Codex, AgentId::ClaudeCode]
        );
        assert!(take_requested(computer).is_empty());
        request_for_creation(computer, &[]);
        assert!(take_requested(computer).is_empty());
    }

    #[test]
    fn installing_several_agents_runs_each_script_then_registers_computer_use() {
        let script = linux_install_all_script(&[AgentId::Codex, AgentId::ClaudeCode]);
        let codex = script.find("setup codex").unwrap();
        let claude = script.find("setup claude-code").unwrap();
        let register = script.find("silo-computer-use reconcile").unwrap();
        assert!(codex < claude && claude < register);
        assert!(script.trim_end().ends_with("exit $status"));
        assert_eq!(script.matches("|| status=1").count(), 2);
    }
}
