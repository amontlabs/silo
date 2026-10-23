//! The package installer's claim on an engine update (design/p2 §3.3 step 5 and §3.4).
//!
//! The installer coordinator holds an exclusive `flock` on `installer.lock` in the engine
//! directory from before `engine.update.prepare` until the new package is verified. The lock
//! disappears with the coordinator process, so the engine learns that it died by probing the
//! lock: a probe that succeeds means nobody holds it, and the probe releases it at once.
//!
//! * While an update is prepared, a watchdog probes every [`PROBE_INTERVAL`]; after
//!   [`ABANDONED_AFTER`] of continuously finding no coordinator it aborts the update.
//! * An engine that starts with a prepared, releasing or released journal while the lock is
//!   held, and whose version is the one that prepared it (an old engine relaunched by the
//!   service manager), must not resume computers behind the installer. It waits, with
//!   ordinary admission closed, until the coordinator finishes or disappears.
use std::{
    fs,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const LOCK_FILE: &str = "installer.lock";
/// How often the engine probes for the coordinator while an update is held.
pub const PROBE_INTERVAL: Duration = Duration::from_secs(5);
/// How long the coordinator must be continuously absent before the engine acts.
pub const ABANDONED_AFTER: Duration = Duration::from_secs(30);

pub fn lock_path(engine_directory: &Path) -> PathBuf {
    engine_directory.join(LOCK_FILE)
}

/// Whether a coordinator currently holds the install lock. A missing lock file means no
/// coordinator; a lock that cannot be examined counts as held, because acting on a guess
/// could resume computers behind a live installer.
pub fn coordinator_present(engine_directory: &Path) -> bool {
    let file = match fs::File::open(lock_path(engine_directory)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    // SAFETY: `file` is an open descriptor for the duration of both calls.
    let acquired = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    if acquired {
        // SAFETY: as above. The probe never keeps the lock.
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        false
    } else {
        true
    }
}

/// The coordinator's side of the lock, for the installer and for tests. Held until dropped.
pub struct Held(fs::File);

impl Held {
    pub fn take(engine_directory: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(engine_directory)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path(engine_directory))?;
        // SAFETY: `file` is an open descriptor.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self(file))
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // SAFETY: the descriptor is open until the file drops.
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// Counts how long the coordinator has been continuously absent.
#[derive(Clone, Copy, Debug, Default)]
pub struct Watchdog {
    absent_since: Option<Instant>,
}

impl Watchdog {
    /// Records one probe at `now`. True once the coordinator has been absent for
    /// [`ABANDONED_AFTER`] without interruption.
    pub fn observe(&mut self, coordinator_present: bool, now: Instant) -> bool {
        if coordinator_present {
            self.absent_since = None;
            return false;
        }
        let since = *self.absent_since.get_or_insert(now);
        now.saturating_duration_since(since) >= ABANDONED_AFTER
    }

    pub fn reset(&mut self) {
        self.absent_since = None;
    }
}
