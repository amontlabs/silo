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

/// A version as dotted numbers, or `None` for anything else.
fn dotted_numbers(version: &str) -> Option<Vec<u64>> {
    version
        .split('.')
        .map(|part| {
            (!part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        })
        .collect()
}

/// Whether `installed` is the pinned version or a newer one. Versions compare as dotted
/// numbers (a missing part is zero); a version that is not dotted numbers counts only when it
/// is equal.
pub(crate) fn at_least_pinned(installed: &str, pinned: &str) -> bool {
    let (Some(mut installed_parts), Some(mut pinned_parts)) =
        (dotted_numbers(installed), dotted_numbers(pinned))
    else {
        return installed == pinned;
    };
    let length = installed_parts.len().max(pinned_parts.len());
    installed_parts.resize(length, 0);
    pinned_parts.resize(length, 0);
    installed_parts >= pinned_parts
}

/// The state of an agent whose installed version is `installed`: ready when it is the pinned
/// version or a newer one.
fn state_of_version(agent: AgentId, installed: Option<&str>) -> AgentState {
    match installed {
        None => AgentState::Missing,
        Some(version) if at_least_pinned(version, &agent_tools::expected_agent_version(agent)) => {
            AgentState::Ready
        }
        Some(_) => AgentState::Outdated,
    }
}

/// The longest reason shown for a failed install, in characters.
const REASON_CHARS: usize = 200;

/// A line a computer printed, made safe to show: control characters removed and cut to
/// [`REASON_CHARS`] characters.
pub(crate) fn safe_reason(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.chars().count() <= REASON_CHARS {
        return cleaned.to_owned();
    }
    let cut: String = cleaned.chars().take(REASON_CHARS).collect();
    format!("{}…", cut.trim_end())
}

fn text(value: &Value, key: &str) -> Option<String> {
    value[key]
        .as_str()
        .filter(|text| !text.is_empty() && text.len() <= 64)
        .map(str::to_owned)
}

/// The longest status line read from a computer, in bytes.
const STATUS_LINE_BYTES: usize = 4096;

/// The agents of a running Linux computer from the line `agent_tools::status_command` printed.
/// A line that is empty, too long or not a JSON object means the computer did not answer, so
/// every agent is `unknown`; an answer without a version means that agent is missing. Claude
/// Code follows the agent tools projection (it needs the Node.js runtime, and an older setup
/// recipe is outdated).
pub fn from_linux_status(line: Option<&str>) -> Vec<AgentInfo> {
    let raw: Value = line
        .map(str::trim)
        .filter(|line| line.len() <= STATUS_LINE_BYTES)
        .and_then(|line| serde_json::from_str(line).ok())
        .filter(Value::is_object)
        .unwrap_or(Value::Null);
    if raw.is_null() {
        return unknown();
    }
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
    let claude = if claude_installed
        .as_deref()
        .is_some_and(|version| at_least_pinned(version, &claude_expected))
    {
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

/// The hosts an agent's installer downloads from (Node.js and the agent itself).
fn install_hosts(agent: AgentId) -> &'static [&'static str] {
    match agent {
        AgentId::ClaudeCode => &["nodejs.org", "downloads.claude.ai"],
        AgentId::Codex => &["nodejs.org", "github.com"],
    }
}

/// Plain words for a failed install when the computer's network policy keeps the installer
/// from its download sites; `None` when the network is not the reason it can be told.
pub fn network_reason(
    held: &crate::network_policy::Held,
    computer: &str,
    agent: AgentId,
) -> Option<String> {
    use crate::network_policy::{reach_of, Reach};
    let name = agent.display_name();
    match reach_of(held, install_hosts(agent)) {
        Reach::All => None,
        Reach::Nothing => Some(format!(
            "{name} could not be installed: {computer} has no network access, and {name} is downloaded from {}. Open its network, or allow those sites, then try again.",
            install_hosts(agent).join(" and ")
        )),
        Reach::Missing(hosts) => Some(format!(
            "{name} could not be installed: the network of {computer} does not allow {}, which {name} is downloaded from. Allow {} in its network settings, then try again.",
            hosts.join(" and "),
            if hosts.len() == 1 { "it" } else { "them" }
        )),
    }
}

/// [`network_reason`] for the policy the computer `id` runs with now.
pub fn network_reason_for(id: &str, computer: &str, agent: AgentId) -> Option<String> {
    let (held, _) = crate::network_policy::held_for(id, None).ok()?;
    network_reason(&held, computer, agent)
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

/// The line the install script of several agents prints for an agent that failed.
const FAILED_PREFIX: &str = "silo-agent-failed:";

/// The agents named by [`FAILED_PREFIX`] lines in what an install script printed.
pub(crate) fn failed_in(output: &str) -> Vec<AgentId> {
    AgentId::ALL
        .into_iter()
        .filter(|agent| {
            output
                .lines()
                .any(|line| line.strip_prefix(FAILED_PREFIX) == Some(agent.as_str()))
        })
        .collect()
}

/// The guest command that registers computer use and Silo's own MCP server with agents installed
/// since they were set up; every install path ends with it.
pub(crate) fn register_after_install_command() -> String {
    format!(
        "{}\n{}",
        crate::computer_use::register_installed_agents_command(),
        crate::guest_components::register_after_install_command(
            crate::guest_components::run::Kind::Linux
        )
    )
}

/// The guest script that installs `agents` in a Linux computer and then registers computer use
/// and Silo's own MCP server with them. An install that fails does not stop the others; it prints a [`FAILED_PREFIX`]
/// line naming the agent.
pub(crate) fn linux_install_all_script(agents: &[AgentId]) -> String {
    let mut script = String::from("status=0\n");
    for agent in agents {
        script.push_str(&format!(
            "({}) || {{ echo {FAILED_PREFIX}{}; status=1; }}\n",
            agent_tools::setup_script_for(*agent),
            agent.as_str()
        ));
    }
    script.push_str(&register_after_install_command());
    script.push_str("\nexit $status\n");
    script
}

/// The workspace folder an agent starts in, on a Linux computer.
const LINUX_WORKSPACE: &str = "/workspace";
/// The workspace folder an agent starts in, on a macOS computer.
const MACOS_WORKSPACE: &str = "/Users/silo/workspace";

/// The shell command line that starts `agent` in the computer's workspace folder, for a
/// terminal on a Linux (`macos` false) or macOS computer. Both systems link the program at
/// `/usr/local/bin`. It is fixed text chosen from the agent: no part of it comes from a caller.
/// When the program is not there it says so and leaves the person in a login shell, so the
/// terminal does not close on a message nobody read.
pub fn launch_script(agent: AgentId, macos: bool) -> String {
    let (workspace, shell) = if macos {
        (MACOS_WORKSPACE, "/bin/zsh")
    } else {
        (LINUX_WORKSPACE, "/bin/bash")
    };
    let program = match agent {
        AgentId::ClaudeCode => "/usr/local/bin/claude",
        AgentId::Codex => "/usr/local/bin/codex",
    };
    format!(
        "cd {workspace} 2>/dev/null; if [ -x {program} ]; then exec {program}; fi; \
         echo '{} is not installed in this computer. Install it from the computer page in Silo.'; \
         exec {shell} -l",
        agent.display_name()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_agent_launches_by_its_fixed_program_in_its_systems_workspace() {
        let claude = launch_script(AgentId::ClaudeCode, false);
        assert!(claude.starts_with("cd /workspace 2>/dev/null; "));
        assert!(claude.contains("exec /usr/local/bin/claude; fi;"));
        assert!(claude.ends_with("exec /bin/bash -l"));
        let codex = launch_script(AgentId::Codex, true);
        assert!(codex.starts_with("cd /Users/silo/workspace 2>/dev/null; "));
        assert!(codex.contains("[ -x /usr/local/bin/codex ]"));
        assert!(codex.contains("Codex is not installed in this computer."));
        assert!(codex.ends_with("exec /bin/zsh -l"));
        assert!(!codex.contains("claude"));
    }

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
    fn an_answer_with_no_versions_means_both_agents_are_missing() {
        for line in [Some("{}"), Some(r#"{"nodeVersion":null,"claudeVersion":null,"codexVersion":null,"recipeVersion":null}"#)] {
            let agents = from_linux_status(line);
            assert!(agents
                .iter()
                .all(|agent| agent.state == AgentState::Missing));
            assert!(agents.iter().all(|agent| agent.version.is_none()));
        }
    }

    #[test]
    fn a_running_linux_computer_that_gave_no_readable_answer_is_unknown_not_missing() {
        let too_long = format!(r#"{{"codexVersion":"0.167.0","pad":"{}"}}"#, "x".repeat(5000));
        for line in [
            None,
            Some(""),
            Some("  "),
            Some("not json"),
            Some("[1]"),
            Some("\"text\""),
            Some(too_long.as_str()),
        ] {
            let agents = from_linux_status(line);
            assert_eq!(agents.len(), 2);
            assert!(
                agents.iter().all(|agent| agent.state == AgentState::Unknown),
                "{line:?}"
            );
            assert!(agents.iter().all(|agent| agent.version.is_none()));
        }
    }

    #[test]
    fn a_newer_installed_version_is_ready_and_versions_compare_as_dotted_numbers() {
        assert!(at_least_pinned("0.167.0", "0.167.0"));
        assert!(at_least_pinned("0.168.0", "0.167.0"));
        assert!(at_least_pinned("0.10.0", "0.9.0"));
        assert!(at_least_pinned("1.0", "0.167.0"));
        assert!(at_least_pinned("0.167", "0.167.0"));
        assert!(!at_least_pinned("0.166.9", "0.167.0"));
        assert!(!at_least_pinned("0.9.0", "0.10.0"));
        // Anything that is not dotted numbers counts only when equal.
        assert!(!at_least_pinned("0.168.0-beta", "0.167.0"));
        assert!(!at_least_pinned("1..2", "0.167.0"));
        assert!(!at_least_pinned("", "0.167.0"));
        assert!(at_least_pinned("nightly", "nightly"));
        let codex = agent_tools::expected_agent_version(AgentId::Codex);
        assert_eq!(
            state_of_version(AgentId::Codex, Some("99.0.0")),
            AgentState::Ready
        );
        assert_eq!(
            state_of_version(AgentId::Codex, Some("0.1.0")),
            AgentState::Outdated
        );
        assert_eq!(
            state_of_version(AgentId::Codex, Some(&codex)),
            AgentState::Ready
        );
        let newer = json!({"version": "99.0.0"});
        let agents = from_macos(true, false, Some(&newer), Some(Some(&newer)));
        assert_eq!(by_id(&agents, AgentId::ClaudeCode).state, AgentState::Ready);
        assert_eq!(by_id(&agents, AgentId::Codex).state, AgentState::Ready);
    }

    #[test]
    fn a_blocked_network_is_named_as_the_reason_an_install_failed() {
        use crate::network_policy::{Held, NetworkPolicy, NetworkPolicyMode};
        let held = |mode, hosts: &[&str]| {
            Held::latest(NetworkPolicy {
                mode,
                hosts: hosts.iter().map(|host| (*host).to_owned()).collect(),
                ..NetworkPolicy::default()
            })
        };
        assert_eq!(network_reason(&held(NetworkPolicyMode::Open, &[]), "dev", AgentId::Codex), None);
        let closed = network_reason(&held(NetworkPolicyMode::Closed, &[]), "dev", AgentId::Codex).unwrap();
        assert!(closed.contains("dev has no network access"), "{closed}");
        let some = network_reason(
            &held(NetworkPolicyMode::Allowlist, &["example.com"]),
            "dev",
            AgentId::Codex,
        )
        .unwrap();
        assert!(some.contains("does not allow nodejs.org and github.com"), "{some}");
        let partly = network_reason(
            &held(NetworkPolicyMode::Allowlist, &["nodejs.org", "api.github.com"]),
            "dev",
            AgentId::Codex,
        )
        .unwrap();
        assert!(partly.contains("does not allow github.com,"), "{partly}");
        assert_eq!(
            network_reason(
                &held(NetworkPolicyMode::Allowlist, &["nodejs.org", "downloads.claude.ai"]),
                "dev",
                AgentId::ClaudeCode
            ),
            None
        );
    }

    #[test]
    fn a_reason_from_a_computer_has_no_control_characters_and_is_cut_short() {
        assert_eq!(safe_reason("  plain words \n"), "plain words");
        assert_eq!(safe_reason("a\u{1b}[31mb\u{7}\r\nc"), "a[31mbc");
        let long = safe_reason(&"é".repeat(70_000));
        assert_eq!(long.chars().count(), REASON_CHARS + 1);
        assert!(long.ends_with('…'));
        assert_eq!(safe_reason(&"y".repeat(REASON_CHARS)).chars().count(), REASON_CHARS);
    }

    #[test]
    fn an_install_script_names_each_failed_agent_and_only_those() {
        let script = linux_install_all_script(&[AgentId::Codex, AgentId::ClaudeCode]);
        assert!(script.contains("echo silo-agent-failed:codex"));
        assert!(script.contains("echo silo-agent-failed:claude-code"));
        assert_eq!(failed_in("ok\nsilo-agent-failed:codex\n"), [AgentId::Codex]);
        assert_eq!(
            failed_in("silo-agent-failed:claude-code\nsilo-agent-failed:codex"),
            [AgentId::ClaudeCode, AgentId::Codex]
        );
        assert!(failed_in("xsilo-agent-failed:codex\nsilo-agent-failed:other").is_empty());
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

        let mut agents = from_linux_status(Some("{}"));
        mark_installing(computer, &mut agents);
        assert_eq!(by_id(&agents, AgentId::Codex).state, AgentState::Installing);
        assert_eq!(
            by_id(&agents, AgentId::ClaudeCode).state,
            AgentState::Missing
        );
        drop(guard);
        let mut agents = from_linux_status(Some("{}"));
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
    fn installing_several_agents_runs_each_script_then_registers_computer_use_and_silos_tools() {
        let script = linux_install_all_script(&[AgentId::Codex, AgentId::ClaudeCode]);
        let codex = script.find("setup codex").unwrap();
        let claude = script.find("setup claude-code").unwrap();
        let register = script.find("silo-computer-use reconcile").unwrap();
        let silo = script.find("server-add --name silo").unwrap();
        assert!(codex < claude && claude < register && register < silo);
        assert!(script.trim_end().ends_with("exit $status"));
        assert_eq!(script.matches("status=1; }").count(), 2);
    }
}
