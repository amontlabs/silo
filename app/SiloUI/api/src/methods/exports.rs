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
        /// The exact file to write. When set, `destination` is not matched against the
        /// remembered export folder and the file must not exist yet.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        archive_path: Option<String>,
        /// Write the export into this machine's transfer area, under a name the machine picks,
        /// so another machine can fetch it over Connections. `destination` and `archivePath`
        /// are then ignored; the finished job's `archive.transfer` names the file.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transfer_area: Option<bool>,
        /// Keep the logins found on the computer in the file. By default an export of a Linux
        /// computer is made from a copy with the known login files removed; `archive.leftOut`
        /// of the result names them. Traces of them may remain in the file's disk layers.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        include_logins: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    ExportCheckParams {
        computer_id: String,
        /// The export packages this checkpoint rather than the live disk.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkpoint_id: Option<String>,
        /// The export keeps the logins in the file, so none will be left out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        include_logins: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    ExportCheckResult {
        /// Logins found, by tool name and path.
        logins: Vec<String>,
        /// `disk` and `fork-time record`: the places that were listed.
        checked: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unchecked: Option<String>,
        /// The export removes these logins from the computer it writes: it is made from a copy
        /// they are removed from. Traces of them may remain in the file's disk layers.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        leaves_out: bool,
    }
}

wire_struct! {
    ExportImportParams {
        /// The file to import. Empty when `transferName` names a file in the transfer area.
        #[serde(default)]
        archive_path: String,
        new_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_name: Option<String>,
        /// A file this machine received into its transfer area (`exports.transfer`), by the
        /// name the machine gave it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transfer_name: Option<String>,
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

wire_struct! {
    ExportDiscardParams {
        /// A file of the transfer area, by the name the machine gave it.
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ExportTransferDirection {
    /// Fetch an export file from the other machine into `path`.
    Receive,
    /// Send the file at `path` to the other machine, which keeps it for an import.
    Send,
}

wire_struct! {
    ExportTransferParams {
        /// The other machine; the copy runs from this engine over Connections.
        device_id: String,
        direction: ExportTransferDirection,
        /// A path on this machine: where a received file is written (it must not exist) or
        /// the file to send.
        path: String,
        /// For `receive`: the export file in the other machine's transfer area, with the size
        /// and sha256 its export job reported.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bytes: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha256: Option<String>,
        /// The computer the file belongs to, for progress text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer: Option<String>,
    }
}

wire_struct! {
    /// A file in a transfer area, as its owner reports it.
    TransferFile {
        name: String,
        bytes: u64,
        sha256: String,
    }
}

pub type ExportTransferResult = JobStartResult;
pub type ExportTransferFinishedResult = TransferFile;
pub type ExportDiscardResult = bool;
pub type ExportCreateResult = JobStartResult;
pub type ExportImportResult = JobStartResult;
pub type ExportInspectResult = JobStartResult;
pub type ExportCheckStartResult = JobStartResult;
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
        /// Set when the export was written into the transfer area.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transfer: Option<TransferFile>,
        /// The login files the export removed from the computer it wrote, by the name the scan
        /// gives them. Traces of them may remain in the file's disk layers.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        left_out: Vec<String>,
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
