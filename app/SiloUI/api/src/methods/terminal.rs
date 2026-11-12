//! Local command construction methods.

use super::common::ComputerTargetParams;

wire_struct! {
    TerminalCommandParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        /// Start this agent in the computer's workspace folder instead of a plain shell. The
        /// engine runs the agent's fixed command; nothing else can be named.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<super::agents::AgentId>,
    }
}
wire_struct! { EditorCommandParams { computer_id: String, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String>, #[serde(default, skip_serializing_if = "Option::is_none")] path: Option<String> } }
wire_struct! { CommandResult { argv: Vec<String> } }
pub type EditorIncludeNoticeParams = super::common::EmptyParams;
pub type EditorIncludeNoticeResult = Option<String>;
pub type TerminalTargetParams = ComputerTargetParams;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::methods::agents::AgentId;

    #[test]
    fn a_terminal_names_an_agent_only_from_the_known_list() {
        let plain: TerminalCommandParams =
            serde_json::from_value(serde_json::json!({"computerId": "c"})).unwrap();
        assert_eq!(plain.agent, None);
        assert!(serde_json::to_value(&plain).unwrap().get("agent").is_none());

        for (name, agent) in [
            ("claude-code", AgentId::ClaudeCode),
            ("codex", AgentId::Codex),
        ] {
            let params: TerminalCommandParams = serde_json::from_value(
                serde_json::json!({"computerId": "c", "deviceId": "d", "agent": name}),
            )
            .unwrap();
            assert_eq!(params.agent, Some(agent));
            assert_eq!(
                serde_json::to_value(params).unwrap(),
                serde_json::json!({"computerId": "c", "deviceId": "d", "agent": name})
            );
        }

        for refused in [
            serde_json::json!("pi"),
            serde_json::json!("claude"),
            serde_json::json!("rm -rf /"),
            serde_json::json!(""),
            serde_json::json!(7),
        ] {
            assert!(serde_json::from_value::<TerminalCommandParams>(
                serde_json::json!({"computerId": "c", "agent": refused})
            )
            .is_err());
        }
    }
}
