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
//! namespace and runs no container: the container runtime
//! ([`crate::containers::lifecycle`]) opens the TAP and calls [`Gateway::start`]. The macOS supervisor
//! (`macos_computers/proxy/supervisor.rs`) speaks the same protocol for VMs; the framing, the
//! descriptor handoff, the readiness checks and the acknowledgement rule are shared in
//! [`crate::gateway_control`].
//!
//! The gateway fails closed. An update or stats exchange that times out, is answered with
//! anything but its own acknowledgement, or hits a broken link stops the gateway, and every
//! later call answers "unavailable": a refused or half-applied secret change must never look
//! applied, and a late acknowledgement must never be read as the answer to the next request.
//! A stopped gateway is not restarted: the container's registry sees it is gone, the computer
//! reads as stopped (its network is gone, which fails closed), and a start brings up a new
//! gateway with the secrets and GitHub access the engine holds.
//!
//! Dropping a [`Gateway`], [`Gateway::shutdown`] and a failed update all wait for the process,
//! and a gateway that hangs takes up to about 10 s to stop (5 s for the shutdown exchange and
//! 5 s for the process to exit, then it is killed). The caller must therefore not drop a
//! `Gateway` on an async worker thread; stop it on a blocking thread.

use crate::gateway_control::{
    duplicate_for_child, is_update_ack, pass_descriptors, read_response, validate_ready,
    write_frame, ChildGuard, Expected, PROTOCOL_VERSION, USAGE_LOG_ENV,
};
pub use crate::gateway_control::{generate_ca, PublishedPort, Ready};
use serde::Serialize;
use serde_json::Value;
use std::{
    net::Ipv4Addr,
    os::fd::{AsRawFd, OwnedFd},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

/// The gateway executable's file name, beside the engine's own executable.
pub const GATEWAY_BINARY: &str = "silo-macos-gateway";
const START_TIMEOUT: Duration = Duration::from_secs(60);
const UPDATE_TIMEOUT: Duration = if cfg!(test) {
    Duration::from_millis(300)
} else {
    Duration::from_secs(6)
};
const STOP_TIMEOUT: Duration = if cfg!(test) {
    Duration::from_millis(300)
} else {
    Duration::from_secs(5)
};
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
    /// The gateway's own address on the computer's /30 link (also its DNS server), which the
    /// gateway must report back.
    pub gateway_ipv4: Ipv4Addr,
    pub guest_mac: [u8; 6],
    pub mtu: u16,
    /// The computer's own TCP ports to publish on the host's loopback (port zero is asked of
    /// the host; the gateway reports the ports it bound).
    pub published_ports: Vec<u16>,
    pub secrets: Vec<Secret>,
    pub github_profile_json: Option<Zeroizing<String>>,
    /// The hosts the GitHub credential covers; `None` is `github.com` and `api.github.com`.
    pub github_hosts: Option<Vec<String>>,
    /// The network policy, in the shape MicroSandbox saves one; `None` is the gateway's own
    /// (public addresses only).
    pub policy: Option<Value>,
    /// Strict hostname-policy enforcement; `None` is the gateway's own (on). A policy with a
    /// list of allowed hosts needs it off.
    pub strict: Option<bool>,
    /// Where the gateway appends credential-use events (names, hosts, methods; never values).
    pub usage_log: Option<PathBuf>,
    /// Where the gateway's own output goes; discarded when absent.
    pub log: Option<std::fs::File>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    policy: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    strict: Option<bool>,
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
        pass_descriptors(&mut command, tap_fd.as_raw_fd(), control_fd.as_raw_fd());
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
            self.fail();
            return Err(UNAVAILABLE.into());
        }
        Ok(response.get("counters").cloned().unwrap_or(Value::Null))
    }

    /// The TCP ports published on the host's loopback now: the ones named at start and the ones
    /// added since, with the host port the gateway bound for each.
    pub fn ports(&mut self) -> Result<Vec<PublishedPort>, String> {
        self.port_request(serde_json::json!({"version": PROTOCOL_VERSION, "type": "ports"}))
    }

    /// Publishes a guest TCP port on the host's loopback (`host_port` 0 lets the gateway pick),
    /// and returns the ports published after it.
    pub fn add_port(
        &mut self,
        guest_port: u16,
        host_port: u16,
    ) -> Result<Vec<PublishedPort>, String> {
        self.port_request(serde_json::json!({
            "version": PROTOCOL_VERSION, "type": "port_add",
            "guest_port": guest_port, "host_port": host_port,
        }))
    }

    pub fn remove_port(&mut self, guest_port: u16) -> Result<Vec<PublishedPort>, String> {
        self.port_request(serde_json::json!({
            "version": PROTOCOL_VERSION, "type": "port_remove", "guest_port": guest_port,
        }))
    }

    /// Whether the gateway can open a connection to the guest port through its listener.
    pub fn probe_port(&mut self, guest_port: u16) -> Result<bool, String> {
        self.ensure_running()?;
        let request = serde_json::json!({
            "version": PROTOCOL_VERSION, "type": "port_probe", "guest_port": guest_port,
        });
        let response = self.exchange(&request)?;
        match (
            response.get("type").and_then(Value::as_str),
            response.get("reachable").and_then(Value::as_bool),
        ) {
            (Some("port_probe"), Some(reachable)) if has_version(&response) => Ok(reachable),
            _ => self.unexpected(&response),
        }
    }

    fn port_request(&mut self, request: Value) -> Result<Vec<PublishedPort>, String> {
        self.ensure_running()?;
        let response = self.exchange(&request)?;
        if response.get("type").and_then(Value::as_str) != Some("ports") || !has_version(&response)
        {
            return self.unexpected(&response);
        }
        serde_json::from_value::<Vec<PublishedPort>>(
            response.get("ports").cloned().unwrap_or(Value::Null),
        )
        .ok()
        .filter(|ports| ports.iter().all(|port| port.protocol == "tcp"))
        .map_or_else(|| self.unexpected(&response), Ok)
    }

    /// A refusal the gateway reports in order (`port_rejected`) leaves the link in step; any
    /// other answer means it is no longer the answer to this request, so the gateway stops.
    fn unexpected<T>(&mut self, response: &Value) -> Result<T, String> {
        if response.get("type").and_then(Value::as_str) == Some("error")
            && response.get("code").and_then(Value::as_str) == Some("port_rejected")
            && has_version(response)
        {
            return Err("The computer network gateway refused the port.".into());
        }
        self.fail();
        Err(UNAVAILABLE.into())
    }

    /// Asks the gateway to stop, then makes sure it has. A hung gateway takes up to about 10 s;
    /// see the module documentation.
    pub fn shutdown(mut self) {
        self.stop();
    }

    /// Stops the gateway like [`Gateway::shutdown`] without consuming it, for a gateway that is
    /// shared. Every later call answers "unavailable".
    pub fn close(&mut self) {
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

    /// Stops the gateway at once, without asking: its control link can no longer be trusted
    /// (the answer did not come, or was not the one asked for), so whatever policy it holds is
    /// unknown. Every later call answers "unavailable".
    fn fail(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
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
        if !is_update_ack(&response, revision) {
            self.fail();
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
        result.map_err(|_| {
            self.fail();
            UNAVAILABLE.to_owned()
        })
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.stop();
    }
}

fn has_version(response: &Value) -> bool {
    response.get("version").and_then(Value::as_u64) == Some(u64::from(PROTOCOL_VERSION))
}

fn zeroize_value(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(zeroize_value),
        Value::Object(map) => map.values_mut().for_each(zeroize_value),
        _ => {}
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
        gateway_ipv4,
        guest_mac,
        mtu,
        published_ports,
        secrets,
        github_profile_json,
        github_hosts,
        policy,
        strict,
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
        policy: policy.as_ref(),
        strict,
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
        &Expected {
            slot,
            guest_ipv4,
            gateway_ipv4,
            guest_mac,
            mtu,
            ca_certificate_pem: ca_cert_pem,
        },
    )?;
    validate_published_ports(&ready, &published_ports)?;
    Ok(ready)
}

/// The container-specific part of the readiness check: the ports published are exactly the
/// ones asked for, each bound on the host, over TCP.
fn validate_published_ports(ready: &Ready, published_ports: &[u16]) -> Result<(), String> {
    let mut requested = published_ports.to_vec();
    requested.sort_unstable();
    let mut reported = ready
        .loopback_ports
        .iter()
        .map(|port| port.guest_port)
        .collect::<Vec<_>>();
    reported.sort_unstable();
    if requested != reported
        || ready
            .loopback_ports
            .iter()
            .any(|port| port.host_port == 0 || port.protocol != "tcp")
    {
        return Err("The computer network gateway returned inconsistent network identity.".into());
    }
    Ok(())
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
        let (cert, mut key) = generate_ca(binary)?;
        // Moved, not copied: the generator's buffer is left empty.
        proxy_ca::CaMaterial::from_pem(cert, std::mem::take(&mut *key))
    })?;
    Ok((
        material.certificate_pem().to_owned(),
        Zeroizing::new(material.private_key_pem().to_owned()),
    ))
}

#[cfg(test)]
impl Gateway {
    /// A gateway whose process is a long sleep: only the control link is real.
    pub(crate) fn for_test(control: UnixStream) -> Gateway {
        const CERT: &str = "-----BEGIN CERTIFICATE-----\nx\n-----END CERTIFICATE-----";
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        control.set_read_timeout(Some(UPDATE_TIMEOUT)).unwrap();
        control.set_write_timeout(Some(UPDATE_TIMEOUT)).unwrap();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::fd::FromRawFd, thread};

    const CERT: &str = "-----BEGIN CERTIFICATE-----\nx\n-----END CERTIFICATE-----";
    const KEY: &str = "-----BEGIN PRIVATE KEY-----\nx\n-----END PRIVATE KEY-----";

    fn params(ports: Vec<u16>) -> Params {
        Params {
            slot: 7,
            guest_ipv4: Ipv4Addr::new(172, 30, 0, 6),
            gateway_ipv4: Ipv4Addr::new(172, 30, 0, 5),
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
            policy: None,
            strict: None,
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
        let changes: [(&str, Change); 8] = [
            ("a gateway address that was not asked for", |ready| {
                ready["gateway_ipv4"] = "172.30.0.9".into()
            }),
            ("a dns server that is not the gateway", |ready| {
                ready["dns_ipv4"] = "8.8.8.8".into()
            }),
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
        Gateway::for_test(control)
    }

    #[test]
    fn ports_are_published_and_listed_through_the_control_link() {
        let (control, mut peer) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || {
            let mut seen = Vec::new();
            for reply in [
                serde_json::json!({"type": "ports", "version": 1, "ports": [
                    {"host_port": 40001, "guest_port": 3000, "protocol": "tcp"}]}),
                serde_json::json!({"type": "port_probe", "version": 1, "reachable": true}),
                serde_json::json!({"type": "ports", "version": 1, "ports": []}),
                serde_json::json!({"type": "ports", "version": 1, "ports": []}),
            ] {
                let request = read_response(&mut peer).unwrap();
                write_frame(&mut peer, &serde_json::to_vec(&reply).unwrap()).unwrap();
                seen.push(request);
            }
            seen
        });
        let mut gateway = with_sleeping_child(control);
        let added = gateway.add_port(3000, 0).unwrap();
        assert_eq!(added[0].host_port, 40001);
        assert!(gateway.probe_port(3000).unwrap());
        assert!(gateway.remove_port(3000).unwrap().is_empty());
        assert!(gateway.ports().unwrap().is_empty());
        let seen = server.join().unwrap();
        assert_eq!(seen[0]["type"], "port_add");
        assert_eq!(seen[0]["guest_port"], 3000);
        assert_eq!(seen[0]["host_port"], 0);
        assert_eq!(seen[1]["type"], "port_probe");
        assert_eq!(seen[2]["type"], "port_remove");
        assert_eq!(seen[3]["type"], "ports");
        // Port requests are not policy updates: they use no revision.
        assert_eq!(gateway.revision, 0);
        assert!(gateway.is_running());
    }

    #[test]
    fn a_refused_port_leaves_the_link_in_step_and_any_other_answer_stops_the_gateway() {
        let (control, mut peer) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || {
            for reply in [
                serde_json::json!({"type": "error", "version": 1, "code": "port_rejected"}),
                // The answer to a different request, or a port list with a UDP port.
                serde_json::json!({"type": "ack", "version": 1, "revision": 1, "closed_previous_generation": true}),
            ] {
                read_response(&mut peer).unwrap();
                write_frame(&mut peer, &serde_json::to_vec(&reply).unwrap()).unwrap();
            }
        });
        let mut gateway = with_sleeping_child(control);
        let refused = gateway.add_port(3000, 80).unwrap_err();
        assert!(refused.contains("refused the port"), "{refused}");
        assert!(gateway.is_running());
        assert!(gateway.ports().is_err());
        assert!(!gateway.is_running());
        server.join().unwrap();
        assert!(gateway.add_port(3000, 0).is_err());
    }

    #[test]
    fn a_port_list_that_is_not_tcp_stops_the_gateway() {
        let (control, mut peer) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || {
            read_response(&mut peer).unwrap();
            let reply = serde_json::json!({"type": "ports", "version": 1, "ports": [
                {"host_port": 40001, "guest_port": 3000, "protocol": "udp"}]});
            write_frame(&mut peer, &serde_json::to_vec(&reply).unwrap()).unwrap();
        });
        let mut gateway = with_sleeping_child(control);
        assert!(gateway.ports().is_err());
        server.join().unwrap();
        assert!(!gateway.is_running());
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
        // A rejected update does not consume its revision, and it stops the gateway: what it
        // holds is no longer known.
        assert_eq!(gateway.revision, 2);
        assert!(!gateway.is_running());
        assert!(gateway.remove_secret("TOKEN_A").is_err());
        assert!(gateway.stats().is_err());
    }

    fn ack(revision: u64) -> Value {
        serde_json::json!({"type": "ack", "version": 1, "revision": revision, "closed_previous_generation": true})
    }

    #[test]
    fn a_late_ack_after_a_timed_out_update_is_never_taken_for_the_next_answer() {
        let (control, mut peer) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || {
            let first = read_response(&mut peer).unwrap();
            assert_eq!(first["revision"], 1);
            // Nothing is answered within the timeout; the ack arrives afterwards.
            thread::sleep(UPDATE_TIMEOUT + Duration::from_millis(300));
            let _ = write_frame(&mut peer, &serde_json::to_vec(&ack(1)).unwrap());
            // The next request must never reach the gateway, so no second read is answered.
            peer.set_read_timeout(Some(Duration::from_millis(300)))
                .unwrap();
            read_response(&mut peer).is_err()
        });
        let mut gateway = with_sleeping_child(control);
        let pid = gateway.child.as_ref().unwrap().id();
        let revoke = gateway.remove_secret("TOKEN_A");
        assert!(revoke.is_err(), "a timed-out update is not applied");
        assert!(!gateway.is_running(), "the gateway is stopped");
        // SAFETY: signal 0 only checks that the process exists.
        assert_ne!(unsafe { libc::kill(pid as i32, 0) }, 0);
        thread::sleep(Duration::from_millis(400));
        // The late ack is waiting on the link: a fresh call must still be unavailable.
        let error = gateway.remove_secret("TOKEN_B").unwrap_err();
        assert_eq!(error, UNAVAILABLE);
        assert_eq!(gateway.stats().unwrap_err(), UNAVAILABLE);
        assert!(
            server.join().unwrap(),
            "no request was sent after the failure"
        );
    }

    #[test]
    fn a_refused_update_is_not_taken_for_an_applied_one() {
        let (control, mut peer) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || {
            let request = read_response(&mut peer).unwrap();
            let reply = serde_json::json!({"type": "error", "version": 1, "revision": request["revision"], "code": "invalid_revision"});
            write_frame(&mut peer, &serde_json::to_vec(&reply).unwrap()).unwrap();
        });
        let mut gateway = with_sleeping_child(control);
        assert!(gateway.remove_secret("TOKEN_A").is_err());
        server.join().unwrap();
        assert!(!gateway.is_running());
        assert_eq!(gateway.remove_secret("TOKEN_A").unwrap_err(), UNAVAILABLE);
    }

    #[test]
    fn an_ack_of_another_protocol_version_is_refused() {
        let (control, mut peer) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || {
            let request = read_response(&mut peer).unwrap();
            let mut reply = ack(request["revision"].as_u64().unwrap());
            reply["version"] = 2.into();
            write_frame(&mut peer, &serde_json::to_vec(&reply).unwrap()).unwrap();
        });
        let mut gateway = with_sleeping_child(control);
        assert!(gateway.remove_secret("TOKEN_A").is_err());
        server.join().unwrap();
        assert!(!gateway.is_running());
    }

    #[test]
    fn a_failed_stats_exchange_stops_the_gateway() {
        let (control, mut peer) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || {
            read_response(&mut peer).unwrap();
            write_frame(&mut peer, br#"{"type":"error","version":1}"#).unwrap();
        });
        let mut gateway = with_sleeping_child(control);
        assert!(gateway.stats().is_err());
        server.join().unwrap();
        assert!(!gateway.is_running());
        assert_eq!(gateway.remove_secret("TOKEN_A").unwrap_err(), UNAVAILABLE);
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
