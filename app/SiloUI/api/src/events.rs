//! Canonical pushed event envelope and the full §4.2 D event catalogue.

use serde::{Deserialize, Serialize};

use crate::{
    jobs::{JobFinished as FinishedJob, JobProgress as ProgressJob},
    methods::{
        computers::{ApplicationSource, MacosComputersState},
        connections::DeviceHealthResult,
        display::ViewerPageEvent,
        exports::BackupState,
        github::GitHubState,
        guest::{GuestExecStream, GuestFilesChangedPayload},
        migration::{MigrationState, PreUpgradeBackup},
        network::NetworkState,
        notifications::{Notice, NotificationsClearedPayload},
        operations::OperationQueue,
        preparation::{ChatGptPreparationStatus, PreparationStatus},
        push::PushOperation,
        secrets::Secret,
        settings::SettingsSnapshot,
        updates::UpdateSnapshot,
    },
};

/// Event payloads stay domain typed. The common envelope fields use the exact
/// `event`, `subscription`, `deviceId`, `seq`, and `payload` wire names.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "event")]
pub enum EventEnvelope {
    #[serde(rename = "computers.changed")]
    ComputersChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: ComputersChangedPayload,
    },
    #[serde(rename = "checkpoints.changed")]
    CheckpointsChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: serde_json::Value,
    },
    #[serde(rename = "exports.changed")]
    ExportsChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: BackupState,
    },
    #[serde(rename = "migration.changed")]
    MigrationChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: MigrationState,
    },
    #[serde(rename = "migration.preUpgradeBackup.changed")]
    MigrationPreUpgradeBackupChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: Option<PreUpgradeBackup>,
    },
    #[serde(rename = "github.changed")]
    GitHubChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: GitHubState,
    },
    #[serde(rename = "secrets.changed")]
    SecretsChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: Vec<Secret>,
    },
    #[serde(rename = "push.changed")]
    PushChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: Vec<PushOperation>,
    },
    #[serde(rename = "operations.queue.changed")]
    OperationsQueueChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: OperationQueue,
    },
    #[serde(rename = "network.changed")]
    NetworkChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: NetworkState,
    },
    #[serde(rename = "editor.includeNotice.changed")]
    EditorIncludeNoticeChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: Option<String>,
    },
    #[serde(rename = "operations.job.progress")]
    JobProgress {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: ProgressJob,
    },
    #[serde(rename = "operations.job.finished")]
    JobFinished {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: FinishedJob,
    },
    #[serde(rename = "engine.shutdown.changed")]
    EngineShutdownChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: ShutdownChangedPayload,
    },
    #[serde(rename = "settings.changed")]
    SettingsChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: SettingsSnapshot,
    },
    #[serde(rename = "updates.changed")]
    UpdatesChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: UpdateSnapshot,
    },
    #[serde(rename = "preparation.changed")]
    PreparationChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: PreparationStatus,
    },
    #[serde(rename = "preparation.chatgpt.changed")]
    ChatGptPreparationChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: ChatGptPreparationStatus,
    },
    #[serde(rename = "notifications.notice")]
    NotificationNotice {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: Notice,
    },
    #[serde(rename = "connections.devices.changed")]
    ConnectionsDevicesChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: DeviceHealthResult,
    },
    #[serde(rename = "notifications.cleared")]
    NotificationsCleared {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: NotificationsClearedPayload,
    },
    #[serde(rename = "computers.unexpectedChange")]
    ComputerUnexpectedChange {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: UnexpectedComputerChange,
    },
    #[serde(rename = "display.viewer.page")]
    DisplayViewerPage {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: ViewerPageEvent,
    },
    #[serde(rename = "guest.exec.output")]
    GuestExecOutput {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: GuestExecOutputEvent,
    },
    #[serde(rename = "guest.exec.exited")]
    GuestExecExited {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: GuestExecExitedEvent,
    },
    #[serde(rename = "guest.files.changed")]
    GuestFilesChanged {
        subscription: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        seq: u64,
        payload: GuestFilesChangedPayload,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComputersChangedPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux: Option<ApplicationSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos: Option<MacosComputersState>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownChangedPayload {
    pub shutting_down: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnexpectedComputerChange {
    pub computer_id: String,
    pub name: String,
    pub state: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GuestExecOutputEvent {
    pub job_id: crate::jobs::JobId,
    pub seq: u64,
    pub stream: GuestExecStream,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GuestExecExitedEvent {
    pub job_id: crate::jobs::JobId,
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
}

pub const EVENT_NAMES: &[&str] = &[
    "computers.changed",
    "checkpoints.changed",
    "exports.changed",
    "migration.changed",
    "migration.preUpgradeBackup.changed",
    "github.changed",
    "secrets.changed",
    "push.changed",
    "operations.queue.changed",
    "network.changed",
    "editor.includeNotice.changed",
    "operations.job.progress",
    "operations.job.finished",
    "engine.shutdown.changed",
    "settings.changed",
    "updates.changed",
    "preparation.changed",
    "preparation.chatgpt.changed",
    "notifications.notice",
    "connections.devices.changed",
    "notifications.cleared",
    "computers.unexpectedChange",
    "display.viewer.page",
    "guest.exec.output",
    "guest.exec.exited",
    "guest.files.changed",
];

pub const EVENT_COUNT: usize = EVENT_NAMES.len();
