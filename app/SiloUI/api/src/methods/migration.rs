//! Launch migration and pre-upgrade backup queries and commands.

use super::common::DeviceParams;

pub type MigrationStateParams = DeviceParams;
pub type PreUpgradeBackupGetParams = DeviceParams;
pub type PreUpgradeBackupMeasureParams = DeviceParams;
pub type PreUpgradeBackupDeleteParams = DeviceParams;
pub type PreUpgradeBackupAcknowledgeParams = DeviceParams;

wire_struct! {
    MigrationState {
        version: u32,
        status: String,
        stage: Option<String>,
        logs: Vec<String>,
        migrated_count: u64,
        failed_count: u64,
        total_count: u64,
        can_continue: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        log_path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    }
}

wire_struct! {
    PreUpgradeBackup {
        path: String,
        delete_at: Option<String>,
        notice_pending: bool,
    }
}

/// The result is nullable: no backup is represented by JSON `null`.
pub type PreUpgradeBackupResult = Option<PreUpgradeBackup>;
pub type PreUpgradeBackupMeasureResult = Option<u64>;
pub type MigrationStateResult = MigrationState;
pub type MigrationCommandResult = ();
