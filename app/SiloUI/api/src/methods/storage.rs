//! Storage reads and reclamation jobs.

use super::common::JobStartResult;
use serde::{Deserialize, Serialize};

wire_struct! {
    StorageParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

pub type StorageReclaimResult = JobStartResult;

wire_struct! {
    StorageState {
        history: Vec<ReclaimEntry>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_host_bytes: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runtime_host_bytes: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkpoint_host_bytes: Option<u64>,
        checkpoint_count: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_used_bytes: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_capacity_bytes: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_reclaimed_bytes: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_trim_at: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_error: Option<String>,
    }
}

wire_struct! {
    ReclaimEntry {
        at: String,
        trigger: ReclaimTrigger,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reclaimed_bytes: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ReclaimTrigger {
    Manual,
    AfterStart,
    BeforeStop,
    Scheduled,
}
