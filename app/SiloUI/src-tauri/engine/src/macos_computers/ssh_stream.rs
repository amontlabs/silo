use super::{guest_access, ports, run_marker, store};
use std::{
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

fn eligible_record(app_data: &Path, computer_id: &str) -> Result<store::Record, String> {
    uuid::Uuid::parse_str(computer_id).map_err(|_| "Invalid computer identity.".to_owned())?;
    let record = store::load_all(app_data)
        .into_iter()
        .find(|record| record.id == computer_id)
        .ok_or_else(|| "The macOS computer was not found on this device.".to_owned())?;
    if !record.installed || !record.setup_complete() || record.inherited_access {
        return Err("Finish setting up this computer before connecting over SSH.".into());
    }
    Ok(record)
}

fn guest_address(
    app_data: &Path,
    computer_id: &str,
    lookup: impl FnOnce(&str) -> Result<Ipv4Addr, String>,
) -> Result<Ipv4Addr, String> {
    let record = eligible_record(app_data, computer_id)?;
    lookup(&record.mac_address)
}

/// The guest's SSH port. A `guest.ssh` stream is always an SSH transport to this port: the
/// optional published-port label only names the forward for revocation.
const GUEST_SSH_PORT: u16 = 22;

/// An open guest SSH transport and the means to learn that it must end.
pub struct GuestSshStream {
    pub stream: TcpStream,
    pub guard: GuestSshGuard,
}

/// Why a stream may continue: its published port still exists, and the guest is still in the
/// running generation it opened against.
pub struct GuestSshGuard {
    registration: Option<crate::remote::PortRegistration>,
    app_data: PathBuf,
    computer_id: String,
    port: Option<u16>,
    generation: u64,
    alive: fn(u32) -> bool,
}

impl GuestSshGuard {
    pub fn allowed(&self) -> bool {
        if self
            .registration
            .as_ref()
            .is_some_and(crate::remote::PortRegistration::revoked)
        {
            return false;
        }
        if run_marker::current(&self.app_data, &self.computer_id, self.alive)
            != Some(self.generation)
        {
            return false;
        }
        ports::published(&self.app_data, &self.computer_id, self.port) != Some(false)
    }
}

pub(super) fn open(
    app_data: &Path,
    computer_id: &str,
    port: Option<u16>,
) -> Result<GuestSshStream, String> {
    open_with(
        app_data,
        computer_id,
        port,
        run_marker::process_is_alive,
        guest_access::guest_address,
        |address| TcpStream::connect_timeout(&address, CONNECT_TIMEOUT),
    )
}

fn open_with(
    app_data: &Path,
    computer_id: &str,
    port: Option<u16>,
    alive: fn(u32) -> bool,
    lookup: impl FnOnce(&str) -> Result<Ipv4Addr, String>,
    connect: impl FnOnce(SocketAddr) -> std::io::Result<TcpStream>,
) -> Result<GuestSshStream, String> {
    if port == Some(0) {
        return Err("Invalid guest connection.".into());
    }
    let address = guest_address(app_data, computer_id, lookup)?;
    let generation = run_marker::current(app_data, computer_id, alive)
        .ok_or_else(|| "Start this computer before connecting over SSH.".to_owned())?;
    if port.is_some_and(|port| ports::published(app_data, computer_id, Some(port)) == Some(false)) {
        return Err("This port is no longer published.".into());
    }
    // Register before connecting so an unpublish during the connect is not missed.
    let registration = port.map(|port| crate::remote::PortRegistration::open(computer_id, port));
    let stream = connect(SocketAddr::from((address, GUEST_SSH_PORT)))
        .map_err(|_| "Could not open the computer's SSH connection.".to_owned())?;
    let guard = GuestSshGuard {
        registration,
        app_data: app_data.to_path_buf(),
        computer_id: computer_id.to_owned(),
        port,
        generation,
        alive,
    };
    if !guard.allowed() {
        return Err("This computer stopped while its connection opened.".into());
    }
    Ok(GuestSshStream { stream, guard })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn complete_record(app_data: &Path) -> store::Record {
        let request = store::CreateRequest {
            name: "stream-test".into(),
            cpus: 4,
            memory_gib: 8,
            disk_gib: 64,
        };
        let mut record = store::new_record(&request, "02:00:00:00:00:01".into());
        record.installed = true;
        record.setup = store::SetupProgress {
            account: true,
            sip: true,
            developer_tools: true,
            computer_use: true,
            clipboard: true,
            roots: true,
            needs_personalizing: false,
            proxy_bootstrap: true,
        };
        record.developer_tools_version = Some(super::super::developer_tools::version());
        store::save(&store::Layout::new(app_data, &record.id), &record).unwrap();
        record
    }

    #[test]
    fn guest_stream_uses_only_a_fully_set_up_local_computer() {
        let directory = tempfile::tempdir().unwrap();
        let record = complete_record(directory.path());
        let called = AtomicBool::new(false);
        let address = guest_address(directory.path(), &record.id, |mac| {
            called.store(true, Ordering::Release);
            assert_eq!(mac, record.mac_address);
            Ok(Ipv4Addr::LOCALHOST)
        })
        .unwrap();
        assert_eq!(address, Ipv4Addr::LOCALHOST);
        assert!(called.load(Ordering::Acquire));
    }

    #[test]
    fn guest_stream_rejects_missing_uninstalled_incomplete_and_inherited_records() {
        let directory = tempfile::tempdir().unwrap();
        let complete = complete_record(directory.path());
        let uninstalled_id = uuid::Uuid::new_v4().to_string();
        let incomplete_id = uuid::Uuid::new_v4().to_string();
        let inherited_id = uuid::Uuid::new_v4().to_string();

        let mut uninstalled = store::new_record(
            &store::CreateRequest {
                name: "uninstalled".into(),
                cpus: 4,
                memory_gib: 8,
                disk_gib: 64,
            },
            "02:00:00:00:00:02".into(),
        );
        uninstalled.id = uninstalled_id.clone();
        store::save(
            &store::Layout::new(directory.path(), &uninstalled.id),
            &uninstalled,
        )
        .unwrap();

        let mut incomplete = complete.clone();
        incomplete.id = incomplete_id.clone();
        incomplete.name = "incomplete".into();
        incomplete.setup.needs_personalizing = true;
        store::save(
            &store::Layout::new(directory.path(), &incomplete.id),
            &incomplete,
        )
        .unwrap();

        let mut inherited = complete.clone();
        inherited.id = inherited_id.clone();
        inherited.name = "inherited".into();
        inherited.inherited_access = true;
        store::save(
            &store::Layout::new(directory.path(), &inherited.id),
            &inherited,
        )
        .unwrap();

        let missing_id = uuid::Uuid::new_v4().to_string();
        for id in [
            "not-a-uuid",
            missing_id.as_str(),
            uninstalled_id.as_str(),
            incomplete_id.as_str(),
            inherited_id.as_str(),
        ] {
            let called = AtomicBool::new(false);
            assert!(guest_address(directory.path(), id, |_| {
                called.store(true, Ordering::Release);
                Ok(Ipv4Addr::LOCALHOST)
            })
            .is_err());
            assert!(!called.load(Ordering::Acquire), "lookup ran for {id}");
        }
    }

    fn publish_forward(app_data: &Path, record: &store::Record, port: u16, enabled: bool) {
        ports::write_test_configuration(app_data, &record.id, port, enabled);
    }

    fn start_marker(app_data: &Path, record: &store::Record, generation: u64) {
        run_marker::publish(&store::Layout::new(app_data, &record.id), generation).unwrap();
    }

    /// A loopback listener standing in for the guest's SSH port.
    fn guest() -> (std::net::TcpListener, SocketAddr) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        (listener, address)
    }

    fn open_to(
        app_data: &Path,
        record: &store::Record,
        port: Option<u16>,
        target: SocketAddr,
        seen: &std::cell::Cell<Option<u16>>,
    ) -> Result<GuestSshStream, String> {
        open_with(
            app_data,
            &record.id,
            port,
            |_| true,
            |_| Ok(Ipv4Addr::LOCALHOST),
            |address| {
                seen.set(Some(address.port()));
                TcpStream::connect(target)
            },
        )
    }

    #[test]
    fn forwarded_streams_still_reach_the_guest_ssh_port() {
        let directory = tempfile::tempdir().unwrap();
        let record = complete_record(directory.path());
        start_marker(directory.path(), &record, 3);
        publish_forward(directory.path(), &record, 3000, true);
        let (_listener, target) = guest();
        let seen = std::cell::Cell::new(None);
        let forwarded = open_to(directory.path(), &record, Some(3000), target, &seen).unwrap();
        // The label names the forward; it is not the destination port.
        assert_eq!(seen.get(), Some(GUEST_SSH_PORT));
        assert!(forwarded.guard.allowed());
        seen.set(None);
        let interactive = open_to(directory.path(), &record, None, target, &seen).unwrap();
        assert_eq!(seen.get(), Some(GUEST_SSH_PORT));
        assert!(interactive.guard.allowed());
    }

    #[test]
    fn unpublishing_revokes_forwarded_streams_but_not_interactive_ones() {
        let directory = tempfile::tempdir().unwrap();
        let record = complete_record(directory.path());
        start_marker(directory.path(), &record, 3);
        publish_forward(directory.path(), &record, 3000, true);
        publish_forward(directory.path(), &record, 3001, true);
        let (_listener, target) = guest();
        let seen = std::cell::Cell::new(None);
        let forwarded = open_to(directory.path(), &record, Some(3000), target, &seen).unwrap();
        let other = open_to(directory.path(), &record, Some(3001), target, &seen).unwrap();
        let interactive = open_to(directory.path(), &record, None, target, &seen).unwrap();

        // The owner's revocation in this process reaches the open stream at once.
        crate::remote::revoke_port_streams(&record.id, 3000);
        assert!(!forwarded.guard.allowed());
        assert!(other.guard.allowed());
        assert!(interactive.guard.allowed());

        // An owner in another process revokes through the saved forward.
        publish_forward(directory.path(), &record, 3001, false);
        assert!(!other.guard.allowed());
        assert!(interactive.guard.allowed());
    }

    #[test]
    fn a_forward_that_is_not_published_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let record = complete_record(directory.path());
        start_marker(directory.path(), &record, 3);
        let (_listener, target) = guest();
        let seen = std::cell::Cell::new(None);
        assert!(open_to(directory.path(), &record, Some(3000), target, &seen).is_err());
        publish_forward(directory.path(), &record, 3000, false);
        assert!(open_to(directory.path(), &record, Some(3000), target, &seen).is_err());
        assert_eq!(seen.get(), None, "no connection should have been attempted");
    }

    #[test]
    fn streams_follow_the_running_state_and_generation_of_the_computer() {
        let directory = tempfile::tempdir().unwrap();
        let record = complete_record(directory.path());
        let (_listener, target) = guest();
        let seen = std::cell::Cell::new(None);

        // A saved, set-up record alone is not a running computer.
        assert!(open_to(directory.path(), &record, None, target, &seen).is_err());
        assert_eq!(seen.get(), None);

        start_marker(directory.path(), &record, 3);
        let stream = open_to(directory.path(), &record, None, target, &seen).unwrap();
        assert!(stream.guard.allowed());

        // A restart is a new generation: the old stream ends even though the computer runs.
        start_marker(directory.path(), &record, 4);
        assert!(!stream.guard.allowed());

        // A stop clears the marker; a dead owner's marker is not a running computer either.
        let restarted = open_to(directory.path(), &record, None, target, &seen).unwrap();
        assert!(restarted.guard.allowed());
        run_marker::clear(&store::Layout::new(directory.path(), &record.id));
        assert!(!restarted.guard.allowed());
        start_marker(directory.path(), &record, 5);
        assert!(open_with(
            directory.path(),
            &record.id,
            None,
            |_| false,
            |_| Ok(Ipv4Addr::LOCALHOST),
            |address| TcpStream::connect(address),
        )
        .is_err());
    }

    #[test]
    fn guest_stream_rejects_port_zero_before_lookup() {
        let directory = tempfile::tempdir().unwrap();
        let record = complete_record(directory.path());
        assert!(open(directory.path(), &record.id, Some(0)).is_err());
    }
}
