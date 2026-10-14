//! Shared with the pinned runtime patch. Only call from Silo for stopped VMs.
//! Live runtime writers serialize retention and writes with their process lock.
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// Split equally between host execution records and guest console output.
pub const MAX_BYTES: u64 = 250 * 1024 * 1024;
pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
#[allow(dead_code)] // Used by the live runtime writer; desktop only cleans stopped logs.
pub const SEGMENT_AGE: Duration = Duration::from_secs(24 * 60 * 60);

pub fn marker(path: &Path) -> PathBuf {
    let mut marker = path.as_os_str().to_owned();
    marker.push(".started");
    PathBuf::from(marker)
}
pub fn mark(path: &Path, now: SystemTime) -> io::Result<()> {
    if fs::symlink_metadata(marker(path)).is_ok_and(|m| !m.is_file()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "log age marker is not a regular file",
        ));
    }
    fs::write(
        marker(path),
        now.duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
    )
}
pub fn started(path: &Path) -> io::Result<SystemTime> {
    if fs::symlink_metadata(marker(path)).is_ok_and(|m| m.is_file()) {
        if let Ok(value) = fs::read_to_string(marker(path)) {
            if let Ok(seconds) = value.parse::<u64>() {
                if let Some(time) = UNIX_EPOCH.checked_add(Duration::from_secs(seconds)) {
                    return Ok(time);
                }
            }
        }
    }
    let metadata = fs::metadata(path)?;
    // Legacy files have no segment marker. Creation is conservative where available.
    Ok(metadata
        .created()
        .unwrap_or(metadata.modified()?)
        .min(metadata.modified()?))
}
pub fn is_log(name: &str) -> bool {
    ["exec.log", "runtime.log", "kernel.log"]
        .iter()
        .any(|base| {
            name == *base
                || name
                    .strip_prefix(&format!("{base}."))
                    .is_some_and(|suffix| {
                        !suffix.is_empty() && suffix.bytes().all(|c| c.is_ascii_digit())
                    })
        })
}
pub fn enforce(dir: &Path) -> io::Result<()> {
    enforce_at(dir, SystemTime::now(), MAX_BYTES, MAX_AGE)
}
pub fn enforce_at(
    dir: &Path,
    now: SystemTime,
    max_bytes: u64,
    max_age: Duration,
) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !is_log(&entry.file_name().to_string_lossy()) || !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let age = started(&path)?;
        let execution = entry.file_name().to_string_lossy().starts_with("exec.log");
        files.push((age, path, entry.metadata()?.len(), usize::from(execution)));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let budgets = [max_bytes / 2, max_bytes - max_bytes / 2];
    let mut totals = [0_u64; 2];
    for (_, _, size, group) in &files {
        totals[*group] += size;
    }
    for (start, path, size, group) in files {
        if now.duration_since(start).unwrap_or_default() < max_age
            && totals[group] <= budgets[group]
        {
            continue;
        }
        if path.extension().is_some_and(|ext| ext == "log") {
            // Release bytes held by idle writers, then replace the inode so followers
            // detect expiry. The writer reopens the current path after seeing length zero.
            fs::OpenOptions::new().write(true).open(&path)?.set_len(0)?;
            fs::remove_file(&path)?;
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            mark(&path, now)?;
        } else {
            fs::remove_file(&path)?;
            match fs::remove_file(marker(&path)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        totals[group] = totals[group].saturating_sub(size);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // This module is copied into the runtime patch, so keep its tests std-only.
    // The guard also removes fixtures when an assertion unwinds.
    struct TestDir(PathBuf);
    impl std::ops::Deref for TestDir {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn dir() -> TestDir {
        let dir = std::env::temp_dir().join(format!(
            "silo-retention-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        TestDir(dir)
    }
    #[test]
    #[cfg(unix)]
    fn age_marker_preserves_native_directory_bytes() {
        use std::os::unix::ffi::OsStringExt;

        let path = PathBuf::from(std::ffi::OsString::from_vec(
            b"/runtime \xff/logs/exec.log.1".to_vec(),
        ));
        assert_eq!(marker(&path), path.with_file_name("exec.log.1.started"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn retention_expires_logs_under_non_utf8_directories() {
        use std::os::unix::ffi::OsStringExt;

        let root = dir();
        let logs = root.join(std::ffi::OsString::from_vec(b"logs \xff".to_vec()));
        fs::create_dir(&logs).unwrap();
        let path = logs.join("exec.log.1");
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        fs::write(&path, "expired").unwrap();
        mark(&path, now - MAX_AGE).unwrap();
        assert_eq!(started(&path).unwrap(), now - MAX_AGE);
        enforce_at(&logs, now, MAX_BYTES, MAX_AGE).unwrap();
        assert!(!path.exists());
        assert!(!logs.join("exec.log.1.started").exists());
    }

    #[test]
    fn expires_oldest_segment_even_if_recently_modified() {
        let dir = dir();
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let old = dir.join("exec.log.1");
        fs::write(&old, "old and recent records").unwrap();
        mark(&old, now - MAX_AGE).unwrap();
        let fresh = dir.join("kernel.log");
        fs::write(&fresh, "new").unwrap();
        mark(&fresh, now).unwrap();
        enforce_at(&dir, now, MAX_BYTES, MAX_AGE).unwrap();
        assert!(!old.exists());
        assert_eq!(fs::read_to_string(fresh).unwrap(), "new");
    }
    #[test]
    fn console_streams_share_a_budget_and_current_path_survives_expiry() {
        let dir = dir();
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        for (name, seconds) in [("exec.log.1", 3), ("runtime.log.1", 2), ("kernel.log", 1)] {
            let path = dir.join(name);
            fs::write(&path, "12345").unwrap();
            mark(&path, now - Duration::from_secs(seconds)).unwrap();
        }
        fs::write(dir.join("unrelated"), "keep").unwrap();
        enforce_at(&dir, now, 10, MAX_AGE).unwrap();
        assert!(dir.join("exec.log.1").exists());
        assert!(!dir.join("runtime.log.1").exists());
        enforce_at(&dir, now + MAX_AGE, 10, MAX_AGE).unwrap();
        assert_eq!(fs::metadata(dir.join("kernel.log")).unwrap().len(), 0);
        assert!(dir.join("unrelated").exists());
    }
    #[test]
    fn console_flood_cannot_evict_execution_records() {
        let dir = dir();
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        for (name, contents, seconds) in [
            ("exec.log.1", "host", 3),
            ("kernel.log.1", "guest", 2),
            ("runtime.log", "flood", 1),
        ] {
            let path = dir.join(name);
            fs::write(&path, contents).unwrap();
            mark(&path, now - Duration::from_secs(seconds)).unwrap();
        }
        enforce_at(&dir, now, 10, MAX_AGE).unwrap();
        assert_eq!(fs::read_to_string(dir.join("exec.log.1")).unwrap(), "host");
        assert!(!dir.join("kernel.log.1").exists());
        assert_eq!(
            fs::read_to_string(dir.join("runtime.log")).unwrap(),
            "flood"
        );
    }
    #[test]
    fn execution_flood_cannot_evict_console_records() {
        let dir = dir();
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        for (name, contents, seconds) in [
            ("kernel.log.1", "guest", 3),
            ("exec.log.1", "older", 2),
            ("exec.log", "newer", 1),
        ] {
            let path = dir.join(name);
            fs::write(&path, contents).unwrap();
            mark(&path, now - Duration::from_secs(seconds)).unwrap();
        }
        enforce_at(&dir, now, 10, MAX_AGE).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("kernel.log.1")).unwrap(),
            "guest"
        );
        assert!(!dir.join("exec.log.1").exists());
        assert_eq!(fs::read_to_string(dir.join("exec.log")).unwrap(), "newer");
    }
    #[test]
    #[cfg(unix)]
    fn expiry_releases_open_file_bytes_and_changes_generation_for_followers() {
        use std::os::unix::fs::MetadataExt;
        let dir = dir();
        let path = dir.join("kernel.log");
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        fs::write(&path, "expired").unwrap();
        mark(&path, now - MAX_AGE).unwrap();
        let held = fs::File::open(&path).unwrap();
        let old_inode = held.metadata().unwrap().ino();
        enforce_at(&dir, now, MAX_BYTES, MAX_AGE).unwrap();
        assert_eq!(held.metadata().unwrap().len(), 0);
        assert_ne!(fs::metadata(&path).unwrap().ino(), old_inode);
    }

    #[test]
    fn fixture_directory_is_removed_when_an_assertion_unwinds() {
        let mut fixture = PathBuf::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let dir = dir();
            fixture = dir.to_path_buf();
            fs::write(dir.join("fixture"), "test").unwrap();
            panic!("simulate an assertion failure");
        }));
        assert!(result.is_err());
        assert!(!fixture.exists());
    }

    #[test]
    fn only_known_streams_and_numeric_archives_are_logs() {
        assert!(is_log("exec.log.42"));
        assert!(!is_log("exec.log.started"));
        assert!(!is_log("secret.log"));
        assert!(!is_log("kernel.log."));
    }
}
