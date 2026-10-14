//! Agent tools in a computer: a pinned Node.js LTS runtime and the Claude Code CLI.
//!
//! Fjord's guest hook, plugin and run supervisor are JavaScript, and Triage runs are
//! headless `claude -p`; both need these in every computer. Linux computers get them from
//! `guest/agent-tools.py` (an explicit `setup-agent-tools` action that is safe to repeat);
//! macOS computers get them from the developer tools step (`macos_computers::developer_tools`),
//! so the template fingerprint covers them. Silo never signs in to Claude Code.
use serde::Deserialize;
use serde_json::{json, Value};

const LOCK: &str = include_str!("../../guest/agent-tools-lock.json");
const SCRIPT: &str = include_str!("../../guest/agent-tools.py");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lock {
    recipe_version: u32,
    node: Node,
    claude_code: ClaudeCode,
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

/// The versions this Silo installs: `(recipe, node, claude)`.
fn expected() -> (u32, String, String) {
    let lock = lock();
    (lock.recipe_version, lock.node.version, lock.claude_code.version)
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

/// Guest script (run as root) that installs or updates the agent tools.
pub(crate) fn setup_script() -> String {
    let mut script = String::from("set -eu\n");
    script.push_str(&stage(
        "python3 \"$silo_tools/agent-tools.py\" \"$silo_tools/agent-tools-lock.json\" setup",
    ));
    script
}

fn version_of(raw: &Value, key: &str) -> Option<String> {
    raw[key].as_str().filter(|v| !v.is_empty()).map(str::to_owned)
}

fn projection(
    node: Option<String>,
    claude: Option<String>,
    recipe: Option<u32>,
) -> Value {
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
        raw["recipeVersion"].as_u64().and_then(|v| u32::try_from(v).ok()),
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
    fn the_scripts_are_staged_without_unresolved_markers() {
        for script in [status_command(), setup_script()] {
            assert!(script.contains("\"recipeVersion\""));
            assert!(script.contains("agent-tools.py\" \"$silo_tools/agent-tools-lock.json\""));
        }
        assert!(setup_script().trim_end().ends_with("setup"));
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
        let state = linux_state(Some(r#"{"nodeVersion":"24.21.0","claudeVersion":null,"recipeVersion":null}"#));
        assert_eq!(state["state"], "missing");
        assert_eq!(state["missing"], json!(["claude"]));
        assert_eq!(state["nodeVersion"], "24.21.0");
    }

    #[test]
    fn an_older_recipe_is_outdated_and_the_current_one_is_ready() {
        let (recipe, node, claude) = expected();
        let line = |recipe: u32| {
            format!(r#"{{"nodeVersion":"{node}","claudeVersion":"{claude}","recipeVersion":{recipe}}}"#)
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
