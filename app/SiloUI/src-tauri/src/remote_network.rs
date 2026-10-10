//! Each controller owns its loopback tunnels. A remote host's localhost address
//! is never presented as an address on this device.
//!
//! A tunnel the user opened is kept as an intent (its local port and scheme) while the
//! device stays saved. When the tunnel dies (sleep, a dropped connection) or the other
//! device publishes the port on a new endpoint, it is opened again in the background on
//! the same local port. Failed polls close live tunnels only when the device failed
//! repeatedly or no longer admits this one (`remote::close_after_failed_poll`).
use crate::bridge_error::{BridgeError, ErrorCode};
use crate::{remote, runtime};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    net::TcpListener,
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};

/// Saved device id, computer id, guest port.
type Key = (String, String, u16);

struct Tunnel {
    child: crate::owned_tunnel::Tunnel,
    local_port: u16,
    // Owner publication endpoint is retained to detect replacement across computer restarts.
    remote_port: u16,
    _directory: Option<tempfile::TempDir>,
}
impl std::fmt::Debug for Tunnel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Tunnel")
            .field("local_port", &self.local_port)
            .field("remote_port", &self.remote_port)
            .finish()
    }
}
impl Tunnel {
    fn alive(&mut self) -> bool {
        self.child.running()
    }
}
/// A port the user opened from this device, kept while its tunnel is down.
#[derive(Clone, Debug, PartialEq)]
struct Intent {
    local_port: u16,
    scheme: Option<String>,
}
#[derive(Debug)]
struct Reconnect {
    key: Key,
    intent: Intent,
    remote_port: u16,
}
struct Observation {
    revision: Option<Arc<()>>,
    token: Arc<()>,
}
#[derive(Default)]
struct Tunnels {
    live: HashMap<Key, Tunnel>,
    intents: HashMap<Key, Intent>,
    /// Each worker owns one attempt; replacement invalidates the previous worker.
    connecting: HashMap<Key, Arc<Reconnect>>,
    saves: HashMap<Key, Arc<()>>,
    revisions: HashMap<String, Arc<()>>,
    observations: HashMap<String, Arc<()>>,
}
impl Tunnels {
    fn observe(&mut self, device: &str) -> Observation {
        let observation = Observation {
            revision: self.revision(device),
            token: Arc::new(()),
        };
        self.observations
            .insert(device.into(), observation.token.clone());
        observation
    }
    fn revision(&self, device: &str) -> Option<Arc<()>> {
        self.revisions.get(device).cloned()
    }
    fn changed(&mut self, device: &str) {
        self.revisions.insert(device.into(), Arc::new(()));
    }
    fn has_connection(&self, key: &Key) -> bool {
        self.live.contains_key(key)
            || self.intents.contains_key(key)
            || self.saves.contains_key(key)
            || self.connecting.contains_key(key)
    }
    fn connection_count(&self) -> usize {
        self.live
            .keys()
            .chain(self.intents.keys())
            .chain(self.saves.keys())
            .chain(self.connecting.keys())
            .collect::<HashSet<_>>()
            .len()
    }
}
static TUNNELS: OnceLock<Mutex<Tunnels>> = OnceLock::new();
/// A short data lock: never held while ssh starts or a port is probed.
fn tunnels() -> MutexGuard<'static, Tunnels> {
    crate::sync::lock_or_recover(
        TUNNELS.get_or_init(|| Mutex::new(Tunnels::default())),
        "remote tunnels",
    )
}
/// A save remains current only until another save or removal supersedes it.
struct PendingSave {
    key: Key,
    token: Arc<()>,
}
impl PendingSave {
    fn new(key: Key) -> Result<Self, String> {
        runtime::shutdown::ensure_accepting_operations()?;
        let token = Arc::new(());
        let mut tunnels = tunnels();
        if !tunnels.has_connection(&key) && tunnels.connection_count() >= TUNNEL_LIMIT {
            return Err("Close an unused connection before opening another port.".into());
        }
        tunnels.changed(&key.0);
        tunnels.connecting.remove(&key);
        tunnels.saves.insert(key.clone(), token.clone());
        Ok(Self { key, token })
    }
    fn current(&self, tunnels: &Tunnels) -> bool {
        tunnels
            .saves
            .get(&self.key)
            .is_some_and(|token| Arc::ptr_eq(token, &self.token))
    }
}
impl Drop for PendingSave {
    fn drop(&mut self) {
        let mut tunnels = tunnels();
        if self.current(&tunnels) {
            tunnels.saves.remove(&self.key);
        }
    }
}

pub(crate) const TUNNEL_LIMIT: usize = 128;
/// How long a new tunnel may take to confirm forwarding.
const READY_WITHIN: Duration = Duration::from_secs(12);
/// How long the owner reuses its network state for polling controllers.
const DEVICE_STATE_MAX_AGE: Duration = Duration::from_secs(2);

/// Network state of this device for a controller, with stable computer ids. Several
/// controllers polling at once share one read (see `cached`).
pub(crate) fn device_state(app: &AppHandle) -> Result<Value, String> {
    cached(&DEVICE_STATE, DEVICE_STATE_MAX_AGE, || {
        read_device_state(app)
    })
}
/// A fresh read after a change on this device; later polls reuse it.
pub(crate) fn fresh_device_state(app: &AppHandle) -> Result<Value, String> {
    cached(&DEVICE_STATE, Duration::ZERO, || read_device_state(app))
}
static DEVICE_STATE: Mutex<Option<(Instant, Value)>> = Mutex::new(None);
/// Returns the stored value while younger than `max_age`, else computes and stores it. The
/// lock is held while computing, so concurrent callers wait for one read instead of each
/// inspecting every computer. Errors are not stored.
fn cached(
    cache: &Mutex<Option<(Instant, Value)>>,
    max_age: Duration,
    compute: impl FnOnce() -> Result<Value, String>,
) -> Result<Value, String> {
    let mut cache = crate::sync::lock_or_recover(cache, "remote network state");
    if let Some((at, value)) = cache.as_ref() {
        if at.elapsed() < max_age {
            return Ok(value.clone());
        }
    }
    let value = compute()?;
    *cache = Some((Instant::now(), value.clone()));
    Ok(value)
}
fn read_device_computer_ids(metadata: &Path) -> Result<HashMap<String, String>, String> {
    let config = runtime::read_metadata(metadata).map_err(|e| e.to_string())?;
    Ok(config
        .computers
        .into_iter()
        .map(|m| (m.name().to_owned(), m.id().to_owned()))
        .collect())
}

fn read_device_state(app: &AppHandle) -> Result<Value, String> {
    let paths = runtime::runtime_paths(app)?;
    let before = read_device_computer_ids(&paths.metadata)?;
    let state = tauri::async_runtime::block_on(crate::network::read_network_state(app.clone()))?;
    let value = serde_json::to_value(state).map_err(|e| e.to_string())?;
    let after = read_device_computer_ids(&paths.metadata)?;
    project_host_ports(value, &before, &after)
}

fn project_host_ports(
    mut value: Value,
    before: &HashMap<String, String>,
    after: &HashMap<String, String>,
) -> Result<Value, String> {
    for row in value["computers"]
        .as_array_mut()
        .ok_or("Invalid network state.")?
    {
        let name = row["computer"].as_str().unwrap_or("");
        let id = after
            .get(name)
            .ok_or("Computer configuration changed. Refresh network services.")?;
        if before.get(name) != Some(id) {
            return Err("Computer configuration changed. Refresh network services.".into());
        }
        row["computerId"] = json!(id);
    }
    Ok(value)
}

fn read(app: &AppHandle, device: &str) -> Result<Value, BridgeError> {
    // A device that just failed repeatedly is answered at once; its snapshot poll
    // (or this read, shortly) tries again.
    if let Some(error) = remote::offline(device) {
        return Err(error.into());
    }
    let observation = tunnels().observe(device);
    let value = match remote::call_remote_typed(app, device, "network.state", json!({})) {
        Ok(value) => {
            remote::poll_succeeded(device);
            value
        }
        Err(error) => {
            if error.code != ErrorCode::UnsupportedRemoteOperation {
                remote::close_after_failed_poll(device, &error);
            }
            return Err(error);
        }
    };
    let projection = project_observed(value, device, observation, &mut tunnels())?;
    let Projection {
        value,
        closed,
        reconnect,
    } = projection;
    drop(closed);
    for request in reconnect {
        reconnect_in_background(app.clone(), request);
    }
    Ok(value)
}

struct Projection {
    value: Value,
    /// Tunnels to stop, dropped after the lock is released.
    closed: Vec<Tunnel>,
    /// Intended tunnels to open again on the owner's current endpoint.
    reconnect: Vec<Arc<Reconnect>>,
}
fn project_observed(
    value: Value,
    device: &str,
    observation: Observation,
    tunnels: &mut Tunnels,
) -> Result<Projection, String> {
    let latest = tunnels
        .observations
        .get(device)
        .is_some_and(|token| Arc::ptr_eq(token, &observation.token));
    if !latest {
        return Err(
            "A newer network refresh superseded this request. Refresh network services.".into(),
        );
    }
    let current = match (observation.revision.as_ref(), tunnels.revisions.get(device)) {
        (None, None) => true,
        (Some(before), Some(now)) => Arc::ptr_eq(before, now),
        _ => false,
    };
    if !current || tunnels.saves.keys().any(|key| key.0 == device) {
        return Err("Network settings changed while refreshing. Refresh network services.".into());
    }
    project_ports(value, device, tunnels)
}

fn validate_remote_ports(value: &Value) -> Result<(), String> {
    for row in value["computers"]
        .as_array()
        .ok_or("Invalid remote network state.")?
    {
        row["computerId"]
            .as_str()
            .ok_or("Missing remote computer identity.")?;
        for port in row["ports"]
            .as_array()
            .ok_or("Invalid remote port state.")?
        {
            port["port"]
                .as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .ok_or("Invalid remote port.")?;
        }
    }
    Ok(())
}

/// Rewrites the owner's rows for this device: rows become remote targets, ports show this
/// device's tunnel (never the owner's loopback endpoint). Each computer gets the host
/// name its websites open at here (C-24; the owner's own host choice is ignored).
/// Dead or outdated tunnels with an intent are reopened.
fn project_ports(
    mut value: Value,
    device: &str,
    tunnels: &mut Tunnels,
) -> Result<Projection, String> {
    // Reject the whole response before changing tunnel ownership or scheduling workers.
    validate_remote_ports(&value)?;
    let mut observed = HashSet::new();
    let mut failed_computers = HashSet::new();
    let mut closed = Vec::new();
    let mut reconnect = Vec::new();
    for row in value["computers"]
        .as_array_mut()
        .ok_or("Invalid remote network state.")?
    {
        let computer = row["computerId"]
            .as_str()
            .ok_or("Missing remote computer identity.")?
            .to_owned();
        if row["error"].as_str().is_some() {
            failed_computers.insert(computer.clone());
        }
        let name = row["computer"].as_str().unwrap_or("").to_owned();
        row["host"] = json!(crate::network::computer_host(&name, &computer));
        row["computer"] = json!(format!("silo-remote:{device}:{computer}"));
        for port in row["ports"]
            .as_array_mut()
            .ok_or("Invalid remote port state.")?
        {
            let guest = port["port"]
                .as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .ok_or("Invalid remote port.")?;
            let key: Key = (device.into(), computer.clone(), guest);
            observed.insert(key.clone());
            let endpoint = port["hostPort"]
                .as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .filter(|n| *n != 0);
            // The owner removed the publication (the guest service may still listen). A row
            // that failed to observe says nothing about publication, so it keeps its tunnel.
            let removed = !failed_computers.contains(&computer)
                && port["configured"] == json!(false)
                && port["state"] == json!("unpublished")
                && endpoint.is_none();
            if removed {
                closed.extend(tunnels.live.remove(&key));
                tunnels.intents.remove(&key);
                tunnels.connecting.remove(&key);
            }
            // A stopped or restarting computer has no endpoint yet; its tunnel waits for it.
            let current = tunnels.live.get_mut(&key).is_some_and(|tunnel| {
                tunnel.alive() && endpoint.is_none_or(|endpoint| endpoint == tunnel.remote_port)
            });
            if !current {
                closed.extend(tunnels.live.remove(&key));
            }
            let intent = tunnels.intents.get(&key).cloned();
            match (tunnels.live.get(&key), intent) {
                (Some(tunnel), intent) => {
                    port["configuredHostPort"] = json!(tunnel.local_port);
                    port["configured"] = json!(true);
                    port["scheme"] = json!(intent.and_then(|intent| intent.scheme));
                    port["hostPort"] = if endpoint.is_some() {
                        json!(tunnel.local_port)
                    } else {
                        Value::Null
                    };
                }
                (None, Some(intent)) => {
                    port["configuredHostPort"] = json!(intent.local_port);
                    port["configured"] = json!(true);
                    port["scheme"] = json!(intent.scheme);
                    port["hostPort"] = Value::Null;
                    if let Some(endpoint) = endpoint {
                        port["state"] = json!("waiting");
                        port["message"] = json!("Reconnecting to the other device…");
                        let unchanged = tunnels.connecting.get(&key).is_some_and(|request| {
                            request.remote_port == endpoint && request.intent == intent
                        });
                        if !unchanged {
                            let request = Arc::new(Reconnect {
                                key: key.clone(),
                                intent,
                                remote_port: endpoint,
                            });
                            tunnels.connecting.insert(key, request.clone());
                            reconnect.push(request);
                        }
                    } else {
                        tunnels.connecting.remove(&key);
                    }
                }
                (None, None) => {
                    port["hostPort"] = Value::Null;
                    port["configuredHostPort"] = Value::Null;
                    port["configured"] = json!(false);
                    port["state"] = json!("unpublished");
                }
            }
        }
    }
    // Missing ports imply deletion only when their computer was observed successfully.
    let gone: Vec<Key> = tunnels
        .live
        .keys()
        .chain(tunnels.intents.keys())
        .filter(|key| {
            key.0 == device && !observed.contains(*key) && !failed_computers.contains(&key.1)
        })
        .cloned()
        .collect();
    for key in gone {
        closed.extend(tunnels.live.remove(&key));
        tunnels.intents.remove(&key);
        tunnels.connecting.remove(&key);
    }
    Ok(Projection {
        value,
        closed,
        reconnect,
    })
}

/// Starts ssh forwarding `local` (or any free port) to the owner's `remote_port` and waits
/// for its private OpenSSH master to acknowledge readiness. Runs without the tunnels lock.
fn open_tunnel(
    commands: impl FnOnce(u16, &Path) -> Result<(Command, Command), String>,
    local: Option<u16>,
    remote_port: u16,
    ready_within: Duration,
) -> Result<Tunnel, String> {
    let reservation = TcpListener::bind(("127.0.0.1", local.unwrap_or(0)))
        .map_err(|_| "This local port is already in use.")?;
    let local = reservation.local_addr().map_err(|e| e.to_string())?.port();
    // Keep the control path short enough for macOS Unix sockets and private to this tunnel.
    let directory = tempfile::Builder::new()
        .prefix("silo-tunnel-")
        .tempdir_in("/tmp")
        .map_err(|_| "Could not prepare the SSH tunnel.")?;
    let socket = directory.path().join("ssh.sock");
    let (command, mut check) = commands(local, &socket)?;
    drop(reservation);
    let child = crate::owned_tunnel::Tunnel::spawn(&command, None)
        .map_err(|_| "Could not open the SSH tunnel.")?;
    let mut tunnel = Tunnel {
        child,
        local_port: local,
        remote_port,
        _directory: Some(directory),
    };
    let until = Instant::now() + ready_within;
    loop {
        if !tunnel.alive() {
            return Err(
                "SSH could not open this port. Check access and local port availability.".into(),
            );
        }
        // OpenSSH serves control requests after local forwarding setup. With
        // ExitOnForwardFailure=yes, a bind failure never reaches this acknowledgement.
        if socket.exists() && control_ready(&mut check, &mut tunnel, until)? {
            return Ok(tunnel);
        }
        if Instant::now() >= until {
            return Err("Timed out opening the SSH tunnel.".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn control_ready(
    command: &mut Command,
    tunnel: &mut Tunnel,
    until: Instant,
) -> Result<bool, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Could not check the SSH tunnel.")?;
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status.success() && tunnel.alive()),
            Ok(None) => (),
            Err(_) => break Err("Could not check the SSH tunnel.".into()),
        }
        if Instant::now() >= until {
            break Err("Timed out opening the SSH tunnel.".into());
        }
        if !tunnel.alive() {
            break Ok(false);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn open_guest_tunnel(
    app: &AppHandle,
    key: &Key,
    local: Option<u16>,
    owner_endpoint: u16,
) -> Result<Tunnel, String> {
    let (alias, config, address) =
        crate::editor::prepare_remote_network_private(app, &key.0, &key.1, key.2)?;
    open_tunnel(
        |local, socket| {
            remote::guest_tunnel_commands(&config, &alias, local, address, key.2, socket)
        },
        local,
        owner_endpoint,
        READY_WITHIN,
    )
}

fn reconnect_in_background(app: AppHandle, request: Arc<Reconnect>) {
    std::thread::spawn(move || {
        let opened = runtime::shutdown::ensure_accepting_operations().and_then(|()| {
            open_guest_tunnel(
                &app,
                &request.key,
                Some(request.intent.local_port),
                request.remote_port,
            )
        });
        let unused = finish_reconnect(&request, opened);
        drop(unused);
        let _ = app.emit("silo://network-state-changed", ());
    });
}

fn finish_reconnect(request: &Arc<Reconnect>, opened: Result<Tunnel, String>) -> Option<Tunnel> {
    let mut tunnels = tunnels();
    let current = tunnels
        .connecting
        .get(&request.key)
        .is_some_and(|pending| Arc::ptr_eq(pending, request));
    if !current {
        return opened.ok();
    }
    tunnels.connecting.remove(&request.key);
    match opened {
        Ok(tunnel)
            if runtime::shutdown::ensure_accepting_operations().is_ok()
                && tunnels.intents.get(&request.key) == Some(&request.intent)
                && !tunnels.live.contains_key(&request.key) =>
        {
            tunnels.live.insert(request.key.clone(), tunnel);
            None
        }
        Ok(tunnel) => Some(tunnel),
        Err(_) => None,
    }
}

/// Opens (or replaces) the tunnel for `key` and records the intent. The lock is taken only
/// around map updates. A different local port is opened before the previous tunnel closes,
/// so a failed replacement keeps the working one; reusing the same local port needs the
/// previous tunnel closed first.
#[cfg(test)]
fn save_tunnel(
    key: Key,
    host_port: Option<u16>,
    scheme: Option<String>,
    endpoint: u16,
    open: impl FnOnce(Option<u16>) -> Result<Tunnel, String>,
) -> Result<(), String> {
    finish_save(PendingSave::new(key)?, host_port, scheme, endpoint, open)
}

fn finish_save(
    pending: PendingSave,
    host_port: Option<u16>,
    scheme: Option<String>,
    endpoint: u16,
    open: impl FnOnce(Option<u16>) -> Result<Tunnel, String>,
) -> Result<(), String> {
    let key = pending.key.clone();
    let (requested, blocking) = {
        let mut tunnels = tunnels();
        runtime::shutdown::ensure_accepting_operations()?;
        if !pending.current(&tunnels) {
            return Err("This port configuration was superseded. Refresh network services.".into());
        }
        // Automatic keeps the port this device used before, so bookmarks keep working.
        let requested =
            host_port.or_else(|| tunnels.intents.get(&key).map(|intent| intent.local_port));
        let unchanged = tunnels.live.get_mut(&key).is_some_and(|tunnel| {
            tunnel.alive()
                && tunnel.remote_port == endpoint
                && requested.is_none_or(|port| port == tunnel.local_port)
        });
        if unchanged {
            let local_port = tunnels.live[&key].local_port;
            tunnels.intents.insert(key, Intent { local_port, scheme });
            tunnels.changed(&pending.key.0);
            return Ok(());
        }
        let occupies = tunnels
            .live
            .get(&key)
            .is_some_and(|tunnel| Some(tunnel.local_port) == requested);
        (
            requested,
            if occupies {
                tunnels.live.remove(&key)
            } else {
                None
            },
        )
    };
    drop(blocking);
    let tunnel = open(requested)?;
    let replaced = {
        let mut tunnels = tunnels();
        if runtime::shutdown::ensure_accepting_operations().is_err() {
            return Err("Silo is quitting.".into());
        }
        if !pending.current(&tunnels) {
            return Err("This port configuration was superseded. Refresh network services.".into());
        }
        tunnels.intents.insert(
            key.clone(),
            Intent {
                local_port: tunnel.local_port,
                scheme,
            },
        );
        tunnels.connecting.remove(&key);
        tunnels.changed(&pending.key.0);
        tunnels.live.insert(key, tunnel)
    };
    drop(replaced);
    Ok(())
}

#[tauri::command]
pub async fn remote_network_state(app: AppHandle, device_id: String) -> Result<Value, BridgeError> {
    tauri::async_runtime::spawn_blocking(move || read(&app, &device_id))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn remote_save_network_port(
    app: AppHandle,
    device_id: String,
    computer_id: String,
    port: u16,
    host_port: Option<u16>,
    scheme: Option<String>,
) -> Result<Value, BridgeError> {
    tauri::async_runtime::spawn_blocking(move || {
        if port == 0
            || host_port == Some(0)
            || !matches!(scheme.as_deref(), None | Some("http" | "https"))
        {
            return Err("Invalid port configuration.".into());
        }
        let pending = PendingSave::new((device_id.clone(), computer_id.clone(), port))?;
        let state = remote::call_remote_typed(
            &app,
            &device_id,
            "network.publish",
            json!({"computerId":computer_id,"port":port,"scheme":scheme}),
        )?;
        let endpoint = state["computers"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["computerId"] == computer_id))
            .and_then(|r| r["ports"].as_array())
            .and_then(|ports| ports.iter().find(|p| p["port"] == port))
            .and_then(|p| p["hostPort"].as_u64())
            .and_then(|p| u16::try_from(p).ok())
            .ok_or("The remote computer port is not available yet. Check the computer service and retry.")?;
        let key = (device_id.clone(), computer_id, port);
        finish_save(pending, host_port, scheme, endpoint, |local| {
            open_guest_tunnel(&app, &key, local, endpoint)
        })?;
        read(&app, &device_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
fn forget_port(key: &Key) {
    let closed = {
        let mut tunnels = tunnels();
        tunnels.changed(&key.0);
        tunnels.saves.remove(key);
        tunnels.connecting.remove(key);
        tunnels.intents.remove(key);
        tunnels.live.remove(key)
    };
    drop(closed);
}

/// Stops publishing the port on the owning device, then closes this device's tunnel.
#[tauri::command]
pub async fn remote_remove_network_port(
    app: AppHandle,
    device_id: String,
    computer_id: String,
    port: u16,
) -> Result<Value, BridgeError> {
    tauri::async_runtime::spawn_blocking(move || {
        remote::call_remote_typed(
            &app,
            &device_id,
            "network.unpublish",
            json!({"computerId":computer_id,"port":port}),
        )?;
        forget_port(&(device_id.clone(), computer_id, port));
        read(&app, &device_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn remote_open_network_port(
    app: AppHandle,
    device_id: String,
    computer_id: String,
    port: u16,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = read(&app, &device_id).map_err(|error| error.message)?;
        let target = format!("silo-remote:{device_id}:{computer_id}");
        let endpoint = state["computers"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["computer"] == target))
            .and_then(|r| r["ports"].as_array())
            .and_then(|ports| {
                ports
                    .iter()
                    .find(|p| p["port"] == port && p["state"] == "reachable")
            })
            .ok_or("This service is not reachable.")?;
        let scheme = endpoint["scheme"]
            .as_str()
            .filter(|s| matches!(*s, "http" | "https"))
            .ok_or("This port is not a website.")?;
        let local = endpoint["hostPort"]
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .ok_or("This service is not connected.")?;
        let url = crate::network::website_url(scheme, row_host(&state, &target), local);
        crate::applications::open_browser(&app, &url)
    })
    .await
    .map_err(|e| e.to_string())?
}
fn row_host<'a>(state: &'a Value, target: &str) -> Option<&'a str> {
    state["computers"]
        .as_array()?
        .iter()
        .find(|row| row["computer"] == target)?["host"]
        .as_str()
}
/// Closes every tunnel and shared SSH connection (quit).
pub(crate) fn close_all() {
    crate::remote::multiplex::close_all();
    let closed: Vec<Tunnel> = {
        let mut tunnels = tunnels();
        tunnels.saves.clear();
        tunnels.connecting.clear();
        tunnels.observations.clear();
        tunnels.live.drain().map(|(_, tunnel)| tunnel).collect()
    };
    drop(closed);
}
/// Closes a device's live tunnels but keeps their intents, so they reopen on the same
/// local ports once it answers again.
pub(crate) fn disconnect_device(device: &str) {
    let closed: Vec<Tunnel> = {
        let mut tunnels = tunnels();
        tunnels.connecting.retain(|key, _| key.0 != device);
        tunnels.observations.remove(device);
        let keys: Vec<Key> = tunnels
            .live
            .keys()
            .filter(|key| key.0 == device)
            .cloned()
            .collect();
        keys.iter()
            .filter_map(|key| tunnels.live.remove(key))
            .collect()
    };
    drop(closed);
}
/// Forgets a device's tunnels and intents (removed, or no longer the same device).
pub(crate) fn close_device(device: &str) {
    let closed: Vec<Tunnel> = {
        let mut tunnels = tunnels();
        tunnels.revisions.remove(device);
        tunnels.observations.remove(device);
        tunnels.saves.retain(|key, _| key.0 != device);
        tunnels.intents.retain(|key, _| key.0 != device);
        tunnels.connecting.retain(|key, _| key.0 != device);
        tunnels.observations.remove(device);
        let keys: Vec<Key> = tunnels
            .live
            .keys()
            .filter(|key| key.0 == device)
            .cloned()
            .collect();
        keys.iter()
            .filter_map(|key| tunnels.live.remove(key))
            .collect()
    };
    drop(closed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Child;
    fn observed(host_port: Option<u16>) -> Value {
        json!({"computers":[{"computer":"dev","computerId":"computer","ports":[{"port":3000,"hostPort":host_port,"configured":true,"state":"reachable","scheme":"http"}]}]})
    }
    fn child() -> crate::owned_tunnel::Tunnel {
        crate::owned_tunnel::Tunnel::spawn(Command::new("/bin/sleep").arg("30"), None).unwrap()
    }
    fn tunnel(local_port: u16, remote_port: u16) -> Tunnel {
        Tunnel {
            child: child(),
            local_port,
            remote_port,
            _directory: None,
        }
    }
    fn intent(local_port: u16) -> Intent {
        Intent {
            local_port,
            scheme: Some("http".into()),
        }
    }
    fn key(device: &str) -> Key {
        (device.into(), "computer".into(), 3000)
    }
    fn port(result: &Projection) -> &Value {
        &result.value["computers"][0]["ports"][0]
    }

    fn reconnect_targets(projection: &Projection) -> Vec<(Key, Intent, u16)> {
        projection
            .reconnect
            .iter()
            .map(|request| {
                (
                    request.key.clone(),
                    request.intent.clone(),
                    request.remote_port,
                )
            })
            .collect()
    }

    #[test]
    fn owner_loopback_is_not_a_controller_endpoint_until_a_tunnel_exists() {
        let mut tunnels = Tunnels::default();
        let result = project_ports(observed(Some(32000)), "office", &mut tunnels).unwrap();
        assert_eq!(
            result.value["computers"][0]["computer"],
            "silo-remote:office:computer"
        );
        assert!(port(&result)["hostPort"].is_null());
        assert_eq!(port(&result)["configured"], false);
        tunnels.live.insert(key("office"), tunnel(43000, 32000));
        tunnels.intents.insert(key("office"), intent(43000));
        let result = project_ports(observed(Some(32000)), "office", &mut tunnels).unwrap();
        assert_eq!(port(&result)["hostPort"], 43000);
        assert_eq!(port(&result)["configured"], true);
        assert!(result.reconnect.is_empty() && result.closed.is_empty());
    }

    #[test]
    fn remote_computers_open_at_their_own_host_on_this_device() {
        let mut tunnels = Tunnels::default();
        let value = json!({"computers":[{"computer":"dev","computerId":"1a2b3c4d-0000-4000-8000-000000000001","host":"owner-choice.localhost","ports":[]}]});
        let result = project_ports(value.clone(), "office", &mut tunnels).unwrap();
        assert_eq!(
            result.value["computers"][0]["host"],
            "dev-1a2b3c4d.localhost"
        );
        assert_eq!(
            row_host(
                &result.value,
                "silo-remote:office:1a2b3c4d-0000-4000-8000-000000000001"
            ),
            Some("dev-1a2b3c4d.localhost")
        );
        // An owner without a named host still gets cookie isolation here.
        let mut value = value;
        value["computers"][0]["host"] = Value::Null;
        let result = project_ports(value, "office", &mut tunnels).unwrap();
        assert_eq!(
            result.value["computers"][0]["host"],
            "dev-1a2b3c4d.localhost"
        );
    }

    #[test]
    fn a_new_owner_endpoint_or_a_dead_tunnel_reopens_on_the_same_local_port() {
        let mut tunnels = Tunnels::default();
        tunnels.live.insert(key("office"), tunnel(43000, 32000));
        tunnels.intents.insert(key("office"), intent(43000));
        // The computer restarted and the owner published the port on a new endpoint.
        let result = project_ports(observed(Some(32001)), "office", &mut tunnels).unwrap();
        assert_eq!(result.closed.len(), 1);
        assert_eq!(
            reconnect_targets(&result),
            [(key("office"), intent(43000), 32001)]
        );
        assert_eq!(port(&result)["configuredHostPort"], 43000);
        assert_eq!(port(&result)["state"], "waiting");
        // A reconnect already under way is not started twice.
        assert!(project_ports(observed(Some(32001)), "office", &mut tunnels)
            .unwrap()
            .reconnect
            .is_empty());
        tunnels.connecting.clear();
        // A tunnel whose forward exited is reopened too.
        let mut dead = tunnel(43000, 32001);
        dead.child =
            crate::owned_tunnel::Tunnel::spawn(&Command::new("/usr/bin/false"), None).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while dead.alive() {
            assert!(Instant::now() < until, "closed tunnel never exited");
            std::thread::sleep(Duration::from_millis(10));
        }
        tunnels.live.insert(key("office"), dead);
        let result = project_ports(observed(Some(32001)), "office", &mut tunnels).unwrap();
        assert_eq!(
            reconnect_targets(&result),
            [(key("office"), intent(43000), 32001)]
        );
    }

    #[test]
    fn a_restarting_computer_keeps_its_tunnel_and_intent() {
        let mut tunnels = Tunnels::default();
        tunnels.live.insert(key("office"), tunnel(43000, 32000));
        tunnels.intents.insert(key("office"), intent(43000));
        let result = project_ports(observed(None), "office", &mut tunnels).unwrap();
        assert!(result.closed.is_empty() && result.reconnect.is_empty());
        assert!(port(&result)["hostPort"].is_null());
        assert_eq!(port(&result)["configuredHostPort"], 43000);
        // Back on the same endpoint: usable again without reconnecting.
        let result = project_ports(observed(Some(32000)), "office", &mut tunnels).unwrap();
        assert_eq!(port(&result)["hostPort"], 43000);
        assert!(result.reconnect.is_empty());
    }

    #[test]
    fn an_explicitly_unpublished_service_closes_its_tunnel_and_forgets_its_intent() {
        let mut tunnels = Tunnels::default();
        tunnels.live.insert(key("office"), tunnel(43000, 32000));
        tunnels.intents.insert(key("office"), intent(43000));
        // The owner removed the publication while the guest service keeps listening.
        let mut value = observed(None);
        value["computers"][0]["ports"][0]["configured"] = json!(false);
        value["computers"][0]["ports"][0]["state"] = json!("unpublished");
        let result = project_ports(value, "office", &mut tunnels).unwrap();
        assert_eq!(result.closed.len(), 1);
        assert!(result.reconnect.is_empty());
        assert!(tunnels.live.is_empty() && tunnels.intents.is_empty());
        assert!(tunnels.connecting.is_empty());
        assert_eq!(port(&result)["configured"], false);
        assert_eq!(port(&result)["state"], "unpublished");
        assert!(port(&result)["hostPort"].is_null());
        // An observation error is not a removal.
        tunnels.live.insert(key("office"), tunnel(43000, 32000));
        tunnels.intents.insert(key("office"), intent(43000));
        let mut failed = observed(None);
        failed["computers"][0]["error"] = json!("unreachable");
        failed["computers"][0]["ports"][0]["configured"] = json!(false);
        failed["computers"][0]["ports"][0]["state"] = json!("unpublished");
        let result = project_ports(failed, "office", &mut tunnels).unwrap();
        assert!(result.closed.is_empty());
        assert!(
            tunnels.live.contains_key(&key("office"))
                && tunnels.intents.contains_key(&key("office"))
        );
    }

    #[test]
    fn a_deleted_port_or_computer_forgets_only_its_own_tunnels() {
        let mut tunnels = Tunnels::default();
        for device in ["office", "other"] {
            tunnels.live.insert(key(device), tunnel(43000, 32000));
            tunnels.intents.insert(key(device), intent(43000));
        }
        let result = project_ports(json!({"computers":[]}), "office", &mut tunnels).unwrap();
        assert_eq!(result.closed.len(), 1);
        assert!(
            !tunnels.live.contains_key(&key("office"))
                && !tunnels.intents.contains_key(&key("office"))
        );
        assert!(
            tunnels.live.contains_key(&key("other")) && tunnels.intents.contains_key(&key("other"))
        );
    }

    #[test]
    fn an_older_poll_cannot_replace_a_newer_reconnect_endpoint() {
        let mut state = Tunnels::default();
        state.intents.insert(key("office"), intent(43000));
        let first = state.observe("office");
        let second = state.observe("office");
        let latest = project_observed(observed(Some(32001)), "office", second, &mut state).unwrap();
        assert_eq!(
            reconnect_targets(&latest),
            [(key("office"), intent(43000), 32001)]
        );
        let older = project_observed(observed(Some(32000)), "office", first, &mut state);
        assert!(older.is_err(), "the old poll replaced the newer endpoint");
        assert_eq!(state.connecting[&key("office")].remote_port, 32001);
    }

    #[test]
    fn an_older_poll_cannot_delete_a_tunnel_verified_by_a_newer_poll() {
        let mut state = Tunnels::default();
        state.intents.insert(key("office"), intent(43000));
        state.live.insert(key("office"), tunnel(43000, 32001));
        let first = state.observe("office");
        let second = state.observe("office");
        project_observed(observed(Some(32001)), "office", second, &mut state).unwrap();
        let older = project_observed(json!({"computers":[]}), "office", first, &mut state);
        assert!(
            older.is_err(),
            "the old poll deleted the newer verified tunnel"
        );
        assert!(state.live.contains_key(&key("office")));
    }

    #[test]
    fn a_recreated_computer_cannot_relabel_an_older_network_snapshot() {
        let before = HashMap::from([("dev".into(), "original-computer".into())]);
        let after = HashMap::from([("dev".into(), "replacement-computer".into())]);
        let snapshot =
            json!({"computers":[{"computer":"dev","ports":[{"port":3000,"hostPort":32000}]}]});
        assert!(
            project_host_ports(snapshot, &before, &after).is_err(),
            "the old endpoint was assigned the replacement computer's identity"
        );
    }

    #[test]
    fn a_computer_created_during_observation_requires_a_fresh_identity_snapshot() {
        let before = HashMap::new();
        let after = HashMap::from([("dev".into(), "new-computer".into())]);
        let snapshot = json!({"computers":[{"computer":"dev","ports":[]}]});
        assert!(project_host_ports(snapshot, &before, &after).is_err());
    }

    #[test]
    fn stable_computer_identity_survives_unrelated_metadata_changes() {
        let before = HashMap::from([
            ("dev".into(), "stable-computer".into()),
            ("other".into(), "old-other".into()),
        ]);
        let after = HashMap::from([
            ("dev".into(), "stable-computer".into()),
            ("other".into(), "new-other".into()),
        ]);
        let snapshot = json!({"computers":[{"computer":"dev","ports":[]}]});
        let projected = project_host_ports(snapshot, &before, &after).unwrap();
        assert_eq!(projected["computers"][0]["computerId"], "stable-computer");
    }

    #[test]
    fn a_computer_observation_error_does_not_forget_its_connections() {
        let mut state = Tunnels::default();
        state.live.insert(key("office"), tunnel(43000, 32000));
        state.intents.insert(key("office"), intent(43000));
        let failed = json!({"computers":[{"computer":"dev","computerId":"computer","ports":[],"error":"Could not read network settings."}]});
        let result = project_ports(failed, "office", &mut state).unwrap();
        assert!(
            result.closed.is_empty(),
            "a partial observation closed a working tunnel"
        );
        assert!(state.intents.contains_key(&key("office")));
        let recovered = project_ports(observed(Some(32000)), "office", &mut state).unwrap();
        assert_eq!(port(&recovered)["hostPort"], 43000);
        assert!(recovered.reconnect.is_empty());
        let deleted = project_ports(
            json!({"computers":[{"computerId":"computer","ports":[],"error":null}]}),
            "office",
            &mut state,
        )
        .unwrap();
        assert_eq!(deleted.closed.len(), 1);
        assert!(!state.intents.contains_key(&key("office")));
    }

    #[test]
    fn an_observation_error_preserves_only_that_computers_intents() {
        let mut state = Tunnels::default();
        state.intents.insert(key("office"), intent(43000));
        let healthy_key = ("office".into(), "healthy".into(), 3000);
        state.intents.insert(healthy_key.clone(), intent(43001));
        let partial = json!({"computers":[
            {"computerId":"computer","ports":[],"error":"Read failed"},
            {"computerId":"healthy","ports":[],"error":null}
        ]});
        project_ports(partial, "office", &mut state).unwrap();
        assert!(
            state.intents.contains_key(&key("office")),
            "an uncertain row was treated as deletion"
        );
        assert!(!state.intents.contains_key(&healthy_key));
    }

    #[test]
    fn a_reconnect_cannot_restore_a_tunnel_after_close_all() {
        let _guard = crate::test_support::global_state();
        let device = uuid::Uuid::new_v4().to_string();
        tunnels().intents.insert(key(&device), intent(43000));
        let request = project_ports(observed(Some(32000)), &device, &mut tunnels())
            .unwrap()
            .reconnect
            .pop()
            .unwrap();
        close_all();
        let unused = finish_reconnect(&request, Ok(tunnel(43000, request.remote_port)));
        let restored = tunnels().live.contains_key(&key(&device));
        close_device(&device);
        assert!(
            unused.is_some(),
            "cleanup did not invalidate the pending reconnect"
        );
        assert!(!restored, "a reconnect restored a tunnel after cleanup");
    }

    #[test]
    fn an_older_reconnect_cannot_clear_a_newer_attempt() {
        let _guard = crate::test_support::global_state();
        let device = uuid::Uuid::new_v4().to_string();
        tunnels().intents.insert(key(&device), intent(43000));
        let old = project_ports(observed(Some(32000)), &device, &mut tunnels())
            .unwrap()
            .reconnect
            .pop()
            .unwrap();
        forget_port(&key(&device));
        tunnels().intents.insert(key(&device), intent(43000));
        let newer = project_ports(observed(Some(32001)), &device, &mut tunnels())
            .unwrap()
            .reconnect
            .pop()
            .unwrap();
        let unused = finish_reconnect(&old, Ok(tunnel(43000, old.remote_port)));
        let marked = tunnels().connecting.contains_key(&key(&device));
        let stale = tunnels().live.contains_key(&key(&device));
        let valid = finish_reconnect(&newer, Ok(tunnel(43000, newer.remote_port)));
        let endpoint = tunnels()
            .live
            .get(&key(&device))
            .map(|tunnel| tunnel.remote_port);
        close_device(&device);
        assert!(unused.is_some(), "an obsolete reconnect was committed");
        assert!(marked, "the older worker cleared a newer attempt's marker");
        assert!(!stale);
        assert!(valid.is_none());
        assert_eq!(endpoint, Some(32001));
    }

    #[test]
    fn an_endpoint_change_replaces_the_pending_reconnect() {
        let mut state = Tunnels::default();
        state.intents.insert(key("office"), intent(43000));
        let first = project_ports(observed(Some(32000)), "office", &mut state).unwrap();
        assert_eq!(first.reconnect.len(), 1);
        let second = project_ports(observed(Some(32001)), "office", &mut state).unwrap();
        assert_eq!(
            second.reconnect.len(),
            1,
            "a new endpoint waited for the obsolete attempt"
        );
    }

    #[test]
    fn an_invalid_snapshot_cannot_strand_a_reconnect() {
        let mut state = Tunnels::default();
        state.intents.insert(key("office"), intent(43000));
        let mut invalid = observed(Some(32000));
        invalid["computers"]
            .as_array_mut()
            .unwrap()
            .push(json!({"computer":"bad","ports":[]}));
        assert!(project_ports(invalid, "office", &mut state).is_err());
        assert!(
            !state.connecting.contains_key(&key("office")),
            "no worker was started for the rejected snapshot"
        );
        let next = project_ports(observed(Some(32000)), "office", &mut state).unwrap();
        assert_eq!(
            reconnect_targets(&next),
            [(key("office"), intent(43000), 32000)]
        );
    }

    #[test]
    fn an_invalid_snapshot_keeps_the_existing_tunnel() {
        let mut state = Tunnels::default();
        state.live.insert(key("office"), tunnel(43000, 32000));
        state.intents.insert(key("office"), intent(43000));
        let mut invalid = observed(Some(32001));
        invalid["computers"]
            .as_array_mut()
            .unwrap()
            .push(json!({"computerId":"bad","ports":[{"port":65536}]}));
        assert!(project_ports(invalid, "office", &mut state).is_err());
        assert!(
            state
                .live
                .get_mut(&key("office"))
                .is_some_and(Tunnel::alive),
            "a rejected snapshot closed the working tunnel"
        );
        assert!(state.connecting.is_empty());
    }

    #[test]
    fn concurrent_saves_reserve_the_last_connection_slot() {
        let _guard = crate::test_support::global_state();
        let device = uuid::Uuid::new_v4().to_string();
        for computer in 0..TUNNEL_LIMIT - 1 {
            tunnels()
                .intents
                .insert((device.clone(), computer.to_string(), 3000), intent(43000));
        }
        let first = PendingSave::new((device.clone(), "first".into(), 3000)).unwrap();
        let second = PendingSave::new((device.clone(), "second".into(), 3000));
        let rejected = second.is_err();
        drop(second);
        drop(first);
        close_device(&device);
        assert!(rejected, "two saves were admitted into the last slot");
    }

    #[test]
    fn a_failed_save_releases_its_connection_slot() {
        let _guard = crate::test_support::global_state();
        let device = uuid::Uuid::new_v4().to_string();
        for computer in 0..TUNNEL_LIMIT - 1 {
            tunnels()
                .intents
                .insert((device.clone(), computer.to_string(), 3000), intent(43000));
        }
        let failed = save_tunnel(
            (device.clone(), "failed".into(), 3000),
            None,
            None,
            32000,
            |_| Err("SSH fixture failed".into()),
        );
        let next = PendingSave::new((device.clone(), "next".into(), 3000));
        let admitted = next.is_ok();
        drop(next);
        close_device(&device);
        assert!(failed.is_err());
        assert!(admitted, "a failed open leaked its connection slot");
    }

    #[test]
    fn disconnected_intents_keep_their_connection_slots() {
        let _guard = crate::test_support::global_state();
        let device = uuid::Uuid::new_v4().to_string();
        for computer in 0..TUNNEL_LIMIT {
            tunnels()
                .intents
                .insert((device.clone(), computer.to_string(), 3000), intent(43000));
        }
        disconnect_device(&device);
        let extra = PendingSave::new((device.clone(), "extra".into(), 3000));
        let rejected = extra.is_err();
        drop(extra);
        // Editing an existing connection remains possible at the limit.
        let existing = PendingSave::new((device.clone(), "0".into(), 3000));
        let editable = existing.is_ok();
        drop(existing);
        close_device(&device);
        assert!(rejected, "disconnecting freed slots promised to reconnects");
        assert!(editable, "a configured connection could not be edited");
    }

    #[test]
    fn an_older_poll_cannot_forget_a_newly_saved_tunnel() {
        let _guard = crate::test_support::global_state();
        let device = uuid::Uuid::new_v4().to_string();
        let key = key(&device);
        let before = tunnels().observe(&device);
        save_tunnel(key.clone(), None, Some("http".into()), 32000, |_| {
            Ok(tunnel(43100, 32000))
        })
        .unwrap();
        let mut state = tunnels();
        let result = project_observed(json!({"computers":[]}), &device, before, &mut state);
        let kept = state.live.contains_key(&key) && state.intents.contains_key(&key);
        let current = state.observe(&device);
        let fresh =
            project_observed(json!({"computers":[]}), &device, current, &mut state).unwrap();
        let deleted = !state.live.contains_key(&key) && !state.intents.contains_key(&key);
        drop(state);
        drop(fresh);
        close_device(&device);
        assert!(result.is_err(), "an obsolete poll was applied");
        assert!(kept, "an obsolete poll deleted the saved connection");
        assert!(deleted, "a current deletion must still close the tunnel");
    }

    #[test]
    fn polls_wait_for_their_own_hosts_pending_save() {
        let _guard = crate::test_support::global_state();
        let device = uuid::Uuid::new_v4().to_string();
        let pending = PendingSave::new(key(&device)).unwrap();
        let mut state = tunnels();
        let observation = state.observe(&device);
        assert!(
            project_observed(json!({"computers":[]}), &device, observation, &mut state).is_err()
        );
        let other = state.observe("other");
        assert!(project_observed(json!({"computers":[]}), "other", other, &mut state).is_ok());
        drop(state);
        drop(pending);
        close_device(&device);
    }

    #[test]
    fn a_removed_port_is_not_restored_by_an_older_save() {
        let _guard = crate::test_support::global_state();
        let device = uuid::Uuid::new_v4().to_string();
        let key = key(&device);
        let result = save_tunnel(key.clone(), None, Some("http".into()), 32000, |_| {
            forget_port(&key);
            Ok(tunnel(43100, 32000))
        });
        let mut state = tunnels();
        let projection = project_ports(observed(None), &device, &mut state).unwrap();
        let restored = state.live.contains_key(&key) || state.intents.contains_key(&key);
        drop(state);
        close_device(&device);
        assert!(result.is_err(), "an obsolete save succeeded after removal");
        assert!(!restored, "removal was undone by the older save");
        assert_eq!(port(&projection)["configured"], false);
    }

    #[test]
    fn a_save_removed_during_publication_never_opens_a_tunnel() {
        let _guard = crate::test_support::global_state();
        let device = uuid::Uuid::new_v4().to_string();
        let key = key(&device);
        let pending = PendingSave::new(key.clone()).unwrap();
        forget_port(&key);
        let result = finish_save(pending, None, Some("http".into()), 32000, |_| {
            panic!("an obsolete publication must not start SSH")
        });
        assert!(result.is_err());
        assert!(!tunnels().saves.contains_key(&key));
        close_device(&device);
    }

    #[test]
    fn saving_a_port_never_holds_the_lock_while_ssh_starts_and_keeps_the_old_tunnel_on_failure() {
        let _guard = crate::test_support::global_state();
        let device = uuid::Uuid::new_v4().to_string();
        let key = key(&device);
        save_tunnel(key.clone(), None, Some("http".into()), 32000, |requested| {
            assert!(
                TUNNELS.get().unwrap().try_lock().is_ok(),
                "the tunnels lock is held during the probe"
            );
            assert_eq!(requested, None);
            Ok(tunnel(43100, 32000))
        })
        .unwrap();
        // Moving to another local port fails: the working tunnel stays.
        let failed = save_tunnel(key.clone(), Some(43101), None, 32000, |requested| {
            assert_eq!(requested, Some(43101));
            assert!(
                tunnels().live.contains_key(&key),
                "replaced before the new tunnel worked"
            );
            Err("This local port is already in use.".into())
        });
        assert!(failed.is_err());
        assert_eq!(tunnels().live[&key].local_port, 43100);
        // Automatic reuses the intended local port after the tunnel died.
        tunnels().live.remove(&key);
        save_tunnel(
            key.clone(),
            None,
            Some("https".into()),
            32005,
            |requested| {
                assert_eq!(requested, Some(43100));
                Ok(tunnel(43100, 32005))
            },
        )
        .unwrap();
        assert_eq!(
            tunnels().intents[&key],
            Intent {
                local_port: 43100,
                scheme: Some("https".into())
            }
        );
        // An unchanged save only updates the scheme.
        save_tunnel(key.clone(), None, Some("http".into()), 32005, |_| {
            panic!("must not reopen")
        })
        .unwrap();
        assert_eq!(tunnels().intents[&key].scheme.as_deref(), Some("http"));
        disconnect_device(&device);
        assert!(!tunnels().live.contains_key(&key) && tunnels().intents.contains_key(&key));
        close_device(&device);
        assert!(!tunnels().intents.contains_key(&key));
    }

    #[test]
    fn an_unrelated_listener_during_authentication_is_never_ready() {
        let sleeper = || {
            let mut command = Command::new("/bin/sleep");
            command.arg("30");
            command
        };
        // Another process takes the selected port while SSH is still authenticating.
        let listener = std::sync::Arc::new(Mutex::new(None));
        let mut worker = None;
        let result = open_tunnel(
            |local, _| {
                let listener = listener.clone();
                worker = Some(std::thread::spawn(move || {
                    let bound = (0..200).find_map(|_| {
                        TcpListener::bind(("127.0.0.1", local)).ok().or_else(|| {
                            std::thread::sleep(Duration::from_millis(10));
                            None
                        })
                    });
                    *listener.lock().unwrap() = bound;
                }));
                let mut command = Command::new("/bin/sh");
                command.args(["-c", "sleep 0.3; exit 1"]);
                Ok((command, Command::new("/usr/bin/false")))
            },
            None,
            32000,
            Duration::from_secs(5),
        );
        worker.unwrap().join().unwrap();
        assert!(listener.lock().unwrap().is_some());
        assert!(result.is_err(), "an unrelated listener was reported Ready");
        let exited = open_tunnel(
            |_, _| {
                Ok((
                    Command::new("/usr/bin/false"),
                    Command::new("/usr/bin/false"),
                ))
            },
            None,
            32000,
            Duration::from_secs(5),
        );
        assert!(exited.unwrap_err().contains("could not open"));
        let silent = open_tunnel(
            |_, _| Ok((sleeper(), Command::new("/usr/bin/false"))),
            None,
            32000,
            Duration::from_millis(300),
        );
        assert_eq!(silent.unwrap_err(), "Timed out opening the SSH tunnel.");
        let taken = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = taken.local_addr().unwrap().port();
        assert_eq!(
            open_tunnel(
                |_, _| Ok((sleeper(), Command::new("/usr/bin/false"))),
                Some(port),
                32000,
                Duration::from_millis(300)
            )
            .unwrap_err(),
            "This local port is already in use."
        );
    }

    fn fixture_forward(local: u16, socket: &Path) -> Command {
        let mut command = Command::new("python3");
        command
            .args([
                "-c",
                r#"
import socket, sys, time
# Delayed authentication, then local forwarding, then the control socket.
time.sleep(0.15)
listener = socket.socket()
listener.bind(('127.0.0.1', int(sys.argv[1])))
listener.listen()
control = socket.socket(socket.AF_UNIX)
control.bind(sys.argv[2])
control.listen()
time.sleep(30)
"#,
            ])
            .arg(local.to_string())
            .arg(socket);
        command
    }

    #[test]
    fn delayed_authentication_waits_for_a_successful_control_reply() {
        let started = Instant::now();
        let tunnel = open_tunnel(
            |local, socket| {
                let mut check = Command::new("/bin/sh");
                check
                    .args(["-c", "sleep 0.15; test -S \"$1\"", "check"])
                    .arg(socket);
                Ok((fixture_forward(local, socket), check))
            },
            None,
            32000,
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(300));
        assert!(std::net::TcpStream::connect(("127.0.0.1", tunnel.local_port)).is_ok());
        assert_eq!(tunnel.remote_port, 32000);
    }

    #[test]
    fn a_listener_and_control_socket_without_an_acknowledgement_are_not_ready() {
        let result = open_tunnel(
            |local, socket| {
                Ok((
                    fixture_forward(local, socket),
                    Command::new("/usr/bin/false"),
                ))
            },
            None,
            32000,
            Duration::from_millis(500),
        );
        assert_eq!(result.unwrap_err(), "Timed out opening the SSH tunnel.");
    }

    #[test]
    fn a_stalled_control_reply_is_bounded_by_the_readiness_deadline() {
        let result = open_tunnel(
            |local, socket| {
                let mut check = Command::new("/bin/sleep");
                check.arg("30");
                Ok((fixture_forward(local, socket), check))
            },
            None,
            32000,
            Duration::from_millis(500),
        );
        assert_eq!(result.unwrap_err(), "Timed out opening the SSH tunnel.");
    }

    const CONTROLLER_FIXTURE: &str = "SILO_TEST_TUNNEL_CONTROLLER";

    fn recorded_forward(directory: &Path, local: u16, socket: &Path) -> Command {
        let script = directory.join("forward.py");
        std::fs::write(
            &script,
            r#"
import os, pathlib, socket, subprocess, sys, time
if sys.argv[1] == 'proxy':
    time.sleep(30)
    sys.exit(0)
proxy = subprocess.Popen([sys.executable, __file__, 'proxy'])
listener = socket.socket()
listener.bind(('127.0.0.1', int(sys.argv[1])))
listener.listen()
control = socket.socket(socket.AF_UNIX)
control.bind(sys.argv[2])
control.listen()
pathlib.Path(sys.argv[3]).write_text(f'{os.getpid()} {proxy.pid}')
time.sleep(30)
"#,
        )
        .unwrap();
        let mut command = Command::new("python3");
        command
            .arg(script)
            .arg(local.to_string())
            .arg(socket)
            .arg(directory.join("pids"));
        command
    }

    fn recorded_tunnel(directory: &Path) -> Tunnel {
        open_tunnel(
            |local, socket| {
                let mut check = Command::new("/bin/test");
                check.arg("-S").arg(socket);
                Ok((recorded_forward(directory, local, socket), check))
            },
            None,
            32000,
            Duration::from_secs(5),
        )
        .unwrap()
    }

    #[test]
    #[ignore = "subprocess fixture for controller lifetime tests"]
    fn published_port_controller_fixture() {
        let directory = std::path::PathBuf::from(
            std::env::var_os(CONTROLLER_FIXTURE).expect("fixture directory"),
        );
        let tunnel = recorded_tunnel(&directory);
        let socket_directory = tunnel._directory.as_ref().unwrap().path();
        std::fs::write(
            directory.join("ready"),
            format!(
                "{} {} {}",
                std::process::id(),
                tunnel.local_port,
                socket_directory.display()
            ),
        )
        .unwrap();
        std::thread::sleep(Duration::from_secs(30));
        drop(tunnel);
    }

    struct FixtureProcess(Child);
    impl Drop for FixtureProcess {
        fn drop(&mut self) {
            if matches!(self.0.try_wait(), Ok(None)) {
                unsafe { libc::kill(self.0.id() as i32, libc::SIGTERM) };
            }
            let _ = self.0.wait();
        }
    }

    fn fixture_process_matches(pid: i32, script: &Path) -> bool {
        Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
            .is_ok_and(|output| {
                String::from_utf8_lossy(&output.stdout).contains(script.to_str().unwrap())
            })
    }

    struct FixtureForwards {
        directory: std::path::PathBuf,
        pids: Vec<i32>,
    }
    impl Drop for FixtureForwards {
        fn drop(&mut self) {
            for pid in &self.pids {
                if fixture_process_matches(*pid, &self.directory.join("forward.py")) {
                    unsafe { libc::kill(*pid, libc::SIGTERM) };
                }
            }
        }
    }

    fn wait_for_fixture_file(path: &Path, fields: usize, controller: &mut Child) -> String {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(text) = std::fs::read_to_string(path) {
                if text.split_whitespace().count() == fields {
                    return text;
                }
            }
            assert!(
                matches!(controller.try_wait(), Ok(None)),
                "fixture controller exited before becoming ready"
            );
            assert!(
                Instant::now() < until,
                "fixture controller never became ready"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn fixture_processes_ended(pids: &[i32]) -> bool {
        pids.iter().all(|pid| unsafe { libc::kill(*pid, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH))
    }

    #[test]
    fn a_controller_crash_closes_the_published_listener_and_its_descendants() {
        let directory = tempfile::Builder::new()
            .prefix("silo-crash-test-")
            .tempdir_in("/tmp")
            .unwrap();
        let mut controller = FixtureProcess(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "remote_network::tests::published_port_controller_fixture",
                    "--ignored",
                ])
                .env(CONTROLLER_FIXTURE, directory.path())
                .env("HOME", directory.path())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let recorded = wait_for_fixture_file(&directory.path().join("pids"), 2, &mut controller.0);
        let forwards = FixtureForwards {
            directory: directory.path().to_owned(),
            pids: recorded
                .split_whitespace()
                .map(|pid| pid.parse().unwrap())
                .collect(),
        };
        let ready = wait_for_fixture_file(&directory.path().join("ready"), 3, &mut controller.0);
        let mut fields = ready.split_whitespace();
        assert_eq!(
            fields.next().unwrap().parse::<u32>().unwrap(),
            controller.0.id()
        );
        let port: u16 = fields.next().unwrap().parse().unwrap();
        let socket_directory = std::path::PathBuf::from(fields.next().unwrap());
        assert_eq!(forwards.pids.len(), 2);
        for pid in &forwards.pids {
            assert!(fixture_process_matches(
                *pid,
                &directory.path().join("forward.py")
            ));
        }
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
        let mut unrelated = FixtureProcess(Command::new("/bin/sleep").arg("30").spawn().unwrap());
        assert!(matches!(controller.0.try_wait(), Ok(None)));
        // SIGTERM ends this verified test controller without running Rust destructors.
        assert_eq!(
            unsafe { libc::kill(controller.0.id() as i32, libc::SIGTERM) },
            0
        );
        controller.0.wait().unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !fixture_processes_ended(&forwards.pids) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        let ended = fixture_processes_ended(&forwards.pids);
        let listener_closed = std::net::TcpStream::connect(("127.0.0.1", port)).is_err();
        assert!(
            matches!(unrelated.0.try_wait(), Ok(None)),
            "an unrelated process was terminated"
        );
        drop(forwards);
        std::fs::remove_dir_all(socket_directory).unwrap();
        assert!(
            ended && listener_closed,
            "published forward or descendant outlived its controller"
        );
    }

    #[test]
    fn the_owner_reads_its_network_state_once_for_concurrent_polls() {
        let cache = Mutex::new(None);
        let mut reads = 0;
        for _ in 0..3 {
            let value = cached(&cache, Duration::from_secs(60), || {
                reads += 1;
                Ok(json!(reads))
            })
            .unwrap();
            assert_eq!(value, json!(1));
        }
        assert_eq!(
            cached(&cache, Duration::ZERO, || Ok(json!("fresh"))).unwrap(),
            json!("fresh")
        );
        assert!(cached(&cache, Duration::ZERO, || Err("failed".into())).is_err());
        assert_eq!(
            cached(&cache, Duration::from_secs(60), || panic!(
                "an error is not stored"
            ))
            .unwrap(),
            json!("fresh")
        );
    }
}
