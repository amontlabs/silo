//! The runtime's local memory cache. Restoring a full checkpoint with its memory realizes
//! that memory as one file under `cache/memory/snapshots`, named after the checkpoint's
//! memory manifest. MicroSandbox never removes these files itself, so deleting the
//! checkpoint (or the computer restored from it) leaves the file behind. The cache is only
//! a copy of what the checkpoint stores, so a file whose manifest no stored checkpoint
//! names is removed; a restore that needs it again rebuilds it from its checkpoint.
use super::RuntimePaths;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

const ENTRY_SUFFIX: &str = ".ram";
const BUILD_LOCK_SUFFIX: &str = ".build-lock";

/// The memory manifests the stored checkpoints name, spelled as the cache names its files
/// (`sha256-<hex>`).
fn referenced_manifests(snapshots: &Path) -> io::Result<HashSet<String>> {
    let mut manifests = HashSet::new();
    let groups = match fs::read_dir(snapshots) {
        Ok(groups) => groups,
        Err(failure) if failure.kind() == io::ErrorKind::NotFound => return Ok(manifests),
        Err(failure) => return Err(failure),
    };
    for group in groups.flatten() {
        let Ok(members) = fs::read_dir(group.path()) else {
            continue;
        };
        for member in members.flatten() {
            let Ok(bytes) = fs::read(member.path().join("checkpoint").join("checkpoint.json"))
            else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                // An unreadable manifest might name any entry; keep them all.
                return Err(io::Error::other("a checkpoint manifest could not be read"));
            };
            if let Some(memory) = value.get("memory").and_then(serde_json::Value::as_str) {
                manifests.insert(memory.replace(':', "-"));
            }
        }
    }
    Ok(manifests)
}

/// `sha256-<hex>` from `sha256-<hex>-<page size>.ram`.
fn manifest_of(file_name: &str) -> Option<&str> {
    let stem = file_name.strip_suffix(ENTRY_SUFFIX)?;
    let (manifest, page) = stem.rsplit_once('-')?;
    (page.bytes().all(|byte| byte.is_ascii_digit()) && !page.is_empty() && manifest.contains('-'))
        .then_some(manifest)
}

fn try_lock_exclusive(file: &File) -> io::Result<bool> {
    loop {
        // The descriptor belongs to `file`, which outlives the call.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(true);
        }
        let failure = io::Error::last_os_error();
        match failure.kind() {
            io::ErrorKind::Interrupted => continue,
            io::ErrorKind::WouldBlock => return Ok(false),
            _ => return Err(failure),
        }
    }
}

/// Removes the cache file at `path` unless a computer maps it (it holds a shared lock) or a
/// restore is building it. Returns whether it was removed.
fn remove_unpinned(path: &Path) -> io::Result<bool> {
    let build_lock = {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
        let stem = name.strip_suffix(ENTRY_SUFFIX).unwrap_or(name);
        path.with_file_name(format!("{stem}{BUILD_LOCK_SUFFIX}"))
    };
    // A missing build lock means nothing is building the entry.
    let building = match File::open(&build_lock) {
        Ok(lock) => {
            if !try_lock_exclusive(&lock)? {
                return Ok(false);
            }
            Some(lock)
        }
        Err(failure) if failure.kind() == io::ErrorKind::NotFound => None,
        Err(failure) => return Err(failure),
    };
    let entry = match File::open(path) {
        Ok(entry) => entry,
        Err(failure) if failure.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(failure) => return Err(failure),
    };
    if !try_lock_exclusive(&entry)? {
        return Ok(false);
    }
    let opened = entry.metadata()?;
    let current = fs::symlink_metadata(path)?;
    if (opened.dev(), opened.ino()) != (current.dev(), current.ino()) {
        return Ok(false);
    }
    fs::remove_file(path)?;
    drop(building);
    Ok(true)
}

/// Removes the cache files no stored checkpoint names. Best effort: the files are
/// rebuildable, so a problem is logged and the next cleanup tries again. Returns how many
/// files were removed.
pub(super) fn prune(paths: &RuntimePaths) -> usize {
    let cache = paths.home.join("cache").join("memory").join("snapshots");
    let entries = match fs::read_dir(&cache) {
        Ok(entries) => entries,
        Err(_) => return 0,
    };
    let referenced = match referenced_manifests(&paths.home.join("snapshots")) {
        Ok(referenced) => referenced,
        Err(failure) => {
            eprintln!("Silo kept the runtime memory cache: {failure}.");
            return 0;
        }
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(manifest) = file_name.to_str().and_then(manifest_of) else {
            continue;
        };
        if referenced.contains(manifest) {
            continue;
        }
        match remove_unpinned(&entry.path()) {
            Ok(true) => removed += 1,
            Ok(false) => {}
            Err(failure) => eprintln!("Silo kept a runtime memory cache file: {failure}."),
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEPT: &str = "sha256-aaaa";
    const ORPHAN: &str = "sha256-bbbb";

    fn fixture() -> (tempfile::TempDir, RuntimePaths) {
        let directory = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(directory.path());
        let member = paths
            .home
            .join("snapshots")
            .join("dev")
            .join("snap_1")
            .join("checkpoint");
        fs::create_dir_all(&member).unwrap();
        fs::write(
            member.join("checkpoint.json"),
            serde_json::json!({"memory": KEPT.replacen('-', ":", 1)}).to_string(),
        )
        .unwrap();
        let cache = paths.home.join("cache").join("memory").join("snapshots");
        fs::create_dir_all(&cache).unwrap();
        for name in [KEPT, ORPHAN] {
            fs::write(cache.join(format!("{name}-16384.ram")), b"ram").unwrap();
            fs::write(cache.join(format!("{name}-16384.build-lock")), b"").unwrap();
        }
        (directory, paths)
    }

    fn files(paths: &RuntimePaths) -> Vec<String> {
        let cache = paths.home.join("cache").join("memory").join("snapshots");
        let mut names: Vec<String> = fs::read_dir(cache)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(ENTRY_SUFFIX))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_file_no_checkpoint_names_is_removed_and_a_named_one_is_kept() {
        let (_directory, paths) = fixture();

        assert_eq!(prune(&paths), 1);

        assert_eq!(files(&paths), [format!("{KEPT}-16384.ram")]);
    }

    #[test]
    fn a_file_a_computer_maps_is_kept_until_it_lets_go() {
        let (_directory, paths) = fixture();
        let cache = paths.home.join("cache").join("memory").join("snapshots");
        let mapped = File::open(cache.join(format!("{ORPHAN}-16384.ram"))).unwrap();
        // The descriptor belongs to `mapped`, which is alive.
        assert_eq!(unsafe { libc::flock(mapped.as_raw_fd(), libc::LOCK_SH) }, 0);

        assert_eq!(prune(&paths), 0);
        assert_eq!(files(&paths).len(), 2);

        drop(mapped);
        assert_eq!(prune(&paths), 1);
        assert_eq!(files(&paths), [format!("{KEPT}-16384.ram")]);
    }

    #[test]
    fn a_file_being_built_is_kept() {
        let (_directory, paths) = fixture();
        let cache = paths.home.join("cache").join("memory").join("snapshots");
        let building = File::open(cache.join(format!("{ORPHAN}-16384.build-lock"))).unwrap();
        // The descriptor belongs to `building`, which is alive.
        assert_eq!(
            unsafe { libc::flock(building.as_raw_fd(), libc::LOCK_EX) },
            0
        );

        assert_eq!(prune(&paths), 0);
        assert_eq!(files(&paths).len(), 2);
    }

    #[test]
    fn nothing_is_removed_when_a_checkpoint_manifest_cannot_be_read() {
        let (_directory, paths) = fixture();
        let unreadable = paths
            .home
            .join("snapshots")
            .join("dev")
            .join("snap_2")
            .join("checkpoint");
        fs::create_dir_all(&unreadable).unwrap();
        fs::write(unreadable.join("checkpoint.json"), b"{not json").unwrap();

        assert_eq!(prune(&paths), 0);
        assert_eq!(files(&paths).len(), 2);
    }

    #[test]
    fn only_cache_entry_names_are_considered() {
        assert_eq!(manifest_of("sha256-ab12-16384.ram"), Some("sha256-ab12"));
        assert_eq!(manifest_of("sha256-ab12-16384.build-lock"), None);
        assert_eq!(manifest_of("notes.ram"), None);
        assert_eq!(manifest_of("sha256-ab12-x.ram"), None);
    }

    #[test]
    fn a_missing_cache_or_store_is_not_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(directory.path());
        assert_eq!(prune(&paths), 0);
    }
}
