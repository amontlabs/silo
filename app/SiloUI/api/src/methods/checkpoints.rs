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
/// `app/SiloUI/src-tauri/src/runtime/checkpoints.rs` (lines 2381-2403).
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
}

wire_struct! {
    CheckpointCreatedResult {
        checkpoint: Checkpoint,
    }
}
