//! Computers on another device: SSH aliases, terminal and editor commands, file transfers
//! and folder watches (`guest.sshAlias.*`, `terminal.command`, `editor.command`,
//! `files.upload`, `files.download`, `guest.files.watch`).
//!
//! These methods act on the controller's own machine (its SSH configuration, its local
//! files, the client that asked), so they are not forwarded as requests. The controller's
//! engine does the local half itself and asks the owner for the rest:
//! - **SSH.** The controller has one key per owner (`desktop-remote/ssh/<deviceId>.key`),
//!   authorized in the guest through the owner's `guest.prepare`. The entry
//!   `silo-remote-<deviceId>-<computerId>` reaches the computer through the ProxyCommand
//!   `--remote-guest <deviceId> <computerId>`, which opens the owner's `guest.ssh` stream
//!   over the Connections bridge. Terminals, editors and SFTP transfers all ride on it.
//! - **Transfers.** The same entry carries `sftp`; the job, its progress and its
//!   cancellation are this engine's.
//! - **Watches.** The owner runs the watch and streams its hints over `guest.files.watch`
//!   (`silo_engine::remote_watch`); this engine re-publishes each one to the subscription of
//!   the client that asked.
//!
//! The owner's OS is read from its `guest.account`: a macOS computer's account is
//! `/Users/silo`, which decides the workspace folders.

use std::{
    path::Path,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use silo_api::{
    jobs::JobId,
    methods::{
        files::{ConflictPolicy, FileDownloadParams, FileUploadParams, UploadOrigin},
        guest::{
            GuestAccount, GuestFilesChangedPayload, GuestFilesWatchParams, GuestFilesWatchResult,
            GuestPrepareResult, GuestSshAliasEnsureParams, GuestSshAliasEnsureResult,
            GuestSshAliasRemoveParams,
        },
        terminal::{CommandResult, EditorCommandParams, TerminalCommandParams},
    },
};
use silo_engine::{
    bridge_error::BridgeError,
    context::{EngineContext, Event},
    editor::{self, RemoteAccess, RemoteAlias},
    guest::GuestError,
    remote_watch::{WatchFrame, GUEST_WATCH_STREAM},
};

use super::{access::call_owner, guest, HandlerContext};
use crate::dispatcher::{DispatchError, HandlerOutput};

/// Silence on a watch stream that means it is lost; the owner sends a heartbeat every 10 s.
const WATCH_STALL: Duration = Duration::from_secs(35);

/// The device a request names when it is not this engine.
pub(super) fn remote_device(context: &HandlerContext, device_id: Option<&str>) -> Option<String> {
    device_id
        .filter(|id| *id != "local" && *id != context.silod_context().local_device().device_id)
        .map(str::to_owned)
}

fn output(value: impl serde::Serialize) -> Result<HandlerOutput, DispatchError> {
    serde_json::to_value(value)
        .map(|value| HandlerOutput {
            value,
            event_barrier: None,
        })
        .map_err(|error| DispatchError::internal(error.to_string()))
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, DispatchError> + Send + 'static,
) -> Result<T, DispatchError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| DispatchError::internal(format!("The remote task failed: {error}")))?
}

fn plain(message: String) -> DispatchError {
    guest::guest_error(GuestError::from(message))
}

/// The owner's working account for a computer: which OS it runs decides its folders.
struct Owned {
    account: GuestAccount,
    macos: bool,
}

fn owned(device: &str, computer: &str) -> Result<Owned, DispatchError> {
    let reply = call_owner(device, "guest.account", json!({ "computerId": computer }))
        .map_err(HandlerContext::map_engine_error)?;
    let account: GuestAccount = serde_json::from_value(reply).map_err(|error| {
        DispatchError::internal(format!(
            "The other device returned an invalid reply to guest.account: {error}"
        ))
    })?;
    let macos = account.home == editor::MACOS_HOME;
    Ok(Owned { account, macos })
}

/// Writes the alias for `computer`, asking the owner to authorize this device's key for
/// `path`, and checks the connection when `probe` is set.
fn write_alias(
    user_home: &Path,
    device: &str,
    computer: &str,
    path: &str,
    install_include: bool,
    probe: bool,
) -> Result<RemoteAlias, DispatchError> {
    editor::require_openssh("connect to computers over SSH").map_err(plain)?;
    let refusal: Mutex<Option<BridgeError>> = Mutex::new(None);
    let authorize = |public: &str| -> Result<RemoteAccess, String> {
        let reply = call_owner(
            device,
            "guest.prepare",
            json!({
                "computerId": computer,
                "publicKey": public,
                "path": path,
                "accountProtocol": 1,
            }),
        )
        .map_err(|error| {
            let message = error.message.clone();
            *refusal
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error);
            message
        })?;
        let prepared: GuestPrepareResult = serde_json::from_value(reply).map_err(|error| {
            format!("The other device returned an invalid reply to guest.prepare: {error}")
        })?;
        Ok(RemoteAccess {
            host_public_key: prepared.host_public_key,
            user: prepared.user,
            guest_address: None,
        })
    };
    let alias =
        editor::ensure_remote_alias(user_home, device, computer, install_include, &authorize)
            .map_err(|message| {
                match refusal
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                {
                    Some(error) => HandlerContext::map_engine_error(error),
                    None => plain(message),
                }
            })?;
    if probe {
        probe_alias(&alias.config, &alias.alias).map_err(plain)?;
    }
    Ok(alias)
}

fn probe_alias(config: &Path, alias: &str) -> Result<(), String> {
    #[cfg(test)]
    {
        tests::probed(config, alias)
    }
    #[cfg(not(test))]
    {
        editor::probe_alias(config, alias)
    }
}

pub(super) async fn ssh_alias_ensure(
    device: String,
    params: GuestSshAliasEnsureParams,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let home = context.silod_context().paths().home.clone();
    let result = blocking(move || {
        let owner = owned(&device, &params.computer_id)?;
        let path = params
            .path
            .unwrap_or_else(|| editor::remote_workspace(owner.macos).to_owned());
        editor::validate_remote_path(&path, owner.macos).map_err(plain)?;
        let alias = write_alias(
            &home,
            &device,
            &params.computer_id,
            &path,
            params.install_include.unwrap_or(true),
            true,
        )?;
        Ok(GuestSshAliasEnsureResult {
            alias: alias.alias,
            user: alias.user,
            home: owner.account.home,
            config_path: alias.config.to_string_lossy().into_owned(),
            include_line: alias.include,
            include_installed: alias.include_installed,
        })
    })
    .await?;
    output(result)
}

pub(super) async fn ssh_alias_remove(
    device: String,
    params: GuestSshAliasRemoveParams,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let home = context.silod_context().paths().home.clone();
    blocking(move || {
        editor::remove_remote_alias(&home, &device, &params.computer_id).map_err(plain)
    })
    .await?;
    output(Value::Null)
}

pub(super) async fn terminal_command(
    device: String,
    params: TerminalCommandParams,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let home = context.silod_context().paths().home.clone();
    let argv = blocking(move || {
        let owner = owned(&device, &params.computer_id)?;
        let alias = write_alias(
            &home,
            &device,
            &params.computer_id,
            editor::remote_workspace(owner.macos),
            true,
            false,
        )?;
        editor::remote_terminal_argv(&alias.config, &alias.alias).map_err(plain)
    })
    .await?;
    output(CommandResult { argv })
}

pub(super) async fn editor_command(
    device: String,
    params: EditorCommandParams,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let home = context.silod_context().paths().home.clone();
    let argv = blocking(move || {
        let owner = owned(&device, &params.computer_id)?;
        let path = params
            .path
            .unwrap_or_else(|| editor::remote_workspace(owner.macos).to_owned());
        editor::validate_remote_path(&path, owner.macos).map_err(plain)?;
        let command = selected_editor_command()?;
        editor_argv(
            &home,
            &device,
            &params.computer_id,
            &path,
            owner.macos,
            &command,
        )
    })
    .await?;
    output(CommandResult { argv })
}

fn selected_editor_command(
) -> Result<silo_engine::applications::launch::EditorCommand, DispatchError> {
    let settings = silo_engine::settings::current_settings()
        .map_err(|error| guest::guest_error(error.into()))?;
    let application =
        silo_engine::applications::selected_editor_from_settings(&settings).map_err(plain)?;
    silo_engine::applications::editor_command(&application).map_err(plain)
}

/// The argv that opens `path` of a computer on another device in the editor `command`.
fn editor_argv(
    home: &Path,
    device: &str,
    computer: &str,
    path: &str,
    macos: bool,
    command: &silo_engine::applications::launch::EditorCommand,
) -> Result<Vec<String>, DispatchError> {
    let alias = write_alias(home, device, computer, path, true, true)?;
    editor::remote_editor_argv(command, &alias.alias, path, home, macos).map_err(plain)
}

// ---------------------------------------------------------------------------------------
// files.upload, files.download

/// This device's SFTP configuration for a computer on another device.
fn transport(
    home: std::path::PathBuf,
    device: String,
    computer: String,
) -> impl FnOnce() -> Result<silo_engine::transfer::RemoteTransport, String> {
    move || {
        let owner = owned(&device, &computer).map_err(|error| error.message)?;
        let alias = write_alias(
            &home,
            &device,
            &computer,
            editor::remote_workspace(owner.macos),
            false,
            false,
        )
        .map_err(|error| error.message)?;
        let roots = if owner.macos {
            silo_engine::macos_computers::transfer_roots()
        } else {
            silo_engine::transfer::linux_transfer_roots()
        };
        Ok(silo_engine::transfer::RemoteTransport {
            config: alias.config,
            alias: alias.alias,
            roots,
        })
    }
}

pub(super) async fn files_upload(
    device: String,
    params: FileUploadParams,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    guest::check_transfer_id(&params.transfer_id)?;
    if params.selection.is_some() {
        return Err(DispatchError::internal(
            "Invalid params: pass the files to upload as paths; selection tokens belong to the app's own pickers.",
        ));
    }
    let Some(paths) = params.paths.filter(|paths| !paths.is_empty()) else {
        return Err(DispatchError::internal(
            "Invalid params: pass the files to upload as paths.",
        ));
    };
    let owner = match params.origin {
        Some(UploadOrigin::Viewer) => {
            let Some(connection) = params.connection_id.as_deref().filter(|id| !id.is_empty())
            else {
                return Err(DispatchError::internal(
                    "Invalid params: a viewer upload needs its connectionId.",
                ));
            };
            format!("desktop-shell-{connection}")
        }
        _ => "main".to_owned(),
    };
    let conflict = match params.conflict {
        ConflictPolicy::Ask => silo_engine::transfer::ConflictPolicy::Ask,
        ConflictPolicy::Replace => silo_engine::transfer::ConflictPolicy::Replace,
        ConflictPolicy::KeepBoth => silo_engine::transfer::ConflictPolicy::KeepBoth,
    };
    let engine = Arc::new(context.silod_context().clone());
    let home = engine.paths().home.clone();
    let job_id = JobId::new(format!("transfer:{}", params.transfer_id))
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    let (transfer, directory, computer) =
        (params.transfer_id, params.directory, params.computer_id);
    guest::run_transfer(engine, job_id, move || {
        let selection = silo_engine::transfer::issue_upload_selection(&owner, paths);
        let token = serde_json::to_value(&selection)
            .ok()
            .and_then(|value| value["token"].as_str().map(str::to_owned))
            .ok_or("Could not prepare the upload.")?;
        silo_engine::transfer::upload_files_remote(
            transport(home, device, computer.clone()),
            owner,
            transfer,
            computer,
            directory,
            token,
            conflict,
        )
        .map(|_| ())
    })
    .await
}

pub(super) async fn files_download(
    device: String,
    params: FileDownloadParams,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    guest::check_transfer_id(&params.transfer_id)?;
    if params
        .destination
        .as_deref()
        .is_some_and(|destination| !destination.starts_with('/'))
    {
        return Err(DispatchError::internal(
            "Invalid params: destination must be an absolute path.",
        ));
    }
    let engine = Arc::new(context.silod_context().clone());
    let home = engine.paths().home.clone();
    let job_id = JobId::new(format!("transfer:{}", params.transfer_id))
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    let downloads = home.join("Downloads");
    let (transfer, path, destination, computer) = (
        params.transfer_id,
        params.path,
        params.destination,
        params.computer_id,
    );
    guest::run_transfer(engine, job_id, move || {
        let (session, file_name) = silo_engine::transfer::prepare_download_remote(
            transport(home, device, computer.clone()),
            transfer,
            computer,
            path,
        )?;
        let destination = destination
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| downloads.join(file_name));
        silo_engine::transfer::download_prepared(session, Some(destination)).map(|_| ())
    })
    .await
}

// ---------------------------------------------------------------------------------------
// guest.files.watch

/// An open `guest.files.watch` stream from an owner.
pub(crate) struct WatchLink {
    pub frames: mpsc::Receiver<Result<WatchFrame, String>>,
    /// Closes the stream when dropped.
    pub _guard: Box<dyn Send>,
}

fn open_watch(device: &str, params: Value) -> Result<WatchLink, DispatchError> {
    #[cfg(test)]
    if let Some(link) = tests::stubbed_watch(device, &params) {
        return link.map_err(plain);
    }
    let opened =
        silo_engine::remote::open_bridge_stream(device, GUEST_WATCH_STREAM, params, &|| false)
            .map_err(HandlerContext::map_engine_error)?;
    let silo_engine::remote::BridgeStream {
        child,
        input,
        mut output,
    } = opened;
    let (sender, frames) = mpsc::sync_channel(256);
    std::thread::Builder::new()
        .name("silod-watch-reader".into())
        .spawn(move || loop {
            let frame = silo_engine::remote_watch::read_watch_frame(&mut output);
            let failed = frame.is_err();
            if sender.send(frame).is_err() || failed {
                break;
            }
        })
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    Ok(WatchLink {
        frames,
        _guard: Box::new((child, input)),
    })
}

/// `guest.files.watch` for a computer on another device: the owner watches, this engine
/// hands each hint to the caller's own subscription, exactly as for a local watch. The
/// watch ends with `events.unsubscribe`, when the client disconnects, or when the stream to
/// the owner is lost (the subscription is then closed, as when a local computer stops).
pub(super) async fn files_watch(
    device: String,
    params: GuestFilesWatchParams,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    if params.paths.is_empty() {
        return Err(DispatchError::internal(
            "Invalid params: watch between 1 and 32 folders.",
        ));
    }
    let client = context.client_id().clone();
    let subscription = context
        .silod_context()
        .subscribe_events(&client, vec!["guest.files.changed".to_owned()])
        .map_err(DispatchError::internal)?;
    let bus = context.event_bus().clone();
    let request = json!({ "computerId": params.computer_id, "paths": params.paths });
    let opened = blocking(move || open_watch(&device, request)).await;
    let link = match opened {
        Ok(link) => link,
        Err(error) => {
            context
                .silod_context()
                .unsubscribe_events(&client, &subscription);
            return Err(error);
        }
    };
    let engine = Arc::new(context.silod_context().clone());
    let spawned = {
        let (client, subscription) = (client.clone(), subscription.clone());
        std::thread::Builder::new()
            .name("silod-watch-relay".into())
            .spawn(move || relay_watch(&engine, &bus, &client, &subscription, link))
    };
    if let Err(error) = spawned {
        context
            .silod_context()
            .unsubscribe_events(&client, &subscription);
        return Err(DispatchError::internal(error.to_string()));
    }
    output(GuestFilesWatchResult {
        subscription_id: subscription,
    })
}

/// Re-publishes one owner watch to the subscription that asked for it, until it is no longer
/// wanted or the stream ends.
fn relay_watch(
    engine: &crate::context::SilodContext,
    bus: &silo_engine::events::EventBus,
    client: &silo_engine::context::ClientId,
    subscription: &str,
    link: WatchLink,
) {
    let mut last_frame = Instant::now();
    let lost = loop {
        if !bus.has_subscription(client, subscription) {
            break false;
        }
        match link.frames.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(WatchFrame::Heartbeat)) => last_frame = Instant::now(),
            Ok(Ok(WatchFrame::Changed(path))) => {
                last_frame = Instant::now();
                let payload = GuestFilesChangedPayload {
                    subscription_id: subscription.to_owned(),
                    path,
                };
                if let Ok(payload) = serde_json::to_value(payload) {
                    bus.publish_to_subscription(
                        client,
                        subscription,
                        Event::new("guest.files.changed", payload),
                    );
                }
            }
            Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => break true,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if last_frame.elapsed() >= WATCH_STALL {
                    break true;
                }
            }
        }
    };
    drop(link);
    if lost {
        engine.unsubscribe_events(client, subscription);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{collections::HashMap, sync::OnceLock};

    type WatchStub = Arc<dyn Fn(&Value) -> Result<WatchLink, String> + Send + Sync>;

    fn watch_stubs() -> &'static Mutex<HashMap<String, WatchStub>> {
        static STUBS: OnceLock<Mutex<HashMap<String, WatchStub>>> = OnceLock::new();
        STUBS.get_or_init(Default::default)
    }

    /// Answers `guest.files.watch` streams to `device` until the guard drops.
    pub(crate) struct StubWatch(String);

    pub(crate) fn stub_watch(
        device: &str,
        open: impl Fn(&Value) -> Result<WatchLink, String> + Send + Sync + 'static,
    ) -> StubWatch {
        watch_stubs()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(device.to_owned(), Arc::new(open));
        StubWatch(device.to_owned())
    }

    impl Drop for StubWatch {
        fn drop(&mut self) {
            watch_stubs()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&self.0);
        }
    }

    pub(super) fn stubbed_watch(device: &str, params: &Value) -> Option<Result<WatchLink, String>> {
        let stub = watch_stubs()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(device)
            .cloned()?;
        Some(stub(params))
    }

    static PROBED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
    /// Aliases whose connection check fails.
    static PROBE_FAILS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    pub(super) fn probed(config: &Path, alias: &str) -> Result<(), String> {
        PROBED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((config.to_string_lossy().into_owned(), alias.to_owned()));
        if PROBE_FAILS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .any(|failing| failing == alias)
        {
            Err("Could not connect to this computer over SSH. Retry after checking that it is running.".into())
        } else {
            Ok(())
        }
    }

    /// Makes the connection check of one alias fail until dropped.
    pub(crate) struct FailProbes(String);

    impl FailProbes {
        pub(crate) fn new(alias: &str) -> Self {
            PROBE_FAILS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(alias.to_owned());
            Self(alias.to_owned())
        }
    }

    impl Drop for FailProbes {
        fn drop(&mut self) {
            PROBE_FAILS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .retain(|alias| *alias != self.0);
        }
    }

    pub(crate) fn probes() -> Vec<(String, String)> {
        PROBED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    // -----------------------------------------------------------------------------------

    use crate::methods::access::tests::{fixture, request_context, stub_owner, StubOwner};
    use silo_api::{jobs::ErrorCode, methods::MethodName};
    use silo_engine::bridge_error::ErrorCode as BridgeCode;

    const HOST_KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKaDgVuE9Eq4wyQDZgT0kPA3H";

    type Seen = Arc<Mutex<Vec<(String, Value)>>>;

    /// An owner that reports a Linux (`/home/silo`) or macOS (`/Users/silo`) account and
    /// authorizes whatever key it is given.
    fn owner(device: &str, home: &'static str) -> (Seen, StubOwner) {
        let seen: Seen = Arc::default();
        let log = seen.clone();
        let stub = stub_owner(device, move |method, params| {
            log.lock()
                .unwrap()
                .push((method.to_owned(), params.clone()));
            match method {
                "guest.account" => Ok(json!({"user": "silo", "uid": 1000, "home": home})),
                "guest.prepare" => {
                    Ok(json!({"hostPublicKey": HOST_KEY, "user": "silo", "guestAddress": null}))
                }
                other => panic!("unexpected owner request {other}"),
            }
        });
        (seen, stub)
    }

    async fn call(method: MethodName, params: Value) -> Result<Value, DispatchError> {
        crate::methods::dispatch(&request_context(), method, &params)
            .await
            .map(|output| output.value)
    }

    fn ids(n: u8) -> (String, String) {
        (
            format!("88888888-8888-4888-8888-8888888899{n:02}"),
            format!("99999999-9999-4999-8999-9999999999{n:02}"),
        )
    }

    fn alias_name(device: &str, computer: &str) -> String {
        format!(
            "{}-{device}-{computer}",
            silo_engine::channel::current().remote_alias_prefix()
        )
    }

    fn entry(device: &str, computer: &str) -> std::path::PathBuf {
        silo_engine::channel::current()
            .state_dir(&home())
            .join(format!("desktop-remote/ssh/{device}-{computer}.conf"))
    }

    fn home() -> std::path::PathBuf {
        fixture().context.paths().home.clone()
    }

    #[tokio::test]
    async fn a_remote_alias_proxies_through_the_owner_with_this_devices_key() {
        let (device, computer) = ids(1);
        let (seen, _owner) = owner(&device, "/home/silo");
        let result = call(
            MethodName::GuestSshAliasEnsure,
            json!({"computerId": computer, "deviceId": device, "path": "/workspace/project"}),
        )
        .await
        .unwrap();
        let alias = alias_name(&device, &computer);
        assert_eq!(result["alias"], alias.as_str());
        assert_eq!(result["user"], "silo");
        assert_eq!(result["home"], "/home/silo");
        assert_eq!(result["includeInstalled"], true);
        // The owner is asked, in the canonical names and without a device, who the account
        // is and to authorize this device's public key for the folder.
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].0, "guest.account");
        assert_eq!(seen[1].0, "guest.prepare");
        let prepare = &seen[1].1;
        assert_eq!(prepare["computerId"], computer.as_str());
        assert_eq!(prepare["path"], "/workspace/project");
        assert_eq!(prepare["accountProtocol"], 1);
        assert!(prepare.get("deviceId").is_none());
        let public = prepare["publicKey"].as_str().unwrap();
        assert!(public.starts_with("ssh-ed25519 "), "{public}");
        assert!(!public.contains("PRIVATE"));
        // The configuration is this device's: its key, the owner's pinned host key and a
        // ProxyCommand that opens the owner's guest.ssh stream.
        let config = std::fs::read_to_string(result["configPath"].as_str().unwrap()).unwrap();
        assert!(config.starts_with(&format!("Host {alias}\n")), "{config}");
        assert!(config.contains("User silo\n"));
        assert!(config.contains(&format!("{device}.key")));
        assert!(config.contains("IdentityAgent none\n"));
        assert!(config.contains("StrictHostKeyChecking yes\n"));
        assert!(config.contains(&format!("'--remote-guest' '{device}' '{computer}'")));
        let known_hosts = result["configPath"]
            .as_str()
            .unwrap()
            .replace(".conf", ".known_hosts");
        assert_eq!(
            std::fs::read_to_string(known_hosts).unwrap(),
            format!("{alias} {HOST_KEY}\n")
        );
        // The Include makes it visible to any client, and the connection was checked.
        let user_config = std::fs::read_to_string(home().join(".ssh/config")).unwrap();
        assert!(user_config.contains(result["includeLine"].as_str().unwrap()));
        assert!(tests::probes().iter().any(|(_, probed)| *probed == alias));
    }

    #[tokio::test]
    async fn the_include_can_be_left_out_and_a_macos_owner_uses_its_own_workspace() {
        let (device, computer) = ids(2);
        let (seen, _owner) = owner(&device, "/Users/silo");
        let result = call(
            MethodName::GuestSshAliasEnsure,
            json!({"computerId": computer, "deviceId": device, "installInclude": false}),
        )
        .await
        .unwrap();
        assert_eq!(result["home"], "/Users/silo");
        let prepare = seen.lock().unwrap()[1].1.clone();
        assert_eq!(prepare["path"], "/Users/silo/workspace");
        // A Linux folder is not a macOS folder, and nothing reaches the owner for it.
        let before = seen.lock().unwrap().len();
        let error = call(
            MethodName::GuestSshAliasEnsure,
            json!({"computerId": computer, "deviceId": device, "path": "/workspace/x"}),
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("/Users/silo/workspace"), "{error:?}");
        assert!(
            seen.lock().unwrap()[before..]
                .iter()
                .all(|(method, _)| method != "guest.prepare"),
            "an invalid folder must not authorize a key"
        );
    }

    #[tokio::test]
    async fn the_owners_refusal_keeps_its_code_and_writes_no_entry() {
        let (device, computer) = ids(3);
        let _owner = stub_owner(&device, |method, _| match method {
            "guest.account" => Ok(json!({"user": "silo", "uid": 1000, "home": "/home/silo"})),
            _ => Err(
                BridgeError::new(BridgeCode::Internal, "Start this computer to connect.")
                    .with_api_code("not_running"),
            ),
        });
        let error = call(
            MethodName::GuestSshAliasEnsure,
            json!({"computerId": computer, "deviceId": device}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotRunning);
        assert!(!entry(&device, &computer).exists());
        // An unreachable owner is named, not mistaken for the computer being gone.
        let (gone, computer) = ids(4);
        let error = call(
            MethodName::GuestSshAliasEnsure,
            json!({"computerId": computer, "deviceId": gone}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn a_failing_connection_check_is_an_error_and_the_entry_stays_for_the_next_try() {
        let (device, computer) = ids(5);
        let (_seen, _owner) = owner(&device, "/home/silo");
        let _guard = tests::FailProbes::new(&alias_name(&device, &computer));
        let error = call(
            MethodName::GuestSshAliasEnsure,
            json!({"computerId": computer, "deviceId": device}),
        )
        .await
        .unwrap_err();
        assert!(error.message.starts_with("Could not connect"), "{error:?}");
        drop(_guard);
        assert!(entry(&device, &computer).exists());
    }

    #[tokio::test]
    async fn removing_a_remote_alias_deletes_its_files_and_is_idempotent() {
        let (device, computer) = ids(6);
        let (_seen, _owner) = owner(&device, "/home/silo");
        let result = call(
            MethodName::GuestSshAliasEnsure,
            json!({"computerId": computer, "deviceId": device}),
        )
        .await
        .unwrap();
        let config = std::path::PathBuf::from(result["configPath"].as_str().unwrap());
        assert!(config.exists());
        for _ in 0..2 {
            let removed = call(
                MethodName::GuestSshAliasRemove,
                json!({"computerId": computer, "deviceId": device}),
            )
            .await
            .unwrap();
            assert!(removed.is_null());
        }
        assert!(!config.exists());
        assert!(!config.with_extension("known_hosts").exists());
        // This device's key stays: it belongs to the owner, not to one computer.
        assert!(config
            .parent()
            .unwrap()
            .join(format!("{device}.key"))
            .exists());
    }

    #[tokio::test]
    async fn a_remote_terminal_is_ssh_through_the_alias() {
        let (device, computer) = ids(7);
        let (seen, _owner) = owner(&device, "/home/silo");
        let result = call(
            MethodName::TerminalCommand,
            json!({"computerId": computer, "deviceId": device}),
        )
        .await
        .unwrap();
        let alias = alias_name(&device, &computer);
        let argv: Vec<&str> = result["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_str().unwrap())
            .collect();
        assert_eq!(argv[..2], ["/usr/bin/ssh", "-F"]);
        assert!(
            argv[2].ends_with(&format!("{device}-{computer}.conf")),
            "{argv:?}"
        );
        assert_eq!(argv[3..], ["-t", alias.as_str()]);
        assert!(std::path::Path::new(argv[2]).exists());
        let seen = seen.lock().unwrap();
        assert_eq!(seen[1].1["path"], "/workspace");
    }

    #[test]
    fn a_remote_editor_opens_the_folder_through_the_alias_for_either_os() {
        let (device, computer) = ids(8);
        let (_seen, _owner) = owner(&device, "/home/silo");
        let command = silo_engine::applications::launch::EditorCommand {
            program: "/usr/local/bin/zed".into(),
            args: vec![],
            zed: true,
        };
        let alias = alias_name(&device, &computer);
        let linux = editor_argv(
            &home(),
            &device,
            &computer,
            "/workspace/app",
            false,
            &command,
        )
        .unwrap();
        assert_eq!(
            linux,
            [
                "/usr/local/bin/zed".to_owned(),
                format!("ssh://{alias}/workspace/app")
            ]
        );
        let macos = editor_argv(
            &home(),
            &device,
            &computer,
            "/Users/silo/workspace/app",
            true,
            &command,
        )
        .unwrap();
        assert_eq!(
            macos,
            [
                "/usr/local/bin/zed".to_owned(),
                format!("ssh://{alias}/Users/silo/workspace/app")
            ]
        );
    }

    #[test]
    fn a_remote_transfer_uses_the_aliases_config_and_the_owners_folders() {
        let (device, computer) = ids(9);
        let (_seen, _owner) = owner(&device, "/home/silo");
        let linux = transport(home(), device.clone(), computer.clone())().unwrap();
        assert_eq!(linux.alias, alias_name(&device, &computer));
        assert!(linux.config.exists());
        assert_eq!(linux.roots[0], "/workspace");
        let (device, computer) = ids(10);
        let (_seen, _owner) = owner(&device, "/Users/silo");
        let macos = transport(home(), device, computer)().unwrap();
        assert_eq!(macos.roots[0], "/Users/silo/workspace");
        assert_eq!(macos.roots[1], "/Users/silo/Downloads");
    }

    #[tokio::test]
    async fn a_remote_transfer_that_cannot_connect_fails_through_its_job() {
        let (device, computer) = ids(11);
        let _owner = stub_owner(&device, |method, _| match method {
            "guest.account" => Ok(json!({"user": "silo", "uid": 1000, "home": "/home/silo"})),
            _ => Err(
                BridgeError::new(BridgeCode::Internal, "Start this computer to connect.")
                    .with_api_code("not_running"),
            ),
        });
        let started = call(
            MethodName::FilesDownload,
            json!({
                "transferId": "remote-down-1", "computerId": computer, "deviceId": device,
                "path": "/workspace/a"
            }),
        )
        .await
        .unwrap();
        // The same job contract as a local transfer: the job exists and carries the failure.
        assert_eq!(started["jobId"], "transfer:remote-down-1");
        let registry = fixture().context.job_registry().unwrap();
        let job: JobId = "transfer:remote-down-1".parse().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let lookup = loop {
            let lookup = registry.get(&job).unwrap().expect("the job exists");
            if format!("{lookup:?}").contains("Start this computer") {
                break lookup;
            }
            assert!(
                Instant::now() < deadline,
                "the job never failed: {lookup:?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert!(
            format!("{lookup:?}").contains("Start this computer to connect."),
            "{lookup:?}"
        );
        // Bad requests are refused as for a local transfer, before the owner is asked.
        for bad in [
            json!({"transferId": "bad id", "computerId": computer, "deviceId": device, "path": "/workspace/a"}),
            json!({"transferId": "t", "computerId": computer, "deviceId": device, "path": "/workspace/a", "destination": "relative"}),
        ] {
            assert_eq!(
                call(MethodName::FilesDownload, bad).await.unwrap_err().code,
                ErrorCode::Internal
            );
        }
        for bad in [
            json!({"transferId": "u", "computerId": computer, "deviceId": device, "directory": "/workspace", "conflict": "ask"}),
            json!({"transferId": "u", "computerId": computer, "deviceId": device, "directory": "/workspace", "conflict": "ask", "selection": "t"}),
        ] {
            assert_eq!(
                call(MethodName::FilesUpload, bad).await.unwrap_err().code,
                ErrorCode::Internal
            );
        }
    }

    #[tokio::test]
    async fn a_remote_watch_hands_the_owners_hints_to_the_callers_subscription() {
        let (device, computer) = ids(12);
        let (sender, frames_receiver) = mpsc::sync_channel(16);
        let slot = Mutex::new(Some((frames_receiver, sender.clone())));
        let opened: Arc<Mutex<Vec<Value>>> = Arc::default();
        let log = opened.clone();
        let _stream = stub_watch(&device, move |params| {
            log.lock().unwrap().push(params.clone());
            let (frames, _) = slot.lock().unwrap().take().expect("one stream");
            Ok(WatchLink {
                frames,
                _guard: Box::new(()),
            })
        });
        let client = silo_engine::context::ClientId::new("remote-watch-client");
        let mut events = fixture().context.connect_event_stream(client.clone());
        let context = HandlerContext::new(
            fixture().context.clone(),
            crate::dispatcher::RequestContext {
                client_id: "remote-watch-client".to_owned(),
                protocol: 6,
                client: None,
            },
        );
        let watch = crate::methods::dispatch(
            &context,
            MethodName::GuestFilesWatch,
            &json!({"computerId": computer, "deviceId": device, "paths": ["/workspace/inbox"]}),
        )
        .await
        .unwrap()
        .value;
        let subscription = watch["subscriptionId"].as_str().unwrap().to_owned();
        fixture()
            .context
            .activate_events(&client, &subscription)
            .unwrap();
        // The owner is asked to watch the folders of its own computer, without a device.
        assert_eq!(
            opened.lock().unwrap()[0],
            json!({"computerId": computer, "paths": ["/workspace/inbox"]})
        );
        sender.send(Ok(WatchFrame::Heartbeat)).unwrap();
        sender
            .send(Ok(WatchFrame::Changed("/workspace/inbox".into())))
            .unwrap();
        let delivery = tokio::time::timeout(Duration::from_secs(10), events.recv())
            .await
            .unwrap()
            .unwrap();
        let silo_engine::events::EventDelivery::Event(event) = delivery else {
            panic!("expected a change event");
        };
        assert_eq!(event.subscription, subscription);
        assert_eq!(event.event.name, "guest.files.changed");
        assert_eq!(event.event.payload["subscriptionId"], subscription.as_str());
        assert_eq!(event.event.payload["path"], "/workspace/inbox");
        // A stream that is lost ends the subscription, as a computer that stops does.
        drop(sender);
        let deadline = Instant::now() + Duration::from_secs(10);
        while fixture()
            .context
            .event_bus()
            .has_subscription(&client, &subscription)
        {
            assert!(Instant::now() < deadline, "the watch never ended");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn a_remote_watch_that_cannot_open_is_refused_and_leaves_no_subscription() {
        let (device, computer) = ids(13);
        let _stream = stub_watch(&device, |_| Err("Start this computer to watch it.".into()));
        let client = silo_engine::context::ClientId::new("remote-watch-refused");
        let _events = fixture().context.connect_event_stream(client.clone());
        let context = HandlerContext::new(
            fixture().context.clone(),
            crate::dispatcher::RequestContext {
                client_id: "remote-watch-refused".to_owned(),
                protocol: 6,
                client: None,
            },
        );
        let error = crate::methods::dispatch(
            &context,
            MethodName::GuestFilesWatch,
            &json!({"computerId": computer, "deviceId": device, "paths": ["/workspace"]}),
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("Start this computer"), "{error:?}");
        let empty = crate::methods::dispatch(
            &context,
            MethodName::GuestFilesWatch,
            &json!({"computerId": computer, "deviceId": device, "paths": []}),
        )
        .await
        .unwrap_err();
        assert_eq!(empty.code, ErrorCode::Internal);
    }
}
