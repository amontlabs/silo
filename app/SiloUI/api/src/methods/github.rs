//! GitHub authorization, repository policy, and state contracts.

use serde::{Deserialize, Serialize};

use super::common::{DeviceParams, EmptyParams, JobStartResult};

pub type GitHubStateParams = DeviceParams;
pub type GitHubConnectParams = DeviceParams;
pub type GitHubConnectReopenParams = DeviceParams;
pub type GitHubDisconnectParams = DeviceParams;
pub type GitHubPersonalTokenRemoveParams = DeviceParams;
pub type GitHubRepositoriesRefreshParams = DeviceParams;

wire_struct! {
    GitHubAccessSetParams {
        enabled: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    GitHubConfigurationRetryParams {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
/// Public projection of `app/SiloUI/src-tauri/src/github.rs::public_snapshot`;
/// `computer` references are replaced with canonical `computerId` identifiers.
pub struct GitHubPolicy {
    pub computer_id: String,
    pub identity: GitHubIdentity,
    pub repository_mode: GitHubRepositoryMode,
    pub repositories: Vec<GitHubRepositoryPolicy>,
    pub all_repositories_allow_changes: bool,
    pub authentication_method: GitHubAuthenticationMethod,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRepositoryPolicy {
    pub repository: String,
    pub allow_pushes: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitHubIdentity {
    pub name: String,
    pub email: String,
    pub apply: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GitHubRepositoryMode {
    Selected,
    All,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GitHubAuthenticationMethod {
    Oauth,
    Token,
}

wire_struct! {
    GitHubConfiguration {
        computers: Vec<GitHubPolicy>,
        base_revision: Option<u64>,
    }
}

wire_struct! {
    GitHubConfigurationSaveParams {
        configuration: GitHubConfiguration,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! { GitHubConnectCallbackParams { job_id: crate::jobs::JobId, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String> } }

wire_struct! {
    GitHubPersonalTokenSaveParams {
        token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

pub type GitHubRepositoriesManageParams = DeviceParams;

wire_struct! {
    GitHubRepositoriesManageResult {
        url: String,
    }
}

pub type GitHubConnectResult = JobStartResult;
pub type GitHubCommandResult = GitHubState;
pub type GitHubCallbackResult = ();
pub type GitHubNoParams = EmptyParams;

/// Public projection of `app/SiloUI/src-tauri/src/github.rs::public_snapshot`
/// from 50 §2. Token values and other credential material are absent.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitHubState {
    pub policy_revision: u64,
    pub personal_token: GitHubPersonalToken,
    pub state: GitHubConnectionState,
    pub account: Option<String>,
    pub access_enabled: bool,
    pub device_identity: Option<GitHubDeviceIdentity>,
    pub repository_catalog: Vec<String>,
    pub repository_catalog_status: GitHubRepositoryCatalogStatus,
    pub computers: Vec<GitHubPolicy>,
    pub computer_operations: Vec<GitHubComputerOperation>,
    pub pending_authorization_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPersonalToken {
    pub state: GitHubPersonalTokenState,
    pub saved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GitHubPersonalTokenState {
    Connected,
    Disconnected,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GitHubConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitHubDeviceIdentity {
    pub name: String,
    pub email: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(
    tag = "status",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum GitHubRepositoryCatalogStatus {
    Available,
    Unavailable { message: String, can_retry: bool },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitHubComputerOperation {
    pub computer_id: String,
    pub status: GitHubComputerOperationStatus,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_retry: Option<bool>,
    /// 50 §2 leaves the nested diagnostic details shape unspecified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_details: Option<serde_json::Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GitHubComputerOperationStatus {
    Applying,
    Succeeded,
    Failed,
}
