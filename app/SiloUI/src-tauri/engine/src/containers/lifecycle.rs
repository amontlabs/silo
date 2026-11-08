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
//! engine process, so a container does not outlive it: the first look after an engine restart
//! stops a container whose gateway is gone.

use super::{
    args::Exec,
    layout::{ComputerDir, ComputerRecord, Layout},
    oci,
    tools::Tools,
    users,
};
use crate::computer_gateway::{computer_ca, Gateway, Params};
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
    sync::Mutex,
    time::{Duration, Instant},
};

const GUEST_IPV4: Ipv4Addr = Ipv4Addr::new(172, 30, 0, 6);
const GATEWAY_IPV4: Ipv4Addr = Ipv4Addr::new(172, 30, 0, 5);
const GUEST_MAC: [u8; 6] = [0x02, 0x53, 0x49, 0x4c, 0x4f, 0x01];
const TASKS_MAX: u32 = 4096;
const TERM_WAIT: Duration = Duration::from_secs(15);

/// The gateways of the running containers of this engine, by computer name.
static GATEWAYS: Mutex<Option<HashMap<String, Gateway>>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Created,
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

/// Whether `pid` is a live `silo-ctr` (a recycled pid is not).
fn is_holder(pid: u32) -> bool {
    std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|bytes| {
        bytes
            .split(|b| *b == 0)
            .next()
            .is_some_and(|argv0| argv0.ends_with(b"silo-ctr"))
    })
}

pub fn holder_alive(dir: &ComputerDir) -> bool {
    holder_pid(&dir.state()).is_some_and(is_holder)
}

fn gateway_tracked(name: &str) -> bool {
    GATEWAYS
        .lock()
        .ok()
        .and_then(|map| map.as_ref().map(|map| map.contains_key(name)))
        .unwrap_or(false)
}

pub fn status(paths: &RuntimePaths, tools: &Tools, name: &str, record: &ComputerRecord) -> Status {
    let dir = Layout::new(&paths.home).computer(name);
    if holder_alive(&dir) {
        if gateway_tracked(name) {
            return Status::Running;
        }
        // Left over from an earlier engine process, which took its gateway with it.
        let _ = stop(paths, tools, name, record);
    }
    if record.started_once {
        Status::Stopped
    } else {
        Status::Created
    }
}

fn boot_layer(layout: &Layout, tools: &Tools) -> Result<PathBuf, String> {
    let bytes = std::fs::read(&tools.init).map_err(io)?;
    let digest = Sha256::digest(&bytes);
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
    let _ = std::fs::remove_dir_all(&partial);
    std::fs::create_dir_all(partial.join("silo")).map_err(io)?;
    std::fs::create_dir_all(partial.join("workspace")).map_err(io)?;
    let target = partial.join("silo/init");
    std::fs::write(&target, &bytes).map_err(io)?;
    // Every user of the computer reads these, whatever umask the engine runs with.
    use std::os::unix::fs::PermissionsExt;
    for directory in [&partial, &partial.join("silo"), &partial.join("workspace")] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755)).map_err(io)?;
    }
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).map_err(io)?;
    match std::fs::rename(&partial, &boot) {
        Ok(()) => Ok(boot),
        Err(_) if boot.join("silo/init").is_file() => Ok(boot),
        Err(error) => Err(io(error)),
    }
}

fn private_dir(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(io)
}

/// `recvmsg` of one file descriptor sent with `SCM_RIGHTS`.
fn receive_descriptor(stream: &std::os::unix::net::UnixStream) -> Result<OwnedFd, String> {
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
    pid: u32,
}

pub fn start(
    paths: &RuntimePaths,
    tools: &Tools,
    name: &str,
    timeout: Duration,
) -> Result<(), RuntimeError> {
    let unavailable = |detail: String| {
        eprintln!("Starting container computer {name}: {detail}");
        RuntimeError::Unavailable(format!(
            "Silo could not start container computer {name}. {detail}"
        ))
    };
    let layout = Layout::new(&paths.home);
    let dir = layout.computer(name);
    let mut record = dir.read_record().map_err(RuntimeError::Unavailable)?;
    if status(paths, tools, name, &record) == Status::Running {
        return Ok(());
    }
    let image = super::image::ensure(paths, tools)?;
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
    )
    .map_err(|detail| {
        let _ = stop(paths, tools, name, &record);
        unavailable(detail)
    })?;
    let _ = started.pid;
    record.started_once = true;
    dir.write_record(&record)
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
) -> Result<Started, String> {
    let state = dir.state();
    private_dir(&state)?;
    // The overlay's root directory takes the upper directory's mode: it must stay traversable.
    std::fs::create_dir_all(dir.upper()).map_err(io)?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.upper(), std::fs::Permissions::from_mode(0o755))
            .map_err(io)?;
    }
    private_dir(&dir.work())?;
    private_dir(&dir.merged())?;
    private_dir(&dir.bundle())?;
    let boot = boot_layer(layout, tools)?;
    let uid = unsafe { libc::getuid() };
    let unit = unit_name(paths, name);
    let spec = oci::build(&oci::Params {
        name,
        rootfs: &dir.merged(),
        cgroup_path: &cgroup_path(uid, &unit),
        resolv_conf: &state.join("resolv.conf"),
        hosts: &state.join("hosts"),
        arch: std::env::consts::ARCH,
    })?;
    std::fs::write(
        dir.bundle().join("config.json"),
        serde_json::to_vec_pretty(&spec).map_err(|e| e.to_string())?,
    )
    .map_err(io)?;
    std::fs::write(
        state.join("hosts"),
        format!("127.0.0.1 localhost\n127.0.1.1 {name}\n::1 localhost\n"),
    )
    .map_err(io)?;
    std::fs::write(
        state.join("resolv.conf"),
        format!("nameserver {GATEWAY_IPV4}\n"),
    )
    .map_err(io)?;

    let socket = state.join("ctl.sock");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).map_err(io)?;
    listener.set_nonblocking(true).map_err(io)?;

    let lower = dir
        .lower_dirs(record, &boot, rootfs)
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(":");
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join("holder.log"))
        .map_err(io)?;
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
    let mut child = command
        .spawn()
        .map_err(|e| format!("systemd-run could not start: {e}"))?;
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
                if let Ok(Some(status)) = child.try_wait() {
                    return Err(format!(
                        "The container holder exited ({status}): {}",
                        tail(&state)
                    ));
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
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
    let tap = receive_descriptor(&stream)?;
    let id = record.label("silo.machine-id").unwrap_or(name).to_owned();
    let (ca_cert, ca_key) = computer_ca(&tools.gateway, &id)?;
    let gateway_log = std::fs::File::create(state.join("gateway.log")).map_err(io)?;
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
            secrets: Vec::new(),
            github_profile_json: None,
            github_hosts: None,
            usage_log: None,
            log: Some(gateway_log),
        },
    )?;
    let ready = gateway.ready().clone();
    std::fs::write(
        state.join("resolv.conf"),
        format!("nameserver {}\n", ready.dns_ipv4),
    )
    .map_err(io)?;
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
    GATEWAYS
        .lock()
        .map_err(|_| "The gateway registry is unavailable.".to_owned())?
        .get_or_insert_with(HashMap::new)
        .insert(name.to_owned(), gateway);
    // The holder runs on: a thread reaps systemd-run's exit status so no zombie is left.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    wait_running(&tools.crun, &state, name, deadline)?;
    let pid = holder_pid(&state).ok_or("The container holder left no pid file.")?;
    Ok(Started { pid })
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

fn systemctl(args: &[&str]) {
    let _ = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// What an earlier run left: a scope, crun state, a socket.
fn cleanup_stale(paths: &RuntimePaths, tools: &Tools, dir: &ComputerDir, name: &str) {
    let state = dir.state();
    if holder_alive(dir) {
        systemctl(&[
            "kill",
            "--signal=SIGKILL",
            &format!("{}.scope", unit_name(paths, name)),
        ]);
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
    if holder_alive(&dir) {
        let _ = crun(tools, &state)
            .args(["kill", name, "TERM"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let deadline = Instant::now() + TERM_WAIT;
        while holder_alive(&dir) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        if holder_alive(&dir) {
            let unit = unit_name(paths, name);
            systemctl(&["kill", "--signal=SIGKILL", &format!("{unit}.scope")]);
            let deadline = Instant::now() + Duration::from_secs(10);
            while holder_alive(&dir) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        if holder_alive(&dir) {
            return Err(RuntimeError::Unavailable(format!(
                "Container computer {name} did not stop."
            )));
        }
    }
    cleanup_stale(paths, tools, &dir, name);
    let gateway = GATEWAYS
        .lock()
        .ok()
        .and_then(|mut map| map.as_mut().and_then(|map| map.remove(name)));
    if let Some(gateway) = gateway {
        gateway.shutdown();
    }
    if !record.started_once {
        let mut updated = record.clone();
        updated.started_once = true;
        let _ = dir.write_record(&updated);
    }
    Ok(())
}

/// Deletes the computer's files, including the layers owned by sub-ids.
pub fn remove(paths: &RuntimePaths, tools: &Tools, name: &str) -> Result<(), RuntimeError> {
    let dir = Layout::new(&paths.home).computer(name);
    if holder_alive(&dir) {
        return Err(RuntimeError::Invalid(format!(
            "Stop {name} before dropping it."
        )));
    }
    super::image::remove_tree(tools, dir.path()).map_err(|detail| {
        eprintln!("Removing container computer {name}: {detail}");
        RuntimeError::Unavailable(format!("Silo could not remove container computer {name}."))
    })
}

/// The `crun exec` command for `exec`, run as `uid`/`gid` with the given environment.
pub fn exec_command(
    paths: &RuntimePaths,
    tools: &Tools,
    name: &str,
    exec: &Exec,
    with_timeout: bool,
) -> Result<Command, String> {
    let layout = Layout::new(&paths.home);
    let dir = layout.computer(name);
    let record = dir.read_record()?;
    let image =
        super::image::unpacked(&layout, &super::image::pinned().map_err(|e| e.to_string())?)
            .ok_or("The container image is not unpacked.")?;
    let boot = boot_layer(&layout, tools)?;
    let mut layers = vec![dir.upper()];
    layers.extend(dir.lower_dirs(&record, &boot, &image.rootfs));
    let passwd = users::read_from_layers(&layers, "etc/passwd").unwrap_or_default();
    let group = users::read_from_layers(&layers, "etc/group").unwrap_or_default();
    let user = exec.user.as_deref().unwrap_or("root");
    let identity = users::resolve(user, &passwd, &group)
        .ok_or_else(|| format!("The user {user} does not exist in {name}."))?;
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
        .arg("--cgroup-manager=cgroupfs");
    command.arg("exec").arg("--no-new-privs");
    command
        .arg("--cwd")
        .arg(exec.workdir.as_deref().unwrap_or("/"));
    command
        .arg("--user")
        .arg(format!("{}:{}", identity.uid, identity.gid));
    if identity.uid == 0 {
        for capability in oci::ROOT_EXEC_CAPABILITIES {
            command.arg("--cap").arg(capability);
        }
    }
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
    for (key, value) in &exec.env {
        environment.retain(|(existing, _)| existing != key);
        environment.push((key.clone(), value.clone()));
    }
    for (key, value) in environment {
        command.arg("--env").arg(format!("{key}={value}"));
    }
    command.arg(name).arg(oci::INIT_PATH).arg("--");
    if with_timeout {
        if let Some(limit) = exec.timeout {
            command
                .arg("timeout")
                .arg("--kill-after=5s")
                .arg(format!("{}s", limit.as_secs().max(1)));
        }
    }
    command.args(&exec.argv);
    Ok(command)
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
    fn only_a_live_silo_ctr_counts_as_the_holder() {
        assert!(!is_holder(std::process::id()));
        assert!(!is_holder(u32::MAX));
    }
}
