//! Per-computer guest network and public CA bootstrap.

#![allow(dead_code)] // Provisioning and launch migration consume the exported staging hooks.

use super::super::engine;
use super::super::guest_access;
use super::super::proxy_address::{self, ProxyAddress};
use super::super::proxy_ca::{self, CaMaterial};
use super::super::store::{Layout, Record};
use super::super::MacosContext;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, PoisonError, Weak},
    time::{Duration, Instant},
};
use tempfile::Builder;
use zeroize::Zeroize;

const IDENTITY_FILE: &str = "proxy-bootstrap.json";
const BOOTSTRAP_VERSION: u32 = 1;
const SSH_WAIT: Duration = Duration::from_secs(300);
const SSH_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const GUEST_READY_WAIT: Duration = Duration::from_secs(300);
const STOP_WAIT: Duration = Duration::from_secs(45);
/// How long the guest gets to shut down by itself before it is stopped hard.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(90);
const SHUTDOWN_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const RECEIPT_LIMIT: usize = 64 * 1024;
const RECEIPT_PATH: &str = "/Library/Application Support/Silo/NetworkBootstrap/receipt.json";
const PROBE_URL: &str = "https://example.com/";
const PLACEHOLDERS_PATH: &str =
    "/Library/Application Support/Silo/NetworkBootstrap/placeholders.zsh";
const PLACEHOLDER_NAMES_PATH: &str =
    "/Library/Application Support/Silo/NetworkBootstrap/placeholder-names.txt";
const RETIRED_NAMES_PATH: &str =
    "/Library/Application Support/Silo/NetworkBootstrap/retired-names.txt";
static PLACEHOLDER_SYNC_LOCKS: OnceLock<Mutex<HashMap<String, Weak<Mutex<()>>>>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BootstrapIdentity {
    schema_version: u32,
    computer_id: String,
    slot: u16,
    guest_ipv4: std::net::Ipv4Addr,
    gateway_ipv4: std::net::Ipv4Addr,
    dns_ipv4: std::net::Ipv4Addr,
    guest_mac: String,
    ca_fingerprint: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuestConfig<'a> {
    version: u32,
    computer_id: &'a str,
    expected_mac: &'a str,
    address: std::net::Ipv4Addr,
    prefix_length: u8,
    router: std::net::Ipv4Addr,
    dns_server: std::net::Ipv4Addr,
    ca_fingerprint: &'a str,
    /// The guest helper decodes `probeURL` (capital URL), which camelCase would not write.
    #[serde(rename = "probeURL")]
    probe_url: &'static str,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GuestReceipt {
    schema_version: u32,
    computer_id: String,
    bootstrap_version: u32,
    ca_fingerprint: String,
    status: String,
    observed_at: String,
    interface: Option<String>,
    service_id: Option<String>,
    checks: std::collections::BTreeMap<String, bool>,
    failure: Option<String>,
}

fn expected_identity(
    record: &Record,
    address: ProxyAddress,
    ca: &CaMaterial,
) -> Result<BootstrapIdentity, String> {
    Ok(BootstrapIdentity {
        schema_version: 1,
        computer_id: record.id.clone(),
        slot: address.slot(),
        guest_ipv4: address.guest(),
        gateway_ipv4: address.host(),
        dns_ipv4: address.host(),
        guest_mac: record.mac_address.to_ascii_lowercase(),
        ca_fingerprint: ca.certificate_fingerprint()?,
    })
}

/// Stage network files and per-computer trust while provisioning already owns a reachable guest.
/// The caller owns the guest lifecycle and must stop it before switching to FileHandle networking.
pub(in crate::macos_computers_engine) fn stage_guest_bootstrap(
    app: &MacosContext,
    record: &Record,
    layout: &Layout,
) -> Result<(), String> {
    let routes = proxy_address::host_ipv4_routes()?;
    let address = proxy_address::load_or_allocate(layout, &routes)?;
    let ca = proxy_ca::get_or_create(&record.id, || super::generate_ca(app))?;
    let identity = expected_identity(record, address, &ca)?;
    let placeholder_names = host_placeholder_names(&record.name)?;
    match read_identity(layout)? {
        Some(saved) if saved == identity => return Ok(()),
        Some(_) => {
            return Err(
                "The saved macOS proxy network identity differs from its Keychain CA or guest address."
                    .into(),
            );
        }
        None => {}
    }

    install_guest_files(
        app,
        record,
        layout,
        &address,
        &ca,
        &identity,
        &placeholder_names,
    )?;
    write_identity(layout, &identity)
}

/// Convert a pre-proxy installation during the launch migration only.
/// Ordinary Start must use `require_guest_bootstrap` and never perform this conversion.
pub(in crate::macos_computers_engine) fn migrate_legacy_guest_bootstrap(
    app: &MacosContext,
    record: &Record,
    layout: &Layout,
) -> Result<(), String> {
    let routes = proxy_address::host_ipv4_routes()?;
    let address = proxy_address::load_or_allocate(layout, &routes)?;
    let ca = proxy_ca::get_or_create(&record.id, || super::generate_ca(app))?;
    let identity = expected_identity(record, address, &ca)?;
    if read_identity(layout)?.as_ref() == Some(&identity) {
        return Ok(());
    }
    if read_identity(layout)?.is_some() {
        return Err(
            "The saved macOS proxy network identity differs from its Keychain CA or guest address."
                .into(),
        );
    }
    let generation = engine::start_bootstrap_nat(app, record, layout)?;
    let staged = stage_guest_bootstrap(app, record, layout);
    // A force stop drops the guest's recent writes: the keychain trust, the launch daemon and
    // the git configuration the script installs last were lost that way. Ask macOS to shut
    // down first and force the stop only when it does not.
    let mut shut_down = false;
    if staged.is_ok() {
        let _ = guest_access::run(
            layout,
            record,
            "/usr/bin/sudo -n /sbin/shutdown -h now",
            None,
            SHUTDOWN_COMMAND_TIMEOUT,
        );
        shut_down = engine::wait_until_released(app, &record.id, generation, SHUTDOWN_WAIT);
    }
    let stop = if shut_down {
        Ok(())
    } else {
        engine::force_stop_generation(app, &record.id, generation)
    };
    let released = shut_down || engine::wait_until_released(app, &record.id, generation, STOP_WAIT);
    if !released {
        return Err("The NAT bootstrap computer could not be stopped before gateway setup.".into());
    }
    stop?;
    staged
}

/// Validate every persisted network input without allocating or generating Keychain state.
/// Ordinary Start calls this before constructing a FileHandle attachment.
pub(in crate::macos_computers_engine) fn require_guest_bootstrap(
    record: &Record,
    layout: &Layout,
) -> Result<(), String> {
    let address = proxy_address::load_existing(layout, &proxy_address::host_ipv4_routes()?)?;
    let ca = proxy_ca::get_existing(&record.id)?
        .ok_or_else(|| "The macOS proxy CA is missing. Retry computer setup.".to_owned())?;
    let identity = expected_identity(record, address, &ca)?;
    if read_identity(layout)?.as_ref() != Some(&identity) {
        return Err(
            "The macOS proxy bootstrap identity is missing or has changed. Retry computer setup."
                .into(),
        );
    }
    Ok(())
}

/// Verify guest readiness over the private SSH loopback after a FileHandle boot.
pub(in crate::macos_computers_engine) fn verify_guest_ready(
    layout: &Layout,
    record: &Record,
    not_before: time::OffsetDateTime,
    require_fresh_receipt: bool,
) -> Result<(), String> {
    let address = proxy_address::load_existing(layout, &proxy_address::host_ipv4_routes()?)?;
    let ca = proxy_ca::get_existing(&record.id)?
        .ok_or_else(|| "The macOS proxy CA is missing. Retry computer setup.".to_owned())?;
    let identity = expected_identity(record, address, &ca)?;
    if read_identity(layout)?.as_ref() != Some(&identity) {
        return Err("The macOS proxy bootstrap identity is missing or has changed.".into());
    }
    guest_access::wait_for_ssh(layout, record, GUEST_READY_WAIT, &|| false)?;
    let deadline = Instant::now() + GUEST_READY_WAIT;
    let command = format!(
        "sudo -n /bin/cat {}",
        guest_access::shell_quote(RECEIPT_PATH)
    );
    let mut last_failure = None;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match guest_access::run_capped(
            layout,
            record,
            &command,
            None,
            remaining.min(Duration::from_secs(20)),
            Some(RECEIPT_LIMIT),
        ) {
            Ok(output) if output.status == 0 => {
                let receipt: GuestReceipt = serde_json::from_str(&output.stdout)
                    .map_err(|_| "The guest network bootstrap receipt is invalid.".to_owned())?;
                match validate_receipt(
                    record,
                    &identity,
                    &receipt,
                    not_before,
                    require_fresh_receipt,
                ) {
                    Ok(()) => return Ok(()),
                    Err(error) => last_failure = Some(error),
                }
            }
            Ok(_) => {
                last_failure =
                    Some("The guest network bootstrap has not written a readiness receipt.".into())
            }
            Err(error) if error == guest_access::OUTPUT_TOO_LARGE => return Err(error),
            Err(error) => last_failure = Some(error),
        }
        std::thread::sleep(
            Duration::from_secs(1).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
    Err(last_failure.unwrap_or_else(|| {
        "The guest network bootstrap did not become ready before the deadline.".into()
    }))
}

fn validate_receipt(
    record: &Record,
    identity: &BootstrapIdentity,
    receipt: &GuestReceipt,
    not_before: time::OffsetDateTime,
    require_fresh_receipt: bool,
) -> Result<(), String> {
    const REQUIRED_CHECKS: &[&str] = &[
        "ipv6DisabledInPreferences",
        "activeIPv4",
        "connectedRoute",
        "defaultRoute",
        "serviceDNS",
        "globalDNS",
        "resolverFile",
        "scutilDNS",
        "ipv6Disabled",
        "proxyCATrusted",
        "explicitDig",
        "plainDig",
        "getaddrinfo",
        "dscacheutil",
        "nativeHTTPS",
    ];
    let checks_ready = REQUIRED_CHECKS
        .iter()
        .all(|name| receipt.checks.get(*name) == Some(&true));
    let service_identity_ready = receipt
        .interface
        .as_deref()
        .is_some_and(valid_receipt_identity)
        && receipt
            .service_id
            .as_deref()
            .is_some_and(valid_receipt_identity);
    if receipt.schema_version != 1
        || receipt.computer_id != record.id
        || receipt.bootstrap_version != BOOTSTRAP_VERSION
        || receipt.ca_fingerprint != identity.ca_fingerprint
        || receipt.status != "ready"
        || receipt.failure.is_some()
        || !checks_ready
        || !service_identity_ready
    {
        return Err(receipt.failure.as_ref().map_or_else(
            || "The guest network bootstrap is not ready on the FileHandle network.".into(),
            |failure| format!("The guest network bootstrap failed: {failure}."),
        ));
    }
    if require_fresh_receipt {
        let observed = time::OffsetDateTime::parse(
            &receipt.observed_at,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|_| "The guest network bootstrap receipt has an invalid timestamp.".to_owned())?;
        let now = time::OffsetDateTime::now_utc();
        if observed < not_before - time::Duration::seconds(5)
            || observed > now + time::Duration::seconds(5)
        {
            return Err("The guest network bootstrap receipt is stale for this start.".into());
        }
    }
    Ok(())
}

fn valid_receipt_identity(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

fn read_identity(layout: &Layout) -> Result<Option<BootstrapIdentity>, String> {
    let path = layout.dir.join(IDENTITY_FILE);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot read the saved macOS proxy bootstrap identity.".into()),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "The saved macOS proxy bootstrap identity is invalid.".into())
}

fn write_identity(layout: &Layout, identity: &BootstrapIdentity) -> Result<(), String> {
    fs::create_dir_all(&layout.dir)
        .map_err(|_| "Cannot save the macOS proxy bootstrap identity.".to_owned())?;
    let path = layout.dir.join(IDENTITY_FILE);
    let parent = path
        .parent()
        .ok_or_else(|| "Cannot save the macOS proxy bootstrap identity.".to_owned())?;
    let mut temporary = Builder::new()
        .prefix("proxy-bootstrap-")
        .tempfile_in(parent)
        .map_err(|_| "Cannot save the macOS proxy bootstrap identity.".to_owned())?;
    serde_json::to_writer(&mut temporary, identity)
        .map_err(|_| "Cannot save the macOS proxy bootstrap identity.".to_owned())?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "Cannot save the macOS proxy bootstrap identity.".to_owned())?;
    temporary
        .persist(&path)
        .map_err(|_| "Cannot save the macOS proxy bootstrap identity.".to_owned())?;
    fs::File::open(parent)
        .and_then(|parent| parent.sync_all())
        .map_err(|_| "Cannot save the macOS proxy bootstrap identity.".to_owned())
}

/// Refresh the placeholder-only environment after live secret changes or a computer start.
pub(in crate::macos_computers_engine) fn sync_guest_placeholders(
    _app: &MacosContext,
    layout: &Layout,
    record: &Record,
) -> Result<(), String> {
    with_placeholder_sync_lock(&record.id, || {
        sync_guest_placeholders_locked(layout, record)
    })
}

fn sync_guest_placeholders_locked(layout: &Layout, record: &Record) -> Result<(), String> {
    let Some(mut names) = super::guest_secret_names(&record.id)? else {
        return Ok(());
    };
    guest_access::wait_for_ssh(layout, record, SSH_WAIT, &|| false)?;
    names.extend([
        "GH_TOKEN".to_owned(),
        "NODE_EXTRA_CA_CERTS".to_owned(),
        "SSL_CERT_FILE".to_owned(),
        "REQUESTS_CA_BUNDLE".to_owned(),
        "CURL_CA_BUNDLE".to_owned(),
        "GIT_SSL_CAINFO".to_owned(),
    ]);
    let (script, names_text) = placeholder_environment(&names)?;
    let existing = guest_access::run_capped(
        layout,
        record,
        &format!(
            "sudo -n /bin/cat {}",
            guest_access::shell_quote(PLACEHOLDER_NAMES_PATH)
        ),
        None,
        Duration::from_secs(10),
        Some(16 * 1024),
    )
    .ok()
    .filter(|output| output.status == 0)
    .map(|output| output.stdout)
    .unwrap_or_default();
    let retired_names = retired_environment_names(&existing, &names_text);

    let temporary = Builder::new()
        .prefix("proxy-placeholders-")
        .tempdir_in(&layout.dir)
        .map_err(|_| "Cannot prepare macOS computer placeholders.".to_owned())?;
    let script_path = temporary.path().join("placeholders.zsh");
    let names_path = temporary.path().join("placeholder-names.txt");
    let retired_path = temporary.path().join("retired-names.txt");
    write_private(&script_path, script.as_bytes())?;
    write_private(&names_path, names_text.as_bytes())?;
    write_private(&retired_path, retired_names.as_bytes())?;
    guest_access::copy(
        layout,
        record,
        &script_path,
        "/tmp/silo-proxy-placeholders.zsh",
        SSH_COMMAND_TIMEOUT,
    )?;
    guest_access::copy(
        layout,
        record,
        &names_path,
        "/tmp/silo-proxy-placeholder-names.txt",
        SSH_COMMAND_TIMEOUT,
    )?;
    guest_access::copy(
        layout,
        record,
        &retired_path,
        "/tmp/silo-proxy-retired-names.txt",
        SSH_COMMAND_TIMEOUT,
    )?;

    let command = format!(
        "set -eu\n/usr/bin/install -o root -g wheel -m 0644 /tmp/silo-proxy-placeholders.zsh {}\n/usr/bin/install -o root -g wheel -m 0644 /tmp/silo-proxy-placeholder-names.txt {}\n/usr/bin/install -o root -g wheel -m 0644 /tmp/silo-proxy-retired-names.txt {}\n/bin/rm -f /tmp/silo-proxy-placeholders.zsh /tmp/silo-proxy-placeholder-names.txt /tmp/silo-proxy-retired-names.txt\nuid=$(/usr/bin/id -u silo)\nif /bin/launchctl print gui/$uid >/dev/null 2>&1; then /bin/launchctl asuser $uid /usr/local/libexec/silo-proxy-environment.zsh >/dev/null 2>&1 || true; fi\n",
        guest_access::shell_quote(PLACEHOLDERS_PATH),
        guest_access::shell_quote(PLACEHOLDER_NAMES_PATH),
        guest_access::shell_quote(RETIRED_NAMES_PATH),
    );
    let output = guest_access::run(
        layout,
        record,
        "sudo -n /bin/sh -s",
        Some(command.as_bytes()),
        SSH_COMMAND_TIMEOUT,
    )?;
    if output.status != 0 {
        return Err("Could not update the guest's placeholder environment.".into());
    }
    Ok(())
}

fn with_placeholder_sync_lock<T>(
    computer_id: &str,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let lock = {
        let mut locks = PLACEHOLDER_SYNC_LOCKS
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

fn host_placeholder_names(computer: &str) -> Result<Vec<String>, String> {
    let mut material = crate::secrets::runtime_material(computer)?;
    let mut names = BTreeSet::new();
    for (name, value, _) in &mut material {
        names.insert(name.clone());
        value.zeroize();
    }
    Ok(names.into_iter().collect())
}

fn placeholder_environment(names: &[String]) -> Result<(String, String), String> {
    let mut names = names.iter().cloned().collect::<BTreeSet<_>>();
    names.extend([
        "GH_TOKEN".to_owned(),
        "NODE_EXTRA_CA_CERTS".to_owned(),
        "SSL_CERT_FILE".to_owned(),
        "REQUESTS_CA_BUNDLE".to_owned(),
        "CURL_CA_BUNDLE".to_owned(),
        "GIT_SSL_CAINFO".to_owned(),
    ]);
    if names.iter().any(|name| !valid_environment_name(name)) {
        return Err("The macOS computer has an invalid placeholder name.".into());
    }
    let ca_bundle = "/Library/Application Support/Silo/NetworkBootstrap/ca-bundle.pem";
    let proxy_ca = "/Library/Application Support/Silo/NetworkBootstrap/proxy-ca.pem";
    let mut script = String::new();
    for name in &names {
        let value = match name.as_str() {
            "GH_TOKEN" => "$MSB_SILO_GITHUB".to_owned(),
            "NODE_EXTRA_CA_CERTS" => proxy_ca.to_owned(),
            "SSL_CERT_FILE" | "REQUESTS_CA_BUNDLE" | "CURL_CA_BUNDLE" | "GIT_SSL_CAINFO" => {
                ca_bundle.to_owned()
            }
            _ => format!("$MSB_{name}"),
        };
        script.push_str(&format!("export {name}='{value}'\n"));
    }
    let names_text = names.into_iter().collect::<Vec<_>>().join("\n") + "\n";
    Ok((script, names_text))
}

fn valid_environment_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn retired_environment_names(existing: &str, active: &str) -> String {
    let active_names = active.lines().collect::<BTreeSet<_>>();
    let retired = existing
        .lines()
        .filter(|name| valid_environment_name(name) && !active_names.contains(name))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("\n");
    if retired.is_empty() {
        retired
    } else {
        format!("{retired}\n")
    }
}

/// Installs the staged files in the guest. The trust-probe leaf and the CA are both
/// installed before the daemon, which is installed last, can run.
const INSTALL_SCRIPT: &str = r#"set -eu
/bin/launchctl bootout system/com.silo.network-bootstrap >/dev/null 2>&1 || true
/usr/bin/install -d -o root -g wheel -m 0755 /Library/PrivilegedHelperTools /usr/local/libexec /Library/LaunchAgents "/Library/Application Support/Silo/NetworkBootstrap"
/usr/bin/install -o root -g wheel -m 0755 /tmp/silo-network-bootstrap /Library/PrivilegedHelperTools/silo-network-bootstrap
/usr/bin/install -o root -g wheel -m 0644 /tmp/silo-network-bootstrap-config.json "/Library/Application Support/Silo/NetworkBootstrap/config.json"
old_ca="/Library/Application Support/Silo/NetworkBootstrap/proxy-ca.pem"
if [ -s "$old_ca" ]; then
    old_sha256=$(/usr/bin/openssl x509 -in "$old_ca" -noout -fingerprint -sha256 | /usr/bin/sed 's/^[^=]*=//' | /usr/bin/tr -d ':' | /usr/bin/tr '[:upper:]' '[:lower:]')
    new_sha256=$(/usr/bin/openssl x509 -in /tmp/silo-proxy-ca.pem -noout -fingerprint -sha256 | /usr/bin/sed 's/^[^=]*=//' | /usr/bin/tr -d ':' | /usr/bin/tr '[:upper:]' '[:lower:]')
    if [ "$old_sha256" != "$new_sha256" ]; then
        /bin/sh /tmp/silo-remove-inherited-proxy-ca.sh "$old_sha256" /Library/Keychains/System.keychain
    fi
fi
/usr/bin/install -o root -g wheel -m 0644 /tmp/silo-proxy-ca.pem "/Library/Application Support/Silo/NetworkBootstrap/proxy-ca.pem"
/usr/bin/install -o root -g wheel -m 0644 /tmp/silo-proxy-ca-probe.pem "/Library/Application Support/Silo/NetworkBootstrap/proxy-ca-probe.pem"
/usr/bin/install -o root -g wheel -m 0755 /tmp/silo-proxy-environment.zsh /usr/local/libexec/silo-proxy-environment.zsh
/usr/bin/install -o root -g wheel -m 0755 /tmp/silo-github-credential.sh /usr/local/libexec/silo-github-credential
/usr/bin/install -o root -g wheel -m 0644 /tmp/com.silo.proxy-environment.plist /Library/LaunchAgents/com.silo.proxy-environment.plist
/usr/bin/install -o root -g wheel -m 0644 /tmp/silo-proxy-placeholders.zsh "/Library/Application Support/Silo/NetworkBootstrap/placeholders.zsh"
/usr/bin/install -o root -g wheel -m 0644 /tmp/silo-proxy-placeholder-names.txt "/Library/Application Support/Silo/NetworkBootstrap/placeholder-names.txt"
/usr/bin/install -o root -g wheel -m 0644 /tmp/silo-proxy-retired-names.txt "/Library/Application Support/Silo/NetworkBootstrap/retired-names.txt"
/usr/bin/security find-certificate -a -p /System/Library/Keychains/SystemRootCertificates.keychain > "/Library/Application Support/Silo/NetworkBootstrap/system-roots.pem"
/bin/cat "/Library/Application Support/Silo/NetworkBootstrap/system-roots.pem" "/Library/Application Support/Silo/NetworkBootstrap/proxy-ca.pem" > "/Library/Application Support/Silo/NetworkBootstrap/ca-bundle.pem.tmp"
/usr/bin/install -o root -g wheel -m 0644 "/Library/Application Support/Silo/NetworkBootstrap/ca-bundle.pem.tmp" "/Library/Application Support/Silo/NetworkBootstrap/ca-bundle.pem"
/bin/rm -f "/Library/Application Support/Silo/NetworkBootstrap/system-roots.pem" "/Library/Application Support/Silo/NetworkBootstrap/ca-bundle.pem.tmp"
/usr/bin/security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain "/Library/Application Support/Silo/NetworkBootstrap/proxy-ca.pem"
/usr/bin/git config --system --replace-all credential.https://github.com.helper /usr/local/libexec/silo-github-credential
file=/Users/silo/.zshenv
line='[[ -r "/Library/Application Support/Silo/NetworkBootstrap/placeholders.zsh" ]] && source "/Library/Application Support/Silo/NetworkBootstrap/placeholders.zsh"'
[ ! -L "$file" ] || exit 1
[ -e "$file" ] || /usr/bin/install -o silo -g staff -m 0644 /dev/null "$file"
/usr/bin/grep -Fqx "$line" "$file" || /usr/bin/printf '\n%s\n' "$line" >> "$file"
/usr/sbin/chown silo:staff "$file"
/bin/rm -f "/Library/Application Support/Silo/NetworkBootstrap/receipt.json"
/usr/bin/install -o root -g wheel -m 0644 /tmp/silo-network-bootstrap.plist /Library/LaunchDaemons/com.silo.network-bootstrap.plist
/bin/rm -f /tmp/silo-network-bootstrap /tmp/silo-network-bootstrap-config.json /tmp/silo-proxy-ca.pem /tmp/silo-proxy-ca-probe.pem /tmp/silo-remove-inherited-proxy-ca.sh /tmp/silo-network-bootstrap.plist /tmp/silo-proxy-environment.zsh /tmp/silo-github-credential.sh /tmp/com.silo.proxy-environment.plist /tmp/silo-proxy-placeholders.zsh /tmp/silo-proxy-placeholder-names.txt /tmp/silo-proxy-retired-names.txt
"#;

fn install_guest_files(
    app: &MacosContext,
    record: &Record,
    layout: &Layout,
    address: &ProxyAddress,
    ca: &CaMaterial,
    identity: &BootstrapIdentity,
    placeholder_names: &[String],
) -> Result<(), String> {
    let helper = bootstrap_binary(app)?;
    // A leaf signed by this CA lets the guest prove its trust settings accept the CA, not
    // just that the certificate is present.
    let trust_probe = super::issue_trust_probe(app, ca)?;
    guest_access::wait_for_ssh(layout, record, SSH_WAIT, &|| false)?;
    let old_names = guest_access::run_capped(
        layout,
        record,
        &format!(
            "sudo -n /bin/cat {}",
            guest_access::shell_quote(PLACEHOLDER_NAMES_PATH)
        ),
        None,
        Duration::from_secs(10),
        Some(16 * 1024),
    )
    .ok()
    .filter(|output| output.status == 0)
    .map(|output| output.stdout)
    .unwrap_or_default();
    guest_access::copy(
        layout,
        record,
        &helper,
        "/tmp/silo-network-bootstrap",
        SSH_COMMAND_TIMEOUT,
    )?;

    let config = GuestConfig {
        version: BOOTSTRAP_VERSION,
        computer_id: &record.id,
        expected_mac: &record.mac_address,
        address: address.guest(),
        prefix_length: 30,
        router: address.host(),
        dns_server: address.host(),
        ca_fingerprint: &identity.ca_fingerprint,
        probe_url: PROBE_URL,
    };
    let temporary = Builder::new()
        .prefix("proxy-network-bootstrap-")
        .tempdir_in(&layout.dir)
        .map_err(|_| "Cannot prepare macOS proxy bootstrap files.".to_owned())?;
    let config_path = temporary.path().join("config.json");
    let ca_path = temporary.path().join("proxy-ca.pem");
    let trust_probe_path = temporary.path().join("proxy-ca-probe.pem");
    let ca_cleanup_path = temporary.path().join("remove-inherited-proxy-ca.sh");
    let plist_path = temporary.path().join("com.silo.network-bootstrap.plist");
    let environment_path = temporary.path().join("silo-proxy-environment.zsh");
    let credential_path = temporary.path().join("silo-github-credential.sh");
    let environment_agent_path = temporary.path().join("com.silo.proxy-environment.plist");
    let placeholders_path = temporary.path().join("placeholders.zsh");
    let names_path = temporary.path().join("placeholder-names.txt");
    let retired_path = temporary.path().join("retired-names.txt");
    write_private(
        &config_path,
        &serde_json::to_vec_pretty(&config)
            .map_err(|_| "Cannot encode macOS proxy bootstrap configuration.".to_owned())?,
    )?;
    write_private(&ca_path, ca.certificate_pem().as_bytes())?;
    write_private(&trust_probe_path, trust_probe.as_bytes())?;
    write_private(
        &ca_cleanup_path,
        include_bytes!("../../../../guest/macos/remove-inherited-proxy-ca.sh"),
    )?;
    write_private(
        &plist_path,
        include_bytes!("../../../../guest/macos/com.silo.network-bootstrap.plist"),
    )?;
    write_private(
        &environment_path,
        include_bytes!("../../../../guest/macos/silo-proxy-environment.zsh"),
    )?;
    write_private(
        &credential_path,
        include_bytes!("../../../../guest/macos/silo-github-credential.sh"),
    )?;
    write_private(
        &environment_agent_path,
        include_bytes!("../../../../guest/macos/com.silo.proxy-environment.plist"),
    )?;
    let (placeholder_script, placeholder_names) = placeholder_environment(placeholder_names)?;
    write_private(&placeholders_path, placeholder_script.as_bytes())?;
    write_private(&names_path, placeholder_names.as_bytes())?;
    let retired_names = retired_environment_names(&old_names, &placeholder_names);
    write_private(&retired_path, retired_names.as_bytes())?;
    guest_access::copy(
        layout,
        record,
        &config_path,
        "/tmp/silo-network-bootstrap-config.json",
        SSH_COMMAND_TIMEOUT,
    )?;
    guest_access::copy(
        layout,
        record,
        &ca_path,
        "/tmp/silo-proxy-ca.pem",
        SSH_COMMAND_TIMEOUT,
    )?;
    guest_access::copy(
        layout,
        record,
        &trust_probe_path,
        "/tmp/silo-proxy-ca-probe.pem",
        SSH_COMMAND_TIMEOUT,
    )?;
    guest_access::copy(
        layout,
        record,
        &ca_cleanup_path,
        "/tmp/silo-remove-inherited-proxy-ca.sh",
        SSH_COMMAND_TIMEOUT,
    )?;
    guest_access::copy(
        layout,
        record,
        &plist_path,
        "/tmp/silo-network-bootstrap.plist",
        SSH_COMMAND_TIMEOUT,
    )?;
    guest_access::copy(
        layout,
        record,
        &environment_path,
        "/tmp/silo-proxy-environment.zsh",
        SSH_COMMAND_TIMEOUT,
    )?;
    guest_access::copy(
        layout,
        record,
        &credential_path,
        "/tmp/silo-github-credential.sh",
        SSH_COMMAND_TIMEOUT,
    )?;
    guest_access::copy(
        layout,
        record,
        &environment_agent_path,
        "/tmp/com.silo.proxy-environment.plist",
        SSH_COMMAND_TIMEOUT,
    )?;
    guest_access::copy(
        layout,
        record,
        &placeholders_path,
        "/tmp/silo-proxy-placeholders.zsh",
        SSH_COMMAND_TIMEOUT,
    )?;
    guest_access::copy(
        layout,
        record,
        &names_path,
        "/tmp/silo-proxy-placeholder-names.txt",
        SSH_COMMAND_TIMEOUT,
    )?;
    guest_access::copy(
        layout,
        record,
        &retired_path,
        "/tmp/silo-proxy-retired-names.txt",
        SSH_COMMAND_TIMEOUT,
    )?;

    let script = INSTALL_SCRIPT;

    let output = guest_access::run(
        layout,
        record,
        "sudo -n /bin/sh -s",
        Some(script.as_bytes()),
        SSH_COMMAND_TIMEOUT,
    )?;
    if output.status != 0 {
        // The script holds only public material, so its output is safe to keep for diagnosis.
        if let Some(log) = super::super::setup_log::SetupLog::open(app, &record.id) {
            log.command(
                "proxy bootstrap install script",
                output.status,
                &output.stdout,
                &output.stderr,
            );
        }
        return Err("Could not install the guest network bootstrap and public CA.".into());
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|_| "Cannot write macOS proxy bootstrap files.".to_owned())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|_| "Cannot protect macOS proxy bootstrap files.".to_owned())
}

fn bootstrap_binary(app: &MacosContext) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(current) = std::env::current_exe() {
        if let Some(directory) = current.parent() {
            candidates.push(directory.join("silo-network-bootstrap"));
        }
    }
    candidates.push(app.paths().resources.join("silo-network-bootstrap"));
    candidates.push(
        app.paths()
            .resources
            .join("binaries")
            .join("silo-network-bootstrap"),
    );
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| "The signed guest network bootstrap is missing from this build.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Record {
        let request = super::super::super::store::CreateRequest {
            name: "Computer A".into(),
            cpus: 2,
            memory_gib: 4,
            disk_gib: 64,
        };
        let mut record =
            super::super::super::store::new_record(&request, "02:00:00:00:00:02".into());
        record.id = "computer-a".into();
        record.installed = true;
        record
    }

    fn address() -> ProxyAddress {
        let directory = tempfile::tempdir().unwrap();
        let layout = Layout {
            dir: directory.path().join("computer-a"),
            access: None,
        };
        proxy_address::load_or_allocate(&layout, &[]).unwrap()
    }

    fn ca() -> CaMaterial {
        CaMaterial::from_pem(
            "-----BEGIN CERTIFICATE-----\nY2VydA==\n-----END CERTIFICATE-----\n".into(),
            "-----BEGIN PRIVATE KEY-----\na2V5\n-----END PRIVATE KEY-----\n".into(),
        )
        .unwrap()
    }

    #[test]
    fn the_guest_config_has_the_keys_the_guest_helper_decodes() {
        // The helper's `BootstrapConfig` is case-sensitive: a key spelled `probeUrl` made it
        // fail with `configuration_read_failed` on every boot (live qualification, 2026-10-14).
        let example: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../guest/macos/network-bootstrap-config.example.json"
        ))
        .unwrap();
        let address = address();
        let written = serde_json::to_value(GuestConfig {
            version: BOOTSTRAP_VERSION,
            computer_id: "computer-a",
            expected_mac: "02:00:00:00:00:02",
            address: address.guest(),
            prefix_length: 30,
            router: address.host(),
            dns_server: address.host(),
            ca_fingerprint: "00",
            probe_url: PROBE_URL,
        })
        .unwrap();
        let keys = |value: &serde_json::Value| {
            let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
        assert_eq!(keys(&written), keys(&example));
    }

    #[test]
    fn guest_bootstrap_identity_is_bound_to_computer_address_mac_and_ca() {
        let expected = expected_identity(&record(), address(), &ca()).unwrap();
        assert_eq!(expected.computer_id, "computer-a");
        assert_eq!(
            u32::from(expected.guest_ipv4) - u32::from(expected.gateway_ipv4),
            1
        );
        assert_eq!(expected.guest_ipv4.octets()[3] & 0x03, 2);
        assert_eq!(expected.guest_mac, "02:00:00:00:00:02");
        assert_eq!(expected.ca_fingerprint.len(), 64);
    }

    #[test]
    fn placeholder_policy_exports_only_references_and_retires_removed_names() {
        let (script, names) = placeholder_environment(&["API_KEY".into()]).unwrap();
        assert!(script.contains("export API_KEY='$MSB_API_KEY'"));
        assert!(script.contains("export GH_TOKEN='$MSB_SILO_GITHUB'"));
        assert!(script.contains("export NODE_EXTRA_CA_CERTS='/Library/Application Support/Silo/NetworkBootstrap/proxy-ca.pem'"));
        assert!(script.contains("export SSL_CERT_FILE='/Library/Application Support/Silo/NetworkBootstrap/ca-bundle.pem'"));
        assert!(!script.contains("synthetic-value"));
        assert!(names.lines().any(|name| name == "API_KEY"));
        assert_eq!(
            retired_environment_names("API_KEY\nOLD_KEY\nGH_TOKEN\nBAD-NAME\n", &names),
            "OLD_KEY\n"
        );
        assert_eq!(retired_environment_names("OLD_KEY\n", &names), "OLD_KEY\n");
        assert_eq!(retired_environment_names("API_KEY\n", &names), "");
    }

    /// The host stages the trust-probe leaf where the guest helper reads it, and the helper
    /// evaluates it for the name the gateway signs, so a present-but-untrusted CA cannot pass.
    #[test]
    fn the_guest_trust_probe_is_staged_where_the_helper_evaluates_it() {
        let swift = include_str!("../../../../guest/macos/silo-network-bootstrap.swift");
        let gateway = include_str!("../../../../macos-gateway/src/network.rs");
        assert!(INSTALL_SCRIPT.contains(
            "/tmp/silo-proxy-ca-probe.pem \"/Library/Application Support/Silo/NetworkBootstrap/proxy-ca-probe.pem\""
        ));
        assert!(swift
            .contains("\"/Library/Application Support/Silo/NetworkBootstrap/proxy-ca-probe.pem\""));
        assert!(swift.contains("proxyCATrusted("));
        assert!(
            !swift.contains("trustedProxyCAExists"),
            "presence of the certificate must not decide readiness"
        );
        let host = "silo-ca-trust-probe.invalid";
        assert!(swift.contains(&format!("let proxyCAProbeHost = \"{host}\"")));
        assert!(gateway.contains(&format!("TRUST_PROBE_HOST: &str = \"{host}\"")));
        // The leaf is installed with the CA, before the daemon that reads both.
        let leaf = INSTALL_SCRIPT.find("proxy-ca-probe.pem\"").unwrap();
        let daemon = INSTALL_SCRIPT
            .find("/Library/LaunchDaemons/com.silo.network-bootstrap.plist")
            .unwrap();
        assert!(leaf < daemon);
    }

    #[test]
    fn bootstrap_receipt_is_fresh_and_contains_every_readiness_check() {
        let record = record();
        let identity = expected_identity(&record, address(), &ca()).unwrap();
        let now = time::OffsetDateTime::now_utc();
        let checks = [
            "ipv6DisabledInPreferences",
            "activeIPv4",
            "connectedRoute",
            "defaultRoute",
            "serviceDNS",
            "globalDNS",
            "resolverFile",
            "scutilDNS",
            "ipv6Disabled",
            "proxyCATrusted",
            "explicitDig",
            "plainDig",
            "getaddrinfo",
            "dscacheutil",
            "nativeHTTPS",
        ]
        .into_iter()
        .map(|name| (name.to_owned(), true))
        .collect();
        let receipt = GuestReceipt {
            schema_version: 1,
            computer_id: record.id.clone(),
            bootstrap_version: BOOTSTRAP_VERSION,
            ca_fingerprint: identity.ca_fingerprint.clone(),
            status: "ready".into(),
            observed_at: now
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            interface: Some("en7".into()),
            service_id: Some("service-id".into()),
            checks,
            failure: None,
        };
        assert!(validate_receipt(&record, &identity, &receipt, now, true).is_ok());

        let mut unidentified = receipt.clone();
        unidentified.service_id = None;
        assert!(validate_receipt(&record, &identity, &unidentified, now, true).is_err());

        let mut stale = receipt;
        stale.observed_at = (now - time::Duration::seconds(10))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        assert!(validate_receipt(&record, &identity, &stale, now, true).is_err());
    }
}
