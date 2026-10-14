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
/// `app/SiloUI/src-tauri/src/computer_use.rs`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseState {
    pub state: ComputerUseInstallState,
    pub reason: Option<String>,
    pub cause: Option<ComputerUseCause>,
    pub compatibility: Option<ComputerUseCompatibility>,
    pub warning: Option<String>,
    pub approval: Option<ComputerUseApprovalState>,
    pub applied_approval: Option<ComputerUseApprovalState>,
    pub approval_apply: Option<ComputerUseApprovalApplyState>,
    pub approval_apply_reason: Option<String>,
    pub app_version: Option<String>,
    pub runtime_version: Option<String>,
    pub lcu_version: Option<String>,
    pub agents: Option<Vec<String>>,
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
