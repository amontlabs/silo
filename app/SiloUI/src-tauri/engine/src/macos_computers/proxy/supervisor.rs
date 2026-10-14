//! Supervision and private control for one macOS gateway child per VM.

use super::super::proxy_address::ProxyAddress;
use super::super::proxy_ca::CaMaterial;
use super::super::store::Record;
use super::super::MacosContext;
use super::{mark_unavailable, publish, remove};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    ffi::OsStr,
    io::{self, Read, Write},
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    os::unix::{
        net::{UnixDatagram, UnixStream},
        process::CommandExt,
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak,
    },
    thread,
    time::Duration,
};
use zeroize::{Zeroize, Zeroizing};

const PROTOCOL_VERSION: u16 = 1;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const FRAME_FD: RawFd = 3;
const CONTROL_FD: RawFd = 4;
const START_TIMEOUT: Duration = Duration::from_secs(60);
const UPDATE_TIMEOUT: Duration = Duration::from_secs(6);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const GATEWAY_BINARY: &str = "silo-macos-gateway";

static NEXT_HELPER_GENERATION: AtomicU64 = AtomicU64::new(1);
static ACTIVE: OnceLock<Mutex<HashMap<String, ActiveGateway>>> = OnceLock::new();
static POLICY_LOCKS: OnceLock<Mutex<HashMap<String, Weak<Mutex<()>>>>> = OnceLock::new();

fn active() -> &'static Mutex<HashMap<String, ActiveGateway>> {
    ACTIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn active_lock() -> MutexGuard<'static, HashMap<String, ActiveGateway>> {
    active().lock().unwrap_or_else(PoisonError::into_inner)
}

pub(in crate::macos_computers_engine) fn with_policy_lock<T>(
    computer_id: &str,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let lock = {
        let mut locks = POLICY_LOCKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        locks.retain(|_, lock| lock.strong_count() != 0);
        locks
            .get(computer_id)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(computer_id.to_owned(), Arc::downgrade(&lock));
                lock
            })
    };
    let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
    work()
}

#[derive(Clone)]
struct ActiveGateway {
    vm_generation: u64,
    helper_generation: u64,
    child: Arc<Mutex<GatewayChild>>,
}

/// Child and the guest-facing socket retained by the matching VM configuration.
#[derive(Clone)]
pub(in crate::macos_computers_engine) struct PreparedGateway {
    helper_generation: u64,
    ready: ReadyResponse,
    child: Arc<Mutex<GatewayChild>>,
}

impl PreparedGateway {
    pub(in crate::macos_computers_engine) fn frame_fd(&self) -> Result<RawFd, String> {
        self.child
            .lock()
            .map(|child| child.frame_socket.as_raw_fd())
            .map_err(|_| "The computer gateway is unavailable.".to_owned())
    }

    pub(in crate::macos_computers_engine) fn activate(
        &self,
        computer_id: &str,
        vm_generation: u64,
    ) -> Result<(), String> {
        if self
            .child
            .lock()
            .map_err(|_| "The computer gateway is unavailable.")?
            .child
            .try_wait()
            .map_err(|_| "The computer gateway stopped before the VM was ready.")?
            .is_some()
        {
            return Err("The computer gateway stopped before the VM was ready.".into());
        }
        let current = ActiveGateway {
            vm_generation,
            helper_generation: self.helper_generation,
            child: self.child.clone(),
        };
        let mut active = active_lock();
        if active.contains_key(computer_id) {
            return Err("A computer gateway is already active.".into());
        }
        publish(
            computer_id,
            vm_generation,
            self.helper_generation,
            self.ready.ssh_host_port(),
            self.ready.screen_host_port(),
        )?;
        active.insert(computer_id.to_owned(), current);
        drop(active);
        monitor(
            computer_id.to_owned(),
            vm_generation,
            self.helper_generation,
            self.child.clone(),
        );
        Ok(())
    }

    pub(in crate::macos_computers_engine) fn deactivate(
        &self,
        computer_id: &str,
        vm_generation: u64,
    ) {
        let active = active_lock().get(computer_id).is_some_and(|gateway| {
            gateway.vm_generation == vm_generation
                && gateway.helper_generation == self.helper_generation
        });
        if active {
            stop(computer_id, vm_generation, self.helper_generation);
        } else {
            remove(computer_id, vm_generation, self.helper_generation);
            shutdown_child(self.child.clone());
        }
    }
}

#[derive(Clone, Deserialize)]
struct ReadyResponse {
    version: u16,
    #[serde(rename = "type")]
    kind: String,
    slot: u16,
    guest_ipv4: std::net::Ipv4Addr,
    gateway_ipv4: std::net::Ipv4Addr,
    prefix_len: u8,
    dns_ipv4: std::net::Ipv4Addr,
    guest_mac: [u8; 6],
    gateway_mac: [u8; 6],
    mtu: u16,
    ca_cert_pem: String,
    loopback_ports: Vec<LoopbackPort>,
}

impl ReadyResponse {
    fn ssh_host_port(&self) -> u16 {
        self.loopback_ports
            .iter()
            .find(|port| port.guest_port == 22)
            .map_or(0, |port| port.host_port)
    }

    fn screen_host_port(&self) -> u16 {
        self.loopback_ports
            .iter()
            .find(|port| port.guest_port == 5900)
            .map_or(0, |port| port.host_port)
    }
}

#[derive(Clone, Deserialize)]
struct LoopbackPort {
    host_port: u16,
    guest_port: u16,
    protocol: String,
}

struct GatewayChild {
    child: Child,
    frame_socket: UnixDatagram,
    control: UnixStream,
    revision: u64,
    secrets: HashMap<String, SecretFingerprint>,
    github_policy_revision: u64,
    github_profile: Option<Zeroizing<String>>,
    github_tokens: Vec<Zeroizing<String>>,
    failed: bool,
}

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn take(&mut self) -> Child {
        self.0.take().expect("gateway child guard owns its child")
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

impl Drop for GatewayChild {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

#[derive(Clone, PartialEq, Eq)]
struct SecretFingerprint {
    value_hash: [u8; 32],
    allowed_hosts: Vec<String>,
}

#[derive(Serialize)]
struct CaInput<'a> {
    cert_pem: &'a str,
    key_pem: &'a str,
}

#[derive(Serialize)]
struct SecretInput<'a> {
    name: &'a str,
    value: &'a str,
    placeholder: String,
    allowed_hosts: &'a [String],
}

#[derive(Serialize)]
struct InitRequest<'a> {
    version: u16,
    #[serde(rename = "type")]
    kind: &'static str,
    slot: u16,
    guest_ipv4: std::net::Ipv4Addr,
    guest_mac: [u8; 6],
    mtu: u16,
    dns_nameservers: Vec<String>,
    intercepted_ports: [u16; 1],
    loopback_ports: [InitPort; 2],
    ca: CaInput<'a>,
    secrets: Vec<SecretInput<'a>>,
    github_profile_json: Option<&'a str>,
}

#[derive(Serialize)]
struct InitPort {
    host_port: u16,
    guest_port: u16,
    protocol: &'static str,
}

#[derive(Serialize)]
struct UpdateRequest<'a, T> {
    version: u16,
    #[serde(rename = "type")]
    kind: &'static str,
    revision: u64,
    operation: T,
    #[serde(skip)]
    _marker: std::marker::PhantomData<&'a ()>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum UpdateOperation<'a> {
    PutSecret {
        name: &'a str,
        value: &'a str,
        placeholder: String,
        allowed_hosts: &'a [String],
    },
    RemoveSecret {
        name: &'a str,
    },
    SetGithubProfile {
        profile_json: Option<&'a str>,
    },
}

/// Start one helper with only its private frame and control descriptor handoff.
pub(in crate::macos_computers_engine) fn prepare(
    app: &MacosContext,
    record: &Record,
    address: ProxyAddress,
    ca: CaMaterial,
    material: Vec<(String, String, Vec<String>)>,
    github_policy_revision: u64,
    github_profile: Option<String>,
) -> Result<PreparedGateway, String> {
    crate::runtime::validate_secret_material(&material)?;
    let binary = gateway_binary(app)?;
    let helper_generation = NEXT_HELPER_GENERATION.fetch_add(1, Ordering::SeqCst);
    let (frame_parent, frame_child) = UnixDatagram::pair()
        .map_err(|_| "Silo could not create the computer gateway frame link.".to_owned())?;
    let (mut control_parent, control_child) = UnixStream::pair()
        .map_err(|_| "Silo could not create the computer gateway control link.".to_owned())?;
    let frame_child_fd = duplicate_for_child(frame_child.as_raw_fd())?;
    let control_child_fd = duplicate_for_child(control_child.as_raw_fd())?;
    drop(frame_child);
    drop(control_child);

    let mut command = Command::new(binary);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("MSB_LIBKRUNFW_PATH", bundled_runtime_library(app)?);
    let frame_source = frame_child_fd.as_raw_fd();
    let control_source = control_child_fd.as_raw_fd();
    // SAFETY: pre_exec performs only async-signal-safe descriptor operations.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(frame_source, FRAME_FD) < 0 || libc::dup2(control_source, CONTROL_FD) < 0
            {
                return Err(io::Error::last_os_error());
            }
            libc::close(frame_source);
            libc::close(control_source);
            Ok(())
        });
    }
    let mut child =
        ChildGuard(Some(command.spawn().map_err(|_| {
            "Silo could not start the computer network gateway.".to_owned()
        })?));
    drop(frame_child_fd);
    drop(control_child_fd);
    control_parent
        .set_read_timeout(Some(START_TIMEOUT))
        .and_then(|_| control_parent.set_write_timeout(Some(START_TIMEOUT)))
        .map_err(|_| "Silo could not configure the computer gateway control link.".to_owned())?;

    let mut secret_values = material
        .into_iter()
        .map(|(name, value, allowed_hosts)| (name, Zeroizing::new(value), allowed_hosts))
        .collect::<Vec<_>>();
    let initial_secrets = secret_values
        .iter()
        .map(|(name, value, allowed_hosts)| {
            (
                name.clone(),
                SecretFingerprint {
                    value_hash: Sha256::digest(value.as_bytes()).into(),
                    allowed_hosts: allowed_hosts.clone(),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let github_profile = github_profile.map(Zeroizing::new);
    let github_tokens = token_values(github_profile.as_deref().map(String::as_str));
    let secrets = secret_values
        .iter()
        .map(|(name, value, allowed_hosts)| SecretInput {
            name,
            value,
            placeholder: format!("$MSB_{name}"),
            allowed_hosts,
        })
        .collect::<Vec<_>>();
    let guest_mac = parse_mac(&record.mac_address)?;
    let request = InitRequest {
        version: PROTOCOL_VERSION,
        kind: "init",
        slot: address.slot(),
        guest_ipv4: address.guest(),
        guest_mac,
        mtu: 1500,
        dns_nameservers: Vec::new(),
        intercepted_ports: [443],
        loopback_ports: [
            InitPort {
                host_port: 0,
                guest_port: 22,
                protocol: "tcp",
            },
            InitPort {
                host_port: 0,
                guest_port: 5900,
                protocol: "tcp",
            },
        ],
        ca: CaInput {
            cert_pem: ca.certificate_pem(),
            key_pem: ca.private_key_pem(),
        },
        secrets,
        github_profile_json: github_profile.as_deref().map(String::as_str),
    };
    let mut body = Zeroizing::new(
        serde_json::to_vec(&request)
            .map_err(|_| "Silo could not encode the computer gateway configuration.".to_owned())?,
    );
    let result =
        write_frame(&mut control_parent, &body).and_then(|()| read_response(&mut control_parent));
    body.zeroize();
    secret_values.clear();

    let response = match result {
        Ok(response) => response,
        Err(_) => {
            return Err("The computer network gateway did not become ready.".into());
        }
    };
    if response.get("type").and_then(Value::as_str) != Some("ready") {
        return Err("The computer network gateway rejected its configuration.".into());
    }
    let ready: ReadyResponse = serde_json::from_value(response)
        .map_err(|_| "The computer network gateway returned invalid readiness data.".to_owned())?;
    validate_ready(&ready, address, guest_mac, ca.certificate_pem())?;

    let child_state = GatewayChild {
        child: child.take(),
        frame_socket: frame_parent,
        control: control_parent,
        revision: 0,
        secrets: initial_secrets,
        github_policy_revision,
        github_tokens,
        github_profile,
        failed: false,
    };
    drop(ca);
    Ok(PreparedGateway {
        helper_generation,
        ready,
        child: Arc::new(Mutex::new(child_state)),
    })
}

/// Generate a CA by asking the pinned gateway build's local certificate generator.
#[allow(dead_code)] // Provisioning calls this only when it stages the initial CA.
pub(in crate::macos_computers_engine) fn generate_ca(
    app: &MacosContext,
) -> Result<CaMaterial, String> {
    let binary = gateway_binary(app)?;
    let mut output = Command::new(binary)
        .arg(OsStr::new("--generate-ca"))
        .output()
        .map_err(|_| "Silo could not generate the computer network CA.".to_owned())?;
    if !output.status.success() || !output.stderr.is_empty() {
        return Err("Silo could not generate the computer network CA.".into());
    }
    let decoded = decode_frame(&output.stdout);
    output.stdout.zeroize();
    let response = Zeroizing::new(decoded?);
    let mut bundle: GeneratedCa = serde_json::from_slice(&response)
        .map_err(|_| "The computer network CA generator returned invalid data.".to_owned())?;
    let cert = std::mem::take(&mut bundle.cert_pem);
    let key = std::mem::take(&mut bundle.key_pem);
    CaMaterial::from_pem(cert, key)
}

/// Asks the pinned gateway build to sign a certificate for the trust-probe name with this
/// computer's CA. The guest evaluates it to prove its system trust accepts the CA; the
/// leaf's key is discarded by the helper.
pub(in crate::macos_computers_engine) fn issue_trust_probe(
    app: &MacosContext,
    ca: &CaMaterial,
) -> Result<String, String> {
    let binary = gateway_binary(app)?;
    let request = trust_probe_request(ca)?;
    let mut child = Command::new(binary)
        .arg(OsStr::new("--issue-trust-probe"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| TRUST_PROBE_FAILED.to_owned())?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| TRUST_PROBE_FAILED.to_owned())?;
    let written = stdin.write_all(&request);
    drop(stdin);
    drop(request);
    let output = child
        .wait_with_output()
        .map_err(|_| TRUST_PROBE_FAILED.to_owned())?;
    written.map_err(|_| TRUST_PROBE_FAILED.to_owned())?;
    if !output.status.success() || !output.stderr.is_empty() {
        return Err(TRUST_PROBE_FAILED.into());
    }
    parse_trust_probe_response(&output.stdout)
}

const TRUST_PROBE_FAILED: &str = "Silo could not prepare the computer's network trust check.";

/// The framed CA bundle the helper reads on stdin. It holds the CA key, so it is zeroized.
fn trust_probe_request(ca: &CaMaterial) -> Result<Zeroizing<Vec<u8>>, String> {
    #[derive(Serialize)]
    struct Request<'a> {
        cert_pem: &'a str,
        key_pem: &'a str,
    }
    let mut body = serde_json::to_vec(&Request {
        cert_pem: ca.certificate_pem(),
        key_pem: ca.private_key_pem(),
    })
    .map_err(|_| TRUST_PROBE_FAILED.to_owned())?;
    if body.is_empty() || body.len() > MAX_MESSAGE_BYTES {
        body.zeroize();
        return Err(TRUST_PROBE_FAILED.into());
    }
    let mut framed = Zeroizing::new(Vec::with_capacity(body.len() + 4));
    framed.extend_from_slice(&(body.len() as u32).to_be_bytes());
    framed.extend_from_slice(&body);
    body.zeroize();
    Ok(framed)
}

/// The probe certificate from the helper's framed response: exactly one certificate and no
/// key material.
fn parse_trust_probe_response(bytes: &[u8]) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Response {
        leaf_pem: String,
    }
    let invalid = || "The computer network trust check returned invalid data.".to_owned();
    if bytes.len() < 4 {
        return Err(invalid());
    }
    let size = u32::from_be_bytes(bytes[..4].try_into().expect("four-byte prefix")) as usize;
    if size == 0 || size > MAX_MESSAGE_BYTES || bytes.len() != size + 4 {
        return Err(invalid());
    }
    let response: Response = serde_json::from_slice(&bytes[4..]).map_err(|_| invalid())?;
    let pem = response.leaf_pem;
    if pem.matches("-----BEGIN CERTIFICATE-----").count() != 1
        || pem.matches("-----END CERTIFICATE-----").count() != 1
        || pem.contains("PRIVATE KEY")
        || pem.len() > 16 * 1024
    {
        return Err(invalid());
    }
    Ok(pem)
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct GeneratedCa {
    cert_pem: String,
    key_pem: String,
}

impl Drop for GeneratedCa {
    fn drop(&mut self) {
        self.key_pem.zeroize();
    }
}

fn gateway_binary(app: &MacosContext) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(current) = std::env::current_exe() {
        if let Some(directory) = current.parent() {
            candidates.push(directory.join(GATEWAY_BINARY));
        }
    }
    candidates.push(app.paths().resources.join(GATEWAY_BINARY));
    candidates.push(app.paths().resources.join("binaries").join(GATEWAY_BINARY));
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| "The signed computer network gateway is missing from this build.".into())
}

fn bundled_runtime_library(app: &MacosContext) -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|_| "Silo could not locate the bundled network runtime library.".to_owned())?;
    let library = crate::runtime::bundled_runtime_library(
        &executable,
        &app.paths().resources,
        crate::runtime::RuntimePackageKind::MacApp,
    );
    if library.is_file() {
        Ok(library)
    } else {
        Err("The bundled network runtime library is missing from this build.".into())
    }
}

fn duplicate_for_child(fd: RawFd) -> Result<OwnedFd, String> {
    // F_DUPFD_CLOEXEC avoids inheriting any descriptor except the two dup2 targets below.
    let duplicated = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
    if duplicated < 0 {
        return Err("Silo could not prepare the computer gateway descriptors.".into());
    }
    // SAFETY: `fcntl` returned a new owned descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
}

fn parse_mac(value: &str) -> Result<[u8; 6], String> {
    let octets = value
        .split(':')
        .map(|octet| u8::from_str_radix(octet, 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "The computer's network address is damaged.".to_owned())?;
    octets
        .try_into()
        .map_err(|_| "The computer's network address is damaged.".to_owned())
}

fn validate_ready(
    ready: &ReadyResponse,
    address: ProxyAddress,
    guest_mac: [u8; 6],
    ca_certificate_pem: &str,
) -> Result<(), String> {
    if ready.version != PROTOCOL_VERSION
        || ready.kind != "ready"
        || ready.slot != address.slot()
        || ready.guest_ipv4 != address.guest()
        || ready.gateway_ipv4 != address.host()
        || ready.dns_ipv4 != address.host()
        || ready.prefix_len != 30
        || ready.guest_mac != guest_mac
        || ready.gateway_mac == [0; 6]
        || ready.gateway_mac[0] & 1 != 0
        || ready.mtu != 1500
        || ready.ca_cert_pem != ca_certificate_pem
        || ready.ssh_host_port() == 0
        || ready.screen_host_port() == 0
        || ready.ssh_host_port() == ready.screen_host_port()
        || ready.loopback_ports.len() != 2
        || ready
            .loopback_ports
            .iter()
            .any(|port| port.protocol != "tcp")
    {
        return Err("The computer network gateway returned inconsistent network identity.".into());
    }
    Ok(())
}

fn write_frame(stream: &mut UnixStream, body: &[u8]) -> io::Result<()> {
    if body.is_empty() || body.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid control frame",
        ));
    }
    stream.write_all(&(body.len() as u32).to_be_bytes())?;
    stream.write_all(body)
}

fn read_response(stream: &mut UnixStream) -> io::Result<Value> {
    let mut prefix = [0u8; 4];
    stream.read_exact(&mut prefix)?;
    let size = u32::from_be_bytes(prefix) as usize;
    if size == 0 || size > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid control frame",
        ));
    }
    let mut body = Zeroizing::new(vec![0; size]);
    stream.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid control response"))
}

#[allow(dead_code)]
fn decode_frame(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < 4 {
        return Err("The computer network CA generator returned invalid data.".into());
    }
    let size = u32::from_be_bytes(bytes[..4].try_into().expect("four-byte prefix")) as usize;
    if size == 0 || size > MAX_MESSAGE_BYTES || bytes.len() != size + 4 {
        return Err("The computer network CA generator returned invalid data.".into());
    }
    Ok(bytes[4..].to_vec())
}

fn shutdown_child(child: Arc<Mutex<GatewayChild>>) {
    thread::spawn(move || {
        if let Ok(mut child) = child.lock() {
            let _ = child.control.set_read_timeout(Some(SHUTDOWN_TIMEOUT));
            let _ = child.control.set_write_timeout(Some(SHUTDOWN_TIMEOUT));
            let body = serde_json::json!({"version": PROTOCOL_VERSION, "type":"shutdown"});
            let mut bytes = serde_json::to_vec(&body).unwrap_or_default();
            let _ = write_frame(&mut child.control, &bytes);
            bytes.zeroize();
            let _ = read_response(&mut child.control);
            if child.child.try_wait().ok().flatten().is_none() {
                let _ = child.child.kill();
            }
            let _ = child.child.wait();
            child.failed = true;
        }
    });
}

fn monitor(
    computer_id: String,
    vm_generation: u64,
    helper_generation: u64,
    child: Arc<Mutex<GatewayChild>>,
) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(500));
        let exited = child
            .lock()
            .map(|mut child| child.child.try_wait().ok().flatten().is_some())
            .unwrap_or(true);
        if exited {
            mark_unavailable(&computer_id, vm_generation, helper_generation);
            break;
        }
        if !active_lock().get(&computer_id).is_some_and(|current| {
            current.vm_generation == vm_generation && current.helper_generation == helper_generation
        }) {
            break;
        }
    });
}

pub(in crate::macos_computers_engine) fn stop(
    computer_id: &str,
    vm_generation: u64,
    helper_generation: u64,
) {
    let child = {
        let mut active = active_lock();
        let matches = active.get(computer_id).is_some_and(|gateway| {
            gateway.vm_generation == vm_generation && gateway.helper_generation == helper_generation
        });
        if matches {
            active.remove(computer_id).map(|gateway| gateway.child)
        } else {
            None
        }
    };
    remove(computer_id, vm_generation, helper_generation);
    if let Some(child) = child {
        shutdown_child(child);
    }
}

pub(in crate::macos_computers_engine) fn apply_secrets(
    computer: &str,
    material: Vec<(String, String, Vec<String>)>,
) -> Result<Vec<String>, String> {
    crate::runtime::validate_secret_material(&material)?;
    let Some(gateway) = active_lock().get(computer).cloned() else {
        return Ok(material.into_iter().map(|(name, _, _)| name).collect());
    };
    let mut child = gateway
        .child
        .lock()
        .map_err(|_| "The computer network gateway is unavailable.")?;
    ensure_alive(&mut child, computer, &gateway)?;
    let mut desired = HashMap::new();
    let mut values = HashMap::new();
    for (name, value, allowed_hosts) in material {
        let fingerprint = SecretFingerprint {
            value_hash: Sha256::digest(value.as_bytes()).into(),
            allowed_hosts: allowed_hosts.clone(),
        };
        desired.insert(name.clone(), fingerprint);
        values.insert(name, Zeroizing::new(value));
    }
    let mut pending = Vec::new();
    let removals = child
        .secrets
        .keys()
        .filter(|name| !desired.contains_key(*name))
        .cloned()
        .collect::<Vec<_>>();
    for name in removals {
        apply_update(
            &mut child,
            computer,
            &gateway,
            UpdateOperation::RemoveSecret { name: &name },
        )?;
        child.secrets.remove(&name);
    }
    for (name, fingerprint) in desired {
        match child.secrets.get(&name) {
            None => pending.push(name),
            Some(current) if current == &fingerprint => {}
            Some(_) => {
                let value = values
                    .get(&name)
                    .ok_or("Secret material changed while applying the computer policy.")?;
                let allowed_hosts = &fingerprint.allowed_hosts;
                apply_update(
                    &mut child,
                    computer,
                    &gateway,
                    UpdateOperation::PutSecret {
                        name: &name,
                        value,
                        placeholder: format!("$MSB_{name}"),
                        allowed_hosts,
                    },
                )?;
                child.secrets.insert(name, fingerprint);
            }
        }
    }
    pending.sort();
    Ok(pending)
}

pub(in crate::macos_computers_engine) fn revoke_secret(
    computer: &str,
    name: &str,
) -> Result<bool, String> {
    let Some(gateway) = active_lock().get(computer).cloned() else {
        return Ok(true);
    };
    let mut child = gateway
        .child
        .lock()
        .map_err(|_| "The computer network gateway is unavailable.")?;
    ensure_alive(&mut child, computer, &gateway)?;
    if !child.secrets.contains_key(name) {
        return Ok(true);
    }
    apply_update(
        &mut child,
        computer,
        &gateway,
        UpdateOperation::RemoveSecret { name },
    )?;
    child.secrets.remove(name);
    Ok(true)
}

pub(in crate::macos_computers_engine) fn apply_github_profile(
    computer: &str,
    policy_revision: u64,
    profile: &Value,
) -> Result<(), String> {
    apply_github_profile_state(computer, policy_revision, Some(profile))
}

pub(in crate::macos_computers_engine) fn apply_github_profile_state(
    computer: &str,
    policy_revision: u64,
    profile: Option<&Value>,
) -> Result<(), String> {
    let Some(gateway) = active_lock().get(computer).cloned() else {
        return Ok(());
    };
    let mut child = gateway
        .child
        .lock()
        .map_err(|_| "The computer network gateway is unavailable.")?;
    ensure_alive(&mut child, computer, &gateway)?;
    if policy_revision < child.github_policy_revision {
        return Ok(());
    }
    let mut serialized = profile
        .map(|profile| {
            serde_json::to_string(profile)
                .map(Zeroizing::new)
                .map_err(|_| "The GitHub access profile is invalid.".to_owned())
        })
        .transpose()?;
    let operation = UpdateOperation::SetGithubProfile {
        profile_json: serialized.as_deref().map(String::as_str),
    };
    apply_update(&mut child, computer, &gateway, operation)?;
    child.github_policy_revision = policy_revision;
    child.github_tokens = token_values(serialized.as_deref().map(String::as_str));
    if let Some(current) = &mut child.github_profile {
        current.zeroize();
    }
    child.github_profile = serialized.take();
    Ok(())
}

pub(in crate::macos_computers_engine) fn github_profile_is_cached(
    computer: &str,
    profile: &Value,
) -> Result<bool, String> {
    let Some(gateway) = active_lock().get(computer).cloned() else {
        return Ok(false);
    };
    let expected = serde_json::to_string(profile).map_err(|_| "Invalid GitHub profile.")?;
    let mut child = gateway
        .child
        .lock()
        .map_err(|_| "The computer network gateway is unavailable.")?;
    Ok(!child.failed
        && child.child.try_wait().ok().flatten().is_none()
        && child
            .github_profile
            .as_deref()
            .is_some_and(|current| current.as_str() == expected))
}

pub(in crate::macos_computers_engine) fn scoped_cached_tokens(
    computer: &str,
) -> Result<Vec<String>, String> {
    let Some(gateway) = active_lock().get(computer).cloned() else {
        return Ok(Vec::new());
    };
    let child = gateway
        .child
        .lock()
        .map_err(|_| "The computer network gateway is unavailable.")?;
    Ok(child
        .github_tokens
        .iter()
        .map(|token| token.to_string())
        .collect())
}

/// Return only names in the current helper policy; secret values stay in the child.
pub(in crate::macos_computers_engine) fn guest_secret_names(
    computer: &str,
) -> Result<Option<Vec<String>>, String> {
    let Some(gateway) = active_lock().get(computer).cloned() else {
        return Ok(None);
    };
    let mut child = gateway
        .child
        .lock()
        .map_err(|_| "The computer network gateway is unavailable.")?;
    ensure_alive(&mut child, computer, &gateway)?;
    let mut names = child.secrets.keys().cloned().collect::<Vec<_>>();
    names.sort();
    Ok(Some(names))
}

fn token_values(profile: Option<&str>) -> Vec<Zeroizing<String>> {
    let Some(profile) = profile else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(profile) else {
        return Vec::new();
    };
    let mut tokens = Vec::new();
    if let Some(token) = value.get("personalToken").and_then(Value::as_str) {
        tokens.push(Zeroizing::new(token.to_owned()));
    }
    if let Some(owners) = value.get("owners").and_then(Value::as_array) {
        for owner in owners {
            for field in ["readToken", "writeToken"] {
                if let Some(token) = owner.get(field).and_then(Value::as_str) {
                    tokens.push(Zeroizing::new(token.to_owned()));
                }
            }
        }
    }
    tokens
}

fn apply_update(
    child: &mut GatewayChild,
    computer: &str,
    gateway: &ActiveGateway,
    operation: UpdateOperation<'_>,
) -> Result<(), String> {
    child.revision = child.revision.wrapping_add(1).max(1);
    let request = UpdateRequest {
        version: PROTOCOL_VERSION,
        kind: "update",
        revision: child.revision,
        operation,
        _marker: std::marker::PhantomData,
    };
    let mut body = Zeroizing::new(
        serde_json::to_vec(&request)
            .map_err(|_| "Silo could not encode a network policy update.".to_owned())?,
    );
    let result = child
        .control
        .set_read_timeout(Some(UPDATE_TIMEOUT))
        .and_then(|_| child.control.set_write_timeout(Some(UPDATE_TIMEOUT)))
        .and_then(|_| write_frame(&mut child.control, &body))
        .and_then(|_| read_response(&mut child.control));
    body.zeroize();
    match result {
        Ok(response)
            if response.get("type").and_then(Value::as_str) == Some("ack")
                && response.get("version").and_then(Value::as_u64)
                    == Some(u64::from(PROTOCOL_VERSION))
                && response.get("revision").and_then(Value::as_u64) == Some(child.revision)
                && response
                    .get("closed_previous_generation")
                    .and_then(Value::as_bool)
                    == Some(true) =>
        {
            Ok(())
        }
        _ => {
            invalidate(child, computer, gateway);
            Err("The computer network policy update did not close previous connections.".into())
        }
    }
}

fn ensure_alive(
    child: &mut GatewayChild,
    computer: &str,
    gateway: &ActiveGateway,
) -> Result<(), String> {
    if child.failed || child.child.try_wait().ok().flatten().is_some() {
        invalidate(child, computer, gateway);
        return Err("The computer network gateway is unavailable.".into());
    }
    Ok(())
}

fn invalidate(child: &mut GatewayChild, computer: &str, gateway: &ActiveGateway) {
    child.failed = true;
    mark_unavailable(computer, gateway.vm_generation, gateway.helper_generation);
    if child.child.try_wait().ok().flatten().is_none() {
        let _ = child.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ca() -> CaMaterial {
        CaMaterial::from_pem(
            "-----BEGIN CERTIFICATE-----\nY2VydA==\n-----END CERTIFICATE-----\n".into(),
            "-----BEGIN PRIVATE KEY-----\na2V5\n-----END PRIVATE KEY-----\n".into(),
        )
        .unwrap()
    }

    fn frame(body: &str) -> Vec<u8> {
        let mut bytes = (body.len() as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(body.as_bytes());
        bytes
    }

    #[test]
    fn the_trust_probe_request_frames_the_ca_for_the_helper() {
        let request = trust_probe_request(&ca()).unwrap();
        let size = u32::from_be_bytes(request[..4].try_into().unwrap()) as usize;
        assert_eq!(request.len(), size + 4);
        let json: Value = serde_json::from_slice(&request[4..]).unwrap();
        assert_eq!(json["cert_pem"], ca().certificate_pem());
        assert_eq!(json["key_pem"], ca().private_key_pem());
    }

    #[test]
    fn the_trust_probe_response_must_be_one_certificate_and_no_key() {
        let leaf = "-----BEGIN CERTIFICATE-----\nbGVhZg==\n-----END CERTIFICATE-----\n";
        let ok = frame(&serde_json::json!({ "leaf_pem": leaf }).to_string());
        assert_eq!(parse_trust_probe_response(&ok).unwrap(), leaf);

        for body in [
            serde_json::json!({ "leaf_pem": format!("{leaf}{leaf}") }),
            serde_json::json!({ "leaf_pem": format!("{leaf}-----BEGIN PRIVATE KEY-----") }),
            serde_json::json!({ "leaf_pem": "" }),
            serde_json::json!({ "other": leaf }),
        ] {
            assert!(parse_trust_probe_response(&frame(&body.to_string())).is_err());
        }
        assert!(parse_trust_probe_response(&[]).is_err());
        assert!(parse_trust_probe_response(&ok[..ok.len() - 1]).is_err());
    }
}
