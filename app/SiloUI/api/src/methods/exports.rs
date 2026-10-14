//! Export and import state, jobs, and archive inspection.

use serde::{Deserialize, Serialize};

use super::common::JobStartResult;

wire_struct! {
    ExportStateParams {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    ExportDestinationSetParams {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    ExportDestinationResult {
        destination: String,
    }
}

wire_struct! {
    ExportInspectParams {
        archive_path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    ExportCreateParams {
        destination: String,
        computer_ids: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkpoint_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    ExportImportParams {
        archive_path: String,
        new_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    ExportAcknowledgeParams {
        expected_operation_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    ExportDismissParams {
        expected_operation: BackupOperation,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_operation_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

pub type ExportCreateResult = JobStartResult;
pub type ExportImportResult = JobStartResult;
pub type ExportInspectResult = JobStartResult;
pub type ExportDismissResult = bool;
pub type ExportAcknowledgeResult = bool;
pub type ExportStateResult = BackupState;

wire_struct! {
    Archive {
        name: String,
        archive_path: String,
        completed_label: String,
        size: String,
        destination: String,
        computers: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkpoint_name: Option<String>,
    }
}

wire_struct! {
    BackupPhase {
        title: String,
        detail: String,
        tone: String,
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BackupOperation {
    #[serde(rename = "running")]
    Running {
        operation: BackupOperationKind,
        archive: Archive,
        running_names: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_name: Option<String>,
        progress: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        indeterminate: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        can_cancel: Option<bool>,
        phases: Vec<BackupPhase>,
    },
    #[serde(rename = "result")]
    Result {
        operation: BackupOperationKind,
        archive: Archive,
        running_names: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_name: Option<String>,
        outcome: BackupOutcome,
        title: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BackupOperationKind {
    Backup,
    Restore,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BackupOutcome {
    Success,
    Failed,
    Cancelled,
}

/// Mirrors `app/SiloUI/src-tauri/engine/src/backup_controller.rs::BackupState`, with
/// the API's remembered destination field added for a client-owned folder picker.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BackupState {
    pub snapshot_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub availability: BackupAvailability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability_message: Option<String>,
    pub archives: Vec<Archive>,
    pub operation: Option<BackupOperation>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub result_unseen: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BackupAvailability {
    Available,
    Unavailable,
}

wire_struct! {
    ArchiveInspectionResult {
        archive: Archive,
        valid: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    }
}
