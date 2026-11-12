//! Checkpoint jobs and usage reads.

use serde::{Deserialize, Serialize};

use super::common::JobStartResult;

wire_struct! {
    CheckpointCreateParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<CheckpointKind>,
        /// For a `step` checkpoint: how many of the computer's step checkpoints to keep (the
        /// newest ones). The engine keeps 20 when it is left out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep_newest: Option<u32>,
    }
}

wire_struct! {
    CheckpointTargetParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        checkpoint_id: String,
    }
}

wire_struct! {
    CheckpointForkParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        checkpoint_id: Option<String>,
        new_name: String,
        /// Names of Silo-managed secrets the fork may use. The special name `github` copies
        /// GitHub access, read-only. Nothing is copied unless named.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        secrets: Vec<String>,
        /// What to quiet in the fork before it runs anything.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        quiet: Option<QuietSpec>,
        /// The content hash of the recipe this fork was built from. The fork's idle baseline
        /// is cached under it, so every fork of one recipe shares a baseline.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recipe_hash: Option<String>,
        /// The fork's network policy. Absent, the fork keeps its source's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        network_policy: Option<super::network::NetworkPolicy>,
    }
}

wire_struct! {
    /// Things a fork turns off on its first boot, beyond the scheduled jobs it always turns off.
    QuietSpec {
        /// Process names and systemd units to disable in the fork.
        #[serde(default)]
        stop: Vec<String>,
        /// Absolute guest paths (globs allowed) to delete in the fork.
        #[serde(default)]
        scrub: Vec<String>,
    }
}

pub type CheckpointAbandonRestoreParams = crate::methods::common::ComputerTargetParams;
pub type CheckpointCreateResult = JobStartResult;
pub type CheckpointDeleteResult = JobStartResult;
pub type CheckpointForkResult = JobStartResult;
pub type CheckpointRestoreResult = JobStartResult;
pub type CheckpointAbandonRestoreResult = JobStartResult;
pub type CheckpointJobResult = CheckpointCreatedResult;
pub type CheckpointUsageResult = CheckpointUsage;

wire_struct! {
    CheckpointUsageParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    CheckpointUsage {
        total_bytes: Option<u64>,
        checkpoints: Vec<CheckpointUsageEntry>,
    }
}

wire_struct! {
    CheckpointUsageEntry {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_bytes: Option<u64>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        used_by: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delete_blocker: Option<String>,
    }
}

/// The common checkpoint view returned by Linux and macOS computers.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub scope: CheckpointScope,
    pub reason: CheckpointReason,
    pub kind: CheckpointKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// The Linux and macOS forms described in 20 §1.4. Their field names and
/// optionality match `runtime/checkpoints.rs` and `macos_computers/checkpoints.rs`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum PendingRestore {
    Linux {
        checkpoint_id: String,
        source_computer: String,
        state: CheckpointScope,
    },
    Macos {
        checkpoint_id: String,
        memory: bool,
    },
}

/// Unified API view of Linux and macOS checkpoint operations.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub kind: OperationKind,
    pub status: OperationStatus,
    pub stage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum OperationKind {
    Capture,
    Restore,
    Fork,
    Delete,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum OperationStatus {
    Running,
    Failed,
}

/// Mirrors the Linux restore journal view in
/// `app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs` (lines 2381-2403).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnfinishedRestore {
    pub checkpoint_id: String,
    pub checkpoint_name: Option<String>,
    pub phase: RestorePhase,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RestorePhase {
    Capturing,
    Secured,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CheckpointScope {
    Full,
    Disk,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CheckpointReason {
    Manual,
    BeforeRestore,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CheckpointKind {
    Manual,
    Automatic,
    // Taken before changes are applied to main; trimmed by its own retention count.
    #[serde(rename = "before-keep")]
    BeforeKeep,
    // The disks (not the memory) of a computer's current state, which forks start from: made
    // by Fork, or by a create of this kind when several forks share one; kept while a fork
    // made from it exists.
    #[serde(rename = "fork-point")]
    ForkPoint,
    // Read-only: a checkpoint `silo up` of an earlier version kept of main. Nothing makes
    // one now and a create with this kind is refused; it stays so saved ones still load.
    Warm,
    // Taken after a step (a command an agent ran through Silo); trimmed by its own count.
    Step,
}

wire_struct! {
    CheckpointCreatedResult {
        checkpoint: Checkpoint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fork_params_default_to_nothing_copied_and_no_quiet_spec() {
        let params: CheckpointForkParams =
            serde_json::from_str(r#"{"computerId":"c","checkpointId":null,"newName":"n"}"#)
                .unwrap();
        assert!(params.secrets.is_empty());
        assert_eq!(params.quiet, None);
        let text = serde_json::to_string(&params).unwrap();
        assert!(!text.contains("quiet"));
    }

    #[test]
    fn fork_params_carry_secrets_and_quiet_spec() {
        let params: CheckpointForkParams = serde_json::from_str(
            r#"{"computerId":"c","checkpointId":"x","newName":"n","secrets":["github","API_KEY"],"quiet":{"stop":["poller"],"scrub":["/root/.env"]}}"#,
        )
        .unwrap();
        assert_eq!(params.secrets, ["github", "API_KEY"]);
        let quiet = params.quiet.unwrap();
        assert_eq!(quiet.stop, ["poller"]);
        assert_eq!(quiet.scrub, ["/root/.env"]);
        let partial: QuietSpec = serde_json::from_str(r#"{"stop":["a"]}"#).unwrap();
        assert!(partial.scrub.is_empty());
    }
}
