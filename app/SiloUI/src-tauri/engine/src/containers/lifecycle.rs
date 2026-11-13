//! Starting, stopping and reaching one container computer (Linux hosts only).
//!
//! A running container is a tree under a transient user scope (`systemd-run --user --scope`,
//! delegated so crun can make the container's cgroup, with the computer's memory, task and CPU
//! limits as scope properties):
//!
//!     silo-ctr run          the holder: user + mount + network namespaces, the overlay root, the TAP
//!       crun run            the container runtime (same process, after exec)
//!         /silo/init        the container's first process: Landlock, signal forwarding, zombie reaping
//!       sleep infinity
//!
//! The holder hands the TAP to the engine, which gives it to one gateway process (CT1) and sends
//! back the address, route and resolver the gateway answered with. The gateway belongs to this
//! engine process, so a container does not outlive it: [`reconcile`] stops one whose gateway is
//! gone when the engine starts. Reading a computer's state ([`status`]) never changes it.

use super::{
    args::Exec,
    hold::{self, ExecGroup, Reaped, StartMark},
    layout::{ComputerDir, ComputerRecord, Layout},
    oci,
    tools::{run_checked, Tools},
    users, Material,
};
use crate::computer_gateway::{computer_ca, Gateway, Params, Secret};
use crate::gateway_control::Ready;
use crate::runtime::{RuntimeError, RuntimePaths};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::Ipv4Addr,
    os::{
        fd::{FromRawFd, OwnedFd},
        unix::{net::UnixListener, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const GUEST_IPV4: Ipv4Addr = Ipv4Addr::new(172, 30, 0, 6);
const GATEWAY_IPV4: Ipv4Addr = Ipv4Addr::new(172, 30, 0, 5);
const GUEST_MAC: [u8; 6] = [0x02, 0x53, 0x49, 0x4c, 0x4f, 0x01];
const TASKS_MAX: u32 = 4096;
const TERM_WAIT: Duration = Duration::from_secs(15);

/// A running container's gateway and what it holds.
pub struct Running {
    gateway: Mutex<Gateway>,
    /// The general secrets the gateway substitutes (their placeholders are in the guest's
    /// environment). It follows the gateway: a change the plan applies at once (a changed value,
    /// a removal) updates both together; a new assignment made while the computer runs is only
    /// recorded (`--next-start`) and joins at the next start, as for a virtual machine.
    active: Mutex<Vec<String>>,
    /// Where the active names are written for the processes of other commands (the SSH server
    /// runs apart from the engine and gives its sessions the same placeholders).
    published: Option<PathBuf>,
}

/// The file in a computer's state directory that lists its active secrets, one name per line.
const ACTIVE_SECRETS_FILE: &str = "secrets.active";

fn publish_active(file: &Option<PathBuf>, names: &[String]) {
    if let Some(file) = file {
        let _ = std::fs::write(file, names.join("\n"));
    }
}

/// The active secrets of a running computer as its engine published them; none when unknown.
pub fn published_secrets(dir: &ComputerDir) -> Vec<String> {
    std::fs::read_to_string(dir.state().join(ACTIVE_SECRETS_FILE))
        .map(|text| {
            text.lines()
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

impl Running {
    /// The gateway, for one exchange at a time. A poisoned lock is a gateway call that panicked:
    /// the link is unknown, so the gateway is stopped and every later call answers unavailable.
    pub fn gateway(&self) -> MutexGuard<'_, Gateway> {
        self.gateway.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn active_secrets(&self) -> Vec<String> {
        self.active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set_active(&self, names: Vec<String>) {
        publish_active(&self.published, &names);
        *self.active.lock().unwrap_or_else(PoisonError::into_inner) = names;
    }
}

/// The gateways of the running containers of this engine, by computer name. The map is locked
/// only to find or change an entry; a gateway exchange (up to seconds) takes the entry's own
/// lock, so reading one computer's state never waits for another's update.
static GATEWAYS: Mutex<Option<HashMap<String, Arc<Running>>>> = Mutex::new(None);

fn registry() -> MutexGuard<'static, Option<HashMap<String, Arc<Running>>>> {
    GATEWAYS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The gateway of a container this engine started, if it is registered.
pub fn running(name: &str) -> Option<Arc<Running>> {
    registry().as_ref().and_then(|map| map.get(name).cloned())
}

/// Registers `gateway` as the running gateway of `name`, as a start does.
#[cfg(test)]
pub fn register_for_test(name: &str, gateway: Gateway, active: Vec<String>) -> Arc<Running> {
    let running = Arc::new(Running {
        gateway: Mutex::new(gateway),
        active: Mutex::new(active),
        published: None,
    });
    registry()
        .get_or_insert_with(HashMap::new)
        .insert(name.to_owned(), running.clone());
    running
}

/// The general secrets the running gateway of `name` holds; none when it is not running.
pub fn active_secrets(name: &str) -> Vec<String> {
    running(name).map_or_else(Vec::new, |running| running.active_secrets())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Created,
    /// This engine is starting it: the holder may exist before the gateway does.
    Starting,
    Stopped,
    Running,
}

/// The scope's unit name: per runtime home, so Silo and Silo Dev never share one.
pub fn unit_name(paths: &RuntimePaths, name: &str) -> String {
    let digest = Sha256::digest(paths.home.to_string_lossy().as_bytes());
    let hash: String = digest
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("silo-ct-{hash}-{name}")
}

/// Points a command that talks to the account's systemd user manager (`systemd-run --user`,
/// `systemctl --user`) at this account's runtime folder. An engine started from another account's
/// session (a CI runner's, a `sudo` shell's) can carry that account's `XDG_RUNTIME_DIR`, and the
/// manager's socket is then refused to it ("Failed to connect to bus: Permission denied").
fn for_user_manager(command: &mut Command) {
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    for_user_manager_of(command, uid, std::env::var_os("XDG_RUNTIME_DIR").as_deref());
}

fn for_user_manager_of(command: &mut Command, uid: u32, current: Option<&std::ffi::OsStr>) {
    use std::os::unix::fs::MetadataExt;
    let owned =
        current.is_some_and(|dir| std::fs::metadata(dir).is_ok_and(|meta| meta.uid() == uid));
    if !owned {
        command
            .env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"))
            .env_remove("DBUS_SESSION_BUS_ADDRESS");
    }
}

/// The container's cgroup below the cgroup root: a leaf of its scope.
pub fn cgroup_path(uid: u32, unit: &str) -> String {
    format!("/user.slice/user-{uid}.slice/user@{uid}.service/app.slice/{unit}.scope/ctr")
}

fn io(error: std::io::Error) -> String {
    error.to_string()
}

fn holder_pid(state: &Path) -> Option<u32> {
    std::fs::read_to_string(state.join("holder.pid"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Whether `pid` is the live holder of the computer whose scope is `unit`: a `silo-ctr` process in
/// that scope's cgroup. A recycled pid, or another program that happens to be called `silo-ctr`,
/// is not in the scope.
fn is_holder(proc: &Path, pid: u32, unit: &str) -> bool {
    let named = std::fs::read(proc.join(pid.to_string()).join("cmdline")).is_ok_and(|bytes| {
        bytes
            .split(|b| *b == 0)
            .next()
            .is_some_and(|argv0| argv0.ends_with(b"silo-ctr"))
    });
    named
        && std::fs::read_to_string(proc.join(pid.to_string()).join("cgroup"))
            .is_ok_and(|text| hold::in_scope(&text, unit))
}

fn holder_alive_in(proc: &Path, paths: &RuntimePaths, name: &str) -> bool {
    let state = Layout::new(&paths.home).computer(name).state();
    holder_pid(&state).is_some_and(|pid| is_holder(proc, pid, &unit_name(paths, name)))
}

pub fn holder_alive(paths: &RuntimePaths, name: &str) -> bool {
    holder_alive_in(Path::new("/proc"), paths, name)
}

/// Whether this engine holds a live gateway for the computer. A gateway that stopped (a failed
/// update stops it, and a refused or late answer must never look applied) is not tracked, so the
/// computer reads as stopped and a start brings it back.
fn gateway_tracked(name: &str) -> bool {
    let Some(running) = running(name) else {
        return false;
    };
    let alive = match running.gateway.try_lock() {
        Ok(mut gateway) => gateway.is_running(),
        // An exchange is in progress: the gateway is there.
        Err(TryLockError::WouldBlock) => true,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner().is_running(),
    };
    alive
}

/// Whether the computer is running as this engine process knows it: a live holder and its gateway.
fn is_running(proc: &Path, paths: &RuntimePaths, name: &str) -> bool {
    holder_alive_in(proc, paths, name) && gateway_tracked(name)
}

/// What a computer is doing. Only reads: it never stops, starts or cleans up anything, so a list
/// refresh can never disturb a container (one being started has a holder before it has a
/// gateway). A holder with no gateway of this engine is not running; [`reconcile`] stops it when
/// the engine starts and [`start`] clears it.
pub fn status(paths: &RuntimePaths, name: &str, record: &ComputerRecord) -> Status {
    status_in(Path::new("/proc"), paths, name, record)
}

fn status_in(proc: &Path, paths: &RuntimePaths, name: &str, record: &ComputerRecord) -> Status {
    if hold::is_starting(name) {
        Status::Starting
    } else if is_running(proc, paths, name) {
        Status::Running
    } else if record.started_once {
        Status::Stopped
    } else {
        Status::Created
    }
}

/// Once, when the engine starts: stops the containers an earlier engine process left running.
/// Their gateways died with that process, so they have no network and nothing to manage them.
///
/// A container whose holder is alive and whose gateway this process does not hold counts as a
/// leftover. That is right only because `silod` is the one process that starts containers for an
/// account, and it runs once per channel and account (it holds that channel's `engine.lock`
/// before serving), so no other live engine can own a gateway for this runtime home.
pub fn reconcile(paths: &RuntimePaths, tools: &Tools) {
    let layout = Layout::new(&paths.home);
    for name in layout.names() {
        // Marked as starting while it is looked at, so a start that begins now is not mistaken for
        // a leftover (and a reconcile never races a start).
        let Some(_mark) = StartMark::begin(&name) else {
            continue;
        };
        if holder_alive(paths, &name) && !gateway_tracked(&name) {
            let Ok(record) = layout.computer(&name).read_record() else {
                continue;
            };
            // A virtual machine outlives the engine and is taken back by the next one; so is a
            // container, whose network device the namespace keeps. One that cannot be taken back
            // (its device is gone, a secret cannot be read) is stopped, as before.
            match adopt(paths, tools, &name, &record) {
                Ok(()) => {
                    eprintln!("Container computer {name}: taken back after an engine restart.")
                }
                Err(detail) => {
                    eprintln!(
                        "Container computer {name}: stopping a leftover of an earlier Silo engine ({detail})."
                    );
                    let _ = stop(paths, tools, &name, &record);
                }
            }
        }
    }
    prune_storage(paths, tools);
    collect_layers(paths, tools);
}

/// Takes back a running container whose gateway went with an earlier engine: the namespace kept
/// its network device, so `silo-ctr tap` hands it over again and a new gateway serves it. The
/// GitHub profile is not cached by a new engine; the engine's refresh applies it.
pub fn adopt(
    paths: &RuntimePaths,
    tools: &Tools,
    name: &str,
    record: &ComputerRecord,
) -> Result<(), String> {
    let layout = Layout::new(&paths.home);
    let dir = layout.computer(name);
    let state = dir.state();
    // A capture the earlier engine did not finish may have left the processes frozen; a computer
    // its user paused stays paused (the pause is recorded in its state folder).
    if !is_paused(paths, name) {
        let _ = thaw(paths, name);
    }
    let general = crate::secrets::runtime_material(name)?;
    let material = Material {
        secrets: &general,
        github_profile: crate::runtime::DISABLED_GITHUB_PROFILE,
    };
    let socket = state.join("adopt.sock");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).map_err(|e| at("bind", &socket, e))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| at("set up", &socket, e))?;
    let mut command = Command::new(&tools.ctr);
    command
        .arg("tap")
        .arg("--state")
        .arg(&state)
        .arg("--sock")
        .arg(&socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = Reaped::new(
        command
            .spawn()
            .map_err(|e| format!("silo-ctr could not start: {e}"))?,
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if matches!(child.child().try_wait(), Ok(Some(status)) if !status.success()) {
                    return Err("the network device could not be opened again".into());
                }
                if Instant::now() >= deadline {
                    let _ = child.child().kill();
                    return Err("the network device was not handed over in time".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(io(error)),
        }
    };
    stream.set_nonblocking(false).map_err(io)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(io)?;
    let tap = receive_descriptor(&stream).map_err(|e| format!("network device: {e}"))?;
    let id = record.label("silo.machine-id").unwrap_or(name).to_owned();
    let (ca_cert, ca_key) =
        computer_ca(&tools.gateway, &id).map_err(|e| format!("computer CA: {e}"))?;
    let (gateway, _, active) = start_gateway(
        paths, tools, name, record, &state, tap, &ca_cert, &ca_key, &material,
    )?;
    register(paths, name, &state, gateway, active)?;
    let _ = std::fs::remove_file(&socket);
    Ok(())
}

/// The name of the boot layer for the init shim `tools` names (and the ownership layout).
fn boot_hash(tools: &Tools) -> Result<(String, Vec<u8>), String> {
    let bytes = std::fs::read(&tools.init).map_err(|e| at("read the init shim", &tools.init, e))?;
    // The ownership layout is part of what the layer is: another one is another directory.
    let digest = Sha256::new()
        .chain_update(&bytes)
        .chain_update(super::layout::OWNERSHIP.as_bytes())
        .finalize();
    let hash: String = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((hash, bytes))
}

fn boot_layer(layout: &Layout, tools: &Tools) -> Result<PathBuf, String> {
    let (hash, bytes) = boot_hash(tools)?;
    let boot = layout.boot(&hash);
    if boot.join("silo/init").is_file() {
        return Ok(boot);
    }
    let partial = boot.with_extension("partial");
    // An earlier attempt's files may belong to root (a sub-id).
    let _ = super::image::remove_tree(tools, &partial);
    std::fs::create_dir_all(partial.join("silo")).map_err(|e| at("create", &partial, e))?;
    std::fs::create_dir_all(partial.join("workspace")).map_err(|e| at("create", &partial, e))?;
    let target = partial.join("silo/init");
    std::fs::write(&target, &bytes).map_err(|e| at("write", &target, e))?;
    // Every user of the computer reads these, whatever umask the engine runs with.
    use std::os::unix::fs::PermissionsExt;
    for directory in [&partial, &partial.join("silo"), &partial.join("workspace")] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| at("set the mode of", directory, e))?;
    }
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| at("set the mode of", &target, e))?;
    // Root's, not the working account's (which is the engine's own user): the account must not be
    // able to replace the init shim or its folder.
    own_as_root(tools, &[&partial.join("silo"), &partial.join("workspace")])?;
    match std::fs::rename(&partial, &boot) {
        Ok(()) => Ok(boot),
        Err(_) if boot.join("silo/init").is_file() => Ok(boot),
        Err(error) => Err(at("move the boot layer to", &boot, error)),
    }
}

/// Removes what the storage holds that no computer needs: unpacked images no computer's record
/// and no checkpoint names (the pinned image stays: the next computer starts from it), and, when
/// no container is running, boot layers of other init shims. A record or checkpoint that cannot
/// be read leaves everything in place. Best effort: what cannot be removed stays for the next
/// sweep.
pub fn prune_storage(paths: &RuntimePaths, tools: &Tools) {
    let layout = Layout::new(&paths.home);
    let Ok(pinned) = super::image::pinned() else {
        return;
    };
    let Ok(named) = super::layers::images_named(&layout) else {
        return;
    };
    let name_of = |digest: &str| {
        layout
            .image(digest)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let images: Vec<String> = std::iter::once(pinned.image_digest.as_str())
        .chain(named.iter().map(String::as_str))
        .map(name_of)
        .collect();
    let running = layout
        .names()
        .iter()
        .any(|name| holder_alive(paths, name) || hold::is_starting(name));
    for stale in super::image::unused_dirs(&layout.images(), &images) {
        let _ = super::image::remove_tree(tools, &stale);
    }
    if running {
        return;
    }
    if let Ok((hash, _)) = boot_hash(tools) {
        for stale in super::image::unused_dirs(&layout.root().join("boot"), &[hash]) {
            let _ = super::image::remove_tree(tools, &stale);
        }
    }
}

/// Gives `paths` (and what is below them) to the container's root, which is a sub-id on the host.
/// Runs in a user namespace laid out like the container's.
/// An I/O error with what was being done and where.
fn at(what: &str, path: &Path, error: std::io::Error) -> String {
    use std::os::unix::fs::MetadataExt;
    let owner = |path: Option<&Path>| {
        path.and_then(|path| std::fs::symlink_metadata(path).ok())
            .map_or_else(
                || "absent".to_owned(),
                |meta| format!("{}:{} {:o}", meta.uid(), meta.gid(), meta.mode() & 0o7777),
            )
    };
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    format!(
        "Could not {what} {}: {error} (it is {}, its folder {}; this process is user {me})",
        path.display(),
        owner(Some(path)),
        owner(path.parent())
    )
}

fn own_as_root(tools: &Tools, paths: &[&Path]) -> Result<(), String> {
    let agent = super::idmap::AGENT_ID.to_string();
    let mut args: Vec<&std::ffi::OsStr> = vec![
        "userns".as_ref(),
        "--keep-id".as_ref(),
        agent.as_ref(),
        "--".as_ref(),
        "chown".as_ref(),
        "-R".as_ref(),
        "0:0".as_ref(),
        "--".as_ref(),
    ];
    args.extend(paths.iter().map(|path| path.as_os_str()));
    run_checked(&tools.ctr, &args).map(drop)
}

/// Whether the computer's upper directory, when it has one, was made with the id mapping now in
/// force: the root of the container's file system is root's (a sub-id), and a directory an earlier
/// build left owned by the engine's own user cannot be used with it.
pub fn check_upper(dir: &ComputerDir, record: &ComputerRecord, name: &str) -> Result<bool, String> {
    let sub_ids = super::idmap::current_sub_ids().ok_or(
        "Silo could not read this account's sub-uid range in /etc/subuid. Run silo host init to see what container computers need.",
    )?;
    check_upper_in(&dir.upper_of(record), name, &sub_ids)
}

fn check_upper_in(
    upper: &Path,
    name: &str,
    sub_ids: &std::ops::Range<u32>,
) -> Result<bool, String> {
    use std::os::unix::fs::MetadataExt;
    match std::fs::symlink_metadata(upper) {
        Ok(meta)
            if meta.is_dir()
                && super::idmap::KeepId::current().upper_owner_is_current(meta.uid(), sub_ids) =>
        {
            Ok(true)
        }
        Ok(_) => Err(super::idmap::recreate_message(name)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io(error)),
    }
}

/// The layer store (`containers/layers`) and the folder holding it are the engine account's
/// alone: a layer's files carry the owners and modes of a file system somebody else may have made
/// (an import), and with the account's own id kept in the container's namespace a setuid file
/// there is setuid to this account on the host, so no other account may reach it. Set each time a
/// layer is made, whatever the umask or a default ACL gave.
pub fn seal_store(layers: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let holder = layers.parent().filter(|_| {
        layers.file_name().is_some_and(|name| name == "layers")
            && layers
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "containers")
    });
    for dir in std::iter::once(layers).chain(holder) {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| at("set the mode of", dir, e))?;
    }
    Ok(())
}

/// Makes the empty layer `path`: aside, given to the container's root, then renamed into place
/// (within one folder, so no directory of a sub-id moves), so an interrupted start never leaves a
/// layer that looks like an earlier build's.
pub fn make_layer(tools: &Tools, path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let parent = path.parent().ok_or("A layer has no folder.")?;
    std::fs::create_dir_all(parent).map_err(|e| at("create", parent, e))?;
    super::layout::strip_default_acl(parent);
    seal_store(parent)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("layer");
    let staging = parent.join(format!(".new-{name}"));
    super::image::remove_tree(tools, &staging)?;
    std::fs::create_dir(&staging).map_err(|e| at("create", &staging, e))?;
    // The overlay's root directory takes the upper directory's mode: it must stay traversable.
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| at("set the mode of", &staging, e))?;
    own_as_root(tools, &[&staging])?;
    std::fs::rename(&staging, path).map_err(|e| at("move it to", path, e))
}

/// Makes the computer's upper directory, or checks the one it has.
fn prepare_upper(
    tools: &Tools,
    dir: &ComputerDir,
    record: &ComputerRecord,
    name: &str,
) -> Result<(), String> {
    match check_upper(dir, record, name)? {
        true => Ok(()),
        false => make_layer(tools, &dir.upper_of(record)),
    }
}

/// The container's control group, below the cgroup mount.
fn cgroup_dir(paths: &RuntimePaths, name: &str) -> PathBuf {
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    Path::new("/sys/fs/cgroup")
        .join(cgroup_path(uid, &unit_name(paths, name)).trim_start_matches('/'))
}

/// The file in a computer's state folder that says its user paused it. A checkpoint freezes the
/// processes too, for the copy only; the two are told apart by this file, so a checkpoint of a paused
/// computer leaves it paused, `inspect` says Paused, and an engine that takes the computer back
/// after a restart does not thaw it. A stop or a start removes it.
const PAUSED_FILE: &str = "paused";

fn paused_file(paths: &RuntimePaths, name: &str) -> PathBuf {
    Layout::new(&paths.home)
        .computer(name)
        .state()
        .join(PAUSED_FILE)
}

/// Whether the computer's user paused it (and it has not been resumed, stopped or started since).
pub fn is_paused(paths: &RuntimePaths, name: &str) -> bool {
    paused_file(paths, name).exists()
}

/// Held while a computer is frozen or thawed, so a user's resume cannot thaw the processes in the
/// middle of a checkpoint's copy (and a pause cannot be undone by the checkpoint ending).
static FREEZING: Mutex<()> = Mutex::new(());

fn freezing() -> MutexGuard<'static, ()> {
    FREEZING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A container whose processes are frozen for a copy; they are thawed when this is dropped, unless
/// their user paused the computer.
pub struct Frozen {
    file: PathBuf,
    paused: PathBuf,
    _freezing: MutexGuard<'static, ()>,
}

impl Drop for Frozen {
    fn drop(&mut self) {
        if !self.paused.exists() {
            let _ = std::fs::write(&self.file, "0");
        }
    }
}

/// Freezes the container's control group and waits until the kernel reports it frozen, so a copy
/// of its files sees none of them change.
pub fn freeze(paths: &RuntimePaths, name: &str) -> Result<Frozen, String> {
    freeze_holding(paths, name, freezing())
}

fn freeze_holding(
    paths: &RuntimePaths,
    name: &str,
    guard: MutexGuard<'static, ()>,
) -> Result<Frozen, String> {
    let directory = cgroup_dir(paths, name);
    let frozen = Frozen {
        file: directory.join("cgroup.freeze"),
        paused: paused_file(paths, name),
        _freezing: guard,
    };
    std::fs::write(&frozen.file, "1").map_err(|e| at("freeze", &frozen.file, e))?;
    let events = directory.join("cgroup.events");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if std::fs::read_to_string(&events).is_ok_and(|text| text.lines().any(|l| l == "frozen 1"))
        {
            return Ok(frozen);
        }
        if Instant::now() >= deadline {
            return Err(format!("Container computer {name} did not freeze in time."));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The user's pause: records it, then freezes the processes (they stay frozen until `resume`).
pub fn pause(paths: &RuntimePaths, name: &str) -> Result<(), String> {
    let guard = freezing();
    let marker = paused_file(paths, name);
    std::fs::write(&marker, "").map_err(|e| at("record the pause of", &marker, e))?;
    match freeze_holding(paths, name, guard) {
        // Dropped with the marker in place: the processes stay frozen.
        Ok(frozen) => {
            drop(frozen);
            Ok(())
        }
        Err(detail) => {
            let _ = std::fs::remove_file(&marker);
            Err(detail)
        }
    }
}

/// The user's resume.
pub fn resume(paths: &RuntimePaths, name: &str) -> Result<(), String> {
    let _freezing = freezing();
    let _ = std::fs::remove_file(paused_file(paths, name));
    thaw(paths, name)
}

/// Ends a pause the way [`resume`] does, for [`stop`]: under the freeze lock, the marker goes and
/// the processes thaw. A frozen process cannot run to answer a signal, so a stop that signalled
/// first would wait out TERM and end in SIGKILL. The thaw may fail (a cgroup already gone); the
/// stop proceeds anyway, as does the marker's removal in the stale-state cleanup.
fn end_pause(paths: &RuntimePaths, name: &str) {
    let _freezing = freezing();
    let _ = std::fs::remove_file(paused_file(paths, name));
    let _ = thaw(paths, name);
}

/// Thaws the container (a no-op when it is not frozen).
fn thaw(paths: &RuntimePaths, name: &str) -> Result<(), String> {
    let file = cgroup_dir(paths, name).join("cgroup.freeze");
    std::fs::write(&file, "0").map_err(|e| at("thaw", &file, e))
}

fn in_container_namespace(tools: &Tools, command: &[&std::ffi::OsStr]) -> Result<(), String> {
    let agent = super::idmap::AGENT_ID.to_string();
    run_checked(&tools.ctr, &namespace_args(&agent, command)).map(drop)
}

/// Why a bounded copy of a layer did not finish.
#[derive(Debug, PartialEq, Eq)]
pub enum CopyFailure {
    /// The copy was still running at the limit and was killed.
    TooSlow,
    Failed(String),
}

/// The arguments that run `command` as the container's owner in its user namespace.
fn namespace_args<'a>(agent: &'a str, command: &[&'a std::ffi::OsStr]) -> Vec<&'a std::ffi::OsStr> {
    let mut args: Vec<&std::ffi::OsStr> = vec![
        "userns".as_ref(),
        "--keep-id".as_ref(),
        agent.as_ref(),
        "--".as_ref(),
    ];
    args.extend_from_slice(command);
    args
}

/// Copies the files of the layer `from` into the empty layer `to`, owners and attributes kept, as
/// a reflink where the file system has them. It is killed when it takes longer than `limit`: a
/// running computer's processes are frozen while it copies.
pub fn copy_layer(
    tools: &Tools,
    from: &Path,
    to: &Path,
    limit: Duration,
) -> Result<(), CopyFailure> {
    let (mut source, mut target) = (from.as_os_str().to_owned(), to.as_os_str().to_owned());
    source.push("/.");
    target.push("/");
    let agent = super::idmap::AGENT_ID.to_string();
    let copy: [&std::ffi::OsStr; 6] = [
        "cp".as_ref(),
        "-a".as_ref(),
        "--reflink=auto".as_ref(),
        "--".as_ref(),
        source.as_ref(),
        target.as_ref(),
    ];
    match super::tools::run_bounded(&tools.ctr, &namespace_args(&agent, &copy), limit) {
        Ok(done) if done.success => Ok(()),
        Ok(done) => Err(CopyFailure::Failed(format!(
            "cp failed: {}",
            done.last_error
        ))),
        Err(super::tools::BoundedError::TimedOut) => Err(CopyFailure::TooSlow),
        Err(super::tools::BoundedError::CouldNotStart(detail)) => Err(CopyFailure::Failed(detail)),
    }
}

/// The bytes a writable layer occupies on disk, counted inside the user namespace so directories
/// the engine's own account cannot read count too. `Some(None)` when the layer could not be
/// measured (the measurement ran too long, or `du` ran and printed nothing readable); `None` when
/// the tools around `du` failed (`silo-ctr`, the id mapping), which says nothing about the layer.
pub fn layer_bytes(tools: &Tools, layer: &Path) -> Option<Option<u64>> {
    let agent = super::idmap::AGENT_ID.to_string();
    let measure: [&std::ffi::OsStr; 5] = [
        "sh".as_ref(),
        "-c".as_ref(),
        super::host_disk::DU_SCRIPT.as_ref(),
        "sh".as_ref(),
        layer.as_os_str(),
    ];
    match super::tools::run_bounded(
        &tools.ctr,
        &namespace_args(&agent, &measure),
        Duration::from_secs(300),
    ) {
        Err(super::tools::BoundedError::CouldNotStart(detail)) => {
            eprintln!(
                "Container disk guard: measuring {}: {detail}",
                layer.display()
            );
            None
        }
        Err(super::tools::BoundedError::TimedOut) => Some(None),
        Ok(done) => {
            let result = super::host_disk::du_result(&done.stdout);
            if result.is_none() {
                eprintln!(
                    "Container disk guard: measuring {} did not run `du`: {}",
                    layer.display(),
                    done.last_error
                );
            }
            result
        }
    }
}

/// Folds `layers` (oldest first) into the new layer `out`.
pub fn merge_layers(tools: &Tools, out: &Path, layers: &[PathBuf]) -> Result<(), String> {
    make_layer(tools, out)?;
    let mut command: Vec<&std::ffi::OsStr> = vec![
        tools.ctr.as_os_str(),
        "merge".as_ref(),
        "--out".as_ref(),
        out.as_os_str(),
    ];
    command.extend(layers.iter().map(|layer| layer.as_os_str()));
    in_container_namespace(tools, &command)
}

/// Folds the oldest sealed layers of a stopped computer whose chain is too long into one. Nothing
/// is locked unless there is something to fold, and the fold is made before the store is locked:
/// the new layer is reserved, so collection leaves it alone, and the lock is held only to put it
/// into the record.
pub fn flatten(paths: &RuntimePaths, tools: &Tools, name: &str) -> Result<(), String> {
    let layout = Layout::new(&paths.home);
    let dir = layout.computer(name);
    let record = dir.read_record()?;
    let count = super::layers::flatten_count(&record.layers);
    if count == 0 {
        return Ok(());
    }
    let merged = super::layers::new_id();
    let _reservation = super::layers::reserve(&merged);
    let sources: Vec<PathBuf> = record.layers[..count]
        .iter()
        .map(|id| layout.layer(id))
        .collect();
    if let Err(detail) = merge_layers(tools, &layout.layer(&merged), &sources) {
        let _ = super::image::remove_tree(tools, &layout.layer(&merged));
        return Err(detail);
    }
    let _store = super::layers::lock();
    dir.update_record(|record| {
        record.layers = super::layers::flattened(&record.layers, count, &merged);
    })
    .map(drop)
}

/// Removes the layers nothing names (see [`super::layers`]). Best effort: a layer that cannot go
/// stays for the next pass.
pub fn collect_layers(paths: &RuntimePaths, tools: &Tools) {
    let _store = super::layers::lock();
    for layer in super::layers::collectible(&Layout::new(&paths.home)) {
        let _ = super::image::remove_tree(tools, &layer);
    }
}

/// One look of the host disk guard (see [`super::host_disk`]): below the margin of free space it
/// stops the running computers whose layer is over their stated size, largest first, and leaves a
/// message in each one's state folder. It returns the names it stopped.
pub fn guard_pass(paths: &RuntimePaths, tools: &Tools) -> Vec<String> {
    let layout = Layout::new(&paths.home);
    let Ok(free) = crate::transfer_area::free_space(&paths.home) else {
        return Vec::new();
    };
    let margin = super::host_disk::margin_bytes();
    if free >= margin {
        return Vec::new();
    }
    let running: Vec<String> = layout
        .names()
        .into_iter()
        .filter(|name| {
            layout
                .computer(name)
                .read_record()
                .is_ok_and(|record| status(paths, name, &record) == Status::Running)
        })
        .collect();
    super::host_disk::pass(
        free,
        margin,
        running,
        |name| {
            let dir = layout.computer(name);
            let record = dir.read_record().ok()?;
            Some(super::host_disk::Sizes {
                layer: layer_bytes(tools, &dir.upper_of(&record))?,
                limit: super::host_disk::limit_bytes(&record),
            })
        },
        |name, sizes| {
            let dir = layout.computer(name);
            let Ok(record) = dir.read_record() else {
                return false;
            };
            let notice = dir.state().join(super::host_disk::NOTICE);
            let text = super::host_disk::stop_notice(free, margin, sizes);
            let _ = write_readable(&notice, &text);
            eprintln!("Container computer {name}: {text}");
            match stop(paths, tools, name, &record) {
                Ok(()) => true,
                Err(detail) => {
                    // It is still running, so the message would be false.
                    let _ = std::fs::remove_file(&notice);
                    eprintln!("Container computer {name}: it could not be stopped: {detail:?}");
                    false
                }
            }
        },
    )
}

/// The message the host disk guard left when it stopped the computer, if it did and nothing has
/// started the computer since.
pub fn guard_notice(paths: &RuntimePaths, name: &str) -> Option<String> {
    std::fs::read_to_string(
        Layout::new(&paths.home)
            .computer(name)
            .state()
            .join(super::host_disk::NOTICE),
    )
    .ok()
    .map(|text| text.trim().to_owned())
    .filter(|text| !text.is_empty())
}

/// Starts the host disk guard once for this engine process. It holds nothing about any computer
/// between looks: each look reads the records and the free space again.
pub fn spawn_guard(paths: &RuntimePaths) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    let paths = paths.clone();
    STARTED.call_once(move || {
        let _ = std::thread::Builder::new()
            .name("silo-disk-guard".into())
            .spawn(move || loop {
                if let Ok(tools) = Tools::locate(&paths) {
                    guard_pass(&paths, &tools);
                }
                let low = crate::transfer_area::free_space(&paths.home)
                    .is_ok_and(|free| free < super::host_disk::margin_bytes());
                std::thread::sleep(if low {
                    super::host_disk::CHECK_EVERY_WHEN_LOW
                } else {
                    super::host_disk::CHECK_EVERY
                });
            });
    });
}

/// Creates what the container binds as its per-computer share (`/.msb`): the computer's runtime
/// folder. Only the account that owns the engine reaches it from the host, and the container's
/// other users only get to search it, as in a virtual machine.
fn share_directory(home: &Path, name: &str) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    let computer = home.join("sandboxes").join(name);
    private_dir(&computer).map_err(|e| format!("create {}: {e}", computer.display()))?;
    std::fs::set_permissions(&computer, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| at("set the mode of", &computer, e))?;
    let runtime = computer.join("runtime");
    std::fs::create_dir_all(&runtime).map_err(|e| at("create", &runtime, e))?;
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o711))
        .map_err(|e| at("set the mode of", &runtime, e))?;
    Ok(runtime)
}

/// Writes a file the container's users read (it is bound into the container), whatever umask the
/// engine runs with.
fn write_readable(path: &Path, contents: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, contents).map_err(|e| at("write", path, e))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))
        .map_err(|e| at("set the mode of", path, e))
}

pub(super) fn private_dir(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|e| at("create", path, e))
}

/// The file the gateway appends its credential-use events to: where `computers.credentials`
/// reads a Linux computer's events, so the credentials-used line needs no container case.
fn usage_log_file(home: &Path, name: &str) -> Result<PathBuf, String> {
    let file = crate::credentials_usage::linux_usage_file(home, name);
    if let Some(parent) = file.parent() {
        private_dir(parent)?;
    }
    Ok(file)
}

/// `recvmsg` of one file descriptor sent with `SCM_RIGHTS`.
pub(super) fn receive_descriptor(
    stream: &std::os::unix::net::UnixStream,
) -> Result<OwnedFd, String> {
    use std::os::fd::AsRawFd;
    let mut data = [0u8; 8];
    let mut iov = libc::iovec {
        iov_base: data.as_mut_ptr().cast(),
        iov_len: data.len(),
    };
    let mut control = [0u64; 8];
    // SAFETY: a zeroed msghdr is valid; the pointers refer to buffers that outlive the call.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = std::mem::size_of_val(&control) as _;
    // SAFETY: `message` points at valid buffers for the whole call.
    let received =
        unsafe { libc::recvmsg(stream.as_raw_fd(), &mut message, libc::MSG_CMSG_CLOEXEC) };
    if received <= 0 {
        return Err(
            "the container holder closed the link before sending its network device".into(),
        );
    }
    // SAFETY: the kernel filled `message`'s control buffer; CMSG_* only read within it.
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        if header.is_null()
            || (*header).cmsg_level != libc::SOL_SOCKET
            || (*header).cmsg_type != libc::SCM_RIGHTS
        {
            return Err("the container holder sent no network device".into());
        }
        let descriptor = std::ptr::read_unaligned(libc::CMSG_DATA(header).cast::<i32>());
        Ok(OwnedFd::from_raw_fd(descriptor))
    }
}

struct Started {
    /// What the holder warned about while it started (`silo-ctr: warning: ...`).
    warnings: Vec<String>,
}

pub fn start(
    paths: &RuntimePaths,
    tools: &Tools,
    name: &str,
    timeout: Duration,
    material: &Material<'_>,
) -> Result<(), RuntimeError> {
    let unavailable = |detail: String| {
        eprintln!("Starting container computer {name}: {detail}");
        RuntimeError::Unavailable(format!(
            "Silo could not start container computer {name}. {detail}"
        ))
    };
    // Marked first: from here until the end nothing may take this computer's holder, which exists
    // before its gateway does, for a leftover.
    let _mark = StartMark::begin(name).ok_or_else(|| {
        RuntimeError::Unavailable(format!("Container computer {name} is already starting."))
    })?;
    let layout = Layout::new(&paths.home);
    let dir = layout.computer(name);
    if is_running(Path::new("/proc"), paths, name) {
        return Ok(());
    }
    // A host short of disk space starts nothing (see `host_disk`).
    super::host_disk::require_room(&paths.home, "start a container computer")?;
    // The reason an earlier stop left in the status is over once the computer starts again.
    let _ = std::fs::remove_file(dir.state().join(super::host_disk::NOTICE));
    // A chain of sealed layers that has grown long is folded before it is mounted.
    flatten(paths, tools, name).map_err(&unavailable)?;
    let record = dir.read_record().map_err(RuntimeError::Unavailable)?;
    // Before any image is unpacked for it.
    check_upper(&dir, &record, name).map_err(RuntimeError::Unavailable)?;
    // The image the computer was made from, not whatever is pinned now.
    let image = super::image::ensure_recorded(paths, tools, name, &record)?;
    cleanup_stale(paths, tools, &dir, name);
    let started = launch(
        paths,
        tools,
        &layout,
        &dir,
        name,
        &record,
        &image.rootfs,
        timeout,
        material,
    )
    .map_err(|detail| {
        let _ = stop(paths, tools, name, &record);
        unavailable(detail)
    })?;
    for warning in &started.warnings {
        eprintln!("Container computer {name}: {warning}");
    }
    // Re-read: a secret may have been assigned while the computer was starting.
    dir.update_record(|record| record.started_once = true)
        .map_err(RuntimeError::Unavailable)?;
    Ok(())
}

fn launch(
    paths: &RuntimePaths,
    tools: &Tools,
    layout: &Layout,
    dir: &ComputerDir,
    name: &str,
    record: &ComputerRecord,
    rootfs: &Path,
    timeout: Duration,
    material: &Material<'_>,
) -> Result<Started, String> {
    let state = dir.state();
    private_dir(&state)?;
    prepare_upper(tools, dir, record, name)?;
    let share = share_directory(&paths.home, name)?;
    private_dir(&dir.work())?;
    private_dir(&dir.merged())?;
    private_dir(&dir.bundle())?;
    let boot = boot_layer(layout, tools)?;
    let uid = unsafe { libc::getuid() };
    let unit = unit_name(paths, name);
    // The computer's certificate authority is bound into the guest as one local certificate and
    // joins the guest's trust bundle when Silo runs `update-ca-certificates` after the start (see
    // `containers::install_gateway_ca`), as the runtime does for a virtual machine, so a guest
    // accepts the certificate the gateway presents for a host a secret is allowed for. The
    // bundle itself stays the guest's own file: nothing is bound over it, and nothing is read
    // from the image here.
    let id = record.label("silo.machine-id").unwrap_or(name).to_owned();
    let (ca_cert, ca_key) =
        computer_ca(&tools.gateway, &id).map_err(|e| format!("computer CA: {e}"))?;
    write_readable(
        &state.join("gateway-ca.crt"),
        &format!("{}\n", ca_cert.trim_end()),
    )?;
    let spec = oci::build(&oci::Params {
        name,
        rootfs: &dir.merged(),
        cgroup_path: &cgroup_path(uid, &unit),
        resolv_conf: &state.join("resolv.conf"),
        hosts: &state.join("hosts"),
        ca_cert: &state.join("gateway-ca.crt"),
        share: &share,
        arch: std::env::consts::ARCH,
    })?;
    std::fs::write(
        dir.bundle().join("config.json"),
        serde_json::to_vec_pretty(&spec).map_err(|e| e.to_string())?,
    )
    .map_err(|e| at("write", &dir.bundle().join("config.json"), e))?;
    write_readable(
        &state.join("hosts"),
        &format!("127.0.0.1 localhost\n127.0.1.1 {name}\n::1 localhost\n"),
    )?;
    write_readable(
        &state.join("resolv.conf"),
        &format!("nameserver {GATEWAY_IPV4}\n"),
    )?;

    let socket = state.join("ctl.sock");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).map_err(|e| at("bind", &socket, e))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| at("set up", &socket, e))?;

    let lower_dirs = dir.lower_dirs(record, &boot, rootfs);
    if super::layers::lower_option_bytes(&lower_dirs) > super::layers::MAX_LOWER_BYTES {
        return Err("This computer's layers do not fit one mount. Delete some checkpoints and start it again.".into());
    }
    let lower = lower_dirs
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(":");
    // The holder's output is the container's standard output and error too, and the container
    // runtime gives those files to the user the container's first process runs as: root, a
    // sub-id the engine cannot write as. A new file each start, open to its owner and to others
    // (inside the engine's private state folder), stays usable by the engine and the holder
    // after that change.
    let holder_log = state.join("holder.log");
    let _ = std::fs::remove_file(&holder_log);
    let log = {
        use std::os::unix::fs::PermissionsExt;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&holder_log)
            .map_err(|e| at("open", &holder_log, e))?;
        log.set_permissions(std::fs::Permissions::from_mode(0o666))
            .map_err(|e| at("set the mode of", &holder_log, e))?;
        log
    };
    let mut command = Command::new(&tools.systemd_run);
    for_user_manager(&mut command);
    command
        .args(["--user", "--scope", "--quiet", "--collect"])
        .arg(format!("--unit={unit}"))
        .args(["-p", "Delegate=yes"])
        .arg("-p")
        .arg(format!("MemoryMax={}M", record.memory_mib))
        .args(["-p", "MemorySwapMax=0"])
        .arg("-p")
        .arg(format!("TasksMax={TASKS_MAX}"))
        .arg("-p")
        .arg(format!("CPUQuota={}%", record.cpus * 100))
        .arg("--")
        .arg(&tools.ctr)
        .arg("run")
        .arg("--state")
        .arg(&state)
        .arg("--lower")
        .arg(&lower)
        .arg("--upper")
        .arg(dir.upper_of(record))
        .arg("--work")
        .arg(dir.work())
        .arg("--merged")
        .arg(dir.merged())
        .arg("--bundle")
        .arg(dir.bundle())
        .arg("--id")
        .arg(name)
        .arg("--sock")
        .arg(&socket)
        .arg("--crun")
        .arg(&tools.crun)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(io)?)
        .stderr(log);
    // SAFETY: setsid is async-signal-safe; the holder must not die with the engine's session.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    // Every way out below reaps the child: it is waited for even when the start fails.
    let mut child = Reaped::new(
        command
            .spawn()
            .map_err(|e| format!("systemd-run could not start: {e}"))?,
    );
    let deadline = Instant::now() + timeout.min(Duration::from_secs(120));
    let tail = |state: &Path| {
        std::fs::read_to_string(state.join("holder.log"))
            .ok()
            .and_then(|text| {
                text.lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .map(str::to_owned)
            })
            .unwrap_or_default()
    };
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if let Ok(Some(status)) = child.child().try_wait() {
                    return Err(format!(
                        "The container holder exited ({status}): {}",
                        tail(&state)
                    ));
                }
                if Instant::now() >= deadline {
                    let _ = child.child().kill();
                    return Err("The container holder did not start in time.".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(io(error)),
        }
    };
    stream.set_nonblocking(false).map_err(io)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(io)?;
    let tap = receive_descriptor(&stream).map_err(|e| format!("network device: {e}"))?;
    let (gateway, ready, active) = start_gateway(
        paths, tools, name, record, &state, tap, &ca_cert, &ca_key, material,
    )?;
    let mac = ready
        .guest_mac
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":");
    let reply = format!(
        "mac={mac}\nmtu={}\naddress={}\nprefix_len={}\ngateway={}\n\n",
        ready.mtu, ready.guest_ipv4, ready.prefix_len, ready.gateway_ipv4
    );
    (&stream).write_all(reply.as_bytes()).map_err(io)?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).map_err(io)?;
    if line.trim() != "up" {
        return Err(format!(
            "The container network did not come up: {}",
            tail(&state)
        ));
    }
    let running = register(paths, name, &state, gateway, active)?;
    wait_running(&tools.crun, &state, name, deadline)?;
    holder_pid(&state).ok_or("The container holder left no pid file.")?;
    // The holder runs on; dropping the guard hands its exit status to a thread.
    let log = std::fs::read_to_string(state.join("holder.log")).unwrap_or_default();
    Ok(Started {
        warnings: hold::warnings_since(&log, 0),
    })
}

/// Starts the gateway on the container's network device with the secrets this computer is assigned
/// (the values the caller read for this start), and writes the resolver file the container reads.
#[allow(clippy::too_many_arguments)]
fn start_gateway(
    paths: &RuntimePaths,
    tools: &Tools,
    name: &str,
    record: &ComputerRecord,
    state: &Path,
    tap: OwnedFd,
    ca_cert: &str,
    ca_key: &str,
    material: &Material<'_>,
) -> Result<(Gateway, Ready, Vec<String>), String> {
    let gateway_log = std::fs::File::create(state.join("gateway.log"))
        .map_err(|e| at("create", &state.join("gateway.log"), e))?;
    let usage_log = usage_log_file(&paths.home, name).map_err(|e| format!("usage log: {e}"))?;
    // The secrets this computer is assigned, with the values the engine read for this start. A
    // value the engine does not hold is left out: nothing substitutes it.
    let secrets: Vec<Secret> = record
        .secrets
        .iter()
        .filter_map(|known| {
            material
                .secrets
                .iter()
                .find(|(secret, _, _)| secret == &known.name)
        })
        .map(|(name, value, hosts)| Secret {
            name: name.clone(),
            value: Zeroizing::new(value.clone()),
            allowed_hosts: hosts.clone(),
        })
        .collect();
    let active: Vec<String> = secrets.iter().map(|secret| secret.name.clone()).collect();
    let (gateway_policy, gateway_strict) = super::secrets::gateway_network(name, record);
    let gateway = Gateway::start(
        &tools.gateway,
        tap,
        ca_cert,
        ca_key,
        Params {
            slot: 1,
            guest_ipv4: GUEST_IPV4,
            gateway_ipv4: GATEWAY_IPV4,
            guest_mac: GUEST_MAC,
            mtu: 1500,
            published_ports: Vec::new(),
            secrets,
            github_profile_json: record
                .github
                .then(|| Zeroizing::new(material.github_profile.to_owned())),
            github_hosts: record
                .github
                .then(|| super::secrets::GITHUB_HOSTS.map(str::to_owned).to_vec()),
            policy: gateway_policy.clone(),
            strict: gateway_strict,
            usage_log: Some(usage_log),
            log: Some(gateway_log),
        },
    )
    .map_err(|e| format!("gateway: {e}"))?;
    let ready = gateway.ready().clone();
    write_readable(
        &state.join("resolv.conf"),
        &format!("nameserver {}\n", ready.dns_ipv4),
    )?;
    Ok((gateway, ready, active))
}

/// Records the running gateway and serves the computer's published ports.
fn register(
    paths: &RuntimePaths,
    name: &str,
    state: &Path,
    gateway: Gateway,
    active: Vec<String>,
) -> Result<Arc<Running>, String> {
    let running = Arc::new(Running {
        gateway: Mutex::new(gateway),
        active: Mutex::new(active.clone()),
        published: Some(state.join(ACTIVE_SECRETS_FILE)),
    });
    publish_active(&running.published, &active);
    registry()
        .get_or_insert_with(HashMap::new)
        .insert(name.to_owned(), running.clone());
    super::ports::serve(paths, name, &running)?;
    Ok(running)
}

fn crun(tools: &Tools, state: &Path) -> Command {
    let mut command = Command::new(&tools.crun);
    command
        .arg("--root")
        .arg(state.join("crun"))
        .arg("--cgroup-manager=cgroupfs");
    command
}

fn wait_running(
    crun_path: &Path,
    state: &Path,
    name: &str,
    deadline: Instant,
) -> Result<(), String> {
    loop {
        let output = Command::new(crun_path)
            .arg("--root")
            .arg(state.join("crun"))
            .args(["state", name])
            .output();
        if let Ok(output) = output {
            if String::from_utf8_lossy(&output.stdout).contains("\"running\"") {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            let log = std::fs::read_to_string(state.join("holder.log")).unwrap_or_default();
            let last = log
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("");
            return Err(format!(
                "The container did not reach the running state. {last}"
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn systemctl(tools: &Tools, args: &[&str]) {
    let mut command = Command::new(&tools.systemctl);
    for_user_manager(&mut command);
    let _ = command
        .arg("--user")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// The files of a computer's state folder that belong to one run: a stop or a start removes them
/// (a pause with them).
const STALE_STATE_FILES: [&str; 5] = [
    "ctl.sock",
    "holder.pid",
    "outer.pid",
    "container.pid",
    PAUSED_FILE,
];

/// What an earlier run left: a scope, crun state, a socket.
fn cleanup_stale(paths: &RuntimePaths, tools: &Tools, dir: &ComputerDir, name: &str) {
    let state = dir.state();
    if holder_alive(paths, name) {
        systemctl(
            tools,
            &[
                "kill",
                "--signal=SIGKILL",
                &format!("{}.scope", unit_name(paths, name)),
            ],
        );
    }
    let _ = crun(tools, &state)
        .args(["delete", "--force", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    for stale in STALE_STATE_FILES {
        let _ = std::fs::remove_file(state.join(stale));
    }
}

pub fn stop(
    paths: &RuntimePaths,
    tools: &Tools,
    name: &str,
    record: &ComputerRecord,
) -> Result<(), RuntimeError> {
    let layout = Layout::new(&paths.home);
    let dir = layout.computer(name);
    let state = dir.state();
    // A paused computer's processes are frozen; end the pause first, so TERM is answered like
    // any other stop's instead of ending in SIGKILL after the wait.
    end_pause(paths, name);
    if holder_alive(paths, name) {
        let _ = crun(tools, &state)
            .args(["kill", name, "TERM"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let deadline = Instant::now() + TERM_WAIT;
        while holder_alive(paths, name) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        if holder_alive(paths, name) {
            let unit = unit_name(paths, name);
            systemctl(
                tools,
                &["kill", "--signal=SIGKILL", &format!("{unit}.scope")],
            );
            let deadline = Instant::now() + Duration::from_secs(10);
            while holder_alive(paths, name) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        if holder_alive(paths, name) {
            return Err(RuntimeError::Unavailable(format!(
                "Container computer {name} did not stop."
            )));
        }
    }
    cleanup_stale(paths, tools, &dir, name);
    let gateway = registry().as_mut().and_then(|map| map.remove(name));
    super::ports::remove(paths, name);
    if let Some(running) = gateway {
        running.gateway().close();
    }
    if !record.started_once {
        let _ = dir.update_record(|record| record.started_once = true);
    }
    Ok(())
}

/// Deletes the computer's files, including the layers owned by sub-ids.
pub fn remove(paths: &RuntimePaths, tools: &Tools, name: &str) -> Result<(), RuntimeError> {
    let dir = Layout::new(&paths.home).computer(name);
    if holder_alive(paths, name) {
        return Err(RuntimeError::Invalid(format!(
            "Stop {name} before dropping it."
        )));
    }
    let id = dir
        .read_record()
        .ok()
        .and_then(|record| record.label("silo.machine-id").map(str::to_owned))
        .unwrap_or_else(|| name.to_owned());
    super::image::remove_tree(tools, dir.path()).map_err(|detail| {
        eprintln!("Removing container computer {name}: {detail}");
        RuntimeError::Unavailable(format!("Silo could not remove container computer {name}."))
    })?;
    // The events the gateway wrote, the share (the in-guest client's bridge folder, which the guest
    // wrote into), the SSH host key and the computer's certificate authority go with it. The guest's
    // files are owned by sub-ids, so the namespace removes them.
    let _ = super::image::remove_tree(tools, &paths.home.join("sandboxes").join(name));
    let _ = crate::macos_computers::proxy_ca::retire(&id);
    collect_layers(paths, tools);
    Ok(())
}

/// `command` run in a scope of the user manager (`systemd-run --user --scope`). Joining the
/// container's control group needs an ancestor this account may write, which a process started
/// from a login session (a terminal, an editor's proxy) does not share with the container; the
/// engine's own processes are inside the user manager already.
pub fn in_user_scope(tools: &Tools, command: &Command) -> Command {
    let mut scoped = Command::new(&tools.systemd_run);
    for_user_manager(&mut scoped);
    scoped
        .args(["--user", "--scope", "--quiet", "--"])
        .arg(command.get_program())
        .args(command.get_args());
    scoped
}

/// A `crun exec` command and the handle that ends what it runs.
pub struct ExecPlan {
    pub command: Command,
    /// Kills the command's whole process group inside the container.
    pub group: ExecGroup,
}

/// An exec directory with no pid recorded is a leftover after this long.
const EXEC_IDLE_MAX: Duration = Duration::from_secs(600);

/// A new private directory for one exec, inside the computer's `exec` directory. The directories
/// of execs whose command is gone are removed first; one still running is never touched, however
/// long it has run.
fn exec_directory(dir: &ComputerDir) -> Result<PathBuf, String> {
    let root = dir.state().join("exec");
    private_dir(&root)?;
    for entry in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        let path = entry.path();
        let leftover = if path.is_dir() {
            hold::exec_leftover(&path, EXEC_IDLE_MAX)
        } else {
            true
        };
        if leftover {
            let _ = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
        }
    }
    let directory = root.join(uuid::Uuid::new_v4().simple().to_string());
    private_dir(&directory)?;
    Ok(directory)
}

/// What points tools at the guest's trust, as the runtime's guest agent sets it in a virtual
/// machine: the system bundle (which `update-ca-certificates` has added the gateway CA to) for
/// OpenSSL, curl and Python's `requests`, and the gateway CA itself for Node, which adds a file
/// to its built-in roots instead of replacing them.
fn trust_environment() -> [(String, String); 4] {
    let bundle = oci::CA_BUNDLE_PATH.to_owned();
    [
        ("SSL_CERT_FILE".to_owned(), bundle.clone()),
        ("REQUESTS_CA_BUNDLE".to_owned(), bundle.clone()),
        ("CURL_CA_BUNDLE".to_owned(), bundle),
        (
            "NODE_EXTRA_CA_CERTS".to_owned(),
            oci::GATEWAY_CA_PATH.to_owned(),
        ),
    ]
}

/// Who `user` is in the computer: its ids, home and groups, read from the account files of the image
/// the computer was made from and its own writable layer.
pub fn identity(
    paths: &RuntimePaths,
    tools: &Tools,
    name: &str,
    user: &str,
) -> Result<users::Identity, String> {
    let layout = Layout::new(&paths.home);
    let dir = layout.computer(name);
    let record = dir.read_record()?;
    let image =
        super::image::unpacked_digest(&layout, &record.image_digest, &record.image_reference)
            .ok_or("The container image of this computer is not unpacked. Start it again.")?;
    let boot = boot_layer(&layout, tools)?;
    let mut layers = vec![dir.upper_of(&record)];
    layers.extend(dir.lower_dirs(&record, &boot, &image.rootfs));
    let passwd = users::read_from_layers(&layers, "etc/passwd")?.unwrap_or_default();
    let group = users::read_from_layers(&layers, "etc/group")?.unwrap_or_default();
    users::resolve(user, &passwd, &group)
        .ok_or_else(|| format!("The user {user} does not exist in {name}."))
}

/// The `crun exec` command for `exec`, run as `uid`/`gid` (and the user's other groups) with the
/// given environment.
///
/// The command runs from the image the computer was made from (its `/etc/passwd` and
/// `/etc/group` are read through the overlay's layers, never through a link the container could
/// plant), as the leader of its own process group, so a timeout or a cancellation ends all of it
/// ([`ExecPlan::group`]).
pub fn exec_command(
    paths: &RuntimePaths,
    tools: &Tools,
    name: &str,
    exec: &Exec,
    with_timeout: bool,
) -> Result<ExecPlan, String> {
    let layout = Layout::new(&paths.home);
    let dir = layout.computer(name);
    let record = dir.read_record()?;
    let user = exec.user.as_deref().unwrap_or("root");
    let identity = identity(paths, tools, name, user)?;

    let mut environment = vec![
        (
            "PATH".to_owned(),
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_owned(),
        ),
        ("HOME".to_owned(), identity.home.clone()),
        ("TERM".to_owned(), "xterm".to_owned()),
        ("LANG".to_owned(), "C.UTF-8".to_owned()),
        ("SILO_RUNTIME".to_owned(), "container".to_owned()),
    ];
    environment.extend(trust_environment());
    // The computer's own environment, then the placeholders of the secrets its gateway holds; the
    // engine's own commands take neither.
    let own = record
        .env
        .iter()
        .cloned()
        .chain(super::secrets::placeholder_env(&active_secrets(name)))
        .chain(exec.env.iter().cloned())
        .filter(|_| !exec.fixed_environment);
    for (key, value) in own {
        // The kind is the engine's to say: the guest scripts refuse to install packages by it.
        if key == "SILO_RUNTIME" {
            continue;
        }
        environment.retain(|(existing, _)| existing != &key);
        environment.push((key, value));
    }
    let mut argv = vec![
        oci::INIT_PATH.to_owned(),
        "--own-group".to_owned(),
        "--".to_owned(),
    ];
    if with_timeout {
        if let Some(limit) = exec.timeout {
            argv.extend([
                "timeout".to_owned(),
                "--kill-after=5s".to_owned(),
                format!("{}s", limit.as_secs().max(1)),
            ]);
        }
    }
    argv.extend(exec.argv.iter().cloned());
    let capabilities: &[&str] = if identity.uid == 0 {
        &oci::ROOT_EXEC_CAPABILITIES
    } else {
        &[]
    };
    let process = oci::exec_process(
        identity.uid,
        identity.gid,
        &identity.groups,
        exec.workdir.as_deref().unwrap_or("/"),
        &environment,
        &argv,
        exec.tty,
        capabilities,
    );
    let group = ExecGroup::new(exec_directory(&dir)?, &unit_name(paths, name));
    let process_file = group.process_file();
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&process_file)
            .map_err(io)?;
        file.write_all(&serde_json::to_vec(&process).map_err(|e| e.to_string())?)
            .map_err(io)?;
    }
    // Inside the holder's namespaces, so the command has the container's network.
    let mut command = Command::new(&tools.ctr);
    command
        .arg("enter")
        .arg("--state")
        .arg(dir.state())
        .arg("--")
        .arg(&tools.crun)
        .arg("--root")
        .arg(dir.state().join("crun"))
        .arg("--cgroup-manager=cgroupfs")
        .arg("exec")
        .arg("--process")
        .arg(&process_file)
        .arg("--pid-file")
        .arg(group.pid_file());
    if let Some(socket) = &exec.console_socket {
        // The server holding the terminal's controlling side receives it here (SCM_RIGHTS).
        command.arg("--console-socket").arg(socket);
    }
    command.arg(name);
    Ok(ExecPlan { command, group })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_layer_store_and_its_folder_are_the_accounts_alone() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let store = home.path().join("containers/layers");
        std::fs::create_dir_all(&store).unwrap();
        for dir in [&store, store.parent().unwrap()] {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        }
        seal_store(&store).unwrap();
        for dir in [&store, store.parent().unwrap()] {
            let mode = std::fs::metadata(dir).unwrap().permissions().mode() & 0o7777;
            assert_eq!(mode, 0o700, "{}", dir.display());
        }
        // Only the store's own folders: a layer made elsewhere leaves its parents alone.
        let elsewhere = home.path().join("elsewhere/layers");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::set_permissions(
            elsewhere.parent().unwrap(),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        seal_store(&elsewhere).unwrap();
        let mode = std::fs::metadata(elsewhere.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(mode, 0o755);
    }

    #[test]
    fn a_command_for_the_user_manager_gets_this_accounts_runtime_folder_unless_it_already_has_it() {
        use std::os::unix::fs::MetadataExt;
        let folder = tempfile::tempdir().unwrap();
        let uid = std::fs::metadata(folder.path()).unwrap().uid();
        let env = |command: &Command, key: &str| {
            command
                .get_envs()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.map(|v| v.to_owned()))
        };
        // Another account's folder (here: one nobody this account owns) is replaced.
        let mut other = Command::new("true");
        for_user_manager_of(&mut other, uid + 1, Some(folder.path().as_os_str()));
        assert_eq!(
            env(&other, "XDG_RUNTIME_DIR"),
            Some(Some(format!("/run/user/{}", uid + 1).into()))
        );
        assert_eq!(env(&other, "DBUS_SESSION_BUS_ADDRESS"), Some(None));
        // A missing one too.
        let mut missing = Command::new("true");
        for_user_manager_of(&mut missing, uid, None);
        assert!(env(&missing, "XDG_RUNTIME_DIR").is_some());
        // One this account owns is left alone.
        let mut own = Command::new("true");
        for_user_manager_of(&mut own, uid, Some(folder.path().as_os_str()));
        assert_eq!(env(&own, "XDG_RUNTIME_DIR"), None);
    }

    use super::*;

    #[test]
    fn the_cgroup_is_a_leaf_of_the_users_scope() {
        assert_eq!(
            cgroup_path(1000, "silo-ct-ab12-dev"),
            "/user.slice/user-1000.slice/user@1000.service/app.slice/silo-ct-ab12-dev.scope/ctr"
        );
    }

    #[test]
    fn the_unit_differs_per_runtime_home_and_computer() {
        let paths = |home: &str| RuntimePaths {
            guest_image: "g".into(),
            executable: "e".into(),
            home: home.into(),
            storage_home: None,
            library: "l".into(),
            metadata: "m".into(),
            volumes: "v".into(),
        };
        let a = unit_name(&paths("/a"), "dev");
        assert_ne!(a, unit_name(&paths("/b"), "dev"));
        assert_ne!(a, unit_name(&paths("/a"), "other"));
        assert!(a.starts_with("silo-ct-") && a.ends_with("-dev"));
    }

    #[test]
    fn files_bound_into_the_container_are_readable_whatever_the_umask() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("resolv.conf");
        // SAFETY: umask is process-wide; this test restores it before it ends.
        let previous = unsafe { libc::umask(0o077) };
        let written = write_readable(&path, "nameserver 1.2.3.4\n");
        unsafe { libc::umask(previous) };
        written.unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        write_readable(&path, "nameserver 5.6.7.8\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "nameserver 5.6.7.8\n"
        );
    }

    fn test_paths(home: &Path) -> RuntimePaths {
        crate::test_support::paths(home)
    }

    #[test]
    fn a_checkpoints_freeze_thaws_unless_the_user_paused_the_computer() {
        let home = tempfile::tempdir().unwrap();
        let paths = test_paths(home.path());
        let state = Layout::new(&paths.home).computer("dev").state();
        std::fs::create_dir_all(&state).unwrap();
        let file = home.path().join("cgroup.freeze");
        let frozen = |guard| Frozen {
            file: file.clone(),
            paused: paused_file(&paths, "dev"),
            _freezing: guard,
        };
        // No pause: the end of the copy thaws.
        std::fs::write(&file, "1").unwrap();
        drop(frozen(freezing()));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "0");
        // The user paused the computer: the checkpoint leaves it frozen, and inspect says so.
        assert!(!is_paused(&paths, "dev"));
        std::fs::write(paused_file(&paths, "dev"), "").unwrap();
        assert!(is_paused(&paths, "dev"));
        std::fs::write(&file, "1").unwrap();
        drop(frozen(freezing()));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "1");
    }

    #[test]
    fn a_stop_or_start_ends_a_pause() {
        assert!(STALE_STATE_FILES.contains(&PAUSED_FILE));
    }

    #[test]
    fn ending_a_pause_removes_its_marker_even_when_the_thaw_fails() {
        let home = tempfile::tempdir().unwrap();
        let paths = test_paths(home.path());
        let dir = Layout::new(&paths.home).computer("dev");
        std::fs::create_dir_all(dir.state()).unwrap();
        std::fs::write(paused_file(&paths, "dev"), "").unwrap();
        // The thaw fails where there is no cgroup (here); the pause ends regardless.
        end_pause(&paths, "dev");
        assert!(!is_paused(&paths, "dev"));
    }

    #[test]
    fn a_stop_waits_for_a_checkpoint_before_it_ends_a_pause() {
        let home = tempfile::tempdir().unwrap();
        let paths = test_paths(home.path());
        std::fs::create_dir_all(Layout::new(&paths.home).computer("dev").state()).unwrap();
        std::fs::write(paused_file(&paths, "dev"), "").unwrap();
        // A checkpoint holds the freeze lock across its copy; the pause of the user must not be
        // undone in the middle of one.
        let guard = freezing();
        let (done, taken) = std::sync::mpsc::channel();
        let paused = paths.clone();
        let tools = fake_tools(home.path());
        std::thread::spawn(move || {
            stop(&paused, &tools, "dev", &record(true)).unwrap();
            done.send(()).unwrap();
        });
        assert!(
            taken.recv_timeout(Duration::from_millis(100)).is_err(),
            "the stop went on while a checkpoint held the freeze lock"
        );
        drop(guard);
        taken.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(!is_paused(&paths, "dev"));
    }

    fn fake_process(proc: &Path, pid: u32, argv0: &str, cgroup: &str) {
        let dir = proc.join(pid.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cmdline"), format!("{argv0}\0run\0")).unwrap();
        std::fs::write(dir.join("cgroup"), cgroup).unwrap();
    }

    fn scope_of(unit: &str) -> String {
        format!("0::/user.slice/user-1000.slice/user@1000.service/app.slice/{unit}.scope\n")
    }

    #[test]
    fn the_holder_is_a_silo_ctr_in_the_computers_own_scope_not_any_process_of_that_name() {
        let proc = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let paths = test_paths(home.path());
        let unit = unit_name(&paths, "dev");
        fake_process(proc.path(), 10, "/opt/silo/silo-ctr", &scope_of(&unit));
        assert!(is_holder(proc.path(), 10, &unit));
        // A recycled pid: another program in some other cgroup.
        fake_process(proc.path(), 11, "/usr/bin/vim", &scope_of(&unit));
        fake_process(
            proc.path(),
            12,
            "/tmp/silo-ctr",
            "0::/user.slice/session-1.scope\n",
        );
        fake_process(
            proc.path(),
            13,
            "/opt/silo/silo-ctr",
            &scope_of(&unit_name(&paths, "other")),
        );
        for pid in [11, 12, 13, 14] {
            assert!(!is_holder(proc.path(), pid, &unit), "{pid}");
        }
        // The pid file decides which process is asked about.
        let state = Layout::new(&paths.home).computer("dev").state();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("holder.pid"), "12\n").unwrap();
        assert!(!holder_alive_in(proc.path(), &paths, "dev"));
        std::fs::write(state.join("holder.pid"), "10\n").unwrap();
        assert!(holder_alive_in(proc.path(), &paths, "dev"));
    }

    fn record(started: bool) -> ComputerRecord {
        ComputerRecord {
            schema_version: super::super::layout::RECORD_VERSION,
            name: "dev".into(),
            cpus: 1,
            max_cpus: 1,
            memory_mib: 1024,
            max_memory_mib: 1024,
            root_disk_mib: 1024,
            workspace_mib: 1024,
            labels: Vec::new(),
            env: Vec::new(),
            image_digest: "sha256:made-from".into(),
            image_reference: "ref:1".into(),
            layers: Vec::new(),
            upper: "l0".into(),
            created_ms: 1,
            started_once: started,
            secrets: Vec::new(),
            github: false,
            network_blocked: false,
        }
    }

    #[test]
    fn reading_the_state_of_a_live_holder_without_a_gateway_changes_nothing() {
        let proc = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let paths = test_paths(home.path());
        let unit = unit_name(&paths, "starting-box");
        let dir = Layout::new(&paths.home).computer("starting-box");
        let state = dir.state();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("holder.pid"), "10\n").unwrap();
        std::fs::write(state.join("ctl.sock"), "").unwrap();
        fake_process(proc.path(), 10, "silo-ctr", &scope_of(&unit));
        // A holder whose gateway this engine does not have is not running, and a read leaves it alone.
        for started in [false, true] {
            let expected = if started {
                Status::Stopped
            } else {
                Status::Created
            };
            for _ in 0..3 {
                assert_eq!(
                    status_in(proc.path(), &paths, "starting-box", &record(started)),
                    expected
                );
            }
        }
        assert!(state.join("holder.pid").exists() && state.join("ctl.sock").exists());
        // While a start is in flight it says so.
        let mark = StartMark::begin("starting-box").unwrap();
        assert_eq!(
            status_in(proc.path(), &paths, "starting-box", &record(false)),
            Status::Starting
        );
        drop(mark);
    }

    #[test]
    fn a_running_computer_held_for_a_checkpoint_is_still_running() {
        let proc = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let paths = test_paths(home.path());
        let name = "held-box";
        let state = Layout::new(&paths.home).computer(name).state();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("holder.pid"), "10\n").unwrap();
        std::fs::write(state.join("ctl.sock"), "").unwrap();
        fake_process(
            proc.path(),
            10,
            "silo-ctr",
            &scope_of(&unit_name(&paths, name)),
        );
        let (control, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let running = register_for_test(name, Gateway::for_test(control), Vec::new());
        let status = || status_in(proc.path(), &paths, name, &record(true));
        assert_eq!(status(), Status::Running);
        // A checkpoint keeps a start from beginning. Commands and secret changes ask for the
        // status: to them the computer must not look as if it were starting.
        let hold = StartMark::hold(name).unwrap();
        assert!(StartMark::begin(name).is_none());
        assert_eq!(status(), Status::Running);
        drop(hold);
        registry().as_mut().and_then(|map| map.remove(name));
        running.gateway().close();
    }

    fn fake_tools(directory: &Path) -> Tools {
        let init = directory.join("silo-init");
        std::fs::write(&init, b"init").unwrap();
        // A stand-in for `silo-ctr userns`: it runs the command after `--`, except the ownership
        // change that only a user namespace allows.
        let ctr = directory.join("silo-ctr");
        std::fs::write(
            &ctr,
            "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done; shift\n[ \"$1\" = chown ] && exit 0\nexec \"$@\"\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&ctr, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let other = |name: &str| directory.join(name);
        Tools {
            ctr,
            init,
            gateway: other("gateway"),
            crun: other("crun"),
            skopeo: other("skopeo"),
            umoci: other("umoci"),
            systemd_run: other("systemd-run"),
            systemctl: other("systemctl"),
        }
    }

    fn exec_of(user: &str) -> Exec {
        Exec {
            name: "dev".into(),
            user: Some(user.into()),
            env: vec![("A".into(), "b".into())],
            workdir: Some("/workspace".into()),
            no_start: true,
            timeout: Some(Duration::from_secs(7)),
            argv: vec!["id".into()],
            ..Exec::default()
        }
    }

    #[test]
    fn a_command_runs_on_the_image_the_computer_was_made_from_with_the_users_groups_and_its_own_group(
    ) {
        let home = tempfile::tempdir().unwrap();
        let paths = test_paths(home.path());
        let layout = Layout::new(&paths.home);
        let dir = layout.computer("dev");
        std::fs::create_dir_all(dir.path()).unwrap();
        dir.write_record(&record(true)).unwrap();
        let tools = fake_tools(home.path());
        // The recorded image is not unpacked (only some other one is): refused, not substituted.
        let other = layout.image("sha256:pinned-now");
        std::fs::create_dir_all(other.join("rootfs/etc")).unwrap();
        std::fs::write(other.join("digest"), "sha256:pinned-now\n").unwrap();
        std::fs::write(other.join("rootfs/etc/passwd"), "silo:x:5:5::/h:/bin/sh\n").unwrap();
        let error = exec_command(&paths, &tools, "dev", &exec_of("silo"), true)
            .err()
            .unwrap();
        assert!(error.contains("not unpacked"), "{error}");
        // With the recorded image unpacked, its own account files are used.
        let made = layout.image("sha256:made-from");
        std::fs::create_dir_all(made.join("rootfs/etc")).unwrap();
        std::fs::write(made.join("digest"), "sha256:made-from\n").unwrap();
        std::fs::write(
            made.join("rootfs/etc/passwd"),
            "silo:x:1001:1001::/home/silo:/bin/sh\n",
        )
        .unwrap();
        std::fs::write(
            made.join("rootfs/etc/group"),
            "silo:x:1001:\nvideo:x:44:silo\n",
        )
        .unwrap();
        let plan = exec_command(&paths, &tools, "dev", &exec_of("silo"), true).unwrap();
        let args: Vec<String> = plan
            .command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let at = |flag: &str| args.iter().position(|arg| arg == flag).unwrap();
        assert_eq!(
            args[at("--pid-file") + 1],
            plan.group.pid_file().to_string_lossy()
        );
        use std::os::unix::fs::PermissionsExt;
        // Both files live in one directory that is the exec's alone, closed to other users.
        let exec_directory = plan.group.pid_file().parent().unwrap().to_owned();
        assert_eq!(
            args[at("--process") + 1],
            plan.group.process_file().to_string_lossy()
        );
        assert_eq!(plan.group.process_file().parent().unwrap(), exec_directory);
        assert_eq!(
            std::fs::metadata(&exec_directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(args.last().unwrap(), "dev");
        let process: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&args[at("--process") + 1]).unwrap()).unwrap();
        assert_eq!(process["user"]["uid"], 1001);
        assert_eq!(process["user"]["additionalGids"], serde_json::json!([44]));
        assert_eq!(process["cwd"], "/workspace");
        let argv: Vec<&str> = process["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        assert_eq!(
            argv,
            [
                "/silo/init",
                "--own-group",
                "--",
                "timeout",
                "--kill-after=5s",
                "7s",
                "id"
            ]
        );
        assert!(process.get("capabilities").is_none());
        // Tools trust the gateway CA the way they do in a virtual machine: OpenSSL, curl and
        // `requests` through the system bundle, Node through the CA file itself.
        let env: Vec<&str> = process["env"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.as_str().unwrap())
            .collect();
        for wanted in [
            "SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt",
            "REQUESTS_CA_BUNDLE=/etc/ssl/certs/ca-certificates.crt",
            "CURL_CA_BUNDLE=/etc/ssl/certs/ca-certificates.crt",
            "NODE_EXTRA_CA_CERTS=/usr/local/share/ca-certificates/silo-gateway.crt",
        ] {
            assert!(env.contains(&wanted), "{wanted} missing from {env:?}");
        }
        // Without a limit (a streaming command) there is no timeout wrapper.
        let mut open = exec_of("silo");
        open.timeout = None;
        let streaming = exec_command(&paths, &tools, "dev", &open, false).unwrap();
        let text = streaming
            .command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let file = &text[text.iter().position(|a| a == "--process").unwrap() + 1];
        assert!(!std::fs::read_to_string(file).unwrap().contains("timeout"));
        // The files, environment values included, are removed when the exec is over.
        let streaming_directory = streaming.group.process_file().parent().unwrap().to_owned();
        drop(streaming);
        assert!(!streaming_directory.exists());
        drop(plan);
        assert!(!exec_directory.exists());
    }

    /// The environment of the process a root `exec` of `dev` gets, as `NAME=value` lines.
    fn process_environment(exec: &Exec) -> Vec<String> {
        let home = tempfile::tempdir().unwrap();
        let paths = test_paths(home.path());
        let layout = Layout::new(&paths.home);
        let dir = layout.computer("dev");
        std::fs::create_dir_all(dir.path()).unwrap();
        let mut stored = record(true);
        stored.env = [
            "PATH=/work/bin",
            "LD_PRELOAD=/work/evil.so",
            "HOME=/work",
            "OWN=1",
        ]
        .iter()
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap();
            (key.to_owned(), value.to_owned())
        })
        .collect();
        dir.write_record(&stored).unwrap();
        let made = layout.image("sha256:made-from");
        std::fs::create_dir_all(made.join("rootfs/etc")).unwrap();
        std::fs::write(made.join("digest"), "sha256:made-from\n").unwrap();
        std::fs::write(
            made.join("rootfs/etc/passwd"),
            "root:x:0:0::/root:/bin/sh\n",
        )
        .unwrap();
        let plan = exec_command(&paths, &fake_tools(home.path()), "dev", exec, true).unwrap();
        let process: serde_json::Value =
            serde_json::from_slice(&std::fs::read(plan.group.process_file()).unwrap()).unwrap();
        process["env"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry.as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn a_command_the_engine_runs_takes_none_of_the_computers_own_environment() {
        let mut engine = exec_of("root");
        engine.fixed_environment = true;
        let env = process_environment(&engine);
        // A program an account can replace runs as root with the engine's defaults only.
        for set in [
            "PATH=/work/bin",
            "LD_PRELOAD=/work/evil.so",
            "HOME=/work",
            "OWN=1",
            "A=b",
        ] {
            assert!(
                !env.iter().any(|entry| entry == set),
                "{set} reached {env:?}"
            );
        }
        assert!(env
            .iter()
            .any(|entry| entry
                == "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"));
        assert!(env.iter().any(|entry| entry == "HOME=/root"));
        assert!(env.iter().any(|entry| entry == "SILO_RUNTIME=container"));
        // The owner's own command keeps the computer's variables and its own.
        let env = process_environment(&exec_of("root"));
        for set in [
            "PATH=/work/bin",
            "LD_PRELOAD=/work/evil.so",
            "HOME=/work",
            "OWN=1",
            "A=b",
        ] {
            assert!(
                env.iter().any(|entry| entry == set),
                "{set} missing from {env:?}"
            );
        }
    }

    #[test]
    fn the_engines_own_root_commands_are_fixed_environment_commands() {
        assert!(super::super::desktop::boot_command("dev").fixed_environment);
        assert!(super::super::gateway_ca_command("dev").fixed_environment);
    }

    #[test]
    fn a_long_running_exec_keeps_its_files_and_only_finished_ones_are_removed() {
        use std::os::unix::process::CommandExt;
        let home = tempfile::tempdir().unwrap();
        let paths = test_paths(home.path());
        let layout = Layout::new(&paths.home);
        let dir = layout.computer("dev");
        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::create_dir_all(layout.layer("l0").join("etc")).unwrap();
        dir.write_record(&record(true)).unwrap();
        let made = layout.image("sha256:made-from");
        std::fs::create_dir_all(made.join("rootfs/etc")).unwrap();
        std::fs::write(made.join("digest"), "sha256:made-from\n").unwrap();
        std::fs::write(
            made.join("rootfs/etc/passwd"),
            "root:x:0:0::/root:/bin/sh\n",
        )
        .unwrap();
        let tools = fake_tools(home.path());
        let first = exec_command(&paths, &tools, "dev", &exec_of("root"), true).unwrap();
        let mut leader = std::process::Command::new("sleep")
            .arg("300")
            .process_group(0)
            .spawn()
            .unwrap();
        std::fs::write(first.group.pid_file(), format!("{}\n", leader.id())).unwrap();
        // Another finished exec left its files behind.
        let finished = dir.state().join("exec").join("finished");
        std::fs::create_dir_all(&finished).unwrap();
        std::fs::write(finished.join("exec.pid"), "999999999\n").unwrap();
        std::fs::write(finished.join("process.json"), "{}").unwrap();
        let second = exec_command(&paths, &tools, "dev", &exec_of("root"), true).unwrap();
        assert!(
            first.group.process_file().exists(),
            "a running exec lost its files"
        );
        assert!(second.group.process_file().exists());
        assert!(!finished.exists(), "a finished exec's files stayed behind");
        // So the timeout can still end the command: a process of the computer's scope, as
        // `crun exec` leaves it.
        let proc = tempfile::tempdir().unwrap();
        let described = proc.path().join(leader.id().to_string());
        std::fs::create_dir_all(&described).unwrap();
        std::fs::write(
            described.join("cgroup"),
            format!("0::/user.slice/x/{}.scope/ctr\n", unit_name(&paths, "dev")),
        )
        .unwrap();
        assert!(first.group.with_proc(proc.path()).kill());
        leader.wait().unwrap();
    }

    #[test]
    fn a_planted_link_in_the_writable_layer_makes_the_exec_refuse_instead_of_reading_a_host_file() {
        let home = tempfile::tempdir().unwrap();
        let paths = test_paths(home.path());
        let layout = Layout::new(&paths.home);
        let dir = layout.computer("dev");
        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::create_dir_all(layout.layer("l0").join("etc")).unwrap();
        dir.write_record(&record(true)).unwrap();
        let made = layout.image("sha256:made-from");
        std::fs::create_dir_all(made.join("rootfs/etc")).unwrap();
        std::fs::write(made.join("digest"), "sha256:made-from\n").unwrap();
        std::fs::write(
            made.join("rootfs/etc/passwd"),
            "root:x:0:0::/root:/bin/sh\n",
        )
        .unwrap();
        let host_file = home.path().join("host-passwd");
        std::fs::write(&host_file, "root:x:0:0::/host:/bin/sh\n").unwrap();
        std::os::unix::fs::symlink(&host_file, layout.layer("l0").join("etc/passwd")).unwrap();
        let error = exec_command(
            &paths,
            &fake_tools(home.path()),
            "dev",
            &exec_of("root"),
            false,
        )
        .err()
        .unwrap();
        assert!(error.contains("not a regular file"), "{error}");
    }

    fn a_computer_with_an_image(home: &Path) -> (RuntimePaths, Tools) {
        let paths = test_paths(home);
        let layout = Layout::new(&paths.home);
        let dir = layout.computer("dev");
        std::fs::create_dir_all(dir.path()).unwrap();
        dir.write_record(&record(true)).unwrap();
        let made = layout.image("sha256:made-from");
        std::fs::create_dir_all(made.join("rootfs/etc")).unwrap();
        std::fs::write(made.join("digest"), "sha256:made-from\n").unwrap();
        std::fs::write(
            made.join("rootfs/etc/passwd"),
            "silo:x:1001:1001::/home/silo:/bin/bash\n",
        )
        .unwrap();
        (paths, fake_tools(home))
    }

    #[test]
    fn a_terminal_runs_on_a_pseudo_terminal_and_hands_it_to_the_console_socket() {
        let home = tempfile::tempdir().unwrap();
        let (paths, tools) = a_computer_with_an_image(home.path());
        let mut exec = exec_of("silo");
        exec.tty = true;
        exec.console_socket = Some("/tmp/silo-pty-x/c".into());
        let plan = exec_command(&paths, &tools, "dev", &exec, false).unwrap();
        let args: Vec<String> = plan
            .command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let at = args
            .iter()
            .position(|arg| arg == "--console-socket")
            .unwrap();
        assert_eq!(args[at + 1], "/tmp/silo-pty-x/c");
        // The container name stays last, after every option.
        assert_eq!(args.last().unwrap(), "dev");
        let process: serde_json::Value =
            serde_json::from_slice(&std::fs::read(plan.group.process_file()).unwrap()).unwrap();
        assert_eq!(process["terminal"], true);
        assert_eq!(process["user"]["uid"], 1001);
        // Without a terminal, no socket is passed.
        let plain = exec_command(&paths, &tools, "dev", &exec_of("silo"), false).unwrap();
        assert!(!plain
            .command
            .get_args()
            .any(|arg| arg == "--console-socket"));
    }

    #[test]
    fn who_a_user_is_comes_from_the_image_the_computer_was_made_from() {
        let home = tempfile::tempdir().unwrap();
        let (paths, tools) = a_computer_with_an_image(home.path());
        let identity = identity(&paths, &tools, "dev", "silo").unwrap();
        assert_eq!(
            (identity.uid, identity.gid, identity.home.as_str()),
            (1001, 1001, "/home/silo")
        );
        assert!(identity_error(&paths, &tools, "nobody").contains("does not exist"));
    }

    fn identity_error(paths: &RuntimePaths, tools: &Tools, user: &str) -> String {
        identity(paths, tools, "dev", user).unwrap_err()
    }

    #[test]
    fn an_upper_directory_from_an_earlier_build_is_refused_with_a_recreate_message() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let dir = layout.computer("dev");
        std::fs::create_dir_all(dir.path()).unwrap();
        // None yet: it is made on the first start.
        let sub_ids = 500_000..565_535;
        let upper = dir.legacy_upper();
        assert_eq!(check_upper_in(&upper, "dev", &sub_ids), Ok(false));
        // One this process made is owned by the engine's own user, as an earlier build's was.
        std::fs::create_dir_all(&upper).unwrap();
        let error = check_upper_in(&upper, "dev", &sub_ids).unwrap_err();
        assert!(
            error.contains("Recreate it") && error.contains("dev"),
            "{error}"
        );
        // A file in its place is no upper directory either.
        std::fs::remove_dir(&upper).unwrap();
        std::fs::write(&upper, "x").unwrap();
        assert!(check_upper_in(&upper, "dev", &sub_ids).is_err());
    }

    #[test]
    fn the_boot_layer_is_named_by_the_init_shim_and_the_ownership_layout() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let tools = fake_tools(home.path());
        let boot = boot_layer(&layout, &tools).unwrap();
        assert!(boot.join("silo/init").is_file() && boot.join("workspace").is_dir());
        // A layer an earlier build made under the init shim's plain digest is never reused.
        let plain = hex_prefix(&Sha256::digest(b"init"));
        assert_ne!(boot.file_name().unwrap().to_str().unwrap(), plain);
    }

    fn hex_prefix(digest: &[u8]) -> String {
        digest
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[test]
    fn the_share_directory_is_private_to_the_account_and_only_searchable_below() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let runtime = share_directory(home.path(), "dev").unwrap();
        assert_eq!(runtime, home.path().join("sandboxes/dev/runtime"));
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&home.path().join("sandboxes/dev")), 0o700);
        assert_eq!(mode(&runtime), 0o711);
    }

    #[test]
    fn the_secrets_the_engine_published_are_what_other_processes_read() {
        let home = tempfile::tempdir().unwrap();
        let dir = Layout::new(home.path()).computer("dev");
        std::fs::create_dir_all(dir.state()).unwrap();
        assert!(published_secrets(&dir).is_empty());
        let file = Some(dir.state().join(ACTIVE_SECRETS_FILE));
        publish_active(&file, &["A".into(), "B_TOKEN".into()]);
        assert_eq!(published_secrets(&dir), ["A", "B_TOKEN"]);
        publish_active(&file, &[]);
        assert!(published_secrets(&dir).is_empty());
    }
}
