//! The rule each method follows when the agent inside a computer calls it. The match has no
//! wildcard arm, so a method added to the registry does not compile until it is classified
//! here. `docs/SiloUI-POLICY.md` is the same table in plain words.

use silo_api::{methods::MethodName, transport::ApprovalKind};

/// What a computer's own agent may do with a method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// Always allowed: it shows nothing about other computers and changes nothing.
    Open,
    /// Allowed for a job the caller started.
    OwnJob,
    /// The target is the caller or a computer it made; lists are cut to that set.
    Tree,
    /// The target is a computer the caller made, never the caller itself.
    Made,
    /// Forks the caller or a computer it made, or makes a new computer. Access handed over
    /// must be the caller's own, else the call asks the owner.
    Create,
    /// Widens what a computer has, so it asks the owner.
    Ask(ApprovalKind),
    /// Never for a computer.
    OwnerOnly,
}

/// The rule of `method`.
pub fn rule(method: MethodName) -> Rule {
    use MethodName as M;
    use Rule::*;
    match method {
        // The engine itself.
        M::EngineHandshake | M::EngineState | M::EngineDisconnect | M::EventsUnsubscribe => Open,
        M::DeviceFacts | M::DeviceResources => Open,
        // Events and notices cover every computer and are not cut per caller.
        M::EventsSubscribe | M::NotificationsList => OwnerOnly,
        M::EngineShutdown
        | M::EngineUpdatePrepare
        | M::EngineUpdateAbort
        | M::EngineCredentialsRepair
        | M::EngineCredentialsStatus
        | M::DeviceDependencies => OwnerOnly,
        M::AuditList => Open,
        // Approvals: a computer reads and withdraws its own, and those of computers it made
        // (the handler cuts them); only the owner answers.
        M::ApprovalsList | M::ApprovalsGet | M::ApprovalsCancel | M::ApprovalsNote => Open,
        M::ApprovalsDecide => OwnerOnly,

        // Jobs.
        M::OperationsJobGet | M::OperationsJobAcknowledge | M::OperationsCancel => OwnJob,
        M::GuestExecGet | M::GuestExecInput => OwnJob,
        M::OperationsList => OwnerOnly,

        // Computers.
        M::ComputersList
        | M::ComputersTree
        | M::ComputersCredentials
        | M::ComputersAccess
        | M::ComputersRecipe => Tree,
        M::ComputersCreate => Create,
        M::ComputersStart
        | M::ComputersStop
        | M::ComputersRestart
        | M::ComputersForceStop
        | M::ComputersDelete
        | M::ComputersDismissError => Made,
        M::ComputersConfigurationChange
        | M::ComputersConfigurationRead
        | M::ComputersConfigurationRetry
        | M::ComputersIdentitiesConfigure
        | M::ComputersIdentitiesVerify
        | M::ComputersLaunch
        | M::ComputersRebuild
        | M::ComputersShell
        | M::ActivitySetupRead => OwnerOnly,
        M::MacosSetupRetry => Made,
        M::MacosTemplateDelete => OwnerOnly,

        // Checkpoints and steps.
        M::CheckpointsCreate | M::CheckpointsUsage | M::StepsList => Tree,
        M::CheckpointsFork => Create,
        M::CheckpointsRestore
        | M::CheckpointsDelete
        | M::CheckpointsAbandonRestore
        | M::StepsRecord => Made,

        // Changes, merge and review requests.
        M::ChangesDiff | M::ChangesRequestsList | M::ChangesRequestsShow => Tree,
        M::ChangesRequest | M::ChangesRequestsThrowAway => Made,
        M::ChangesKeep | M::ChangesRequestsKeep | M::ChangesUndo => Ask(ApprovalKind::Merge),
        M::ChangesPromote => Ask(ApprovalKind::ReplaceMain),
        // Baselines are cached under keys other computers share.
        M::ChangesBaseline | M::ChangesCapture => OwnerOnly,
        M::ChangesRequestsMarkKept | M::ChangesRequestsOpen => OwnerOnly,

        // Inside a computer: commands, files, the desktop and computer use.
        M::GuestExec | M::GuestExecStart | M::GuestFilesWrite | M::GuestFilesRemove => Made,
        M::GuestSsh => Made,
        M::GuestAccount | M::GuestFilesList | M::GuestFilesRead | M::GuestFilesWatch => Tree,
        M::FilesList | M::LogsQuery => Tree,
        M::DesktopAction | M::ComputerUseSetup | M::ClipboardWrite => Made,
        M::DesktopStatus | M::ComputerUseState | M::ClipboardRead => Tree,
        // These reach the device's own files, clipboard, screen or applications.
        M::FilesUpload
        | M::FilesDownload
        | M::LogsExport
        | M::ClipboardTransfer
        | M::ComputerUseApproval
        | M::DesktopConnect
        | M::DisplayShow
        | M::DisplayClose
        | M::DisplayNativeAttach
        | M::DisplayViewerOpen
        | M::DisplayViewerClose
        | M::DisplayViewerAudioSet
        | M::DisplayViewerScreenReset
        | M::DisplayViewerSoundCancel
        | M::DisplayViewerSoundCheck
        | M::MacosDisplayConnect
        | M::MacosDisplayResize
        | M::MacosDisplaySession
        | M::MacosDisplayStream
        | M::EditorCommand
        | M::EditorIncludeNotice
        | M::TerminalCommand
        | M::GuestSshAliasEnsure
        | M::GuestSshAliasRemove => OwnerOnly,
        // Prepares the device-wide authorized_keys file every computer shares.
        M::GuestPrepare => OwnerOnly,
        // A computer reads its own tools status; turning them on or off is the owner's.
        M::GuestClientStatus => Tree,
        M::GuestClientEnable | M::GuestClientRevoke => OwnerOnly,
        // The agents a computer has: reading is like its other status, installing runs in it.
        M::AgentsList => Tree,
        M::AgentsInstall => Made,

        // Network and SSH access.
        M::NetworkUrls | M::NetworkUnpublish => Tree,
        M::NetworkPublish => Ask(ApprovalKind::Port),
        M::NetworkPolicyGet => Tree,
        // Narrowing is free; a wider policy asks (see `Judge::network_policy`).
        M::NetworkPolicySet => Ask(ApprovalKind::Network),
        M::NetworkOpen | M::NetworkState => OwnerOnly,
        M::SshAccessSave => Ask(ApprovalKind::Ssh),
        M::SshAccessState | M::SshAccessConnection => OwnerOnly,

        // GitHub, pushes and secrets.
        M::PushRepositories | M::PushStatus => Tree,
        M::PushDismiss => Made,
        M::PushStart => Ask(ApprovalKind::Github),
        M::GitHubConfigurationSave => Ask(ApprovalKind::Github),
        M::GitHubState
        | M::GitHubAccessSet
        | M::GitHubConfigurationRetry
        | M::GitHubConnect
        | M::GitHubConnectCallback
        | M::GitHubConnectReopen
        | M::GitHubDisconnect
        | M::GitHubPersonalTokenRemove
        | M::GitHubPersonalTokenSave
        | M::GitHubRepositoriesManage
        | M::GitHubRepositoriesRefresh => OwnerOnly,
        M::SecretsSave => Ask(ApprovalKind::Secret),
        M::SecretsList | M::SecretsRemove | M::SecretsRetry => OwnerOnly,

        // The device: settings, Connections, exports, storage, updates and setup.
        M::SettingsRead | M::SettingsUpdate | M::SettingsFlush => OwnerOnly,
        M::ConnectionsDevicesConnect
        | M::ConnectionsDevicesHealth
        | M::ConnectionsDevicesList
        | M::ConnectionsDevicesRemove
        | M::ConnectionsKeysAuthorize
        | M::ConnectionsKeysList
        | M::ConnectionsKeysOwn
        | M::ConnectionsKeysRevoke
        | M::ConnectionsSetEnabled
        | M::ConnectionsSshAuthorizationCommand
        | M::ConnectionsSshKeySetupCommand
        | M::ConnectionsStatus => OwnerOnly,
        M::ExportsAcknowledge
        | M::ExportsCheck
        | M::ExportsCreate
        | M::ExportsDestinationSet
        | M::ExportsDiscard
        | M::ExportsDismiss
        | M::ExportsImport
        | M::ExportsInspect
        | M::ExportsState
        | M::ExportsTransfer => OwnerOnly,
        M::StorageRead | M::StorageReclaim => OwnerOnly,
        M::UpdatesCheck
        | M::UpdatesDownload
        | M::UpdatesFocused
        | M::UpdatesSetAutomaticChecks
        | M::UpdatesState => OwnerOnly,
        M::MigrationPreUpgradeBackupAcknowledgeNotice
        | M::MigrationPreUpgradeBackupDelete
        | M::MigrationPreUpgradeBackupGet
        | M::MigrationPreUpgradeBackupMeasure
        | M::MigrationState => OwnerOnly,
        M::PreparationRetry | M::PreparationStatus => OwnerOnly,
        // Recipes: a computer may read them; saving, removing and building them is the owner's.
        M::RecipesGet | M::RecipesList => Open,
        M::RecipesBuild | M::RecipesRemove | M::RecipesSave => OwnerOnly,
    }
}

/// The command that runs `method`, for a refusal a person or an agent reads: it names what was
/// attempted in the command's words and never the engine's wire name. `None` when no command
/// is worth naming.
pub fn command_words(method: MethodName) -> Option<&'static str> {
    use MethodName as M;
    match method {
        M::GuestClientEnable => Some("silo tools enable"),
        M::GuestClientRevoke => Some("silo tools disable"),
        M::GuestClientStatus => Some("silo tools status"),
        M::SecretsList | M::SecretsRemove | M::SecretsRetry => Some("silo secrets"),
        M::ExportsCreate | M::ExportsTransfer | M::ExportsCheck | M::ExportsDiscard => {
            Some("silo export")
        }
        M::ExportsImport | M::ExportsInspect => Some("silo import"),
        M::RecipesBuild | M::RecipesSave | M::RecipesRemove => Some("silo recipe"),
        M::ComputersRebuild => Some("silo rebuild"),
        M::ComputersConfigurationChange | M::ComputersConfigurationRead => {
            Some("changing a computer's settings")
        }
        M::ConnectionsDevicesConnect
        | M::ConnectionsDevicesHealth
        | M::ConnectionsDevicesList
        | M::ConnectionsDevicesRemove => Some("silo devices"),
        M::SettingsRead | M::SettingsUpdate | M::SettingsFlush => Some("changing Silo's settings"),
        M::ComputersShell | M::ComputersLaunch | M::TerminalCommand => {
            Some("opening a terminal or an app")
        }
        _ => None,
    }
}
