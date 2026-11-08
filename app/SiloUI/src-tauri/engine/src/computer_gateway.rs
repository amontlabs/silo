//! The network gateway of a container computer on a Linux host.
//!
//! `silo-macos-gateway` (the name it was built under; it now builds for Linux too) runs
//! MicroSandbox's network stack as a host process: the Public egress policy, header-only secret
//! substitution after upstream TLS verification, a per-computer CA, DNS and published loopback
//! ports. A macOS VM hands it a datagram socketpair as fd 3; a container hands it a TAP device
//! (`IFF_TAP | IFF_NO_PI`) that was opened inside the container's own user and network
//! namespace, so the gateway is the only way out of that namespace. fd 4 is the same revisioned
//! JSON control link in both cases (`init` with `"profile":"container"`, `update`, `stats`,
//! `shutdown`).
//!
//! This module starts and drives one gateway for an already-open TAP descriptor. It creates no
//! namespace and runs no container: the container runtime (a later phase) opens the TAP and
//! calls [`Gateway::start`]. The macOS supervisor
//! (`macos_computers/proxy/supervisor.rs`) speaks the same protocol for VMs and is unchanged.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{self, Read, Write},
    net::Ipv4Addr,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    os::unix::{net::UnixStream, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

/// The gateway executable's file name, beside the engine's own executable.
pub const GATEWAY_BINARY: &str = "silo-macos-gateway";
const PROTOCOL_VERSION: u16 = 1;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const FRAME_FD: i32 = 3;
const CONTROL_FD: i32 = 4;
const START_TIMEOUT: Duration = Duration::from_secs(60);
const UPDATE_TIMEOUT: Duration = Duration::from_secs(6);
const STOP_TIMEOUT: Duration = if cfg!(test) {
    Duration::from_millis(300)
} else {
    Duration::from_secs(5)
};
/// Names the file the gateway appends credential-use events to.
const USAGE_LOG_ENV: &str = "SILO_SECRET_USAGE_LOG";
const NOT_READY: &str = "The computer network gateway did not become ready.";
const UNAVAILABLE: &str = "The computer network gateway is unavailable.";

/// One secret the gateway substitutes for its placeholder (`$MSB_<name>`) toward `allowed_hosts`.
pub struct Secret {
    pub name: String,
    pub value: Zeroizing<String>,
    pub allowed_hosts: Vec<String>,
}

/// What a container's gateway is started with.
pub struct Params {
    pub slot: u16,
    pub guest_ipv4: Ipv4Addr,
    pub guest_mac: [u8; 6],
    pub mtu: u16,
    /// The computer's own TCP ports to publish on the host's loopback (port zero is asked of
    /// the host; the gateway reports the ports it bound).
    pub published_ports: Vec<u16>,
    pub secrets: Vec<Secret>,
    pub github_profile_json: Option<Zeroizing<String>>,
    /// The hosts the GitHub credential covers; `None` is `github.com` and `api.github.com`.
    pub github_hosts: Option<Vec<String>>,
    /// Where the gateway appends credential-use events (names, hosts, methods; never values).
    pub usage_log: Option<PathBuf>,
    /// Where the gateway's own output goes; discarded when absent.
    pub log: Option<std::fs::File>,
}

/// A port the gateway published on the host's loopback.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PublishedPort {
    pub host_port: u16,
    pub guest_port: u16,
    pub protocol: String,
}

/// What the gateway reports once it serves: the identity it gave the computer's side.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Ready {
    pub version: u16,
    #[serde(rename = "type")]
    pub kind: String,
    pub slot: u16,
    pub guest_ipv4: Ipv4Addr,
    pub gateway_ipv4: Ipv4Addr,
    pub prefix_len: u8,
    pub dns_ipv4: Ipv4Addr,
    pub guest_mac: [u8; 6],
    pub gateway_mac: [u8; 6],
    pub mtu: u16,
    pub ca_cert_pem: String,
    pub loopback_ports: Vec<PublishedPort>,
}

#[derive(Serialize)]
struct InitRequest<'a> {
    version: u16,
    #[serde(rename = "type")]
    kind: &'static str,
    profile: &'static str,
    slot: u16,
    guest_ipv4: Ipv4Addr,
    guest_mac: [u8; 6],
    mtu: u16,
    dns_nameservers: Vec<String>,
    intercepted_ports: [u16; 1],
    loopback_ports: Vec<InitPort>,
    ca: CaInput<'a>,
    secrets: Vec<SecretInput<'a>>,
    github_profile_json: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    github_hosts: Option<&'a [String]>,
}

#[derive(Serialize)]
struct CaInput<'a> {
    cert_pem: &'a str,
    key_pem: &'a str,
}

#[derive(Serialize)]
struct InitPort {
    host_port: u16,
    guest_port: u16,
    protocol: &'static str,
}

#[derive(Serialize)]
struct SecretInput<'a> {
    name: &'a str,
    value: &'a str,
    placeholder: String,
    allowed_hosts: &'a [String],
}

/// A running gateway for one container computer.
pub struct Gateway {
    child: Option<Child>,
    control: UnixStream,
    revision: u64,
    ready: Ready,
}

impl Gateway {
    /// Starts the gateway at `binary` with `tap` as its frame descriptor and waits until it
    /// serves. `tap` must be a TAP device opened with `IFF_TAP | IFF_NO_PI` inside the
    /// computer's own network namespace; the gateway refuses any other descriptor.
    pub fn start(
        binary: &Path,
        tap: OwnedFd,
        ca_cert_pem: &str,
        ca_key_pem: &str,
        params: Params,
    ) -> Result<Self, String> {
        let (mut control_parent, control_child) = UnixStream::pair()
            .map_err(|_| "Silo could not create the computer gateway control link.".to_owned())?;
        let tap_fd = duplicate_for_child(tap.as_raw_fd())?;
        let control_fd = duplicate_for_child(control_child.as_raw_fd())?;
        drop(control_child);
        drop(tap);

        let mut command = Command::new(binary);
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(
            match params.log.as_ref().map(std::fs::File::try_clone) {
                Some(Ok(log)) => Stdio::from(log),
                _ => Stdio::null(),
            },
        );
        if let Some(usage) = &params.usage_log {
            command.env(USAGE_LOG_ENV, usage);
        }
        let (frame_source, control_source) = (tap_fd.as_raw_fd(), control_fd.as_raw_fd());
        // SAFETY: pre_exec performs only async-signal-safe descriptor operations.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(frame_source, FRAME_FD) < 0
                    || libc::dup2(control_source, CONTROL_FD) < 0
                {
                    return Err(io::Error::last_os_error());
                }
                libc::close(frame_source);
                libc::close(control_source);
                Ok(())
            });
        }
        let child = command
            .spawn()
            .map_err(|_| "Silo could not start the computer network gateway.".to_owned())?;
        drop(tap_fd);
        drop(control_fd);
        let mut gateway = ChildGuard(Some(child));
        control_parent
            .set_read_timeout(Some(START_TIMEOUT))
            .and_then(|()| control_parent.set_write_timeout(Some(START_TIMEOUT)))
            .map_err(|_| {
                "Silo could not configure the computer gateway control link.".to_owned()
            })?;
        let ready = handshake(&mut control_parent, ca_cert_pem, ca_key_pem, params)?;
        control_parent
            .set_read_timeout(Some(UPDATE_TIMEOUT))
            .and_then(|()| control_parent.set_write_timeout(Some(UPDATE_TIMEOUT)))
            .map_err(|_| {
                "Silo could not configure the computer gateway control link.".to_owned()
            })?;
        Ok(Self {
            child: Some(gateway.take()),
            control: control_parent,
            revision: 0,
            ready,
        })
    }

    /// What the gateway reported: the computer's address, gateway, DNS and published ports.
    pub fn ready(&self) -> &Ready {
        &self.ready
    }

    /// Whether the gateway process is still running.
    pub fn is_running(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|child| child.try_wait().ok().flatten().is_none())
    }

    /// Adds or replaces a secret. The gateway answers after every connection that could still
    /// use the old policy has closed.
    pub fn put_secret(&mut self, secret: &Secret) -> Result<(), String> {
        let operation = serde_json::json!({
            "kind": "put_secret",
            "name": secret.name,
            "value": secret.value.as_str(),
            "placeholder": format!("$MSB_{}", secret.name),
            "allowed_hosts": secret.allowed_hosts,
        });
        self.update(operation)
    }

    pub fn remove_secret(&mut self, name: &str) -> Result<(), String> {
        self.update(serde_json::json!({"kind": "remove_secret", "name": name}))
    }

    pub fn set_github_profile(&mut self, profile_json: Option<&str>) -> Result<(), String> {
        self.update(serde_json::json!({"kind": "set_github_profile", "profile_json": profile_json}))
    }

    /// The gateway's frame counters.
    pub fn stats(&mut self) -> Result<Value, String> {
        self.ensure_running()?;
        let request = serde_json::json!({"version": PROTOCOL_VERSION, "type": "stats"});
        let response = self.exchange(&request)?;
        if response.get("type").and_then(Value::as_str) != Some("stats") {
            return Err(UNAVAILABLE.into());
        }
        Ok(response.get("counters").cloned().unwrap_or(Value::Null))
    }

    /// Asks the gateway to stop, then makes sure it has.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let request = serde_json::json!({"version": PROTOCOL_VERSION, "type": "shutdown"});
        let _ = self.control.set_read_timeout(Some(STOP_TIMEOUT));
        let _ = self.control.set_write_timeout(Some(STOP_TIMEOUT));
        let _ = self.exchange(&request);
        let deadline = Instant::now() + STOP_TIMEOUT;
        while child.try_wait().ok().flatten().is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = child.wait();
    }

    fn ensure_running(&mut self) -> Result<(), String> {
        if self.is_running() {
            Ok(())
        } else {
            Err(UNAVAILABLE.into())
        }
    }

    fn update(&mut self, operation: Value) -> Result<(), String> {
        self.ensure_running()?;
        let revision = self.revision + 1;
        let mut request = serde_json::json!({
            "version": PROTOCOL_VERSION,
            "type": "update",
            "revision": revision,
            "operation": operation,
        });
        let response = self.exchange(&request);
        zeroize_value(&mut request);
        let response = response?;
        let acknowledged = response.get("type").and_then(Value::as_str) == Some("ack")
            && response.get("revision").and_then(Value::as_u64) == Some(revision)
            && response
                .get("closed_previous_generation")
                .and_then(Value::as_bool)
                == Some(true);
        if !acknowledged {
            return Err("The computer network gateway rejected the policy update.".into());
        }
        self.revision = revision;
        Ok(())
    }

    fn exchange(&mut self, request: &Value) -> Result<Value, String> {
        let mut body = Zeroizing::new(
            serde_json::to_vec(request).map_err(|_| "Invalid computer gateway request.")?,
        );
        let result =
            write_frame(&mut self.control, &body).and_then(|()| read_response(&mut self.control));
        body.zeroize();
        result.map_err(|_| UNAVAILABLE.to_owned())
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.stop();
    }
}

fn zeroize_value(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(zeroize_value),
        Value::Object(map) => map.values_mut().for_each(zeroize_value),
        _ => {}
    }
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

/// Sends `init` and returns the validated `ready`.
fn handshake(
    control: &mut UnixStream,
    ca_cert_pem: &str,
    ca_key_pem: &str,
    params: Params,
) -> Result<Ready, String> {
    let Params {
        slot,
        guest_ipv4,
        guest_mac,
        mtu,
        published_ports,
        secrets,
        github_profile_json,
        github_hosts,
        ..
    } = params;
    let request = InitRequest {
        version: PROTOCOL_VERSION,
        kind: "init",
        profile: "container",
        slot,
        guest_ipv4,
        guest_mac,
        mtu,
        dns_nameservers: Vec::new(),
        intercepted_ports: [443],
        loopback_ports: published_ports
            .iter()
            .map(|&guest_port| InitPort {
                host_port: 0,
                guest_port,
                protocol: "tcp",
            })
            .collect(),
        ca: CaInput {
            cert_pem: ca_cert_pem,
            key_pem: ca_key_pem,
        },
        secrets: secrets
            .iter()
            .map(|secret| SecretInput {
                name: &secret.name,
                value: &secret.value,
                placeholder: format!("$MSB_{}", secret.name),
                allowed_hosts: &secret.allowed_hosts,
            })
            .collect(),
        github_profile_json: github_profile_json.as_deref().map(String::as_str),
        github_hosts: github_hosts.as_deref(),
    };
    let mut body = Zeroizing::new(
        serde_json::to_vec(&request)
            .map_err(|_| "Silo could not encode the computer gateway configuration.".to_owned())?,
    );
    let result = write_frame(control, &body).and_then(|()| read_response(control));
    body.zeroize();
    let response = result.map_err(|_| NOT_READY.to_owned())?;
    if response.get("type").and_then(Value::as_str) != Some("ready") {
        return Err("The computer network gateway rejected its configuration.".into());
    }
    let ready: Ready = serde_json::from_value(response)
        .map_err(|_| "The computer network gateway returned invalid readiness data.".to_owned())?;
    validate_ready(
        &ready,
        slot,
        guest_ipv4,
        guest_mac,
        mtu,
        ca_cert_pem,
        &published_ports,
    )?;
    Ok(ready)
}

fn validate_ready(
    ready: &Ready,
    slot: u16,
    guest_ipv4: Ipv4Addr,
    guest_mac: [u8; 6],
    mtu: u16,
    ca_cert_pem: &str,
    published_ports: &[u16],
) -> Result<(), String> {
    let mut requested = published_ports.to_vec();
    requested.sort_unstable();
    let mut reported = ready
        .loopback_ports
        .iter()
        .map(|port| port.guest_port)
        .collect::<Vec<_>>();
    reported.sort_unstable();
    if ready.version != PROTOCOL_VERSION
        || ready.kind != "ready"
        || ready.slot != slot
        || ready.guest_ipv4 != guest_ipv4
        || ready.guest_mac != guest_mac
        || ready.mtu != mtu
        || ready.gateway_ipv4 == guest_ipv4
        || ready.gateway_mac == [0; 6]
        || ready.gateway_mac[0] & 1 != 0
        || ready.ca_cert_pem != ca_cert_pem
        || requested != reported
        || ready
            .loopback_ports
            .iter()
            .any(|port| port.host_port == 0 || port.protocol != "tcp")
    {
        return Err("The computer network gateway returned inconsistent network identity.".into());
    }
    Ok(())
}

/// A CA from the gateway's own generator: the certificate, and the key (zeroized when dropped).
pub fn generate_ca(binary: &Path) -> Result<(String, Zeroizing<String>), String> {
    const FAILED: &str = "Silo could not generate the computer network CA.";
    #[derive(Deserialize)]
    struct Bundle {
        cert_pem: String,
        key_pem: String,
    }
    impl Drop for Bundle {
        fn drop(&mut self) {
            self.key_pem.zeroize();
        }
    }
    let mut output = Command::new(binary)
        .arg("--generate-ca")
        .output()
        .map_err(|_| FAILED.to_owned())?;
    if !output.status.success() || !output.stderr.is_empty() {
        output.stdout.zeroize();
        return Err(FAILED.into());
    }
    let framed = Zeroizing::new(std::mem::take(&mut output.stdout));
    let size = framed
        .get(..4)
        .map(|prefix| u32::from_be_bytes(prefix.try_into().expect("four-byte prefix")) as usize);
    let body = match size {
        Some(size) if size != 0 && size <= MAX_MESSAGE_BYTES && framed.len() == size + 4 => {
            &framed[4..]
        }
        _ => return Err(FAILED.into()),
    };
    let mut bundle: Bundle = serde_json::from_slice(body).map_err(|_| FAILED.to_owned())?;
    let cert = std::mem::take(&mut bundle.cert_pem);
    let key = Zeroizing::new(std::mem::take(&mut bundle.key_pem));
    Ok((cert, key))
}

/// The computer's stable CA, created through the gateway's generator on first use and kept in
/// the engine's credential store (the file store on a Linux host). Returns the certificate and
/// the key.
pub fn computer_ca(
    binary: &Path,
    computer_id: &str,
) -> Result<(String, Zeroizing<String>), String> {
    use crate::macos_computers::proxy_ca;
    let material = proxy_ca::get_or_create(computer_id, || {
        let (cert, key) = generate_ca(binary)?;
        proxy_ca::CaMaterial::from_pem(cert, key.to_string())
    })?;
    Ok((
        material.certificate_pem().to_owned(),
        Zeroizing::new(material.private_key_pem().to_owned()),
    ))
}

fn duplicate_for_child(fd: i32) -> Result<OwnedFd, String> {
    // F_DUPFD_CLOEXEC avoids inheriting any descriptor except the two dup2 targets.
    // SAFETY: `fcntl` is called on an open descriptor and returns a new owned one.
    let duplicated = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
    if duplicated < 0 {
        return Err("Silo could not prepare the computer gateway descriptors.".into());
    }
    // SAFETY: `fcntl` returned a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    const CERT: &str = "-----BEGIN CERTIFICATE-----\nx\n-----END CERTIFICATE-----";
    const KEY: &str = "-----BEGIN PRIVATE KEY-----\nx\n-----END PRIVATE KEY-----";

    fn params(ports: Vec<u16>) -> Params {
        Params {
            slot: 7,
            guest_ipv4: Ipv4Addr::new(172, 30, 0, 6),
            guest_mac: [2, 0x53, 0x49, 0x4c, 0x4f, 7],
            mtu: 1500,
            published_ports: ports,
            secrets: vec![Secret {
                name: "API_KEY".into(),
                value: Zeroizing::new("synthetic-value".into()),
                allowed_hosts: vec!["api.example.test".into()],
            }],
            github_profile_json: None,
            github_hosts: None,
            usage_log: None,
            log: None,
        }
    }

    fn ready_for(request: &Value, host_ports: &[u16]) -> Value {
        let ports = request["loopback_ports"].as_array().unwrap();
        serde_json::json!({
            "type": "ready", "version": 1, "slot": request["slot"],
            "guest_ipv4": request["guest_ipv4"], "gateway_ipv4": "172.30.0.5",
            "prefix_len": 30, "dns_ipv4": "172.30.0.5", "guest_mac": request["guest_mac"],
            "gateway_mac": [2, 0x53, 0x49, 0x4c, 0x4f, 8], "mtu": request["mtu"],
            "ca_cert_pem": request["ca"]["cert_pem"],
            "loopback_ports": ports.iter().zip(host_ports).map(|(port, host)| serde_json::json!({
                "host_port": host, "guest_port": port["guest_port"], "protocol": "tcp",
            })).collect::<Vec<_>>(),
        })
    }

    /// Plays the gateway's side of `init`: records the request and answers `reply`.
    fn serve_init(
        peer: UnixStream,
        reply: impl FnOnce(&Value) -> Value + Send + 'static,
    ) -> thread::JoinHandle<Value> {
        thread::spawn(move || {
            let mut peer = peer;
            let request = read_response(&mut peer).unwrap();
            write_frame(&mut peer, &serde_json::to_vec(&reply(&request)).unwrap()).unwrap();
            request
        })
    }

    #[test]
    fn init_names_the_container_profile_and_the_computers_own_ports() {
        let (mut control, peer) = UnixStream::pair().unwrap();
        let server = serve_init(peer, |request| ready_for(request, &[40001, 40002]));
        let ready = handshake(&mut control, CERT, KEY, params(vec![8080, 3000])).unwrap();
        let request = server.join().unwrap();
        assert_eq!(request["type"], "init");
        assert_eq!(request["profile"], "container");
        assert_eq!(request["intercepted_ports"], serde_json::json!([443]));
        assert_eq!(
            request["loopback_ports"],
            serde_json::json!([
                {"host_port": 0, "guest_port": 8080, "protocol": "tcp"},
                {"host_port": 0, "guest_port": 3000, "protocol": "tcp"},
            ])
        );
        assert_eq!(request["secrets"][0]["placeholder"], "$MSB_API_KEY");
        assert!(request.get("github_hosts").is_none());
        assert_eq!(ready.loopback_ports[0].host_port, 40001);
    }

    #[test]
    fn a_container_without_published_ports_is_valid() {
        let (mut control, peer) = UnixStream::pair().unwrap();
        let server = serve_init(peer, |request| ready_for(request, &[]));
        let ready = handshake(&mut control, CERT, KEY, params(Vec::new())).unwrap();
        server.join().unwrap();
        assert!(ready.loopback_ports.is_empty());
    }

    #[test]
    fn an_inconsistent_ready_is_refused() {
        type Change = fn(&mut Value);
        let changes: [(&str, Change); 6] = [
            ("another address", |ready| {
                ready["guest_ipv4"] = "172.30.0.10".into()
            }),
            ("another certificate", |ready| {
                ready["ca_cert_pem"] = "other".into()
            }),
            ("a multicast gateway mac", |ready| {
                ready["gateway_mac"] = serde_json::json!([1, 0, 0, 0, 0, 1])
            }),
            ("an unbound port", |ready| {
                ready["loopback_ports"][0]["host_port"] = 0.into()
            }),
            ("another port", |ready| {
                ready["loopback_ports"][0]["guest_port"] = 9.into()
            }),
            ("a udp port", |ready| {
                ready["loopback_ports"][0]["protocol"] = "udp".into()
            }),
        ];
        for (label, change) in changes {
            let (mut control, peer) = UnixStream::pair().unwrap();
            let server = serve_init(peer, move |request| {
                let mut ready = ready_for(request, &[40001]);
                change(&mut ready);
                ready
            });
            let result = handshake(&mut control, CERT, KEY, params(vec![8080]));
            server.join().unwrap();
            assert!(result.is_err(), "{label}");
        }
    }

    #[test]
    fn a_rejected_init_is_an_error_without_the_configuration() {
        let (mut control, peer) = UnixStream::pair().unwrap();
        let server = serve_init(
            peer,
            |_| serde_json::json!({"type": "error", "version": 1, "code": "invalid_configuration"}),
        );
        let error = handshake(&mut control, CERT, KEY, params(vec![8080])).unwrap_err();
        server.join().unwrap();
        assert!(!error.contains("synthetic-value"));
        assert!(error.contains("rejected"));
    }

    #[test]
    fn github_hosts_are_sent_only_when_named() {
        let mut named = params(vec![]);
        named.github_hosts = Some(vec![
            "github.com".into(),
            "api.github.com".into(),
            "uploads.github.com".into(),
        ]);
        let (mut control, peer) = UnixStream::pair().unwrap();
        let server = serve_init(peer, |request| ready_for(request, &[]));
        handshake(&mut control, CERT, KEY, named).unwrap();
        let request = server.join().unwrap();
        assert_eq!(request["github_hosts"][2], "uploads.github.com");
    }

    /// A gateway whose process is a long sleep: only the control link is real.
    fn with_sleeping_child(control: UnixStream) -> Gateway {
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        Gateway {
            child: Some(child),
            control,
            revision: 0,
            ready: Ready {
                version: 1,
                kind: "ready".into(),
                slot: 7,
                guest_ipv4: Ipv4Addr::new(172, 30, 0, 6),
                gateway_ipv4: Ipv4Addr::new(172, 30, 0, 5),
                prefix_len: 30,
                dns_ipv4: Ipv4Addr::new(172, 30, 0, 5),
                guest_mac: [2, 0, 0, 0, 0, 7],
                gateway_mac: [2, 0, 0, 0, 0, 8],
                mtu: 1500,
                ca_cert_pem: CERT.into(),
                loopback_ports: Vec::new(),
            },
        }
    }

    #[test]
    fn updates_carry_increasing_revisions_and_wait_for_the_close_barrier() {
        let (control, mut peer) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || {
            let mut seen = Vec::new();
            for answer in [true, true, false] {
                let request = read_response(&mut peer).unwrap();
                let revision = request["revision"].as_u64().unwrap();
                let reply = if answer {
                    serde_json::json!({"type": "ack", "version": 1, "revision": revision, "closed_previous_generation": true})
                } else {
                    serde_json::json!({"type": "error", "version": 1, "revision": revision, "code": "update_rejected"})
                };
                write_frame(&mut peer, &serde_json::to_vec(&reply).unwrap()).unwrap();
                seen.push(request);
            }
            seen
        });
        let mut gateway = with_sleeping_child(control);
        gateway
            .put_secret(&Secret {
                name: "TOKEN_A".into(),
                value: Zeroizing::new("synthetic".into()),
                allowed_hosts: vec!["api.github.com".into()],
            })
            .unwrap();
        gateway.remove_secret("TOKEN_A").unwrap();
        assert!(gateway.set_github_profile(Some("{}")).is_err());
        let seen = server.join().unwrap();
        assert_eq!(
            seen.iter()
                .map(|r| r["revision"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(seen[0]["operation"]["kind"], "put_secret");
        assert_eq!(seen[0]["operation"]["placeholder"], "$MSB_TOKEN_A");
        assert_eq!(seen[1]["operation"]["kind"], "remove_secret");
        assert_eq!(seen[2]["operation"]["profile_json"], "{}");
        // A rejected update does not consume its revision.
        assert_eq!(gateway.revision, 2);
    }

    #[test]
    fn an_ack_without_the_close_barrier_is_not_an_acknowledgement() {
        let (control, mut peer) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || {
            let request = read_response(&mut peer).unwrap();
            let reply = serde_json::json!({"type": "ack", "version": 1, "revision": request["revision"], "closed_previous_generation": false});
            write_frame(&mut peer, &serde_json::to_vec(&reply).unwrap()).unwrap();
        });
        let mut gateway = with_sleeping_child(control);
        assert!(gateway.remove_secret("TOKEN_A").is_err());
        server.join().unwrap();
    }

    #[test]
    fn a_stopped_gateway_is_unavailable_and_shutdown_ends_the_process() {
        let (control, peer) = UnixStream::pair().unwrap();
        let mut gateway = with_sleeping_child(control);
        let pid = gateway.child.as_ref().unwrap().id();
        assert!(gateway.is_running());
        drop(peer);
        gateway.shutdown();
        // SAFETY: signal 0 only checks that the process exists.
        assert_ne!(unsafe { libc::kill(pid as i32, 0) }, 0);

        let (control, _peer) = UnixStream::pair().unwrap();
        let mut gateway = with_sleeping_child(control);
        gateway.child.as_mut().unwrap().kill().unwrap();
        gateway.child.as_mut().unwrap().wait().unwrap();
        assert!(gateway.stats().is_err());
        assert!(gateway.remove_secret("TOKEN_A").is_err());
    }

    #[test]
    fn a_descriptor_that_is_not_a_tap_never_becomes_a_running_gateway() {
        let Some(binary) = std::env::var_os("SILO_TEST_GATEWAY_BINARY") else {
            return;
        };
        // A pipe is not a TAP device: the gateway refuses it before reading any frame.
        let mut ends = [0; 2];
        // SAFETY: `pipe` fills two new descriptors that this test owns.
        let (read, write) = unsafe {
            assert_eq!(libc::pipe(ends.as_mut_ptr()), 0);
            (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1]))
        };
        let (cert, key) = generate_ca(Path::new(&binary)).unwrap();
        let result = Gateway::start(Path::new(&binary), read, &cert, &key, params(vec![8080]));
        drop(write);
        assert!(result.is_err());
    }
}
