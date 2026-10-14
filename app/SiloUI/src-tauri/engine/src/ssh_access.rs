//! Opt-in SSH listeners with isolated client keys and pipe-owned runtime children.
use crate::{
    editor,
    runtime::{self, ProcessRunner, RuntimePaths},
};
use serde::{Deserialize, Serialize};
use silo_engine::{Audience, Event};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader},
    net::Ipv4Addr,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock},
    time::Duration,
};

const FAILED: &str = "Could not read SSH access settings.";
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configuration {
    computer: String,
    computer_id: String,
    enabled: bool,
    port: u16,
    bind_address: String,
    keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    managed_key: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Computer {
    computer: String,
    computer_id: String,
    enabled: bool,
    port: u16,
    bind_address: String,
    keys: Vec<String>,
    state: &'static str,
    message: Option<String>,
    fingerprint: Option<String>,
    device_name: String,
    addresses: Vec<String>,
    /// Guest account SSH clients log in as.
    user: &'static str,
}
#[derive(Serialize)]
pub struct State {
    computers: Vec<Computer>,
}
struct Listener {
    children: Vec<Child>,
    config: Configuration,
}
impl Drop for Listener {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
#[derive(Default)]
struct Owners {
    listeners: BTreeMap<String, Listener>,
    errors: BTreeMap<String, String>,
    /// Bumped by every close. A reconcile pass discards a listener it started when
    /// that computer (or everything) was closed meanwhile, instead of reopening access
    /// the close just revoked.
    generation: u64,
    closed: BTreeMap<String, u64>,
    all_closed: u64,
}
impl Owners {
    fn closed_since(&self, computer: &str, generation: u64) -> bool {
        self.all_closed > generation || self.closed.get(computer).is_some_and(|g| *g > generation)
    }
}
/// The listeners and their errors. Held only briefly, never across runtime
/// inspection or listener start-up, so state reads and closes stay immediate (C-10).
fn owners() -> &'static Mutex<Owners> {
    static OWNERS: OnceLock<Mutex<Owners>> = OnceLock::new();
    OWNERS.get_or_init(Default::default)
}
/// Every reconcile pass re-derives the listeners from settings and the runtime, so
/// a panic under the lock leaves nothing a later pass cannot repair: recover it.
fn owned() -> std::sync::MutexGuard<'static, Owners> {
    owners()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
/// Serializes whole reconcile passes so two passes never start the same listener.
/// It is held across runtime inspection and listener start-up; `owners()` is not.
static RECONCILE: Mutex<()> = Mutex::new(());
fn path(paths: &RuntimePaths) -> PathBuf {
    paths.metadata.with_file_name("ssh-access.json")
}
fn read(paths: &RuntimePaths) -> Result<Vec<Configuration>, String> {
    let bytes = editor::read_regular(&path(paths))?;
    if bytes.is_empty() {
        return Ok(vec![]);
    }
    let configs: Vec<Configuration> = serde_json::from_slice(&bytes).map_err(|_| FAILED)?;
    if configs.len() > 4096 {
        return Err(FAILED.into());
    }
    let mut seen = BTreeSet::new();
    for config in &configs {
        validate(config)?;
        if !seen.insert(&config.computer) {
            return Err(FAILED.into());
        }
    }
    Ok(configs)
}
fn validate(config: &Configuration) -> Result<(), String> {
    runtime::validate_name(&config.computer).map_err(|e| e.to_string())?;
    uuid::Uuid::parse_str(&config.computer_id).map_err(|_| "Invalid computer identity.")?;
    if config.port == 0 {
        return Err("Enter a port from 1 to 65535.".into());
    }
    let ip: Ipv4Addr = config
        .bind_address
        .parse()
        .map_err(|_| "Choose an IPv4 address assigned to the device.")?;
    if ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_broadcast()
        || (ip.is_loopback() && ip != Ipv4Addr::LOCALHOST)
    {
        return Err("Choose 127.0.0.1 or a specific network interface address. Wildcard bindings are not allowed.".into());
    }
    if config.keys.len() > 128 {
        return Err("Use at most 128 authorized keys.".into());
    }
    if let Some(key) = &config.managed_key {
        editor::validate_public_key(key)?;
    }
    let mut seen = BTreeSet::new();
    for key in &config.keys {
        let normalized = normalize_key(key)?;
        if !seen.insert(normalized) {
            return Err("This public key is already authorized.".into());
        }
    }
    Ok(())
}
fn normalize_key(key: &str) -> Result<String, String> {
    if key.len() > 4096 || key.contains(['\n', '\r', '\0']) {
        return Err("Paste one Ed25519 public key per entry.".into());
    }
    let mut words = key.split_whitespace();
    let normalized = format!(
        "{} {}",
        words.next().unwrap_or(""),
        words.next().unwrap_or("")
    );
    editor::validate_public_key(&normalized)?;
    Ok(normalized)
}
fn device_name() -> String {
    let mut bytes = [0u8; 256];
    // SAFETY: gethostname writes at most the supplied buffer length.
    if unsafe { libc::gethostname(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        return "Unnamed device".into();
    }
    String::from_utf8_lossy(&bytes)
        .trim_end_matches('\0')
        .to_owned()
}
pub(crate) fn addresses() -> Vec<String> {
    let mut list = std::ptr::null_mut();
    let mut result = BTreeSet::new();
    // SAFETY: getifaddrs owns a linked list until freeifaddrs. Check family before casting.
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return vec![];
        }
        let mut item = list;
        while !item.is_null() {
            let addr = (*item).ifa_addr;
            if !addr.is_null()
                && (*addr).sa_family as i32 == libc::AF_INET
                && (*item).ifa_flags & libc::IFF_UP as u32 != 0
            {
                let ip = Ipv4Addr::from(
                    (*(addr as *const libc::sockaddr_in))
                        .sin_addr
                        .s_addr
                        .to_ne_bytes(),
                );
                if !ip.is_loopback()
                    && !ip.is_unspecified()
                    && !ip.is_multicast()
                    && !ip.is_broadcast()
                {
                    result.insert(ip.to_string());
                }
            }
            item = (*item).ifa_next;
        }
        libc::freeifaddrs(list);
    }
    result.into_iter().collect()
}
fn fingerprint(paths: &RuntimePaths, name: &str) -> Result<String, String> {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let root = paths.home.join("sandboxes").join(name).join("ssh");
    let key = root.join("host_ed25519");
    let public = editor::public_key(&key)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(public.split_whitespace().nth(1).ok_or(FAILED)?)
        .map_err(|_| FAILED)?;
    Ok(format!(
        "SHA256:{}",
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(bytes))
    ))
}
fn spawn(paths: &RuntimePaths, config: &Configuration) -> Result<Listener, String> {
    let root = paths.home.join("ssh/managed-access");
    editor::private_directory(&paths.home.join("ssh"))?;
    editor::private_directory(&root)?;
    let authorized = root.join(format!("{}.authorized_keys", config.computer_id));
    // Deliberately never read or write the editor's shared authorized_keys or private client key.
    editor::write_private(
        &authorized,
        format!("{}\n", config.keys.join("\n")).as_bytes(),
    )?;
    let host_root = paths
        .home
        .join("sandboxes")
        .join(&config.computer)
        .join("ssh");
    editor::private_directory(&host_root)?;
    editor::key(&host_root.join("host_ed25519"))?;
    fingerprint(paths, &config.computer)?;
    let mut listener = Listener {
        children: vec![],
        config: config.clone(),
    };
    let mut hosts = vec!["127.0.0.1"];
    if config.bind_address != "127.0.0.1" {
        hosts.push(&config.bind_address);
    }
    for host in hosts {
        let mut child = Command::new(&paths.executable)
            .args([
                "ssh",
                "serve",
                &config.computer,
                "--no-start",
                "--no-inactivity-timeout",
                "--exit-on-stdin-close",
                "--authorized-keys",
            ])
            .arg(&authorized)
            .args(["--expected-machine-id", &config.computer_id])
            .args(["--host", host, "--port", &config.port.to_string()])
            .env("MSB_HOME", &paths.home)
            .env("MSB_PATH", &paths.executable)
            .env("MSB_LIBKRUNFW_PATH", &paths.library)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "Could not start the SSH listener.")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("Could not read SSH listener readiness.")?;
        listener.children.push(child);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line == "SILO_SSH_READY" {
                    let _ = tx.try_send(());
                }
            }
        });
        rx.recv_timeout(Duration::from_secs(8)).map_err(|_| "SSH could not listen. Check whether the port is in use, the network address is available, and the bundled runtime is current.")?;
        if listener
            .children
            .last_mut()
            .unwrap()
            .try_wait()
            .map_err(|_| "Could not check SSH listener.")?
            .is_some()
        {
            return Err("The SSH listener exited.".into());
        }
    }
    Ok(listener)
}
fn inspect_running(paths: &RuntimePaths, config: &Configuration) -> Result<bool, String> {
    let inspected = match runtime::observe_computer(&ProcessRunner, paths, &config.computer)
        .map_err(|e| format!("Could not verify computer status ({e}). SSH access is closed."))?
    {
        runtime::ComputerRuntime::Absent => return Ok(false),
        runtime::ComputerRuntime::Present(inspected) => inspected,
    };
    runtime::ensure_managed(&inspected).map_err(|_| "This computer is not managed by Silo.")?;
    if inspected
        .config
        .pointer("/labels/silo.machine-id")
        .and_then(serde_json::Value::as_str)
        != Some(config.computer_id.as_str())
    {
        return Err("The computer identity changed. Configure SSH access again.".into());
    }
    crate::working_account::require_runtime(paths)?;
    Ok(inspected.status == "Running")
}
/// Bring the listeners in line with the saved settings and each computer's runtime
/// state. Runtime inspection and listener start-up (up to 8 s each) run without the
/// `owners()` lock, so state reads and closes never wait behind a pass (C-10).
pub(crate) fn reconcile(paths: &RuntimePaths) {
    if paths.metadata.as_os_str().is_empty() {
        return;
    }
    let _pass = RECONCILE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if runtime::shutdown::ensure_accepting_operations().is_err() {
        close_all();
        return;
    }
    let Ok(configs) = read(paths) else {
        close_all();
        return;
    };
    let Ok(metadata) = runtime::read_metadata(&paths.metadata) else {
        close_all();
        return;
    };
    // A close after this point (a computer stopping, a save revoking access, Quit)
    // makes a listener this pass starts for that computer stale.
    let generation = owned().generation;
    let addresses = addresses();
    let observed: Vec<_> = configs
        .iter()
        .filter(|c| c.enabled)
        .map(|config| {
            let observed = if metadata
                .computers
                .iter()
                .any(|m| m.name() == config.computer && m.id() == config.computer_id)
            {
                inspect_running(paths, config)
            } else {
                Err("The computer identity changed. Configure SSH access again.".into())
            };
            (config, observed)
        })
        .collect();
    // Listeners are killed after the lock is released, but before any replacement
    // starts, so a changed listener can reuse its port.
    let mut retired = Vec::new();
    let launches: Vec<&Configuration> = {
        let mut owned = owned();
        let stale: Vec<String> = owned
            .listeners
            .keys()
            .filter(|name| {
                !configs
                    .iter()
                    .any(|c| &c.computer == *name && c.enabled && !c.keys.is_empty())
            })
            .cloned()
            .collect();
        retired.extend(stale.iter().filter_map(|name| owned.listeners.remove(name)));
        owned.errors.clear();
        observed
            .into_iter()
            .filter_map(|(config, observed)| {
                prepare_one(&mut owned, config, observed, &addresses, &mut retired)
                    .then_some(config)
            })
            .collect()
    };
    drop(retired);
    if launches.is_empty() {
        return;
    }
    let launched: Vec<_> = launches
        .into_iter()
        .map(|config| {
            let listener = runtime::shutdown::ensure_accepting_operations()
                .and_then(|()| spawn(paths, config));
            (config, listener)
        })
        .collect();
    let current = read(paths).unwrap_or_default();
    let mut retired = Vec::new();
    let mut owned = owned();
    let accepting = runtime::shutdown::ensure_accepting_operations().is_ok();
    for (config, listener) in launched {
        if accepting
            && !owned.closed_since(&config.computer, generation)
            && current.contains(config)
        {
            finish_one(&mut owned, config, listener, &mut retired);
        } else {
            // Closed or changed while starting: the next pass decides from fresh state.
            retired.extend(listener.ok());
        }
    }
    drop(owned);
}
/// Settle one enabled configuration against its observed runtime state under the
/// `owners()` lock. Returns true when a new listener must be started; the caller
/// starts it without the lock and records the outcome with `finish_one`. Listeners
/// to stop are moved to `retired`.
fn prepare_one(
    owned: &mut Owners,
    config: &Configuration,
    observed: Result<bool, String>,
    addresses: &[String],
    retired: &mut Vec<Listener>,
) -> bool {
    let result = (|| {
        if !config.enabled || !observed? {
            retired.extend(owned.listeners.remove(&config.computer));
            return Ok(false);
        }
        if config.keys.is_empty() {
            return Err("Add an authorized public key to open SSH access.".into());
        }
        if config.bind_address != "127.0.0.1" && !addresses.contains(&config.bind_address) {
            return Err(
                "The selected network address is unavailable. Choose an active interface.".into(),
            );
        }
        if let Some(listener) = owned.listeners.get_mut(&config.computer) {
            if listener.config == *config
                && listener
                    .children
                    .iter_mut()
                    .all(|c| matches!(c.try_wait(), Ok(None)))
            {
                return Ok(false);
            }
        }
        retired.extend(owned.listeners.remove(&config.computer));
        runtime::shutdown::ensure_accepting_operations()?;
        Ok(true)
    })();
    match result {
        Ok(launch) => {
            owned.errors.remove(&config.computer);
            launch
        }
        Err(error) => {
            retired.extend(owned.listeners.remove(&config.computer));
            owned.errors.insert(config.computer.clone(), error);
            false
        }
    }
}
fn finish_one(
    owned: &mut Owners,
    config: &Configuration,
    listener: Result<Listener, String>,
    retired: &mut Vec<Listener>,
) {
    match listener {
        Ok(listener) => {
            retired.extend(owned.listeners.insert(config.computer.clone(), listener));
            owned.errors.remove(&config.computer);
        }
        Err(error) => {
            retired.extend(owned.listeners.remove(&config.computer));
            owned.errors.insert(config.computer.clone(), error);
        }
    }
}
/// `prepare_one`, start, `finish_one` in one step, for tests of a single computer.
#[cfg(test)]
fn reconcile_one(
    owned: &mut Owners,
    config: &Configuration,
    observed: Result<bool, String>,
    addresses: &[String],
    launch: impl FnOnce() -> Result<Listener, String>,
) {
    let mut retired = Vec::new();
    if prepare_one(owned, config, observed, addresses, &mut retired) {
        drop(std::mem::take(&mut retired));
        finish_one(owned, config, launch(), &mut retired);
    }
}

pub(crate) fn close_computer(name: &str) {
    let mut owned = owned();
    owned.generation += 1;
    let generation = owned.generation;
    owned.closed.insert(name.to_owned(), generation);
    let listener = owned.listeners.remove(name);
    owned.errors.remove(name);
    drop(owned);
    drop(listener);
}
pub fn close_all() {
    let mut owned = owned();
    owned.generation += 1;
    owned.all_closed = owned.generation;
    owned.closed.clear();
    let listeners = std::mem::take(&mut owned.listeners);
    owned.errors.clear();
    drop(owned);
    drop(listeners);
}
/// Repair the shared SSH listeners under the operation gate, skipping busy periods.
/// Listener reconciliation changes shared device state, so it takes the gate;
/// observation paths (`state`, `read_ssh_access_state`) must never call this
/// inline. Callers already holding a gate guard (the computer lifecycle in `run_msb`,
/// backups, checkpoints, and the write commands) call `reconcile` directly.
fn reconcile_if_idle(paths: &RuntimePaths) {
    // Background listener reconcile touches shared SSH state; skip when busy.
    if let Ok(_guard) = runtime::OPERATIONS.try_device_hidden("Reconciling SSH access") {
        if runtime::shutdown::ensure_accepting_operations().is_ok() {
            reconcile(paths);
        }
    }
}
/// Schedule a one-off listener reconcile off the calling thread. The read path
/// uses this so it can return observed state immediately while any needed repair
/// converges in the background.
fn schedule_reconcile(paths: RuntimePaths) {
    // Reads can arrive in bursts; one repair pass at a time is enough.
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !needs_reconcile(&paths) || RUNNING.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(move || {
        reconcile_if_idle(&paths);
        RUNNING.store(false, std::sync::atomic::Ordering::Release);
    });
}
/// Background safety net for changes nothing reports: a computer stopping outside
/// Silo, a listener exiting, a network interface going away. Lifecycle commands,
/// saves, backups and checkpoints reconcile directly, so this runs rarely (C-10).
const MONITOR_INTERVAL: Duration = Duration::from_secs(15);
/// Whether a background pass has anything to open, check or close. Without any
/// enabled SSH access and without owned listeners or errors, a pass would only take
/// the device gate and inspect nothing, so it is skipped.
fn needs_reconcile(paths: &RuntimePaths) -> bool {
    let enabled = read(paths).map_or(true, |configs| configs.iter().any(|c| c.enabled));
    enabled || {
        let owned = owned();
        !owned.listeners.is_empty() || !owned.errors.is_empty()
    }
}
pub fn monitor(paths: &RuntimePaths) {
    if needs_reconcile(paths) {
        reconcile_if_idle(paths);
    }
}
fn state(paths: &RuntimePaths) -> Result<State, String> {
    let configs = read(paths)?;
    let metadata = runtime::read_metadata(&paths.metadata).map_err(|_| FAILED)?;
    // Snapshot the owned state; fingerprints and addresses are read without the lock.
    let (errors, listening): (BTreeMap<String, String>, BTreeSet<String>) = {
        let owned = owned();
        (
            owned.errors.clone(),
            owned.listeners.keys().cloned().collect(),
        )
    };
    let device_name = device_name();
    let addresses = addresses();
    Ok(State {
        computers: metadata
            .computers
            .iter()
            .map(|configuration| {
                let config = configs.iter().find(|c| {
                    c.computer == configuration.name() && c.computer_id == configuration.id()
                });
                let enabled = config.is_some_and(|c| c.enabled);
                let message = errors.get(configuration.name()).cloned();
                let status = if !enabled {
                    "disabled"
                } else if message.is_some() {
                    "error"
                } else if listening.contains(configuration.name()) {
                    "listening"
                } else {
                    "waiting"
                };
                Computer {
                    computer: configuration.name().into(),
                    computer_id: configuration.id().into(),
                    enabled,
                    port: config.map_or(2222, |c| c.port),
                    bind_address: config
                        .map_or("127.0.0.1", |c| c.bind_address.as_str())
                        .into(),
                    keys: config.map_or_else(Vec::new, |c| c.keys.clone()),
                    state: status,
                    message,
                    fingerprint: if enabled {
                        fingerprint(paths, configuration.name()).ok()
                    } else {
                        None
                    },
                    device_name: device_name.clone(),
                    addresses: addresses.clone(),
                    user: "silo",
                }
            })
            .collect(),
    })
}
pub fn read_state(paths: &RuntimePaths) -> Result<State, String> {
    // Observation only: report the currently owned listeners without taking the
    // gate or mutating anything, so a status read never waits behind a long
    // operation. Any needed listener repair is scheduled in the background.
    let result = state(paths);
    schedule_reconcile(paths.clone());
    result
}
pub fn save(
    paths: &RuntimePaths,
    computer: String,
    enabled: bool,
    port: u16,
    bind_address: String,
    keys: Option<Vec<String>>,
) -> Result<State, String> {
    let _operation = crate::updates::operation_guard()?;
    // Saving SSH access reconciles the shared listeners/port map; device scope.
    let _guard = runtime::OPERATIONS
        .device("Saving SSH access")
        .map_err(|_| FAILED)?;
    runtime::shutdown::ensure_accepting_operations()?;
    let result = save_with(
        paths,
        Target::Name(&computer),
        Settings {
            enabled,
            port,
            bind_address,
            keys,
        },
    );
    publish_network_state_changed();
    result
}

fn publish_network_state_changed() {
    if let Some(context) = silo_engine::context::current() {
        context.publish(
            Event::new("silo://network-state-changed", serde_json::Value::Null),
            Audience::All,
        );
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Settings {
    pub(crate) enabled: bool,
    pub(crate) port: u16,
    pub(crate) bind_address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) keys: Option<Vec<String>>,
}
enum Target<'a> {
    Name(&'a str),
    Id(&'a str),
}
fn client_key(paths: &RuntimePaths, id: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid computer identity.")?;
    let root = paths.home.join("ssh/managed-clients");
    editor::private_directory(&root)?;
    let path = root.join(id);
    editor::key(&path)?;
    Ok(path)
}

/// Recover the managed public identity from key files for settings written before
/// it was recorded separately. Never creates a key.
fn managed_public_key(paths: &RuntimePaths, id: &str) -> Option<String> {
    uuid::Uuid::parse_str(id).ok()?;
    let path = paths.home.join("ssh/managed-clients").join(id);
    let public = path
        .exists()
        .then(|| editor::public_key(&path).ok())
        .flatten();
    public.or_else(|| {
        let bytes = editor::read_regular(&path.with_extension("pub")).ok()?;
        normalize_key(std::str::from_utf8(&bytes).ok()?.trim()).ok()
    })
}
/// Comment marking a key another device registered through `ssh.access.connection`,
/// followed by that device's Silo identity.
const CONTROLLER_TAG: &str = "silo-controller:";
fn is_controller_key(key: &str) -> bool {
    key.split_whitespace()
        .nth(2)
        .is_some_and(|comment| comment.starts_with(CONTROLLER_TAG))
}

// Only an explicit local connection/export request reads the managed private key,
// and it never leaves this device. Ordinary state responses, persisted settings
// and remote responses contain public keys only.
pub(crate) fn connection_material(
    paths: &RuntimePaths,
    computer_id: &str,
) -> Result<serde_json::Value, String> {
    uuid::Uuid::parse_str(computer_id).map_err(|_| "Invalid computer identity.")?;
    let config = read(paths)?
        .into_iter()
        .find(|c| c.computer_id == computer_id && c.enabled)
        .ok_or("Enable SSH access first.")?;
    let user = crate::working_account::inspect_user(paths, &config.computer)?;
    save_with(
        paths,
        Target::Id(computer_id),
        Settings {
            enabled: true,
            port: config.port,
            bind_address: config.bind_address.clone(),
            keys: None,
        },
    )?;
    let key = client_key(paths, computer_id)?;
    let private = std::fs::read_to_string(key).map_err(|_| "Could not read the connection key.")?;
    Ok(
        serde_json::json!({"privateKey":private,"port":config.port,"address":config.bind_address,"user":user}),
    )
}

/// Authorize another device's own SSH key for one computer and tell it where to
/// connect (C-15). The connecting device generates and keeps its private key; only
/// its public key crosses devices, tagged with that device's Silo identity so a
/// new key replaces its previous one. Turning SSH access off revokes every such key.
fn authorize_controller(
    paths: &RuntimePaths,
    computer_id: &str,
    request: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    uuid::Uuid::parse_str(computer_id).map_err(|_| "Invalid computer identity.")?;
    let config = read(paths)?
        .into_iter()
        .find(|c| c.computer_id == computer_id && c.enabled)
        .ok_or("Enable SSH access first.")?;
    let user = crate::working_account::inspect_user(paths, &config.computer)?;
    crate::working_account::require_client_protocol(request)?;
    let (Some(public), Some(controller)) = (
        request["publicKey"].as_str(),
        request["controllerId"].as_str(),
    ) else {
        return Err("Update Silo on the connecting device to connect over SSH.".into());
    };
    editor::validate_public_key(public)?;
    uuid::Uuid::parse_str(controller).map_err(|_| "Invalid device identity.")?;
    let entry = format!("{public} {CONTROLLER_TAG}{controller}");
    let mut keys = config.keys;
    if !keys.contains(&entry) {
        let tag = format!("{CONTROLLER_TAG}{controller}");
        keys.retain(|key| key.split_whitespace().nth(2) != Some(tag.as_str()));
        if !keys
            .iter()
            .any(|key| normalize_key(key).ok().as_deref() == Some(public))
        {
            keys.push(entry);
        }
    }
    save_with(
        paths,
        Target::Id(computer_id),
        Settings {
            enabled: true,
            port: config.port,
            bind_address: config.bind_address.clone(),
            keys: Some(keys),
        },
    )?;
    Ok(serde_json::json!({"port":config.port,"address":config.bind_address,"user":user}))
}

fn available_port(
    configs: &[Configuration],
    bind_address: &str,
    mut can_bind: impl FnMut(&str, u16) -> bool,
) -> Option<u16> {
    (2222..=65535).find(|candidate| {
        !configs.iter().any(|c| c.enabled && c.port == *candidate)
            && can_bind("127.0.0.1", *candidate)
            && (bind_address == "127.0.0.1" || can_bind(bind_address, *candidate))
    })
}

fn save_with(
    paths: &RuntimePaths,
    target: Target<'_>,
    settings: Settings,
) -> Result<State, String> {
    let metadata = runtime::read_metadata(&paths.metadata).map_err(|_| FAILED)?;
    let configuration = metadata
        .computers
        .iter()
        .find(|m| match target {
            Target::Name(name) => m.name() == name,
            Target::Id(id) => m.id() == id,
        })
        .ok_or("This computer no longer exists on its device. Refresh SSH access.")?;
    let computer = configuration.name().to_owned();
    let mut configs = read(paths)?;
    let previous = configs.iter().find(|c| c.computer_id == configuration.id());
    let managed = previous
        .and_then(|c| c.managed_key.clone())
        .or_else(|| managed_public_key(paths, configuration.id()));
    let Settings {
        enabled,
        port,
        bind_address,
        keys,
    } = settings;
    let mut keys = keys.unwrap_or_else(|| previous.map_or_else(Vec::new, |c| c.keys.clone()));
    let managed_key = if enabled {
        let public = editor::public_key(&client_key(paths, configuration.id())?)?;
        if let Some(old) = &managed {
            if old != &public {
                keys.retain(|key| normalize_key(key).ok().as_deref() != Some(old.as_str()));
            }
        }
        if !keys
            .iter()
            .any(|key| normalize_key(key).ok().as_deref() == Some(public.trim()))
        {
            keys.push(public.trim().to_owned());
        }
        Some(public)
    } else {
        // Turning access off revokes every key Silo manages: other devices'
        // registered keys and this device's managed key, which is replaced on the
        // next enable (C-15). Keys the user added stay for next time.
        if managed.is_none() && previous.is_some_and(|c| c.enabled) && !keys.is_empty() {
            return Err("Could not identify the previous SSH connection key. Restore its managed public key file before disabling SSH access.".into());
        }
        keys.retain(|key| {
            !is_controller_key(key)
                && managed.as_deref().is_none_or(|managed| {
                    normalize_key(key).ok().as_deref() != Some(managed.trim())
                })
        });
        None
    };
    let mut config = Configuration {
        computer: computer.clone(),
        computer_id: configuration.id().into(),
        enabled,
        port,
        bind_address,
        keys,
        managed_key,
    };
    validate(&config)?;
    if enabled && config.bind_address != "127.0.0.1" && !addresses().contains(&config.bind_address)
    {
        return Err("Choose an active network interface address.".into());
    }
    // Deleted or replaced identities must not reserve a port forever.
    configs.retain(|c| {
        metadata
            .computers
            .iter()
            .any(|m| m.name() == c.computer && m.id() == c.computer_id)
    });
    if enabled && port == 2222 && !configs.iter().any(|c| c.computer_id == configuration.id()) {
        config.port = available_port(&configs, &config.bind_address, |address, port| {
            std::net::TcpListener::bind((address, port)).is_ok()
        })
        .ok_or("No SSH port is available.")?;
    }
    if enabled
        && configs
            .iter()
            .any(|c| c.enabled && c.computer != computer && c.port == config.port)
    {
        return Err("Another computer already uses this SSH port. Choose a different port.".into());
    }
    if !configs.iter().any(|c| c == &config) {
        configs.retain(|c| c.computer != computer);
        configs.push(config);
        let bytes = serde_json::to_vec(&configs).map_err(|_| FAILED)?;
        if bytes.len() > 1024 * 1024 || configs.len() > 4096 {
            return Err("SSH settings are too large. Remove unused client keys first.".into());
        }
        // Revoke live sessions before committing changed authorization.
        close_computer(&computer);
        editor::write_private(&path(&paths), &bytes)?;
    }
    if !enabled {
        // The revoked managed key is no longer authorized; drop it and this
        // device's connection copy so the next enable creates a fresh one.
        let managed = paths.home.join("ssh/managed-clients");
        for key in [
            managed.join(configuration.id()),
            managed.join(format!("{}.pub", configuration.id())),
            paths.home.join("ssh/connections").join(configuration.id()),
        ] {
            if let Err(error) = std::fs::remove_file(&key) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err("Could not remove the previous SSH connection key.".into());
                }
            }
        }
    }
    reconcile(paths);
    state(paths)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RemoteSave {
    computer_id: String,
    settings: Settings,
}

// Executed by the owning Silo app, never by its short-lived SSH bridge.
pub(crate) fn remote_dispatch(
    paths: &RuntimePaths,
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    crate::remote::ensure_connections_enabled()?;
    if method == "ssh.access.state" {
        // Observation only: report the currently owned listeners without taking the
        // gate or mutating anything, so a remote status read never waits behind a long
        // operation. Any needed listener repair is scheduled in the background, matching
        // the local `read_ssh_access_state` path.
        let result = serde_json::to_value(state(paths)?).map_err(|_| FAILED.to_string());
        schedule_reconcile(paths.clone());
        return result;
    }
    let _operation = crate::updates::operation_guard()?;
    // Remote SSH changes reconcile the shared listeners; wait in order (device).
    let _guard = runtime::OPERATIONS
        .device("Applying remote SSH change")
        .map_err(|e| e.to_string())?;
    runtime::shutdown::ensure_accepting_operations()?;
    let result = remote_with(paths, method, params);
    if method == "ssh.access.save" {
        publish_network_state_changed();
    }
    result
}
fn remote_with(
    paths: &RuntimePaths,
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    if method == "ssh.access.connection" {
        let id = params["computerId"]
            .as_str()
            .ok_or("Missing computer identity.")?;
        return authorize_controller(paths, id, params);
    }
    let result = match method {
        "ssh.access.state" => {
            reconcile(paths);
            state(paths)?
        }
        "ssh.access.save" => {
            let request: RemoteSave = serde_json::from_value(params.clone())
                .map_err(|_| "Invalid remote SSH settings.")?;
            uuid::Uuid::parse_str(&request.computer_id)
                .map_err(|_| "Invalid computer identity.")?;
            save_with(paths, Target::Id(&request.computer_id), request.settings)?
        }
        _ => return Err("Unsupported remote SSH operation.".into()),
    };
    serde_json::to_value(result).map_err(|_| FAILED.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrated_ssh_access_settings_load() {
        let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
        let configs = read(&migrated.runtime_paths()).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].computer, "dev");
        assert_eq!(
            configs[0].computer_id,
            crate::runtime_migration::vocabulary_tests::ID
        );
        assert_eq!(configs[0].port, 2222);
    }
    use std::{
        fs,
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        os::unix::fs::PermissionsExt,
        time::Instant,
    };
    fn config() -> Configuration {
        use base64::Engine;
        let mut bytes = b"\x00\x00\x00\x0bssh-ed25519\x00\x00\x00\x20".to_vec();
        bytes.extend_from_slice(&[1; 32]);
        Configuration {
            computer: "dev".into(),
            computer_id: "00000000-0000-4000-8000-000000000001".into(),
            enabled: true,
            port: 2222,
            bind_address: "127.0.0.1".into(),
            keys: vec![format!(
                "ssh-ed25519 {} test laptop",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )],
            managed_key: None,
        }
    }
    fn paths(dir: &tempfile::TempDir) -> RuntimePaths {
        crate::test_support::paths(dir.path())
    }
    #[test]
    fn rejects_wildcards_invalid_ports_keys_and_duplicate_key_comments() {
        let _test_state = crate::test_support::global_state();
        let mut c = config();
        validate(&c).unwrap();
        for ip in [
            "0.0.0.0",
            "::",
            "localhost",
            "224.0.0.1",
            "255.255.255.255",
            "127.0.0.2",
            "1.2.3.999",
        ] {
            c.bind_address = ip.into();
            assert!(validate(&c).is_err(), "{ip}");
        }
        c.bind_address = "192.168.1.20".into();
        validate(&c).unwrap();
        c.port = 0;
        assert!(validate(&c).is_err());
        c.port = 65535;
        c.keys
            .push(c.keys[0].replace("test laptop", "different comment"));
        assert!(validate(&c).is_err());
        for key in [
            "command=\"sh\" ssh-ed25519 AAAA",
            "ssh-ed25519 AAAA",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "ssh-ed25519 AAAA\nssh-ed25519 AAAA",
        ] {
            c.keys = vec![key.into()];
            assert!(validate(&c).is_err());
        }
    }
    #[test]
    fn persists_identity_and_disabled_settings_without_touching_internal_keys() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        fs::create_dir_all(p.home.join("ssh")).unwrap();
        let internal = p.home.join("ssh/authorized_keys");
        fs::write(&internal, "Silo internal sentinel\n").unwrap();
        let mut c = config();
        c.enabled = false;
        editor::write_private(&path(&p), &serde_json::to_vec(&vec![c.clone()]).unwrap()).unwrap();
        assert_eq!(read(&p).unwrap(), vec![c]);
        assert_eq!(
            fs::read_to_string(internal).unwrap(),
            "Silo internal sentinel\n"
        );
        assert_eq!(
            fs::metadata(path(&p)).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::write(path(&p), "{broken").unwrap();
        assert!(read(&p).is_err());
    }
    #[test]
    fn refuses_symlinked_settings() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        let target = dir.path().join("target");
        fs::write(&target, "[]").unwrap();
        std::os::unix::fs::symlink(&target, path(&p)).unwrap();
        assert!(read(&p).is_err());
        assert!(editor::write_private(&path(&p), b"[]").is_err());
    }
    // A deterministic runtime substitute validates actual TCP ownership, stream
    // cleanup and command arguments. It never starts or accesses a real computer.
    fn fake_runtime(p: &RuntimePaths) {
        fs::write(
            &p.executable,
            r#"#!/usr/bin/python3
import socket, sys, threading
args = sys.argv[1:]
if args == ['--silo-working-account-protocol']:
    print('1')
    sys.exit(0)
assert args[:3] == ['ssh', 'serve', 'dev']
for flag in ['--no-start', '--no-inactivity-timeout', '--exit-on-stdin-close', '--authorized-keys', '--expected-machine-id']:
    assert flag in args
assert args[args.index('--expected-machine-id')+1] == '00000000-0000-4000-8000-000000000001'
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind((args[args.index('--host')+1], int(args[args.index('--port')+1])))
s.listen()
print('SILO_SSH_READY', flush=True)
def echo():
    while True:
        conn, _ = s.accept()
        def session(c):
            with c:
                while True:
                    b = c.recv(100)
                    if not b: return
                    c.sendall(b)
        threading.Thread(target=session, args=(conn,), daemon=True).start()
threading.Thread(target=echo, daemon=True).start()
sys.stdin.buffer.read()
"#,
        )
        .unwrap();
        fs::set_permissions(&p.executable, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fn unused_port() -> u16 {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }
    #[test]
    fn listener_drop_closes_socket_and_sessions_and_preserves_internal_authorization() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        fake_runtime(&p);
        fs::create_dir_all(p.home.join("ssh")).unwrap();
        fs::write(p.home.join("ssh/authorized_keys"), "internal sentinel").unwrap();
        let mut c = config();
        c.port = unused_port();
        let listener = spawn(&p, &c).unwrap();
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, c.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.write_all(b"alive").unwrap();
        let mut bytes = [0; 5];
        stream.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"alive");
        drop(listener);
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, c.port)).is_err());
        assert!(stream.read(&mut bytes).map_or(true, |n| n == 0));
        assert_eq!(
            fs::read_to_string(p.home.join("ssh/authorized_keys")).unwrap(),
            "internal sentinel"
        );
        assert_eq!(
            fs::read_to_string(p.home.join(format!(
                "ssh/managed-access/{}.authorized_keys",
                c.computer_id
            )))
            .unwrap(),
            format!("{}\n", c.keys.join("\n"))
        );
    }
    #[test]
    fn closing_owner_pipe_exits_child_without_drop_cleanup() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        fake_runtime(&p);
        let mut c = config();
        c.port = unused_port();
        let mut listener = spawn(&p, &c).unwrap();
        drop(listener.children[0].stdin.take());
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            if listener.children[0].try_wait().unwrap().is_some() {
                break;
            }
            assert!(Instant::now() < until, "child survived owner pipe EOF");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, c.port)).is_err());
    }
    #[test]
    fn occupied_port_never_reports_an_unrelated_listener_as_ready() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        fake_runtime(&p);
        let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut c = config();
        c.port = occupied.local_addr().unwrap().port();
        assert!(spawn(&p, &c).is_err());
        assert!(TcpStream::connect(occupied.local_addr().unwrap()).is_ok());
    }
    #[test]
    fn reconciliation_never_launches_stopped_disabled_unverified_or_keyless_computers() {
        let _test_state = crate::test_support::global_state();
        let mut owned = Owners::default();
        let mut c = config();
        for observed in [Ok(false), Err("status unavailable".into())] {
            reconcile_one(&mut owned, &c, observed, &[], || panic!("must not start"));
            assert!(owned.listeners.is_empty());
        }
        c.enabled = false;
        reconcile_one(&mut owned, &c, Ok(true), &[], || panic!("disabled"));
        c.enabled = true;
        c.keys.clear();
        reconcile_one(&mut owned, &c, Ok(true), &[], || {
            panic!("no authorized keys")
        });
        assert!(owned.errors["dev"].contains("public key"));
        c = config();
        c.bind_address = "192.168.50.2".into();
        reconcile_one(&mut owned, &c, Ok(true), &[], || {
            panic!("missing interface")
        });
        assert!(owned.errors["dev"].contains("unavailable"));
    }
    #[test]
    fn reconciliation_reuses_live_listener_restarts_on_keys_and_revokes_on_stop() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        fake_runtime(&p);
        let mut c = config();
        c.port = unused_port();
        let mut owned = Owners::default();
        reconcile_one(&mut owned, &c, Ok(true), &[], || spawn(&p, &c));
        let initial = owned.listeners["dev"].children[0].id();
        reconcile_one(&mut owned, &c, Ok(true), &[], || {
            panic!("unchanged listener")
        });
        assert_eq!(owned.listeners["dev"].children[0].id(), initial);
        let mut session = TcpStream::connect((Ipv4Addr::LOCALHOST, c.port)).unwrap();
        session
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        use base64::Engine;
        let mut key_bytes = base64::engine::general_purpose::STANDARD
            .decode(c.keys[0].split_whitespace().nth(1).unwrap())
            .unwrap();
        *key_bytes.last_mut().unwrap() = 2;
        c.keys[0] = format!(
            "ssh-ed25519 {} replacement client",
            base64::engine::general_purpose::STANDARD.encode(key_bytes)
        );
        reconcile_one(&mut owned, &c, Ok(true), &[], || spawn(&p, &c));
        assert!(owned.errors.is_empty(), "{:?}", owned.errors);
        assert_ne!(owned.listeners["dev"].children[0].id(), initial);
        assert!(session.read(&mut [0]).map_or(true, |n| n == 0));
        reconcile_one(&mut owned, &c, Ok(false), &[], || panic!("stopped"));
        assert!(owned.listeners.is_empty());
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, c.port)).is_err());
    }
    #[test]
    fn network_listener_failure_rolls_back_the_ready_loopback_listener() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        fake_runtime(&p);
        let script = fs::read_to_string(&p.executable).unwrap().replace(
            "s = socket.socket()",
            "if args[args.index('--host')+1] != '127.0.0.1': sys.exit(1)\ns = socket.socket()",
        );
        fs::write(&p.executable, script).unwrap();
        let mut c = config();
        c.port = unused_port();
        c.bind_address = "192.0.2.1".into();
        assert!(spawn(&p, &c).is_err());
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, c.port)).is_err());
    }
    fn remote_fixture(p: &RuntimePaths) {
        fake_runtime(p);
        fs::create_dir_all(&p.home).unwrap();
        let script = fs::read_to_string(&p.executable).unwrap().replace(
            "args = sys.argv[1:]",
            "args = sys.argv[1:]\nif args[0] == 'inspect':\n    import json, os, pathlib\n    running = (pathlib.Path(os.environ['MSB_HOME'])/'running').exists()\n    print(json.dumps({'name':'dev','status':'Running' if running else 'Stopped','config':{'labels':{'silo.managed':'true','silo.machine-id':'00000000-0000-4000-8000-000000000001'}}}))\n    sys.exit(0)",
        );
        fs::write(&p.executable, script).unwrap();
        fs::write(&p.library, "fixture").unwrap();
        fs::write(
            &p.metadata,
            serde_json::json!({"schemaVersion":1,"computers":[{
                "id":config().computer_id,"name":"dev","cpus":2,"maxCPUs":2,
                "memoryGiB":2,"maxMemoryGiB":2,"workspaceStorageGiB":10,"runtimeStorageGiB":10
            }]})
            .to_string(),
        )
        .unwrap();
    }
    fn remote_request(c: &Configuration) -> serde_json::Value {
        serde_json::json!({"computerId":c.computer_id,"settings":{
            "enabled":c.enabled,"port":c.port,"bindAddress":c.bind_address,"keys":c.keys
        }})
    }
    #[test]
    fn working_account_outdated_runtime_closes_listener_before_reuse_or_binding() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        remote_fixture(&p);
        fs::write(p.home.join("running"), "running").unwrap();
        let script = fs::read_to_string(&p.executable)
            .unwrap()
            .replace("print('1')", "print('0')");
        fs::write(&p.executable, script).unwrap();
        let c = config();
        let mut owned = Owners::default();
        owned.listeners.insert(
            c.computer.clone(),
            Listener {
                children: vec![],
                config: c.clone(),
            },
        );
        for _ in 0..2 {
            let observed = inspect_running(&p, &c);
            assert_eq!(
                observed.as_ref().unwrap_err(),
                "The bundled runtime cannot open this computer's Linux account. Relaunch Silo to rerun system checks, then repair or update Silo."
            );
            reconcile_one(&mut owned, &c, observed, &["127.0.0.1".into()], || {
                panic!("unsafe runtime must not bind a listener")
            });
            assert!(!owned.listeners.contains_key(&c.computer));
            assert_eq!(
                owned.errors[&c.computer],
                "The bundled runtime cannot open this computer's Linux account. Relaunch Silo to rerun system checks, then repair or update Silo."
            );
        }
    }

    #[test]
    fn working_account_remote_export_requires_protocol_before_creating_client_key() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        remote_fixture(&p);
        let c = config();
        editor::write_private(&path(&p), &serde_json::to_vec(&vec![c.clone()]).unwrap()).unwrap();
        let (public, controller) = controller_identity(&dir, "laptop");
        let request = serde_json::json!({"computerId":c.computer_id,"publicKey":public,"controllerId":controller});
        let error = remote_with(&p, "ssh.access.connection", &request).unwrap_err();
        assert!(error.contains("Update Silo on the connecting device"));
        assert!(!p.home.join("ssh/managed-clients").exists());
        let request = serde_json::json!({"computerId":c.computer_id,"accountProtocol":1,"publicKey":public,"controllerId":controller});
        let connection = remote_with(&p, "ssh.access.connection", &request).unwrap();
        assert_eq!(connection["user"], "silo");
        assert!(connection.get("privateKey").is_none());
    }

    /// A connecting device's own key pair and identity. Only the public key is sent.
    fn controller_identity(dir: &tempfile::TempDir, name: &str) -> (String, String) {
        let key = dir.path().join(format!("controller-{name}"));
        editor::key(&key).unwrap();
        (
            editor::public_key(&key).unwrap(),
            uuid::Uuid::new_v4().to_string(),
        )
    }
    fn connect(
        p: &RuntimePaths,
        c: &Configuration,
        public: &str,
        controller: &str,
    ) -> Result<serde_json::Value, String> {
        remote_with(
            p,
            "ssh.access.connection",
            &serde_json::json!({
                "computerId":c.computer_id,"accountProtocol":1,"publicKey":public,"controllerId":controller
            }),
        )
    }

    #[test]
    fn remote_connections_authorize_each_devices_own_key_and_never_send_a_private_key() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        remote_fixture(&p);
        let mut c = config();
        c.port = unused_port();
        remote_with(&p, "ssh.access.save", &remote_request(&c)).unwrap();
        let (laptop, laptop_id) = controller_identity(&dir, "laptop");
        let connection = connect(&p, &c, &laptop, &laptop_id).unwrap();
        assert!(
            !connection.to_string().contains("PRIVATE KEY"),
            "{connection}"
        );
        assert_eq!(connection["port"], c.port);
        assert_eq!(connection["address"], "127.0.0.1");
        let tagged = |public: &str, id: &str| format!("{public} {CONTROLLER_TAG}{id}");
        assert!(read(&p).unwrap()[0]
            .keys
            .contains(&tagged(&laptop, &laptop_id)));
        // Asking again with the same key changes nothing.
        let before = read(&p).unwrap();
        connect(&p, &c, &laptop, &laptop_id).unwrap();
        assert_eq!(read(&p).unwrap(), before);
        // A second device gets its own entry; a new key from the first replaces its old one.
        let (desk, desk_id) = controller_identity(&dir, "desk");
        connect(&p, &c, &desk, &desk_id).unwrap();
        let (replacement, _) = controller_identity(&dir, "laptop-2");
        connect(&p, &c, &replacement, &laptop_id).unwrap();
        let keys = read(&p).unwrap()[0].keys.clone();
        assert!(keys.contains(&tagged(&desk, &desk_id)));
        assert!(keys.contains(&tagged(&replacement, &laptop_id)));
        assert!(!keys.iter().any(|key| key.starts_with(&laptop)));
        // The user's own key and the owner's managed key are untouched.
        assert!(keys.contains(&c.keys[0]));
        assert_eq!(keys.len(), 4);
        for invalid in [
            serde_json::json!({"computerId":c.computer_id,"accountProtocol":1}),
            serde_json::json!({"computerId":c.computer_id,"accountProtocol":1,"publicKey":format!("{laptop} comment"),"controllerId":laptop_id}),
            serde_json::json!({"computerId":c.computer_id,"accountProtocol":1,"publicKey":laptop,"controllerId":"laptop"}),
        ] {
            assert!(
                remote_with(&p, "ssh.access.connection", &invalid).is_err(),
                "{invalid}"
            );
        }
        assert_eq!(read(&p).unwrap()[0].keys, keys);
    }

    #[test]
    fn endpoint_saves_without_keys_preserve_registered_controllers() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        remote_fixture(&p);
        let mut c = config();
        c.port = unused_port();
        remote_with(&p, "ssh.access.save", &remote_request(&c)).unwrap();
        let (public, controller) = controller_identity(&dir, "laptop");
        connect(&p, &c, &public, &controller).unwrap();
        let before = read(&p).unwrap()[0].keys.clone();
        let mut request = remote_request(&c);
        request["settings"].as_object_mut().unwrap().remove("keys");
        request["settings"]["port"] = serde_json::json!(unused_port());
        remote_with(&p, "ssh.access.save", &request).unwrap();
        assert_eq!(read(&p).unwrap()[0].keys, before);
        request["settings"]["enabled"] = serde_json::json!(false);
        remote_with(&p, "ssh.access.save", &request).unwrap();
        assert_eq!(read(&p).unwrap()[0].keys, c.keys);
        request["settings"]["enabled"] = serde_json::json!(true);
        remote_with(&p, "ssh.access.save", &request).unwrap();
        connect(&p, &c, &public, &controller).unwrap();
        request["settings"]["keys"] = serde_json::json!([]);
        remote_with(&p, "ssh.access.save", &request).unwrap();
        assert!(!read(&p).unwrap()[0]
            .keys
            .iter()
            .any(|key| key.starts_with(&public)));
    }

    #[test]
    fn initial_port_skips_an_occupied_network_binding() {
        assert_eq!(
            available_port(&[], "192.0.2.1", |address, port| {
                address != "192.0.2.1" || port != 2222
            }),
            Some(2223)
        );
        let mut reserved = config();
        reserved.port = 2223;
        assert_eq!(
            available_port(&[reserved], "192.0.2.1", |address, port| {
                address != "192.0.2.1" || port != 2222
            }),
            Some(2224)
        );
    }

    #[test]
    fn controller_registration_preserves_matching_user_key() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        remote_fixture(&p);
        let (public, controller) = controller_identity(&dir, "laptop");
        let mut c = config();
        c.port = unused_port();
        let user_entry = format!("{public} user laptop");
        c.keys.push(user_entry.clone());
        remote_with(&p, "ssh.access.save", &remote_request(&c)).unwrap();
        connect(&p, &c, &public, &controller).unwrap();
        assert!(read(&p).unwrap()[0].keys.contains(&user_entry));
        let (replacement, _) = controller_identity(&dir, "replacement");
        connect(&p, &c, &replacement, &controller).unwrap();
        let mut disabled = read(&p).unwrap()[0].clone();
        disabled.enabled = false;
        remote_with(&p, "ssh.access.save", &remote_request(&disabled)).unwrap();
        assert_eq!(read(&p).unwrap()[0].keys, c.keys);
        let mut reenabled = read(&p).unwrap()[0].clone();
        reenabled.enabled = true;
        remote_with(&p, "ssh.access.save", &remote_request(&reenabled)).unwrap();
        assert!(read(&p).unwrap()[0].keys.contains(&user_entry));
        assert!(!read(&p).unwrap()[0]
            .keys
            .iter()
            .any(|key| key.starts_with(&replacement)));
    }

    #[test]
    fn disabling_ssh_access_revokes_managed_key_when_key_files_are_missing() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        remote_fixture(&p);
        let mut c = config();
        c.port = unused_port();
        remote_with(&p, "ssh.access.save", &remote_request(&c)).unwrap();
        let key = client_key(&p, &c.computer_id).unwrap();
        let managed = editor::public_key(&key).unwrap();
        fs::remove_file(&key).unwrap();
        fs::remove_file(key.with_extension("pub")).unwrap();
        let mut disabled = read(&p).unwrap()[0].clone();
        disabled.enabled = false;
        remote_with(&p, "ssh.access.save", &remote_request(&disabled)).unwrap();
        assert_eq!(read(&p).unwrap()[0].keys, c.keys);
        let mut reenabled = read(&p).unwrap()[0].clone();
        reenabled.enabled = true;
        remote_with(&p, "ssh.access.save", &remote_request(&reenabled)).unwrap();
        let rotated = editor::public_key(&client_key(&p, &c.computer_id).unwrap()).unwrap();
        assert_ne!(rotated, managed);
        assert!(!read(&p).unwrap()[0].keys.contains(&managed));
    }

    #[test]
    fn disabling_legacy_ssh_access_uses_surviving_public_key() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        remote_fixture(&p);
        let mut c = config();
        c.port = unused_port();
        remote_with(&p, "ssh.access.save", &remote_request(&c)).unwrap();
        let key = client_key(&p, &c.computer_id).unwrap();
        let public = fs::read(key.with_extension("pub")).unwrap();
        fs::remove_file(&key).unwrap();
        fs::remove_file(key.with_extension("pub")).unwrap();
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(path(&p)).unwrap()).unwrap();
        legacy[0].as_object_mut().unwrap().remove("managedKey");
        editor::write_private(&path(&p), &serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut disabled = read(&p).unwrap()[0].clone();
        disabled.enabled = false;
        let error = remote_with(&p, "ssh.access.save", &remote_request(&disabled)).unwrap_err();
        assert!(
            error.contains("Restore its managed public key file"),
            "{error}"
        );
        assert!(read(&p).unwrap()[0].enabled);
        fs::write(key.with_extension("pub"), public).unwrap();
        remote_with(&p, "ssh.access.save", &remote_request(&disabled)).unwrap();
        assert_eq!(read(&p).unwrap()[0].keys, c.keys);
    }

    #[test]
    fn disabling_ssh_access_revokes_device_keys_and_rotates_the_managed_key() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        remote_fixture(&p);
        let mut c = config();
        c.port = unused_port();
        remote_with(&p, "ssh.access.save", &remote_request(&c)).unwrap();
        let (laptop, laptop_id) = controller_identity(&dir, "laptop");
        connect(&p, &c, &laptop, &laptop_id).unwrap();
        let managed = editor::public_key(&client_key(&p, &c.computer_id).unwrap()).unwrap();
        connection_material(&p, &c.computer_id).unwrap();
        // Disable with every key the state reported, as the app does.
        let mut disabled = read(&p).unwrap()[0].clone();
        disabled.enabled = false;
        remote_with(&p, "ssh.access.save", &remote_request(&disabled)).unwrap();
        assert_eq!(
            read(&p).unwrap()[0].keys,
            c.keys,
            "only the user's own keys survive"
        );
        assert!(!p
            .home
            .join("ssh/managed-clients")
            .join(&c.computer_id)
            .exists());
        remote_with(&p, "ssh.access.save", &remote_request(&c)).unwrap();
        let rotated = editor::public_key(&client_key(&p, &c.computer_id).unwrap()).unwrap();
        assert_ne!(rotated, managed);
        let keys = read(&p).unwrap()[0].keys.clone();
        assert!(!keys
            .iter()
            .any(|key| key.starts_with(&managed) || key.starts_with(&laptop)));
    }

    #[test]
    fn automatic_client_key_is_stable_isolated_and_exported_only_on_request() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        remote_fixture(&p);
        let internal = p.home.join("ssh/authorized_keys");
        fs::create_dir_all(internal.parent().unwrap()).unwrap();
        fs::write(&internal, "internal sentinel").unwrap();
        let mut c = config();
        c.keys.clear();
        c.port = unused_port();
        let request = remote_request(&c);
        let first = remote_with(&p, "ssh.access.save", &request).unwrap();
        assert_eq!(first["computers"][0]["keys"].as_array().unwrap().len(), 1);
        assert!(!first.to_string().contains("PRIVATE KEY"));
        let key_path = client_key(&p, &c.computer_id).unwrap();
        let first_key = fs::read(&key_path).unwrap();
        assert_eq!(
            fs::metadata(&key_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let second = remote_with(&p, "ssh.access.save", &request).unwrap();
        assert_eq!(
            first["computers"][0]["keys"],
            second["computers"][0]["keys"]
        );
        assert_eq!(first_key, fs::read(&key_path).unwrap());
        // Only a local connection reads the managed private key; it never crosses devices.
        let exported = connection_material(&p, &c.computer_id).unwrap();
        assert!(exported["privateKey"]
            .as_str()
            .unwrap()
            .contains("BEGIN OPENSSH PRIVATE KEY"));
        assert_eq!(fs::read_to_string(internal).unwrap(), "internal sentinel");
        assert!(!p.home.join("running").exists());
        c.enabled = false;
        remote_with(&p, "ssh.access.save", &remote_request(&c)).unwrap();
        assert!(connection_material(&p, &c.computer_id).is_err());
        assert!(client_key(&p, "../../escape").is_err());
    }

    #[test]
    fn remote_enable_waits_for_owner_start_and_retries_preserve_sessions_until_remote_disable() {
        let _test_state = crate::test_support::global_state();
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                close_computer("dev");
            }
        }
        let _cleanup = Cleanup;
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        remote_fixture(&p);
        let mut c = config();
        c.port = unused_port();
        let request = remote_request(&c);
        let waiting = remote_with(&p, "ssh.access.save", &request).unwrap();
        assert_eq!(waiting["computers"][0]["state"], "waiting");
        assert!(
            !p.home.join("running").exists(),
            "remote toggle must not start a computer"
        );
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, c.port)).is_err());
        fs::write(p.home.join("running"), "").unwrap();
        let response = remote_with(&p, "ssh.access.state", &serde_json::json!({})).unwrap();
        assert_eq!(response["computers"][0]["computerId"], c.computer_id);
        assert_eq!(response["computers"][0]["state"], "listening");
        drop(response); // Returning/dropping a controller reply does not own the listener.
        let mut session = TcpStream::connect((Ipv4Addr::LOCALHOST, c.port)).unwrap();
        session
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        remote_with(&p, "ssh.access.save", &request).unwrap();
        session.write_all(b"still alive").unwrap();
        let mut bytes = [0; 11];
        session.read_exact(&mut bytes).unwrap();
        assert_eq!(
            &bytes, b"still alive",
            "identical remote save must not restart SSH"
        );
        c.enabled = false;
        let result = remote_with(&p, "ssh.access.save", &remote_request(&c)).unwrap();
        assert_eq!(result["computers"][0]["state"], "disabled");
        assert!(session.read(&mut [0]).map_or(true, |n| n == 0));
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, c.port)).is_err());
        assert_eq!(read(&p).unwrap()[0].keys, c.keys);
    }
    /// A running `dev` with SSH access enabled, whose listener takes `delay` to
    /// become ready, like a runtime under load.
    fn slow_listener_fixture(p: &RuntimePaths, delay: &str) -> Configuration {
        remote_fixture(p);
        let script = fs::read_to_string(&p.executable).unwrap().replace(
            "print('SILO_SSH_READY', flush=True)",
            &format!("import time; time.sleep({delay})\nprint('SILO_SSH_READY', flush=True)"),
        );
        fs::write(&p.executable, script).unwrap();
        fs::write(p.home.join("running"), "").unwrap();
        let mut c = config();
        c.port = unused_port();
        editor::write_private(&path(p), &serde_json::to_vec(&vec![c.clone()]).unwrap()).unwrap();
        c
    }
    struct CloseDev;
    impl Drop for CloseDev {
        fn drop(&mut self) {
            close_computer("dev");
        }
    }
    #[test]
    fn state_and_closes_never_wait_for_listener_start_up() {
        let _test_state = crate::test_support::global_state();
        let _cleanup = CloseDev;
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        let c = slow_listener_fixture(&p, "2.5");
        let pass = {
            let p = p.clone();
            std::thread::spawn(move || reconcile(&p))
        };
        // Let the pass inspect the computer and begin starting the listener.
        std::thread::sleep(Duration::from_millis(600));
        let start = Instant::now();
        let observed = state(&p).unwrap();
        close_computer("other");
        let waited = start.elapsed();
        pass.join().unwrap();
        assert!(
            waited < Duration::from_millis(1000),
            "state waited {waited:?} for listener start-up"
        );
        assert_eq!(observed.computers[0].state, "waiting");
        assert_eq!(state(&p).unwrap().computers[0].state, "listening");
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, c.port)).is_ok());
    }
    #[test]
    fn a_close_during_listener_start_up_discards_the_new_listener() {
        let _test_state = crate::test_support::global_state();
        let _cleanup = CloseDev;
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        let c = slow_listener_fixture(&p, "1");
        let pass = {
            let p = p.clone();
            std::thread::spawn(move || reconcile(&p))
        };
        std::thread::sleep(Duration::from_millis(500));
        // The computer is being stopped: its sessions are revoked before the stop.
        close_computer("dev");
        pass.join().unwrap();
        assert!(!owners().lock().unwrap().listeners.contains_key("dev"));
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, c.port)).is_err());
    }
    #[test]
    fn background_checks_skip_the_gate_and_runtime_when_nothing_is_configured() {
        let _test_state = crate::test_support::global_state();
        close_all();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        remote_fixture(&p);
        assert!(!needs_reconcile(&p));
        let mut c = config();
        c.enabled = false;
        editor::write_private(&path(&p), &serde_json::to_vec(&vec![c.clone()]).unwrap()).unwrap();
        assert!(!needs_reconcile(&p));
        c.enabled = true;
        editor::write_private(&path(&p), &serde_json::to_vec(&vec![c]).unwrap()).unwrap();
        assert!(needs_reconcile(&p));
        fs::write(path(&p), "{broken").unwrap();
        assert!(
            needs_reconcile(&p),
            "unreadable settings must still close listeners"
        );
        assert!(MONITOR_INTERVAL >= Duration::from_secs(15));
    }
    #[test]
    fn state_read_returns_without_waiting_for_the_operation_gate() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        remote_fixture(&p);
        // An empty settings file makes `read` (and therefore `state`) succeed.
        editor::write_private(&path(&p), b"[]").unwrap();
        // Hold a device guard on another thread for the whole read. `state` must
        // observe without taking the gate, so it returns promptly regardless.
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (held_tx, held_rx) = std::sync::mpsc::channel::<()>();
        let holder = std::thread::spawn(move || {
            let _guard = runtime::OPERATIONS
                .device("Blocking SSH read test")
                .unwrap();
            held_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        held_rx.recv().unwrap();
        let start = Instant::now();
        let result = state(&p);
        let elapsed = start.elapsed();
        release_tx.send(()).unwrap();
        holder.join().unwrap();
        assert!(result.is_ok(), "{:?}", result.err());
        assert!(
            elapsed < Duration::from_secs(2),
            "SSH state read waited for the operation gate: {elapsed:?}"
        );
    }

    #[test]
    fn remote_save_rejects_replaced_ids_and_invalid_settings_without_writing() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let p = paths(&dir);
        remote_fixture(&p);
        let request = remote_request(&config());
        let mut replaced = request.clone();
        replaced["computerId"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
        assert!(remote_with(&p, "ssh.access.save", &replaced)
            .unwrap_err()
            .contains("no longer exists"));
        let mut invalid = request.clone();
        invalid["computerId"] = serde_json::json!("dev");
        assert!(remote_with(&p, "ssh.access.save", &invalid).is_err());
        let mut invalid = request.clone();
        invalid["computer"] = serde_json::json!("dev");
        assert!(remote_with(&p, "ssh.access.save", &invalid).is_err());
        for (field, value) in [
            ("port", serde_json::json!(0)),
            ("bindAddress", serde_json::json!("0.0.0.0")),
            (
                "keys",
                serde_json::json!(["-----BEGIN OPENSSH PRIVATE KEY-----"]),
            ),
        ] {
            let mut invalid = request.clone();
            invalid["settings"][field] = value;
            assert!(remote_with(&p, "ssh.access.save", &invalid).is_err());
        }
        assert!(!path(&p).exists());
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn ssh_access_matches_wire_contract() {
        let states: Vec<State> = ["disabled", "waiting", "listening", "error"]
            .into_iter()
            .map(|status| State {
                computers: vec![Computer {
                    computer: "dev".into(),
                    computer_id: "00000000-0000-4000-8000-000000000001".into(),
                    enabled: status != "disabled",
                    port: 2222,
                    bind_address: "127.0.0.1".into(),
                    keys: if status == "disabled" {
                        vec![]
                    } else {
                        vec![
                            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeContractPublicKeyOnly".into(),
                        ]
                    },
                    state: status,
                    message: (status == "error").then(|| "SSH listener could not start.".into()),
                    fingerprint: (status == "listening")
                        .then(|| "SHA256:contract-public-fingerprint".into()),
                    device_name: "Contract device".into(),
                    addresses: vec!["192.0.2.10".into()],
                    user: "silo",
                }],
            })
            .collect();
        crate::runtime::contract_tests::assert_fixture("ssh-access-state.json", states);
    }
}
