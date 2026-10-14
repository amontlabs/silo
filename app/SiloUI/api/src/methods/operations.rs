//! Queue and retained job lookup/cancellation methods.

use serde::{Deserialize, Serialize};

wire_struct! {
    OperationsListParams {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    CancelJobParams {
        job_id: crate::jobs::JobId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    GetJobParams {
        job_id: crate::jobs::JobId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    AcknowledgeJobParams {
        job_id: crate::jobs::JobId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

/// Mirrors `app/SiloUI/src-tauri/src/runtime/operation_gate.rs::OperationKind`
/// and its camelCase serde names.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    Lifecycle,
    CheckpointCapture,
    CheckpointRestore,
    CheckpointFork,
    CheckpointDelete,
    Export,
    Import,
    StorageReclaim,
    GithubApply,
    Push,
    PortPublish,
    PortRemove,
    Shutdown,
    ComputerConfiguration,
    #[default]
    Other,
}

/// Mirrors `app/SiloUI/src-tauri/src/runtime/operation_gate.rs::OperationEntry`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationEntry {
    pub id: u64,
    pub label: String,
    pub kind: OperationKind,
    #[serde(default)]
    pub computer_id: Option<String>,
    #[serde(default)]
    pub computer_name: Option<String>,
    pub since_ms: u64,
    pub cancellable: bool,
    #[serde(default)]
    pub expected_ms: Option<u64>,
    #[serde(default)]
    pub blocked_by_hidden: bool,
    /// Added by the canonical job contract; absent for non-job queue entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<crate::jobs::JobId>,
}

/// Mirrors `app/SiloUI/src-tauri/src/runtime/operation_gate.rs::OperationQueue`
/// and adds the retained completed-job discovery list required by 95 §3.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationQueue {
    pub running: Vec<OperationEntry>,
    pub waiting: Vec<OperationEntry>,
    pub unacknowledged_job_ids: Vec<crate::jobs::JobId>,
}

pub type OperationsListResult = OperationQueue;
pub type OperationsCancelResult = ();
