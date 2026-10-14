//! A computer's running generation, published for other processes. The Silo engine socket
//! may run apart from the process that owns the virtual machine, so it cannot read the
//! owner's registry. The owner writes this marker while a computer is Running and removes
//! it on any other state; a reader accepts it only while the owner process is alive.
use super::store::Layout;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Marker {
    pub(super) pid: u32,
    pub(super) generation: u64,
}

fn marker_path(layout: &Layout) -> std::path::PathBuf {
    layout.dir.join("running.json")
}

/// Publishes `generation` as this process's running generation of the computer.
pub(super) fn publish(layout: &Layout, generation: u64) -> Result<(), String> {
    let bytes = serde_json::to_vec(&Marker {
        pid: std::process::id(),
        generation,
    })
    .map_err(|error| error.to_string())?;
    crate::editor::write_private(&marker_path(layout), &bytes)
}

/// Removes the marker; a missing marker is already cleared.
pub(super) fn clear(layout: &Layout) {
    match fs::remove_file(marker_path(layout)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => eprintln!("silo-engine: could not clear the running marker: {error}"),
    }
}

fn read(layout: &Layout) -> Option<Marker> {
    let bytes = fs::read(marker_path(layout)).ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

pub(super) fn process_is_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    // SAFETY: signal 0 only checks that the process exists and may be signalled.
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The generation the owner currently publishes for this computer, if its owner is alive.
pub(super) fn current(
    app_data: &Path,
    computer_id: &str,
    alive: impl Fn(u32) -> bool,
) -> Option<u64> {
    let marker = read(&Layout::new(app_data, computer_id))?;
    alive(marker.pid).then_some(marker.generation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_is_current_only_while_published_and_its_owner_lives() {
        let directory = tempfile::tempdir().unwrap();
        let id = "c690f54f-4cc9-4950-8782-30f21e89d18b";
        let layout = Layout::new(directory.path(), id);
        fs::create_dir_all(&layout.dir).unwrap();
        assert_eq!(current(directory.path(), id, |_| true), None);

        publish(&layout, 7).unwrap();
        assert_eq!(current(directory.path(), id, |_| true), Some(7));
        assert_eq!(current(directory.path(), id, |_| false), None);
        assert!(process_is_alive(std::process::id()));

        publish(&layout, 8).unwrap();
        assert_eq!(current(directory.path(), id, process_is_alive), Some(8));
        clear(&layout);
        clear(&layout);
        assert_eq!(current(directory.path(), id, |_| true), None);
    }

    #[test]
    fn oversized_or_malformed_markers_are_not_current() {
        let directory = tempfile::tempdir().unwrap();
        let id = "c690f54f-4cc9-4950-8782-30f21e89d18b";
        let layout = Layout::new(directory.path(), id);
        fs::create_dir_all(&layout.dir).unwrap();
        fs::write(marker_path(&layout), b"{").unwrap();
        assert_eq!(current(directory.path(), id, |_| true), None);
        fs::write(marker_path(&layout), vec![b' '; 5000]).unwrap();
        assert_eq!(current(directory.path(), id, |_| true), None);
    }
}
