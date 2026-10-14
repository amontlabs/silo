//! Local command construction methods.

use super::common::ComputerTargetParams;

wire_struct! { TerminalCommandParams { computer_id: String, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String> } }
wire_struct! { EditorCommandParams { computer_id: String, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String>, #[serde(default, skip_serializing_if = "Option::is_none")] path: Option<String> } }
wire_struct! { CommandResult { argv: Vec<String> } }
pub type EditorIncludeNoticeParams = super::common::EmptyParams;
pub type EditorIncludeNoticeResult = Option<String>;
pub type TerminalTargetParams = ComputerTargetParams;
