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
    Material,
    hold::{self, ExecGroup, Reaped, StartMark},
    layout::{ComputerDir, ComputerRecord, Layout},
    oci,
    tools::{run_checked, Tools},
    users,
};
use crate::computer_gateway::{computer_ca, Gateway, Params, Secret};
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
            eprintln!("Container computer {name}: stopping a leftover of an earlier Silo engine.");
            let _ = stop(paths, tools, &name, &record);
        }
    }
}

fn boot_layer(layout: &Layout, tools: &Tools) -> Result<PathBuf, String> {
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
pub fn check_upper(dir: &ComputerDir, name: &str) -> Result<bool, String> {
    use std::os::unix::fs::MetadataExt;
    match std::fs::symlink_metadata(dir.upper()) {
        Ok(meta)
            if meta.is_dir()
                && super::idmap::KeepId::current().upper_owner_is_current(meta.uid()) =>
        {
            Ok(true)
        }
        Ok(_) => Err(super::idmap::recreate_message(name)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io(error)),
    }
}

/// Makes the computer's upper directory, or checks the one it has.
fn prepare_upper(tools: &Tools, dir: &ComputerDir, name: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let upper = dir.upper();
    match check_upper(dir, name)? {
        true => Ok(()),
        false => {
            // Made aside and renamed into place, so an interrupted start never leaves an upper
            // directory that looks like an earlier build's.
            let staging = dir.path().join("upper.new");
            super::image::remove_tree(tools, &staging)?;
            std::fs::create_dir(&staging).map_err(|e| at("create", &staging, e))?;
            // The overlay's root directory takes the upper directory's mode: it must stay traversable.
            std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| at("set the mode of", &staging, e))?;
            own_as_root(tools, &[&staging])?;
            std::fs::rename(&staging, &upper).map_err(|e| at("move it to", &upper, e))
        }
    }
}

/// Creates what the container binds as its per-computer share (`/.msb`): the computer's runtime
/// folder, which the in-guest client's bridge folder lives in. Only the account that owns the engine
/// reaches it from the host, and the container's other users only get to search it, as in a virtual
/// machine (`guest_client::Spool` seals the folders below it).
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
pub(super) fn receive_descriptor(stream: &std::os::unix::net::UnixStream) -> Result<OwnedFd, String> {
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
    let record = dir.read_record().map_err(RuntimeError::Unavailable)?;
    if is_running(Path::new("/proc"), paths, name) {
        return Ok(());
    }
    // Before any image is unpacked for it.
    check_upper(&dir, name).map_err(RuntimeError::Unavailable)?;
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
    prepare_upper(tools, dir, name)?;
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

    let lower = dir
        .lower_dirs(record, &boot, rootfs)
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(":");
    // The holder appends to one log across starts: only what it writes from here is this start's.
    let log_from =
        std::fs::metadata(state.join("holder.log")).map_or(0, |meta| meta.len() as usize);
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join("holder.log"))
        .map_err(|e| at("open", &state.join("holder.log"), e))?;
    let mut command = Command::new(&tools.systemd_run);
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
        .arg(dir.upper())
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
    let gateway = Gateway::start(
        &tools.gateway,
        tap,
        &ca_cert,
        &ca_key,
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
    wait_running(&tools.crun, &state, name, deadline)?;
    holder_pid(&state).ok_or("The container holder left no pid file.")?;
    // The holder runs on; dropping the guard hands its exit status to a thread.
    let log = std::fs::read_to_string(state.join("holder.log")).unwrap_or_default();
    Ok(Started {
        warnings: hold::warnings_since(&log, log_from),
    })
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
    let _ = Command::new(&tools.systemctl)
        .arg("--user")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

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
    for stale in ["ctl.sock", "holder.pid", "outer.pid", "container.pid"] {
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
    Ok(())
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
    let mut layers = vec![dir.upper()];
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
    // The computer's own environment, then the placeholders of the secrets its gateway holds.
    let own = record
        .env
        .iter()
        .cloned()
        .chain(super::secrets::placeholder_env(&active_secrets(name)));
    for (key, value) in own.chain(exec.env.iter().cloned()) {
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
    let group = ExecGroup::new(exec_directory(&dir)?);
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
            created_ms: 1,
            started_once: started,
            secrets: Vec::new(),
            github: false,
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
            std::fs::metadata(&exec_directory).unwrap().permissions().mode() & 0o777,
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

    #[test]
    fn a_long_running_exec_keeps_its_files_and_only_finished_ones_are_removed() {
        use std::os::unix::process::CommandExt;
        let home = tempfile::tempdir().unwrap();
        let paths = test_paths(home.path());
        let layout = Layout::new(&paths.home);
        let dir = layout.computer("dev");
        std::fs::create_dir_all(dir.upper().join("etc")).unwrap();
        dir.write_record(&record(true)).unwrap();
        let made = layout.image("sha256:made-from");
        std::fs::create_dir_all(made.join("rootfs/etc")).unwrap();
        std::fs::write(made.join("digest"), "sha256:made-from\n").unwrap();
        std::fs::write(made.join("rootfs/etc/passwd"), "root:x:0:0::/root:/bin/sh\n").unwrap();
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
        assert!(first.group.process_file().exists(), "a running exec lost its files");
        assert!(second.group.process_file().exists());
        assert!(!finished.exists(), "a finished exec's files stayed behind");
        // So the timeout can still end the command.
        assert!(first.group.kill());
        leader.wait().unwrap();
    }

    #[test]
    fn a_planted_link_in_the_writable_layer_makes_the_exec_refuse_instead_of_reading_a_host_file() {
        let home = tempfile::tempdir().unwrap();
        let paths = test_paths(home.path());
        let layout = Layout::new(&paths.home);
        let dir = layout.computer("dev");
        std::fs::create_dir_all(dir.upper().join("etc")).unwrap();
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
        std::os::unix::fs::symlink(&host_file, dir.upper().join("etc/passwd")).unwrap();
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
        let at = args.iter().position(|arg| arg == "--console-socket").unwrap();
        assert_eq!(args[at + 1], "/tmp/silo-pty-x/c");
        // The container name stays last, after every option.
        assert_eq!(args.last().unwrap(), "dev");
        let process: serde_json::Value = serde_json::from_slice(
            &std::fs::read(plan.group.process_file()).unwrap(),
        )
        .unwrap();
        assert_eq!(process["terminal"], true);
        assert_eq!(process["user"]["uid"], 1001);
        // Without a terminal, no socket is passed.
        let plain = exec_command(&paths, &tools, "dev", &exec_of("silo"), false).unwrap();
        assert!(!plain.command.get_args().any(|arg| arg == "--console-socket"));
    }

    #[test]
    fn who_a_user_is_comes_from_the_image_the_computer_was_made_from() {
        let home = tempfile::tempdir().unwrap();
        let (paths, tools) = a_computer_with_an_image(home.path());
        let identity = identity(&paths, &tools, "dev", "silo").unwrap();
        assert_eq!((identity.uid, identity.gid, identity.home.as_str()), (1001, 1001, "/home/silo"));
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
        assert_eq!(check_upper(&dir, "dev"), Ok(false));
        // One this process made is owned by the engine's own user, as an earlier build's was.
        std::fs::create_dir_all(dir.upper()).unwrap();
        let error = check_upper(&dir, "dev").unwrap_err();
        assert!(error.contains("Recreate it") && error.contains("dev"), "{error}");
        // A file in its place is no upper directory either.
        std::fs::remove_dir(dir.upper()).unwrap();
        std::fs::write(dir.upper(), "x").unwrap();
        assert!(check_upper(&dir, "dev").is_err());
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
        digest.iter().take(8).map(|byte| format!("{byte:02x}")).collect()
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
