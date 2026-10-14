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
        #[serde(default)]
        workspace_host_bytes: Option<u64>,
        #[serde(default)]
        runtime_host_bytes: Option<u64>,
        #[serde(default)]
        checkpoint_host_bytes: Option<u64>,
        checkpoint_count: u64,
        #[serde(default)]
        workspace_used_bytes: Option<u64>,
        #[serde(default)]
        workspace_capacity_bytes: Option<u64>,
        #[serde(default)]
        last_reclaimed_bytes: Option<u64>,
        #[serde(default)]
        last_trim_at: Option<u64>,
        #[serde(default)]
        last_error: Option<String>,
    }
}

wire_struct! {
    ReclaimEntry {
        at: u64,
        trigger: ReclaimTrigger,
        #[serde(default)]
        reclaimed_bytes: Option<u64>,
        #[serde(default)]
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
