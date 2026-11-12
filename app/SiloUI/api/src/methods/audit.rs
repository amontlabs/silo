//! `audit.list`: the engine's record of calls that changed something, and of calls it refused
//! or held for the owner's approval. Parameters and results of a call are never recorded.

use serde::{Deserialize, Serialize};

/// Who made a recorded call.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AuditCallerKind {
    /// A program on this device, through the engine's own socket (the app, the command line).
    Owner,
    /// Another device, through Connections.
    Device,
    /// The agent inside a computer.
    Computer,
}

/// How a recorded call ended.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AuditOutcome {
    Ok,
    Error,
    Refused,
    Asked,
}

wire_struct! {
    AuditListParams {
        /// Only rows about this computer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
        /// Only rows of calls this kind of caller made.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caller_kind: Option<AuditCallerKind>,
        /// Only rows older than this row id (the `nextBeforeId` of the previous page).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before_id: Option<u64>,
        /// At most this many rows, newest first; 100 when left out, never more than 500.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    }
}

wire_struct! {
    AuditEntry {
        id: u64,
        /// RFC 3339 time the call was answered.
        at: String,
        caller_kind: AuditCallerKind,
        /// The computer's id for a computer, the key fingerprint for a device when it is known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caller_id: Option<String>,
        /// The name the calling program gave; diagnostic only, never an identity.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client: Option<String>,
        method: String,
        /// The computer the call named, when it named one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
        outcome: AuditOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_code: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approval_id: Option<String>,
        /// How many times the same call was made in a row by the same caller. `at` is the
        /// first, `lastAt` the latest.
        #[serde(default = "one")]
        count: u64,
        /// RFC 3339 time of the latest of the `count` calls.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_at: Option<String>,
    }
}

fn one() -> u64 {
    1
}

wire_struct! {
    AuditListResult {
        entries: Vec<AuditEntry>,
        /// Pass as `beforeId` to read the next, older page; absent on the last page.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_before_id: Option<u64>,
    }
}
