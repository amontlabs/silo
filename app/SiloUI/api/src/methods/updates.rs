//! Engine update state and control methods.

use super::common::EmptyParams;
use serde::{Deserialize, Serialize};

pub type UpdatesCheckParams = EmptyParams;
pub type UpdatesDownloadParams = EmptyParams;
pub type UpdatesFocusedParams = EmptyParams;
pub type UpdatesResult = UpdateSnapshot;
pub type UpdatesFocusedResult = ();

wire_struct! {
    UpdatesSetAutomaticChecksParams {
        enabled: bool,
    }
}

/// Mirrors `app/SiloUI/src-tauri/engine/src/updates.rs::Snapshot` with camelCase
/// fields. `installStatus` is excluded by 60 §5.1 even though the current Rust
/// source still serializes its obsolete Debian helper value.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSnapshot {
    pub phase: String,
    pub last_checked: Option<String>,
    pub retry_action: Option<String>,
    pub current_version: String,
    pub available_version: Option<String>,
    pub release_notes: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub automatic_checks: bool,
    pub package_kind: String,
    pub release_url: String,
    pub error: Option<String>,
    pub error_details: Option<String>,
    pub running_computers: Vec<String>,
    pub can_install: bool,
    pub install_block_reason: Option<String>,
}

/// The `operations.job.progress` source projection mirrors the current Rust
/// `updates::Snapshot` at `app/SiloUI/src-tauri/engine/src/updates.rs:41-61` exactly.
/// Method and `updates.changed` results use `UpdateSnapshot`, which omits the
/// obsolete install helper field per 60 §5.1.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgressData {
    pub phase: String,
    pub last_checked: Option<String>,
    pub retry_action: Option<String>,
    pub current_version: String,
    pub available_version: Option<String>,
    pub release_notes: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub automatic_checks: bool,
    pub package_kind: String,
    pub release_url: String,
    pub error: Option<String>,
    pub error_details: Option<String>,
    pub running_computers: Vec<String>,
    pub can_install: bool,
    pub install_block_reason: Option<String>,
    pub install_status: Option<String>,
}
