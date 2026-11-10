//! Agent tools in a computer: a pinned Node.js LTS runtime, the Claude Code CLI and the Codex CLI.
//!
//! Fjord's guest hook, plugin and run supervisor are JavaScript, and Triage runs are
//! headless `claude -p`; both need Node.js and Claude Code in every computer. Linux computers
//! get the tools from `guest/agent-tools.py`, run per agent by `agents.install` (and by the
//! `setup-agent-tools` desktop action, which installs Claude Code) and safe to repeat; the
//! lock pins Codex for Linux and macOS alike. macOS computers get Node.js and Claude Code from
//! the developer tools step (`macos_computers::developer_tools`), so the template fingerprint
//! covers them, and Codex from `macos_computers::agent_tools`. Silo never signs in to an agent.
use serde::Deserialize;
use serde_json::{json, Value};
use silo_api::methods::agents::AgentId;

const LOCK: &str = include_str!("../../guest/agent-tools-lock.json");
const SCRIPT: &str = include_str!("../../guest/agent-tools.py");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lock {
    recipe_version: u32,
    node: Node,
    claude_code: ClaudeCode,
    codex: Codex,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Codex {
    pub(crate) version: String,
    pub(crate) assets: CodexAssets,
}

/// The Codex archive of each platform the guests run on.
#[derive(Debug, Deserialize)]
pub(crate) struct CodexAssets {
    #[serde(rename = "linux-arm64")]
    pub(crate) linux_arm64: CodexAsset,
    #[serde(rename = "linux-amd64")]
    pub(crate) linux_amd64: CodexAsset,
    #[serde(rename = "macos-arm64")]
    pub(crate) macos_arm64: CodexAsset,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CodexAsset {
    pub(crate) url: String,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
    /// The one file the archive holds.
    pub(crate) member: String,
}

#[derive(Debug, Deserialize)]
struct Node {
    version: String,
}

#[derive(Debug, Deserialize)]
struct ClaudeCode {
    version: String,
}

fn lock() -> Lock {
    serde_json::from_str(LOCK).expect("the bundled agent tools lock is valid")
}

/// The Codex pin: the version and the macOS archive, which the host downloads and checks.
pub(crate) fn codex_pin() -> Codex {
    lock().codex
}

/// The version of `agent` an install puts in a computer.
pub(crate) fn expected_agent_version(agent: AgentId) -> String {
    let lock = lock();
    match agent {
        AgentId::ClaudeCode => lock.claude_code.version,
        AgentId::Codex => lock.codex.version,
    }
}

/// The versions this Silo installs: `(recipe, node, claude)`.
fn expected() -> (u32, String, String) {
    let lock = lock();
    (
        lock.recipe_version,
        lock.node.version,
        lock.claude_code.version,
    )
}

fn stage(command: &str) -> String {
    format!(
        "silo_tools=$(mktemp -d /tmp/silo-agent-tools.XXXXXXXX)\n\
         trap 'rm -rf \"$silo_tools\"' EXIT\n\
         cat > \"$silo_tools/agent-tools-lock.json\" <<'SILO_AGENT_TOOLS_LOCK_EOF'\n{LOCK}\nSILO_AGENT_TOOLS_LOCK_EOF\n\
         cat > \"$silo_tools/agent-tools.py\" <<'SILO_AGENT_TOOLS_EOF'\n{SCRIPT}\nSILO_AGENT_TOOLS_EOF\n\
         {command}\n"
    )
}

/// Guest command that prints one JSON line with what is installed. It never fails: a guest
/// that cannot run it reports nothing installed.
pub(crate) fn status_command() -> String {
    stage(
        "(python3 \"$silo_tools/agent-tools.py\" \"$silo_tools/agent-tools-lock.json\" status 2>/dev/null | tail -n 1) || true\n\
         printf '\\n'",
    )
}

/// Guest script (run as root) that installs or updates `agent` in a Linux computer. Claude
/// Code comes with the Node.js runtime Fjord needs.
pub(crate) fn setup_script_for(agent: AgentId) -> String {
    let mut script = String::from("set -eu\n");
    script.push_str(&stage(&format!(
        "python3 \"$silo_tools/agent-tools.py\" \"$silo_tools/agent-tools-lock.json\" setup {}",
        agent.as_str()
    )));
    script
}

/// What the `setup-agent-tools` desktop action runs: it installs Claude Code.
pub(crate) fn setup_script() -> String {
    setup_script_for(AgentId::ClaudeCode)
}

fn version_of(raw: &Value, key: &str) -> Option<String> {
    raw[key]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn projection(node: Option<String>, claude: Option<String>, recipe: Option<u32>) -> Value {
    let (expected_recipe, expected_node, expected_claude) = expected();
    let mut missing = Vec::new();
    if node.is_none() {
        missing.push("node");
    }
    if claude.is_none() {
        missing.push("claude");
    }
    let state = if !missing.is_empty() {
        "missing"
    } else if recipe != Some(expected_recipe) {
        "outdated"
    } else {
        "ready"
    };
    json!({
        "state": state, "missing": missing,
        "nodeVersion": node, "claudeVersion": claude,
        "expectedNodeVersion": expected_node, "expectedClaudeVersion": expected_claude,
        "recipeVersion": recipe, "expectedRecipeVersion": expected_recipe,
    })
}

/// The public `agentTools` object of a running Linux computer from the line printed by
/// [`status_command`]. An empty or unreadable line means nothing is installed.
pub(crate) fn linux_state(line: Option<&str>) -> Value {
    let raw: Value = line
        .and_then(|line| serde_json::from_str(line.trim()).ok())
        .unwrap_or(Value::Null);
    projection(
        version_of(&raw, "nodeVersion"),
        version_of(&raw, "claudeVersion"),
        raw["recipeVersion"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok()),
    )
}

/// The `agentTools` object of a macOS computer. The developer tools step installs both tools,
/// and the fingerprint of that step (`developer_tools_current`) says whether this computer
/// ran the current one.
pub(crate) fn macos_state(developer_tools_installed: bool, developer_tools_current: bool) -> Value {
    let (recipe, node, claude) = expected();
    match (developer_tools_installed, developer_tools_current) {
        (false, _) => projection(None, None, None),
        // An older step installed whichever versions it pinned; they are not known here.
        (true, false) => json!({
            "state": "outdated", "missing": [],
            "nodeVersion": null, "claudeVersion": null,
            "expectedNodeVersion": node, "expectedClaudeVersion": claude,
            "recipeVersion": null, "expectedRecipeVersion": recipe,
        }),
        (true, true) => projection(Some(node), Some(claude), Some(recipe)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_pins_a_node_lts_and_claude_code() {
        let (recipe, node, claude) = expected();
        assert!(recipe >= 1);
        assert_eq!(node, "24.21.0");
        assert_eq!(claude, "2.1.287");
    }

    #[test]
    fn the_lock_pins_codex_for_every_platform_from_its_official_release() {
        let codex = codex_pin();
        assert_eq!(codex.version, "0.167.0");
        assert_eq!(expected_agent_version(AgentId::Codex), "0.167.0");
        assert_eq!(expected_agent_version(AgentId::ClaudeCode), "2.1.287");
        let release = format!(
            "https://github.com/openai/codex/releases/download/rust-v{}/",
            codex.version
        );
        for (asset, target) in [
            (&codex.assets.linux_arm64, "aarch64-unknown-linux-musl"),
            (&codex.assets.linux_amd64, "x86_64-unknown-linux-musl"),
            (&codex.assets.macos_arm64, "aarch64-apple-darwin"),
        ] {
            assert_eq!(asset.url, format!("{release}codex-{target}.tar.gz"));
            assert_eq!(asset.member, format!("codex-{target}"));
            assert_eq!(asset.sha256.len(), 64);
            assert!(asset
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
            assert!(asset.bytes > 0);
        }
    }

    #[test]
    fn each_agent_has_its_own_setup_script_and_the_old_action_installs_claude_code() {
        let claude = setup_script_for(AgentId::ClaudeCode);
        let codex = setup_script_for(AgentId::Codex);
        assert!(claude.trim_end().ends_with("setup claude-code"));
        assert!(codex.trim_end().ends_with("setup codex"));
        assert_eq!(setup_script(), claude);
    }

    #[test]
    fn the_scripts_are_staged_without_unresolved_markers() {
        for script in [status_command(), setup_script()] {
            assert!(script.contains("\"recipeVersion\""));
            assert!(script.contains("agent-tools.py\" \"$silo_tools/agent-tools-lock.json\""));
        }
        assert!(setup_script().trim_end().ends_with("setup claude-code"));
        assert!(status_command().contains("status 2>/dev/null"));
    }

    #[test]
    fn nothing_installed_is_missing_both() {
        for line in [None, Some(""), Some("not json"), Some("{}")] {
            let state = linux_state(line);
            assert_eq!(state["state"], "missing");
            assert_eq!(state["missing"], json!(["node", "claude"]));
        }
    }

    #[test]
    fn a_half_installed_computer_names_what_is_missing() {
        let state = linux_state(Some(
            r#"{"nodeVersion":"24.21.0","claudeVersion":null,"recipeVersion":null}"#,
        ));
        assert_eq!(state["state"], "missing");
        assert_eq!(state["missing"], json!(["claude"]));
        assert_eq!(state["nodeVersion"], "24.21.0");
    }

    #[test]
    fn an_older_recipe_is_outdated_and_the_current_one_is_ready() {
        let (recipe, node, claude) = expected();
        let line = |recipe: u32| {
            format!(
                r#"{{"nodeVersion":"{node}","claudeVersion":"{claude}","recipeVersion":{recipe}}}"#
            )
        };
        assert_eq!(linux_state(Some(&line(recipe - 1)))["state"], "outdated");
        let ready = linux_state(Some(&line(recipe)));
        assert_eq!(ready["state"], "ready");
        assert_eq!(ready["expectedRecipeVersion"], recipe);
    }

    #[test]
    fn macos_follows_the_developer_tools_fingerprint() {
        assert_eq!(macos_state(false, false)["state"], "missing");
        assert_eq!(macos_state(true, false)["state"], "outdated");
        let ready = macos_state(true, true);
        assert_eq!(ready["state"], "ready");
        assert_eq!(ready["missing"], json!([]));
        assert_eq!(ready["nodeVersion"], "24.21.0");
    }
}
