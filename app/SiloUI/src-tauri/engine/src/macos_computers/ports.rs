//! macOS guest listener discovery and loopback SSH publishing.
use super::{guest_access, MacosContext};
use crate::{applications::launch, editor, network};
use serde::{Deserialize, Serialize};
use silo_api::methods::network::{
    NetworkComputer, NetworkScheme, NetworkState, PublishedPort, PublishedPortState,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddrV4, TcpListener},
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

const LSOF_COMMAND: &str = "/usr/sbin/lsof -nP -iTCP -sTCP:LISTEN -FpcfPTn";
const FAILED: &str = "Could not read network services. Try again.";
const MAX_LISTENERS: usize = 4096;
const MAX_MAPPINGS: usize = 4096;
const MAX_PORTS_PER_COMPUTER: usize = 128;
const MAX_OUTPUT: usize = 1024 * 1024;
const STARTUP_LIMIT: Duration = Duration::from_secs(4);
const RECONCILE_INTERVAL: Duration = Duration::from_secs(10);
const HIDDEN_PORTS: [u16; 2] = [22, 5900];
static CONFIGURATION_LOCK: Mutex<()> = Mutex::new(());

fn configuration_lock() -> std::sync::MutexGuard<'static, ()> {
    CONFIGURATION_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Mapping {
    computer_id: String,
    port: u16,
    host_port: Option<u16>,
    scheme: Option<String>,
    enabled: bool,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configuration {
    mappings: Vec<Mapping>,
}

type Listeners = BTreeMap<u16, IpAddr>;

#[derive(Default)]
struct Endpoint {
    protocol: Option<String>,
    name: Option<String>,
    listening: bool,
}

fn prefer_target(current: IpAddr, next: IpAddr) -> IpAddr {
    match (current, next) {
        (IpAddr::V6(_), IpAddr::V4(_)) => next,
        (IpAddr::V4(old), IpAddr::V4(new)) if old.is_loopback() && !new.is_loopback() => next,
        _ => current,
    }
}

fn endpoint_target(value: &str) -> Result<(u16, IpAddr), String> {
    let value = value.strip_suffix(" (LISTEN)").unwrap_or(value);
    let (host, raw_port) = if let Some(rest) = value.strip_prefix('[') {
        let end = rest.find(']').ok_or(FAILED)?;
        let host = &rest[..end];
        let port = rest
            .get(end + 1..)
            .and_then(|tail| tail.strip_prefix(':'))
            .ok_or(FAILED)?;
        (host, port)
    } else {
        value.rsplit_once(':').ok_or(FAILED)?
    };
    let port = raw_port.parse::<u16>().map_err(|_| FAILED)?;
    if port == 0 {
        return Err(FAILED.into());
    }
    let ip = match host {
        "*" => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        "" => return Err(FAILED.into()),
        _ => host.parse::<IpAddr>().map_err(|_| FAILED.to_string())?,
    };
    let target = match ip {
        IpAddr::V4(address) if address.is_unspecified() || address.is_loopback() => {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        }
        IpAddr::V6(address) if address.is_unspecified() || address.is_loopback() => {
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        }
        address => address,
    };
    Ok((port, target))
}

fn finish_endpoint(endpoint: &mut Endpoint, listeners: &mut Listeners) -> Result<(), String> {
    if endpoint.protocol.as_deref() == Some("TCP") && endpoint.listening {
        if let Some(name) = endpoint.name.take() {
            let (port, target) = endpoint_target(&name)?;
            if !HIDDEN_PORTS.contains(&port) {
                listeners
                    .entry(port)
                    .and_modify(|old| *old = prefer_target(*old, target))
                    .or_insert(target);
                if listeners.len() > MAX_LISTENERS {
                    return Err(FAILED.into());
                }
            }
        }
    }
    endpoint.protocol = None;
    endpoint.name = None;
    endpoint.listening = false;
    Ok(())
}

fn parse_lsof(output: &str) -> Result<Listeners, String> {
    if output.len() > MAX_OUTPUT {
        return Err(FAILED.into());
    }
    if output.is_empty() {
        return Ok(Listeners::new());
    }
    let mut listeners = Listeners::new();
    let mut endpoint = Endpoint::default();
    let mut process_seen = false;
    for line in output.lines() {
        let Some((field, value)) = line.split_at_checked(1) else {
            continue;
        };
        match field {
            "p" => {
                finish_endpoint(&mut endpoint, &mut listeners)?;
                process_seen = true;
            }
            "f" => finish_endpoint(&mut endpoint, &mut listeners)?,
            "P" => endpoint.protocol = Some(value.into()),
            "n" => endpoint.name = Some(value.into()),
            "T" if value == "ST=LISTEN" || value == "ST=LISTEN\r" => endpoint.listening = true,
            _ => {}
        }
    }
    finish_endpoint(&mut endpoint, &mut listeners)?;
    if !process_seen {
        return Err(FAILED.into());
    }
    Ok(listeners)
}

fn config_path(app_data: &Path) -> PathBuf {
    app_data.join("network/macos-ports.json")
}

fn read_configuration_unlocked(app_data: &Path) -> Result<Configuration, String> {
    let path = config_path(app_data);
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Configuration::default())
        }
        Err(_) => return Err("Could not read saved ports.".into()),
    };
    let mut bytes = Vec::new();
    use std::io::Read as _;
    file.take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read saved ports.")?;
    if bytes.len() > 128 * 1024 {
        return Err("Saved ports are invalid.".into());
    }
    let config: Configuration =
        serde_json::from_slice(&bytes).map_err(|_| "Saved ports are invalid.")?;
    validate_configuration(&config)?;
    Ok(config)
}

fn validate_configuration(config: &Configuration) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    if config.mappings.len() > MAX_MAPPINGS {
        return Err("Too many saved ports.".into());
    }
    for mapping in &config.mappings {
        validate_mapping(mapping)?;
        if !seen.insert((&mapping.computer_id, mapping.port)) {
            return Err("Saved ports contain duplicates.".into());
        }
    }
    Ok(())
}

fn read_configuration(app: &MacosContext) -> Result<Configuration, String> {
    let _lock = configuration_lock();
    read_configuration_unlocked(&app.paths().app_data)
}

#[cfg(test)]
pub(super) fn write_test_configuration(
    app_data: &Path,
    computer_id: &str,
    port: u16,
    enabled: bool,
) {
    let _lock = configuration_lock();
    let mut config = read_configuration_unlocked(app_data).unwrap();
    config
        .mappings
        .retain(|mapping| mapping.computer_id != computer_id || mapping.port != port);
    config.mappings.push(Mapping {
        computer_id: computer_id.into(),
        port,
        host_port: None,
        scheme: None,
        enabled,
    });
    write_configuration_unlocked(app_data, &config).unwrap();
}

/// Whether the saved state still publishes `port` of the computer. `None` for a stream that
/// carries no published-port label, or when the saved state cannot be read.
pub(super) fn published(app_data: &Path, computer_id: &str, port: Option<u16>) -> Option<bool> {
    let port = port?;
    let _lock = configuration_lock();
    let config = read_configuration_unlocked(app_data).ok()?;
    Some(config.mappings.iter().any(|mapping| {
        mapping.computer_id == computer_id && mapping.port == port && mapping.enabled
    }))
}

fn validate_mapping(mapping: &Mapping) -> Result<(), String> {
    uuid::Uuid::parse_str(&mapping.computer_id).map_err(|_| "Saved ports are invalid.")?;
    if mapping.port == 0
        || mapping.host_port == Some(0)
        || !matches!(mapping.scheme.as_deref(), None | Some("http" | "https"))
    {
        return Err("Enter a port from 1 to 65535 and a supported protocol.".into());
    }
    Ok(())
}

fn write_configuration_unlocked(app_data: &Path, config: &Configuration) -> Result<(), String> {
    if config.mappings.len() > MAX_MAPPINGS {
        return Err("Too many saved ports. Remove an unused port first.".into());
    }
    let path = config_path(app_data);
    let parent = path.parent().ok_or("Could not save ports.")?;
    editor::private_directory(parent).map_err(|_| "Could not save ports.")?;
    let bytes = serde_json::to_vec(config).map_err(|_| "Could not save ports.")?;
    if bytes.len() > 128 * 1024 {
        return Err("Too many saved ports. Remove an unused port first.".into());
    }
    editor::write_private(&path, &bytes).map_err(|_| "Could not save ports.".into())
}

fn listeners(
    layout: &super::store::Layout,
    record: &super::store::Record,
) -> Result<Listeners, String> {
    let output = guest_access::run_capped(
        layout,
        record,
        LSOF_COMMAND,
        None,
        Duration::from_secs(10),
        Some(MAX_OUTPUT),
    )
    .map_err(|_| FAILED.to_string())?;
    listeners_from_output(output.status, &output.stdout)
}

/// `lsof` exits 1 with no output when nothing it may see is listening: the unprivileged
/// guest account does not see root-owned services. Any other nonzero status is a failure.
fn listeners_from_output(status: i32, stdout: &str) -> Result<Listeners, String> {
    match status {
        0 => parse_lsof(stdout),
        1 if stdout.trim().is_empty() => Ok(Listeners::new()),
        _ => Err(FAILED.into()),
    }
}

#[derive(Clone, Copy)]
struct TunnelTarget {
    host_port: u16,
    guest_address: IpAddr,
}

struct OwnedTunnel {
    target: TunnelTarget,
    tunnel: crate::owned_tunnel::Tunnel,
}

type TunnelKey = (String, u16);

/// The tunnels this process owns, with a revision per computer. Anything that changes what
/// should be published (unpublish, publish, stop, removal) advances the revision under this
/// lock. A repair records the revision before it reads the saved state and installs its
/// tunnels only while the revision is unchanged, so it cannot resurrect a retired forward.
#[derive(Default)]
struct TunnelRegistry {
    owned: HashMap<TunnelKey, OwnedTunnel>,
    revisions: HashMap<String, u64>,
}

impl TunnelRegistry {
    fn revision(&self, computer_id: &str) -> u64 {
        self.revisions.get(computer_id).copied().unwrap_or(0)
    }

    fn advance(&mut self, computer_id: &str) {
        let revision = self.revisions.entry(computer_id.to_owned()).or_default();
        *revision = revision.wrapping_add(1);
    }
}

fn tunnels() -> std::sync::MutexGuard<'static, TunnelRegistry> {
    static TUNNELS: OnceLock<Mutex<TunnelRegistry>> = OnceLock::new();
    TUNNELS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn tunnel_errors() -> &'static Mutex<HashMap<TunnelKey, String>> {
    static ERRORS: OnceLock<Mutex<HashMap<TunnelKey, String>>> = OnceLock::new();
    ERRORS.get_or_init(Default::default)
}

fn reconcile_lock(computer_id: &str) -> std::sync::Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, std::sync::Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(computer_id.into())
        .or_default()
        .clone()
}

#[derive(Default)]
struct RepairState {
    in_flight: HashSet<String>,
    last_started: HashMap<String, Instant>,
}

fn repairs() -> &'static Mutex<RepairState> {
    static REPAIRS: OnceLock<Mutex<RepairState>> = OnceLock::new();
    REPAIRS.get_or_init(Default::default)
}

fn tunnel_error(key: &TunnelKey, message: Option<String>) {
    let mut errors = tunnel_errors()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(message) = message {
        errors.insert(key.clone(), message);
    } else {
        errors.remove(key);
    }
}

fn tunnel_is_live(key: &TunnelKey, target: TunnelTarget) -> bool {
    let mut registry = tunnels();
    let Some(current) = registry.owned.get_mut(key) else {
        return false;
    };
    let same = current.target.host_port == target.host_port
        && current.target.guest_address == target.guest_address;
    same && current.tunnel.running()
}

fn live_target(key: &TunnelKey) -> Option<TunnelTarget> {
    let mut registry = tunnels();
    let current = registry.owned.get_mut(key)?;
    current.tunnel.running().then_some(current.target)
}

fn target_for_ssh(address: IpAddr) -> String {
    match address {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{address}]"),
    }
}

fn tunnel_command(
    config: &Path,
    alias: &str,
    proxy: &str,
    key: &TunnelKey,
    target: TunnelTarget,
) -> Result<Command, String> {
    let mut command = Command::new("/usr/bin/ssh");
    launch::sanitize_child(&mut command);
    command
        .args(["-F"])
        .arg(config)
        .args([
            "-N",
            "-o",
            "BatchMode=yes",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ClearAllForwardings=no",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "ServerAliveInterval=15",
        ])
        // The stream this tunnel rides on carries the forward's port as its label, so
        // unpublishing the port can end it. The command line overrides the alias's own
        // proxy command.
        .arg("-o")
        .arg(format!("ProxyCommand={proxy}"))
        .arg("-L")
        .arg(format!(
            "127.0.0.1:{}:{}:{}",
            target.host_port,
            target_for_ssh(target.guest_address),
            key.1
        ))
        .arg(alias);
    Ok(command)
}

fn spawn_tunnel(
    config: &Path,
    alias: &str,
    proxy: &str,
    key: &TunnelKey,
    target: TunnelTarget,
) -> Result<OwnedTunnel, String> {
    let command = tunnel_command(config, alias, proxy, key, target)?;
    let mut tunnel = crate::owned_tunnel::Tunnel::spawn(&command, None)
        .map_err(|_| "Could not publish this port.".to_string())?;
    let deadline = Instant::now() + STARTUP_LIMIT;
    loop {
        if !tunnel.running() {
            return Err("Could not publish this port.".into());
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(OwnedTunnel { target, tunnel })
}

fn host_port_available(port: u16) -> bool {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).is_ok()
}

fn reserve_host_port() -> Result<u16, String> {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|_| "Could not choose a local port.".into())
}

/// Retires one forward: advances the computer's revision, then closes its tunnel.
fn close_tunnel(key: &TunnelKey) {
    let retired = {
        let mut registry = tunnels();
        registry.advance(&key.0);
        registry.owned.remove(key)
    };
    drop(retired);
    tunnel_error(key, None);
}

/// Retires every forward of a computer, as Stop and removal do.
fn close_computer(computer_id: &str) {
    let retired: Vec<_> = {
        let mut registry = tunnels();
        registry.advance(computer_id);
        let keys: Vec<_> = registry
            .owned
            .keys()
            .filter(|(id, _)| id == computer_id)
            .cloned()
            .collect();
        keys.iter()
            .filter_map(|key| registry.owned.remove(key))
            .collect()
    };
    drop(retired);
    tunnel_errors()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|(id, _), _| id != computer_id);
}

/// Installs a tunnel a user action just prepared. The action also advances the revision, so
/// a repair that started earlier cannot act on what it read before.
fn install_tunnel(key: TunnelKey, owned: OwnedTunnel) {
    let replaced = {
        let mut registry = tunnels();
        registry.advance(&key.0);
        registry.owned.insert(key.clone(), owned)
    };
    drop(replaced);
    tunnel_error(&key, None);
}

/// Installs a repair's tunnel only if nothing retired or changed the computer's forwards since
/// the repair recorded `revision` and the guest is still in the generation it repaired.
/// The check and the insertion happen under one lock, so a concurrent retirement either runs
/// first (and the tunnel is refused) or later (and removes the tunnel).
fn install_repaired_tunnel(
    key: TunnelKey,
    owned: OwnedTunnel,
    revision: u64,
    still_running: impl FnOnce() -> bool,
) -> bool {
    let (installed, replaced) = {
        let mut registry = tunnels();
        if registry.revision(&key.0) != revision || !still_running() {
            (false, None)
        } else {
            (true, registry.owned.insert(key.clone(), owned))
        }
    };
    if installed {
        drop(replaced);
        tunnel_error(&key, None);
    }
    installed
}

/// Closes one tunnel the guest no longer serves, unless the computer's forwards changed since
/// `revision` (then the observation is stale).
fn drop_stale(key: &TunnelKey, revision: u64) {
    let stale = {
        let mut registry = tunnels();
        if registry.revision(&key.0) != revision {
            return;
        }
        registry.owned.remove(key)
    };
    drop(stale);
    tunnel_error(key, None);
}

/// Closes this computer's tunnels that are not in the saved enabled set, unless something
/// already changed the computer's forwards since `revision` (then the saved set is stale).
fn retire_absent(computer_id: &str, enabled: &HashSet<u16>, revision: u64) {
    let retired: Vec<_> = {
        let mut registry = tunnels();
        if registry.revision(computer_id) != revision {
            return;
        }
        let keys: Vec<_> = registry
            .owned
            .keys()
            .filter(|(id, port)| id == computer_id && !enabled.contains(port))
            .cloned()
            .collect();
        keys.iter()
            .filter_map(|key| {
                registry
                    .owned
                    .remove(key)
                    .map(|tunnel| (key.clone(), tunnel))
            })
            .collect()
    };
    for (key, _tunnel) in retired {
        tunnel_error(&key, None);
    }
}

fn start_tunnel_for(
    app: &MacosContext,
    computer_id: &str,
    key: &TunnelKey,
    target: TunnelTarget,
) -> Result<OwnedTunnel, String> {
    AppRepair(app).spawn(computer_id, key, target)
}

/// What a repair pass needs from the running application. Tests replace it to hold a pass
/// at chosen points while another operation runs.
trait RepairHost {
    /// The computer's generation when it is Running, `None` otherwise.
    fn running(&self, computer_id: &str) -> Option<u64>;
    fn discover(&self, computer_id: &str) -> Result<Listeners, String>;
    fn saved(&self, computer_id: &str) -> Result<Vec<Mapping>, String>;
    /// Prepares the SSH client configuration and starts one forward's tunnel.
    fn spawn(
        &self,
        computer_id: &str,
        key: &TunnelKey,
        target: TunnelTarget,
    ) -> Result<OwnedTunnel, String>;
}

struct AppRepair<'a>(&'a MacosContext);

impl RepairHost for AppRepair<'_> {
    fn running(&self, computer_id: &str) -> Option<u64> {
        super::ensure_loaded(self.0).ok()?;
        let (_, state) = super::computer(computer_id).ok()?;
        (state == super::store::State::Running)
            .then(|| super::computer_generation(computer_id).ok())
            .flatten()
    }

    fn discover(&self, computer_id: &str) -> Result<Listeners, String> {
        let (layout, record) = super::layout_and_record(self.0, computer_id)?;
        listeners(&layout, &record)
    }

    fn saved(&self, computer_id: &str) -> Result<Vec<Mapping>, String> {
        Ok(read_configuration(self.0)?
            .mappings
            .into_iter()
            .filter(|mapping| mapping.computer_id == computer_id && mapping.enabled)
            .collect())
    }

    fn spawn(
        &self,
        computer_id: &str,
        key: &TunnelKey,
        target: TunnelTarget,
    ) -> Result<OwnedTunnel, String> {
        let connection = super::ssh_alias::ensure(self.0, computer_id, None, Some(false))?;
        let proxy = super::ssh_alias::proxy_command(computer_id, Some(key.1))?;
        spawn_tunnel(
            Path::new(&connection.config_path),
            &connection.alias,
            &proxy,
            key,
            target,
        )
    }
}

fn reconcile_computer(app: &MacosContext, computer_id: &str) -> Result<(), String> {
    reconcile_with(&AppRepair(app), computer_id)
}

fn reconcile_with(host: &dyn RepairHost, computer_id: &str) -> Result<(), String> {
    let lock = reconcile_lock(computer_id);
    let _serial = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Record the revision before reading anything else: a retirement after this point makes
    // every install below refuse, and one before it is already visible in the saved state.
    let revision = tunnels().revision(computer_id);
    let Some(generation) = host.running(computer_id) else {
        close_computer(computer_id);
        return Ok(());
    };
    let saved = host.saved(computer_id)?;
    let discovered = match host.discover(computer_id) {
        Ok(listeners) => listeners,
        Err(error) => {
            for mapping in &saved {
                tunnel_error(&(computer_id.to_owned(), mapping.port), Some(error.clone()));
            }
            return Err(error);
        }
    };
    retire_absent(
        computer_id,
        &saved.iter().map(|mapping| mapping.port).collect(),
        revision,
    );
    for mapping in saved {
        let key = (computer_id.to_string(), mapping.port);
        let Some(address) = discovered.get(&mapping.port).copied() else {
            // Not listening: the forward waits, and a stale tunnel for it is closed.
            drop_stale(&key, revision);
            continue;
        };
        let existing = live_target(&key);
        let host_port = mapping
            .host_port
            .or_else(|| {
                existing
                    .filter(|current| current.guest_address == address)
                    .map(|current| current.host_port)
            })
            .map(Ok)
            .unwrap_or_else(reserve_host_port)?;
        let target = TunnelTarget {
            host_port,
            guest_address: address,
        };
        if tunnel_is_live(&key, target) {
            tunnel_error(&key, None);
            continue;
        }
        if mapping.host_port.is_some()
            && existing.is_none_or(|current| current.host_port != host_port)
            && !host_port_available(host_port)
        {
            tunnel_error(&key, Some("This local port is already in use.".into()));
            continue;
        }
        match host.spawn(computer_id, &key, target) {
            Ok(tunnel) => {
                // The spawn took time; refuse the tunnel if the forward was retired meanwhile.
                install_repaired_tunnel(key, tunnel, revision, || {
                    host.running(computer_id) == Some(generation)
                });
            }
            Err(error) => tunnel_error(&key, Some(error)),
        }
    }
    Ok(())
}

pub(super) fn computer_started(app: &MacosContext, computer_id: &str) {
    schedule_repair(app, computer_id, true);
}

fn schedule_repair(app: &MacosContext, computer_id: &str, force: bool) {
    {
        let mut state = repairs()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.in_flight.contains(computer_id)
            || (!force
                && state
                    .last_started
                    .get(computer_id)
                    .is_some_and(|last| last.elapsed() < RECONCILE_INTERVAL))
        {
            return;
        }
        state.in_flight.insert(computer_id.into());
        state
            .last_started
            .insert(computer_id.into(), Instant::now());
    }
    let app = app.clone();
    let id = computer_id.to_string();
    let scheduled = thread::Builder::new()
        .name("macos-port-repair".into())
        .spawn(move || {
            let _ = reconcile_computer(&app, &id);
            repairs()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .in_flight
                .remove(&id);
        });
    if scheduled.is_err() {
        repairs()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .in_flight
            .remove(computer_id);
    }
}

fn schedule_repairs(app: &MacosContext, records: &[super::store::Record]) {
    for record in records {
        if super::computer(&record.id).is_ok_and(|(_, state)| state == super::store::State::Running)
        {
            schedule_repair(app, &record.id, false);
        }
    }
}

/// Schedules a repair for every running computer without reading the network state.
pub(super) fn repair_running(app: &MacosContext) -> Result<(), String> {
    super::ensure_loaded(app)?;
    let records = super::store::load_all(&app.paths().app_data);
    schedule_repairs(app, &records);
    Ok(())
}

pub(super) fn state(app: &MacosContext) -> Result<NetworkState, String> {
    super::ensure_loaded(app)?;
    let records = super::store::load_all(&app.paths().app_data);
    schedule_repairs(app, &records);
    let config = read_configuration(app)?;
    let computers = records
        .into_iter()
        .map(|record| {
            let state = super::computer(&record.id)
                .map(|(_, state)| state)
                .unwrap_or(super::store::State::Failed);
            let (listeners, error) = if state == super::store::State::Running {
                match super::layout_and_record(app, &record.id)
                    .and_then(|(layout, record)| listeners(&layout, &record))
                {
                    Ok(listeners) => (listeners, None),
                    Err(error) => (Listeners::new(), Some(error)),
                }
            } else {
                (Listeners::new(), None)
            };
            let mappings: BTreeMap<u16, Mapping> = config
                .mappings
                .iter()
                .filter(|mapping| mapping.computer_id == record.id)
                .map(|mapping| (mapping.port, mapping.clone()))
                .collect();
            let ports = listeners
                .keys()
                .chain(mappings.keys())
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|port| {
                    let mapping = mappings.get(&port);
                    let configured = mapping.is_some_and(|mapping| mapping.enabled);
                    let key = (record.id.clone(), port);
                    let target = listeners.get(&port).copied().and_then(|guest_address| {
                        live_target(&key).map(|owned| (guest_address, owned))
                    });
                    let is_reachable = configured
                        && target.is_some_and(|(address, owned)| {
                            owned.guest_address == address && live_target(&key).is_some()
                        });
                    let port_error = tunnel_errors()
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .get(&key)
                        .cloned();
                    let port_state = if error.is_some() {
                        PublishedPortState::Unknown
                    } else if !configured {
                        PublishedPortState::Unpublished
                    } else if is_reachable {
                        PublishedPortState::Reachable
                    } else {
                        PublishedPortState::Waiting
                    };
                    PublishedPort {
                        port,
                        host_port: is_reachable
                            .then(|| live_target(&key).map(|target| target.host_port))
                            .flatten(),
                        configured_host_port: mapping.and_then(|mapping| mapping.host_port),
                        scheme: mapping.and_then(|mapping| match mapping.scheme.as_deref() {
                            Some("http") => Some(NetworkScheme::Http),
                            Some("https") => Some(NetworkScheme::Https),
                            _ => None,
                        }),
                        state: port_state,
                        configured,
                        message: port_error.or_else(|| error.clone()).or_else(|| {
                            (configured && !is_reachable && error.is_none())
                                .then(|| "This service is waiting for a listener.".into())
                        }),
                    }
                })
                .collect();
            let host = network::computer_host(&record.name, &record.id);
            NetworkComputer {
                computer: record.name,
                computer_id: Some(record.id),
                host: Some(host),
                error,
                ports,
            }
        })
        .collect();
    Ok(NetworkState { computers })
}

pub(super) fn publish(
    app: &MacosContext,
    computer_id: &str,
    port: u16,
    host_port: Option<u16>,
    scheme: Option<String>,
) -> Result<NetworkState, String> {
    if port == 0
        || host_port == Some(0)
        || !matches!(scheme.as_deref(), None | Some("http" | "https"))
    {
        return Err("Enter a port from 1 to 65535 and a supported protocol.".into());
    }
    crate::runtime::shutdown::ensure_accepting_operations()?;
    let _update = crate::updates::operation_guard()?;
    super::with_macos_computer_operation(
        app,
        computer_id,
        crate::runtime::operation_gate::OperationKind::PortPublish,
        "Publishing a port",
        || {
            let (_, state) = super::computer(computer_id)?;
            let mut config = read_configuration(app)?;
            let active_for_computer = config
                .mappings
                .iter()
                .filter(|mapping| {
                    mapping.computer_id == computer_id && mapping.enabled && mapping.port != port
                })
                .count();
            if active_for_computer >= MAX_PORTS_PER_COMPUTER {
                return Err("This computer already has 128 published ports.".into());
            }
            if config.mappings.iter().any(|mapping| {
                mapping.computer_id == computer_id
                    && mapping.port != port
                    && mapping.enabled
                    && host_port.is_some()
                    && mapping.host_port == host_port
            }) {
                return Err(
                    "This local port is already in use. Choose another or use Automatic.".into(),
                );
            }
            let (layout, record) = super::layout_and_record(app, computer_id)?;
            let discovered = if state == super::store::State::Running {
                listeners(&layout, &record)?
            } else {
                Listeners::new()
            };
            let target_address = discovered.get(&port).copied();
            let existing_target = live_target(&(computer_id.to_string(), port));
            let selected_host_port = match (host_port, target_address, existing_target) {
                (None, Some(address), Some(existing)) if existing.guest_address == address => {
                    existing.host_port
                }
                (Some(requested), _, Some(existing)) if existing.host_port == requested => {
                    requested
                }
                (Some(requested), _, _) => {
                    if !host_port_available(requested) {
                        return Err(
                            "This local port is already in use. Choose another or use Automatic."
                                .into(),
                        );
                    }
                    requested
                }
                (None, _, _) => reserve_host_port()?,
            };
            let tunnel_target = target_address.map(|guest_address| TunnelTarget {
                host_port: selected_host_port,
                guest_address,
            });
            let key = (computer_id.to_string(), port);
            let staged_tunnel = if state == super::store::State::Running {
                if let Some(target) = tunnel_target {
                    if tunnel_is_live(&key, target) {
                        None
                    } else {
                        Some(start_tunnel_for(app, computer_id, &key, target)?)
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let mapping = Mapping {
                computer_id: computer_id.into(),
                port,
                host_port,
                scheme,
                enabled: true,
            };
            validate_mapping(&mapping)?;
            {
                let _lock = configuration_lock();
                config = read_configuration_unlocked(&app.paths().app_data)?;
                if config.mappings.iter().any(|saved| {
                    saved.computer_id != computer_id
                        && saved.enabled
                        && host_port.is_some()
                        && saved.host_port == host_port
                }) {
                    return Err(
                        "This local port is already in use. Choose another or use Automatic."
                            .into(),
                    );
                }
                config
                    .mappings
                    .retain(|saved| saved.computer_id != computer_id || saved.port != port);
                config.mappings.push(mapping);
                write_configuration_unlocked(&app.paths().app_data, &config)?;
            }
            if let Some(tunnel) = staged_tunnel {
                install_tunnel(key.clone(), tunnel);
            } else if tunnel_target.is_none() {
                close_tunnel(&key);
            }
            let _ = selected_host_port;
            Ok(())
        },
    )?;
    state(app)
}

pub(super) fn unpublish(
    app: &MacosContext,
    computer_id: &str,
    port: u16,
) -> Result<NetworkState, String> {
    if port == 0 {
        return Err("Enter a port from 1 to 65535.".into());
    }
    crate::runtime::shutdown::ensure_accepting_operations()?;
    let _update = crate::updates::operation_guard()?;
    super::with_macos_computer_operation(
        app,
        computer_id,
        crate::runtime::operation_gate::OperationKind::PortRemove,
        "Removing a published port",
        || disable_mapping(&app.paths().app_data, computer_id, port),
    )?;
    state(app)
}

/// Turns the saved forward off, then retires its tunnel and revokes its open streams. A repair
/// that read the forward as enabled before this runs cannot install its tunnel afterwards.
fn disable_mapping(app_data: &Path, computer_id: &str, port: u16) -> Result<(), String> {
    {
        let _lock = configuration_lock();
        let mut config = read_configuration_unlocked(app_data)?;
        if let Some(mapping) = config
            .mappings
            .iter_mut()
            .find(|mapping| mapping.computer_id == computer_id && mapping.port == port)
        {
            mapping.enabled = false;
        }
        write_configuration_unlocked(app_data, &config)?;
    }
    close_tunnel(&(computer_id.to_string(), port));
    crate::remote::revoke_port_streams(computer_id, port);
    Ok(())
}

pub(super) fn open_url(app: &MacosContext, computer_id: &str, port: u16) -> Result<String, String> {
    let state = state(app)?;
    let computer = state
        .computers
        .iter()
        .find(|computer| computer.computer_id.as_deref() == Some(computer_id))
        .ok_or("This computer no longer exists.")?;
    let published = computer
        .ports
        .iter()
        .find(|published| {
            published.port == port
                && published.configured
                && published.state == PublishedPortState::Reachable
        })
        .ok_or("This service is not reachable.")?;
    let scheme = published
        .scheme
        .ok_or("This TCP service is not configured as a website.")?;
    let host_port = published
        .host_port
        .ok_or("This service is not reachable.")?;
    Ok(network::website_url(
        match scheme {
            NetworkScheme::Http => "http",
            NetworkScheme::Https => "https",
        },
        computer.host.as_deref(),
        host_port,
    ))
}

pub(super) fn computer_removed(app: &MacosContext, computer_id: &str) -> Result<(), String> {
    close_computer(computer_id);
    let _lock = configuration_lock();
    let mut config = read_configuration_unlocked(&app.paths().app_data)?;
    let before = config.mappings.len();
    config
        .mappings
        .retain(|mapping| mapping.computer_id != computer_id);
    if config.mappings.len() != before {
        write_configuration_unlocked(&app.paths().app_data, &config)?;
    }
    Ok(())
}

pub(super) fn computer_stopped(computer_id: &str) {
    close_computer(computer_id);
    repairs()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .last_started
        .remove(computer_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsof_machine_fields_deduplicate_addresses_and_hide_engine_endpoints() {
        let output = concat!(
            "p100\ncnode\nf10\nPTCP\nTST=LISTEN\nn*:3000\n",
            "p101\ncpython\nf11\nPTCP\nTST=LISTEN\nn127.0.0.1:3000\n",
            "p102\ncsshd\nf12\nPTCP\nTST=LISTEN\nn*:22\n",
            "p103\ncvnc\nf13\nPTCP\nTST=LISTEN\nn*:5900\n",
            "p104\ncv6\nf14\nPTCP\nTST=LISTEN\nn[::1]:4000\n",
        );
        let listeners = parse_lsof(output).unwrap();
        assert_eq!(listeners.len(), 2);
        assert_eq!(listeners[&3000], IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(listeners[&4000], IpAddr::V6(Ipv6Addr::LOCALHOST));
    }

    #[test]
    fn lsof_parser_rejects_malformed_listeners_and_bounds_output() {
        assert!(parse_lsof("not-lsof\n").is_err());
        assert!(parse_lsof("p100\nf10\nPTCP\nTST=LISTEN\nn*:0\n").is_err());
        assert!(parse_lsof(&"x".repeat(MAX_OUTPUT + 1)).is_err());
        assert!(parse_lsof("").unwrap().is_empty());
    }

    #[test]
    fn loopback_tunnel_target_quotes_ipv6_for_ssh_forward_syntax() {
        assert_eq!(target_for_ssh(IpAddr::V6(Ipv6Addr::LOCALHOST)), "[::1]");
        assert_eq!(target_for_ssh(IpAddr::V4(Ipv4Addr::LOCALHOST)), "127.0.0.1");
    }

    #[test]
    fn saved_mac_port_intent_is_keyed_by_computer_id_and_rejects_duplicates() {
        let id = "c690f54f-4cc9-4950-8782-30f21e89d18b";
        let config = Configuration {
            mappings: vec![Mapping {
                computer_id: id.into(),
                port: 3000,
                host_port: None,
                scheme: Some("http".into()),
                enabled: true,
            }],
        };
        validate_configuration(&config).unwrap();
        let encoded = serde_json::to_value(&config).unwrap();
        assert_eq!(encoded["mappings"][0]["computerId"], id);
        assert!(encoded["mappings"][0].get("computer").is_none());

        let duplicate = Configuration {
            mappings: vec![config.mappings[0].clone(), config.mappings[0].clone()],
        };
        assert!(validate_configuration(&duplicate).is_err());
    }

    #[test]
    fn ssh_publisher_overrides_alias_forwarding_suppression_for_loopback_only() {
        let alias = "silo-c690f54f-4cc9-4950-8782-30f21e89d18b";
        let config = tempfile::tempdir().unwrap();
        let config_path = config.path().join("macos-guest.conf");
        std::fs::write(
            &config_path,
            format!("Host {alias}\n  ClearAllForwardings yes\n"),
        )
        .unwrap();
        let command = tunnel_command(
            &config_path,
            alias,
            "'/Applications/Silo Dev.app/Contents/MacOS/silod' --remote-guest 'a' 'c690f54f-4cc9-4950-8782-30f21e89d18b' '8080'",
            &("c690f54f-4cc9-4950-8782-30f21e89d18b".into(), 8080),
            TunnelTarget {
                host_port: 49152,
                guest_address: IpAddr::V6(Ipv6Addr::LOCALHOST),
            },
        )
        .unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[0], "-F");
        assert_eq!(args[1], config_path.to_str().unwrap());
        assert!(args.contains(&"-N".into()));
        assert!(args.contains(&"ExitOnForwardFailure=yes".into()));
        assert!(args.contains(&"ClearAllForwardings=no".into()));
        assert!(args.contains(&"127.0.0.1:49152:[::1]:8080".into()));
        assert!(args.contains(&alias.into()));

        let resolved = Command::new(command.get_program())
            .arg("-G")
            .args(command.get_args())
            .output()
            .expect("run OpenSSH config resolution without connecting");
        assert!(
            resolved.status.success(),
            "OpenSSH config resolution failed: {}",
            String::from_utf8_lossy(&resolved.stderr)
        );
        let resolved = String::from_utf8_lossy(&resolved.stdout);
        assert!(resolved
            .lines()
            .any(|line| line == "clearallforwardings no"));
        assert!(
            resolved
                .lines()
                .any(|line| line == "localforward [127.0.0.1]:49152 [::1]:8080"),
            "OpenSSH dropped the requested loopback forward:\n{resolved}"
        );
    }

    #[test]
    fn lsof_status_one_with_no_output_means_no_visible_listeners() {
        assert!(listeners_from_output(1, "").unwrap().is_empty());
        assert!(listeners_from_output(1, "\n").unwrap().is_empty());
        assert!(listeners_from_output(0, "").unwrap().is_empty());
        assert_eq!(
            listeners_from_output(0, "p100\nf10\nPTCP\nTST=LISTEN\nn*:3000\n")
                .unwrap()
                .len(),
            1
        );
        // Genuine failures stay failures: output with status 1, other statuses, SSH loss.
        assert!(listeners_from_output(1, "p100\nf10\nPTCP\nTST=LISTEN\nn*:3000\n").is_err());
        assert!(listeners_from_output(2, "").is_err());
        assert!(listeners_from_output(255, "").is_err());
        assert!(listeners_from_output(-1, "").is_err());
    }

    /// A repair pass that stops inside `spawn` until the test releases it.
    struct HeldRepair {
        app_data: PathBuf,
        generation: std::sync::atomic::AtomicU64,
        running: std::sync::atomic::AtomicBool,
        listeners: Listeners,
        entered: Mutex<std::sync::mpsc::Sender<()>>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl RepairHost for HeldRepair {
        fn running(&self, _computer_id: &str) -> Option<u64> {
            use std::sync::atomic::Ordering;
            self.running
                .load(Ordering::SeqCst)
                .then(|| self.generation.load(Ordering::SeqCst))
        }

        fn discover(&self, _computer_id: &str) -> Result<Listeners, String> {
            Ok(self.listeners.clone())
        }

        fn saved(&self, computer_id: &str) -> Result<Vec<Mapping>, String> {
            let _lock = configuration_lock();
            Ok(read_configuration_unlocked(&self.app_data)?
                .mappings
                .into_iter()
                .filter(|mapping| mapping.computer_id == computer_id && mapping.enabled)
                .collect())
        }

        fn spawn(
            &self,
            _computer_id: &str,
            _key: &TunnelKey,
            target: TunnelTarget,
        ) -> Result<OwnedTunnel, String> {
            self.entered.lock().unwrap().send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
            sleeping_tunnel(target)
        }
    }

    fn sleeping_tunnel(target: TunnelTarget) -> Result<OwnedTunnel, String> {
        let mut command = Command::new("/bin/sleep");
        command.arg("120");
        let tunnel = crate::owned_tunnel::Tunnel::spawn(&command, None)
            .map_err(|error| error.to_string())?;
        Ok(OwnedTunnel { target, tunnel })
    }

    struct RepairFixture {
        _directory: tempfile::TempDir,
        host: HeldRepair,
        entered: std::sync::mpsc::Receiver<()>,
        release: std::sync::mpsc::Sender<()>,
        id: String,
        key: TunnelKey,
    }

    impl Drop for RepairFixture {
        fn drop(&mut self) {
            close_computer(&self.id);
        }
    }

    fn repair_fixture(enabled: bool) -> RepairFixture {
        let directory = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        write_configuration_unlocked(
            directory.path(),
            &Configuration {
                mappings: vec![Mapping {
                    computer_id: id.clone(),
                    port: 3000,
                    host_port: None,
                    scheme: None,
                    enabled,
                }],
            },
        )
        .unwrap();
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let host = HeldRepair {
            app_data: directory.path().to_path_buf(),
            generation: std::sync::atomic::AtomicU64::new(1),
            running: std::sync::atomic::AtomicBool::new(true),
            listeners: Listeners::from([(3000, IpAddr::V4(Ipv4Addr::LOCALHOST))]),
            entered: Mutex::new(entered_tx),
            release: Mutex::new(release_rx),
        };
        RepairFixture {
            _directory: directory,
            host,
            entered,
            release,
            key: (id.clone(), 3000),
            id,
        }
    }

    fn tunnel_installed(key: &TunnelKey) -> bool {
        tunnels().owned.contains_key(key)
    }

    #[test]
    fn repair_installs_a_tunnel_for_an_enabled_listening_forward() {
        let fixture = repair_fixture(true);
        fixture.release.send(()).unwrap();
        reconcile_with(&fixture.host, &fixture.id).unwrap();
        fixture.entered.try_recv().unwrap();
        assert!(tunnel_installed(&fixture.key));
    }

    #[test]
    fn unpublish_during_a_repair_is_not_undone_by_the_repair() {
        let fixture = repair_fixture(true);
        std::thread::scope(|scope| {
            let repair = scope.spawn(|| reconcile_with(&fixture.host, &fixture.id));
            // The repair read the forward as enabled and is starting its tunnel.
            fixture.entered.recv().unwrap();
            disable_mapping(fixture.host.app_data.as_path(), &fixture.id, 3000).unwrap();
            fixture.release.send(()).unwrap();
            repair.join().unwrap().unwrap();
        });
        assert!(
            !tunnel_installed(&fixture.key),
            "the retired forward came back"
        );
        // A later repair still sees the forward disabled and leaves nothing running.
        reconcile_with(&fixture.host, &fixture.id).unwrap();
        assert!(!tunnel_installed(&fixture.key));
    }

    #[test]
    fn stop_during_a_repair_is_not_undone_by_the_repair() {
        let fixture = repair_fixture(true);
        std::thread::scope(|scope| {
            let repair = scope.spawn(|| reconcile_with(&fixture.host, &fixture.id));
            fixture.entered.recv().unwrap();
            // Stop closes the computer's forwards. The pass cannot tell from its own reads.
            close_computer(&fixture.id);
            fixture.release.send(()).unwrap();
            repair.join().unwrap().unwrap();
        });
        assert!(
            !tunnel_installed(&fixture.key),
            "the stopped computer's forward came back"
        );
    }

    #[test]
    fn repair_does_not_install_into_a_restarted_guest_generation() {
        use std::sync::atomic::Ordering;
        let fixture = repair_fixture(true);
        std::thread::scope(|scope| {
            let repair = scope.spawn(|| reconcile_with(&fixture.host, &fixture.id));
            fixture.entered.recv().unwrap();
            // The guest restarted without the repair learning of it any other way.
            fixture.host.generation.store(2, Ordering::SeqCst);
            fixture.release.send(()).unwrap();
            repair.join().unwrap().unwrap();
        });
        assert!(!tunnel_installed(&fixture.key));
    }

    #[test]
    fn repair_closes_tunnels_that_are_not_in_the_enabled_saved_state() {
        let fixture = repair_fixture(false);
        install_tunnel(
            fixture.key.clone(),
            sleeping_tunnel(TunnelTarget {
                host_port: 1,
                guest_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            })
            .unwrap(),
        );
        assert!(tunnel_installed(&fixture.key));
        reconcile_with(&fixture.host, &fixture.id).unwrap();
        assert!(!tunnel_installed(&fixture.key));
        assert!(
            fixture.entered.try_recv().is_err(),
            "nothing should be started"
        );
    }

    #[test]
    fn published_state_reads_the_saved_forward() {
        let fixture = repair_fixture(true);
        let data = fixture.host.app_data.as_path();
        assert_eq!(published(data, &fixture.id, None), None);
        assert_eq!(published(data, &fixture.id, Some(3000)), Some(true));
        assert_eq!(published(data, &fixture.id, Some(3001)), Some(false));
        disable_mapping(data, &fixture.id, 3000).unwrap();
        assert_eq!(published(data, &fixture.id, Some(3000)), Some(false));
    }

    #[test]
    fn tunnel_command_labels_its_ssh_stream_with_the_forwarded_port() {
        let alias = "silo-c690f54f-4cc9-4950-8782-30f21e89d18b";
        let config = tempfile::tempdir().unwrap();
        let config_path = config.path().join("macos-guest.conf");
        let proxy = "'/bin/silod' --remote-guest 'a' 'c690f54f-4cc9-4950-8782-30f21e89d18b' '3000'";
        std::fs::write(
            &config_path,
            format!(
                "Host {alias}\n  ProxyCommand '/bin/silod' --remote-guest 'a' 'c690f54f-4cc9-4950-8782-30f21e89d18b'\n"
            ),
        )
        .unwrap();
        let command = tunnel_command(
            &config_path,
            alias,
            proxy,
            &("c690f54f-4cc9-4950-8782-30f21e89d18b".into(), 3000),
            TunnelTarget {
                host_port: 49152,
                guest_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            },
        )
        .unwrap();
        let resolved = Command::new(command.get_program())
            .arg("-G")
            .args(command.get_args())
            .output()
            .expect("run OpenSSH config resolution without connecting");
        assert!(resolved.status.success());
        let resolved = String::from_utf8_lossy(&resolved.stdout);
        assert!(
            resolved
                .lines()
                .any(|line| line == format!("proxycommand {proxy}")),
            "the command-line proxy label did not override the alias:\n{resolved}"
        );
    }

    #[test]
    fn macos_websites_use_the_computers_localhost_name() {
        let id = "c690f54f-4cc9-4950-8782-30f21e89d18b";
        let host = network::computer_host("Build Box", id);
        assert_eq!(
            network::website_url("http", Some(&host), 49152),
            "http://build-box-c690f54f.localhost:49152"
        );
    }
}
