//! Public secret metadata and mutation requests. Secret values are write-only.

use super::common::{ComputerTargetParams, DeviceParams};

pub type SecretsListParams = DeviceParams;

wire_struct! {
    Secret {
        id: String,
        name: String,
        /// `generic` secrets reach a computer as a placeholder for the hosts they allow;
        /// `github-token` secrets reach it only as its GitHub access. Absent on older engines.
        #[serde(default)]
        kind: SecretKind,
        /// For a `github-token` secret, the computers whose GitHub access uses it.
        computer_ids: Vec<String>,
        allowed_domains: Vec<String>,
        state: SecretState,
        pending_computer_ids: Vec<String>,
        removing: bool,
        error: Option<String>,
        /// The GitHub account a `github-token` secret belongs to, once GitHub confirmed it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account: Option<String>,
        /// Whether GitHub accepts a `github-token` secret.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        connection: Option<SecretConnection>,
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    serde::Deserialize,
    PartialEq,
    Eq,
    serde::Serialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum SecretKind {
    #[default]
    Generic,
    GithubToken,
}

wire_struct! {
    SecretConnection {
        state: SecretConnectionState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    }
}

#[derive(
    Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum SecretConnectionState {
    Connected,
    Disconnected,
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
    /// A `github-token` secret takes no computers and no hosts here.
    #[serde(default)]
    pub kind: SecretKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub computer_ids: Vec<String>,
    pub allowed_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_any_destination: Option<bool>,
    /// Confirms that the computers using the secret may reach every site under each wildcard
    /// host over shared hosting among `allowed_domains`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_shared_hosting: Option<bool>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_from_older_engines_are_generic_and_token_secrets_round_trip() {
        let old: Secret = serde_json::from_value(serde_json::json!({
            "id": "1", "name": "API_KEY", "computerIds": [], "allowedDomains": ["example.com"],
            "state": "active", "pendingComputerIds": [], "removing": false, "error": null
        }))
        .unwrap();
        assert_eq!(old.kind, SecretKind::Generic);
        assert_eq!(old.account, None);
        let json = serde_json::to_value(&old).unwrap();
        assert_eq!(json["kind"], "generic");
        assert!(json.get("account").is_none() && json.get("connection").is_none());

        let token = serde_json::json!({
            "id": "2", "name": "work", "kind": "github-token", "computerIds": ["c1"],
            "allowedDomains": [], "state": "active", "pendingComputerIds": [], "removing": false,
            "error": null, "account": "alice",
            "connection": {"state": "disconnected", "message": "GitHub rejected this token."}
        });
        let parsed: Secret = serde_json::from_value(token.clone()).unwrap();
        assert_eq!(parsed.kind, SecretKind::GithubToken);
        assert_eq!(
            parsed.connection.as_ref().unwrap().state,
            SecretConnectionState::Disconnected
        );
        assert_eq!(serde_json::to_value(&parsed).unwrap(), token);
    }

    #[test]
    fn a_save_request_defaults_to_a_generic_secret() {
        let request: SecretSaveRequest = serde_json::from_value(serde_json::json!({
            "operation": "add", "name": "API_KEY", "value": "v",
            "computerIds": [], "allowedDomains": []
        }))
        .unwrap();
        assert_eq!(request.kind, SecretKind::Generic);
        let token: SecretSaveRequest = serde_json::from_value(serde_json::json!({
            "operation": "add", "name": "work", "kind": "github-token", "value": "v",
            "computerIds": [], "allowedDomains": []
        }))
        .unwrap();
        assert_eq!(token.kind, SecretKind::GithubToken);
    }
}
