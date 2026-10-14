//! Local TCP services and loopback-only published ports.
use crate::runtime::{self, ProcessRunner, RuntimePaths};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    sync::Mutex,
    time::Duration,
};

static NETWORK_LOCK: Mutex<()> = Mutex::new(());
const FAILED: &str = "Could not read network services. Try again.";

/// The short data lock around the saved port table. It guards no in-memory state:
/// every holder re-reads `network.json` (written atomically) and the runtime's live
/// forwards, so a panic while holding it leaves nothing to repair. Recover instead
/// of failing every network read and save until restart (C-25, K-24 policy).
fn network_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::sync::lock_or_recover(&NETWORK_LOCK, "network settings")
}

/// Serializes one computer's runtime forwarding calls: a repair's add/remove sequence and
/// a read's snapshot of that computer's live forwards. Runtime calls can each take up to
/// their 3 s timeout, so they never run under the shared `network_lock`; other computers'
/// reads and saves do not wait behind them (C-26). Lock order: this lock first, then
/// `network_lock` only briefly for the settings file. Like `network_lock` it guards
/// no in-memory state, so poisoning is recovered.
fn forwarding_lock(computer: &str) -> std::sync::Arc<Mutex<()>> {
    static LOCKS: std::sync::OnceLock<Mutex<BTreeMap<String, std::sync::Arc<Mutex<()>>>>> =
        std::sync::OnceLock::new();
    crate::sync::lock_or_recover(LOCKS.get_or_init(Default::default), "port forwarding")
        .entry(computer.to_owned())
        .or_default()
        .clone()
}
fn hold(lock: &Mutex<()>) -> std::sync::MutexGuard<'_, ()> {
    crate::sync::lock_or_recover(lock, "port forwarding")
}
const LIMIT: usize = 128;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Mapping {
    computer: String,
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
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Port {
    configured_host_port: Option<u16>,
    port: u16,
    host_port: Option<u16>,
    scheme: Option<String>,
    state: &'static str,
    configured: bool,
    message: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Computer {
    computer: String,
    ports: Vec<Port>,
    error: Option<String>,
    /// Host name this computer's published websites open at (see `computer_host`).
    /// Assigned when local observations are assembled into the network state.
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
}

/// A per-computer loopback host name for published websites (C-24). Browsers keep
/// cookies per host name, not per port, so opening every computer at `127.0.0.1`
/// let one computer's page read and overwrite cookies of other local services and
/// computers. `*.localhost` resolves to loopback (RFC 6761) while the forward still
/// binds `127.0.0.1` only. The label joins the sanitised computer name and the start
/// of its immutable id, so a recreated computer with the same name gets a new host.
/// Cross-site requests to other loopback services are not prevented by this.
pub(crate) fn computer_host(name: &str, computer_id: &str) -> String {
    let id: String = computer_id
        .chars()
        .filter(char::is_ascii_hexdigit)
        .take(8)
        .collect::<String>()
        .to_ascii_lowercase();
    let mut label = String::new();
    for character in name.chars() {
        let character = character.to_ascii_lowercase();
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            label.push(character);
        } else if !label.is_empty() && !label.ends_with('-') {
            label.push('-');
        }
    }
    // One DNS label is at most 63 bytes, including the "-" and the id suffix.
    label.truncate(63 - 1 - id.len());
    let label = label.trim_end_matches('-');
    let label = if label.is_empty() { "sandbox" } else { label };
    if id.is_empty() {
        format!("{label}.localhost")
    } else {
        format!("{label}-{id}.localhost")
    }
}

/// The address a published website opens at.
pub(crate) fn website_url(scheme: &str, host: Option<&str>, port: u16) -> String {
    format!("{scheme}://{}:{port}", host.unwrap_or("127.0.0.1"))
}
#[derive(Serialize)]
pub struct State {
    computers: Vec<Computer>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Published {
    guest_port: u16,
    host_port: u16,
    host_bind: String,
}

fn validate(mapping: &Mapping) -> Result<(), String> {
    if mapping.port == 0
        || mapping.host_port == Some(0)
        || !matches!(mapping.scheme.as_deref(), None | Some("http" | "https"))
    {
        return Err("Enter a port from 1 to 65535 and a supported protocol.".into());
    }
    Ok(())
}
fn config_path(paths: &RuntimePaths) -> std::path::PathBuf {
    paths.metadata.with_file_name("network.json")
}
fn read_config(paths: &RuntimePaths) -> Result<Configuration, String> {
    let path = config_path(paths);
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Configuration::default()),
        Err(_) => return Err("Could not read saved ports.".into()),
    };
    let mut bytes = Vec::new();
    file.take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read saved ports.")?;
    if bytes.len() > 128 * 1024 {
        return Err("Saved ports are invalid.".into());
    }
    let config: Configuration =
        serde_json::from_slice(&bytes).map_err(|_| "Saved ports are invalid.")?;
    let mut seen = BTreeSet::new();
    if config.mappings.len() > 4096 {
        return Err("Too many saved ports.".into());
    }
    for mapping in &config.mappings {
        validate(mapping)?;
        if !seen.insert((&mapping.computer, mapping.port)) {
            return Err("Saved ports contain duplicates.".into());
        }
    }
    Ok(config)
}
fn write_config(paths: &RuntimePaths, config: &Configuration) -> Result<(), String> {
    let path = config_path(paths);
    let bytes = serde_json::to_vec(config).map_err(|_| "Could not save ports.")?;
    if bytes.len() > 128 * 1024 || config.mappings.len() > 4096 {
        return Err("Too many saved ports. Remove an unused port first.".into());
    }
    let parent = path.parent().ok_or("Could not save ports.")?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| "Could not save ports.")?;
    file.as_file_mut()
        .write_all(&bytes)
        .map_err(|_| "Could not save ports.")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "Could not save ports.")?;
    file.persist(&path).map_err(|_| "Could not save ports.")?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "Could not save ports.")?;
    Ok(())
}

// Linux /proc socket tables are available inside guests on both supported hosts.
// A loopback-only listener cannot be reached by the runtime's guest-IP forwarder.
fn parse_listeners(output: &str) -> Result<BTreeMap<u16, bool>, String> {
    let mut ports = BTreeMap::new();
    let mut header = false;
    if output.len() > 1024 * 1024 {
        return Err(FAILED.into());
    }
    for line in output.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.is_empty() {
            continue;
        }
        if fields[0] == "sl" {
            header = true;
            continue;
        }
        if fields.len() < 4 {
            return Err(FAILED.into());
        }
        if fields[3] != "0A" {
            continue;
        }
        let (address, port) = fields[1].split_once(':').ok_or(FAILED)?;
        if !matches!(address.len(), 8 | 32) || !address.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(FAILED.into());
        }
        let port = u16::from_str_radix(port, 16).map_err(|_| FAILED)?;
        if port == 0 {
            return Err(FAILED.into());
        }
        // IPv6-only sockets are not proof of reachability over the IPv4 publisher.
        let reachable_bind = address.len() == 8 && !address.ends_with("7F");
        ports
            .entry(port)
            .and_modify(|known| *known |= reachable_bind)
            .or_insert(reachable_bind);
        if ports.len() > 4096 {
            return Err(FAILED.into());
        }
    }
    if !header {
        return Err(FAILED.into());
    }
    Ok(ports)
}
fn listeners(paths: &RuntimePaths, computer: &str) -> Result<BTreeMap<u16, bool>, String> {
    let output = runtime::run_msb(
        paths,
        &[
            "exec".into(),
            computer.into(),
            "--no-start".into(),
            "--no-tty".into(),
            "--quiet".into(),
            "--timeout".into(),
            "3s".into(),
            "--".into(),
            "sh".into(),
            "-c".into(),
            "cat /proc/net/tcp && { if [ -f /proc/net/tcp6 ]; then cat /proc/net/tcp6; fi; }"
                .into(),
        ],
        Duration::from_secs(5),
    )
    .map_err(|_| FAILED)?;
    parse_listeners(&output.stdout)
}

#[cfg(unix)]
fn control_response(socket: &Path, request: Value) -> Result<Value, String> {
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(socket)
        .map_err(|_| "Network controls are unavailable. Restart this computer and retry.")?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|_| FAILED)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|_| FAILED)?;
    serde_json::to_writer(&mut stream, &request).map_err(|_| FAILED)?;
    stream.write_all(b"\n").map_err(|_| FAILED)?;
    let mut response = Vec::new();
    use std::io::Read;
    BufReader::new(stream)
        .take(64 * 1024 + 1)
        .read_until(b'\n', &mut response)
        .map_err(|_| "Network operation timed out. Retry to check its result.")?;
    if response.len() > 64 * 1024 || response.last() != Some(&b'\n') {
        return Err(FAILED.into());
    }
    let value: Value = serde_json::from_slice(&response).map_err(|_| FAILED)?;
    if value["ok"] != true {
        let message = value["error"].as_str().unwrap_or("");
        return Err(if message.contains("Address already in use") || message.contains("address already in use") { "This local port is already in use. Choose another or use Automatic." }
            else if message.contains("Permission denied") || message.contains("permission denied") { "This local port needs elevated privileges. Choose a port above 1023." }
            else { "Could not update port forwarding. Restart the computer if its runtime was updated, then retry." }.into());
    }
    Ok(value)
}
fn control(socket: &Path, request: Value) -> Result<Vec<Published>, String> {
    let value = control_response(socket, request)?;
    let mut ports: Vec<Published> =
        serde_json::from_value(value["ports"].clone()).map_err(|_| FAILED)?;
    if ports.len() > LIMIT
        || ports
            .iter()
            .any(|p| p.guest_port == 0 || p.host_port == 0 || p.host_bind != "127.0.0.1")
    {
        return Err(FAILED.into());
    }
    ports.sort_by_key(|p| p.guest_port);
    if ports
        .windows(2)
        .any(|pair| pair[0].guest_port == pair[1].guest_port)
    {
        return Err(FAILED.into());
    }
    Ok(ports)
}

fn socket_path(paths: &RuntimePaths, computer: &str) -> std::path::PathBuf {
    use sha2::{Digest, Sha256};
    let digest = format!("{:x}", Sha256::digest(computer.as_bytes()));
    paths
        .home
        .join("run/sandboxes")
        .join(&digest[..24])
        .join("control.sock")
}
/// Forget saved forwarding intent before a deleted computer's name becomes reusable.
pub(crate) fn computer_removed(paths: &RuntimePaths, computer: &str) -> Result<(), String> {
    let forwarding = forwarding_lock(computer);
    let _forwarding = hold(&forwarding);
    let _data = network_lock();
    let mut config = read_config(paths)?;
    let before = config.mappings.len();
    config.mappings.retain(|m| m.computer != computer);
    if config.mappings.len() != before {
        write_config(paths, &config)?;
    }
    Ok(())
}

/// Observe one computer's saved forwards with a single enabled mapping for `port`.
#[cfg(test)]
pub(crate) fn observe_saved_port_for_test(
    paths: &RuntimePaths,
    computer: &str,
    port: u16,
) -> (Option<String>, Vec<(&'static str, Option<String>)>) {
    let config = Configuration {
        mappings: vec![Mapping {
            computer: computer.into(),
            port,
            host_port: None,
            scheme: Some("http".into()),
            enabled: true,
        }],
    };
    let observed = observe(paths, computer, &config, &BTreeMap::new());
    (
        observed.error,
        observed
            .ports
            .into_iter()
            .map(|port| (port.state, port.message))
            .collect(),
    )
}

/// A Silo computer's runtime state, or `None` when it is stopped with no runtime computer yet.
fn configured_computer(
    paths: &RuntimePaths,
    name: &str,
) -> Result<Option<runtime::InspectedSandbox>, String> {
    let metadata = runtime::read_metadata(&paths.metadata)
        .map_err(|_| "Could not read computer configuration.")?;
    if !metadata.computers.iter().any(|m| m.name() == name) {
        return Err("Choose a local Silo computer.".into());
    }
    let inspected = match runtime::observe_computer(&ProcessRunner, paths, name)
        .map_err(|error| format!("Could not inspect this computer: {error}"))?
    {
        runtime::ComputerRuntime::Present(inspected) => inspected,
        // Not created yet (waiting for a checkpoint restore on Start): it is stopped.
        runtime::ComputerRuntime::Absent => return Ok(None),
    };
    runtime::ensure_managed(&inspected).map_err(|_| "This computer is not managed by Silo.")?;
    Ok(Some(inspected))
}
fn pending(mapping: &Mapping, message: Option<String>, state: &'static str) -> Port {
    Port {
        configured_host_port: mapping.host_port,
        port: mapping.port,
        host_port: None,
        scheme: mapping.scheme.clone(),
        configured: true,
        state,
        message,
    }
}
/// Read-only observation of one computer's forwards. It never publishes, removes, or
/// rewrites a forward and never takes the operation gate, so a status read stays
/// available while other operations run. `failures` carries per-port messages from
/// a just-completed repair on a write path; it is empty for a plain read. Repair
/// itself lives in `reconcile_forwarding`.
fn observe(
    paths: &RuntimePaths,
    computer: &str,
    config: &Configuration,
    failures: &BTreeMap<u16, String>,
) -> Computer {
    let desired: Vec<_> = config
        .mappings
        .iter()
        .filter(|m| m.computer == computer)
        .collect();
    let mut result = Computer {
        computer: computer.into(),
        ports: vec![],
        error: None,
        host: None,
    };
    let state = match configured_computer(paths, computer) {
        Ok(Some(state)) => state,
        Ok(None) => {
            result.ports = desired
                .iter()
                .filter(|m| m.enabled)
                .map(|m| pending(m, None, "waiting"))
                .collect();
            return result;
        }
        Err(e) => {
            result.ports = desired
                .iter()
                .map(|m| pending(m, Some(e.clone()), "unknown"))
                .collect();
            result.error = Some(e);
            return result;
        }
    };
    if state.status != "Running" {
        result.ports = desired
            .iter()
            .filter(|m| m.enabled)
            .map(|m| pending(m, None, "waiting"))
            .collect();
        return result;
    }
    // Hold this computer's forwarding lock only to read a consistent snapshot of the desired
    // settings and the live forwards, so a repair of this computer is never seen half done.
    // It is dropped before the slow guest probes, and this read never mutates the
    // forwarding table or the settings file. Other computers never wait on it.
    let forwarding = forwarding_lock(computer);
    let guard = hold(&forwarding);
    // Read the current desired revision only after obtaining the lock.
    // An older refresh must never observe against access removed by another window.
    let config = match {
        let _data = network_lock();
        read_config(paths)
    } {
        Ok(config) => config,
        Err(e) => {
            result.error = Some(e);
            return result;
        }
    };
    let desired: Vec<_> = config
        .mappings
        .iter()
        .filter(|m| m.computer == computer)
        .collect();
    let socket = socket_path(paths, computer);
    let published = match control(&socket, json!({"op":"ports_list"})) {
        Ok(ports) => ports,
        Err(e) => {
            drop(guard);
            result.ports = listeners(paths, computer)
                .unwrap_or_default()
                .keys()
                .map(|port| Port {
                    configured_host_port: None,
                    port: *port,
                    host_port: None,
                    scheme: None,
                    configured: false,
                    state: "unpublished",
                    message: None,
                })
                .collect();
            for mapping in desired {
                result.ports.retain(|p| p.port != mapping.port);
                result
                    .ports
                    .push(pending(mapping, Some(e.clone()), "unknown"));
            }
            result.error = Some(e);
            result.ports.sort_by_key(|p| p.port);
            return result;
        }
    };
    drop(guard);
    // Discovery and reachability can be slow; they run without the data lock.
    let listeners = match listeners(paths, computer) {
        Ok(ports) => ports,
        Err(e) => {
            result.ports = desired
                .iter()
                .filter(|m| m.enabled || published.iter().any(|p| p.guest_port == m.port))
                .map(|m| pending(m, Some(e.clone()), "unknown"))
                .collect();
            result.error = Some(e);
            return result;
        }
    };
    let mut reachable = BTreeMap::new();
    let candidates: Vec<_> = desired
        .iter()
        .filter(|m| {
            m.enabled
                && listeners.contains_key(&m.port)
                && published.iter().any(|p| p.guest_port == m.port)
        })
        .collect();
    for batch in candidates.chunks(3) {
        std::thread::scope(|scope| {
            let handles: Vec<_> = batch
                .iter()
                .map(|m| {
                    let socket = &socket;
                    scope.spawn(move || {
                        control_response(socket, json!({"op":"port_probe","guest_port":m.port}))
                            .and_then(|v| v["reachable"].as_bool().ok_or_else(|| FAILED.into()))
                    })
                })
                .collect();
            for (mapping, handle) in batch.iter().zip(handles) {
                reachable.insert(
                    mapping.port,
                    handle.join().unwrap_or_else(|_| Err(FAILED.into())),
                );
            }
        });
    }
    let mut rows: BTreeMap<u16, Port> = listeners
        .keys()
        .map(|port| {
            (
                *port,
                Port {
                    configured_host_port: None,
                    port: *port,
                    host_port: None,
                    scheme: None,
                    state: "unpublished",
                    configured: false,
                    message: None,
                },
            )
        })
        .collect();
    for mapping in desired {
        let active = published.iter().find(|p| p.guest_port == mapping.port);
        if !mapping.enabled && active.is_none() && !failures.contains_key(&mapping.port) {
            continue;
        }
        let (status, message) = if let Some(e) = failures.get(&mapping.port) {
            (
                "unknown",
                Some(if mapping.enabled {
                    e.clone()
                } else {
                    format!("Access could not be removed. {e}")
                }),
            )
        } else if !mapping.enabled {
            (
                "unknown",
                Some("Access could not be removed. Retry removing this port.".into()),
            )
        } else if !listeners.contains_key(&mapping.port) {
            ("waiting", None)
        } else if matches!(reachable.get(&mapping.port), Some(Ok(true))) {
            ("reachable", None)
        } else if let Some(Err(e)) = reachable.get(&mapping.port) {
            ("unknown", Some(e.clone()))
        } else if active.is_some() {
            ("waiting", Some("The service is not reachable through this port. Check its bind address and network policy; use 0.0.0.0 inside the computer.".into()))
        } else {
            (
                "unknown",
                Some("Port forwarding could not be verified.".into()),
            )
        };
        rows.insert(
            mapping.port,
            Port {
                configured_host_port: mapping.host_port,
                port: mapping.port,
                host_port: active.map(|p| p.host_port),
                scheme: mapping.scheme.clone(),
                state: status,
                configured: true,
                message,
            },
        );
    }
    result.ports = rows.into_values().collect();
    // Guest probes run without blocking mutations. Never publish their result
    // against settings or endpoints that changed while those probes were running.
    let latest = (|| -> Result<(), String> {
        let _guard = hold(&forwarding);
        let current = {
            let _data = network_lock();
            read_config(paths)?
        };
        let current: Vec<_> = current
            .mappings
            .iter()
            .filter(|m| m.computer == computer)
            .cloned()
            .collect();
        let previous: Vec<_> = config
            .mappings
            .iter()
            .filter(|m| {
                m.computer == computer
                    && (m.enabled || published.iter().any(|p| p.guest_port == m.port))
            })
            .cloned()
            .collect();
        if current != previous {
            return Err("Network settings changed. Refresh to check the current ports.".into());
        }
        if control(&socket, json!({"op":"ports_list"}))? != published {
            return Err("Port forwarding changed. Refresh to check the current ports.".into());
        }
        Ok(())
    })();
    if let Err(error) = latest {
        for port in &mut result.ports {
            port.state = "unknown";
            port.host_port = None;
        }
        result.error = Some(error);
    }
    result
}
/// Apply one computer's saved desired forwards to its live runtime: add and remove
/// published ports and prune confirmed tombstones. This changes shared host
/// networking, so every caller must already hold the operation gate for this computer.
/// It never takes the gate itself (that would be `GateError::Nested`): the write
/// commands hold `OPERATIONS.computer`, the background scheduler holds `OPERATIONS.try_computer`,
/// and `reconcile_started` runs while the computer lifecycle caller holds the computer guard.
/// Returns per-port failure messages so a write path can surface them, and an
/// error when the saved settings cannot be read. When the runtime's controls cannot
/// be reached every saved port of the computer carries that failure; a write path shows it
/// only while the computer runs (a stopped computer's ports read as waiting and reconcile on the
/// next start). Nothing is published when the computer is not running.
///
/// The shared `network_lock` is held only to read the settings and to prune them;
/// the runtime calls run under this computer's `forwarding_lock`, so a slow or hung runtime
/// never blocks other computers' reads and saves (C-26).
fn reconcile_forwarding(
    paths: &RuntimePaths,
    computer: &str,
) -> Result<BTreeMap<u16, String>, String> {
    debug_assert!(
        runtime::operation_gate::held(),
        "port reconciliation requires the computer operation gate"
    );
    let mut failures = BTreeMap::new();
    let forwarding = forwarding_lock(computer);
    let _forwarding = hold(&forwarding);
    let desired: Vec<Mapping> = {
        let _data = network_lock();
        read_config(paths)?
            .mappings
            .into_iter()
            .filter(|m| m.computer == computer)
            .collect()
    };
    if desired.is_empty() {
        return Ok(failures);
    }
    let socket = socket_path(paths, computer);
    let mut published = match control(&socket, json!({"op":"ports_list"})) {
        Ok(ports) => ports,
        Err(e) => {
            return Ok(desired.iter().map(|m| (m.port, e.clone())).collect());
        }
    };
    for mapping in &desired {
        let exists = published.iter().find(|p| p.guest_port == mapping.port);
        let request = if !mapping.enabled {
            exists.map(|_| json!({"op":"port_remove","guest_port":mapping.port}))
        } else if exists.is_some_and(|p| mapping.host_port.is_none_or(|host| host == p.host_port)) {
            None
        } else {
            Some(
                json!({"op":"port_add","guest_port":mapping.port,"host_port":mapping.host_port.unwrap_or(0)}),
            )
        };
        if let Some(request) = request {
            match control(&socket, request) {
                Ok(ports) => published = ports,
                Err(e) => {
                    failures.insert(mapping.port, e.clone());
                    if e.contains("timed out") || e.to_lowercase().contains("restart") {
                        for remaining in &desired {
                            failures.entry(remaining.port).or_insert_with(|| e.clone());
                        }
                        break;
                    }
                }
            }
        }
    }
    // Remove confirmed tombstones, so repeated add/remove never grows settings forever.
    // Other computers may have saved while the runtime calls ran: prune the current file.
    let _data = network_lock();
    if let Ok(mut current) = read_config(paths) {
        let before = current.mappings.len();
        current.mappings.retain(|m| {
            m.computer != computer || m.enabled || published.iter().any(|p| p.guest_port == m.port)
        });
        if current.mappings.len() != before {
            let _ = write_config(paths, &current);
        }
    }
    Ok(failures)
}
fn reconcile_computers(config: &Configuration) -> BTreeSet<String> {
    config.mappings.iter().map(|m| m.computer.clone()).collect()
}
/// Reconcile the affected computer's forwards on a background thread, skipping it when
/// that computer is busy, so a read can return immediately while repair converges. Each
/// computer is repaired under its own gate guard.
fn schedule_network_reconcile(paths: RuntimePaths, config: &Configuration) {
    let computers = reconcile_computers(config);
    if computers.is_empty() {
        return;
    }
    // Reads can arrive in bursts; one repair pass at a time is enough.
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if RUNNING.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(move || {
        struct Done;
        impl Drop for Done {
            fn drop(&mut self) {
                RUNNING.store(false, std::sync::atomic::Ordering::Release);
            }
        }
        let _done = Done;
        if runtime::shutdown::ensure_accepting_operations().is_err() {
            return;
        }
        for computer in computers {
            let Ok(computer_id) = runtime::resolve_computer_id(&paths, &computer) else {
                continue;
            };
            if let Ok(_gate) = runtime::OPERATIONS.try_computer_hidden(
                &computer_id,
                &computer,
                &format!("Reconciling ports on {computer}"),
            ) {
                let _ = reconcile_forwarding(&paths, &computer);
            }
        }
    });
}
/// Every local computer's observed ports, each with its computer host name.
fn state_with(paths: &RuntimePaths, config: &Configuration) -> Result<State, String> {
    let metadata = runtime::read_metadata(&paths.metadata)
        .map_err(|_| "Could not read computer configuration.")?;
    let mut computers = vec![];
    let empty = BTreeMap::new();
    let configured: Vec<_> = metadata
        .computers
        .iter()
        .map(|m| (m.name(), m.id()))
        .collect();
    for batch in configured.chunks(3) {
        std::thread::scope(|scope| {
            let handles: Vec<_> = batch
                .iter()
                .map(|(name, _)| {
                    let empty = &empty;
                    scope.spawn(move || observe(paths, name, config, empty))
                })
                .collect();
            for ((name, id), handle) in batch.iter().zip(handles) {
                let mut computer = handle.join().unwrap_or_else(|_| Computer {
                    computer: (*name).into(),
                    ports: vec![],
                    error: Some(FAILED.into()),
                    host: None,
                });
                computer.host = Some(computer_host(name, id));
                computers.push(computer);
            }
        });
    }
    Ok(State { computers })
}

/// Apply one computer's just-saved intent under the caller's computer gate and return the new
/// state, with any failure from that repair shown on the computer's ports. Every window is
/// told to refresh even when the repair could not run, because the intent was saved.
fn apply_saved(paths: &RuntimePaths, computer: &str) -> Result<State, String> {
    let result = reconcile_forwarding(paths, computer).and_then(|failures| {
        let config = read_config(paths)?;
        let mut state = state_with(paths, &config)?;
        if !failures.is_empty() {
            let mut repaired = observe(paths, computer, &config, &failures);
            if let Some(slot) = state.computers.iter_mut().find(|w| w.computer == computer) {
                repaired.host = slot.host.take();
                *slot = repaired;
            }
        }
        Ok(state)
    });
    if let Some(context) = silo_engine::context::current() {
        context.publish(
            silo_engine::context::Event::new("silo://network-state-changed", json!(null)),
            silo_engine::context::Audience::All,
        );
    }
    result
}

pub fn read_network_state(paths: &RuntimePaths) -> Result<State, String> {
    // Observation only: never take the operation gate and never publish or
    // remove a forward, so a read cannot wait behind a long operation. Any
    // forward that drifted from its saved intent is repaired in the background.
    let config = read_config(paths)?;
    let state = state_with(paths, &config);
    schedule_network_reconcile(paths.clone(), &config);
    state
}
pub fn save_network_port(
    paths: &RuntimePaths,
    computer: String,
    port: u16,
    host_port: Option<u16>,
    scheme: Option<String>,
) -> Result<State, String> {
    let _update = crate::updates::operation_guard()?;
    // Publishing a port changes this computer's shared host forwarding; wait its turn
    // for that computer. NETWORK_LOCK stays the short data lock around the saved table.
    let computer_id = runtime::resolve_computer_id(paths, &computer).map_err(|e| e.to_string())?;
    let _gate = runtime::OPERATIONS
        .kind(runtime::operation_gate::OperationKind::PortPublish)
        .computer(
            &computer_id,
            &computer,
            &format!("Publishing a port on {computer}"),
        )
        .map_err(|e| e.to_string())?;
    runtime::shutdown::ensure_accepting_operations()?;
    configured_computer(paths, &computer)?;
    let mapping = Mapping {
        computer: computer.clone(),
        port,
        host_port,
        scheme,
        enabled: true,
    };
    validate(&mapping)?;
    {
        let _guard = network_lock();
        let mut config = read_config(paths)?;
        if config
            .mappings
            .iter()
            .filter(|m| m.computer == computer && m.enabled && m.port != port)
            .count()
            >= LIMIT
        {
            return Err("This computer already has 128 published ports.".into());
        }
        config
            .mappings
            .retain(|m| m.computer != computer || m.port != port);
        config.mappings.push(mapping);
        if config.mappings.len() > 4096 {
            return Err("Too many saved ports. Remove an unused port first.".into());
        }
        write_config(paths, &config)?;
    }
    apply_saved(paths, &computer)
}
pub fn remove_network_port(
    paths: &RuntimePaths,
    computer: String,
    port: u16,
) -> Result<State, String> {
    let _update = crate::updates::operation_guard()?;
    // Removing a port changes this computer's shared host forwarding; wait its turn
    // for that computer. NETWORK_LOCK stays the short data lock around the saved table.
    let computer_id = runtime::resolve_computer_id(paths, &computer).map_err(|e| e.to_string())?;
    let _gate = runtime::OPERATIONS
        .kind(runtime::operation_gate::OperationKind::PortRemove)
        .computer(
            &computer_id,
            &computer,
            &format!("Removing a port on {computer}"),
        )
        .map_err(|e| e.to_string())?;
    runtime::shutdown::ensure_accepting_operations()?;
    configured_computer(paths, &computer)?;
    {
        let _guard = network_lock();
        let mut config = read_config(paths)?;
        // Persist removal intent before touching the live listener. Failed removals
        // remain visible and reconcile on retry/relaunch, never silently reopen.
        if let Some(mapping) = config
            .mappings
            .iter_mut()
            .find(|m| m.computer == computer && m.port == port)
        {
            mapping.enabled = false;
        }
        write_config(paths, &config)?;
    }
    // Other devices forward the port through guest streams, not this listener.
    crate::remote::revoke_port_streams(&computer_id, port);
    apply_saved(paths, &computer)
}
pub fn network_port_url(
    paths: &RuntimePaths,
    computer: String,
    port: u16,
) -> Result<String, String> {
    // Opening an already-reachable endpoint only observes; it takes no gate.
    let state = observe(paths, &computer, &read_config(paths)?, &BTreeMap::new());
    let endpoint = state
        .ports
        .iter()
        .find(|p| p.port == port && p.configured && p.state == "reachable")
        .ok_or("This service is not reachable.")?;
    let scheme = endpoint
        .scheme
        .as_deref()
        .ok_or("This TCP service is not configured as a website.")?;
    let host_port = endpoint.host_port.ok_or("This service is not reachable.")?;
    let computer_id = runtime::resolve_computer_id(paths, &computer).map_err(|e| e.to_string())?;
    let host = computer_host(&computer, &computer_id);
    Ok(website_url(scheme, Some(&host), host_port))
}

// Called after successful starts, including launch-at-login and temporary starts.
// Port failures must not turn a successful computer start into a computer lifecycle failure.
// The lifecycle caller already holds this computer's operation guard, so this repair must
// NOT take the gate (that would be `GateError::Nested`); it calls the gate-free
// `reconcile_forwarding` directly.
pub(crate) fn reconcile_started(paths: &RuntimePaths, computer: &str) {
    if let Ok(config) = read_config(paths) {
        if config.mappings.iter().any(|m| m.computer == computer) {
            let _ = reconcile_forwarding(paths, computer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrated_port_forwards_load() {
        let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
        let config = read_config(&migrated.runtime_paths()).unwrap();
        assert_eq!(config.mappings.len(), 1);
        assert_eq!(config.mappings[0].computer, "dev");
        assert_eq!(config.mappings[0].host_port, Some(8080));
    }

    #[cfg(unix)]
    #[test]
    fn saved_ports_report_an_unreadable_parent_after_publication() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let directory = tempfile::tempdir().unwrap();
        if fs::metadata(directory.path()).unwrap().uid() == 0 {
            return; // Root bypasses the permission boundary exercised here.
        }
        let paths = crate::test_support::paths(directory.path());
        let config = Configuration {
            mappings: vec![Mapping {
                computer: "fixture-computer".into(),
                port: 3000,
                host_port: None,
                scheme: Some("http".into()),
                enabled: false,
            }],
        };
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o300)).unwrap();
        let result = write_config(&paths, &config);
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(read_config(&paths).unwrap().mappings, config.mappings);
        assert!(
            result.is_err(),
            "an unsynchronized rename must not report success"
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        write_config(&paths, &config).unwrap();
    }

    #[test]
    fn saved_ports_accept_the_size_limit_and_reject_one_extra_byte() {
        let directory = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(directory.path());
        let mut bytes = br#"{"mappings":[]}"#.to_vec();
        bytes.resize(128 * 1024, b' ');
        fs::write(config_path(&paths), &bytes).unwrap();
        assert!(read_config(&paths).unwrap().mappings.is_empty());
        bytes.push(b' ');
        fs::write(config_path(&paths), &bytes).unwrap();
        assert_eq!(
            read_config(&paths).err().unwrap(),
            "Saved ports are invalid."
        );
    }

    #[cfg(unix)]
    #[test]
    fn oversized_saved_ports_stop_reading_before_the_input_closes() {
        use std::{ffi::CString, os::unix::ffi::OsStrExt, sync::mpsc, thread};
        let directory = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(directory.path());
        let path = config_path(&paths);
        let fifo = CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let (release, released) = mpsc::channel();
        let writer = thread::spawn(move || {
            let mut file = fs::OpenOptions::new().write(true).open(path).unwrap();
            file.write_all(&vec![b' '; 128 * 1024 + 1]).unwrap();
            released.recv_timeout(Duration::from_secs(5)).is_ok()
        });
        let error = read_config(&paths).err().unwrap();
        let _ = release.send(());
        let stopped_before_eof = writer.join().unwrap();
        assert_eq!(error, "Saved ports are invalid.");
        assert!(stopped_before_eof, "oversized reader waited for EOF");
    }
    #[cfg(unix)]
    fn control_reply(response: &str) -> Result<Vec<Published>, String> {
        use std::os::unix::net::UnixListener;
        // Keep the socket below macOS's pathname length limit.
        let directory = tempfile::tempdir_in(crate::test_support::live::temp_root()).unwrap();
        let path = directory.path().join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let response = response.to_owned();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&request).unwrap()["op"],
                "ports_list"
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        let result = control(&path, json!({"op":"ports_list"}));
        server.join().unwrap();
        result
    }

    #[test]
    #[cfg(unix)]
    fn runtime_errors_are_short_and_do_not_expose_raw_details() {
        let _test_state = crate::test_support::global_state();
        let conflict = control_reply(
            "{\"ok\":false,\"error\":\"Address already in use: private runtime diagnostics\"}\n",
        )
        .unwrap_err();
        assert_eq!(
            conflict,
            "This local port is already in use. Choose another or use Automatic."
        );
        let unknown = control_reply("{\"ok\":false,\"error\":\"private runtime diagnostics\"}\n")
            .unwrap_err();
        assert!(!unknown.contains("private runtime diagnostics"));
        assert!(unknown.contains("Could not update port forwarding"));
    }

    #[test]
    #[cfg(unix)]
    fn invalid_or_externally_bound_runtime_ports_never_succeed() {
        let _test_state = crate::test_support::global_state();
        assert!(control_reply("{\"ok\":true,\"ports\":[{\"guest_port\":3000,\"host_port\":43000,\"host_bind\":\"0.0.0.0\"}]}\n").is_err());
        assert!(control_reply("{\"ok\":true}\n").is_err());
        assert!(control_reply("{\"ok\":true,\"ports\":[]}").is_err());
        assert!(control_reply("not json\n").is_err());
        let ports = control_reply("{\"ok\":true,\"ports\":[{\"guest_port\":3000,\"host_port\":43000,\"host_bind\":\"127.0.0.1\"}]}\n").unwrap();
        assert_eq!(ports[0].host_port, 43000);
    }

    #[test]
    fn discovers_tcp_listeners_without_guessing_from_connections() {
        let _test_state = crate::test_support::global_state();
        let input="sl local_address rem_address st\n0: 00000000:0BB8 00000000:0000 0A\n1: 0100007F:1538 00000000:0000 0A\n2: 00000000:0050 00000000:0000 01\n";
        let ports = parse_listeners(input).unwrap();
        assert_eq!(ports.get(&3000), Some(&true));
        assert_eq!(ports.get(&5432), Some(&false));
        assert!(!ports.contains_key(&80));
    }
    #[test]
    fn invalid_socket_output_is_not_empty_success() {
        let _test_state = crate::test_support::global_state();
        assert!(parse_listeners("broken").is_err());
        assert!(parse_listeners("").is_err());
        assert!(parse_listeners("0: ZZZZZZZZ:1234 x 0A").is_err());
        assert!(parse_listeners("sl local_address rem_address st\n")
            .unwrap()
            .is_empty());
    }
    #[test]
    fn rejects_invalid_ports_and_schemes() {
        let _test_state = crate::test_support::global_state();
        let mut m = Mapping {
            computer: "dev".into(),
            port: 3000,
            host_port: None,
            scheme: Some("http".into()),
            enabled: true,
        };
        assert!(validate(&m).is_ok());
        m.host_port = Some(0);
        assert!(validate(&m).is_err());
        m.host_port = None;
        m.scheme = Some("file".into());
        assert!(validate(&m).is_err());
    }
    #[test]
    fn pending_removal_survives_relaunch_and_corrupt_settings_are_rejected() {
        let _test_state = crate::test_support::global_state();
        let temp = tempfile::tempdir().unwrap();
        let paths = RuntimePaths {
            guest_image: crate::test_support::manifest_root().join("runtime/guest-image"),
            executable: temp.path().join("msb"),
            home: temp.path().into(),
            storage_home: None,
            library: temp.path().join("lib"),
            metadata: temp.path().join("computers.json"),
            volumes: temp.path().join("volumes"),
        };
        let config = Configuration {
            mappings: vec![Mapping {
                computer: "dev".into(),
                port: 3000,
                host_port: None,
                scheme: Some("http".into()),
                enabled: false,
            }],
        };
        write_config(&paths, &config).unwrap();
        assert!(!read_config(&paths).unwrap().mappings[0].enabled);
        fs::write(config_path(&paths), "broken").unwrap();
        assert!(read_config(&paths).is_err());
    }
    #[test]
    fn read_state_returns_without_waiting_for_the_operation_gate() {
        let _test_state = crate::test_support::global_state();
        use std::time::Instant;
        let temp = tempfile::tempdir().unwrap();
        let paths = RuntimePaths {
            guest_image: crate::test_support::manifest_root().join("runtime/guest-image"),
            // A nonexistent executable makes computer inspection fail fast, so the read
            // never blocks on a live runtime and never mutates anything.
            executable: temp.path().join("msb"),
            home: temp.path().into(),
            storage_home: None,
            library: temp.path().join("lib"),
            metadata: temp.path().join("computers.json"),
            volumes: temp.path().join("volumes"),
        };
        fs::write(
            &paths.metadata,
            serde_json::json!({"schemaVersion":1,"computers":[{
                "id":"00000000-0000-4000-8000-000000000001","name":"dev",
                "cpus":2,"maxCPUs":2,"memoryGiB":2,"maxMemoryGiB":2,
                "workspaceStorageGiB":10,"runtimeStorageGiB":10
            }]})
            .to_string(),
        )
        .unwrap();
        let config = Configuration::default();
        // Hold a per-computer guard for "dev" on another thread for the whole read. The
        // read path takes no gate, so it must return promptly regardless.
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (held_tx, held_rx) = std::sync::mpsc::channel::<()>();
        let holder = std::thread::spawn(move || {
            let _guard = runtime::OPERATIONS
                .computer(
                    "00000000-0000-4000-8000-000000000001",
                    "dev",
                    "Blocking network read test",
                )
                .unwrap();
            held_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        held_rx.recv().unwrap();
        let start = Instant::now();
        let state = state_with(&paths, &config);
        let elapsed = start.elapsed();
        release_tx.send(()).unwrap();
        holder.join().unwrap();
        let state = state.expect("read state");
        assert_eq!(state.computers.len(), 1);
        assert_eq!(
            state.computers[0].host.as_deref(),
            Some("dev-00000000.localhost")
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "network read waited for the operation gate: {elapsed:?}"
        );
    }

    #[test]
    fn the_control_socket_is_where_microsandbox_creates_it() {
        let temp = tempfile::tempdir_in(crate::test_support::live::temp_root()).unwrap();
        let paths = temp_paths(&temp);
        // MicroSandbox's own layout: run/sandboxes/<first 24 hex of SHA-256(name)>/control.sock.
        assert_eq!(
            socket_path(&paths, "dev"),
            paths
                .home
                .join("run/sandboxes/ef260e9aa3c673af240d17a2/control.sock")
        );
    }

    #[test]
    fn each_computer_gets_its_own_valid_localhost_name() {
        let _test_state = crate::test_support::global_state();
        let id = "1A2B3C4D-0000-4000-8000-000000000001";
        assert_eq!(computer_host("dev", id), "dev-1a2b3c4d.localhost");
        // Same name, different computer: a different host, so no shared cookies.
        assert_ne!(
            computer_host("dev", id),
            computer_host("dev", "99999999-0000-4000-8000-000000000001")
        );
        // Names from another device are sanitised into one DNS label.
        assert_eq!(
            computer_host("My App_2!", id),
            "my-app-2-1a2b3c4d.localhost"
        );
        assert_eq!(computer_host("--", id), "sandbox-1a2b3c4d.localhost");
        assert_eq!(computer_host("dev", ""), "dev.localhost");
        for name in [
            "a".repeat(80),
            format!("{}-b", "a".repeat(53)),
            "ünïcode.évil/../x".into(),
        ] {
            let host = computer_host(&name, id);
            let label = host.strip_suffix(".localhost").unwrap();
            assert!(label.len() <= 63, "{host}");
            assert!(
                !label.starts_with('-') && !label.ends_with('-') && !label.contains("--"),
                "{host}"
            );
            assert!(
                label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "{host}"
            );
        }
        assert_eq!(
            website_url("http", Some("dev-1a2b3c4d.localhost"), 43000),
            "http://dev-1a2b3c4d.localhost:43000"
        );
        assert_eq!(website_url("https", None, 43000), "https://127.0.0.1:43000");
    }

    fn temp_paths(temp: &tempfile::TempDir) -> RuntimePaths {
        RuntimePaths {
            guest_image: temp.path().join("image"),
            executable: temp.path().join("msb"),
            home: temp.path().into(),
            storage_home: None,
            library: temp.path().join("lib"),
            metadata: temp.path().join("computers.json"),
            volumes: temp.path().join("volumes"),
        }
    }

    fn one_port(computer: &str, port: u16, enabled: bool) -> Configuration {
        Configuration {
            mappings: vec![Mapping {
                computer: computer.into(),
                port,
                host_port: None,
                scheme: Some("http".into()),
                enabled,
            }],
        }
    }

    #[test]
    fn poisoned_network_lock_is_recovered_and_reconcile_failures_are_reported() {
        let _test_state = crate::test_support::global_state();
        let gate = runtime::operation_gate::OperationGate::new();
        let _guard = gate
            .computer("test-id", "dev", "Reconciling test ports")
            .unwrap();
        let temp = tempfile::tempdir_in(crate::test_support::live::temp_root()).unwrap();
        let paths = temp_paths(&temp);
        write_config(&paths, &one_port("dev", 3000, true)).unwrap();
        let _ = std::thread::spawn(|| {
            let _guard = NETWORK_LOCK.lock();
            panic!("poison the network lock for this test");
        })
        .join();
        assert!(NETWORK_LOCK.is_poisoned());
        // No control socket exists, so the runtime cannot be reached: that is a
        // failure for every saved port of the computer, never an empty (successful) result.
        let failures = reconcile_forwarding(&paths, "dev").unwrap();
        assert!(
            failures
                .get(&3000)
                .is_some_and(|e| e.contains("Network controls are unavailable")),
            "{failures:?}"
        );
        // Unreadable settings are an error, not "nothing to repair".
        fs::write(config_path(&paths), "broken").unwrap();
        assert!(reconcile_forwarding(&paths, "dev").is_err());
        NETWORK_LOCK.clear_poison();
    }

    /// A runtime control socket that answers `ports_list` at once and each
    /// `port_add` only after `delay`, like a runtime under load. It reports each
    /// accepted `port_add` on `added` before waiting.
    #[cfg(unix)]
    fn slow_runtime(
        socket: std::path::PathBuf,
        connections: usize,
        delay: Duration,
        added: std::sync::mpsc::Sender<u16>,
    ) -> std::thread::JoinHandle<()> {
        use std::os::unix::net::UnixListener;
        fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            let mut published: Vec<Value> = vec![];
            for _ in 0..connections {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                let request: Value = serde_json::from_str(&request).unwrap();
                if request["op"] == "port_add" {
                    let port = request["guest_port"].as_u64().unwrap() as u16;
                    added.send(port).unwrap();
                    std::thread::sleep(delay);
                    published.push(
                        json!({"guest_port":port,"host_port":40000 + port,"host_bind":"127.0.0.1"}),
                    );
                }
                let reply = json!({"ok":true,"ports":published});
                stream.write_all(format!("{reply}\n").as_bytes()).unwrap();
            }
        })
    }

    #[test]
    #[cfg(unix)]
    fn repairing_one_computer_never_holds_the_network_lock_across_runtime_calls() {
        let _test_state = crate::test_support::global_state();
        let temp = tempfile::tempdir_in(crate::test_support::live::temp_root()).unwrap();
        let paths = temp_paths(&temp);
        let mut config = one_port("dev", 3000, true);
        config.mappings.push(Mapping {
            port: 3001,
            ..config.mappings[0].clone()
        });
        write_config(&paths, &config).unwrap();
        let (added_tx, added) = std::sync::mpsc::channel();
        // ports_list, then two slow port_add calls.
        let runtime = slow_runtime(
            socket_path(&paths, "dev"),
            3,
            Duration::from_millis(800),
            added_tx,
        );
        let repair = {
            let paths = paths.clone();
            std::thread::spawn(move || {
                let gate = runtime::operation_gate::OperationGate::new();
                let _guard = gate
                    .computer("test-id", "dev", "Reconciling test ports")
                    .unwrap();
                reconcile_forwarding(&paths, "dev")
            })
        };
        assert_eq!(added.recv_timeout(Duration::from_secs(5)).unwrap(), 3000);
        // Another computer's read or save needs only the short data lock.
        let started = std::time::Instant::now();
        drop(network_lock());
        let waited = started.elapsed();
        let failures = repair.join().unwrap().unwrap();
        runtime.join().unwrap();
        assert!(failures.is_empty(), "{failures:?}");
        assert!(
            waited < Duration::from_millis(400),
            "waited {waited:?} behind runtime calls"
        );
    }

    #[test]
    #[cfg(unix)]
    fn tombstone_cleanup_keeps_settings_saved_during_a_repair() {
        let _test_state = crate::test_support::global_state();
        let temp = tempfile::tempdir_in(crate::test_support::live::temp_root()).unwrap();
        let paths = temp_paths(&temp);
        let mut config = one_port("dev", 3000, true);
        // A removed port whose forward is already gone is pruned by the repair.
        config.mappings.push(Mapping {
            port: 3001,
            enabled: false,
            ..config.mappings[0].clone()
        });
        write_config(&paths, &config).unwrap();
        let (added_tx, added) = std::sync::mpsc::channel();
        let runtime = slow_runtime(
            socket_path(&paths, "dev"),
            2,
            Duration::from_millis(300),
            added_tx,
        );
        let repair = {
            let paths = paths.clone();
            std::thread::spawn(move || {
                let gate = runtime::operation_gate::OperationGate::new();
                let _guard = gate
                    .computer("test-id", "dev", "Reconciling test ports")
                    .unwrap();
                reconcile_forwarding(&paths, "dev")
            })
        };
        assert_eq!(added.recv_timeout(Duration::from_secs(5)).unwrap(), 3000);
        // Another computer saves a port while this computer's repair waits on its runtime.
        {
            let _guard = network_lock();
            let mut current = read_config(&paths).unwrap();
            current
                .mappings
                .extend(one_port("other", 8080, true).mappings);
            write_config(&paths, &current).unwrap();
        }
        assert!(repair.join().unwrap().unwrap().is_empty());
        runtime.join().unwrap();
        let saved: Vec<_> = read_config(&paths)
            .unwrap()
            .mappings
            .into_iter()
            .map(|m| (m.computer, m.port))
            .collect();
        assert_eq!(
            saved,
            vec![("dev".to_string(), 3000), ("other".to_string(), 8080)]
        );
    }

    #[test]
    #[cfg(unix)]
    fn pending_removal_is_selected_for_background_repair() {
        use std::os::unix::net::UnixListener;
        let _test_state = crate::test_support::global_state();
        let config = one_port("dev", 3000, false);
        let computers = reconcile_computers(&config);
        assert_eq!(computers, BTreeSet::from(["dev".into()]));
        let temp = tempfile::tempdir_in(crate::test_support::live::temp_root()).unwrap();
        let paths = temp_paths(&temp);
        write_config(&paths, &config).unwrap();
        let socket = socket_path(&paths, "dev");
        fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(socket).unwrap();
        let server = std::thread::spawn(move || {
            for op in ["ports_list", "port_remove"] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                let request: Value = serde_json::from_str(&request).unwrap();
                assert_eq!(request["op"], op);
                let ports = if op == "ports_list" {
                    json!([{"guest_port":3000,"host_port":43000,"host_bind":"127.0.0.1"}])
                } else {
                    assert_eq!(request["guest_port"], 3000);
                    json!([])
                };
                stream
                    .write_all(format!("{}\n", json!({"ok":true,"ports":ports})).as_bytes())
                    .unwrap();
            }
        });
        let gate = runtime::operation_gate::OperationGate::new();
        for computer in computers {
            let _guard = gate
                .computer("test-id", &computer, "Retrying removed port")
                .unwrap();
            assert!(reconcile_forwarding(&paths, &computer).unwrap().is_empty());
        }
        server.join().unwrap();
        assert!(read_config(&paths).unwrap().mappings.is_empty());
    }

    #[cfg(unix)]
    fn running_paths(temp: &tempfile::TempDir) -> RuntimePaths {
        let paths = temp_paths(temp);
        fs::write(&paths.library, "fixture").unwrap();
        fs::write(
            &paths.metadata,
            json!({"schemaVersion":1,"computers":[{
                "id":"00000000-0000-4000-8000-000000000001","name":"dev",
                "cpus":2,"maxCPUs":2,"memoryGiB":2,"maxMemoryGiB":2,
                "workspaceStorageGiB":10,"runtimeStorageGiB":10
            }]})
            .to_string(),
        )
        .unwrap();
        crate::test_support::write_shell_script(
            &paths.executable,
            r#"
case "$1" in
    inspect) echo '{"name":"dev","status":"Running","config":{"labels":{"silo.managed":"true"}}}' ;;
    exec) printf 'sl local_address rem_address st\n0: 00000000:0BB8 00000000:0000 0A\n' ;;
    *) exit 1 ;;
esac
"#,
        );
        paths
    }

    #[test]
    #[cfg(unix)]
    fn pending_removal_stays_visible_on_an_ordinary_refresh() {
        use std::os::unix::net::UnixListener;
        let _test_state = crate::test_support::global_state();
        let temp = tempfile::tempdir_in(crate::test_support::live::temp_root()).unwrap();
        let paths = running_paths(&temp);
        let config = one_port("dev", 3000, false);
        write_config(&paths, &config).unwrap();
        let socket = socket_path(&paths, "dev");
        fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(socket).unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                assert_eq!(
                    serde_json::from_str::<Value>(&request).unwrap()["op"],
                    "ports_list"
                );
                let reply = json!({"ok":true,"ports":[{
                    "guest_port":3000,"host_port":43000,"host_bind":"127.0.0.1"
                }]});
                stream.write_all(format!("{reply}\n").as_bytes()).unwrap();
            }
        });
        let state = observe(&paths, "dev", &config, &BTreeMap::new());
        server.join().unwrap();
        assert!(state.error.is_none(), "{:?}", state.error);
        assert_eq!(state.ports.len(), 1);
        let port = &state.ports[0];
        assert!(
            port.configured,
            "active removal must keep its retry control"
        );
        assert_eq!(port.state, "unknown");
        assert_eq!(port.host_port, Some(43000));
        assert!(port
            .message
            .as_deref()
            .unwrap()
            .contains("Access could not be removed"));
    }

    #[test]
    fn loopback_ipv6_is_not_reported_as_ipv4_reachable() {
        let _test_state = crate::test_support::global_state();
        let input="sl local_address rem_address st\n0: 00000000000000000000000001000000:0BB8 00000000:0000 0A\n";
        assert_eq!(parse_listeners(input).unwrap().get(&3000), Some(&false));
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn network_state_matches_wire_contract() {
        let mapping = Mapping {
            computer: "dev".into(),
            port: 3000,
            host_port: Some(13000),
            scheme: Some("http".into()),
            enabled: true,
        };
        let states = vec![
            State {
                computers: vec![Computer {
                    computer: "dev".into(),
                    host: Some(computer_host("dev", "00000000-0000-4000-8000-000000000001")),
                    error: None,
                    ports: vec![
                        Port {
                            configured_host_port: None,
                            port: 443,
                            host_port: Some(14443),
                            scheme: Some("https".into()),
                            state: "reachable",
                            configured: true,
                            message: None,
                        },
                        pending(&mapping, None, "waiting"),
                        Port {
                            configured_host_port: None,
                            port: 8080,
                            host_port: None,
                            scheme: None,
                            state: "unpublished",
                            configured: false,
                            message: None,
                        },
                    ],
                }],
            },
            State {
                computers: vec![Computer {
                    computer: "dev".into(),
                    host: None,
                    error: Some("Network state could not be read.".into()),
                    ports: vec![pending(
                        &mapping,
                        Some("Port state could not be read.".into()),
                        "unknown",
                    )],
                }],
            },
        ];
        crate::runtime::contract_tests::assert_fixture("network-state.json", states);
    }
}
