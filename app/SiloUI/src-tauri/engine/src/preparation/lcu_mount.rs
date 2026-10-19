//! The folder computers mount for LCU, published and collected by silod only.
//!
//! The mount source is `<root>/current`. Its path never changes: an update renames the new
//! verified archive into it and removes the old one, so a computer that recorded the path
//! keeps a valid mount. Computers created by older versions recorded `<root>/<version>`
//! folders instead; those are recreated empty when missing (the guest downloads its pinned
//! archive when the folder holds none) and collected once no computer records them.
//!
//! One lock serializes publication, repair and collection, and collection also holds the
//! device-wide operation gate, so no lifecycle work boots a computer meanwhile.
use super::{
    locked, remove_tree, safe_name, ArchiveSpec, ProcessRunner, RuntimePaths, RuntimeRunner,
    DOWNLOAD_DIR, PUBLISH_PREFIX,
};
use crate::{
    chatgpt_app::{self, Maintenance, COLLECTION_RETRY},
    runtime::operation_gate::OperationGate,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// The stable folder computers mount.
pub(super) const FOLDER: &str = "current";

static FOLDERS: Mutex<()> = Mutex::new(());

fn exclusive() -> MutexGuard<'static, ()> {
    locked(&FOLDERS)
}

/// Moves the verified `part` into the stable folder as `spec.archive` and removes every
/// other file there. The folder is created once and never renamed or removed; the archive
/// is complete from the moment it has its name.
pub(super) fn publish(root: &Path, spec: &ArchiveSpec, part: &Path) -> std::io::Result<()> {
    let _lock = exclusive();
    let folder = root.join(FOLDER);
    if fs::symlink_metadata(&folder).is_ok_and(|meta| !meta.is_dir()) {
        fs::remove_file(&folder)?;
    }
    fs::create_dir_all(&folder)?;
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755))?;
    fs::set_permissions(part, fs::Permissions::from_mode(0o444))?;
    fs::rename(part, folder.join(&spec.archive))?;
    for entry in fs::read_dir(&folder)?.flatten() {
        if entry.file_name() != spec.archive.as_str() {
            remove_entry(&entry.path());
        }
    }
    Ok(())
}

/// Recreates `source` as an empty folder when nothing exists there and it is a direct
/// child of `root`. Returns whether it did.
pub(super) fn restore_missing(source: &Path, root: &Path) -> bool {
    let (Some(parent), Some(name)) = (source.parent(), source.file_name()) else {
        return false;
    };
    if !name.to_str().is_some_and(safe_name) {
        return false;
    }
    let _lock = exclusive();
    if fs::symlink_metadata(source).is_ok() {
        return false;
    }
    let same = parent == root
        || matches!((parent.canonicalize(), root.canonicalize()), (Ok(a), Ok(b)) if a == b);
    if !same || fs::create_dir_all(root).is_err() {
        return false;
    }
    let target = root.join(name);
    if fs::create_dir(&target).is_err() {
        return false;
    }
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).is_ok()
}

/// What the computers record: for each folder name directly under the root, whether a
/// running computer mounts it. `None` when any computer cannot be read.
fn references(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    root: &Path,
) -> Option<HashMap<OsString, bool>> {
    let listed = runner
        .run(
            paths,
            &["list".into(), "--format".into(), "json".into()],
            READ_TIMEOUT,
        )
        .ok()?;
    let listed: Vec<Value> = serde_json::from_str(&listed.stdout).ok()?;
    let root = root.canonicalize().ok()?;
    let mut referenced = HashMap::new();
    for entry in listed {
        let name = entry.get("name")?.as_str()?;
        let inspected = crate::runtime::inspect_computer(runner, paths, name).ok()?;
        let Some(source) = crate::computer_use::lcu_mount_source(&inspected.config) else {
            continue;
        };
        let (Some(parent), Some(folder)) = (source.parent(), source.file_name()) else {
            continue;
        };
        if parent != root && parent.canonicalize().ok().as_deref() != Some(root.as_path()) {
            continue;
        }
        let running = !matches!(
            inspected.status.to_ascii_lowercase().as_str(),
            "created" | "stopped" | "crashed"
        );
        let slot = referenced.entry(folder.to_owned()).or_insert(false);
        *slot |= running;
    }
    Some(referenced)
}

/// Removes the folders no computer records, empties the ones only stopped computers record
/// (their guest downloads the pinned archive instead) and drops partial downloads of other
/// versions. Runs under the device-wide gate and the folder lock; returns false, having
/// changed nothing, when it must wait: an operation is running or queued, or what the
/// computers record cannot be read.
pub(super) fn collect(
    gate: &OperationGate,
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    root: &Path,
    version: &str,
) -> bool {
    let Ok(_gate) = gate.try_device_hidden("Removing unused LCU folders") else {
        return false;
    };
    let _lock = exclusive();
    if !root.is_dir() {
        return true;
    }
    let Some(referenced) = references(runner, paths, root) else {
        return false;
    };
    let Ok(entries) = fs::read_dir(root) else {
        return true;
    };
    let prefix = format!("{version}-");
    for entry in entries.flatten() {
        let name = entry.file_name();
        let path = entry.path();
        if name == FOLDER {
            continue;
        }
        if name == DOWNLOAD_DIR {
            remove_other_partials(&path, &prefix);
            continue;
        }
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        match referenced.get(&name) {
            None => remove_tree(&path),
            Some(false) if !name.to_string_lossy().starts_with(PUBLISH_PREFIX) => {
                empty_folder(&path)
            }
            Some(_) => {}
        }
    }
    true
}

fn empty_folder(folder: &Path) {
    let _ = chatgpt_app::make_tree_deletable(folder);
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        remove_entry(&entry.path());
    }
}

fn remove_entry(path: &Path) {
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir()) {
        remove_tree(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn remove_other_partials(downloads: &Path, keep_prefix: &str) {
    let Ok(entries) = fs::read_dir(downloads) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(keep_prefix) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

static COLLECTION: Maintenance = Maintenance::new();

/// Collects now, or keeps trying in the background until it ran.
pub(super) fn request_collection() {
    if COLLECTION.note(collect_once()) {
        let _ = std::thread::Builder::new().name("lcu-gc".into()).spawn(|| {
            COLLECTION.run_retries(collect_once, || std::thread::sleep(COLLECTION_RETRY))
        });
    }
}

fn collect_once() -> bool {
    let (Some(paths), Some(root), Ok(spec)) = (
        super::PATHS.get(),
        super::LCU_ROOT.get(),
        ArchiveSpec::lcu(),
    ) else {
        return true;
    };
    collect(
        &crate::runtime::OPERATIONS,
        &ProcessRunner,
        paths,
        root,
        &spec.version,
    )
}

#[cfg(test)]
#[path = "lcu_mount/tests.rs"]
mod tests;
