//! The engine's own Keychain items: which ones it can use, and the user-started repair of
//! the ones it was refused (a self-signed engine build is outside the access list of items
//! an earlier build made; see 95 §3, Keychain with self-signed builds).

use super::common::EmptyParams;

pub type EngineCredentialsStatusParams = EmptyParams;
pub type EngineCredentialsRepairParams = EmptyParams;

/// Which of the engine's Keychain items this is.
#[derive(
    Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum CredentialKind {
    /// The GitHub sign-in.
    GithubSignIn,
    /// The ledger of GitHub tokens handed to running computers.
    GithubGrants,
    /// A GitHub personal access token.
    GithubPersonalToken,
    /// The secrets vault.
    SecretsVault,
    /// A macOS computer's proxy certificate authority (`computerId` names the computer).
    ProxyCa,
}

/// Whether the engine can use an item without asking.
#[derive(
    Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum CredentialState {
    Accessible,
    /// The item exists, but this engine build is not allowed to use it.
    Denied,
    /// The Keychain failed in some other way.
    Failed,
}

/// What a repair did for one item.
#[derive(
    Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum CredentialRepairOutcome {
    /// It was usable already; nothing was asked.
    AlreadyAccessible,
    /// macOS asked and the user allowed it.
    Repaired,
    /// macOS asked and the user declined, or could not ask.
    StillDenied,
    Failed,
}

wire_struct! {
    CredentialItem {
        kind: CredentialKind,
        /// Short name to show.
        label: String,
        /// Set for `proxyCa`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
        state: CredentialState,
    }
}
wire_struct! {
    #[doc = "Only items that exist are listed; an item the engine never made is not a problem."]
    CredentialsStatus { items: Vec<CredentialItem>, denied_count: u32 }
}
wire_struct! {
    CredentialRepairItem {
        kind: CredentialKind,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
        outcome: CredentialRepairOutcome,
    }
}
wire_struct! {
    #[doc = "`status` is read again after the repair, with Keychain prompts off again."]
    CredentialsRepairResult { items: Vec<CredentialRepairItem>, status: CredentialsStatus }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_uses_the_wire_names() {
        let status = CredentialsStatus {
            items: vec![CredentialItem {
                kind: CredentialKind::ProxyCa,
                label: "Proxy certificate".into(),
                computer_id: Some("c1".into()),
                state: CredentialState::Denied,
            }],
            denied_count: 1,
        };
        let wire = serde_json::to_value(&status).unwrap();
        assert_eq!(wire["deniedCount"], 1);
        assert_eq!(wire["items"][0]["kind"], "proxyCa");
        assert_eq!(wire["items"][0]["computerId"], "c1");
        assert_eq!(wire["items"][0]["state"], "denied");
        assert_eq!(
            serde_json::from_value::<CredentialsStatus>(wire).unwrap(),
            status
        );
    }
}
