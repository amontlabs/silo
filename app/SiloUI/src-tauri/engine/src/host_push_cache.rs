//! Bounded, private publishing repositories; credentials never belong here.
//! Each cached repository has its own lock, held for a whole push, so pushes
//! of different repositories run concurrently. A short root lock serializes
//! choosing, creating and evicting cache directories across processes.
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    fs::{self, File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    time::SystemTime,
};

const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const FAILED: &str = "Could not prepare the private publishing cache.";

pub(crate) struct Cache {
    pub(crate) directory: PathBuf,
    root: PathBuf,
    budget: u64,
    discarded: Cell<bool>,
    /// This repository's lock; Git children inherit it.
    lock: File,
}

pub(crate) fn acquire(root: &Path, key: &str) -> Result<Cache, String> {
    acquire_with_budget(root, key, MAX_BYTES)
}

fn private_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(FAILED.into())
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| FAILED)?;
        }
        Err(_) => return Err(FAILED.into()),
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| FAILED.into())
}

fn open_lock(path: &Path, create: bool) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    Ok(file)
}

fn try_lock(file: &File) -> bool {
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

/// Held only while choosing, creating or evicting cache directories, never
/// during a push and never by a Git process.
struct RootLock(File);
impl RootLock {
    fn take(root: &Path) -> Result<Self, String> {
        let file = open_lock(&root.join(".lock"), true).map_err(|_| FAILED)?;
        while unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                return Err(FAILED.into());
            }
        }
        Ok(Self(file))
    }
}
impl Drop for RootLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn is_cache_name(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn acquire_with_budget(root: &Path, key: &str, budget: u64) -> Result<Cache, String> {
    private_directory(root)?;
    let name = format!("{:x}", Sha256::digest(key.as_bytes()));
    let root_lock = RootLock::take(root)?;
    // Repository locks are opened and taken only under the root lock, so an
    // eviction never deletes a lock file someone is about to take.
    let lock = open_lock(&root.join(format!("{name}.lock")), true).map_err(|_| FAILED)?;
    if !try_lock(&lock) {
        return Err(
            "Another push of this repository is still using its publishing cache. Wait for it to finish."
                .into(),
        );
    }
    let directory = root.join(&name);
    private_directory(&directory)?;
    // Obtaining this repository's lock proves no previous Git child is still
    // using its cache. A surviving marker means its process died before cleanup.
    if fs::symlink_metadata(directory.join(".active")).is_ok() {
        fs::remove_dir_all(&directory).map_err(|_| FAILED)?;
        private_directory(&directory)?;
    }
    sweep(root, budget)?;
    drop(root_lock);
    tree_size(&directory)?;
    let mut marker = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory.join(".last-used"))
        .map_err(|_| FAILED)?;
    use std::io::Write;
    marker.write_all(b"used\n").map_err(|_| FAILED)?;
    let mut active = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory.join(".active"))
        .map_err(|_| FAILED)?;
    active
        .write_all(b"active\n")
        .and_then(|_| active.sync_all())
        .map_err(|_| FAILED)?;
    File::open(&directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| FAILED)?;
    Ok(Cache {
        directory,
        root: root.into(),
        budget,
        discarded: Cell::new(false),
        lock,
    })
}

impl Cache {
    /// Invalidate failed publication state while retaining the operation lock.
    /// Preserve the active marker if deletion fails so acquisition retries it.
    pub(crate) fn discard(&self) {
        self.discarded.set(true);
        let _ = fs::remove_dir_all(&self.directory);
    }

    pub(crate) fn lock_fd(&self) -> std::os::fd::RawFd {
        self.lock.as_raw_fd()
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        // Publication has already completed. Cache cleanup must not relabel a
        // successful remote ref update as a failed push. Retry on next acquire.
        if !self.discarded.get() {
            let _ = fs::remove_file(self.directory.join(".active"));
        }
        let root = RootLock::take(&self.root);
        // Explicitly release before close: an unrelated concurrent fork can
        // briefly inherit this open-file description until its exec closes it.
        unsafe {
            libc::flock(self.lock.as_raw_fd(), libc::LOCK_UN);
        }
        if root.is_ok() {
            let _ = sweep(&self.root, self.budget);
        }
    }
}

fn tree_size(path: &Path) -> Result<u64, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| FAILED)?;
    if metadata.file_type().is_symlink() {
        return Err(
            "The publishing cache contains a symbolic link. Remove this cache before retrying."
                .into(),
        );
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Err(FAILED.into());
    }
    let mut bytes = 0_u64;
    for entry in fs::read_dir(path).map_err(|_| FAILED)? {
        bytes = bytes.saturating_add(tree_size(&entry.map_err(|_| FAILED)?.path())?);
    }
    Ok(bytes)
}

/// The repository's lock when nobody else holds it; `None` while another push
/// (or a Git process surviving a crash) uses that cache. Caller holds the root lock.
fn idle_lock(root: &Path, name: &str) -> Result<Option<Option<File>>, String> {
    match open_lock(&root.join(format!("{name}.lock")), false) {
        Ok(file) => Ok(try_lock(&file).then_some(Some(file))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Some(None)),
        Err(_) => Err(FAILED.into()),
    }
}

/// Evict least recently used caches beyond the budget, never one in use.
/// Caller holds the root lock.
fn sweep(root: &Path, budget: u64) -> Result<(), String> {
    sweep_with_size(root, budget, tree_size)
}
fn sweep_with_size(
    root: &Path,
    budget: u64,
    size: impl Fn(&Path) -> Result<u64, String>,
) -> Result<(), String> {
    let mut entries = Vec::new();
    let mut total = 0_u64;
    for entry in fs::read_dir(root).map_err(|_| FAILED)? {
        let entry = entry.map_err(|_| FAILED)?;
        let name = entry.file_name();
        // Unknown entries (for example Finder's .DS_Store) are not cache
        // repositories; leave them alone instead of failing every push.
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(stem) = name
            .strip_suffix(".lock")
            .filter(|stem| is_cache_name(stem))
        {
            // A lock left by a discarded or evicted cache.
            if fs::symlink_metadata(root.join(stem)).is_err() {
                if let Some(Some(_lock)) = idle_lock(root, stem)? {
                    let _ = fs::remove_file(entry.path());
                }
            }
            continue;
        }
        if !is_cache_name(name) {
            continue;
        }
        let path = entry.path();
        let lock = idle_lock(root, name)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(FAILED.into()),
        };
        if !metadata.is_dir() {
            if lock.is_none() {
                continue;
            }
            fs::remove_file(&path).map_err(|_| FAILED)?;
            continue;
        }
        // An active owner can rename Git objects or discard its directory.
        // Its size is advisory until it releases the repository lock.
        let bytes = match size(&path) {
            Ok(bytes) => bytes,
            Err(_) if lock.is_none() => continue,
            Err(error) => return Err(error),
        };
        let Some(lock) = lock else {
            total = total.saturating_add(bytes);
            continue;
        };
        if bytes > budget {
            evict(root, &path, name)?;
            continue;
        }
        let modified = fs::symlink_metadata(path.join(".last-used"))
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        total = total.saturating_add(bytes);
        entries.push((modified, path, name.to_owned(), bytes, lock));
    }
    entries.sort_by_key(|(modified, ..)| *modified);
    for (_, path, name, bytes, _lock) in entries {
        if total <= budget {
            break;
        }
        evict(root, &path, &name)?;
        total = total.saturating_sub(bytes);
    }
    Ok(())
}

/// Caller holds the root lock and this repository's lock.
fn evict(root: &Path, path: &Path, name: &str) -> Result<(), String> {
    fs::remove_dir_all(path).map_err(|_| FAILED)?;
    let _ = fs::remove_file(root.join(format!("{name}.lock")));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// flock references survive an unrelated test worker's fork until exec. Keep
    /// these tests' file descriptor tables private while the outer suite runs in
    /// parallel; serializing only this module cannot exclude other modules' forks.
    fn in_subprocess(test: &str) -> bool {
        const CHILD_TEST: &str = "SILO_CACHE_ISOLATED_TEST";
        let name = format!("host_push_cache::tests::{test}");
        if std::env::var(CHILD_TEST).as_deref() == Ok(name.as_str()) {
            return false;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--nocapture"])
            .env(CHILD_TEST, &name)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stdout.contains("1 passed; 0 failed"),
            "isolated test {name} failed: {}\n{stdout}\n{stderr}",
            output.status
        );
        true
    }
    #[test]
    fn sweep_skips_unknown_entries() {
        if in_subprocess("sweep_skips_unknown_entries") {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        fs::write(temporary.path().join(".DS_Store"), b"finder").unwrap();
        fs::create_dir(temporary.path().join("notes")).unwrap();
        fs::write(temporary.path().join("a".repeat(64)), b"stray").unwrap();
        sweep(temporary.path(), 1024).unwrap();
        assert!(temporary.path().join(".DS_Store").exists());
        assert!(temporary.path().join("notes").exists());
        assert!(!temporary.path().join("a".repeat(64)).exists());
    }
    #[test]
    fn inherited_lock_survives_parent_exit_until_git_child_finishes() {
        if in_subprocess("inherited_lock_survives_parent_exit_until_git_child_finishes") {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("cache");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "host_push_cache::tests::inherited_lock_child",
                "--ignored",
            ])
            .env("SILO_CACHE_LOCK_TEST_ROOT", &root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        // Other repositories are unaffected; this one stays locked by the surviving child.
        drop(acquire(&root, "second").unwrap());
        assert!(acquire(&root, "first")
            .err()
            .unwrap()
            .contains("Another push of this repository"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if acquire(&root, "first").is_ok() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "child did not release cache lock"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    #[ignore = "helper executed by inherited_lock_survives_parent_exit_until_git_child_finishes"]
    fn inherited_lock_child() {
        use std::os::unix::process::CommandExt;
        let root = std::env::var_os("SILO_CACHE_LOCK_TEST_ROOT").expect("helper requires root");
        let cache = acquire(Path::new(&root), "first").unwrap();
        let fd = cache.lock_fd();
        let mut child = std::process::Command::new("/bin/sleep");
        child
            .arg("1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        unsafe {
            child.pre_exec(move || {
                if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        child.spawn().unwrap();
        // Match a crash: process exit skips Cache::drop and its explicit unlock.
        std::process::exit(0);
    }

    #[test]
    fn reuses_same_repository_and_excludes_concurrent_process_handles() {
        if in_subprocess("reuses_same_repository_and_excludes_concurrent_process_handles") {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("cache");
        let cache = acquire(&root, "device/computer/repository").unwrap();
        fs::write(cache.directory.join("retained-object"), b"payload").unwrap();
        assert!(acquire(&root, "device/computer/repository")
            .err()
            .unwrap()
            .contains("Another push of this repository"));
        let directory = cache.directory.clone();
        drop(cache);
        let second = acquire(&root, "device/computer/repository").unwrap();
        assert_eq!(second.directory, directory);
        assert_eq!(
            fs::read(second.directory.join("retained-object")).unwrap(),
            b"payload"
        );
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    #[test]
    fn different_repositories_push_concurrently_and_eviction_skips_caches_in_use() {
        if in_subprocess(
            "different_repositories_push_concurrently_and_eviction_skips_caches_in_use",
        ) {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("cache");
        let first = acquire_with_budget(&root, "first", 100).unwrap();
        fs::write(first.directory.join("objects"), [0; 60]).unwrap();
        // Another computer's push does not wait for, or fail because of, the first one.
        let second = acquire_with_budget(&root, "second", 100).unwrap();
        fs::write(second.directory.join("objects"), [0; 60]).unwrap();
        let second_path = second.directory.clone();
        drop(second);
        // Over budget: the cache still in use survives; the idle one is evicted.
        assert!(first.directory.join("objects").is_file());
        assert!(!second_path.exists());
        let first_path = first.directory.clone();
        drop(first);
        assert!(first_path.exists());
        drop(acquire_with_budget(&root, "first", 100).unwrap());
    }
    #[test]
    fn a_concurrent_discard_does_not_fail_another_repository_sweep() {
        if in_subprocess("a_concurrent_discard_does_not_fail_another_repository_sweep") {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("cache");
        let active = acquire_with_budget(&root, "active", 100).unwrap();
        fs::write(active.directory.join("objects"), b"committed data").unwrap();
        let root_lock = RootLock::take(&root).unwrap();
        // Another repository's scan has enumerated this directory when its
        // owner discards a failed import, retaining the repository lock.
        let result = sweep_with_size(&root, 100, |path| {
            if path == active.directory {
                active.discard();
            }
            tree_size(path)
        });
        assert!(
            result.is_ok(),
            "concurrent discard failed the sweep: {result:?}"
        );
        drop(root_lock);
        let other = acquire_with_budget(&root, "other", 100).unwrap();
        assert!(other.directory.is_dir());
        assert!(acquire_with_budget(&root, "active", 100).is_err());
    }
    #[test]
    fn evicts_oldest_repository_and_removes_single_oversized_repository() {
        if in_subprocess("evicts_oldest_repository_and_removes_single_oversized_repository") {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("cache");
        let first = acquire_with_budget(&root, "first", 100).unwrap();
        let first_path = first.directory.clone();
        fs::write(first.directory.join("objects"), [0; 60]).unwrap();
        drop(first);
        let second = acquire_with_budget(&root, "second", 100).unwrap();
        let second_path = second.directory.clone();
        fs::write(second.directory.join("objects"), [0; 60]).unwrap();
        drop(second);
        assert!(!first_path.exists());
        assert!(second_path.exists());
        let huge = acquire_with_budget(&root, "huge", 100).unwrap();
        let huge_path = huge.directory.clone();
        fs::write(huge.directory.join("objects"), [0; 101]).unwrap();
        drop(huge);
        assert!(!huge_path.exists());
        assert!(second_path.exists());
    }
    #[test]
    fn crash_markers_and_failed_publications_discard_stale_git_locks() {
        if in_subprocess("crash_markers_and_failed_publications_discard_stale_git_locks") {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("cache");
        let cache = acquire(&root, "key").unwrap();
        let directory = cache.directory.clone();
        fs::write(directory.join("retained-object"), b"payload").unwrap();
        drop(cache);
        assert!(!directory.join(".active").exists());
        fs::write(directory.join(".active"), b"crashed").unwrap();
        fs::write(directory.join("config.lock"), b"incomplete configuration").unwrap();
        let recovered = acquire(&root, "key").unwrap();
        assert!(!recovered.directory.join("config.lock").exists());
        assert!(!recovered.directory.join("retained-object").exists());
        assert!(recovered.directory.join(".active").is_file());
        fs::write(directory.join("config.lock"), b"failed operation").unwrap();
        recovered.discard();
        assert!(!directory.exists());
        assert!(
            acquire(&root, "key").is_err(),
            "discard must retain the lock"
        );
        drop(recovered);
        let clean = acquire(&root, "key").unwrap();
        assert!(!clean.directory.join("config.lock").exists());
    }

    #[test]
    fn never_follows_symbolic_links_in_cache_or_lock() {
        if in_subprocess("never_follows_symbolic_links_in_cache_or_lock") {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("cache");
        let target = temporary.path().join("outside");
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &root).unwrap();
        assert!(acquire(&root, "key").is_err());
        fs::remove_file(&root).unwrap();
        fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(&target, root.join(".lock")).unwrap();
        assert!(acquire(&root, "key").is_err());
        fs::remove_file(root.join(".lock")).unwrap();
        let cache = acquire(&root, "key").unwrap();
        std::os::unix::fs::symlink(&target, cache.directory.join("objects")).unwrap();
        drop(cache);
        assert!(acquire(&root, "key").is_err());
        assert!(target.is_dir());
    }
}
