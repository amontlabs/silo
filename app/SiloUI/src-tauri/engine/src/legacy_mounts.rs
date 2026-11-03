//! The host folders that computers from before Cua Driver recorded as bind mounts.
//!
//! Computers created with a v4 guest image mounted the device's published ChatGPT app
//! folder at `/opt/silo/chatgpt` and a folder holding the LCU archive at `/opt/silo/lcu`,
//! both read-only. Silo no longer downloads either, but the runtime cannot change a
//! computer's mounts after creation and refuses to boot, restore or fork one whose mount
//! source is missing. This module therefore
//!
//! * recreates a recorded source as an empty folder when it is gone, before a start, a
//!   checkpoint restore or a fork (the guest ignores the empty folders, and its helper installs
//!   Cua Driver itself);
//! * passes the same mounts again when a checkpoint of such a computer is restored or forked,
//!   because a snapshot never carries host mounts;
//! * strips both mounts from an exported configuration, because host paths mean nothing on
//!   another device; and
//! * empties the old download folders once no computer is running, to give the disk back.
//!
//! New computers never get these mounts.
use crate::runtime::{operation_gate::OperationGate, ProcessRunner, RuntimePaths, RuntimeRunner};
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

/// Where the guest saw the device's published ChatGPT app folder.
pub(crate) const CHATGPT_GUEST_MOUNT: &str = "/opt/silo/chatgpt";
/// Where the guest saw the folder holding the LCU archive.
pub(crate) const LCU_GUEST_MOUNT: &str = "/opt/silo/lcu";
const GUEST_MOUNTS: [&str; 2] = [CHATGPT_GUEST_MOUNT, LCU_GUEST_MOUNT];
/// The folders under the application data directory that held the downloads.
const ROOT_FOLDERS: [&str; 2] = ["chatgpt", "lcu"];
const READ_TIMEOUT: Duration = Duration::from_secs(10);

fn is_legacy_mount(mount: &Value) -> bool {
    mount.get("type").and_then(Value::as_str) == Some("Bind")
        && mount
            .get("guest")
            .and_then(Value::as_str)
            .is_some_and(|guest| GUEST_MOUNTS.contains(&guest))
}

fn read_only(mount: &Value) -> bool {
    mount.pointer("/options/readonly").and_then(Value::as_bool) == Some(true)
        || mount.get("readonly").and_then(Value::as_bool) == Some(true)
}

/// The host folders a configuration records as legacy mount sources.
fn sources(config: &Value) -> Vec<PathBuf> {
    config
        .get("mounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|mount| is_legacy_mount(mount))
        .filter_map(|mount| mount.get("host").and_then(Value::as_str).map(PathBuf::from))
        .collect()
}

/// The folders under which Silo itself created mount sources.
fn roots() -> Vec<PathBuf> {
    silo_engine::context::current()
        .map(|context| {
            ROOT_FOLDERS
                .into_iter()
                .map(|folder| context.paths().app_data.join(folder))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether `source` is a direct child of one of `roots` with a plain name.
fn is_silo_source(source: &Path, roots: &[PathBuf]) -> bool {
    let (Some(parent), Some(name)) = (source.parent(), source.file_name()) else {
        return false;
    };
    let plain = name.to_str().is_some_and(|name| {
        !name.is_empty()
            && name.len() <= 128
            && !name.starts_with('.')
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
    });
    plain
        && roots.iter().any(|root| {
            parent == root
                || matches!((parent.canonicalize(), root.canonicalize()), (Ok(a), Ok(b)) if a == b)
        })
}

/// Recreates every recorded source that is missing as an empty folder, when it is a direct
/// child of one of `roots`. Anything else is left for the runtime to report.
fn restore_missing(config: &Value, roots: &[PathBuf]) {
    for source in sources(config) {
        if fs::symlink_metadata(&source).is_ok() || !is_silo_source(&source, roots) {
            continue;
        }
        if let Some(root) = source.parent() {
            if fs::create_dir_all(root).is_err() {
                continue;
            }
        }
        if fs::create_dir(&source).is_ok() {
            let _ = fs::set_permissions(&source, fs::Permissions::from_mode(0o755));
        }
    }
}

/// Before a start: gives a computer that recorded legacy mounts the folders the runtime needs.
/// Best effort: a computer whose configuration cannot be read is left to the runtime to report.
pub(crate) fn restore_missing_before_start(paths: &RuntimePaths, computer: &str) {
    let Ok(inspected) = crate::runtime::inspect_computer(&ProcessRunner, paths, computer) else {
        return;
    };
    restore_missing(&inspected.config, &roots());
}

/// The `msb restore` arguments that mount again what `source_computer` recorded, which is
/// nothing for a computer created since Cua Driver. A snapshot never carries host mounts, so a
/// restore or fork of a computer that had these must pass them again to keep its guest
/// consistent. Best effort: a source that cannot be read adds nothing.
pub(crate) fn restore_args(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    source_computer: &str,
) -> Vec<String> {
    let Ok(inspected) = crate::runtime::inspect_computer(runner, paths, source_computer) else {
        return Vec::new();
    };
    restore_missing(&inspected.config, &roots());
    mount_args(&inspected.config)
}

/// The legacy mounts `config` records, kept for a restore after the computer is removed.
pub(crate) fn recorded(config: &Value) -> Vec<Value> {
    config
        .get("mounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|mount| is_legacy_mount(mount))
        .cloned()
        .collect()
}

/// The `msb restore` arguments for mounts kept by [`recorded`], recreating missing sources as
/// [`restore_args`] does.
pub(crate) fn restore_args_for(mounts: &[Value]) -> Vec<String> {
    if mounts.is_empty() {
        return Vec::new();
    }
    let config = serde_json::json!({ "mounts": mounts });
    restore_missing(&config, &roots());
    mount_args(&config)
}

/// `-v host:guest:ro,uid=0,gid=0` for each read-only legacy mount of `config` whose source
/// exists. `uid=0,gid=0` pins the guest owner as the original mount did.
fn mount_args(config: &Value) -> Vec<String> {
    let Some(mounts) = config.get("mounts").and_then(Value::as_array) else {
        return Vec::new();
    };
    mounts
        .iter()
        .filter(|mount| is_legacy_mount(mount) && read_only(mount))
        .filter_map(|mount| {
            let host = mount.get("host").and_then(Value::as_str)?;
            let guest = mount.get("guest").and_then(Value::as_str)?;
            fs::symlink_metadata(host).ok()?;
            Some(["-v".to_owned(), format!("{host}:{guest}:ro,uid=0,gid=0")])
        })
        .flatten()
        .collect()
}

/// Removes the legacy mounts from a configuration about to be exported. Their host paths mean
/// nothing on another device, and the imported computer gets none. A writable mount at one of
/// the paths is not Silo's and is refused.
pub(crate) fn strip_for_export(config: &mut Value) -> Result<(), String> {
    let Some(mounts) = config.get_mut("mounts").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    if mounts
        .iter()
        .any(|mount| is_legacy_mount(mount) && !read_only(mount))
    {
        return Err("The computer mounts a Silo folder writable, so it cannot be exported.".into());
    }
    mounts.retain(|mount| !is_legacy_mount(mount));
    Ok(())
}

// ------------------------------------------------------------ reclaiming disk

/// Empties the old ChatGPT and LCU download folders under `app_data`, keeping the folders
/// that exist (a computer may still record one of them as a mount source), unless a
/// computer runs or its state cannot be read. Holds the device-wide gate, so no lifecycle
/// work boots a computer meanwhile. Returns whether it ran: `false` means it should be tried
/// again at the next start.
pub fn reclaim_disk(paths: &RuntimePaths, app_data: &Path) -> bool {
    reclaim_disk_with(&crate::runtime::OPERATIONS, &ProcessRunner, paths, app_data)
}

fn reclaim_disk_with(
    gate: &OperationGate,
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    app_data: &Path,
) -> bool {
    let present: Vec<PathBuf> = ROOT_FOLDERS
        .into_iter()
        .map(|folder| app_data.join(folder))
        .filter(|root| fs::symlink_metadata(root).is_ok())
        .collect();
    if present.is_empty() {
        return true;
    }
    let Ok(_gate) = gate.try_device_hidden("Removing unused downloads") else {
        return false;
    };
    if !none_running(runner, paths) {
        return false;
    }
    for root in &present {
        empty_folders(root);
    }
    true
}

/// Whether every computer of the runtime is stopped. `false` when any runs or one cannot be read.
fn none_running(runner: &dyn RuntimeRunner, paths: &RuntimePaths) -> bool {
    let Ok(listed) = runner.run(
        paths,
        &["list".into(), "--format".into(), "json".into()],
        READ_TIMEOUT,
    ) else {
        return false;
    };
    let Ok(listed) = serde_json::from_str::<Vec<Value>>(&listed.stdout) else {
        return false;
    };
    listed.iter().all(|entry| {
        let Some(name) = entry.get("name").and_then(Value::as_str) else {
            return false;
        };
        crate::runtime::inspect_computer(runner, paths, name).is_ok_and(|inspected| {
            matches!(
                inspected.status.to_ascii_lowercase().as_str(),
                "created" | "stopped" | "crashed"
            )
        })
    })
}

/// Removes everything below `root` but keeps `root` and the folders directly in it.
fn empty_folders(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir()) {
            let _ = crate::download::make_tree_deletable(&path);
            let Ok(children) = fs::read_dir(&path) else {
                continue;
            };
            for child in children.flatten() {
                remove_entry(&child.path());
            }
        } else {
            let _ = fs::remove_file(&path);
        }
    }
}

fn remove_entry(path: &Path) {
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir()) {
        let _ = crate::download::make_tree_deletable(path);
        // std's remove_dir_all never follows symlinks.
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
#[path = "legacy_mounts/tests.rs"]
mod tests;
