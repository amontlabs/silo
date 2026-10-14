//! Controller-managed SSH access settings and connection handoff.

pub type SshAccessStateParams = super::common::DeviceParams;

wire_struct! {
    SshAccessState {
        computers: Vec<SshAccessComputer>,
    }
}

#[derive(Clone, Debug, serde::Deserialize, PartialEq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SshAccessComputer {
    pub computer: String,
    pub computer_id: String,
    pub enabled: bool,
    pub port: u16,
    pub bind_address: String,
    pub keys: Vec<String>,
    pub state: SshAccessComputerState,
    pub message: Option<String>,
    pub fingerprint: Option<String>,
    pub device_name: String,
    pub addresses: Vec<String>,
    pub user: String,
}

#[derive(
    Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum SshAccessComputerState {
    Disabled,
    Waiting,
    Listening,
    Error,
}

wire_struct! {
    SshAccessSaveParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        enabled: bool,
        port: u16,
        bind_address: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    }
}

#[derive(Clone, Debug, serde::Deserialize, PartialEq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SshAccessConnectionParams {
    pub computer_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<bool>,
    pub download: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
}

pub type SshAccessSaveResult = SshAccessState;

/// `download: false` returns the SSH command string. The true branch returns
/// null in the current Tauri source; 50 does not specify the API destination
/// success result, so this preserves the source's nullable result.
pub type SshAccessConnectionResult = Option<String>;
