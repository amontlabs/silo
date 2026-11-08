//! The control link to `silo-macos-gateway`, shared by the macOS supervisor
//! (`macos_computers/proxy/supervisor.rs`) and the Linux container gateway (`computer_gateway`).
//!
//! The gateway child gets two descriptors: fd 3 carries Ethernet frames (a datagram socketpair
//! for a macOS VM, a TAP device for a container) and fd 4 is a stream socket carrying
//! length-prefixed JSON control messages (`init`, `update`, `stats`, `shutdown`). Everything
//! both callers must do the same way lives here: the framing, the descriptor handoff, the
//! identity the gateway reports (`ready`) and what is accepted as an acknowledgement of an
//! update. Each caller fails closed on its own: a failed update leaves the gateway running with
//! a policy nobody can vouch for, so the caller stops it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{self, Read, Write},
    net::Ipv4Addr,
    os::fd::{FromRawFd, OwnedFd, RawFd},
    os::unix::{net::UnixStream, process::CommandExt},
    path::Path,
    process::{Child, Command},
};
use zeroize::{Zeroize, Zeroizing};

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Where the gateway finds its frame descriptor.
pub const FRAME_FD: RawFd = 3;
/// Where the gateway finds its control descriptor.
pub const CONTROL_FD: RawFd = 4;
/// Names the file the gateway appends credential-use events to.
pub const USAGE_LOG_ENV: &str = "SILO_SECRET_USAGE_LOG";
/// The prefix length of the point-to-point link between the gateway and a computer.
pub const LINK_PREFIX_LEN: u8 = 30;

const GENERATE_CA_FAILED: &str = "Silo could not generate the computer network CA.";

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

/// The identity the caller asked the gateway for.
pub struct Expected<'a> {
    pub slot: u16,
    pub guest_ipv4: Ipv4Addr,
    /// The gateway's own address on the link, which is also the computer's DNS server.
    pub gateway_ipv4: Ipv4Addr,
    pub guest_mac: [u8; 6],
    pub mtu: u16,
    pub ca_certificate_pem: &'a str,
}

/// Checks the part of `ready` that does not depend on the kind of computer: protocol, slot,
/// addresses (DNS and gateway are the one address the caller expects, both inside the
/// computer's /30), MACs, MTU and the CA the gateway holds. The published ports differ per
/// kind and stay with the caller.
pub fn validate_ready(ready: &Ready, expected: &Expected<'_>) -> Result<(), String> {
    let mask = u32::MAX << (32 - u32::from(LINK_PREFIX_LEN));
    let same_link = |a: Ipv4Addr, b: Ipv4Addr| u32::from(a) & mask == u32::from(b) & mask;
    if ready.version != PROTOCOL_VERSION
        || ready.kind != "ready"
        || ready.slot != expected.slot
        || ready.guest_ipv4 != expected.guest_ipv4
        || ready.gateway_ipv4 != expected.gateway_ipv4
        || ready.dns_ipv4 != expected.gateway_ipv4
        || ready.gateway_ipv4 == ready.guest_ipv4
        || ready.prefix_len != LINK_PREFIX_LEN
        || !same_link(ready.gateway_ipv4, ready.guest_ipv4)
        || ready.guest_mac != expected.guest_mac
        || ready.gateway_mac == [0; 6]
        || ready.gateway_mac[0] & 1 != 0
        || ready.mtu != expected.mtu
        || ready.ca_cert_pem != expected.ca_certificate_pem
    {
        return Err("The computer network gateway returned inconsistent network identity.".into());
    }
    Ok(())
}

/// Whether `response` is the gateway's acknowledgement of update `revision`: the same protocol
/// version, the same revision, and the barrier that every connection of the previous policy
/// has closed. Anything else (an error answer, a stale revision) is a refused update.
pub fn is_update_ack(response: &Value, revision: u64) -> bool {
    response.get("type").and_then(Value::as_str) == Some("ack")
        && response.get("version").and_then(Value::as_u64) == Some(u64::from(PROTOCOL_VERSION))
        && response.get("revision").and_then(Value::as_u64) == Some(revision)
        && response
            .get("closed_previous_generation")
            .and_then(Value::as_bool)
            == Some(true)
}

pub fn write_frame(stream: &mut UnixStream, body: &[u8]) -> io::Result<()> {
    if body.is_empty() || body.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid control frame",
        ));
    }
    stream.write_all(&(body.len() as u32).to_be_bytes())?;
    stream.write_all(body)
}

pub fn read_response(stream: &mut UnixStream) -> io::Result<Value> {
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

/// A copy of `fd` above the descriptors a child inherits by default, closed on exec so that
/// only the two `dup2` targets set by [`pass_descriptors`] reach the gateway.
pub fn duplicate_for_child(fd: RawFd) -> Result<OwnedFd, String> {
    // SAFETY: `fcntl` is called on an open descriptor and returns a new owned one.
    let duplicated = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
    if duplicated < 0 {
        return Err("Silo could not prepare the computer gateway descriptors.".into());
    }
    // SAFETY: `fcntl` returned a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
}

/// Makes the child see `frame` as fd 3 and `control` as fd 4.
pub fn pass_descriptors(command: &mut Command, frame: RawFd, control: RawFd) {
    // SAFETY: pre_exec performs only async-signal-safe descriptor operations.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(frame, FRAME_FD) < 0 || libc::dup2(control, CONTROL_FD) < 0 {
                return Err(io::Error::last_os_error());
            }
            libc::close(frame);
            libc::close(control);
            Ok(())
        });
    }
}

/// Kills a freshly spawned gateway unless it is taken, so a failed handshake leaves no process.
pub struct ChildGuard(pub Option<Child>);

impl ChildGuard {
    pub fn take(&mut self) -> Child {
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

/// A CA from the gateway's own generator: the certificate, and the key (zeroized when dropped).
pub fn generate_ca(binary: &Path) -> Result<(String, Zeroizing<String>), String> {
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
        .map_err(|_| GENERATE_CA_FAILED.to_owned())?;
    if !output.status.success() || !output.stderr.is_empty() {
        output.stdout.zeroize();
        return Err(GENERATE_CA_FAILED.into());
    }
    let framed = Zeroizing::new(std::mem::take(&mut output.stdout));
    let body = decode_frame(&framed)?;
    let mut bundle: Bundle =
        serde_json::from_slice(body).map_err(|_| GENERATE_CA_FAILED.to_owned())?;
    let cert = std::mem::take(&mut bundle.cert_pem);
    let key = Zeroizing::new(std::mem::take(&mut bundle.key_pem));
    Ok((cert, key))
}

/// The body of one length-prefixed message that is the whole of `bytes`.
fn decode_frame(bytes: &[u8]) -> Result<&[u8], String> {
    let size = bytes
        .get(..4)
        .map(|prefix| u32::from_be_bytes(prefix.try_into().expect("four-byte prefix")) as usize);
    match size {
        Some(size) if size != 0 && size <= MAX_MESSAGE_BYTES && bytes.len() == size + 4 => {
            Ok(&bytes[4..])
        }
        _ => Err(GENERATE_CA_FAILED.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CERT: &str = "-----BEGIN CERTIFICATE-----\nx\n-----END CERTIFICATE-----";

    fn ready() -> Ready {
        Ready {
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
        }
    }

    fn expected() -> Expected<'static> {
        Expected {
            slot: 7,
            guest_ipv4: Ipv4Addr::new(172, 30, 0, 6),
            gateway_ipv4: Ipv4Addr::new(172, 30, 0, 5),
            guest_mac: [2, 0, 0, 0, 0, 7],
            mtu: 1500,
            ca_certificate_pem: CERT,
        }
    }

    #[test]
    fn a_consistent_ready_is_accepted() {
        assert!(validate_ready(&ready(), &expected()).is_ok());
    }

    #[test]
    fn every_inconsistent_field_of_ready_is_refused() {
        type Change = fn(&mut Ready);
        let changes: [(&str, Change); 12] = [
            ("version", |r| r.version = 2),
            ("kind", |r| r.kind = "error".into()),
            ("slot", |r| r.slot = 8),
            ("guest address", |r| {
                r.guest_ipv4 = Ipv4Addr::new(172, 30, 0, 10)
            }),
            ("gateway address", |r| {
                r.gateway_ipv4 = Ipv4Addr::new(172, 30, 0, 9);
                r.dns_ipv4 = Ipv4Addr::new(172, 30, 0, 9);
            }),
            ("a dns server that is not the gateway", |r| {
                r.dns_ipv4 = Ipv4Addr::new(8, 8, 8, 8)
            }),
            ("a prefix other than /30", |r| r.prefix_len = 24),
            ("guest mac", |r| r.guest_mac = [2, 0, 0, 0, 0, 9]),
            ("an all-zero gateway mac", |r| r.gateway_mac = [0; 6]),
            ("a multicast gateway mac", |r| {
                r.gateway_mac = [1, 0, 0, 0, 0, 1]
            }),
            ("mtu", |r| r.mtu = 9000),
            ("certificate", |r| r.ca_cert_pem = "other".into()),
        ];
        for (label, change) in changes {
            let mut candidate = ready();
            change(&mut candidate);
            assert!(validate_ready(&candidate, &expected()).is_err(), "{label}");
        }
    }

    #[test]
    fn a_gateway_outside_the_computers_link_is_refused() {
        // The caller asked for it and the gateway agreed, but it is not on the computer's /30.
        let mut elsewhere = expected();
        elsewhere.gateway_ipv4 = Ipv4Addr::new(172, 30, 0, 9);
        let mut candidate = ready();
        candidate.gateway_ipv4 = Ipv4Addr::new(172, 30, 0, 9);
        candidate.dns_ipv4 = Ipv4Addr::new(172, 30, 0, 9);
        assert!(validate_ready(&candidate, &elsewhere).is_err());
    }

    #[test]
    fn an_update_is_acknowledged_only_by_its_own_revision_with_the_close_barrier() {
        let ack = |revision: u64, closed: bool, version: u16| serde_json::json!({"type": "ack", "version": version, "revision": revision, "closed_previous_generation": closed});
        assert!(is_update_ack(&ack(3, true, 1), 3));
        assert!(!is_update_ack(&ack(2, true, 1), 3), "a stale revision");
        assert!(!is_update_ack(&ack(3, false, 1), 3), "no close barrier");
        assert!(!is_update_ack(&ack(3, true, 2), 3), "another version");
        assert!(!is_update_ack(
            &serde_json::json!({"type": "error", "version": 1, "revision": 3, "code": "invalid_revision"}),
            3
        ));
    }

    #[test]
    fn frames_round_trip_and_reject_empty_or_oversized_bodies() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        write_frame(&mut a, br#"{"type":"stats"}"#).unwrap();
        assert_eq!(read_response(&mut b).unwrap()["type"], "stats");
        assert!(write_frame(&mut a, b"").is_err());
        assert!(write_frame(&mut a, &vec![b' '; MAX_MESSAGE_BYTES + 1]).is_err());
        a.write_all(&0u32.to_be_bytes()).unwrap();
        assert!(read_response(&mut b).is_err());
    }

    #[test]
    fn a_generated_ca_is_one_whole_frame() {
        let body = br#"{"cert_pem":"c","key_pem":"k"}"#;
        let mut framed = (body.len() as u32).to_be_bytes().to_vec();
        framed.extend_from_slice(body);
        assert_eq!(decode_frame(&framed).unwrap(), body);
        assert!(decode_frame(&framed[..framed.len() - 1]).is_err());
        assert!(decode_frame(&[]).is_err());
        assert!(decode_frame(&0u32.to_be_bytes()).is_err());
    }
}
