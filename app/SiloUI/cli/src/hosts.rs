//! `silo host` prepares this machine to host computers that other devices manage through
//! Connections; `silo devices` adds, lists and removes the devices this one manages.
//!
//! A host is any machine with a Silo engine: a Mac (Linux and macOS computers) or a Linux server
//! (Linux computers over KVM). `silo host init` checks that the machine can run computers,
//! installs the engine as a service that starts on its own (a systemd user unit through
//! `silod --service register` on Linux; on a Mac the engine's login item), links the command on
//! PATH, turns Connections on, authorizes the controller keys it is given and prints the address
//! a controller uses. Everything it changes goes through the engine's own methods and
//! `silod --service`; it never writes `authorized_keys` or a unit file itself.
//!
//! A controller adds the host with `silo devices add <user@host>`, which is the app's
//! Connect device: `connections.devices.connect`, with the app's two SSH repairs (trust the host
//! key, install this device's Silo key) run in this terminal when SSH asks for them.

use std::{
    ffi::OsString,
    fs,
    io::IsTerminal,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use clap::Subcommand;
use serde_json::{json, Value};
use silo_api::methods::{connections, requests, EmptyParams};
use silo_client::channel::Channel;

use crate::{engine::Engine, error::CliError};

#[derive(Subcommand, Debug)]
pub enum HostCommand {
    /// Prepare this machine to host computers for other devices: check it can run them,
    /// start the engine as a service, put `silo` on PATH, turn Connections on and print the
    /// address to add it by. Safe to run again.
    Init {
        /// A controller's Silo key to accept (`ssh-ed25519 ...`, as `silo devices key` prints
        /// it on the controller), or a file of them. Repeatable.
        #[arg(long = "authorize", value_name = "KEY|FILE")]
        authorize: Vec<String>,
    },
    /// Show whether this machine hosts computers: the engine service, VM support,
    /// Connections, the controller keys it accepts and its address.
    Status,
    /// Stop accepting a controller's Silo key (`ssh-ed25519 ...`, or a file of them).
    Revoke {
        #[arg(value_name = "KEY|FILE")]
        key: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum DevicesCommand {
    /// Connect a device by its SSH address (`user@host`, an SSH alias or
    /// `ssh://user@host:port`), as Connect device does in the app. When SSH asks, it verifies the
    /// host key and installs this device's Silo key there, in this terminal.
    Add {
        /// `user@host`, `user@host:port`, an SSH alias, or `ssh://user@host:port`.
        address: String,
        /// The device is already saved at another address: save this one instead.
        #[arg(long)]
        replace: bool,
        /// The host's SSH key to trust, added to ~/.ssh/known_hosts first so nothing asks: its
        /// fingerprint (`SHA256:...`, matched against what the host offers), the key
        /// (`ssh-ed25519 AAAA...`), or a file holding it (`ssh_host_ed25519_key.pub`).
        #[arg(long, value_name = "FINGERPRINT|KEY|FILE")]
        host_key: Option<String>,
    },
    /// List the connected devices and whether they answer.
    List,
    /// Remove a connected device (its name or id); its key is revoked there first.
    Remove {
        /// Its name or id (the field is not called `device`: that is the global `--device`).
        #[arg(value_name = "DEVICE")]
        name: String,
    },
    /// Print this device's Silo key, for `silo host init --authorize` on a host.
    Key,
}

/// Which kind of machine this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Mac,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Linux
        }
    }

    /// The kinds of computers this machine runs.
    fn kinds(self) -> &'static [&'static str] {
        match self {
            Self::Linux => &["linux"],
            Self::Mac => &["linux", "macos"],
        }
    }
}

/// Group membership of this account for the group that owns `/dev/kvm`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Membership {
    /// This process has the group.
    Active,
    /// The user database lists the account in the group, but this login session predates it.
    Pending,
    Absent,
    Unknown,
}

/// What this machine says about running VMs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VmProbe {
    Linux {
        /// `/dev/kvm` exists and is a character device.
        device: bool,
        /// This account can open it for reading and writing.
        accessible: bool,
        /// The group that owns it.
        group: Option<String>,
        membership: Membership,
    },
    Mac {
        apple_silicon: bool,
    },
}

impl VmProbe {
    /// The real machine's answer.
    pub fn detect(platform: Platform, kvm: &Path) -> Self {
        match platform {
            Platform::Mac => Self::Mac {
                apple_silicon: cfg!(target_arch = "aarch64"),
            },
            Platform::Linux => probe_kvm(kvm),
        }
    }

    /// `Ok` with a one-line summary, or why computers cannot run here and what to do.
    pub fn check(&self) -> Result<String, String> {
        match self {
            Self::Mac { apple_silicon: true } => Ok(
                "Apple silicon: Linux and macOS computers (macOS allows 2 macOS computers running at once)".into(),
            ),
            Self::Mac { apple_silicon: false } => Err(
                "This Mac cannot host computers: Silo runs computers on Apple silicon Macs only."
                    .into(),
            ),
            Self::Linux { device: false, .. } => Err(
                "This machine cannot run Linux computers: /dev/kvm is missing. Turn on hardware virtualization (VT-x or AMD-V) in the firmware, or nested virtualization for a VM, and load the kvm module (modprobe kvm_intel or kvm_amd)."
                    .into(),
            ),
            Self::Linux {
                accessible: true, ..
            } => Ok("KVM: /dev/kvm is readable and writable by this account".into()),
            Self::Linux {
                group, membership, ..
            } => {
                let group = group.as_deref().unwrap_or("kvm");
                Err(match membership {
                    Membership::Pending => format!(
                        "/dev/kvm is not readable and writable by this account yet. It is in the {group} group, but this login session started before that: log out and back in (or run newgrp {group}), then run silo host init again."
                    ),
                    Membership::Active => format!(
                        "/dev/kvm is not readable and writable by this account although it is in the {group} group. Check the device's permissions (ls -l /dev/kvm)."
                    ),
                    Membership::Absent | Membership::Unknown => format!(
                        "/dev/kvm is not readable and writable by this account. An administrator adds it to the {group} group (sudo usermod -aG {group} \"$(id -un)\"); then log out and back in and run silo host init again."
                    ),
                })
            }
        }
    }
}

fn probe_kvm(kvm: &Path) -> VmProbe {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let Ok(metadata) = fs::metadata(kvm) else {
        return VmProbe::Linux {
            device: false,
            accessible: false,
            group: None,
            membership: Membership::Unknown,
        };
    };
    let device = metadata.file_type().is_char_device();
    let accessible = c_path(kvm)
        .map(|path| unsafe { libc::access(path.as_ptr(), libc::R_OK | libc::W_OK) } == 0)
        .unwrap_or(false);
    let gid = metadata.gid();
    let (group, members) = group_entry(gid);
    let user = account_name(unsafe { libc::geteuid() });
    let membership = if process_groups().contains(&gid) {
        Membership::Active
    } else if members.is_none() || user.is_none() {
        Membership::Unknown
    } else if members
        .as_ref()
        .zip(user.as_ref())
        .is_some_and(|(members, user)| members.contains(user))
    {
        Membership::Pending
    } else {
        Membership::Absent
    };
    VmProbe::Linux {
        device,
        accessible,
        group,
        membership,
    }
}

fn c_path(path: &Path) -> Option<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes()).ok()
}

fn process_groups() -> Vec<libc::gid_t> {
    let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    if count <= 0 {
        return vec![unsafe { libc::getegid() }];
    }
    let mut groups = vec![0 as libc::gid_t; count as usize];
    let count = unsafe { libc::getgroups(count, groups.as_mut_ptr()) };
    groups.truncate(count.max(0) as usize);
    groups.push(unsafe { libc::getegid() });
    groups
}

/// The group's name and its listed members, from the group database.
fn group_entry(gid: libc::gid_t) -> (Option<String>, Option<Vec<String>>) {
    let mut buffer = vec![0 as libc::c_char; 4096];
    loop {
        let mut entry: libc::group = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::group = std::ptr::null_mut();
        let code = unsafe {
            libc::getgrgid_r(
                gid,
                &mut entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            )
        };
        if code == libc::ERANGE && buffer.len() < 1 << 20 {
            let larger = buffer.len() * 2;
            buffer.resize(larger, 0);
            continue;
        }
        if code != 0 || result.is_null() || entry.gr_name.is_null() {
            return (None, None);
        }
        let text = |pointer: *const libc::c_char| {
            unsafe { std::ffi::CStr::from_ptr(pointer) }
                .to_string_lossy()
                .into_owned()
        };
        let name = text(entry.gr_name);
        let mut members = Vec::new();
        let mut cursor = entry.gr_mem;
        while !cursor.is_null() && !unsafe { *cursor }.is_null() {
            members.push(text(unsafe { *cursor }));
            cursor = unsafe { cursor.add(1) };
        }
        return (Some(name), Some(members));
    }
}

/// The account's name from the user database, never from `$USER`.
fn account_name(uid: libc::uid_t) -> Option<String> {
    let mut buffer = vec![0 as libc::c_char; 4096];
    loop {
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let code = unsafe {
            libc::getpwuid_r(
                uid,
                &mut entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            )
        };
        if code == libc::ERANGE && buffer.len() < 1 << 20 {
            let larger = buffer.len() * 2;
            buffer.resize(larger, 0);
            continue;
        }
        if code != 0 || result.is_null() || entry.pw_name.is_null() {
            return None;
        }
        return Some(
            unsafe { std::ffi::CStr::from_ptr(entry.pw_name) }
                .to_string_lossy()
                .into_owned(),
        );
    }
}

/// Everything `silo host` reads about this machine, so tests can supply their own.
#[derive(Clone, Debug)]
pub struct HostSetup {
    pub platform: Platform,
    pub channel: Channel,
    pub euid: u32,
    pub home: PathBuf,
    /// The canonical path of this `silo`.
    pub cli: PathBuf,
    /// `$PATH`.
    pub path: Option<OsString>,
    pub socket: PathBuf,
    pub vm: VmProbe,
    /// Where this machine's SSH server should listen (port 22 on the loopback address).
    pub ssh: SocketAddr,
    /// How long to wait for the engine's socket after starting the service.
    pub engine_wait: Duration,
}

impl HostSetup {
    /// This machine, this account and this `silo`.
    pub fn detect(socket: PathBuf, channel: Channel) -> Result<Self, CliError> {
        let platform = Platform::current();
        let cli = std::env::current_exe()
            .and_then(fs::canonicalize)
            .map_err(|error| CliError::new(format!("Cannot locate this silo command: {error}")))?;
        Ok(Self {
            platform,
            channel,
            euid: unsafe { libc::geteuid() },
            home: crate::access::home_dir()?,
            cli,
            path: std::env::var_os("PATH"),
            socket,
            vm: VmProbe::detect(platform, Path::new("/dev/kvm")),
            ssh: SocketAddr::from(([127, 0, 0, 1], 22)),
            engine_wait: Duration::from_secs(60),
        })
    }

    /// The `silod` that serves this `silo`: beside it in a Linux engine directory, or in the
    /// engine bundle of the app that holds it on a Mac.
    pub fn silod(&self) -> Option<PathBuf> {
        let directory = self.cli.parent()?;
        let candidates = match self.platform {
            Platform::Linux => vec![directory.join("silod")],
            Platform::Mac => {
                let contents = directory.parent()?;
                vec![
                    // `Silo Engine.app/Contents/MacOS/silo`
                    directory.join("silod"),
                    // `Silo.app/Contents/MacOS/silo`
                    contents.join(
                        "Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod",
                    ),
                ]
            }
        };
        candidates.into_iter().find(|path| path.is_file())
    }

    fn cli_link(&self) -> PathBuf {
        self.home
            .join(".local/bin")
            .join(self.channel.cli_link_name())
    }

    fn on_path(&self) -> bool {
        let bin = self.home.join(".local/bin");
        self.path
            .as_ref()
            .is_some_and(|path| std::env::split_paths(path).any(|entry| entry == bin))
    }
}

/// The output of `silod --service`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServiceReport {
    /// `enabled`, `disabled`, `notRegistered`, `requiresApproval`, `notFound`, or `running`
    /// for a Mac engine that already answers.
    pub status: String,
    /// The `key: value` lines after the status word (Linux: state, unit, program, linger).
    pub details: Vec<(String, String)>,
    /// What silod advised on its error stream (lingering, a restart to pick up a new unit).
    pub advice: Vec<String>,
}

impl ServiceReport {
    fn parse(stdout: &str, stderr: &str) -> Self {
        let mut lines = stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty());
        let status = lines.next().unwrap_or("unknown").to_owned();
        let details = lines
            .filter_map(|line| {
                let (key, value) = line.split_once(':')?;
                Some((key.trim().to_owned(), value.trim().to_owned()))
            })
            .collect();
        let advice = stderr
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        Self {
            status,
            details,
            advice,
        }
    }

    #[cfg(test)]
    fn detail(&self, key: &str) -> Option<&str> {
        self.details
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    fn to_json(&self) -> Value {
        let mut value = json!({"status": self.status, "advice": self.advice});
        for (key, detail) in &self.details {
            value[key] = json!(detail);
        }
        value
    }
}

/// Runs `silod --service <command>` and reads its report; a failure carries silod's message.
fn silod_service(silod: &Path, command: &str) -> Result<ServiceReport, CliError> {
    let output = Command::new(silod)
        .args(["--service", command])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| CliError::new(format!("Could not run {}: {error}", silod.display())))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let message = stderr.trim();
        return Err(CliError::new(format!(
            "{} --service {command} failed: {}",
            silod.display(),
            if message.is_empty() {
                "no message"
            } else {
                message
            }
        )));
    }
    Ok(ServiceReport::parse(&stdout, &stderr))
}

/// Refuses root and `nobody`, as every entry point of the engine does.
fn check_account(euid: u32) -> Result<(), CliError> {
    if euid == 0 || euid == 65534 {
        return Err(CliError::new(
            "Run silo host init as the account that will own the computers, not root or nobody: the engine refuses those accounts. Use a dedicated account (in the kvm group on Linux).",
        ));
    }
    Ok(())
}

/// What `link_cli` found or did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkOutcome {
    Linked,
    AlreadyLinked,
    /// Something else is at the link's path; it is left alone.
    Foreign(String),
    Failed(String),
}

/// Points `~/.local/bin/<silo or silo-dev>` at this `silo`, only when nothing is there or the
/// link there is ours: a link to a `silo` with a `silod` beside it, or to a `silo` that is gone
/// (an engine directory that was replaced).
pub fn link_cli(setup: &HostSetup) -> LinkOutcome {
    let link = setup.cli_link();
    match fs::symlink_metadata(&link) {
        Ok(metadata) => {
            let previous = fs::read_link(&link).ok();
            if previous.as_deref() == Some(setup.cli.as_path()) {
                return LinkOutcome::AlreadyLinked;
            }
            let ours = metadata.file_type().is_symlink()
                && previous.as_deref().is_some_and(|target| {
                    target.file_name().is_some_and(|name| name == "silo")
                        && (!target.exists()
                            || target
                                .parent()
                                .is_some_and(|dir| dir.join("silod").is_file()))
                });
            if !ours {
                return LinkOutcome::Foreign(format!(
                    "{} already exists and is not a link to a Silo engine's silo; it was left as it is. Run {} directly, or move that file away and run silo host init again.",
                    link.display(),
                    setup.cli.display()
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return LinkOutcome::Failed(error.to_string()),
    }
    let replaced = (|| -> std::io::Result<()> {
        let directory = link.parent().expect("the link has a directory");
        fs::create_dir_all(directory)?;
        let temporary = directory.join(format!(
            ".{}-{}",
            setup.channel.cli_link_name(),
            uuid::Uuid::new_v4()
        ));
        std::os::unix::fs::symlink(&setup.cli, &temporary)?;
        fs::rename(&temporary, &link).inspect_err(|_| {
            let _ = fs::remove_file(&temporary);
        })
    })();
    match replaced {
        Ok(()) => LinkOutcome::Linked,
        Err(error) => LinkOutcome::Failed(format!("Could not link {}: {error}", link.display())),
    }
}

/// The link's state for `silo host status`, without changing it.
fn cli_link_state(setup: &HostSetup) -> Value {
    let link = setup.cli_link();
    let target = fs::read_link(&link).ok();
    json!({
        "link": link,
        "target": target,
        "thisSilo": target.as_deref() == Some(setup.cli.as_path()),
        "onPath": setup.on_path(),
    })
}

fn path_advice(setup: &HostSetup) -> Option<String> {
    (!setup.on_path()).then(|| {
        format!(
            "{} is not on PATH. Add it in your shell's profile, for example: export PATH=\"$HOME/.local/bin:$PATH\"",
            setup.home.join(".local/bin").display()
        )
    })
}

/// Whether something accepts TCP connections at the SSH server's address.
fn ssh_listening(address: SocketAddr) -> bool {
    TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_ok()
}

fn ssh_state(setup: &HostSetup) -> Value {
    let listening = ssh_listening(setup.ssh);
    let mut value = json!({"address": setup.ssh.to_string(), "listening": listening});
    if !listening {
        value["advice"] = json!(match setup.platform {
            Platform::Mac => "Nothing answers on port 22 here. Controllers reach this Mac over SSH: turn on Remote Login in System Settings > General > Sharing (or use the port your SSH server listens on in the address).",
            Platform::Linux => "Nothing answers on port 22 here. Controllers reach this machine over SSH: install and start an SSH server (for example openssh-server), or use the port yours listens on in the address.",
        });
    }
    value
}

/// The keys `--authorize` and `revoke` take: the argument itself when it is a key, else each
/// key line of the file it names.
pub fn read_keys(argument: &str) -> Result<Vec<String>, CliError> {
    let argument = argument.trim();
    if argument.starts_with("ssh-") {
        return Ok(vec![argument.to_owned()]);
    }
    let path = Path::new(argument);
    let metadata = fs::metadata(path).map_err(|error| {
        CliError::new(format!(
            "{argument} is neither a key (ssh-ed25519 ...) nor a readable file: {error}"
        ))
    })?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return Err(CliError::new(format!(
            "{argument} is not a public key file."
        )));
    }
    let contents = fs::read_to_string(path)
        .map_err(|error| CliError::new(format!("Cannot read {argument}: {error}")))?;
    let keys: Vec<String> = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect();
    if keys.is_empty() {
        return Err(CliError::new(format!("{argument} holds no key.")));
    }
    Ok(keys)
}

/// The key's OpenSSH fingerprint (`SHA256:...`), which `ssh-keygen -lf` prints for it too.
pub fn fingerprint(public_key: &str) -> Option<String> {
    use base64::{engine::general_purpose, Engine as _};
    use sha2::{Digest, Sha256};
    let blob = public_key.split_whitespace().nth(1)?;
    let bytes = general_purpose::STANDARD.decode(blob).ok()?;
    Some(format!(
        "SHA256:{}",
        general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(bytes))
    ))
}

fn key_rows(keys: &[String]) -> Value {
    Value::Array(
        keys.iter()
            .map(|key| json!({"publicKey": key, "fingerprint": fingerprint(key)}))
            .collect(),
    )
}

fn controller_keys(listed: &Value) -> Vec<String> {
    listed["publicKeys"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// Waits for the engine socket to accept a connection.
async fn wait_for_engine(socket: &Path, patience: Duration) -> bool {
    let deadline = Instant::now() + patience;
    loop {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn engine_reachable(socket: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(socket).is_ok()
}

/// Starts the engine as a service. Linux: `silod --service register` (idempotent: it rewrites
/// the unit only when it changed and starts it). Mac: an engine that answers already runs
/// as the login item the app registered; otherwise the engine bundle registers its own.
fn install_service(setup: &HostSetup) -> Result<ServiceReport, CliError> {
    let silod = setup.silod();
    match setup.platform {
        Platform::Linux => {
            let silod = silod.ok_or_else(|| {
                CliError::new(format!(
                    "There is no silod beside {}. Run silo host init with the silo of an unpacked Silo engine archive (silo-engine-<version>-linux-<arch>.tar.gz), for example ~/.local/opt/silo-engine/silo host init.",
                    setup.cli.display()
                ))
            })?;
            silod_service(&silod, "register")
        }
        Platform::Mac => {
            if engine_reachable(&setup.socket) {
                return Ok(ServiceReport {
                    status: "running".into(),
                    details: Vec::new(),
                    advice: Vec::new(),
                });
            }
            let silod = silod.ok_or_else(|| {
                CliError::new(format!(
                    "{} is not inside a Silo app, so there is no engine to start. Open {} once, or run the silo inside it ({}.app/Contents/MacOS/silo).",
                    setup.cli.display(),
                    setup.channel.product_name(),
                    setup.channel.product_name()
                ))
            })?;
            let report = silod_service(&silod, "register")?;
            match report.status.as_str() {
                "enabled" => Ok(report),
                "requiresApproval" => Err(CliError::new(format!(
                    "Allow the {} engine in System Settings > General > Login Items, then run silo host init again.",
                    setup.channel.product_name()
                ))),
                other => Err(CliError::new(format!(
                    "The {} engine could not be registered as a login item ({other}). Open {} once and run silo host init again.",
                    setup.channel.product_name(),
                    setup.channel.product_name()
                ))),
            }
        }
    }
}

fn service_name(channel: Channel) -> String {
    format!("{}.engine.service", channel.identifier())
}

/// `silo host init`.
pub async fn init(setup: &HostSetup, authorize: &[String]) -> Result<Value, CliError> {
    check_account(setup.euid)?;
    let vm = setup.vm.check().map_err(CliError::new)?;
    // Every key is read before anything changes, so a typo changes nothing.
    let mut keys = Vec::new();
    for argument in authorize {
        keys.extend(read_keys(argument)?);
    }
    let service = install_service(setup)?;
    let (cli, mut advice) = match setup.platform {
        Platform::Linux => {
            let outcome = link_cli(setup);
            let mut advice = Vec::new();
            let state = match &outcome {
                LinkOutcome::Linked => "linked",
                LinkOutcome::AlreadyLinked => "alreadyLinked",
                LinkOutcome::Foreign(message) | LinkOutcome::Failed(message) => {
                    advice.push(message.clone());
                    "notLinked"
                }
            };
            if matches!(outcome, LinkOutcome::Linked | LinkOutcome::AlreadyLinked) {
                advice.extend(path_advice(setup));
            }
            let mut cli = cli_link_state(setup);
            cli["state"] = json!(state);
            (cli, advice)
        }
        // The engine links the app's `silo` on PATH itself when it starts.
        Platform::Mac => (cli_link_state(setup), Vec::new()),
    };
    if !wait_for_engine(&setup.socket, setup.engine_wait).await {
        let logs = match setup.platform {
            Platform::Linux => format!("journalctl --user -u {}", service_name(setup.channel)),
            Platform::Mac => "Console, process silod".to_owned(),
        };
        return Err(CliError::new(format!(
            "The engine service is registered but the engine does not answer at {}. See its log ({logs}).",
            setup.socket.display()
        )));
    }
    let engine = Engine::connect(&setup.socket).await?;
    let status = engine
        .call(requests::connections_set_enabled(
            connections::ConnectionsSetEnabledParams { enabled: true },
        ))
        .await?;
    if let Some(error) = status["cliLinkError"].as_str() {
        advice.push(error.to_owned());
    }
    let mut authorized = Vec::new();
    for key in &keys {
        let result = engine
            .call(requests::connections_keys_authorize(
                connections::AuthorizeKeyParams {
                    public_key: key.clone(),
                },
            ))
            .await
            .map_err(|error| CliError::new(format!("Could not authorize {key}: {error}")))?;
        authorized.push(json!({
            "publicKey": key,
            "fingerprint": fingerprint(key),
            "added": result["added"].as_bool().unwrap_or(false),
        }));
    }
    let listed = engine
        .call(requests::connections_keys_list(EmptyParams {}))
        .await?;
    let ssh = ssh_state(setup);
    if let Some(text) = ssh["advice"].as_str() {
        advice.push(text.to_owned());
    }
    advice.extend(service.advice.iter().cloned());
    let controllers = controller_keys(&listed);
    if controllers.is_empty() {
        advice.push(
            "No controller key is authorized yet. On the controlling device run silo devices add with the address below (it installs its key over SSH), or run silo devices key there and then silo host init --authorize '<key>' here."
                .to_owned(),
        );
    }
    Ok(json!({
        "platform": platform_name(setup.platform),
        "kinds": setup.platform.kinds(),
        "vm": {"ok": true, "detail": vm},
        "service": service.to_json(),
        "cli": cli,
        "connections": connections_json(&status),
        "authorized": authorized,
        "controllers": key_rows(&controllers),
        "ssh": ssh,
        "advice": advice,
    }))
}

fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Linux => "linux",
        Platform::Mac => "macos",
    }
}

fn connections_json(status: &Value) -> Value {
    json!({
        "enabled": status["enabled"],
        "deviceId": status["deviceId"],
        "name": status["name"],
        "address": status["address"],
        "addresses": status["addresses"],
        "error": status["error"],
    })
}

/// `silo host status`: reads only.
pub async fn status(setup: &HostSetup) -> Result<Value, CliError> {
    let vm = match setup.vm.check() {
        Ok(detail) => json!({"ok": true, "detail": detail}),
        Err(detail) => json!({"ok": false, "detail": detail}),
    };
    let running = engine_reachable(&setup.socket);
    let service = match (setup.platform, setup.silod()) {
        (Platform::Mac, _) if running => json!({"status": "running"}),
        (_, Some(silod)) => match silod_service(&silod, "status") {
            Ok(report) => report.to_json(),
            Err(error) => json!({"status": "unknown", "error": error.message}),
        },
        (_, None) => {
            json!({"status": "unknown", "error": format!("There is no silod beside {}.", setup.cli.display())})
        }
    };
    let mut value = json!({
        "platform": platform_name(setup.platform),
        "kinds": setup.platform.kinds(),
        "vm": vm,
        "service": service,
        "engine": {"running": running, "socket": setup.socket},
        "cli": cli_link_state(setup),
        "ssh": ssh_state(setup),
    });
    if running {
        let engine = Engine::connect(&setup.socket).await?;
        let status = engine
            .call(requests::connections_status(EmptyParams {}))
            .await?;
        value["connections"] = connections_json(&status);
        let bridge = setup
            .home
            .join(".local/bin")
            .join(setup.channel.remote_bridge_name());
        value["connections"]["bridgeLink"] = json!(fs::symlink_metadata(&bridge).is_ok());
        let listed = engine
            .call(requests::connections_keys_list(EmptyParams {}))
            .await?;
        value["controllers"] = key_rows(&controller_keys(&listed));
    }
    Ok(value)
}

/// `silo host revoke`.
pub async fn revoke(engine: &Engine, argument: &str) -> Result<Value, CliError> {
    let keys = read_keys(argument)?;
    for key in &keys {
        engine
            .call(requests::connections_keys_revoke(
                connections::RevokeKeyParams {
                    device_id: None,
                    public_key: key.clone(),
                },
            ))
            .await?;
    }
    let listed = engine
        .call(requests::connections_keys_list(EmptyParams {}))
        .await?;
    Ok(json!({
        "revoked": key_rows(&keys),
        "controllers": key_rows(&controller_keys(&listed)),
    }))
}

pub fn render_host(value: &Value) -> String {
    let mut out = Vec::new();
    let kinds = value["kinds"]
        .as_array()
        .map(|kinds| {
            kinds
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" and ")
        })
        .unwrap_or_default();
    out.push(format!(
        "Computers: {kinds} ({})",
        value["vm"]["detail"].as_str().unwrap_or_default()
    ));
    if value["vm"]["ok"] == false {
        out.push("  This machine cannot run computers yet.".to_owned());
    }
    let service = &value["service"];
    let mut line = format!(
        "Engine service: {}",
        service["status"].as_str().unwrap_or("unknown")
    );
    if let Some(state) = service["state"].as_str() {
        line.push_str(&format!(", {state}"));
    }
    if let Some(linger) = service["linger"].as_str() {
        line.push_str(&format!(
            ", starts at boot: {}",
            if linger == "yes" {
                "yes"
            } else {
                "no (lingering is off)"
            }
        ));
    }
    out.push(line);
    if let Some(unit) = service["unit"].as_str() {
        out.push(format!("  unit: {unit}"));
    }
    if let Some(error) = service["error"].as_str() {
        out.push(format!("  {error}"));
    }
    if value["engine"]["running"] == false {
        out.push("Engine: not running".to_owned());
    }
    if let Some(link) = value["cli"]["link"].as_str() {
        let state = if value["cli"]["thisSilo"] == true {
            "this silo"
        } else if value["cli"]["target"].is_null() {
            "not linked"
        } else {
            "another silo"
        };
        out.push(format!("Command: {link} ({state})"));
    }
    let connections = &value["connections"];
    if connections.is_object() {
        out.push(format!(
            "Connections: {}",
            if connections["enabled"] == true {
                "on"
            } else {
                "off"
            }
        ));
        if let Some(error) = connections["error"].as_str() {
            out.push(format!("  {error}"));
        }
        out.push(format!(
            "  device: {} ({})",
            connections["name"].as_str().unwrap_or_default(),
            connections["deviceId"].as_str().unwrap_or_default()
        ));
    }
    if value["ssh"]["listening"] == false {
        out.push(format!(
            "SSH: nothing answers at {}",
            value["ssh"]["address"].as_str().unwrap_or_default()
        ));
    }
    for row in value["authorized"].as_array().into_iter().flatten() {
        out.push(format!(
            "{} {}",
            if row["added"] == true {
                "Authorized"
            } else {
                "Already authorized"
            },
            row["fingerprint"].as_str().unwrap_or_default()
        ));
    }
    for row in value["revoked"].as_array().into_iter().flatten() {
        out.push(format!(
            "Revoked {}",
            row["fingerprint"].as_str().unwrap_or_default()
        ));
    }
    if let Some(controllers) = value["controllers"].as_array() {
        if controllers.is_empty() {
            out.push("Controllers: none".to_owned());
        } else {
            out.push("Controllers:".to_owned());
            for row in controllers {
                out.push(format!(
                    "  {}",
                    row["fingerprint"].as_str().unwrap_or_default()
                ));
            }
        }
    }
    if connections.is_object() {
        let addresses: Vec<&str> = connections["addresses"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["address"].as_str())
            .collect();
        let first = addresses
            .first()
            .copied()
            .or_else(|| connections["address"].as_str())
            .unwrap_or_default();
        out.push(String::new());
        out.push(format!(
            "Add this machine from the controlling device with: silo devices add {first}"
        ));
        out.push("(or Connect device... in Silo, Settings > Connections).".to_owned());
        if addresses.len() > 1 {
            out.push(format!("Other addresses: {}", addresses[1..].join(", ")));
        }
        out.push("Use ssh://user@host:port for an SSH server on another port.".to_owned());
    }
    let advice: Vec<&str> = value["advice"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !advice.is_empty() {
        out.push(String::new());
        out.extend(advice.iter().map(|line| format!("Note: {line}")));
    }
    if let Some(text) = value["ssh"]["advice"].as_str() {
        if !advice.contains(&text) {
            out.push(format!("Note: {text}"));
        }
    }
    out.join("\n")
}

/// Runs the SSH repairs `silo devices add` needs in this terminal. Tests replace it.
pub trait Terminal {
    /// Whether a person can answer SSH's prompts here.
    fn interactive(&self) -> bool;
    /// Runs `command` (a shell command line the engine built) with this terminal's input and
    /// output, and says whether it succeeded.
    fn run(&self, command: &str) -> Result<bool, CliError>;
    /// The host keys `host` offers on `port`, as `ssh-keyscan` prints them.
    fn keyscan(&self, host: &str, port: u16) -> Result<String, CliError>;
}

/// The real terminal.
pub struct SystemTerminal;

impl Terminal for SystemTerminal {
    fn interactive(&self) -> bool {
        std::io::stdin().is_terminal()
    }

    fn run(&self, command: &str) -> Result<bool, CliError> {
        let status = Command::new("/bin/sh")
            .args(["-c", command])
            .status()
            .map_err(|error| CliError::new(format!("Could not run ssh: {error}")))?;
        Ok(status.success())
    }

    fn keyscan(&self, host: &str, port: u16) -> Result<String, CliError> {
        let output = Command::new("ssh-keyscan")
            .args(["-T", "10", "-p", &port.to_string(), "--", host])
            .stdin(Stdio::null())
            .output()
            .map_err(|error| CliError::new(format!("Could not run ssh-keyscan: {error}")))?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// `user@host:port` as `ssh://user@host:port`, the form ssh reads a port from; every other
/// address as given.
pub fn normalize_address(address: &str) -> String {
    let address = address.trim();
    if address.starts_with("ssh://") || address.contains('[') {
        return address.to_owned();
    }
    match address.rsplit_once(':') {
        Some((host, port))
            if !host.is_empty()
                && !host.contains(':')
                && !port.is_empty()
                && port.parse::<u16>().is_ok_and(|port| port > 0) =>
        {
            format!("ssh://{address}")
        }
        _ => address.to_owned(),
    }
}

/// The host and port SSH connects to for `address` (port 22 when it names none). An SSH alias
/// is returned as it is; its `HostName` in ssh_config is not read.
pub fn ssh_target(address: &str) -> Result<(String, u16), CliError> {
    let invalid = || CliError::new(format!("Cannot read a host from {address}."));
    let rest = address.strip_prefix("ssh://").unwrap_or(address);
    let rest = rest.trim_end_matches('/');
    let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
    let (host, port) = if let Some(bracketed) = rest.strip_prefix('[') {
        let (host, after) = bracketed.split_once(']').ok_or_else(invalid)?;
        let port = match after.strip_prefix(':') {
            Some(port) => port.parse().map_err(|_| invalid())?,
            None if after.is_empty() => 22,
            None => return Err(invalid()),
        };
        (host.to_owned(), port)
    } else if address.starts_with("ssh://") {
        match rest.rsplit_once(':') {
            Some((host, port)) => (host.to_owned(), port.parse().map_err(|_| invalid())?),
            None => (rest.to_owned(), 22),
        }
    } else {
        (rest.to_owned(), 22)
    };
    if host.is_empty() || port == 0 {
        return Err(invalid());
    }
    Ok((host, port))
}

/// The name `known_hosts` lists `host` under: the host itself on port 22, else `[host]:port`.
fn known_hosts_name(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_owned()
    } else {
        format!("[{host}]:{port}")
    }
}

/// `type key` from a line holding a public key (a `.pub` file, a `known_hosts` or
/// `ssh-keyscan` line), whatever comes before or after it.
fn key_in_line(line: &str) -> Option<(String, String)> {
    let words: Vec<&str> = line.split_whitespace().collect();
    words.windows(2).find_map(|pair| {
        let known = pair[0].starts_with("ssh-")
            || pair[0].starts_with("ecdsa-")
            || pair[0].starts_with("sk-");
        (known && fingerprint(&format!("{} {}", pair[0], pair[1])).is_some())
            .then(|| (pair[0].to_owned(), pair[1].to_owned()))
    })
}

/// Adds the host key `pin` names to `known_hosts` for `address` and returns what it pinned.
/// A fingerprint is matched against the keys the host offers; a key or key file is taken as
/// given. Nothing is trusted that the pin does not name.
pub fn pin_host_key(
    known_hosts: &Path,
    address: &str,
    pin: &str,
    terminal: &dyn Terminal,
) -> Result<Value, CliError> {
    let (host, port) = ssh_target(address)?;
    let pin = pin.trim();
    let (kind, key) = if pin.starts_with("SHA256:") {
        let scanned = terminal.keyscan(&host, port)?;
        let offered: Vec<(String, String)> = scanned
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(key_in_line)
            .collect();
        offered
            .iter()
            .find(|(kind, key)| fingerprint(&format!("{kind} {key}")).as_deref() == Some(pin))
            .cloned()
            .ok_or_else(|| {
                let seen: Vec<String> = offered
                    .iter()
                    .filter_map(|(kind, key)| {
                        fingerprint(&format!("{kind} {key}")).map(|print| format!("{kind} {print}"))
                    })
                    .collect();
                CliError::new(if seen.is_empty() {
                    format!("{host} port {port} offered no SSH host key (ssh-keyscan); is its SSH server running?")
                } else {
                    format!(
                        "No host key {host} offers matches {pin}; it offers {}. Check the fingerprint on the host (ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub).",
                        seen.join(", ")
                    )
                })
            })?
    } else {
        let text = if key_in_line(pin).is_some() {
            pin.to_owned()
        } else {
            let path = Path::new(pin);
            if fs::metadata(path)
                .map(|metadata| metadata.len())
                .unwrap_or(u64::MAX)
                > 64 * 1024
            {
                return Err(CliError::new(format!(
                    "{pin} is neither a host key fingerprint (SHA256:...), a key, nor a readable key file."
                )));
            }
            fs::read_to_string(path).map_err(|error| {
                CliError::new(format!(
                    "{pin} is neither a host key fingerprint (SHA256:...), a key, nor a readable key file: {error}"
                ))
            })?
        };
        text.lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .find_map(key_in_line)
            .ok_or_else(|| CliError::new(format!("{pin} holds no SSH public key.")))?
    };
    let name = known_hosts_name(&host, port);
    let printed = fingerprint(&format!("{kind} {key}"));
    let existing = match fs::read_to_string(known_hosts) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(CliError::new(format!(
                "Cannot read {}: {error}",
                known_hosts.display()
            )))
        }
    };
    let mut added = true;
    for line in existing.lines() {
        let mut words = line.split_whitespace();
        let (Some(names), Some(listed_kind), Some(listed_key)) =
            (words.next(), words.next(), words.next())
        else {
            continue;
        };
        if !names.split(',').any(|listed| listed == name) || listed_kind != kind {
            continue;
        }
        if listed_key == key {
            added = false;
        } else {
            return Err(CliError::new(format!(
                "{} already holds another {kind} key for {name}. If the host's key really changed, remove the old one first (ssh-keygen -R '{name}').",
                known_hosts.display()
            )));
        }
    }
    if added {
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        let directory = known_hosts.parent().expect("known_hosts has a directory");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .map_err(|error| {
                CliError::new(format!("Cannot create {}: {error}", directory.display()))
            })?;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(known_hosts)
            .map_err(|error| {
                CliError::new(format!("Cannot open {}: {error}", known_hosts.display()))
            })?;
        let separator = if existing.is_empty() || existing.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        file.write_all(format!("{separator}{name} {kind} {key}\n").as_bytes())
            .map_err(|error| {
                CliError::new(format!("Cannot write {}: {error}", known_hosts.display()))
            })?;
    }
    Ok(json!({"name": name, "type": kind, "fingerprint": printed, "added": added}))
}

/// What `silo devices add` was asked to do.
#[derive(Clone, Debug)]
pub struct AddRequest {
    pub address: String,
    pub replace: bool,
    pub host_key: Option<String>,
    /// `~/.ssh/known_hosts`, where `host_key` is pinned.
    pub known_hosts: PathBuf,
}

/// The repair a failed connection needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum Repair {
    /// SSH does not know the host key yet.
    TrustHostKey,
    /// No key this device offers is accepted there.
    InstallKey,
}

fn repair_for(message: &str) -> Option<Repair> {
    if message.contains("host key changed") {
        // A changed key is never trusted from here.
        None
    } else if message.contains("Host key verification failed") {
        Some(Repair::TrustHostKey)
    } else if message.contains("SSH authentication failed") {
        Some(Repair::InstallKey)
    } else {
        None
    }
}

/// `silo devices add`: connects the device; when SSH needs its host key trusted or this
/// device's Silo key installed, runs the engine's command for that here and tries again.
pub async fn add_device(
    engine: &Engine,
    request: &AddRequest,
    terminal: &dyn Terminal,
    progress: &dyn Fn(&str),
) -> Result<Value, CliError> {
    let address = normalize_address(&request.address);
    let address = address.as_str();
    let replace = request.replace;
    let host_key = match &request.host_key {
        Some(pin) => {
            let pinned = pin_host_key(&request.known_hosts, address, pin, terminal)?;
            progress(&format!(
                "{} {} host key {} for {}",
                if pinned["added"] == true {
                    "Trusting"
                } else {
                    "Already trusting"
                },
                pinned["type"].as_str().unwrap_or_default(),
                pinned["fingerprint"].as_str().unwrap_or_default(),
                pinned["name"].as_str().unwrap_or_default()
            ));
            pinned
        }
        None => Value::Null,
    };
    let mut repaired = Vec::new();
    loop {
        let connected = engine
            .call(requests::connections_devices_connect(
                connections::ConnectDeviceParams {
                    address: address.to_owned(),
                    replace: replace.then_some(true),
                },
            ))
            .await;
        let error = match connected {
            Ok(device) => {
                return Ok(json!({"device": device, "repaired": repaired, "hostKey": host_key}));
            }
            Err(error) => error,
        };
        let Some(repair) = repair_for(&error.message).filter(|repair| !repaired.contains(repair))
        else {
            return Err(error);
        };
        let params = connections::SshAddressParams {
            address: address.to_owned(),
        };
        let (command, what) = match repair {
            Repair::TrustHostKey => (
                engine
                    .call(requests::connections_ssh_authorization_command(params))
                    .await?,
                format!("Verify {address}'s SSH host key (compare its fingerprint with the one the host shows: ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub there)"),
            ),
            Repair::InstallKey => (
                engine
                    .call(requests::connections_ssh_key_setup_command(params))
                    .await?,
                format!("Install this device's Silo key on {address} (SSH may ask for that account's password)"),
            ),
        };
        let command = command["command"]
            .as_str()
            .ok_or_else(|| CliError::new("The engine sent no SSH command."))?
            .to_owned();
        if !terminal.interactive() {
            let instead = match repair {
                Repair::TrustHostKey => " (or pass --host-key with the host's key fingerprint)",
                Repair::InstallKey => " (or authorize this device's key on the host: silo devices key here, then silo host init --authorize there)",
            };
            return Err(CliError::new(format!(
                "{} {what}: run this in a terminal, then silo devices add {address} again{instead}:\n{command}",
                error.message
            )));
        }
        progress(&format!("{what}:\n{command}"));
        if !terminal.run(&command)? {
            return Err(CliError::new(format!(
                "{} The SSH command above did not succeed.",
                error.message
            )));
        }
        repaired.push(repair);
    }
}

pub async fn list_devices(engine: &Engine) -> Result<Value, CliError> {
    let listed = engine
        .call(requests::connections_devices_list(EmptyParams {}))
        .await?;
    let health = engine
        .call(requests::connections_devices_health(EmptyParams {}))
        .await
        .unwrap_or(Value::Null);
    let rows = listed
        .as_array()
        .into_iter()
        .flatten()
        .map(|device| {
            let id = device["id"].as_str().unwrap_or_default();
            let state = health["devices"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|row| row["deviceId"] == id);
            json!({
                "id": id,
                "name": device["name"],
                "address": device["address"],
                "connected": state.map(|row| row["connected"].clone()).unwrap_or(Value::Null),
                "error": state.map(|row| row["error"].clone()).unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({"devices": rows}))
}

pub async fn remove_device(engine: &Engine, name: &str) -> Result<Value, CliError> {
    let listed = engine
        .call(requests::connections_devices_list(EmptyParams {}))
        .await?;
    let id = crate::engine::pick_device(&listed, name)?;
    engine
        .call(requests::connections_devices_remove(
            connections::DeviceIdParams {
                device_id: id.clone(),
            },
        ))
        .await?;
    Ok(json!({"removed": id}))
}

pub async fn own_key(engine: &Engine) -> Result<Value, CliError> {
    let own = engine
        .call(requests::connections_keys_own(EmptyParams {}))
        .await?;
    let key = own["publicKey"]
        .as_str()
        .ok_or_else(|| CliError::new("The engine sent no key."))?;
    Ok(json!({"publicKey": key, "fingerprint": fingerprint(key)}))
}

pub fn render_added(value: &Value) -> String {
    let device = &value["device"];
    let name = device["name"].as_str().unwrap_or_default();
    let pinned = match value["hostKey"]["fingerprint"].as_str() {
        Some(print) => format!(
            "Host key {} {print} is in known_hosts for {}.\n",
            value["hostKey"]["type"].as_str().unwrap_or_default(),
            value["hostKey"]["name"].as_str().unwrap_or_default()
        ),
        None => String::new(),
    };
    format!(
        "{pinned}Connected {name} ({}) at {}.\nIts computers: silo list --all-devices, or silo --device \"{name}\" <command>.",
        device["id"].as_str().unwrap_or_default(),
        device["address"].as_str().unwrap_or_default()
    )
}

pub fn render_devices(value: &Value) -> String {
    let rows: Vec<String> = value["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            let state = match (row["connected"].as_bool(), row["error"].as_str()) {
                (Some(true), _) => "connected".to_owned(),
                (_, Some(error)) => format!("offline: {error}"),
                _ => "not checked yet".to_owned(),
            };
            format!(
                "{}\t{}\t{}\t{state}",
                row["name"].as_str().unwrap_or_default(),
                row["address"].as_str().unwrap_or_default(),
                row["id"].as_str().unwrap_or_default()
            )
        })
        .collect();
    if rows.is_empty() {
        "No connected devices. Add one with: silo devices add <user@host>".to_owned()
    } else {
        rows.join("\n")
    }
}

pub fn render_key(value: &Value) -> String {
    format!(
        "{}\n{}\nOn the host, run: silo host init --authorize '{}'",
        value["publicKey"].as_str().unwrap_or_default(),
        value["fingerprint"].as_str().unwrap_or_default(),
        value["publicKey"].as_str().unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kvm_problems_say_what_to_do() {
        let linux = |device, accessible, membership| VmProbe::Linux {
            device,
            accessible,
            group: Some("kvm".into()),
            membership,
        };
        assert!(linux(true, true, Membership::Active).check().is_ok());
        let missing = linux(false, false, Membership::Unknown)
            .check()
            .unwrap_err();
        assert!(missing.contains("/dev/kvm is missing"), "{missing}");
        let absent = linux(true, false, Membership::Absent).check().unwrap_err();
        assert!(absent.contains("sudo usermod -aG kvm"), "{absent}");
        let pending = linux(true, false, Membership::Pending).check().unwrap_err();
        assert!(pending.contains("log out and back in"), "{pending}");
        let odd = linux(true, false, Membership::Active).check().unwrap_err();
        assert!(odd.contains("ls -l /dev/kvm"), "{odd}");
        assert!(VmProbe::Mac {
            apple_silicon: true
        }
        .check()
        .unwrap()
        .contains("2 macOS computers"));
        assert!(VmProbe::Mac {
            apple_silicon: false
        }
        .check()
        .is_err());
    }

    #[test]
    fn a_missing_device_reads_as_missing() {
        let directory = tempfile::tempdir().unwrap();
        let probe = probe_kvm(&directory.path().join("kvm"));
        assert!(matches!(probe, VmProbe::Linux { device: false, .. }));
        // A regular file is not a KVM device, whoever may open it.
        let file = directory.path().join("file");
        fs::write(&file, "").unwrap();
        assert!(matches!(
            probe_kvm(&file),
            VmProbe::Linux { device: false, .. }
        ));
    }

    #[test]
    fn root_and_nobody_are_refused() {
        assert!(check_account(0).is_err());
        assert!(check_account(65534).is_err());
        assert!(check_account(1000).is_ok());
    }

    #[test]
    fn the_service_report_reads_silods_lines() {
        let report = ServiceReport::parse(
            "enabled\nstate: active\nunit: /h/.config/systemd/user/org.silo.dev.engine.service\nprogram: this silod\nlinger: no\n",
            "Lingering is off for ada: ...\n",
        );
        assert_eq!(report.status, "enabled");
        assert_eq!(report.detail("linger"), Some("no"));
        assert_eq!(report.detail("program"), Some("this silod"));
        assert_eq!(
            report.advice,
            vec!["Lingering is off for ada: ...".to_owned()]
        );
        assert_eq!(report.to_json()["state"], "active");
        assert_eq!(ServiceReport::parse("", "").status, "unknown");
    }

    #[test]
    fn keys_come_from_the_argument_or_a_file() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            read_keys(" ssh-ed25519 AAAA laptop ").unwrap(),
            vec!["ssh-ed25519 AAAA laptop".to_owned()]
        );
        let file = directory.path().join("keys.pub");
        fs::write(
            &file,
            "# controllers\nssh-ed25519 AAAA one\n\nssh-ed25519 BBBB two\n",
        )
        .unwrap();
        assert_eq!(
            read_keys(file.to_str().unwrap()).unwrap(),
            vec![
                "ssh-ed25519 AAAA one".to_owned(),
                "ssh-ed25519 BBBB two".to_owned()
            ]
        );
        fs::write(&file, "# nothing\n").unwrap();
        assert!(read_keys(file.to_str().unwrap()).is_err());
        assert!(read_keys("missing-file").is_err());
        assert!(read_keys(directory.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn fingerprints_match_ssh_keygen() {
        // `ssh-keygen -lf` prints SHA256:LH9ifkGkEurhEj7pgiSwRedJtKQomUQeevYOdCTeGWw for it.
        let key =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKVyZDGnacgUkbjXHi/TsIZ+GxIgprlkfONi9t33BGz3 test";
        assert_eq!(
            fingerprint(key).as_deref(),
            Some("SHA256:LH9ifkGkEurhEj7pgiSwRedJtKQomUQeevYOdCTeGWw")
        );
        assert_eq!(fingerprint("ssh-ed25519 !!! x"), None);
        assert_eq!(fingerprint("ssh-ed25519"), None);
    }

    #[test]
    fn addresses_with_a_port_become_ssh_uris() {
        assert_eq!(normalize_address("ada@box:2222"), "ssh://ada@box:2222");
        assert_eq!(normalize_address("127.0.0.1:22"), "ssh://127.0.0.1:22");
        for unchanged in [
            "ada@box",
            "box",
            "ssh://ada@box:2222",
            "[::1]:22",
            "ada@fe80::1",
            "ada@box:x",
        ] {
            assert_eq!(normalize_address(unchanged), unchanged);
        }
        assert_eq!(ssh_target("ada@box").unwrap(), ("box".into(), 22));
        assert_eq!(
            ssh_target("ssh://ada@box:2222").unwrap(),
            ("box".into(), 2222)
        );
        assert_eq!(
            ssh_target("ssh://ada@[::1]:2200").unwrap(),
            ("::1".into(), 2200)
        );
        assert_eq!(ssh_target("ssh://box").unwrap(), ("box".into(), 22));
        assert!(ssh_target("ssh://ada@box:0").is_err());
        assert_eq!(known_hosts_name("box", 22), "box");
        assert_eq!(known_hosts_name("127.0.0.1", 2222), "[127.0.0.1]:2222");
    }

    #[test]
    fn repairs_follow_the_engines_messages() {
        assert_eq!(
            repair_for("Host key verification failed. Connect once with SSH in a terminal to verify and trust the other device's host key."),
            Some(Repair::TrustHostKey)
        );
        assert_eq!(
            repair_for("The other device's SSH host key changed. Verify the device before trusting its new key (Host key verification failed)."),
            None
        );
        assert_eq!(
            repair_for("SSH authentication failed. Set up Silo's SSH key for this device, or configure an SSH key or agent."),
            Some(Repair::InstallKey)
        );
        assert_eq!(repair_for("Cannot connect to Silo over SSH."), None);
    }
}
