use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;

use microsandbox_network::config::{
    EnvNetworkSecretResolver, InterfaceOverrides, NetworkBuilder,
    PortProtocol as NetworkPortProtocol,
};
use microsandbox_network::dns::Nameserver;
pub use microsandbox_network::network::SecretsHandle;
pub use microsandbox_network::network::{
    PublishedPortBinding, SmoltcpNetwork, generate_intercept_ca_pair,
};
use microsandbox_network::policy::{NetworkPolicy, NetworkProfile};
use microsandbox_types::DeploymentProfile;
use tempfile::TempDir;
use zeroize::{Zeroize, Zeroizing};

use crate::control::{InitRequest, LoopbackPort, PROTOCOL_VERSION, PortProtocol, ReadyResponse};

const MAX_MTU: u16 = 1500;
const MIN_MTU: u16 = 576;
const GITHUB_PLACEHOLDER: &str = "$MSB_SILO_GITHUB";

pub struct NetworkResources {
    pub network: SmoltcpNetwork,
    pub secrets: SecretsHandle,
    pub ready: ReadyResponse,
}

pub struct GeneratedCa {
    pub cert_pem: String,
    pub key_pem: Zeroizing<String>,
}

pub fn install_crypto_provider() {
    // The network stack enables more than one Rustls provider transitively.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

pub fn validate_init(init: &InitRequest) -> Result<(), NetworkSetupError> {
    if init.version != PROTOCOL_VERSION
        || init.mtu < MIN_MTU
        || init.mtu > MAX_MTU
        || init.guest_ipv4.is_unspecified()
        || init.guest_ipv4.is_multicast()
        || init.guest_ipv4.is_broadcast()
        || init.guest_ipv4.octets()[3] & 0x03 != 2
        || u32::from(init.guest_ipv4) < 2
        || !valid_unicast_mac(init.guest_mac)
    {
        return Err(NetworkSetupError::InvalidInput);
    }
    if !init.dns_nameservers.is_empty()
        && init
            .dns_nameservers
            .iter()
            .any(|value| Nameserver::from_str(value).is_err())
    {
        return Err(NetworkSetupError::InvalidInput);
    }
    if init.intercepted_ports.is_empty()
        || !init.intercepted_ports.contains(&443)
        || init.intercepted_ports.contains(&0)
    {
        return Err(NetworkSetupError::InvalidInput);
    }
    if !valid_pem(&init.ca.cert_pem, "CERTIFICATE")
        || !(init.ca.key_pem.contains("-----BEGIN PRIVATE KEY-----")
            || init.ca.key_pem.contains("-----BEGIN EC PRIVATE KEY-----"))
    {
        return Err(NetworkSetupError::InvalidInput);
    }

    let mut names = HashSet::new();
    for secret in &init.secrets {
        if secret.name == "SILO_GITHUB"
            || !valid_secret_name(&secret.name)
            || !valid_secret_value(&secret.value)
            || !valid_placeholder(&secret.placeholder)
            || secret.allowed_hosts.is_empty()
            || !secret
                .allowed_hosts
                .iter()
                .all(|host| host == "*" || valid_host_pattern(host))
            || !names.insert(secret.name.as_str())
        {
            return Err(NetworkSetupError::InvalidInput);
        }
    }
    if let Some(profile) = &init.github_profile_json {
        if profile.is_empty() || !valid_json(profile) || !names.insert("SILO_GITHUB") {
            return Err(NetworkSetupError::InvalidInput);
        }
    }

    let mut host_ports = HashSet::new();
    let mut guest_ports = HashSet::new();
    for mapping in &init.loopback_ports {
        if mapping.guest_port == 0
            || (mapping.host_port != 0 && !host_ports.insert((mapping.host_port, mapping.protocol)))
            || !guest_ports.insert((mapping.guest_port, mapping.protocol))
        {
            return Err(NetworkSetupError::InvalidInput);
        }
    }
    if init.loopback_ports.len() != 2
        || !init.loopback_ports.iter().any(|mapping| {
            mapping.host_port == 0
                && mapping.guest_port == 22
                && mapping.protocol == PortProtocol::Tcp
        })
        || !init.loopback_ports.iter().any(|mapping| {
            mapping.host_port == 0
                && mapping.guest_port == 5900
                && mapping.protocol == PortProtocol::Tcp
        })
    {
        return Err(NetworkSetupError::InvalidInput);
    }
    Ok(())
}

pub fn build_network(mut init: InitRequest) -> Result<NetworkResources, NetworkSetupError> {
    install_crypto_provider();
    let ca_files = TempDir::new().map_err(|_| NetworkSetupError::TemporaryFiles)?;
    let ca_cert_path = write_private_file(
        ca_files.path(),
        "intercept-ca.pem",
        init.ca.cert_pem.as_bytes(),
    )?;
    let ca_key_path = write_private_file(
        ca_files.path(),
        "intercept-ca-key.pem",
        init.ca.key_pem.as_bytes(),
    )?;

    let policy = init
        .policy
        .take()
        .unwrap_or_else(|| NetworkPolicy::from_profiles([NetworkProfile::Public]));
    let nameservers = init
        .dns_nameservers
        .iter()
        .map(|value| Nameserver::from_str(value).map_err(|_| NetworkSetupError::InvalidInput))
        .collect::<Result<Vec<_>, _>>()?;
    let mut builder = NetworkBuilder::new()
        .interface(InterfaceOverrides {
            mac: Some(init.guest_mac),
            mtu: Some(init.mtu),
            ipv4_address: Some(init.guest_ipv4),
            ipv6_enabled: Some(false),
            ..Default::default()
        })
        .policy(policy)
        .tls(|tls| {
            tls.enabled(true)
                .intercepted_ports(init.intercepted_ports.clone())
                .intercept_ca_cert(ca_cert_path)
                .intercept_ca_key(ca_key_path)
        });
    if !nameservers.is_empty() {
        builder = builder.dns(|dns| dns.nameservers(nameservers));
    }

    for mapping in &init.loopback_ports {
        builder = match mapping.protocol {
            PortProtocol::Tcp => builder.port(mapping.host_port, mapping.guest_port),
            PortProtocol::Udp => builder.port_udp(mapping.host_port, mapping.guest_port),
        };
    }

    for secret in init.secrets.drain(..) {
        let mut value = secret.value;
        let mut entry = microsandbox_network::config::builder::SecretBuilder::new()
            .env(secret.name)
            .value(std::mem::take(&mut *value))
            .placeholder(secret.placeholder);
        for host in secret.allowed_hosts {
            entry = if host == "*" {
                entry.allow_any_host_dangerous(true)
            } else {
                entry.allow(host)
            };
        }
        builder = builder.secret(|_| entry);
    }
    if let Some(mut github_profile) = init.github_profile_json.take() {
        let entry = microsandbox_network::config::builder::SecretBuilder::new()
            .env("SILO_GITHUB")
            .value(std::mem::take(&mut *github_profile))
            .placeholder(GITHUB_PLACEHOLDER)
            .allow("github.com")
            .allow("api.github.com");
        builder = builder.secret(|_| entry);
    }

    let config = builder
        .build()
        .map_err(|_| NetworkSetupError::InvalidInput)?
        .resolve(&EnvNetworkSecretResolver)
        .map_err(|_| NetworkSetupError::InvalidInput)?;
    let network = SmoltcpNetwork::new(config, init.slot, DeploymentProfile::SingleTenant)
        .map_err(|_| NetworkSetupError::Network)?;

    // TLS state has loaded the key into its CA issuer. Remove the temporary PEM files before
    // returning any readiness data or accepting update messages.
    drop(ca_files);

    let bootstrap = network.guest_bootstrap_network();
    let ipv4 = bootstrap
        .ipv4
        .ok_or(NetworkSetupError::MissingIpv4Bootstrap)?;
    let ca_cert_pem = network.ca_cert_pem().ok_or(NetworkSetupError::MissingCa)?;
    let ca_cert_pem = String::from_utf8(ca_cert_pem).map_err(|_| NetworkSetupError::InvalidCa)?;
    let ready = ReadyResponse {
        version: PROTOCOL_VERSION,
        slot: init.slot,
        guest_ipv4: ipv4.address,
        gateway_ipv4: ipv4.gateway,
        prefix_len: ipv4.prefix_len,
        dns_ipv4: ipv4.dns.unwrap_or(ipv4.gateway),
        guest_mac: bootstrap.mac,
        gateway_mac: SmoltcpNetwork::default_gateway_mac(init.slot),
        mtu: bootstrap.mtu,
        ca_cert_pem,
        loopback_ports: init.loopback_ports,
    };
    let secrets = network.secrets_handle();

    Ok(NetworkResources {
        network,
        secrets,
        ready,
    })
}

pub fn generate_ca() -> Result<GeneratedCa, NetworkSetupError> {
    install_crypto_provider();
    let (cert_pem, key_pem) =
        generate_intercept_ca_pair().map_err(|_| NetworkSetupError::CaGeneration)?;
    let cert_pem = String::from_utf8(cert_pem).map_err(|_| NetworkSetupError::InvalidCa)?;
    let key_pem = match String::from_utf8(key_pem) {
        Ok(key) => Zeroizing::new(key),
        Err(error) => {
            let mut bytes = error.into_bytes();
            bytes.zeroize();
            return Err(NetworkSetupError::InvalidCa);
        }
    };
    Ok(GeneratedCa { cert_pem, key_pem })
}

pub fn map_bound_ports(
    requested: &[LoopbackPort],
    bound: Vec<PublishedPortBinding>,
) -> Result<Vec<LoopbackPort>, NetworkSetupError> {
    if requested.len() != bound.len() {
        return Err(NetworkSetupError::LoopbackBind);
    }

    let mut used = vec![false; bound.len()];
    let mut actual_ports = HashSet::new();
    let mut result = Vec::with_capacity(requested.len());
    for request in requested {
        let protocol = match request.protocol {
            PortProtocol::Tcp => NetworkPortProtocol::Tcp,
            PortProtocol::Udp => NetworkPortProtocol::Udp,
        };
        let index = bound.iter().enumerate().position(|(index, listener)| {
            !used[index]
                && listener.guest_port == request.guest_port
                && listener.protocol == protocol
                && listener.host_port != 0
                && (request.host_port == 0 || request.host_port == listener.host_port)
        });
        let Some(index) = index else {
            return Err(NetworkSetupError::LoopbackBind);
        };
        used[index] = true;
        let listener = &bound[index];
        if !actual_ports.insert((listener.host_port, request.protocol)) {
            return Err(NetworkSetupError::LoopbackBind);
        }
        result.push(LoopbackPort {
            host_port: listener.host_port,
            guest_port: listener.guest_port,
            protocol: request.protocol,
        });
    }
    if used.iter().any(|used| !used) {
        return Err(NetworkSetupError::LoopbackBind);
    }
    Ok(result)
}

fn write_private_file(
    dir: &Path,
    name: &str,
    contents: &[u8],
) -> Result<PathBuf, NetworkSetupError> {
    let path = dir.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|_| NetworkSetupError::TemporaryFiles)?;
    file.write_all(contents)
        .map_err(|_| NetworkSetupError::TemporaryFiles)?;
    file.sync_all()
        .map_err(|_| NetworkSetupError::TemporaryFiles)?;
    Ok(path)
}

fn valid_unicast_mac(mac: [u8; 6]) -> bool {
    mac != [0; 6] && mac[0] & 1 == 0
}

fn valid_secret_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(ch) if ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && name.len() <= 128
        && !reserved_secret_name(name)
}

#[derive(serde::Deserialize)]
struct ReservedSecretNames {
    names: Vec<String>,
    prefixes: Vec<String>,
}

fn reserved_secret_names() -> &'static ReservedSecretNames {
    static RESERVED: OnceLock<ReservedSecretNames> = OnceLock::new();
    RESERVED.get_or_init(|| {
        serde_json::from_str(include_str!("../reserved-secret-names.json"))
            .expect("bundled reserved secret name data is valid")
    })
}

fn reserved_secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let reserved = reserved_secret_names();
    reserved.names.iter().any(|entry| entry == &upper)
        || reserved
            .prefixes
            .iter()
            .any(|prefix| upper.starts_with(prefix))
}

fn valid_secret_value(value: &str) -> bool {
    !value.is_empty() && !value.contains('\0')
}

fn valid_placeholder(placeholder: &str) -> bool {
    !placeholder.is_empty()
        && placeholder.len() <= 1024
        && !placeholder
            .bytes()
            .any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
}

fn valid_host_pattern(value: &str) -> bool {
    if value == "*" {
        return true;
    }
    let wildcard = value.starts_with("*.");
    let host = value.strip_prefix("*.").unwrap_or(value);
    if host.is_empty() || host.len() > 253 || host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    let labels_valid = host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label.as_bytes()[0].is_ascii_alphanumeric()
            && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    });
    labels_valid && (!wildcard || host.contains('.'))
}

fn valid_pem(value: &str, label: &str) -> bool {
    value.contains(&format!("-----BEGIN {label}-----"))
        && value.contains(&format!("-----END {label}-----"))
}

fn valid_json(value: &str) -> bool {
    serde_json::from_str::<serde::de::IgnoredAny>(value).is_ok()
}

pub fn validate_update(
    operation: &crate::control::UpdateOperation,
) -> Result<(), NetworkSetupError> {
    use crate::control::UpdateOperation;

    match operation {
        UpdateOperation::PutSecret {
            name,
            value,
            placeholder,
            allowed_hosts,
        } => {
            if name == "SILO_GITHUB"
                || !valid_secret_name(name)
                || !valid_secret_value(value)
                || !valid_placeholder(placeholder)
                || allowed_hosts.is_empty()
                || !allowed_hosts
                    .iter()
                    .all(|host| host == "*" || valid_host_pattern(host))
            {
                return Err(NetworkSetupError::InvalidInput);
            }
        }
        UpdateOperation::RotateSecret { name, value } => {
            if name == "SILO_GITHUB" || !valid_secret_name(name) || !valid_secret_value(value) {
                return Err(NetworkSetupError::InvalidInput);
            }
        }
        UpdateOperation::RemoveSecret { name } => {
            if name == "SILO_GITHUB" || !valid_secret_name(name) {
                return Err(NetworkSetupError::InvalidInput);
            }
        }
        UpdateOperation::SetAllowedHosts {
            name,
            allowed_hosts,
        } => {
            if (name != "SILO_GITHUB" && !valid_secret_name(name)) || allowed_hosts.is_empty() {
                return Err(NetworkSetupError::InvalidInput);
            }
            if name == "SILO_GITHUB" {
                let requested = allowed_hosts
                    .iter()
                    .map(String::as_str)
                    .collect::<HashSet<_>>();
                if allowed_hosts.len() != 2
                    || requested != HashSet::from(["github.com", "api.github.com"])
                {
                    return Err(NetworkSetupError::InvalidInput);
                }
            } else if !allowed_hosts
                .iter()
                .all(|host| host == "*" || valid_host_pattern(host))
            {
                return Err(NetworkSetupError::InvalidInput);
            }
        }
        UpdateOperation::SetGithubProfile { profile_json } => {
            if profile_json
                .as_ref()
                .is_some_and(|profile| profile.is_empty() || !valid_json(profile))
            {
                return Err(NetworkSetupError::InvalidInput);
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum NetworkSetupError {
    InvalidInput,
    TemporaryFiles,
    Network,
    MissingIpv4Bootstrap,
    MissingCa,
    InvalidCa,
    CaGeneration,
    LoopbackBind,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{CaInput, SecretInput, UpdateOperation};
    use microsandbox_network::policy::{Action, DestinationGroup};
    use std::net::Ipv4Addr;

    #[test]
    fn missing_policy_uses_public_egress_with_deny_defaults() {
        let policy = NetworkPolicy::from_profiles([NetworkProfile::Public]);
        assert_eq!(policy.default_egress, Action::Deny);
        assert_eq!(policy.default_ingress, Action::Allow);
        assert!(policy.rules.iter().any(|rule| {
            matches!(
                &rule.destination,
                microsandbox_network::policy::Destination::Group(DestinationGroup::Public)
            )
        }));
    }

    #[test]
    fn rejects_wildcard_secret_hosts_and_spoofable_guest_addresses() {
        assert!(valid_host_pattern("*"));
        assert!(valid_host_pattern("*.example.test"));
        assert!(!valid_host_pattern("bad..example.test"));
        assert!(valid_unicast_mac([2, 0, 0, 0, 0, 2]));
        assert!(!valid_unicast_mac([1, 0, 0, 0, 0, 2]));
        assert!(!valid_host_pattern("*.test"));
        assert!(Nameserver::from_str("1.1.1.1").is_ok());
        assert!(Nameserver::from_str("dns.google").is_ok());
        assert!(Nameserver::from_str("not a nameserver").is_err());
        assert!(!valid_secret_name("GH_TOKEN"));
        assert!(!valid_secret_name("silo_custom"));
        assert!(valid_secret_name("API_KEY"));
    }

    #[test]
    fn empty_dns_and_empty_credential_material_are_valid() {
        let init = InitRequest {
            version: PROTOCOL_VERSION,
            slot: 4,
            guest_ipv4: Ipv4Addr::new(172, 16, 0, 18),
            guest_mac: [2, 0, 0, 0, 0, 4],
            mtu: 1500,
            policy: None,
            dns_nameservers: Vec::new(),
            intercepted_ports: vec![443],
            loopback_ports: vec![
                LoopbackPort {
                    host_port: 0,
                    guest_port: 22,
                    protocol: PortProtocol::Tcp,
                },
                LoopbackPort {
                    host_port: 0,
                    guest_port: 5900,
                    protocol: PortProtocol::Tcp,
                },
            ],
            ca: CaInput {
                cert_pem: Zeroizing::new(
                    "-----BEGIN CERTIFICATE-----\nx\n-----END CERTIFICATE-----".into(),
                ),
                key_pem: Zeroizing::new(
                    "-----BEGIN PRIVATE KEY-----\nx\n-----END PRIVATE KEY-----".into(),
                ),
            },
            secrets: Vec::<SecretInput>::new(),
            github_profile_json: None,
        };
        assert!(validate_init(&init).is_ok());
    }

    #[test]
    fn policy_updates_reject_reserved_names_and_unscoped_github_hosts() {
        assert!(
            validate_update(&UpdateOperation::PutSecret {
                name: "API_KEY".into(),
                value: Zeroizing::new("synthetic".into()),
                placeholder: "$MSB_API_KEY".into(),
                allowed_hosts: vec!["api.example.test".into()],
            })
            .is_ok()
        );
        assert!(
            validate_update(&UpdateOperation::PutSecret {
                name: "API_KEY".into(),
                value: Zeroizing::new("synthetic".into()),
                placeholder: "$MSB_API_KEY".into(),
                allowed_hosts: vec!["*".into()],
            })
            .is_ok()
        );
        assert!(
            validate_update(&UpdateOperation::PutSecret {
                name: "PATH".into(),
                value: Zeroizing::new("synthetic".into()),
                placeholder: "$MSB_PATH".into(),
                allowed_hosts: vec!["api.example.test".into()],
            })
            .is_err()
        );
        assert!(
            validate_update(&UpdateOperation::SetAllowedHosts {
                name: "SILO_GITHUB".into(),
                allowed_hosts: vec!["github.com".into(), "api.github.com".into()],
            })
            .is_ok()
        );
        assert!(
            validate_update(&UpdateOperation::SetAllowedHosts {
                name: "SILO_GITHUB".into(),
                allowed_hosts: vec!["*".into()],
            })
            .is_err()
        );
    }

    #[test]
    fn ready_uses_ports_from_owned_listener_bindings() {
        let requested = [
            LoopbackPort {
                host_port: 0,
                guest_port: 9222,
                protocol: PortProtocol::Tcp,
            },
            LoopbackPort {
                host_port: 0,
                guest_port: 9223,
                protocol: PortProtocol::Tcp,
            },
        ];
        let actual = map_bound_ports(
            &requested,
            vec![
                PublishedPortBinding {
                    host_port: 51234,
                    guest_port: 9222,
                    protocol: NetworkPortProtocol::Tcp,
                },
                PublishedPortBinding {
                    host_port: 51235,
                    guest_port: 9223,
                    protocol: NetworkPortProtocol::Tcp,
                },
            ],
        )
        .unwrap();
        assert_eq!(actual[0].host_port, 51234);
        assert_eq!(actual[1].host_port, 51235);
    }

    #[test]
    fn ready_rejects_missing_or_mismatched_listener_bindings() {
        let requested = [LoopbackPort {
            host_port: 0,
            guest_port: 9222,
            protocol: PortProtocol::Tcp,
        }];
        assert!(map_bound_ports(&requested, Vec::new()).is_err());
        assert!(
            map_bound_ports(
                &requested,
                vec![PublishedPortBinding {
                    host_port: 0,
                    guest_port: 9222,
                    protocol: NetworkPortProtocol::Tcp,
                }]
            )
            .is_err()
        );
    }
}
