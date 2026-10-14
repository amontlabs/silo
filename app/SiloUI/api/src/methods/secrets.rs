//! Public secret metadata and mutation requests. Secret values are write-only.

use super::common::{ComputerTargetParams, DeviceParams};

pub type SecretsListParams = DeviceParams;

wire_struct! {
    Secret {
        id: String,
        name: String,
        computer_ids: Vec<String>,
        allowed_domains: Vec<String>,
        state: SecretState,
        pending_computer_ids: Vec<String>,
        removing: bool,
        error: Option<String>,
    }
}

#[derive(
    Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum SecretState {
    Active,
    Applying,
    RestartRequired,
}

#[derive(Clone, Debug, serde::Deserialize, PartialEq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretSaveRequest {
    pub operation: SecretSaveAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub computer_ids: Vec<String>,
    pub allowed_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_any_destination: Option<bool>,
}

#[derive(
    Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum SecretSaveAction {
    Add,
    Edit,
}

wire_struct! { SecretsSaveParams { request: SecretSaveRequest, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String> } }
wire_struct! { SecretIdParams { id: String, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String> } }
pub type SecretsRemoveParams = SecretIdParams;
pub type SecretsRetryParams = SecretIdParams;
pub type SecretsSaveResult = Secret;
pub type SecretsChangeResult = ();
pub type SecretsComputerParams = ComputerTargetParams;
