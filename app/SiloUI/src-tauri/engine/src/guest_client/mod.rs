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

pub use tokens::{
    now_secs, HeldPort, Locked, Minted, OwnedJob, PendingFork, Record, TokenStore, MAX_JOBS,
};

/// What serves the channels: `silod` installs it at start. Each port it is given stays bound
/// until [`Channels::release`]: served (the handshake, then a session), or held (every
/// connection accepted and closed at once, nothing read).
pub trait Channels: Send + Sync {
    /// Serves `computer_id`'s channel on `listener` from now on. A port it was served on before
    /// is held.
    fn serve(&self, computer_id: &str, listener: TcpListener) -> Result<(), String>;
    /// Holds `listener`'s port for `computer_id`.
    fn hold(&self, computer_id: &str, listener: TcpListener) -> Result<(), String>;
    /// Serves `computer_id`'s channel on `port`, which is held for it. False when it is not.
    fn serve_held(&self, computer_id: &str, port: u16) -> bool;
    /// Stops serving `computer_id`'s channel and ends its sessions; its port is held.
    fn close(&self, computer_id: &str);
    /// Ends `computer_id`'s open sessions; its channel is still served.
    fn end_sessions(&self, computer_id: &str);
    /// Lets `port` go.
    fn release(&self, port: u16);
    /// The port `computer_id`'s channel is served on.
    fn serving(&self, computer_id: &str) -> Option<u16>;
    /// The computer `port` is bound for, served or held.
    fn bound(&self, port: u16) -> Option<String>;
}

static CHANNELS: OnceLock<Arc<dyn Channels>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static TEST_CHANNELS: std::cell::RefCell<Option<Arc<dyn Channels>>> =
        const { std::cell::RefCell::new(None) };
}

/// Tests on this thread use `channels` instead of the installed ones.
#[cfg(test)]
pub(crate) fn use_test_channels(channels: Option<Arc<dyn Channels>>) {
    TEST_CHANNELS.with(|test| *test.borrow_mut() = channels);
}

/// Installs what serves the channels. The first installation wins.
pub fn install_channels(channels: Arc<dyn Channels>) {
    let _ = CHANNELS.set(channels);
}

pub(crate) fn channels() -> Option<Arc<dyn Channels>> {
    #[cfg(test)]
    if let Some(channels) = TEST_CHANNELS.with(|test| test.borrow().clone()) {
        return Some(channels);
    }
    CHANNELS.get().cloned()
}

fn bind_at(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

/// Binds a listener on the device's loopback, on `wanted` when it is free and on a port the
/// system picks otherwise. Never on another address.
pub fn bind(wanted: Option<u16>) -> std::io::Result<(TcpListener, u16)> {
    let listener = match wanted.filter(|port| *port != 0).map(bind_at) {
        Some(Ok(listener)) => listener,
        _ => bind_at(0)?,
    };
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Binds a listener on the device's loopback on exactly `port`.
pub fn bind_exact(port: u16) -> std::io::Result<TcpListener> {
    if port == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no port",
        ));
    }
    bind_at(port)
}

/// Turns Silo's tools on for a computer: keeps the port it has when its channel is served, or
/// binds one and hands it to the channels. The computer's network gets the rule at its next
/// create, start, restore or fork; its endpoint file is written when it next runs.
pub fn turn_on(computer_id: &str) -> Result<Record, String> {
    let store = TokenStore::current().ok_or("The engine is not running.")?;
    turn_on_in(&store, computer_id)
}

/// [`turn_on`] with the store given. The store stays locked from reading the record to saving
/// it, across binding and serving the port, so two calls cannot leave a record on a port nothing
/// serves. The port the computer had (on, or held since its tools were turned off) is reused
/// when it is still bound for it or free. When another program holds it, the computer's network
/// may still name it: the new port is held, not served, until the computer runs with rules that
/// match. When serving fails, nothing is saved and the call fails.
pub fn turn_on_in(store: &TokenStore, computer_id: &str) -> Result<Record, String> {
    let locked = store.locked();
    let mut records = locked.records()?;
    let mut held = locked.held()?;
    let current = records
        .iter()
        .find(|record| record.computer_id == computer_id && record.enabled())
        .cloned();
    let channels = channels();
    if let Some(record) = &current {
        let Some(channels) = &channels else {
            return Ok(record.clone());
        };
        let bound = record
            .port
            .is_some_and(|port| channels.bound(port).as_deref() == Some(computer_id));
        if bound && (record.restart_needed || channels.serving(computer_id) == record.port) {
            return Ok(record.clone());
        }
    }
    let wanted = current.as_ref().and_then(|record| record.port).or_else(|| {
        held.iter()
            .rev()
            .find(|entry| entry.computer_id == computer_id)
            .map(|entry| entry.port)
    });
    let open = |error: std::io::Error| {
        format!("Silo could not open a port for the computer's tools: {error}")
    };
    let (port, restart_needed, fresh) = match &channels {
        Some(channels)
            if wanted.is_some_and(|port| channels.bound(port).as_deref() == Some(computer_id)) =>
        {
            let port = wanted.unwrap_or_default();
            if !channels.serve_held(computer_id, port) {
                return Err("Silo could not serve the computer's tools.".into());
            }
            (port, false, false)
        }
        Some(channels) => {
            let (listener, port) = bind(wanted).map_err(open)?;
            let moved = wanted.is_some_and(|wanted| wanted != port);
            if moved {
                channels.hold(computer_id, listener)?;
            } else {
                channels.serve(computer_id, listener)?;
            }
            (port, moved, true)
        }
        None => {
            let (_listener, port) = bind(wanted).map_err(open)?;
            (port, false, false)
        }
    };
    let saved = set_port_in(&mut records, &mut held, computer_id, port, restart_needed).and_then(
        |record| {
            locked.save_held(&held)?;
            locked.save(&records)?;
            Ok(record)
        },
    );
    if saved.is_err() {
        if let Some(channels) = &channels {
            channels.close(computer_id);
            if fresh {
                channels.release(port);
            }
        }
    }
    saved
}

fn set_port_in(
    records: &mut Vec<Record>,
    held: &mut Vec<HeldPort>,
    computer_id: &str,
    port: u16,
    restart_needed: bool,
) -> Result<Record, String> {
    tokens::set_port(records, held, computer_id, port)?;
    let record = records
        .iter_mut()
        .find(|record| record.computer_id == computer_id)
        .ok_or("Silo's tools are not on in that computer.")?;
    record.restart_needed |= restart_needed;
    Ok(record.clone())
}

/// Turns Silo's tools off for these computers: their tokens stop working and their sessions
/// end. Each one's port stays held, accepting and closing, since its network keeps the rule
/// until its next start without it.
pub fn turn_off(computer_ids: &[&str]) -> Result<usize, String> {
    // Without an engine there is no store and nothing is served.
    match TokenStore::current() {
        Some(store) => turn_off_in(&store, computer_ids),
        None => Ok(0),
    }
}

/// [`turn_off`] with the store given.
pub fn turn_off_in(store: &TokenStore, computer_ids: &[&str]) -> Result<usize, String> {
    end_tools(store, computer_ids, true)
}

/// Turns Silo's tools off in a closed copy (the temporary copy an export makes): like
/// [`turn_off`], but nothing is owed to the copy and the engine never reconciles it.
pub fn turn_off_copy(computer_id: &str) -> Result<usize, String> {
    match TokenStore::current() {
        Some(store) => end_tools(&store, &[computer_id], false),
        None => Ok(0),
    }
}

fn end_tools(store: &TokenStore, computer_ids: &[&str], owe_removal: bool) -> Result<usize, String> {
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
    if owe_removal && !had.is_empty() {
        crate::guest_components::owe_removal(store.directory(), &had);
    }
    let locked = store.locked();
    let mut records = locked.records()?;
    let mut held = locked.held()?;
    let before = records.len();
    for record in &records {
        if let (true, Some(port)) = (
            computer_ids.contains(&record.computer_id.as_str()),
            record.port,
        ) {
            tokens::hold(&mut held, &record.computer_id, port);
        }
    }
    records.retain(|record| !computer_ids.contains(&record.computer_id.as_str()));
    let removed = before - records.len();
    if removed > 0 {
        locked.save_held(&held)?;
        locked.save(&records)?;
    }
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

/// Forgets a removed computer's channel and lets every port kept for it go.
pub fn computer_removed(computer_id: &str) {
    if let Some(store) = TokenStore::current() {
        if let Err(error) = forget_in(&store, computer_id) {
            eprintln!("silo-engine: {error}");
        }
    }
    if let Some(store) = TokenStore::current() {
        crate::guest_components::forget(store.directory(), computer_id);
    }
}

/// [`computer_removed`] with the store given.
pub fn forget_in(store: &TokenStore, computer_id: &str) -> Result<(), String> {
    let locked = store.locked();
    let mut records = locked.records()?;
    let mut held = locked.held()?;
    let mut ports: Vec<u16> = records
        .iter()
        .filter(|record| record.computer_id == computer_id)
        .filter_map(|record| record.port)
        .collect();
    ports.extend(
        held.iter()
            .filter(|entry| entry.computer_id == computer_id)
            .map(|entry| entry.port),
    );
    records.retain(|record| record.computer_id != computer_id);
    held.retain(|entry| entry.computer_id != computer_id);
    locked.save_held(&held)?;
    locked.save(&records)?;
    if let Some(channels) = channels() {
        channels.close(computer_id);
        for port in ports {
            if channels.bound(port).as_deref() == Some(computer_id) {
                channels.release(port);
            }
        }
    }
    Ok(())
}

/// Ends the token of a computer whose tools are on, so a copy of it (in an export) proves
/// nothing, and ends its sessions. The computer gets a new token when its endpoint file is next
/// checked, which the engine does soon for a running computer without one. Nothing happens when
/// its tools are off.
pub fn end_token(computer_id: &str) -> Result<(), String> {
    let Some(store) = TokenStore::current() else {
        return Ok(());
    };
    let ended = store.update(computer_id, |record| {
        record.hash.clear();
        Ok(())
    })?;
    if let (Some(()), Some(channels)) = (ended, channels()) {
        channels.end_sessions(computer_id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Condvar, Mutex},
        time::Duration,
    };

    use super::*;

    /// Channels that keep their listeners bound and record what they were asked.
    #[derive(Default)]
    struct Fake {
        /// port -> (computer, served)
        ports: Mutex<HashMap<u16, (String, bool, TcpListener)>>,
        ended: Mutex<Vec<String>>,
        fail_serve: Mutex<bool>,
        /// The first serve waits up to this long for a second one.
        first_waits: Mutex<Option<Duration>>,
        serves: Mutex<usize>,
        served_again: Condvar,
    }

    impl Channels for Fake {
        fn serve(&self, computer_id: &str, listener: TcpListener) -> Result<(), String> {
            if *self.fail_serve.lock().unwrap() {
                return Err("the channel could not be served".into());
            }
            let wait = self.first_waits.lock().unwrap().take();
            {
                let mut serves = self.serves.lock().unwrap();
                *serves += 1;
                self.served_again.notify_all();
                if let Some(wait) = wait {
                    let _ = self
                        .served_again
                        .wait_timeout_while(serves, wait, |serves| *serves < 2)
                        .unwrap();
                }
            }
            let port = listener.local_addr().unwrap().port();
            let mut ports = self.ports.lock().unwrap();
            for (computer, served, _) in ports.values_mut() {
                if computer == computer_id {
                    *served = false;
                }
            }
            ports.insert(port, (computer_id.to_owned(), true, listener));
            Ok(())
        }
        fn hold(&self, computer_id: &str, listener: TcpListener) -> Result<(), String> {
            let port = listener.local_addr().unwrap().port();
            self.ports
                .lock()
                .unwrap()
                .insert(port, (computer_id.to_owned(), false, listener));
            Ok(())
        }
        fn serve_held(&self, computer_id: &str, port: u16) -> bool {
            let mut ports = self.ports.lock().unwrap();
            match ports.get_mut(&port) {
                Some((computer, served, _)) if computer == computer_id => {
                    *served = true;
                    true
                }
                _ => false,
            }
        }
        fn close(&self, computer_id: &str) {
            for (computer, served, _) in self.ports.lock().unwrap().values_mut() {
                if computer == computer_id {
                    *served = false;
                }
            }
            self.ended.lock().unwrap().push(computer_id.to_owned());
        }
        fn end_sessions(&self, computer_id: &str) {
            self.ended.lock().unwrap().push(computer_id.to_owned());
        }
        fn release(&self, port: u16) {
            self.ports.lock().unwrap().remove(&port);
        }
        fn serving(&self, computer_id: &str) -> Option<u16> {
            self.ports
                .lock()
                .unwrap()
                .iter()
                .find(|(_, (computer, served, _))| computer == computer_id && *served)
                .map(|(port, _)| *port)
        }
        fn bound(&self, port: u16) -> Option<String> {
            self.ports
                .lock()
                .unwrap()
                .get(&port)
                .map(|(computer, _, _)| computer.clone())
        }
    }

    /// The tests that bind ports run one at a time, so none takes a port another just freed.
    static PORTS: Mutex<()> = Mutex::new(());

    fn ports() -> std::sync::MutexGuard<'static, ()> {
        PORTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn with_fake(directory: &std::path::Path) -> (TokenStore, Arc<Fake>) {
        let fake = Arc::new(Fake::default());
        use_test_channels(Some(fake.clone()));
        tokens::use_test_directory(Some(directory.join("guest-client")));
        (TokenStore::current().unwrap(), fake)
    }

    fn reset() {
        use_test_channels(None);
        tokens::use_test_directory(None);
    }

    #[test]
    fn a_port_is_on_the_loopback_and_a_taken_one_is_replaced() {
        let _ports = ports();
        let (first, port) = bind(None).unwrap();
        assert_eq!(first.local_addr().unwrap().ip(), Ipv4Addr::LOCALHOST);
        assert_ne!(port, 0);
        // While `first` holds the port no bind can have it, whatever else runs at the same time.
        let (second, other) = bind(Some(port)).unwrap();
        assert_ne!(other, port, "a taken port is not shared");
        assert!(bind_exact(port).is_err());
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

    #[test]
    fn two_enables_at_once_leave_one_record_on_the_served_port() {
        let _ports = ports();
        let directory = tempfile::tempdir().unwrap();
        let (store, fake) = with_fake(directory.path());
        *fake.first_waits.lock().unwrap() = Some(Duration::from_millis(400));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let fake = fake.clone();
                let directory = directory.path().to_owned();
                std::thread::spawn(move || {
                    use_test_channels(Some(fake));
                    turn_on_in(&TokenStore::new(&directory), "c").unwrap()
                })
            })
            .collect();
        let records: Vec<Record> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        let port = store.port_of("c").unwrap();
        assert!(records.iter().all(|record| record.port == Some(port)));
        assert_eq!(fake.serving("c"), Some(port), "the record's port is served");
        reset();
    }

    #[test]
    fn when_serving_fails_the_tools_stay_off_and_the_call_fails() {
        let _ports = ports();
        let directory = tempfile::tempdir().unwrap();
        let (store, fake) = with_fake(directory.path());
        *fake.fail_serve.lock().unwrap() = true;
        assert!(turn_on_in(&store, "c").is_err());
        assert!(store.status("c").unwrap().is_none());
        assert!(fake.ports.lock().unwrap().is_empty());
        reset();
    }

    #[test]
    fn turning_off_holds_the_port_and_turning_on_again_serves_it() {
        let _ports = ports();
        let directory = tempfile::tempdir().unwrap();
        let (store, fake) = with_fake(directory.path());
        let port = turn_on_in(&store, "c").unwrap().port.unwrap();
        assert_eq!(fake.serving("c"), Some(port));
        turn_off_in(&store, &["c"]).unwrap();
        assert_eq!(fake.serving("c"), None);
        assert_eq!(
            fake.bound(port).as_deref(),
            Some("c"),
            "the port stays bound"
        );
        assert_eq!(
            store.locked().held().unwrap(),
            vec![HeldPort {
                computer_id: "c".into(),
                port
            }]
        );
        assert_eq!(turn_on_in(&store, "c").unwrap().port, Some(port));
        assert_eq!(fake.serving("c"), Some(port));
        assert!(store.locked().held().unwrap().is_empty());
        forget_in(&store, "c").unwrap();
        assert_eq!(fake.bound(port), None, "a deleted computer's port goes");
        reset();
    }

    #[test]
    fn turning_off_owes_the_registration_removal_and_a_closed_copy_owes_none() {
        use crate::guest_components::receipt::Reports;
        let _ports = ports();
        let directory = tempfile::tempdir().unwrap();
        let (store, fake) = with_fake(directory.path());
        let port = turn_on_in(&store, "c").unwrap().port.unwrap();
        turn_on_in(&store, "copy").unwrap();
        turn_off_in(&store, &["c"]).unwrap();
        end_tools(&store, &["copy"], false).unwrap();
        assert_eq!(Reports::new(store.directory()).owed(), vec!["c".to_owned()]);
        assert_eq!(fake.bound(port).as_deref(), Some("c"), "its port is still held");
        assert!(store.status("copy").unwrap().is_none());
        reset();
    }

    #[test]
    fn a_port_another_program_took_moves_and_waits_for_a_restart() {
        let _ports = ports();
        let directory = tempfile::tempdir().unwrap();
        let (store, fake) = with_fake(directory.path());
        let (taken, port) = bind(None).unwrap();
        store.enable("c", port).unwrap();
        // The record names a port another program holds and the channels do not.
        let record = turn_on_in(&store, "c").unwrap();
        assert_ne!(record.port, Some(port));
        assert!(record.restart_needed);
        assert_eq!(fake.serving("c"), None, "not served until its rules match");
        assert_eq!(fake.bound(record.port.unwrap()).as_deref(), Some("c"));
        drop(taken);
        reset();
    }

    #[test]
    fn minting_a_token_again_ends_the_computers_sessions() {
        let _ports = ports();
        let directory = tempfile::tempdir().unwrap();
        let (store, fake) = with_fake(directory.path());
        turn_on_in(&store, "c").unwrap();
        fake.ended.lock().unwrap().clear();
        store.mint("c").unwrap();
        assert_eq!(*fake.ended.lock().unwrap(), ["c"]);
        fake.ended.lock().unwrap().clear();
        end_token("c").unwrap();
        assert_eq!(*fake.ended.lock().unwrap(), ["c"]);
        assert!(store.status("c").unwrap().unwrap().hash.is_empty());
        reset();
    }
}
