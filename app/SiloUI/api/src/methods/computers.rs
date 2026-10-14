//! Computer inventory, configuration, and lifecycle contracts.

use serde::{Deserialize, Serialize};

use super::common::ComputerTargetParams;

wire_struct! {
    ComputerListParams {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh_repositories: Option<bool>,
    }
}

wire_struct! {
    ComputerShellParams {
        error: String,
    }
}

pub type ComputerShellResult = ApplicationSource;
pub type ComputerSetupActivityParams = super::common::EmptyParams;
pub type ComputerSetupActivityResult = Vec<ComputerConfigurationProgress>;
pub type ComputerConfigurationReadParams = super::common::DeviceParams;
pub type ComputerConfigurationReadResult = ComputerConfigurationRequest;

pub type ComputerLaunchParams = super::common::DeviceParams;
pub type ComputerLaunchResult = LaunchStatus;
pub type ComputerDismissErrorParams = ComputerTargetParams;
pub type ComputerForceStopParams = ComputerTargetParams;
pub type ComputerLifecycleParams = ComputerTargetParams;
wire_struct! { ComputerCreationSkipWaitParams { request_id: String } }
pub type ComputerDeleteResult = ApplicationSource;
wire_struct! { ComputerConfigurationRetryParams { #[serde(default, skip_serializing_if = "Option::is_none")] request_id: Option<String>, #[serde(default, skip_serializing_if = "Option::is_none")] retry_computer_id: Option<String>, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String> } }
wire_struct! { ComputerCreateResult { job_id: crate::jobs::JobId, #[serde(default, skip_serializing_if = "Option::is_none")] computer: Option<MacosComputer> } }
pub type ComputerJobResult = ApplicationSource;
pub type ComputerCommandResult = ApplicationSource;
pub type ComputerIdentitiesVerifyParams = ComputerIdentitiesParams;
pub type ComputerIdentitiesVerifyResult = bool;
pub type ComputerIdentitiesConfigureResult = super::common::JobStartResult;
pub type ComputerConfigurationRetryResult = super::common::JobStartResult;
pub type ComputerConfigurationChangeResult = super::common::JobStartResult;

/// Mirrors `app/SiloUI/src-tauri/src/runtime.rs::ComputerConfiguration`
/// (lines 304-319), including rejection of unknown fields.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComputerConfiguration {
    pub id: String,
    pub name: String,
    pub cpus: u8,
    #[serde(rename = "maxCPUs")]
    pub max_cpus: u8,
    #[serde(rename = "memoryGiB")]
    pub memory_gib: u32,
    #[serde(rename = "maxMemoryGiB")]
    pub max_memory_gib: u32,
    #[serde(rename = "workspaceStorageGiB")]
    pub workspace_storage_gib: u32,
    #[serde(rename = "runtimeStorageGiB")]
    pub runtime_storage_gib: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop: Option<DesktopConfiguration>,
}

/// Mirrors `app/SiloUI/src-tauri/src/desktop.rs::DesktopConfiguration`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopConfiguration {
    #[serde(default = "default_true")]
    pub start_with_computer: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub built_in: bool,
}

fn default_true() -> bool {
    true
}

fn is_false(value: &bool) -> bool {
    !value
}

/// Mirrors `app/SiloUI/src-tauri/src/runtime.rs::ComputerConfigurationRequest`
/// (lines 297-302).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComputerConfigurationRequest {
    pub schema_version: u8,
    pub computers: Vec<ComputerConfiguration>,
}

wire_struct! {
    ComputerConfigurationChangeParams {
        change: ComputerConfigurationChange,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_computer_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ComputerConfigurationChange {
    Upsert {
        configuration: ComputerConfiguration,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected: Option<ComputerConfiguration>,
    },
    Delete {
        computer_id: String,
        expected: ComputerConfiguration,
    },
    Reorder {
        order: Vec<String>,
        expected_order: Vec<String>,
    },
    Batch {
        changes: Vec<ComputerConfigurationChange>,
    },
}

wire_struct! {
    ComputerCreateLinuxRequest {
        configuration: ComputerConfiguration,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    }
}

wire_struct! {
    MacosComputerCreateRequest {
        name: String,
        cpus: u64,
        #[serde(rename = "memoryGiB")]
        memory_gib: u64,
        #[serde(rename = "diskGiB")]
        disk_gib: u64,
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum ComputerCreateParams {
    Linux {
        configuration: ComputerConfiguration,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    },
    Macos {
        request: MacosComputerCreateRequest,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    },
}

wire_struct! {
    ComputerDeleteParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected: Option<ComputerConfiguration>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    }
}

wire_struct! {
    ComputerRequestIdParams {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    ComputerIdentitiesParams {
        identities: Vec<ComputerIdentity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    ComputerIdentity {
        computer_id: String,
        name: String,
        email: String,
        apply: bool,
    }
}

wire_struct! {
    LaunchStatus {
        enabled: bool,
        computer_ids: Vec<String>,
        last_pass: Option<LaunchPass>,
    }
}

wire_struct! {
    LaunchPass {
        finished_at: u64,
        failures: Vec<String>,
    }
}

wire_struct! {
    ComputersListResult {
        linux: ApplicationSource,
        macos: MacosComputersState,
    }
}

/// Mirrors `app/SiloUI/src-tauri/src/runtime.rs::ApplicationSource` (lines
/// 548-565), retaining its field types and camelCase serialization.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSource {
    pub runtime_repair: Option<serde_json::Value>,
    pub computers: Vec<ApplicationComputer>,
    pub activities: Vec<serde_json::Value>,
    pub computer_configuration_operation: Option<serde_json::Value>,
    pub repository_push_operations: Vec<serde_json::Value>,
    pub github: serde_json::Value,
    pub secrets: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_capacity: Option<DeviceCapacity>,
}

/// Mirrors `app/SiloUI/src-tauri/src/runtime.rs::ApplicationComputer` (lines
/// 591-628). `kind` is the discriminator required by 10 §1.3 and is added by
/// the canonical API; source fields retain their names and omission behavior.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationComputer {
    pub kind: ComputerKind,
    pub configuration: ComputerConfiguration,
    pub purpose: String,
    pub state: LinuxComputerState,
    pub state_detail: String,
    pub can_dismiss_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_failure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_failure_diagnostic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention: Option<ComputerAttention>,
    pub freshness: Freshness,
    #[serde(default, skip_serializing_if = "is_false")]
    pub settling: bool,
    pub repositories: Vec<serde_json::Value>,
    pub files: Vec<serde_json::Value>,
    pub ports: Vec<serde_json::Value>,
    pub logs: Vec<serde_json::Value>,
    pub github_repositories: Vec<String>,
    pub secret_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_secret_revocations: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkpoints: Vec<super::checkpoints::Checkpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_checkpoint_restore: Option<super::checkpoints::PendingRestore>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_operation: Option<super::checkpoints::Operation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unfinished_restore: Option<super::checkpoints::UnfinishedRestore>,
}

/// Mirrors `app/SiloUI/src-tauri/src/runtime.rs::ComputerAttention`
/// (lines 638-650).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComputerAttention {
    pub level: AttentionLevel,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AttentionLevel {
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ComputerKind {
    Linux,
    Macos,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LinuxComputerState {
    Running,
    Starting,
    Stopped,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Freshness {
    Fresh,
    Stale,
}

wire_struct! {
    #[doc = "Mirrors `app/SiloUI/src-tauri/src/runtime.rs::DeviceCapacity`."]
    DeviceCapacity {
        logical_cpus: usize,
        physical_memory_bytes: u64,
        max_memory_gib: u64,
    }
}

/// Mirrors `app/SiloUI/src-tauri/src/macos_computers.rs::MacosComputer`
/// (lines 68-92). `kind` is added by the unified `computers.list` contract;
/// source `Option` fields remain explicit JSON nulls.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MacosComputer {
    pub kind: ComputerKind,
    pub id: String,
    pub name: String,
    pub cpus: u64,
    #[serde(rename = "memoryGiB")]
    pub memory_gib: u64,
    #[serde(rename = "diskGiB")]
    pub disk_gib: u64,
    #[serde(default)]
    pub os_version: Option<String>,
    pub state: MacosComputerState,
    #[serde(default)]
    pub progress: Option<f64>,
    #[serde(default)]
    pub detail: Option<String>,
    pub display_open: bool,
    pub installed: bool,
    pub setup_complete: bool,
    pub needs_personalizing: bool,
    pub checkpoints: Vec<super::checkpoints::Checkpoint>,
    pub checkpoint_operation: Option<super::checkpoints::Operation>,
    pub pending_restore: Option<super::checkpoints::PendingRestore>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
/// Mirrors `app/SiloUI/src-tauri/src/macos_computers/store.rs::State`.
pub enum MacosComputerState {
    Preparing,
    Copying,
    Downloading,
    Installing,
    SettingUp,
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

wire_struct! {
    #[doc = "Mirrors `app/SiloUI/src-tauri/src/macos_computers.rs::TemplateSummary`."]
    TemplateSummary {
        macos_version: String,
        build: String,
        current: bool,
    }
}

wire_struct! {
    #[doc = "Mirrors `app/SiloUI/src-tauri/src/macos_computers.rs::MacosComputersState`."]
    MacosComputersState {
        supported: bool,
        #[serde(default)]
        unsupported_reason: Option<String>,
        computers: Vec<MacosComputer>,
        #[serde(default)]
        template: Option<TemplateSummary>,
        #[serde(rename = "minDiskGiB")]
        min_disk_gib: u64,
    }
}

/// Mirrors `app/SiloUI/src-tauri/src/runtime.rs::ComputerConfigurationProgress`
/// (lines 3343 and 3462-3494), including its explicit `type` field.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComputerConfigurationProgress {
    pub schema_version: u8,
    #[serde(rename = "type")]
    pub event_type: String,
    pub request_id: String,
    pub phase: String,
    pub step: String,
    pub computer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fraction: Option<u8>,
    pub message: String,
    pub safe_for_display: bool,
    pub timestamp: u64,
    pub level: String,
    pub elapsed_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloaded_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub partial: bool,
}
