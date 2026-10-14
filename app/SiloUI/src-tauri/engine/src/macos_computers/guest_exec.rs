//! Raw SSH access to a macOS computer for the engine's `guest.*` methods: which computer a
//! `computerId` names, whether it is running, and the `ssh` invocation that runs a command in
//! it as the working account. Bytes, deadlines and cancellation belong to the caller
//! (`guest::process`); this module only finds the guest and builds the connection.
use super::{
    computer, guest_access,
    store::{self, Layout, Record, State},
};
use std::path::Path;

/// A stored macOS computer that `guest.*` can reach.
#[derive(Clone)]
pub struct GuestSsh {
    layout: Layout,
    record: Record,
}

/// The macOS computer `id`, or `None` when `id` names no macOS computer on this device.
///
/// The live registry answers when the engine has loaded it; otherwise the stored record is
/// read from disk, and reachability is decided by the SSH login itself.
pub fn lookup(app_data: &Path, id: &str) -> Result<Option<GuestSsh>, String> {
    let record = match computer(id) {
        Ok((record, _)) => record,
        Err(_) => match store::load_all(app_data)
            .into_iter()
            .find(|record| record.id == id)
        {
            Some(record) => record,
            None => return Ok(None),
        },
    };
    Ok(Some(GuestSsh {
        layout: Layout::new(app_data, &record.id),
        record,
    }))
}

impl GuestSsh {
    pub fn id(&self) -> &str {
        &self.record.id
    }

    pub fn name(&self) -> &str {
        &self.record.name
    }

    /// The working account every guest command runs as.
    pub fn user(&self) -> &'static str {
        guest_access::USER
    }

    /// "Start <name> first." unless the computer is running (or the engine has not loaded it,
    /// in which case the login decides).
    pub fn ensure_running(&self) -> Result<(), String> {
        match computer(&self.record.id) {
            Ok((_, State::Running)) | Err(_) => Ok(()),
            Ok(_) => Err(format!("Start {} first.", self.record.name)),
        }
    }

    /// The address a controller reaches the guest's SSH port at (the proxy listener once the
    /// guest has one, else the NAT lease).
    pub fn address(&self) -> Result<std::net::IpAddr, String> {
        guest_access::endpoint(&self.record, 22).map(|endpoint| endpoint.ip())
    }

    /// The program and arguments that log in with the computer's key and run `command`
    /// (one shell command line) in the guest.
    pub fn invocation(&self, command: &str) -> Result<(String, Vec<String>), String> {
        guest_access::ssh_invocation(&self.layout, &self.record, command)
    }
}

/// A stored macOS computer for tests of the callers: a record on disk, no live guest.
#[cfg(test)]
pub(crate) fn stored_for_tests(app_data: &Path, name: &str) -> GuestSsh {
    let record = store::new_record(
        &store::CreateRequest {
            name: name.into(),
            cpus: 4,
            memory_gib: 8,
            disk_gib: 64,
        },
        "02:00:00:00:00:01".into(),
    );
    let layout = Layout::new(app_data, &record.id);
    store::save(&layout, &record).unwrap();
    lookup(app_data, &record.id).unwrap().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_macos_computer_is_found_by_id_and_needs_its_access_material_to_connect() {
        let directory = tempfile::tempdir().unwrap();
        let stored = stored_for_tests(directory.path(), "mac-one");
        let found = lookup(directory.path(), stored.id()).unwrap().unwrap();
        assert_eq!((found.name(), found.user()), ("mac-one", "silo"));
        // Nothing has loaded it into the live registry, so the login decides reachability.
        found.ensure_running().unwrap();
        assert_eq!(
            found.invocation("true").unwrap_err(),
            "This computer's access is missing."
        );
    }

    #[test]
    fn an_unknown_id_is_not_a_macos_computer() {
        let directory = tempfile::tempdir().unwrap();
        assert!(
            lookup(directory.path(), "00000000-0000-4000-8000-000000000001")
                .unwrap()
                .is_none()
        );
    }
}
