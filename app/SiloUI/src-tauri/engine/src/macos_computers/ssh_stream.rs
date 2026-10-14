use super::{guest_access, store};
use std::{
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::Path,
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

pub(super) fn connect(
    app_data: &Path,
    computer_id: &str,
    port: u16,
) -> Result<TcpStream, String> {
    if port == 0 {
        return Err("Invalid guest connection.".into());
    }
    let address = guest_address(app_data, computer_id, guest_access::guest_address)?;
    TcpStream::connect_timeout(&SocketAddr::from((address, port)), CONNECT_TIMEOUT)
        .map_err(|_| "Could not open the computer's SSH connection.".into())
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
        store::save(&store::Layout::new(directory.path(), &uninstalled.id), &uninstalled).unwrap();

        let mut incomplete = complete.clone();
        incomplete.id = incomplete_id.clone();
        incomplete.name = "incomplete".into();
        incomplete.setup.needs_personalizing = true;
        store::save(&store::Layout::new(directory.path(), &incomplete.id), &incomplete).unwrap();

        let mut inherited = complete.clone();
        inherited.id = inherited_id.clone();
        inherited.name = "inherited".into();
        inherited.inherited_access = true;
        store::save(&store::Layout::new(directory.path(), &inherited.id), &inherited).unwrap();

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

    #[test]
    fn guest_stream_rejects_port_zero_before_lookup() {
        let directory = tempfile::tempdir().unwrap();
        let record = complete_record(directory.path());
        assert!(connect(directory.path(), &record.id, 0).is_err());
    }
}
