//! Silo's tools in a computer: the host side of the channel a computer's own agent reaches the
//! engine on (`docs/SiloUI-IN-GUEST-CLIENT.md`). Each computer with the tools on has a port on
//! the device's loopback ([`tokens`]), the one network rule that lets it reach that port
//! ([`crate::network_policy::extra_rules`]), and a file in the computer with the address and its
//! token ([`endpoint`]). `silod` serves the ports; what each call may do is the policy's
//! (`crate::policy`).

pub mod endpoint;
pub mod tokens;

use std::{
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    sync::{Arc, OnceLock},
};

pub use tokens::{now_secs, Minted, OwnedJob, PendingFork, Record, TokenStore, MAX_JOBS};

/// What serves the channels: `silod` installs it at start.
pub trait Channels: Send + Sync {
    /// Serves `computer_id`'s channel on `listener` from now on, replacing any listener it had.
    fn serve(&self, computer_id: &str, listener: TcpListener);
    /// Stops serving `computer_id`'s channel and closes its open connections.
    fn close(&self, computer_id: &str);
    /// Whether `computer_id`'s channel is being served.
    fn serving(&self, computer_id: &str) -> bool;
}

static CHANNELS: OnceLock<Arc<dyn Channels>> = OnceLock::new();

/// Installs what serves the channels. The first installation wins.
pub fn install_channels(channels: Arc<dyn Channels>) {
    let _ = CHANNELS.set(channels);
}

fn channels() -> Option<&'static Arc<dyn Channels>> {
    CHANNELS.get()
}

/// Binds a listener on the device's loopback, on `wanted` when it is free and on a port the
/// system picks otherwise. Never on another address.
pub fn bind(wanted: Option<u16>) -> std::io::Result<(TcpListener, u16)> {
    let at = |port: u16| TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let listener = match wanted.filter(|port| *port != 0).map(at) {
        Some(Ok(listener)) => listener,
        _ => at(0)?,
    };
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Turns Silo's tools on for a computer: keeps the port it has when its channel is served, or
/// binds one and hands it to the channels. The computer's network gets the rule at its next
/// create, start, restore or fork; its endpoint file is written when it next runs.
pub fn turn_on(computer_id: &str) -> Result<Record, String> {
    let store = TokenStore::current().ok_or("The engine is not running.")?;
    turn_on_in(&store, computer_id)
}

/// [`turn_on`] with the store given.
pub fn turn_on_in(store: &TokenStore, computer_id: &str) -> Result<Record, String> {
    let current = store.status(computer_id)?;
    if let (Some(record), Some(channels)) = (&current, channels()) {
        if channels.serving(computer_id) {
            return Ok(record.clone());
        }
    }
    let wanted = current.as_ref().and_then(|record| record.port);
    let (listener, port) = bind(wanted)
        .map_err(|error| format!("Silo could not open a port for the computer's tools: {error}"))?;
    let record = store.enable(computer_id, port)?;
    if let Some(channels) = channels() {
        channels.serve(computer_id, listener);
    }
    Ok(record)
}

/// Turns Silo's tools off for these computers: their tokens stop working and their channels
/// close. The network rule stays until the computer's next start without it.
pub fn turn_off(computer_ids: &[&str]) -> Result<usize, String> {
    // Without an engine there is no store and nothing is served.
    match TokenStore::current() {
        Some(store) => turn_off_in(&store, computer_ids),
        None => Ok(0),
    }
}

/// [`turn_off`] with the store given.
pub fn turn_off_in(store: &TokenStore, computer_ids: &[&str]) -> Result<usize, String> {
    // Silo's MCP server is registered with the agents of every computer that had the tools; it is
    // taken out when the computer next runs (at once for a running one, see `guestClient.revoke`).
    let had: Vec<&str> = computer_ids
        .iter()
        .copied()
        .filter(|id| {
            store
                .status(id)
                .ok()
                .flatten()
                .is_some_and(|record| record.enabled())
        })
        .collect();
    if !had.is_empty() {
        crate::guest_components::owe_removal(store.directory(), &had);
    }
    let removed = store.revoke(computer_ids)?;
    if let Some(channels) = channels() {
        for id in computer_ids {
            channels.close(id);
        }
    }
    Ok(removed)
}

/// A fork has Silo's tools when its source has them, on a port and with a token of its own.
pub fn inherit(source_id: &str, child_id: &str) -> Result<(), String> {
    let Some(store) = TokenStore::current() else {
        return Ok(());
    };
    match store.status(source_id)? {
        Some(_) => turn_on_in(&store, child_id).map(|_| ()),
        None => Ok(()),
    }
}

/// Forgets a removed computer's channel.
pub fn computer_removed(computer_id: &str) {
    if let Err(error) = turn_off(&[computer_id]) {
        eprintln!("silo-engine: {error}");
    }
    if let Some(store) = TokenStore::current() {
        crate::guest_components::forget(store.directory(), computer_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_port_is_on_the_loopback_and_a_taken_one_is_replaced() {
        let (first, port) = bind(None).unwrap();
        assert_eq!(first.local_addr().unwrap().ip(), Ipv4Addr::LOCALHOST);
        assert_ne!(port, 0);
        // While `first` holds the port no bind can have it, whatever else runs at the same time.
        let (second, other) = bind(Some(port)).unwrap();
        assert_ne!(other, port, "a taken port is not shared");
        drop(second);
        // Once it is freed, the port is kept when nothing else took it in the meantime. Another
        // test binding port 0 can be handed the freed number, so a few tries are made; each try
        // that fails only shows that the port was taken, which is the case that falls back.
        drop(first);
        let kept = (0..100).any(|_| {
            let (again, same) = bind(Some(port)).unwrap();
            let kept = same == port;
            drop(again);
            if !kept {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            kept
        });
        assert!(kept, "a free port is kept");
    }
}
