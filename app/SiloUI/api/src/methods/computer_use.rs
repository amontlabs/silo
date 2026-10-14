//! Approval and setup methods for computer-use support.

use super::common::ComputerTargetParams;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalMode {
    Ask,
    Auto,
}

wire_struct! {
    ComputerUseApprovalParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        mode: ApprovalMode,
    }
}

pub type ComputerUseSetupParams = ComputerTargetParams;
pub type ComputerUseSetupResult = super::common::JobStartResult;

wire_struct! {
    ComputerUseStateParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

/// State projection described by 40 §4 and `computer_use::state_object` in
/// `app/SiloUI/src-tauri/engine/src/computer_use.rs`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseState {
    /// Nullable before the macOS guest receipt has been read.
    pub schema_version: Option<u8>,
    /// Nullable before the macOS guest receipt has been read.
    pub state: Option<ComputerUseInstallState>,
    pub reason: Option<String>,
    pub cause: Option<ComputerUseCause>,
    pub compatibility: Option<ComputerUseCompatibility>,
    pub warning: Option<String>,
    pub approval: Option<ComputerUseApprovalState>,
    pub applied_approval: Option<ComputerUseApprovalState>,
    pub approval_apply: Option<ComputerUseApprovalApplyState>,
    pub approval_apply_reason: Option<String>,
    pub app_version: Option<String>,
    /// macOS receipt field; Linux projections use `appVersion` instead.
    pub chatgpt_version: Option<String>,
    pub runtime_version: Option<String>,
    pub lcu_version: Option<String>,
    pub agents: Option<Vec<String>>,
    /// RFC 3339 receipt time on macOS; null until the receipt has been read.
    pub at: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ComputerUseInstallState {
    Unavailable,
    Preparing,
    Installing,
    Ready,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ComputerUseCause {
    AppDownload,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ComputerUseCompatibility {
    Tested,
    Untested,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ComputerUseApprovalState {
    Ask,
    Auto,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ComputerUseApprovalApplyState {
    Applied,
    Pending,
    Failed,
    Partial,
    Unknown,
}
