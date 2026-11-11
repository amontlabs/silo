//! Typed request constructors, one per registry method.
//!
//! Generated from `METHOD_REGISTRY` by `requests_source`; do not edit.
//! Regenerate with `cargo run -p silo-api --example generate_requests`.

use crate::transport;
use super::agents;
use super::approvals;
use super::audit;
use super::changes;
use super::checkpoints;
use super::clipboard;
use super::common;
use super::computer_use;
use super::computers;
use super::connections;
use super::credentials;
use super::credentials_used;
use super::desktop;
use super::device;
use super::display;
use super::engine;
use super::exports;
use super::files;
use super::github;
use super::guest;
use super::guest_client;
use super::keep;
use super::logs;
use super::macos;
use super::network;
use super::notifications;
use super::operations;
use super::preparation;
use super::push;
use super::recipes;
use super::review_requests;
use super::secrets;
use super::settings;
use super::ssh_access;
use super::steps;
use super::storage;
use super::terminal;
use super::updates;
use super::request::{kind, Request};
use super::MethodParams;

/// `activity.setup.read`
pub fn activity_setup_read(params: common::DeviceParams) -> Request<kind::Query> {
    Request::new(MethodParams::ActivitySetupRead(params))
}

/// `approvals.cancel`
pub fn approvals_cancel(params: approvals::ApprovalsCancelParams) -> Request<kind::Command> {
    Request::new(MethodParams::ApprovalsCancel(params))
}

/// `approvals.decide`
pub fn approvals_decide(params: approvals::ApprovalsDecideParams) -> Request<kind::Command> {
    Request::new(MethodParams::ApprovalsDecide(params))
}

/// `approvals.get`
pub fn approvals_get(params: approvals::ApprovalsGetParams) -> Request<kind::Query> {
    Request::new(MethodParams::ApprovalsGet(params))
}

/// `approvals.list`
pub fn approvals_list(params: approvals::ApprovalsListParams) -> Request<kind::Query> {
    Request::new(MethodParams::ApprovalsList(params))
}

/// `approvals.note`
pub fn approvals_note(params: approvals::ApprovalsNoteParams) -> Request<kind::Command> {
    Request::new(MethodParams::ApprovalsNote(params))
}

/// `audit.list`
pub fn audit_list(params: audit::AuditListParams) -> Request<kind::Query> {
    Request::new(MethodParams::AuditList(params))
}

/// `agents.install`
pub fn agents_install(params: agents::AgentsInstallParams) -> Request<kind::Job> {
    Request::new(MethodParams::AgentsInstall(params))
}

/// `agents.list`
pub fn agents_list(params: agents::AgentsListParams) -> Request<kind::Query> {
    Request::new(MethodParams::AgentsList(params))
}

/// `changes.keep`
pub fn changes_keep(params: keep::ChangesKeepParams) -> Request<kind::Job> {
    Request::new(MethodParams::ChangesKeep(params))
}

/// `changes.promote`
pub fn changes_promote(params: keep::ChangesPromoteParams) -> Request<kind::Job> {
    Request::new(MethodParams::ChangesPromote(params))
}

/// `changes.request`
pub fn changes_request(params: keep::ChangesRequestParams) -> Request<kind::Command> {
    Request::new(MethodParams::ChangesRequest(params))
}

/// `changes.requests.list`
pub fn changes_requests_list(
    params: review_requests::ChangesRequestsListParams,
) -> Request<kind::Query> {
    Request::new(MethodParams::ChangesRequestsList(params))
}

/// `changes.requests.show`
pub fn changes_requests_show(params: review_requests::RequestIdParams) -> Request<kind::Job> {
    Request::new(MethodParams::ChangesRequestsShow(params))
}

/// `changes.requests.keep`
pub fn changes_requests_keep(
    params: review_requests::ChangesRequestsKeepParams,
) -> Request<kind::Job> {
    Request::new(MethodParams::ChangesRequestsKeep(params))
}

/// `changes.requests.markKept`
pub fn changes_requests_mark_kept(
    params: review_requests::ChangesRequestsMarkKeptParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::ChangesRequestsMarkKept(params))
}

/// `changes.requests.throwAway`
pub fn changes_requests_throw_away(params: review_requests::RequestIdParams) -> Request<kind::Job> {
    Request::new(MethodParams::ChangesRequestsThrowAway(params))
}

/// `changes.requests.open`
pub fn changes_requests_open(params: review_requests::RequestIdParams) -> Request<kind::Job> {
    Request::new(MethodParams::ChangesRequestsOpen(params))
}

/// `changes.undo`
pub fn changes_undo(params: keep::ChangesUndoParams) -> Request<kind::Job> {
    Request::new(MethodParams::ChangesUndo(params))
}

/// `changes.baseline`
pub fn changes_baseline(params: changes::ChangesBaselineParams) -> Request<kind::Job> {
    Request::new(MethodParams::ChangesBaseline(params))
}

/// `changes.capture`
pub fn changes_capture(params: changes::ChangesCaptureParams) -> Request<kind::Job> {
    Request::new(MethodParams::ChangesCapture(params))
}

/// `changes.diff`
pub fn changes_diff(params: changes::ChangesDiffParams) -> Request<kind::Job> {
    Request::new(MethodParams::ChangesDiff(params))
}

/// `checkpoints.abandonRestore`
pub fn checkpoints_abandon_restore(params: common::ComputerTargetParams) -> Request<kind::Job> {
    Request::new(MethodParams::CheckpointsAbandonRestore(params))
}

/// `checkpoints.create`
pub fn checkpoints_create(params: checkpoints::CheckpointCreateParams) -> Request<kind::Job> {
    Request::new(MethodParams::CheckpointsCreate(params))
}

/// `checkpoints.delete`
pub fn checkpoints_delete(params: checkpoints::CheckpointTargetParams) -> Request<kind::Job> {
    Request::new(MethodParams::CheckpointsDelete(params))
}

/// `checkpoints.fork`
pub fn checkpoints_fork(params: checkpoints::CheckpointForkParams) -> Request<kind::Job> {
    Request::new(MethodParams::CheckpointsFork(params))
}

/// `checkpoints.restore`
pub fn checkpoints_restore(params: checkpoints::CheckpointTargetParams) -> Request<kind::Job> {
    Request::new(MethodParams::CheckpointsRestore(params))
}

/// `checkpoints.usage`
pub fn checkpoints_usage(params: checkpoints::CheckpointUsageParams) -> Request<kind::Query> {
    Request::new(MethodParams::CheckpointsUsage(params))
}

/// `clipboard.read`
pub fn clipboard_read(params: clipboard::ClipboardReadParams) -> Request<kind::Command> {
    Request::new(MethodParams::ClipboardRead(params))
}

/// `clipboard.transfer`
pub fn clipboard_transfer(params: clipboard::ClipboardTransferParams) -> Request<kind::Command> {
    Request::new(MethodParams::ClipboardTransfer(params))
}

/// `clipboard.write`
pub fn clipboard_write(params: clipboard::ClipboardWriteParams) -> Request<kind::Command> {
    Request::new(MethodParams::ClipboardWrite(params))
}

/// `computerUse.approval`
pub fn computer_use_approval(
    params: computer_use::ComputerUseApprovalParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::ComputerUseApproval(params))
}

/// `computerUse.setup`
pub fn computer_use_setup(params: computer_use::ComputerUseSetupParams) -> Request<kind::Job> {
    Request::new(MethodParams::ComputerUseSetup(params))
}

/// `computerUse.state`
pub fn computer_use_state(params: computer_use::ComputerUseStateParams) -> Request<kind::Query> {
    Request::new(MethodParams::ComputerUseState(params))
}

/// `computers.configuration.change`
pub fn computers_configuration_change(
    params: computers::ComputerConfigurationChangeParams,
) -> Request<kind::Job> {
    Request::new(MethodParams::ComputersConfigurationChange(params))
}

/// `computers.configuration.read`
pub fn computers_configuration_read(
    params: computers::ComputerConfigurationReadParams,
) -> Request<kind::Query> {
    Request::new(MethodParams::ComputersConfigurationRead(params))
}

/// `computers.configuration.retry`
pub fn computers_configuration_retry(
    params: computers::ComputerConfigurationRetryParams,
) -> Request<kind::Job> {
    Request::new(MethodParams::ComputersConfigurationRetry(params))
}

/// `computers.access`
pub fn computers_access(params: approvals::ComputersAccessParams) -> Request<kind::Query> {
    Request::new(MethodParams::ComputersAccess(params))
}

/// `computers.create`
pub fn computers_create(params: computers::ComputerCreateParams) -> Request<kind::Job> {
    Request::new(MethodParams::ComputersCreate(params))
}

/// `computers.credentials`
pub fn computers_credentials(
    params: credentials_used::ComputerCredentialsParams,
) -> Request<kind::Query> {
    Request::new(MethodParams::ComputersCredentials(params))
}

/// `computers.delete`
pub fn computers_delete(params: computers::ComputerDeleteParams) -> Request<kind::Job> {
    Request::new(MethodParams::ComputersDelete(params))
}

/// `computers.dismissError`
pub fn computers_dismiss_error(
    params: computers::ComputerDismissErrorParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::ComputersDismissError(params))
}

/// `computers.forceStop`
pub fn computers_force_stop(params: computers::ComputerForceStopParams) -> Request<kind::Command> {
    Request::new(MethodParams::ComputersForceStop(params))
}

/// `computers.identities.configure`
pub fn computers_identities_configure(
    params: computers::ComputerIdentitiesParams,
) -> Request<kind::Job> {
    Request::new(MethodParams::ComputersIdentitiesConfigure(params))
}

/// `computers.identities.verify`
pub fn computers_identities_verify(
    params: computers::ComputerIdentitiesVerifyParams,
) -> Request<kind::Query> {
    Request::new(MethodParams::ComputersIdentitiesVerify(params))
}

/// `computers.launch`
pub fn computers_launch(params: computers::ComputerLaunchParams) -> Request<kind::Query> {
    Request::new(MethodParams::ComputersLaunch(params))
}

/// `computers.list`
pub fn computers_list(params: computers::ComputerListParams) -> Request<kind::Query> {
    Request::new(MethodParams::ComputersList(params))
}

/// `computers.restart`
pub fn computers_restart(params: common::ComputerTargetParams) -> Request<kind::Job> {
    Request::new(MethodParams::ComputersRestart(params))
}

/// `computers.rebuild`
pub fn computers_rebuild(params: common::ComputerTargetParams) -> Request<kind::Job> {
    Request::new(MethodParams::ComputersRebuild(params))
}

/// `computers.shell`
pub fn computers_shell(params: computers::ComputerShellParams) -> Request<kind::Query> {
    Request::new(MethodParams::ComputersShell(params))
}

/// `computers.start`
pub fn computers_start(params: common::ComputerTargetParams) -> Request<kind::Job> {
    Request::new(MethodParams::ComputersStart(params))
}

/// `computers.stop`
pub fn computers_stop(params: common::ComputerTargetParams) -> Request<kind::Job> {
    Request::new(MethodParams::ComputersStop(params))
}

/// `computers.recipe`
pub fn computers_recipe(params: common::ComputerTargetParams) -> Request<kind::Query> {
    Request::new(MethodParams::ComputersRecipe(params))
}

/// `computers.tree`
pub fn computers_tree(params: steps::ComputersTreeParams) -> Request<kind::Query> {
    Request::new(MethodParams::ComputersTree(params))
}

/// `connections.devices.connect`
pub fn connections_devices_connect(
    params: connections::ConnectDeviceParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::ConnectionsDevicesConnect(params))
}

/// `connections.devices.health`
pub fn connections_devices_health(params: connections::DeviceHealthParams) -> Request<kind::Query> {
    Request::new(MethodParams::ConnectionsDevicesHealth(params))
}

/// `connections.devices.list`
pub fn connections_devices_list(params: common::EmptyParams) -> Request<kind::Query> {
    Request::new(MethodParams::ConnectionsDevicesList(params))
}

/// `connections.devices.remove`
pub fn connections_devices_remove(params: connections::DeviceIdParams) -> Request<kind::Command> {
    Request::new(MethodParams::ConnectionsDevicesRemove(params))
}

/// `connections.keys.authorize`
pub fn connections_keys_authorize(
    params: connections::AuthorizeKeyParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::ConnectionsKeysAuthorize(params))
}

/// `connections.keys.list`
pub fn connections_keys_list(params: common::EmptyParams) -> Request<kind::Query> {
    Request::new(MethodParams::ConnectionsKeysList(params))
}

/// `connections.keys.own`
pub fn connections_keys_own(params: common::EmptyParams) -> Request<kind::Command> {
    Request::new(MethodParams::ConnectionsKeysOwn(params))
}

/// `connections.keys.revoke`
pub fn connections_keys_revoke(params: connections::RevokeKeyParams) -> Request<kind::Command> {
    Request::new(MethodParams::ConnectionsKeysRevoke(params))
}

/// `connections.setEnabled`
pub fn connections_set_enabled(
    params: connections::ConnectionsSetEnabledParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::ConnectionsSetEnabled(params))
}

/// `connections.ssh.authorizationCommand`
pub fn connections_ssh_authorization_command(
    params: connections::SshAddressParams,
) -> Request<kind::Query> {
    Request::new(MethodParams::ConnectionsSshAuthorizationCommand(params))
}

/// `connections.ssh.keySetupCommand`
pub fn connections_ssh_key_setup_command(
    params: connections::SshAddressParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::ConnectionsSshKeySetupCommand(params))
}

/// `connections.status`
pub fn connections_status(params: common::EmptyParams) -> Request<kind::Query> {
    Request::new(MethodParams::ConnectionsStatus(params))
}

/// `desktop.action`
pub fn desktop_action(params: desktop::DesktopActionParams) -> Request<kind::Job> {
    Request::new(MethodParams::DesktopAction(params))
}

/// `desktop.connect`
pub fn desktop_connect(params: desktop::DesktopConnectParams) -> Request<kind::Query> {
    Request::new(MethodParams::DesktopConnect(params))
}

/// `desktop.status`
pub fn desktop_status(params: desktop::DesktopStatusParams) -> Request<kind::Query> {
    Request::new(MethodParams::DesktopStatus(params))
}

/// `device.dependencies`
pub fn device_dependencies(params: device::DependencyReportParams) -> Request<kind::Query> {
    Request::new(MethodParams::DeviceDependencies(params))
}

/// `device.facts`
pub fn device_facts(params: device::DeviceFactsParams) -> Request<kind::Query> {
    Request::new(MethodParams::DeviceFacts(params))
}

/// `device.resources`
pub fn device_resources(params: device::DeviceResourcesParams) -> Request<kind::Query> {
    Request::new(MethodParams::DeviceResources(params))
}

/// `display.close`
pub fn display_close(params: display::DisplayCloseParams) -> Request<kind::Command> {
    Request::new(MethodParams::DisplayClose(params))
}

/// `display.native.attach`
pub fn display_native_attach(params: display::NativeDisplayAttachParams) -> Request<kind::Stream> {
    Request::new(MethodParams::DisplayNativeAttach(params))
}

/// `display.show`
pub fn display_show(params: display::DisplayShowParams) -> Request<kind::Command> {
    Request::new(MethodParams::DisplayShow(params))
}

/// `display.viewer.audio.set`
pub fn display_viewer_audio_set(params: display::ViewerAudioSetParams) -> Request<kind::Command> {
    Request::new(MethodParams::DisplayViewerAudioSet(params))
}

/// `display.viewer.close`
pub fn display_viewer_close(params: display::ViewerConnectionParams) -> Request<kind::Command> {
    Request::new(MethodParams::DisplayViewerClose(params))
}

/// `display.viewer.open`
pub fn display_viewer_open(params: common::ComputerTargetParams) -> Request<kind::Command> {
    Request::new(MethodParams::DisplayViewerOpen(params))
}

/// `display.viewer.screen.reset`
pub fn display_viewer_screen_reset(
    params: display::ViewerConnectionParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::DisplayViewerScreenReset(params))
}

/// `display.viewer.sound.cancel`
pub fn display_viewer_sound_cancel(
    params: display::ViewerConnectionParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::DisplayViewerSoundCancel(params))
}

/// `display.viewer.sound.check`
pub fn display_viewer_sound_check(params: display::ViewerConnectionParams) -> Request<kind::Query> {
    Request::new(MethodParams::DisplayViewerSoundCheck(params))
}

/// `editor.command`
pub fn editor_command(params: terminal::EditorCommandParams) -> Request<kind::Command> {
    Request::new(MethodParams::EditorCommand(params))
}

/// `editor.includeNotice`
pub fn editor_include_notice(params: common::EmptyParams) -> Request<kind::Query> {
    Request::new(MethodParams::EditorIncludeNotice(params))
}

/// `engine.credentials.repair`
pub fn engine_credentials_repair(
    params: credentials::EngineCredentialsRepairParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::EngineCredentialsRepair(params))
}

/// `engine.credentials.status`
pub fn engine_credentials_status(
    params: credentials::EngineCredentialsStatusParams,
) -> Request<kind::Query> {
    Request::new(MethodParams::EngineCredentialsStatus(params))
}

/// `engine.disconnect`
pub fn engine_disconnect(params: engine::EngineDisconnectParams) -> Request<kind::Command> {
    Request::new(MethodParams::EngineDisconnect(params))
}

/// `engine.handshake`
pub fn engine_handshake(params: transport::HandshakeParams) -> Request<kind::Query> {
    Request::new(MethodParams::EngineHandshake(params))
}

/// `engine.shutdown`
pub fn engine_shutdown(params: engine::EngineShutdownParams) -> Request<kind::Job> {
    Request::new(MethodParams::EngineShutdown(params))
}

/// `engine.state`
pub fn engine_state(params: engine::EngineStateParams) -> Request<kind::Query> {
    Request::new(MethodParams::EngineState(params))
}

/// `engine.update.abort`
pub fn engine_update_abort(params: engine::EngineUpdateAbortParams) -> Request<kind::Command> {
    Request::new(MethodParams::EngineUpdateAbort(params))
}

/// `engine.update.prepare`
pub fn engine_update_prepare(params: engine::EngineUpdatePrepareParams) -> Request<kind::Job> {
    Request::new(MethodParams::EngineUpdatePrepare(params))
}

/// `events.subscribe`
pub fn events_subscribe(params: engine::EventsSubscribeParams) -> Request<kind::Subscription> {
    Request::new(MethodParams::EventsSubscribe(params))
}

/// `events.unsubscribe`
pub fn events_unsubscribe(params: engine::EventsUnsubscribeParams) -> Request<kind::Command> {
    Request::new(MethodParams::EventsUnsubscribe(params))
}

/// `exports.acknowledge`
pub fn exports_acknowledge(params: exports::ExportAcknowledgeParams) -> Request<kind::Command> {
    Request::new(MethodParams::ExportsAcknowledge(params))
}

/// `exports.check`
pub fn exports_check(params: exports::ExportCheckParams) -> Request<kind::Job> {
    Request::new(MethodParams::ExportsCheck(params))
}

/// `exports.create`
pub fn exports_create(params: exports::ExportCreateParams) -> Request<kind::Job> {
    Request::new(MethodParams::ExportsCreate(params))
}

/// `exports.destination.set`
pub fn exports_destination_set(
    params: exports::ExportDestinationSetParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::ExportsDestinationSet(params))
}

/// `exports.discard`
pub fn exports_discard(params: exports::ExportDiscardParams) -> Request<kind::Command> {
    Request::new(MethodParams::ExportsDiscard(params))
}

/// `exports.dismiss`
pub fn exports_dismiss(params: exports::ExportDismissParams) -> Request<kind::Command> {
    Request::new(MethodParams::ExportsDismiss(params))
}

/// `exports.import`
pub fn exports_import(params: exports::ExportImportParams) -> Request<kind::Job> {
    Request::new(MethodParams::ExportsImport(params))
}

/// `exports.inspect`
pub fn exports_inspect(params: exports::ExportInspectParams) -> Request<kind::Job> {
    Request::new(MethodParams::ExportsInspect(params))
}

/// `exports.state`
pub fn exports_state(params: exports::ExportStateParams) -> Request<kind::Query> {
    Request::new(MethodParams::ExportsState(params))
}

/// `exports.transfer`
pub fn exports_transfer(params: exports::ExportTransferParams) -> Request<kind::Job> {
    Request::new(MethodParams::ExportsTransfer(params))
}

/// `files.download`
pub fn files_download(params: files::FileDownloadParams) -> Request<kind::Job> {
    Request::new(MethodParams::FilesDownload(params))
}

/// `files.list`
pub fn files_list(params: files::FileListParams) -> Request<kind::Query> {
    Request::new(MethodParams::FilesList(params))
}

/// `files.upload`
pub fn files_upload(params: files::FileUploadParams) -> Request<kind::Job> {
    Request::new(MethodParams::FilesUpload(params))
}

/// `github.access.set`
pub fn github_access_set(params: github::GitHubAccessSetParams) -> Request<kind::Command> {
    Request::new(MethodParams::GitHubAccessSet(params))
}

/// `github.configuration.retry`
pub fn github_configuration_retry(
    params: github::GitHubConfigurationRetryParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::GitHubConfigurationRetry(params))
}

/// `github.configuration.save`
pub fn github_configuration_save(
    params: github::GitHubConfigurationSaveParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::GitHubConfigurationSave(params))
}

/// `github.connect`
pub fn github_connect(params: github::GitHubConnectParams) -> Request<kind::Job> {
    Request::new(MethodParams::GitHubConnect(params))
}

/// `github.connect.callback`
pub fn github_connect_callback(
    params: github::GitHubConnectCallbackParams,
) -> Request<kind::Stream> {
    Request::new(MethodParams::GitHubConnectCallback(params))
}

/// `github.connect.reopen`
pub fn github_connect_reopen(params: github::GitHubConnectReopenParams) -> Request<kind::Command> {
    Request::new(MethodParams::GitHubConnectReopen(params))
}

/// `github.disconnect`
pub fn github_disconnect(params: github::GitHubDisconnectParams) -> Request<kind::Command> {
    Request::new(MethodParams::GitHubDisconnect(params))
}

/// `github.personalToken.remove`
pub fn github_personal_token_remove(
    params: github::GitHubPersonalTokenRemoveParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::GitHubPersonalTokenRemove(params))
}

/// `github.personalToken.save`
pub fn github_personal_token_save(
    params: github::GitHubPersonalTokenSaveParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::GitHubPersonalTokenSave(params))
}

/// `github.repositories.manage`
pub fn github_repositories_manage(
    params: github::GitHubRepositoriesManageParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::GitHubRepositoriesManage(params))
}

/// `github.repositories.refresh`
pub fn github_repositories_refresh(
    params: github::GitHubRepositoriesRefreshParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::GitHubRepositoriesRefresh(params))
}

/// `github.state`
pub fn github_state(params: github::GitHubStateParams) -> Request<kind::Query> {
    Request::new(MethodParams::GitHubState(params))
}

/// `guest.account`
pub fn guest_account(params: guest::GuestAccountParams) -> Request<kind::Query> {
    Request::new(MethodParams::GuestAccount(params))
}

/// `guest.exec`
pub fn guest_exec(params: guest::GuestExecParams) -> Request<kind::Command> {
    Request::new(MethodParams::GuestExec(params))
}

/// `guest.exec.get`
pub fn guest_exec_get(params: guest::GuestExecGetParams) -> Request<kind::Query> {
    Request::new(MethodParams::GuestExecGet(params))
}

/// `guest.exec.input`
pub fn guest_exec_input(params: guest::GuestExecInputParams) -> Request<kind::Command> {
    Request::new(MethodParams::GuestExecInput(params))
}

/// `guest.exec.start`
pub fn guest_exec_start(params: guest::GuestExecStartParams) -> Request<kind::Job> {
    Request::new(MethodParams::GuestExecStart(params))
}

/// `guest.files.list`
pub fn guest_files_list(params: guest::GuestFilesListParams) -> Request<kind::Query> {
    Request::new(MethodParams::GuestFilesList(params))
}

/// `guest.files.read`
pub fn guest_files_read(params: guest::GuestFilesReadParams) -> Request<kind::Query> {
    Request::new(MethodParams::GuestFilesRead(params))
}

/// `guest.files.remove`
pub fn guest_files_remove(params: guest::GuestFilesRemoveParams) -> Request<kind::Command> {
    Request::new(MethodParams::GuestFilesRemove(params))
}

/// `guest.files.watch`
pub fn guest_files_watch(params: guest::GuestFilesWatchParams) -> Request<kind::Subscription> {
    Request::new(MethodParams::GuestFilesWatch(params))
}

/// `guest.files.write`
pub fn guest_files_write(params: guest::GuestFilesWriteParams) -> Request<kind::Command> {
    Request::new(MethodParams::GuestFilesWrite(params))
}

/// `guest.prepare`
pub fn guest_prepare(params: guest::GuestPrepareParams) -> Request<kind::Command> {
    Request::new(MethodParams::GuestPrepare(params))
}

/// `guestClient.enable`
pub fn guest_client_enable(
    params: guest_client::GuestClientEnableParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::GuestClientEnable(params))
}

/// `guestClient.revoke`
pub fn guest_client_revoke(
    params: guest_client::GuestClientComputerParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::GuestClientRevoke(params))
}

/// `guestClient.status`
pub fn guest_client_status(
    params: guest_client::GuestClientComputerParams,
) -> Request<kind::Query> {
    Request::new(MethodParams::GuestClientStatus(params))
}

/// `guest.ssh`
pub fn guest_ssh(params: guest::GuestSshParams) -> Request<kind::Stream> {
    Request::new(MethodParams::GuestSsh(params))
}

/// `guest.sshAlias.ensure`
pub fn guest_ssh_alias_ensure(params: guest::GuestSshAliasEnsureParams) -> Request<kind::Command> {
    Request::new(MethodParams::GuestSshAliasEnsure(params))
}

/// `guest.sshAlias.remove`
pub fn guest_ssh_alias_remove(params: guest::GuestSshAliasRemoveParams) -> Request<kind::Command> {
    Request::new(MethodParams::GuestSshAliasRemove(params))
}

/// `logs.export`
pub fn logs_export(params: logs::LogsExportParams) -> Request<kind::Job> {
    Request::new(MethodParams::LogsExport(params))
}

/// `logs.query`
pub fn logs_query(params: logs::LogsQueryParams) -> Request<kind::Query> {
    Request::new(MethodParams::LogsQuery(params))
}

/// `macos.display.connect`
pub fn macos_display_connect(params: macos::MacosDisplayConnectParams) -> Request<kind::Query> {
    Request::new(MethodParams::MacosDisplayConnect(params))
}

/// `macos.display.resize`
pub fn macos_display_resize(params: macos::MacosDisplayResizeParams) -> Request<kind::Command> {
    Request::new(MethodParams::MacosDisplayResize(params))
}

/// `macos.display.session`
pub fn macos_display_session(params: macos::MacosDisplaySessionParams) -> Request<kind::Command> {
    Request::new(MethodParams::MacosDisplaySession(params))
}

/// `macos.display.stream`
pub fn macos_display_stream(params: macos::MacosDisplayStreamParams) -> Request<kind::Stream> {
    Request::new(MethodParams::MacosDisplayStream(params))
}

/// `macos.setup.retry`
pub fn macos_setup_retry(params: macos::MacosSetupRetryParams) -> Request<kind::Job> {
    Request::new(MethodParams::MacosSetupRetry(params))
}

/// `macos.template.delete`
pub fn macos_template_delete(params: macos::MacosTemplateDeleteParams) -> Request<kind::Command> {
    Request::new(MethodParams::MacosTemplateDelete(params))
}

/// `migration.preUpgradeBackup.acknowledgeNotice`
pub fn migration_pre_upgrade_backup_acknowledge_notice(
    params: common::DeviceParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::MigrationPreUpgradeBackupAcknowledgeNotice(params))
}

/// `migration.preUpgradeBackup.delete`
pub fn migration_pre_upgrade_backup_delete(params: common::DeviceParams) -> Request<kind::Command> {
    Request::new(MethodParams::MigrationPreUpgradeBackupDelete(params))
}

/// `migration.preUpgradeBackup.get`
pub fn migration_pre_upgrade_backup_get(params: common::DeviceParams) -> Request<kind::Query> {
    Request::new(MethodParams::MigrationPreUpgradeBackupGet(params))
}

/// `migration.preUpgradeBackup.measure`
pub fn migration_pre_upgrade_backup_measure(params: common::DeviceParams) -> Request<kind::Query> {
    Request::new(MethodParams::MigrationPreUpgradeBackupMeasure(params))
}

/// `migration.state`
pub fn migration_state(params: common::DeviceParams) -> Request<kind::Query> {
    Request::new(MethodParams::MigrationState(params))
}

/// `network.open`
pub fn network_open(params: network::NetworkOpenParams) -> Request<kind::Command> {
    Request::new(MethodParams::NetworkOpen(params))
}

/// `network.publish`
pub fn network_publish(params: network::NetworkPublishParams) -> Request<kind::Command> {
    Request::new(MethodParams::NetworkPublish(params))
}

/// `network.state`
pub fn network_state(params: network::NetworkStateParams) -> Request<kind::Query> {
    Request::new(MethodParams::NetworkState(params))
}

/// `network.unpublish`
pub fn network_unpublish(params: network::NetworkUnpublishParams) -> Request<kind::Command> {
    Request::new(MethodParams::NetworkUnpublish(params))
}

/// `network.urls`
pub fn network_urls(params: network::NetworkTargetParams) -> Request<kind::Query> {
    Request::new(MethodParams::NetworkUrls(params))
}

/// `network.policy.get`
pub fn network_policy_get(params: network::NetworkTargetParams) -> Request<kind::Query> {
    Request::new(MethodParams::NetworkPolicyGet(params))
}

/// `network.policy.set`
pub fn network_policy_set(params: network::NetworkPolicySetParams) -> Request<kind::Command> {
    Request::new(MethodParams::NetworkPolicySet(params))
}

/// `notifications.list`
pub fn notifications_list(params: notifications::NotificationsListParams) -> Request<kind::Query> {
    Request::new(MethodParams::NotificationsList(params))
}

/// `operations.cancel`
pub fn operations_cancel(params: operations::CancelJobParams) -> Request<kind::Command> {
    Request::new(MethodParams::OperationsCancel(params))
}

/// `operations.job.acknowledge`
pub fn operations_job_acknowledge(
    params: operations::AcknowledgeJobParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::OperationsJobAcknowledge(params))
}

/// `operations.job.get`
pub fn operations_job_get(params: operations::GetJobParams) -> Request<kind::Query> {
    Request::new(MethodParams::OperationsJobGet(params))
}

/// `operations.list`
pub fn operations_list(params: operations::OperationsListParams) -> Request<kind::Query> {
    Request::new(MethodParams::OperationsList(params))
}

/// `preparation.retry`
pub fn preparation_retry(params: preparation::PreparationRetryParams) -> Request<kind::Command> {
    Request::new(MethodParams::PreparationRetry(params))
}

/// `preparation.status`
pub fn preparation_status(params: preparation::PreparationStatusParams) -> Request<kind::Query> {
    Request::new(MethodParams::PreparationStatus(params))
}

/// `push.dismiss`
pub fn push_dismiss(params: push::PushDismissParams) -> Request<kind::Command> {
    Request::new(MethodParams::PushDismiss(params))
}

/// `push.repositories`
pub fn push_repositories(params: push::PushRepositoriesParams) -> Request<kind::Query> {
    Request::new(MethodParams::PushRepositories(params))
}

/// `push.start`
pub fn push_start(params: push::PushStartParams) -> Request<kind::Job> {
    Request::new(MethodParams::PushStart(params))
}

/// `push.status`
pub fn push_status(params: push::PushStatusParams) -> Request<kind::Query> {
    Request::new(MethodParams::PushStatus(params))
}

/// `recipes.build`
pub fn recipes_build(params: recipes::RecipeBuildParams) -> Request<kind::Job> {
    Request::new(MethodParams::RecipesBuild(params))
}

/// `recipes.get`
pub fn recipes_get(params: recipes::RecipeGetParams) -> Request<kind::Query> {
    Request::new(MethodParams::RecipesGet(params))
}

/// `recipes.list`
pub fn recipes_list(params: common::DeviceParams) -> Request<kind::Query> {
    Request::new(MethodParams::RecipesList(params))
}

/// `recipes.remove`
pub fn recipes_remove(params: recipes::RecipeRemoveParams) -> Request<kind::Command> {
    Request::new(MethodParams::RecipesRemove(params))
}

/// `recipes.save`
pub fn recipes_save(params: recipes::RecipeSaveParams) -> Request<kind::Command> {
    Request::new(MethodParams::RecipesSave(params))
}

/// `secrets.list`
pub fn secrets_list(params: common::DeviceParams) -> Request<kind::Query> {
    Request::new(MethodParams::SecretsList(params))
}

/// `secrets.remove`
pub fn secrets_remove(params: secrets::SecretsRemoveParams) -> Request<kind::Command> {
    Request::new(MethodParams::SecretsRemove(params))
}

/// `secrets.retry`
pub fn secrets_retry(params: secrets::SecretsRetryParams) -> Request<kind::Command> {
    Request::new(MethodParams::SecretsRetry(params))
}

/// `secrets.save`
pub fn secrets_save(params: secrets::SecretsSaveParams) -> Request<kind::Command> {
    Request::new(MethodParams::SecretsSave(params))
}

/// `settings.flush`
pub fn settings_flush(params: settings::SettingsFlushParams) -> Request<kind::Command> {
    Request::new(MethodParams::SettingsFlush(params))
}

/// `settings.read`
pub fn settings_read(params: settings::SettingsReadParams) -> Request<kind::Query> {
    Request::new(MethodParams::SettingsRead(params))
}

/// `settings.update`
pub fn settings_update(params: settings::SettingsUpdateParams) -> Request<kind::Command> {
    Request::new(MethodParams::SettingsUpdate(params))
}

/// `sshAccess.connection`
pub fn ssh_access_connection(
    params: ssh_access::SshAccessConnectionParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::SshAccessConnection(params))
}

/// `sshAccess.save`
pub fn ssh_access_save(params: ssh_access::SshAccessSaveParams) -> Request<kind::Command> {
    Request::new(MethodParams::SshAccessSave(params))
}

/// `sshAccess.state`
pub fn ssh_access_state(params: common::DeviceParams) -> Request<kind::Query> {
    Request::new(MethodParams::SshAccessState(params))
}

/// `steps.list`
pub fn steps_list(params: steps::StepsListParams) -> Request<kind::Query> {
    Request::new(MethodParams::StepsList(params))
}

/// `steps.record`
pub fn steps_record(params: steps::StepsRecordParams) -> Request<kind::Command> {
    Request::new(MethodParams::StepsRecord(params))
}

/// `storage.read`
pub fn storage_read(params: storage::StorageParams) -> Request<kind::Query> {
    Request::new(MethodParams::StorageRead(params))
}

/// `storage.reclaim`
pub fn storage_reclaim(params: storage::StorageParams) -> Request<kind::Job> {
    Request::new(MethodParams::StorageReclaim(params))
}

/// `terminal.command`
pub fn terminal_command(params: terminal::TerminalCommandParams) -> Request<kind::Query> {
    Request::new(MethodParams::TerminalCommand(params))
}

/// `updates.check`
pub fn updates_check(params: updates::UpdatesCheckParams) -> Request<kind::Command> {
    Request::new(MethodParams::UpdatesCheck(params))
}

/// `updates.download`
pub fn updates_download(params: updates::UpdatesDownloadParams) -> Request<kind::Job> {
    Request::new(MethodParams::UpdatesDownload(params))
}

/// `updates.focused`
pub fn updates_focused(params: updates::UpdatesFocusedParams) -> Request<kind::Command> {
    Request::new(MethodParams::UpdatesFocused(params))
}

/// `updates.setAutomaticChecks`
pub fn updates_set_automatic_checks(
    params: updates::UpdatesSetAutomaticChecksParams,
) -> Request<kind::Command> {
    Request::new(MethodParams::UpdatesSetAutomaticChecks(params))
}

/// `updates.state`
pub fn updates_state(params: common::EmptyParams) -> Request<kind::Query> {
    Request::new(MethodParams::UpdatesState(params))
}
