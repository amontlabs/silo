//! Approvals: a call a computer's agent made that widens what it has waits here until the
//! person who runs Silo allows or denies it. On allow the engine runs the stored call itself.
//! `computers.access` is what a computer holds, so an agent knows what it can hand on and what
//! it must ask for. Approvals are answered on the device whose computer asked, so these
//! methods take no device.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::network::PublishedPort;
use crate::transport::ApprovalKind;

/// Where an approval stands.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalState {
    /// Waiting for an answer.
    Pending,
    /// Allowed; the engine ran the call, or is running it while `outcome` is absent.
    Allowed,
    Denied,
    /// Withdrawn by the computer that asked, or ended because that computer was deleted or
    /// the review request it wraps was closed.
    Cancelled,
    /// Nobody answered within seven days.
    Expired,
}

/// The answer to an approval.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalDecision {
    Allow,
    Deny,
}

wire_struct! {
    /// What the allowed call returned: its result, the job it started, or the error it ended
    /// with.
    ApprovalOutcome {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        /// The job the call started; the computer that asked may read it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job_id: Option<String>,
        /// Why the call did not run, or how it failed, in plain words.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    }
}

wire_struct! {
    /// A call held for the owner's answer. Its method, parameters and summary never change
    /// after it is made.
    Approval {
        id: String,
        kind: ApprovalKind,
        /// The computer whose agent asked.
        computer_id: String,
        /// That computer's name when it asked.
        computer_name: String,
        /// The method that runs on allow.
        method: String,
        /// The parameters that run on allow, as the engine checked them. A secret's value is
        /// never shown.
        params: Value,
        /// What is asked, written by the engine from the parameters.
        summary: String,
        /// The agent's own words on why it asks. Never part of the summary.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        state: ApprovalState,
        /// RFC 3339.
        created_at: String,
        /// When a pending approval expires (RFC 3339).
        expires_at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decided_at: Option<String>,
        /// The program the owner answered with (diagnostic only).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decided_by: Option<String>,
        /// The owner's note with the answer; the agent reads it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decision_note: Option<String>,
        /// Why an approval was cancelled or expired, in plain words.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<ApprovalOutcome>,
        /// The review request a merge shows its diff with (`changes.requests.show`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        review_request_id: Option<String>,
        /// The digest of the changes the merge is pinned to.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        review_digest: Option<String>,
    }
}

wire_struct! {
    ApprovalsListParams {
        /// Only approvals in this state.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<ApprovalState>,
        /// Only approvals this computer asked for.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
    }
}

wire_struct! {
    ApprovalsListResult {
        /// Newest first.
        approvals: Vec<Approval>,
    }
}

wire_struct! {
    ApprovalIdParams {
        id: String,
    }
}

pub type ApprovalsGetParams = ApprovalIdParams;
pub type ApprovalsCancelParams = ApprovalIdParams;

wire_struct! {
    ApprovalsDecideParams {
        id: String,
        decision: ApprovalDecision,
        /// Shown to the agent with the answer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    }
}

wire_struct! {
    ApprovalsNoteParams {
        id: String,
        /// At most 500 characters; an empty note removes it.
        note: String,
    }
}

/// The longest note an agent may attach to its approval.
pub const APPROVAL_NOTE_MAX_CHARS: usize = 500;

wire_struct! {
    /// The payload of `approvals.changed`.
    ApprovalsChangedPayload {
        id: String,
        state: ApprovalState,
    }
}

wire_struct! {
    ComputersAccessParams {
        computer_id: String,
    }
}

wire_struct! {
    /// A secret a computer holds: its name and where it may be sent. Never its value.
    ComputerAccessSecret {
        name: String,
        hosts: Vec<String>,
    }
}

wire_struct! {
    ComputerAccessRepository {
        /// `owner/name`.
        repository: String,
        /// Whether the computer may push to it.
        write: bool,
    }
}

wire_struct! {
    ComputerAccessGitHub {
        /// `oauth` (the signed-in account) or `token` (a personal token).
        method: String,
        /// Every repository the account reaches.
        all_repositories: bool,
        /// With `allRepositories`: whether it may push to them.
        all_repositories_write: bool,
        repositories: Vec<ComputerAccessRepository>,
    }
}

wire_struct! {
    ComputerAccessSsh {
        enabled: bool,
        port: u16,
        /// `disabled`, `waiting`, `listening` or `error`.
        state: String,
    }
}

wire_struct! {
    /// What one computer holds.
    ComputerAccess {
        computer_id: String,
        name: String,
        secrets: Vec<ComputerAccessSecret>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        github: Option<ComputerAccessGitHub>,
        /// Its ports published on this device.
        ports: Vec<PublishedPort>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ssh: Option<ComputerAccessSsh>,
        /// Which hosts it may reach.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        network_policy: Option<super::network::NetworkPolicy>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_and_decisions_use_their_wire_names() {
        assert_eq!(
            serde_json::to_value(ApprovalState::Expired).unwrap(),
            "expired"
        );
        let params: ApprovalsDecideParams =
            serde_json::from_str(r#"{"id":"ap-1","decision":"deny","note":"not now"}"#).unwrap();
        assert_eq!(params.decision, ApprovalDecision::Deny);
        assert_eq!(params.note.as_deref(), Some("not now"));
    }
}
