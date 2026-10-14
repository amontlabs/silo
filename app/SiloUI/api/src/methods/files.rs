//! Files page methods and transfer jobs.

use serde::{Deserialize, Serialize};

use super::common::JobStartResult;

wire_struct! {
    FileListParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        path: String,
        offset: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot_id: Option<String>,
    }
}

wire_struct! {
    FileListResult {
        entries: Vec<FileEntry>,
        next_offset: Option<u64>,
        snapshot_id: String,
    }
}

wire_struct! {
    FileEntry {
        name: String,
        path: String,
        kind: FileEntryKind,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FileEntryKind {
    Folder,
    File,
    Symlink,
}

wire_struct! {
    FileUploadParams {
        transfer_id: String,
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        directory: String,
        conflict: ConflictPolicy,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selection: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        paths: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<UploadOrigin>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        connection_id: Option<String>,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    Ask,
    Replace,
    KeepBoth,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum UploadOrigin {
    Client,
    Viewer,
}

wire_struct! {
    FileDownloadParams {
        transfer_id: String,
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<String>,
    }
}

pub type FileUploadResult = JobStartResult;
pub type FileDownloadResult = JobStartResult;
pub type FileUploadFinishedResult = UploadOutcome;
pub type FileDownloadFinishedResult = DownloadOutcome;

/// Mirrors `app/SiloUI/src-tauri/engine/src/transfer.rs::Progress`; the serialized
/// names deliberately retain `id` and `computer`, not the surrounding job's
/// `jobId` and `computerId`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub id: String,
    pub computer: String,
    pub direction: TransferDirection,
    pub state: TransferState,
    pub name: String,
    pub file_index: usize,
    pub file_count: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TransferDirection {
    Upload,
    Download,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TransferState {
    Transferring,
    Done,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum UploadOutcome {
    Conflict { names: Vec<String> },
    Done { names: Vec<String> },
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum DownloadOutcome {
    Done { path: String },
    Cancelled,
}
