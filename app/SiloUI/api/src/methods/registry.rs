//! Typed registry for every canonical method in 80 §3.

use serde::{Deserialize, Serialize};

use super::*;
use crate::{jobs, transport};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MethodKind {
    Query,
    Command,
    Job,
    Stream,
    Subscription,
}

macro_rules! define_registry {
    ($( $variant:ident => ($wire:literal, $kind:ident, $params:ty, $result:ty); )*) => {
        #[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize, schemars::JsonSchema)]
        pub enum MethodName { $( #[serde(rename = $wire)] $variant, )* }

        #[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
        #[serde(tag = "method", content = "params")]
        pub enum MethodParams { $( #[serde(rename = $wire)] $variant($params), )* }

        #[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
        #[serde(tag = "method", content = "result")]
        pub enum MethodResults { $( #[serde(rename = $wire)] $variant($result), )* }

        pub static METHOD_REGISTRY: &[MethodDescriptor] = &[
            $( MethodDescriptor { name: $wire, method: MethodName::$variant, kind: MethodKind::$kind,
                params_type: stringify!($params), result_type: stringify!($result) }, )*
        ];
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodDescriptor {
    pub name: &'static str,
    pub method: MethodName,
    pub kind: MethodKind,
    pub params_type: &'static str,
    pub result_type: &'static str,
}

define_registry! {
    ActivitySetupRead => ("activity.setup.read", Query, common::EmptyParams, Vec<computers::ComputerConfigurationProgress>);
    CheckpointsAbandonRestore => ("checkpoints.abandonRestore", Job, common::ComputerTargetParams, common::JobStartResult);
    CheckpointsCreate => ("checkpoints.create", Job, checkpoints::CheckpointCreateParams, common::JobStartResult);
    CheckpointsDelete => ("checkpoints.delete", Job, checkpoints::CheckpointTargetParams, common::JobStartResult);
    CheckpointsFork => ("checkpoints.fork", Job, checkpoints::CheckpointForkParams, common::JobStartResult);
    CheckpointsRestore => ("checkpoints.restore", Job, checkpoints::CheckpointTargetParams, common::JobStartResult);
    CheckpointsUsage => ("checkpoints.usage", Query, checkpoints::CheckpointUsageParams, checkpoints::CheckpointUsage);
    ClipboardRead => ("clipboard.read", Command, clipboard::ClipboardReadParams, clipboard::ClipboardReadResult);
    ClipboardTransfer => ("clipboard.transfer", Command, clipboard::ClipboardTransferParams, clipboard::ClipboardTransferResult);
    ClipboardWrite => ("clipboard.write", Command, clipboard::ClipboardWriteParams, clipboard::ClipboardWriteResult);
    ComputerUseApproval => ("computerUse.approval", Command, computer_use::ComputerUseApprovalParams, computer_use::ComputerUseState);
    ComputerUseSetup => ("computerUse.setup", Job, computer_use::ComputerUseSetupParams, common::JobStartResult);
    ComputerUseState => ("computerUse.state", Query, computer_use::ComputerUseStateParams, computer_use::ComputerUseState);
    ComputersConfigurationChange => ("computers.configuration.change", Job, computers::ComputerConfigurationChangeParams, common::JobStartResult);
    ComputersConfigurationRead => ("computers.configuration.read", Query, computers::ComputerConfigurationReadParams, computers::ComputerConfigurationRequest);
    ComputersConfigurationRetry => ("computers.configuration.retry", Job, computers::ComputerConfigurationRetryParams, common::JobStartResult);
    ComputersCreate => ("computers.create", Job, computers::ComputerCreateParams, computers::ComputerCreateResult);
    ComputersCreationSkipComputerUseWait => ("computers.creation.skipComputerUseWait", Command, computers::ComputerCreationSkipWaitParams, ());
    ComputersDelete => ("computers.delete", Job, computers::ComputerDeleteParams, common::JobStartResult);
    ComputersDismissError => ("computers.dismissError", Command, computers::ComputerDismissErrorParams, computers::ApplicationSource);
    ComputersForceStop => ("computers.forceStop", Command, computers::ComputerForceStopParams, ());
    ComputersIdentitiesConfigure => ("computers.identities.configure", Job, computers::ComputerIdentitiesParams, common::JobStartResult);
    ComputersIdentitiesVerify => ("computers.identities.verify", Query, computers::ComputerIdentitiesVerifyParams, computers::ComputerIdentitiesVerifyResult);
    ComputersLaunch => ("computers.launch", Query, computers::ComputerLaunchParams, computers::LaunchStatus);
    ComputersList => ("computers.list", Query, computers::ComputerListParams, computers::ComputersListResult);
    ComputersRestart => ("computers.restart", Job, common::ComputerTargetParams, common::JobStartResult);
    ComputersShell => ("computers.shell", Query, computers::ComputerShellParams, computers::ApplicationSource);
    ComputersStart => ("computers.start", Job, common::ComputerTargetParams, common::JobStartResult);
    ComputersStop => ("computers.stop", Job, common::ComputerTargetParams, common::JobStartResult);
    ConnectionsDevicesConnect => ("connections.devices.connect", Command, connections::ConnectDeviceParams, connections::Device);
    ConnectionsDevicesHealth => ("connections.devices.health", Query, connections::DeviceHealthParams, connections::DeviceHealthResult);
    ConnectionsDevicesList => ("connections.devices.list", Query, common::EmptyParams, Vec<connections::Device>);
    ConnectionsDevicesRemove => ("connections.devices.remove", Command, connections::DeviceIdParams, ());
    ConnectionsKeysRevoke => ("connections.keys.revoke", Command, connections::RevokeKeyParams, ());
    ConnectionsSetEnabled => ("connections.setEnabled", Command, connections::ConnectionsSetEnabledParams, connections::ConnectionsStatus);
    ConnectionsSshAuthorizationCommand => ("connections.ssh.authorizationCommand", Query, connections::SshAddressParams, connections::SshCommandResult);
    ConnectionsSshKeySetupCommand => ("connections.ssh.keySetupCommand", Command, connections::SshAddressParams, connections::SshCommandResult);
    ConnectionsStatus => ("connections.status", Query, common::EmptyParams, connections::ConnectionsStatus);
    DesktopAction => ("desktop.action", Job, desktop::DesktopActionParams, common::JobStartResult);
    DesktopConnect => ("desktop.connect", Query, desktop::DesktopConnectParams, desktop::DesktopConnectResult);
    DesktopStatus => ("desktop.status", Query, desktop::DesktopStatusParams, desktop::DesktopStatus);
    DeviceDependencies => ("device.dependencies", Query, device::DependencyReportParams, device::DependencyReport);
    DeviceFacts => ("device.facts", Query, device::DeviceFactsParams, device::DeviceFacts);
    DisplayClose => ("display.close", Command, display::DisplayCloseParams, ());
    DisplayNativeAttach => ("display.native.attach", Stream, display::NativeDisplayAttachParams, display::NativeDisplayMessage);
    DisplayShow => ("display.show", Command, display::DisplayShowParams, ());
    DisplayViewerAudioSet => ("display.viewer.audio.set", Command, display::ViewerAudioSetParams, ());
    DisplayViewerClose => ("display.viewer.close", Command, display::ViewerConnectionParams, ());
    DisplayViewerOpen => ("display.viewer.open", Command, common::ComputerTargetParams, display::ViewerOpenResult);
    DisplayViewerScreenReset => ("display.viewer.screen.reset", Command, display::ViewerConnectionParams, ());
    DisplayViewerSoundCancel => ("display.viewer.sound.cancel", Command, display::ViewerConnectionParams, ());
    DisplayViewerSoundCheck => ("display.viewer.sound.check", Query, display::ViewerConnectionParams, display::ViewerSoundCheckResult);
    EditorCommand => ("editor.command", Command, terminal::EditorCommandParams, terminal::CommandResult);
    EditorIncludeNotice => ("editor.includeNotice", Query, common::EmptyParams, Option<String>);
    EngineDisconnect => ("engine.disconnect", Command, engine::EngineDisconnectParams, ());
    EngineHandshake => ("engine.handshake", Query, transport::HandshakeParams, transport::HandshakeResult);
    EngineShutdown => ("engine.shutdown", Job, engine::EngineShutdownParams, common::JobStartResult);
    EngineState => ("engine.state", Query, engine::EngineStateParams, engine::EngineState);
    EngineUpdateAbort => ("engine.update.abort", Command, engine::EngineUpdateAbortParams, ());
    EngineUpdatePrepare => ("engine.update.prepare", Job, engine::EngineUpdatePrepareParams, common::JobStartResult);
    EventsSubscribe => ("events.subscribe", Subscription, engine::EventsSubscribeParams, engine::EventsSubscribeResult);
    EventsUnsubscribe => ("events.unsubscribe", Command, engine::EventsUnsubscribeParams, ());
    ExportsAcknowledge => ("exports.acknowledge", Command, exports::ExportAcknowledgeParams, bool);
    ExportsCreate => ("exports.create", Job, exports::ExportCreateParams, common::JobStartResult);
    ExportsDestinationSet => ("exports.destination.set", Command, exports::ExportDestinationSetParams, exports::ExportDestinationResult);
    ExportsDismiss => ("exports.dismiss", Command, exports::ExportDismissParams, bool);
    ExportsImport => ("exports.import", Job, exports::ExportImportParams, common::JobStartResult);
    ExportsInspect => ("exports.inspect", Job, exports::ExportInspectParams, common::JobStartResult);
    ExportsState => ("exports.state", Query, exports::ExportStateParams, exports::BackupState);
    FilesDownload => ("files.download", Job, files::FileDownloadParams, common::JobStartResult);
    FilesList => ("files.list", Query, files::FileListParams, files::FileListResult);
    FilesUpload => ("files.upload", Job, files::FileUploadParams, common::JobStartResult);
    GitHubAccessSet => ("github.access.set", Command, github::GitHubAccessSetParams, github::GitHubState);
    GitHubConfigurationRetry => ("github.configuration.retry", Command, github::GitHubConfigurationRetryParams, github::GitHubState);
    GitHubConfigurationSave => ("github.configuration.save", Command, github::GitHubConfigurationSaveParams, github::GitHubState);
    GitHubConnect => ("github.connect", Job, github::GitHubConnectParams, common::JobStartResult);
    GitHubConnectCallback => ("github.connect.callback", Stream, github::GitHubConnectCallbackParams, ());
    GitHubConnectReopen => ("github.connect.reopen", Command, github::GitHubConnectReopenParams, github::GitHubRepositoriesManageResult);
    GitHubDisconnect => ("github.disconnect", Command, github::GitHubDisconnectParams, github::GitHubState);
    GitHubPersonalTokenRemove => ("github.personalToken.remove", Command, github::GitHubPersonalTokenRemoveParams, github::GitHubState);
    GitHubPersonalTokenSave => ("github.personalToken.save", Command, github::GitHubPersonalTokenSaveParams, github::GitHubState);
    GitHubRepositoriesManage => ("github.repositories.manage", Command, github::GitHubRepositoriesManageParams, github::GitHubRepositoriesManageResult);
    GitHubRepositoriesRefresh => ("github.repositories.refresh", Command, github::GitHubRepositoriesRefreshParams, github::GitHubState);
    GitHubState => ("github.state", Query, github::GitHubStateParams, github::GitHubState);
    GuestAccount => ("guest.account", Query, guest::GuestAccountParams, guest::GuestAccount);
    GuestExec => ("guest.exec", Command, guest::GuestExecParams, guest::GuestExecResult);
    GuestExecGet => ("guest.exec.get", Query, guest::GuestExecGetParams, guest::GuestExecGetResult);
    GuestExecInput => ("guest.exec.input", Command, guest::GuestExecInputParams, ());
    GuestExecStart => ("guest.exec.start", Job, guest::GuestExecStartParams, common::JobStartResult);
    GuestFilesList => ("guest.files.list", Query, guest::GuestFilesListParams, guest::GuestFilesListResult);
    GuestFilesRead => ("guest.files.read", Query, guest::GuestFilesReadParams, guest::GuestFilesReadResult);
    GuestFilesRemove => ("guest.files.remove", Command, guest::GuestFilesRemoveParams, guest::GuestFilesRemoveResult);
    GuestFilesWatch => ("guest.files.watch", Subscription, guest::GuestFilesWatchParams, guest::GuestFilesWatchResult);
    GuestFilesWrite => ("guest.files.write", Command, guest::GuestFilesWriteParams, guest::GuestFilesWriteResult);
    GuestPrepare => ("guest.prepare", Command, guest::GuestPrepareParams, guest::GuestPrepareResult);
    GuestSsh => ("guest.ssh", Stream, guest::GuestSshParams, ());
    GuestSshAliasEnsure => ("guest.sshAlias.ensure", Command, guest::GuestSshAliasEnsureParams, guest::GuestSshAliasEnsureResult);
    GuestSshAliasRemove => ("guest.sshAlias.remove", Command, guest::GuestSshAliasRemoveParams, ());
    LogsExport => ("logs.export", Job, logs::LogsExportParams, common::JobStartResult);
    LogsQuery => ("logs.query", Query, logs::LogsQueryParams, logs::LogPage);
    MacosDisplayConnect => ("macos.display.connect", Query, macos::MacosDisplayConnectParams, macos::MacosDisplayConnectResult);
    MacosDisplayResize => ("macos.display.resize", Command, macos::MacosDisplayResizeParams, macos::MacosDisplayResizeResult);
    MacosDisplaySession => ("macos.display.session", Command, macos::MacosDisplaySessionParams, macos::MacosDisplaySessionResult);
    MacosDisplayStream => ("macos.display.stream", Stream, macos::MacosDisplayStreamParams, ());
    MacosSetupRetry => ("macos.setup.retry", Job, macos::MacosSetupRetryParams, common::JobStartResult);
    MacosTemplateDelete => ("macos.template.delete", Command, macos::MacosTemplateDeleteParams, ());
    MigrationPreUpgradeBackupAcknowledgeNotice => ("migration.preUpgradeBackup.acknowledgeNotice", Command, common::DeviceParams, ());
    MigrationPreUpgradeBackupDelete => ("migration.preUpgradeBackup.delete", Command, common::DeviceParams, ());
    MigrationPreUpgradeBackupGet => ("migration.preUpgradeBackup.get", Query, common::DeviceParams, migration::PreUpgradeBackupResult);
    MigrationPreUpgradeBackupMeasure => ("migration.preUpgradeBackup.measure", Query, common::DeviceParams, migration::PreUpgradeBackupMeasureResult);
    MigrationState => ("migration.state", Query, common::DeviceParams, migration::MigrationState);
    NetworkOpen => ("network.open", Command, network::NetworkOpenParams, network::NetworkOpenResult);
    NetworkPublish => ("network.publish", Command, network::NetworkPublishParams, network::NetworkState);
    NetworkState => ("network.state", Query, network::NetworkStateParams, network::NetworkState);
    NetworkUnpublish => ("network.unpublish", Command, network::NetworkUnpublishParams, network::NetworkState);
    NotificationsList => ("notifications.list", Query, notifications::NotificationsListParams, notifications::NotificationsListResult);
    OperationsCancel => ("operations.cancel", Command, operations::CancelJobParams, ());
    OperationsJobAcknowledge => ("operations.job.acknowledge", Command, operations::AcknowledgeJobParams, ());
    OperationsJobGet => ("operations.job.get", Query, operations::GetJobParams, jobs::JobLookupResult);
    OperationsList => ("operations.list", Query, operations::OperationsListParams, operations::OperationQueue);
    PreparationChatgptRetry => ("preparation.chatgpt.retry", Command, preparation::ChatGptPreparationRetryParams, preparation::ChatGptPreparationStatus);
    PreparationChatgptStatus => ("preparation.chatgpt.status", Query, preparation::ChatGptPreparationStatusParams, preparation::ChatGptPreparationStatus);
    PreparationRetry => ("preparation.retry", Command, preparation::PreparationRetryParams, preparation::PreparationStatus);
    PreparationStatus => ("preparation.status", Query, preparation::PreparationStatusParams, preparation::PreparationStatus);
    PushDismiss => ("push.dismiss", Command, push::PushDismissParams, ());
    PushRepositories => ("push.repositories", Query, push::PushRepositoriesParams, Vec<push::PushRepository>);
    PushStart => ("push.start", Job, push::PushStartParams, common::JobStartResult);
    PushStatus => ("push.status", Query, push::PushStatusParams, Option<push::PushOperation>);
    SecretsList => ("secrets.list", Query, common::DeviceParams, Vec<secrets::Secret>);
    SecretsRemove => ("secrets.remove", Command, secrets::SecretsRemoveParams, Vec<secrets::Secret>);
    SecretsRetry => ("secrets.retry", Command, secrets::SecretsRetryParams, Vec<secrets::Secret>);
    SecretsSave => ("secrets.save", Command, secrets::SecretsSaveParams, Vec<secrets::Secret>);
    SettingsFlush => ("settings.flush", Command, settings::SettingsFlushParams, ());
    SettingsRead => ("settings.read", Query, settings::SettingsReadParams, settings::SettingsSnapshot);
    SettingsUpdate => ("settings.update", Command, settings::SettingsUpdateParams, settings::SettingsSnapshot);
    SshAccessConnection => ("sshAccess.connection", Command, ssh_access::SshAccessConnectionParams, ssh_access::SshAccessConnectionResult);
    SshAccessSave => ("sshAccess.save", Command, ssh_access::SshAccessSaveParams, ssh_access::SshAccessState);
    SshAccessState => ("sshAccess.state", Query, common::DeviceParams, ssh_access::SshAccessState);
    StorageRead => ("storage.read", Query, storage::StorageParams, storage::StorageState);
    StorageReclaim => ("storage.reclaim", Job, storage::StorageParams, common::JobStartResult);
    TerminalCommand => ("terminal.command", Query, terminal::TerminalCommandParams, terminal::CommandResult);
    UpdatesCheck => ("updates.check", Command, updates::UpdatesCheckParams, updates::UpdateSnapshot);
    UpdatesDownload => ("updates.download", Job, updates::UpdatesDownloadParams, common::JobStartResult);
    UpdatesFocused => ("updates.focused", Command, updates::UpdatesFocusedParams, ());
    UpdatesSetAutomaticChecks => ("updates.setAutomaticChecks", Command, updates::UpdatesSetAutomaticChecksParams, updates::UpdateSnapshot);
    UpdatesState => ("updates.state", Query, common::EmptyParams, updates::UpdateSnapshot);
}

pub fn method_registry() -> &'static [MethodDescriptor] {
    METHOD_REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_every_canonical_method_and_kind() {
        assert_eq!(METHOD_REGISTRY.len(), 146);
        assert_eq!(
            METHOD_REGISTRY
                .iter()
                .filter(|m| m.kind == MethodKind::Query)
                .count(),
            46
        );
        assert_eq!(
            METHOD_REGISTRY
                .iter()
                .filter(|m| m.kind == MethodKind::Command)
                .count(),
            65
        );
        assert_eq!(
            METHOD_REGISTRY
                .iter()
                .filter(|m| m.kind == MethodKind::Job)
                .count(),
            29
        );
        assert_eq!(
            METHOD_REGISTRY
                .iter()
                .filter(|m| m.kind == MethodKind::Stream)
                .count(),
            4
        );
        assert_eq!(
            METHOD_REGISTRY
                .iter()
                .filter(|m| m.kind == MethodKind::Subscription)
                .count(),
            2
        );
        assert!(METHOD_REGISTRY
            .iter()
            .all(|m| !m.params_type.is_empty() && !m.result_type.is_empty()));
        assert_eq!(
            METHOD_REGISTRY
                .iter()
                .filter(|m| m.name == "macos.display.stream")
                .count(),
            1
        );
    }
}
