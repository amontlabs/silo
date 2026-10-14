//! Client-owned desktop viewer connections used by the engine API.
//!
//! Tauri keeps its window-label registry in `desktop_viewer`; this registry is keyed by a
//! generated connection id and authenticated client so a Fjord client never shares or closes
//! another client's proxy and SSH tunnel.

use crate::{desktop_proxy::Proxy, owned_tunnel::Tunnel};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, OnceLock},
};

pub const INIT_SCRIPT: &str = include_str!("desktop_viewer_bridge.js");
const MAX_CONNECTIONS: usize = 16;

struct Viewer {
    owner: String,
    device: String,
    computer: String,
    /// The computer's name on its owner. Local runtime commands name computers, not ids.
    name: String,
    /// Whether the owner is this engine's device.
    local: bool,
    proxy: Option<Proxy>,
    tunnel: Option<Tunnel>,
    generation: u64,
    connecting: bool,
}

impl Viewer {
    fn healthy(&mut self) -> bool {
        self.proxy.as_ref().is_some_and(Proxy::running)
            && self.tunnel.as_mut().is_some_and(Tunnel::running)
    }

    fn disconnect(&mut self) -> (Option<Proxy>, Option<Tunnel>) {
        self.generation = self.generation.wrapping_add(1);
        self.connecting = false;
        (self.proxy.take(), self.tunnel.take())
    }
}

pub struct OpenResult {
    pub connection_id: String,
    pub url: String,
    pub cookie_name: String,
    pub cookie_value: String,
    pub name: String,
}

pub enum OpenPlan {
    Existing(OpenResult),
    Connect {
        connection_id: String,
        generation: u64,
        stale: (Option<Proxy>, Option<Tunnel>),
    },
}

pub struct BridgeTicket {
    pub generation: u64,
    pub inbox: Arc<crate::desktop_bridge::Inbox>,
}

fn entries() -> &'static Mutex<HashMap<String, Viewer>> {
    static ENTRIES: OnceLock<Mutex<HashMap<String, Viewer>>> = OnceLock::new();
    ENTRIES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The clients whose session is open. A connection can only be reserved for one of them, so
/// a client that disconnected while its open was still reading credentials cannot get a
/// viewer afterwards: nothing would ever close it.
fn sessions() -> &'static Mutex<HashSet<String>> {
    static SESSIONS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Marks the session of client `owner` as open. Its connections are claimed from now until
/// [`close_client`].
pub fn open_client(owner: &str) {
    sessions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(owner.to_owned());
}

fn open_result(connection_id: &str, viewer: &Viewer, name: &str) -> Result<OpenResult, String> {
    let proxy = viewer
        .proxy
        .as_ref()
        .ok_or("Desktop connection is not ready.")?;
    Ok(OpenResult {
        connection_id: connection_id.to_owned(),
        url: format!("http://127.0.0.1:{}/", proxy.port),
        cookie_name: proxy.cookie_name.clone(),
        cookie_value: proxy.token.clone(),
        name: name.to_owned(),
    })
}

/// Reuses a healthy connection for this client and computer, or reserves a fresh generation.
/// No network work or child process cleanup happens while the registry lock is held.
pub fn claim(
    owner: &str,
    device: &str,
    computer: &str,
    name: &str,
    local: bool,
) -> Result<OpenPlan, String> {
    let mut entries = entries().lock().map_err(|_| "Desktop unavailable.")?;
    // Checked while the registry is held: `close_client` ends the session first and
    // closes the connections after, so a reservation made before the session ended is
    // closed with it and none is made after.
    if !sessions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(owner)
    {
        return Err("The client disconnected.".into());
    }
    claim_in(
        &mut entries,
        MAX_CONNECTIONS,
        (owner, device, computer),
        name,
        local,
    )
}

fn claim_in(
    entries: &mut HashMap<String, Viewer>,
    cap: usize,
    (owner, device, computer): (&str, &str, &str),
    name: &str,
    local: bool,
) -> Result<OpenPlan, String> {
    if let Some((id, viewer)) = entries.iter_mut().find(|(_, viewer)| {
        viewer.owner == owner && viewer.device == device && viewer.computer == computer
    }) {
        if viewer.connecting {
            return Err("The desktop is still connecting.".into());
        }
        if viewer.healthy() {
            return open_result(id, viewer, name).map(OpenPlan::Existing);
        }
        let stale = viewer.disconnect();
        viewer.connecting = true;
        return Ok(OpenPlan::Connect {
            connection_id: id.clone(),
            generation: viewer.generation,
            stale,
        });
    }
    if entries.len() >= cap {
        return Err("Close an unused desktop viewer first.".into());
    }
    let connection_id = uuid::Uuid::new_v4().simple().to_string();
    let generation = 1;
    let viewer = Viewer {
        owner: owner.to_owned(),
        device: device.to_owned(),
        computer: computer.to_owned(),
        name: name.to_owned(),
        local,
        proxy: None,
        tunnel: None,
        generation,
        connecting: true,
    };
    entries.insert(connection_id.clone(), viewer);
    Ok(OpenPlan::Connect {
        connection_id,
        generation,
        stale: (None, None),
    })
}

/// Installs the connection only if the client has not closed or replaced this generation.
pub fn finish(
    owner: &str,
    connection_id: &str,
    generation: u64,
    proxy: Proxy,
    tunnel: Tunnel,
    name: &str,
) -> Result<OpenResult, (String, Proxy, Tunnel)> {
    let Ok(mut entries) = entries().lock() else {
        return Err(("Desktop unavailable.".into(), proxy, tunnel));
    };
    let Some(viewer) = entries.get_mut(connection_id).filter(|viewer| {
        viewer.owner == owner && viewer.connecting && viewer.generation == generation
    }) else {
        return Err(("Desktop viewer closed.".into(), proxy, tunnel));
    };
    viewer.connecting = false;
    viewer.proxy = Some(proxy);
    viewer.tunnel = Some(tunnel);
    match open_result(connection_id, viewer, name) {
        Ok(result) => Ok(result),
        Err(error) => {
            let (proxy, tunnel) = viewer.disconnect();
            let Some(proxy) = proxy else {
                unreachable!("just installed a proxy")
            };
            let Some(tunnel) = tunnel else {
                unreachable!("just installed a tunnel")
            };
            Err((error, proxy, tunnel))
        }
    }
}

pub fn abort(owner: &str, connection_id: &str, generation: u64) {
    let stale = entries().lock().ok().and_then(|mut entries| {
        let remove = entries.get(connection_id).is_some_and(|viewer| {
            viewer.owner == owner && viewer.connecting && viewer.generation == generation
        });
        remove.then(|| entries.remove(connection_id)).flatten()
    });
    drop(stale);
}

/// Fails with the unknown-viewer refusal unless `owner` holds `connection_id`. Callers check
/// this before they touch any state keyed by the connection id.
pub fn ensure_owned(owner: &str, connection_id: &str) -> Result<(), String> {
    let entries = entries().lock().map_err(|_| "Desktop unavailable.")?;
    entries
        .get(connection_id)
        .filter(|viewer| viewer.owner == owner)
        .map(|_| ())
        .ok_or_else(|| "Unknown desktop viewer.".into())
}

/// Like [`ensure_owned`], and the connection must be the one for `computer` on `device`: a
/// request that names another computer or device than the viewer was opened for is refused
/// the same way, so a connection id cannot be used to act on a computer it does not show.
pub fn ensure_target(
    owner: &str,
    connection_id: &str,
    device: &str,
    computer: &str,
) -> Result<(), String> {
    let entries = entries().lock().map_err(|_| "Desktop unavailable.")?;
    entries
        .get(connection_id)
        .filter(|viewer| {
            viewer.owner == owner && viewer.device == device && viewer.computer == computer
        })
        .map(|_| ())
        .ok_or_else(|| "Unknown desktop viewer.".into())
}

pub fn bridge_ticket(owner: &str, connection_id: &str) -> Result<BridgeTicket, String> {
    let mut entries = entries().lock().map_err(|_| "Desktop unavailable.")?;
    let viewer = entries
        .get_mut(connection_id)
        .filter(|viewer| viewer.owner == owner)
        .ok_or("Unknown desktop viewer.")?;
    if !viewer.healthy() {
        return Err("The desktop is not connected.".into());
    }
    let proxy = viewer
        .proxy
        .as_ref()
        .ok_or("The desktop is not connected.")?;
    Ok(BridgeTicket {
        generation: viewer.generation,
        inbox: proxy.inbox.clone(),
    })
}

pub fn ticket_is_current(owner: &str, connection_id: &str, ticket: &BridgeTicket) -> bool {
    entries().lock().ok().is_some_and(|mut entries| {
        entries.get_mut(connection_id).is_some_and(|viewer| {
            viewer.owner == owner && viewer.generation == ticket.generation && viewer.healthy()
        })
    })
}

pub fn close(owner: &str, connection_id: &str) -> Result<(), String> {
    let removed = {
        let mut entries = entries().lock().map_err(|_| "Desktop unavailable.")?;
        let viewer = entries
            .get(connection_id)
            .ok_or("Unknown desktop viewer.")?;
        if viewer.owner != owner {
            return Err("Unknown desktop viewer.".into());
        }
        entries.remove(connection_id)
    };
    crate::desktop_viewer_media::forget_viewer(connection_id);
    drop(removed);
    Ok(())
}

fn close_matching(matches: impl Fn(&Viewer) -> bool) -> usize {
    let removed = entries().lock().ok().map(|mut entries| {
        let ids = entries
            .iter()
            .filter(|(_, viewer)| matches(viewer))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in &ids {
            crate::desktop_viewer_media::forget_viewer(id);
        }
        ids.into_iter()
            .filter_map(|id| entries.remove(&id))
            .collect::<Vec<_>>()
    });
    // Tunnels and proxies stop here, after the registry lock is released.
    removed.map_or(0, |removed| removed.len())
}

/// Ends the session of a disconnected client and closes every connection it holds,
/// including one that is still being opened.
pub fn close_client(owner: &str) {
    sessions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(owner);
    close_matching(|viewer| viewer.owner == owner);
}

/// Closes the connections to a computer of this engine's device, named as the runtime names it.
pub fn close_computer(name: &str) {
    close_matching(|viewer| viewer.local && viewer.name == name);
}

/// Closes the connections to the computers of another device.
pub fn close_device(device: &str) {
    close_matching(|viewer| viewer.device == device);
}

pub fn close_all() {
    close_matching(|_| true);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connect(owner: &str, device: &str, computer: &str, name: &str, local: bool) -> String {
        open_client(owner);
        let OpenPlan::Connect {
            connection_id,
            generation,
            ..
        } = claim(owner, device, computer, name, local).unwrap()
        else {
            panic!("expected a fresh connection");
        };
        let proxy =
            Proxy::start("/nonexistent/desktop.sock".into(), 6901, "silo", "secret").unwrap();
        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("60");
        let tunnel = Tunnel::spawn(&command, None).unwrap();
        let opened = finish(owner, &connection_id, generation, proxy, tunnel, name)
            .unwrap_or_else(|(message, _, _)| panic!("{message}"));
        assert_eq!(opened.connection_id, connection_id);
        assert!(opened.url.starts_with("http://127.0.0.1:"));
        assert!(!opened.cookie_value.is_empty());
        connection_id
    }

    #[test]
    fn claims_are_scoped_to_the_client_device_and_computer() {
        open_client("client-a");
        open_client("client-b");
        let first = match claim("client-a", "device-a", "computer-a", "A", false).unwrap() {
            OpenPlan::Connect { connection_id, .. } => connection_id,
            OpenPlan::Existing(_) => unreachable!(),
        };
        let second = match claim("client-b", "device-a", "computer-a", "A", false).unwrap() {
            OpenPlan::Connect { connection_id, .. } => connection_id,
            OpenPlan::Existing(_) => unreachable!(),
        };
        assert_ne!(first, second);
        assert!(claim("client-a", "device-a", "computer-a", "A", false).is_err());
        close_client("client-a");
        close_client("client-b");
    }

    #[test]
    fn a_client_cannot_close_another_clients_viewer() {
        let id = connect("client-c", "device-c", "computer-c", "C", false);
        assert!(close("client-d", &id).is_err());
        assert!(ensure_owned("client-d", &id).is_err());
        assert!(ensure_owned("client-c", &id).is_ok());
        assert!(close("client-c", &id).is_ok());
        assert!(close("client-c", &id).is_err());
    }

    #[test]
    fn each_client_gets_its_own_proxy_and_a_healthy_open_is_idempotent() {
        let first = connect("client-e", "device-e", "computer-e", "E", false);
        let second = connect("client-f", "device-e", "computer-e", "E", false);
        assert_ne!(first, second);
        let ports = [&first, &second].map(|id| {
            let ticket = bridge_ticket(if id == &first { "client-e" } else { "client-f" }, id);
            assert!(ticket.is_ok());
            entries().lock().unwrap()[id].proxy.as_ref().unwrap().port
        });
        assert_ne!(ports[0], ports[1]);
        let OpenPlan::Existing(again) =
            claim("client-e", "device-e", "computer-e", "E", false).unwrap()
        else {
            panic!("a healthy connection is returned again");
        };
        assert_eq!(again.connection_id, first);
        // One client closing leaves the other's viewer working.
        close("client-e", &first).unwrap();
        assert!(bridge_ticket("client-f", &second).is_ok());
        close_client("client-f");
        assert!(bridge_ticket("client-f", &second).is_err());
    }

    #[test]
    fn a_closed_generation_cannot_install_its_late_connection() {
        open_client("client-g");
        let OpenPlan::Connect {
            connection_id,
            generation,
            ..
        } = claim("client-g", "device-g", "computer-g", "G", false).unwrap()
        else {
            panic!("expected a fresh connection");
        };
        close("client-g", &connection_id).unwrap();
        let proxy =
            Proxy::start("/nonexistent/desktop.sock".into(), 6901, "silo", "secret").unwrap();
        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("60");
        let tunnel = Tunnel::spawn(&command, None).unwrap();
        assert!(finish("client-g", &connection_id, generation, proxy, tunnel, "G").is_err());
    }

    #[test]
    fn computers_and_devices_close_only_their_own_viewers() {
        let local = connect("client-h", "device-h", "computer-h", "named-h", true);
        let remote = connect("client-h", "device-i", "computer-i", "named-h", false);
        close_computer("named-h");
        assert!(ensure_owned("client-h", &local).is_err());
        assert!(ensure_owned("client-h", &remote).is_ok());
        close_device("device-i");
        assert!(ensure_owned("client-h", &remote).is_err());
    }

    #[test]
    fn a_client_that_disconnected_before_its_claim_gets_no_connection() {
        // The open read the guest's credentials, the client disconnected meanwhile, and only
        // then the open reached its claim: nothing may be reserved, since nothing would
        // close it, and no capacity slot is used.
        open_client("client-j");
        close_client("client-j");
        let refused = claim("client-j", "device-j", "computer-j", "J", false);
        assert_eq!(refused.err().unwrap(), "The client disconnected.");
        assert!(entries()
            .lock()
            .unwrap()
            .values()
            .all(|viewer| viewer.owner != "client-j"));
        // A client that never had a session is refused the same way.
        assert!(claim("client-never", "device-j", "computer-j", "J", false).is_err());
        // The next session of the same id starts clean.
        open_client("client-j");
        assert!(claim("client-j", "device-j", "computer-j", "J", false).is_ok());
        close_client("client-j");
    }

    #[test]
    fn a_disconnect_between_claim_and_finish_discards_the_late_connection() {
        open_client("client-k");
        let OpenPlan::Connect {
            connection_id,
            generation,
            ..
        } = claim("client-k", "device-k", "computer-k", "K", false).unwrap()
        else {
            panic!("expected a fresh connection");
        };
        close_client("client-k");
        let proxy =
            Proxy::start("/nonexistent/desktop.sock".into(), 6901, "silo", "secret").unwrap();
        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("60");
        let tunnel = Tunnel::spawn(&command, None).unwrap();
        assert!(finish("client-k", &connection_id, generation, proxy, tunnel, "K").is_err());
    }

    #[test]
    fn a_connection_is_only_for_the_computer_and_device_it_was_opened_for() {
        let first = connect("client-l", "device-l", "computer-l1", "L1", false);
        let second = connect("client-l", "device-l", "computer-l2", "L2", false);
        assert!(ensure_target("client-l", &first, "device-l", "computer-l1").is_ok());
        assert!(ensure_target("client-l", &second, "device-l", "computer-l2").is_ok());
        // Another computer, another device, another client or a missing connection.
        assert!(ensure_target("client-l", &first, "device-l", "computer-l2").is_err());
        assert!(ensure_target("client-l", &second, "device-l", "computer-l1").is_err());
        assert!(ensure_target("client-l", &first, "device-m", "computer-l1").is_err());
        assert!(ensure_target("client-m", &first, "device-l", "computer-l1").is_err());
        assert!(ensure_target("client-l", "missing", "device-l", "computer-l1").is_err());
        close_client("client-l");
    }

    #[test]
    fn the_connection_cap_counts_pending_connections() {
        let mut local = HashMap::new();
        for index in 0..3 {
            let owner = format!("client-{index}");
            claim_in(&mut local, 3, (&owner, "device", "computer"), "N", false).unwrap();
        }
        // The three reservations are still connecting, and they already fill the cap.
        let refused = claim_in(
            &mut local,
            3,
            ("client-3", "device", "computer"),
            "N",
            false,
        );
        assert_eq!(
            refused.err().unwrap(),
            "Close an unused desktop viewer first."
        );
        let again = claim_in(
            &mut local,
            3,
            ("client-0", "device", "computer"),
            "N",
            false,
        );
        assert_eq!(again.err().unwrap(), "The desktop is still connecting.");
    }
}
