use std::io::{self, Read, Write};
use std::net::Ipv4Addr;
use std::os::unix::net::UnixStream;

use microsandbox_network::policy::NetworkPolicy;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_BUFFER_BYTES: usize = MAX_MESSAGE_BYTES + 4;
const MAX_READS_PER_POLL: usize = 16;
const MAX_MESSAGES_PER_POLL: usize = 32;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Init(InitRequest),
    Update(UpdateRequest),
    Stats { version: u16 },
    Shutdown { version: u16 },
}

#[derive(Deserialize)]
pub struct InitRequest {
    pub version: u16,
    pub slot: u16,
    pub guest_ipv4: Ipv4Addr,
    pub guest_mac: [u8; 6],
    #[serde(default = "default_mtu")]
    pub mtu: u16,
    #[serde(default)]
    pub policy: Option<NetworkPolicy>,
    pub dns_nameservers: Vec<String>,
    #[serde(default = "default_intercepted_ports")]
    pub intercepted_ports: Vec<u16>,
    pub loopback_ports: Vec<LoopbackPort>,
    pub ca: CaInput,
    #[serde(default)]
    pub secrets: Vec<SecretInput>,
    #[serde(default, deserialize_with = "deserialize_zeroizing_option")]
    pub github_profile_json: Option<Zeroizing<String>>,
}

#[derive(Deserialize)]
pub struct CaInput {
    #[serde(deserialize_with = "deserialize_zeroizing_string")]
    pub cert_pem: Zeroizing<String>,
    #[serde(deserialize_with = "deserialize_zeroizing_string")]
    pub key_pem: Zeroizing<String>,
}

#[derive(Deserialize)]
pub struct SecretInput {
    pub name: String,
    #[serde(deserialize_with = "deserialize_zeroizing_string")]
    pub value: Zeroizing<String>,
    pub placeholder: String,
    pub allowed_hosts: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct LoopbackPort {
    pub host_port: u16,
    pub guest_port: u16,
    pub protocol: PortProtocol,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PortProtocol {
    Tcp,
    Udp,
}

#[derive(Deserialize)]
pub struct UpdateRequest {
    pub version: u16,
    pub revision: u64,
    pub operation: UpdateOperation,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateOperation {
    PutSecret {
        name: String,
        #[serde(deserialize_with = "deserialize_zeroizing_string")]
        value: Zeroizing<String>,
        placeholder: String,
        allowed_hosts: Vec<String>,
    },
    RotateSecret {
        name: String,
        #[serde(deserialize_with = "deserialize_zeroizing_string")]
        value: Zeroizing<String>,
    },
    RemoveSecret {
        name: String,
    },
    SetAllowedHosts {
        name: String,
        allowed_hosts: Vec<String>,
    },
    SetGithubProfile {
        #[serde(default, deserialize_with = "deserialize_zeroizing_option")]
        profile_json: Option<Zeroizing<String>>,
    },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Ready(ReadyResponse),
    Ack {
        version: u16,
        revision: u64,
        closed_previous_generation: bool,
    },
    Error(ErrorResponse),
    Stats {
        version: u16,
        counters: crate::frames::FrameCounters,
    },
    Stopped {
        version: u16,
    },
}

#[derive(Serialize)]
pub struct ReadyResponse {
    pub version: u16,
    pub slot: u16,
    pub guest_ipv4: Ipv4Addr,
    pub gateway_ipv4: Ipv4Addr,
    pub prefix_len: u8,
    pub dns_ipv4: Ipv4Addr,
    pub guest_mac: [u8; 6],
    pub gateway_mac: [u8; 6],
    pub mtu: u16,
    pub ca_cert_pem: String,
    pub loopback_ports: Vec<LoopbackPort>,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub version: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    pub code: &'static str,
}

impl ErrorResponse {
    pub fn for_revision(revision: Option<u64>, code: &'static str) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            revision,
            code,
        }
    }
}

#[derive(Default)]
pub struct ControlDecoder {
    buffer: Vec<u8>,
}

impl Drop for ControlDecoder {
    fn drop(&mut self) {
        self.buffer.zeroize();
    }
}

#[derive(Debug)]
pub enum ControlError {
    Io,
    InvalidLength,
    InvalidJson,
    InvalidRequest,
    PeerClosed,
}

/// Reads one length-prefixed CA bundle from a private pipe, as the leaf issuer mode receives it.
pub fn read_ca_input(reader: &mut impl Read) -> Result<CaInput, ControlError> {
    let mut size_bytes = [0u8; 4];
    reader
        .read_exact(&mut size_bytes)
        .map_err(|_| ControlError::Io)?;
    let size = u32::from_be_bytes(size_bytes) as usize;
    if size == 0 || size > MAX_MESSAGE_BYTES {
        return Err(ControlError::InvalidLength);
    }
    let mut body = Zeroizing::new(vec![0u8; size]);
    reader.read_exact(&mut body).map_err(|_| ControlError::Io)?;
    serde_json::from_slice(&body).map_err(|_| ControlError::InvalidJson)
}

pub fn read_init(stream: &mut UnixStream) -> Result<InitRequest, ControlError> {
    let mut size_bytes = [0u8; 4];
    stream
        .read_exact(&mut size_bytes)
        .map_err(|_| ControlError::Io)?;
    let size = u32::from_be_bytes(size_bytes) as usize;
    if size == 0 || size > MAX_MESSAGE_BYTES {
        return Err(ControlError::InvalidLength);
    }
    let mut body = Zeroizing::new(vec![0u8; size]);
    stream.read_exact(&mut body).map_err(|_| ControlError::Io)?;
    let parsed = decode_request(&mut body);
    match parsed? {
        Request::Init(init) if init.version == PROTOCOL_VERSION => Ok(init),
        _ => Err(ControlError::InvalidRequest),
    }
}

pub fn write_response(stream: &mut UnixStream, response: &Response) -> io::Result<()> {
    let mut body = serde_json::to_vec(response).map_err(io::Error::other)?;
    if body.is_empty() || body.len() > MAX_MESSAGE_BYTES {
        body.zeroize();
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control response exceeds protocol limit",
        ));
    }
    let size = (body.len() as u32).to_be_bytes();
    let result = stream
        .write_all(&size)
        .and_then(|_| stream.write_all(&body));
    body.zeroize();
    result
}

impl ControlDecoder {
    pub fn read_available(
        &mut self,
        stream: &mut UnixStream,
    ) -> Result<Vec<Request>, ControlError> {
        let mut messages = Vec::new();
        for _ in 0..MAX_READS_PER_POLL {
            let mut chunk = Zeroizing::new([0u8; 4096]);
            match stream.read(&mut chunk[..]) {
                Ok(0) => return Err(ControlError::PeerClosed),
                Ok(read) => {
                    self.buffer.extend_from_slice(&chunk[..read]);
                    if self.buffer.len() > MAX_BUFFER_BYTES {
                        self.buffer.zeroize();
                        self.buffer.clear();
                        return Err(ControlError::InvalidLength);
                    }
                    self.decode_available(&mut messages)?;
                    if messages.len() >= MAX_MESSAGES_PER_POLL {
                        break;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => return Err(ControlError::Io),
            }
        }
        Ok(messages)
    }

    fn decode_available(&mut self, messages: &mut Vec<Request>) -> Result<(), ControlError> {
        while messages.len() < MAX_MESSAGES_PER_POLL && self.buffer.len() >= 4 {
            let size = u32::from_be_bytes(self.buffer[..4].try_into().unwrap()) as usize;
            if size == 0 || size > MAX_MESSAGE_BYTES {
                self.buffer.zeroize();
                self.buffer.clear();
                return Err(ControlError::InvalidLength);
            }
            let framed_size = 4 + size;
            if self.buffer.len() < framed_size {
                break;
            }
            let mut body = self.buffer[4..framed_size].to_vec();
            let remaining = self.buffer.len() - framed_size;
            self.buffer.copy_within(framed_size.., 0);
            self.buffer[remaining..].zeroize();
            self.buffer.truncate(remaining);
            let parsed = decode_request(&mut body);
            if parsed.is_err() {
                self.buffer.zeroize();
                self.buffer.clear();
            }
            messages.push(parsed?);
        }
        Ok(())
    }
}

impl ControlDecoder {
    pub fn decode_buffered(&mut self) -> Result<Vec<Request>, ControlError> {
        let mut messages = Vec::new();
        self.decode_available(&mut messages)?;
        Ok(messages)
    }
}

fn decode_request(body: &mut [u8]) -> Result<Request, ControlError> {
    let parsed = serde_json::from_slice(body).map_err(|_| ControlError::InvalidJson);
    body.zeroize();
    parsed
}

fn deserialize_zeroizing_string<'de, D>(deserializer: D) -> Result<Zeroizing<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer).map(Zeroizing::new)
}

fn deserialize_zeroizing_option<'de, D>(
    deserializer: D,
) -> Result<Option<Zeroizing<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(|value| value.map(Zeroizing::new))
}

const fn default_mtu() -> u16 {
    1500
}

fn default_intercepted_ports() -> Vec<u16> {
    vec![443]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_handles_fragmented_and_batched_control_messages() {
        let first = serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "type": "stats"
        }))
        .unwrap();
        let second = serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "type": "shutdown"
        }))
        .unwrap();
        let mut wire = Vec::new();
        wire.extend_from_slice(&(first.len() as u32).to_be_bytes());
        wire.extend_from_slice(&first);
        wire.extend_from_slice(&(second.len() as u32).to_be_bytes());
        wire.extend_from_slice(&second);
        let mut decoder = ControlDecoder::default();
        let mut messages = Vec::new();
        decoder.buffer.extend_from_slice(&wire[..3]);
        decoder.decode_available(&mut messages).unwrap();
        assert!(messages.is_empty());
        decoder.buffer.extend_from_slice(&wire[3..]);
        decoder.decode_available(&mut messages).unwrap();
        assert!(matches!(
            messages.as_slice(),
            [
                Request::Stats { version: 1 },
                Request::Shutdown { version: 1 }
            ]
        ));
        assert!(decoder.buffer.is_empty());
    }

    #[test]
    fn decoder_rejects_oversized_control_message() {
        let mut decoder = ControlDecoder::default();
        decoder
            .buffer
            .extend_from_slice(&((MAX_MESSAGE_BYTES as u32) + 1).to_be_bytes());
        assert!(matches!(
            decoder.decode_available(&mut Vec::new()),
            Err(ControlError::InvalidLength)
        ));
        assert!(decoder.buffer.is_empty());
    }

    #[test]
    fn init_secrets_deserialize_into_zeroizing_buffers() {
        let request: Request = serde_json::from_value(serde_json::json!({
            "version": 1,
            "type": "init",
            "slot": 4,
            "guest_ipv4": "172.16.0.18",
            "guest_mac": [2, 0, 0, 0, 0, 4],
            "policy": null,
            "dns_nameservers": ["1.1.1.1"],
            "loopback_ports": [],
            "ca": {"cert_pem": "cert", "key_pem": "private"},
            "secrets": [{
                "name": "ACCESS_TOKEN",
                "value": "synthetic",
                "placeholder": "$SILO_ACCESS_TOKEN",
                "allowed_hosts": ["api.example.test"]
            }],
            "github_profile_json": null
        }))
        .unwrap();
        let Request::Init(init) = request else {
            panic!("init variant was not decoded")
        };
        assert_eq!(&*init.secrets[0].value, "synthetic");
        assert_eq!(&*init.ca.key_pem, "private");
    }

    #[test]
    fn update_protocol_deserializes_github_profile_without_debug_traits() {
        let request: Request = serde_json::from_value(serde_json::json!({
            "version": 1,
            "type": "update",
            "revision": 8,
            "operation": {
                "kind": "set_github_profile",
                "profile_json": "{\"token\":\"synthetic\"}"
            }
        }))
        .unwrap();
        let Request::Update(update) = request else {
            panic!("update variant was not decoded")
        };
        let UpdateOperation::SetGithubProfile { profile_json } = update.operation else {
            panic!("github update variant was not decoded")
        };
        assert_eq!(
            profile_json.as_deref().map(String::as_str),
            Some("{\"token\":\"synthetic\"}")
        );
    }
}
