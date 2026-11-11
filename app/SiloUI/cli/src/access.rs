//! Published ports and SSH names: what a person or an agent needs to reach a computer from this
//! device. Ports go through the engine's network methods; SSH names (`main.silo`) are an
//! `ssh_config` entry per computer that `silo ssh-setup` writes.

use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use serde_json::{json, Value};
use silo_api::methods::{
    approvals::ComputersAccessParams,
    common::{ComputerTargetParams, DeviceParams},
    guest::GuestSshAliasEnsureParams,
    network::{
        NetworkPolicy, NetworkPolicyMode, NetworkPolicySetParams, NetworkPublishParams,
        NetworkScheme, NetworkUnpublishParams,
    },
    requests,
    ssh_access::{SshAccessConnectionParams, SshAccessSaveParams},
};
use silo_client::channel::Channel;

use crate::{
    engine::{Computer, Engine},
    error::CliError,
};

// MARK: Network policy

/// What `silo network policy` changes; empty shows the policy.
#[derive(Debug, Default)]
pub struct PolicyChange {
    pub mode: Option<NetworkPolicyMode>,
    pub allow: Vec<String>,
    pub remove: Vec<String>,
    pub presets: Vec<String>,
}

impl PolicyChange {
    fn is_empty(&self) -> bool {
        self.mode.is_none()
            && self.allow.is_empty()
            && self.remove.is_empty()
            && self.presets.is_empty()
    }

    /// `policy` with this change: hosts and presets added, then removed (a name with no dot is
    /// a preset), then the mode.
    fn apply(&self, mut policy: NetworkPolicy) -> NetworkPolicy {
        for host in &self.allow {
            let host = host.trim().to_ascii_lowercase();
            if !policy.hosts.contains(&host) {
                policy.hosts.push(host);
            }
        }
        for preset in &self.presets {
            if !policy.presets.contains(preset) {
                policy.presets.push(preset.clone());
            }
        }
        for removed in &self.remove {
            let removed = removed.trim().to_ascii_lowercase();
            policy.hosts.retain(|host| *host != removed);
            policy.presets.retain(|preset| *preset != removed);
        }
        if let Some(mode) = self.mode {
            policy.mode = mode;
        }
        policy
    }
}

/// `silo network policy <computer>`: shows a computer's network policy, or changes it.
pub async fn network_policy(
    engine: &Engine,
    name: &str,
    change: &PolicyChange,
) -> Result<Value, CliError> {
    let computer = engine.computer(name).await?;
    let current = engine
        .call(requests::network_policy_get(ComputerTargetParams {
            computer_id: computer.id.clone(),
            device_id: None,
        }))
        .await?;
    let state = if change.is_empty() {
        current
    } else {
        engine
            .call(requests::network_policy_set(NetworkPolicySetParams {
                computer_id: computer.id.clone(),
                device_id: None,
                policy: change.apply(serde_json::from_value(current["policy"].clone()).map_err(
                    |_| CliError::new("The engine returned an unreadable network policy."),
                )?),
            }))
            .await?
    };
    let mut value = state;
    value["computer"] = json!(computer.name);
    Ok(value)
}

// MARK: What a computer holds

/// `computers.access` for one computer, or for every computer the caller can see.
pub async fn held(engine: &Engine, name: Option<&str>) -> Result<Value, CliError> {
    let computers = match name {
        Some(name) => vec![engine.computer(name).await?],
        None => engine.computers().await?,
    };
    let mut rows = Vec::new();
    for computer in &computers {
        rows.push(
            engine
                .call(requests::computers_access(ComputersAccessParams {
                    computer_id: computer.id.clone(),
                }))
                .await?,
        );
    }
    Ok(json!({ "computers": rows }))
}

// MARK: Ports

fn scheme_of(name: &str) -> Option<NetworkScheme> {
    match name {
        "http" => Some(NetworkScheme::Http),
        "https" => Some(NetworkScheme::Https),
        _ => None,
    }
}

/// The saved published ports of a computer, as `computers.access` lists them: the read a
/// computer may make of itself and of the computers it made, unlike `network.state`.
async fn saved_ports(engine: &Engine, computer: &Computer) -> Result<Vec<Value>, CliError> {
    let held = engine
        .call(requests::computers_access(ComputersAccessParams {
            computer_id: computer.id.clone(),
        }))
        .await?;
    Ok(held["ports"]
        .as_array()
        .map(|ports| {
            ports
                .iter()
                .filter(|port| port["configured"] == true)
                .cloned()
                .collect()
        })
        .unwrap_or_default())
}

async fn ports_of(engine: &Engine, computer: &Computer) -> Result<Value, CliError> {
    let saved = saved_ports(engine, computer).await?;
    let urls = engine
        .call(requests::network_urls(ComputerTargetParams {
            computer_id: computer.id.clone(),
            device_id: None,
        }))
        .await
        .ok()
        .and_then(|reply| reply["urls"].as_array().cloned())
        .unwrap_or_default();
    let ports: Vec<Value> = saved
        .iter()
        .map(|port| {
            let url = urls
                .iter()
                .find(|url| url["port"] == port["port"])
                .map(|url| url["url"].clone())
                .unwrap_or(Value::Null);
            json!({
                "port": port["port"],
                "hostPort": port["hostPort"],
                "scheme": port["scheme"],
                "state": port["state"],
                "url": url,
            })
        })
        .collect();
    Ok(json!({ "computer": computer.name, "ports": ports }))
}

/// `silo ports <computer>`: the ports published from a computer and where they are reachable.
pub async fn ports(engine: &Engine, name: &str) -> Result<Value, CliError> {
    let computer = engine.computer(name).await?;
    ports_of(engine, &computer).await
}

fn check_port(port: u16) -> Result<(), CliError> {
    if port == 0 {
        return Err(CliError::new("A port is a number from 1 to 65535."));
    }
    Ok(())
}

async fn publish_one(
    engine: &Engine,
    computer: &Computer,
    port: u16,
    scheme: Option<NetworkScheme>,
) -> Result<(), CliError> {
    engine
        .call(requests::network_publish(NetworkPublishParams {
            computer_id: computer.id.clone(),
            device_id: None,
            port,
            host_port: None,
            scheme,
        }))
        .await?;
    Ok(())
}

/// `silo publish <computer> <port> [--http]`.
pub async fn publish(
    engine: &Engine,
    name: &str,
    port: u16,
    http: bool,
) -> Result<Value, CliError> {
    check_port(port)?;
    let computer = engine.computer(name).await?;
    publish_one(engine, &computer, port, http.then_some(NetworkScheme::Http)).await?;
    ports_of(engine, &computer).await
}

/// `silo unpublish <computer> <port>`.
pub async fn unpublish(engine: &Engine, name: &str, port: u16) -> Result<Value, CliError> {
    check_port(port)?;
    let computer = engine.computer(name).await?;
    engine
        .call(requests::network_unpublish(NetworkUnpublishParams {
            computer_id: computer.id.clone(),
            device_id: None,
            port,
        }))
        .await?;
    ports_of(engine, &computer).await
}

/// Publishes the ports a recipe names (as websites) on `computer`, skipping those already
/// published. Returns the ports it published.
pub async fn publish_recipe_ports(
    engine: &Engine,
    computer: &Computer,
    wanted: &[u16],
) -> Result<Vec<u16>, CliError> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let have: Vec<u64> = saved_ports(engine, computer)
        .await?
        .iter()
        .filter_map(|port| port["port"].as_u64())
        .collect();
    let mut published = Vec::new();
    for port in wanted.iter().copied().filter(|port| *port != 0) {
        if have.contains(&u64::from(port)) || published.contains(&port) {
            continue;
        }
        publish_one(engine, computer, port, Some(NetworkScheme::Http)).await?;
        published.push(port);
    }
    Ok(published)
}

/// Gives `fork` the ports published from `source`, with the same schemes; the host ports are
/// chosen again so the two never share one. Returns the ports it published.
pub async fn inherit_ports(
    engine: &Engine,
    source: &Computer,
    fork: &Computer,
) -> Result<Vec<u16>, CliError> {
    let mut published = Vec::new();
    for port in saved_ports(engine, source).await? {
        let Some(number) = port["port"].as_u64().and_then(|n| u16::try_from(n).ok()) else {
            continue;
        };
        let scheme = port["scheme"].as_str().and_then(scheme_of);
        publish_one(engine, fork, number, scheme).await?;
        published.push(number);
    }
    Ok(published)
}

// MARK: SSH names

/// The name `ssh` knows a computer by: `main.silo` (`main.silo-dev` from a development build).
pub fn ssh_host(name: &str, channel: Channel) -> Result<String, CliError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(CliError::new(format!(
            "{name} cannot be an SSH name (letters, digits, . - and _ only)."
        )));
    }
    Ok(format!("{name}.{}", channel.ssh_host_suffix()))
}

/// Splits what the engine prints as a shell command: single-quoted words and plain ones.
fn shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '\'' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(current);
    }
    words
}

/// Where `ssh -i KEY -p PORT user@address` reaches a computer.
#[derive(Debug, PartialEq, Eq)]
struct Endpoint {
    key: String,
    port: u16,
    user: String,
    address: String,
}

fn parse_connection(command: &str) -> Result<Endpoint, CliError> {
    let words = shell_words(command);
    let after = |flag: &str| {
        words
            .iter()
            .position(|word| word == flag)
            .and_then(|at| words.get(at + 1))
    };
    let invalid = || CliError::new("The engine returned an SSH connection that could not be read.");
    let key = after("-i").ok_or_else(invalid)?.clone();
    let port = after("-p")
        .and_then(|port| port.parse().ok())
        .ok_or_else(invalid)?;
    let (user, address) = words
        .last()
        .and_then(|target| target.split_once('@'))
        .ok_or_else(invalid)?;
    Ok(Endpoint {
        key,
        port,
        user: user.to_owned(),
        address: address.to_owned(),
    })
}

fn ssh_path(path: &str) -> Result<String, CliError> {
    if path.contains(['"', '\n', '\r']) {
        return Err(CliError::new("That path cannot be written to ssh_config."));
    }
    Ok(format!("\"{path}\""))
}

fn stanza(host: &str, endpoint: &Endpoint) -> Result<String, CliError> {
    Ok(format!(
        "Host {host}\n  HostName {}\n  Port {}\n  User {}\n  IdentityFile {}\n  IdentitiesOnly yes\n  IdentityAgent none\n  StrictHostKeyChecking no\n  UserKnownHostsFile /dev/null\n  LogLevel ERROR\n",
        endpoint.address,
        endpoint.port,
        endpoint.user,
        ssh_path(&endpoint.key)?,
    ))
}

/// `text` with the entry for `host` replaced by `entry` (added when there is none).
fn with_entry(text: &str, host: &str, entry: &str) -> String {
    let mut kept = String::new();
    let mut skipping = false;
    for line in text.split_inclusive('\n') {
        if let Some(names) = line.strip_prefix("Host ") {
            skipping = names.split_whitespace().eq([host]);
        }
        if !skipping {
            kept.push_str(line);
        }
    }
    if !kept.is_empty() && !kept.ends_with('\n') {
        kept.push('\n');
    }
    kept.push_str(entry);
    kept
}

fn has_entry(text: &str, host: &str) -> bool {
    text.lines()
        .filter_map(|line| line.strip_prefix("Host "))
        .any(|names| names.split_whitespace().any(|name| name == host))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let fail = |error: std::io::Error| {
        CliError::new(format!("Could not write {}: {error}.", path.display()))
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(fail)?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(fail)?;
    file.write_all(bytes).map_err(fail)
}

/// Makes `~/.ssh/config` read `file`, once, at the top.
fn install_include(home: &Path, file: &Path) -> Result<(), CliError> {
    let line = format!("Include {}", ssh_path(&file.to_string_lossy())?);
    let directory = home.join(".ssh");
    let config = directory.join("config");
    let manual = || {
        CliError::new(format!(
            "{} is a link Silo will not change. Add this line at its top: {line}",
            config.display()
        ))
    };
    if fs::symlink_metadata(&config).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(manual());
    }
    if !directory.exists() {
        fs::create_dir_all(&directory)
            .and_then(|()| fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)))
            .map_err(|error| CliError::new(format!("Could not create ~/.ssh: {error}.")))?;
    }
    let old = match fs::read_to_string(&config) {
        Ok(old) => old,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(CliError::new(format!(
                "Could not read {}: {error}.",
                config.display()
            )))
        }
    };
    if old.lines().any(|existing| existing.trim() == line) {
        return Ok(());
    }
    write_private(&config, format!("{line}\n{old}").as_bytes())
}

/// Takes `host` off the `Host` line of every other `.conf` entry beside `keep` (the engine's
/// per-computer macOS entries), so a name is answered by one computer.
fn release_name_elsewhere(keep: &Path, host: &str) -> Result<(), CliError> {
    let Some(directory) = keep.parent() else {
        return Ok(());
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep || path.extension().is_none_or(|ext| ext != "conf") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Some((first, rest)) = text.split_once('\n') else {
            continue;
        };
        let Some(names) = first.strip_prefix("Host ") else {
            continue;
        };
        if !names.split_whitespace().any(|name| name == host) {
            continue;
        }
        let names: Vec<&str> = names.split_whitespace().filter(|n| *n != host).collect();
        write_private(
            &path,
            format!("Host {}\n{rest}", names.join(" ")).as_bytes(),
        )?;
    }
    Ok(())
}

/// Adds `host` to the first `Host` line of the engine's entry in `config`.
fn add_name_to_entry(config: &Path, host: &str) -> Result<(), CliError> {
    let text = fs::read_to_string(config)
        .map_err(|error| CliError::new(format!("Could not read {}: {error}.", config.display())))?;
    let Some((first, rest)) = text.split_once('\n') else {
        return Err(CliError::new("The engine wrote an unreadable SSH entry."));
    };
    if first
        .strip_prefix("Host ")
        .is_some_and(|names| names.split_whitespace().any(|name| name == host))
    {
        return Ok(());
    }
    if !first.starts_with("Host ") {
        return Err(CliError::new("The engine wrote an unreadable SSH entry."));
    }
    write_private(config, format!("{first} {host}\n{rest}").as_bytes())
}

fn aliases_file(home: &Path, channel: Channel) -> PathBuf {
    channel.state_dir(home).join("ssh-hosts.conf")
}

/// Whether `silo ssh-setup` wrote an entry for this computer's name (so a promote or undo
/// should write it again for the computer that now has the name).
pub fn has_ssh_name(name: &str, home: &Path, channel: Channel) -> bool {
    let Ok(host) = ssh_host(name, channel) else {
        return false;
    };
    fs::read_to_string(aliases_file(home, channel)).is_ok_and(|text| has_entry(&text, &host))
}

async fn linux_endpoint(engine: &Engine, computer: &Computer) -> Result<Endpoint, CliError> {
    let state = engine
        .call(requests::ssh_access_state(DeviceParams { device_id: None }))
        .await?;
    let row = state["computers"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["computerId"] == computer.id.as_str())
        })
        .cloned()
        .ok_or_else(|| {
            CliError::new(format!(
                "The engine has no SSH settings for {}.",
                computer.name
            ))
        })?;
    if row["enabled"] != true {
        engine
            .call(requests::ssh_access_save(SshAccessSaveParams {
                computer_id: computer.id.clone(),
                device_id: None,
                enabled: true,
                port: row["port"]
                    .as_u64()
                    .and_then(|port| u16::try_from(port).ok())
                    .unwrap_or(2222),
                bind_address: row["bindAddress"]
                    .as_str()
                    .unwrap_or("127.0.0.1")
                    .to_owned(),
                keys: None,
            }))
            .await?;
    }
    let command = engine
        .call(requests::ssh_access_connection(SshAccessConnectionParams {
            computer_id: computer.id.clone(),
            device_id: None,
            network: Some(false),
            download: false,
            destination: None,
        }))
        .await?;
    let command = command
        .as_str()
        .ok_or_else(|| CliError::new("The engine returned no SSH connection."))?;
    parse_connection(command)
}

/// `silo ssh-setup <computer>`: makes `ssh <computer>.silo` reach the computer. A Linux
/// computer gets SSH access turned on and an entry in `~/.silo/ssh-hosts.conf`; a macOS
/// computer's saved entry (which follows a promote) also answers to the name. `~/.ssh/config`
/// reads both.
pub async fn ssh_setup(
    engine: &Engine,
    name: &str,
    home: &Path,
    channel: Channel,
) -> Result<Value, CliError> {
    let host = ssh_host(name, channel)?;
    let computer = engine.computer(name).await?;
    let config = if computer.kind == "macos" {
        let ensured = engine
            .call(requests::guest_ssh_alias_ensure(
                GuestSshAliasEnsureParams {
                    computer_id: computer.id.clone(),
                    device_id: None,
                    install_include: None,
                    path: None,
                },
            ))
            .await?;
        let path = PathBuf::from(
            ensured["configPath"]
                .as_str()
                .ok_or_else(|| CliError::new("The engine returned no SSH entry."))?,
        );
        add_name_to_entry(&path, &host)?;
        release_name_elsewhere(&path, &host)?;
        path
    } else {
        let endpoint = linux_endpoint(engine, &computer).await?;
        let file = aliases_file(home, channel);
        let old = fs::read_to_string(&file).unwrap_or_default();
        write_private(
            &file,
            with_entry(&old, &host, &stanza(&host, &endpoint)?).as_bytes(),
        )?;
        install_include(home, &file)?;
        file
    };
    Ok(json!({
        "computer": computer.name,
        "host": host,
        "connect": format!("ssh {host}"),
        "config": config,
    }))
}

/// After a promote or an undo, writes the entry again for the computer that now has `name`
/// when one was written for it before. A macOS entry follows on its own.
pub async fn refresh_ssh_name(
    engine: &Engine,
    name: &str,
    home: &Path,
    channel: Channel,
) -> Option<Value> {
    if !has_ssh_name(name, home, channel) {
        return None;
    }
    Some(match ssh_setup(engine, name, home, channel).await {
        Ok(value) => value,
        Err(error) => json!({ "error": error.message }),
    })
}

/// `silo up`: makes `ssh main.silo` work. A failure is reported in the result, not raised;
/// `null` for a computer on another device.
pub async fn ssh_for_up(engine: &Engine, home: &Path, channel: Channel) -> Value {
    if engine.device().is_some() {
        return Value::Null;
    }
    match ssh_setup(engine, crate::ops::MAIN, home, channel).await {
        Ok(ssh) => ssh,
        Err(error) => json!({ "error": error.message }),
    }
}

/// After a promote or an undo of one: the name `main.silo` is written again for the
/// computer that now has the name, when it was written before.
pub async fn ssh_after_change(engine: &Engine, value: &mut Value, home: &Path, channel: Channel) {
    if engine.device().is_some() {
        return;
    }
    let name = match value["status"].as_str() {
        Some("promoted") => value["report"]["target"].as_str(),
        Some("undone") if value["kind"] == "promote" => value["computer"].as_str(),
        _ => None,
    }
    .map(str::to_owned);
    if let Some(name) = name {
        if let Some(ssh) = refresh_ssh_name(engine, &name, home, channel).await {
            value["ssh"] = ssh;
        }
    }
}

/// The user's home directory.
pub fn home_dir() -> Result<PathBuf, CliError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| CliError::new("HOME is not set."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_carries_the_channel_suffix() {
        assert_eq!(ssh_host("main", Channel::Production).unwrap(), "main.silo");
        assert_eq!(
            ssh_host("main", Channel::Development).unwrap(),
            "main.silo-dev"
        );
        assert!(ssh_host("a b", Channel::Production).is_err());
    }

    #[test]
    fn the_engine_s_connection_command_is_read() {
        let endpoint = parse_connection(
            "ssh -i '/Users/me/.silo-dev/ssh/connections/abc' -o IdentitiesOnly=yes -p 2223 silo@127.0.0.1",
        )
        .unwrap();
        assert_eq!(
            endpoint,
            Endpoint {
                key: "/Users/me/.silo-dev/ssh/connections/abc".into(),
                port: 2223,
                user: "silo".into(),
                address: "127.0.0.1".into(),
            }
        );
        assert!(parse_connection("ssh nothing").is_err());
    }

    #[test]
    fn an_entry_is_replaced_not_repeated() {
        let endpoint = |port| Endpoint {
            key: "/k".into(),
            port,
            user: "silo".into(),
            address: "127.0.0.1".into(),
        };
        let first = with_entry(
            "",
            "main.silo",
            &stanza("main.silo", &endpoint(2222)).unwrap(),
        );
        let other = with_entry(
            &first,
            "x.silo",
            &stanza("x.silo", &endpoint(2300)).unwrap(),
        );
        let again = with_entry(
            &other,
            "main.silo",
            &stanza("main.silo", &endpoint(2224)).unwrap(),
        );
        assert_eq!(again.matches("Host main.silo").count(), 1);
        assert!(again.contains("Port 2224"));
        assert!(!again.contains("Port 2222"));
        assert!(again.contains("Host x.silo") && again.contains("Port 2300"));
        assert!(has_entry(&again, "x.silo") && !has_entry(&again, "y.silo"));
    }

    #[test]
    fn the_include_is_added_once_at_the_top() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".ssh")).unwrap();
        fs::write(home.path().join(".ssh/config"), "Host old\n  User me\n").unwrap();
        let file = home.path().join(".silo/ssh-hosts.conf");
        install_include(home.path(), &file).unwrap();
        install_include(home.path(), &file).unwrap();
        let text = fs::read_to_string(home.path().join(".ssh/config")).unwrap();
        assert!(text.starts_with("Include \""));
        assert_eq!(text.matches("Include").count(), 1);
        assert!(text.ends_with("Host old\n  User me\n"));
    }

    #[test]
    fn a_name_is_answered_by_one_macos_entry() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.conf"), dir.path().join("b.conf"));
        fs::write(&a, "Host silo-a main.silo\n  User silo\n").unwrap();
        fs::write(&b, "Host silo-b\n  User silo\n").unwrap();
        add_name_to_entry(&b, "main.silo").unwrap();
        add_name_to_entry(&b, "main.silo").unwrap();
        release_name_elsewhere(&b, "main.silo").unwrap();
        assert_eq!(
            fs::read_to_string(&a).unwrap(),
            "Host silo-a\n  User silo\n"
        );
        assert_eq!(
            fs::read_to_string(&b).unwrap(),
            "Host silo-b main.silo\n  User silo\n"
        );
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;

    #[test]
    fn a_change_adds_removes_and_sets_the_mode() {
        let current = NetworkPolicy {
            mode: NetworkPolicyMode::Open,
            hosts: vec!["a.example.com".into()],
            presets: vec!["npm".into()],
        };
        let change = PolicyChange {
            mode: Some(NetworkPolicyMode::Allowlist),
            allow: vec!["B.Example.com".into(), "a.example.com".into()],
            remove: vec!["a.example.com".into(), "npm".into()],
            presets: vec!["pypi".into()],
        };
        let changed = change.apply(current);
        assert_eq!(changed.mode, NetworkPolicyMode::Allowlist);
        assert_eq!(changed.hosts, ["b.example.com"]);
        assert_eq!(changed.presets, ["pypi"]);
        assert!(PolicyChange::default().is_empty());
    }
}
