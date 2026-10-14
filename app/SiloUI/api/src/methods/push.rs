//! Host Push repository discovery and operation methods.

use super::common::JobStartResult;
use serde::{Deserialize, Serialize};

wire_struct! {
    PushRepositoriesParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh: Option<bool>,
    }
}

wire_struct! {
    PushRepository {
        path: String,
        branch: String,
        ahead: u64,
        behind: u64,
        dirty: bool,
        head: Option<String>,
        repository: Option<String>,
    }
}

wire_struct! {
    PushStartParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        repository_path: String,
        operation_id: String,
        target: PushTarget,
    }
}

wire_struct! {
    PushTarget {
        repository: String,
        branch: String,
        commit: String,
    }
}

wire_struct! {
    PushStatusParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        repository_path: String,
        operation_id: String,
    }
}

wire_struct! {
    PushDismissParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        repository_path: String,
    }
}

pub type PushStartResult = JobStartResult;

/// Mirrors the push-operation value recorded by
/// `app/SiloUI/src-tauri/engine/src/host_push_operations.rs`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PushOperation {
    pub operation_id: String,
    pub computer: String,
    pub repository_path: String,
    pub status: PushOperationStatus,
    pub commit_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub target: PushTarget,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PushOperationStatus {
    Pushing,
    Succeeded,
    Unknown,
    Failed,
}
