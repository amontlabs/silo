//! Update policy and durable preferences. This module does not depend on Tauri.
mod schedule;

use schedule::Schedule;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        RwLock, RwLockReadGuard, RwLockWriteGuard,
    },
    time::{Duration, SystemTime},
};

// Existing module locks still order their own operations. This admission gate
// prevents an update from overtaking a queued write, and rejects new writes while
// installation owns the process. Readers never wait behind the installer.
static ADMISSION: RwLock<()> = RwLock::new(());
/// Admitted operations, so readiness can be probed without taking any lock.
static ADMITTED: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct AdmissionGuard(#[allow(dead_code)] RwLockReadGuard<'static, ()>);

impl Drop for AdmissionGuard {
    fn drop(&mut self) {
        ADMITTED.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) fn operation_guard() -> Result<AdmissionGuard, String> {
    crate::runtime::shutdown::ensure_accepting_operations()?;
    let guard = ADMISSION
        .try_read()
        .map_err(|_| "Silo is installing an update. Try again after it restarts.")?;
    ADMITTED.fetch_add(1, Ordering::SeqCst);
    Ok(AdmissionGuard(guard))
}

pub(crate) fn installation_guard() -> Result<RwLockWriteGuard<'static, ()>, String> {
    ADMISSION
        .try_write()
        .map_err(|_| "Wait for active operations to finish before updating.".to_owned())
}

pub(crate) fn readiness(checks: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    if ADMITTED.load(Ordering::SeqCst) > 0 {
        return Err("Wait for active operations to finish before updating.".into());
    }
    checks()
}

pub(crate) const RELEASE_URL: &str = "https://github.com/amontlabs/silo/releases/latest";
pub(crate) const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub(crate) const POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_PREFERENCE_BYTES: u64 = 1024 * 1024;
pub(crate) const PREFERENCE_READ_ERROR: &str =
    "Update preferences could not be read. Save your preference again.";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    pub(crate) phase: String,
    pub(crate) last_checked: Option<String>,
    pub(crate) retry_action: Option<String>,
    pub(crate) current_version: String,
    pub(crate) available_version: Option<String>,
    pub(crate) release_notes: Option<String>,
    pub(crate) downloaded_bytes: u64,
    pub(crate) total_bytes: Option<u64>,
    pub(crate) automatic_checks: bool,
    pub(crate) package_kind: String,
    pub(crate) release_url: String,
    pub(crate) error: Option<String>,
    pub(crate) error_details: Option<String>,
    pub(crate) running_computers: Vec<String>,
    pub(crate) can_install: bool,
    pub(crate) install_block_reason: Option<String>,
    pub(crate) install_status: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct UpdateInfo {
    pub(crate) version: String,
    pub(crate) body: Option<String>,
}

pub(crate) struct State {
    schedule: Schedule,
    pub(crate) snapshot: Snapshot,
    pub(crate) update: Option<UpdateInfo>,
    pub(crate) bytes: Option<Vec<u8>>,
}

impl State {
    pub(crate) fn new(snapshot: Snapshot, now: SystemTime) -> Self {
        Self {
            schedule: Schedule::new(now),
            snapshot,
            update: None,
            bytes: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Preferences {
    automatic_checks: bool,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

pub(crate) fn read_preferences(path: &Path) -> Result<bool, String> {
    Ok(read_saved_preferences(path)?
        .map(|preferences| preferences.automatic_checks)
        .unwrap_or(true))
}

fn read_saved_preferences(path: &Path) -> Result<Option<Preferences>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(PREFERENCE_READ_ERROR.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_PREFERENCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PREFERENCE_READ_ERROR)?;
    if bytes.len() as u64 > MAX_PREFERENCE_BYTES {
        return Err(PREFERENCE_READ_ERROR.into());
    }
    serde_json::from_slice::<Preferences>(&bytes)
        .map(Some)
        .map_err(|_| PREFERENCE_READ_ERROR.into())
}

pub(crate) fn save_preferences(path: &Path, enabled: bool) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Update preference storage is unavailable.")?;
    fs::create_dir_all(parent).map_err(|_| "Update preferences could not be saved.")?;
    let extra = read_saved_preferences(path)
        .ok()
        .flatten()
        .map(|preferences| preferences.extra)
        .unwrap_or_default();
    let bytes = serde_json::to_vec(&Preferences {
        automatic_checks: enabled,
        extra,
    })
    .map_err(|_| "Update preferences could not be saved.")?;
    if bytes.len() as u64 > MAX_PREFERENCE_BYTES {
        return Err(
            "Update preferences are too large to save. Your saved preference was not changed."
                .into(),
        );
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "Update preferences could not be saved.")?;
    std::io::Write::write_all(&mut file, &bytes)
        .map_err(|_| "Update preferences could not be saved.")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "Update preferences could not be saved.")?;
    file.persist(path)
        .map_err(|_| "Update preferences could not be saved.")?;
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|_| "Update preferences could not be saved.".into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BundleKind {
    AppImage,
    Debian,
}

pub(crate) fn package_kind(
    executable: &Path,
    appimage: Option<&Path>,
    bundle: Option<BundleKind>,
) -> &'static str {
    if cfg!(target_os = "macos")
        && executable
            .ancestors()
            .any(|p| p.extension().is_some_and(|e| e == "app"))
    {
        "macos"
    } else if cfg!(target_os = "linux")
        && bundle == Some(BundleKind::AppImage)
        && appimage.is_some_and(|p| p.is_absolute() && p.is_file())
    {
        "appimage"
    } else if cfg!(target_os = "linux")
        && bundle == Some(BundleKind::Debian)
        && executable == Path::new("/usr/bin/silo-ui")
    {
        "debian"
    } else {
        "manual"
    }
}

pub(crate) fn busy(phase: &str) -> bool {
    matches!(phase, "checking" | "downloading" | "installing")
}

pub(crate) fn focused(state: &mut State, now: SystemTime) {
    state.schedule.focus(now);
}

pub(crate) fn automatic_check_due(state: &State, now: SystemTime) -> bool {
    state.schedule.due(
        now,
        state.snapshot.automatic_checks,
        busy(&state.snapshot.phase),
        blocks_automatic_check(
            &state.snapshot.phase,
            state.snapshot.retry_action.as_deref(),
            state.update.is_some(),
            state.bytes.is_some(),
        ),
    )
}

pub(crate) fn begin_check(state: &mut State) -> Result<String, String> {
    if busy(&state.snapshot.phase) {
        return Err("An update operation is already running.".into());
    }
    let previous = std::mem::replace(&mut state.snapshot.phase, "checking".into());
    state.snapshot.error = None;
    state.snapshot.error_details = None;
    Ok(previous)
}

pub(crate) fn complete_check(state: &mut State, now: SystemTime, success: bool) {
    state.schedule.completed(now, success);
}

pub(crate) fn check_failed(
    state: &mut State,
    now: SystemTime,
    previous_phase: &str,
    message: &str,
    details: impl ToString,
) {
    complete_check(state, now, false);
    state.snapshot.phase = if matches!(previous_phase, "available" | "ready") {
        previous_phase.into()
    } else {
        "error".into()
    };
    state.snapshot.error = Some(message.into());
    state.snapshot.error_details = Some(details.to_string());
    state.snapshot.retry_action = Some("check".into());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckErrorKind {
    ReleaseUnavailable,
    InvalidRelease,
    NoPlatformRelease,
    Other,
}

pub(crate) fn check_error_message(kind: CheckErrorKind) -> &'static str {
    match kind {
        CheckErrorKind::ReleaseUnavailable => "The update service is unavailable. Try again later.",
        CheckErrorKind::InvalidRelease => {
            "The update service returned invalid release information. Try again later."
        }
        CheckErrorKind::NoPlatformRelease => {
            "This release has no update for your platform. Try again later."
        }
        CheckErrorKind::Other => "Could not check for updates. Try again later.",
    }
}

pub(crate) fn no_feed_available(state: &mut State) {
    state.snapshot.phase = "idle".into();
    state.snapshot.error = None;
    state.snapshot.error_details = None;
}

pub(crate) fn record_failure(state: &mut State, message: &str, details: impl ToString) -> Snapshot {
    state.snapshot.phase = "error".into();
    state.snapshot.error = Some(message.into());
    state.snapshot.error_details = Some(details.to_string());
    state.snapshot.retry_action = Some(
        if state.bytes.is_some()
            || (state.snapshot.package_kind == "debian" && state.update.is_some())
        {
            "install"
        } else if state.update.is_some() {
            "download"
        } else {
            "check"
        }
        .into(),
    );
    state.snapshot.clone()
}

pub(crate) fn begin_download(state: &mut State) -> Result<(), String> {
    if busy(&state.snapshot.phase) {
        return Err("An update operation is already running.".into());
    }
    if matches!(state.snapshot.package_kind.as_str(), "manual" | "debian") {
        return Err("Download the package from Releases to update this installation.".into());
    }
    if state.update.is_none() {
        return Err("Check for an update before downloading.".into());
    }
    state.snapshot.phase = "downloading".into();
    state.snapshot.error = None;
    state.snapshot.error_details = None;
    state.snapshot.downloaded_bytes = 0;
    state.snapshot.total_bytes = None;
    state.bytes = None;
    Ok(())
}

pub(crate) fn download_progress(state: &mut State, received: u64, total: Option<u64>) {
    state.snapshot.downloaded_bytes = received;
    state.snapshot.total_bytes = total;
}

pub(crate) fn download_ready(state: &mut State, bytes: Vec<u8>) {
    state.snapshot.phase = "ready".into();
    state.snapshot.downloaded_bytes = bytes.len() as u64;
    state.snapshot.total_bytes = Some(bytes.len() as u64);
    state.bytes = Some(bytes);
}

pub(crate) fn begin_install(
    state: &mut State,
    has_updater: bool,
) -> Result<(Vec<u8>, bool), String> {
    if busy(&state.snapshot.phase) {
        return Err("An update operation is already running.".into());
    }
    if state.snapshot.package_kind == "manual" {
        return Err("Install the downloaded package to update this installation.".into());
    }
    if state.update.is_none() || !has_updater {
        return Err("Download and verify an update before installing.".into());
    }
    let is_debian = state.snapshot.package_kind == "debian";
    let bytes = if is_debian {
        Vec::new()
    } else {
        state
            .bytes
            .take()
            .ok_or("Download and verify an update before installing.")?
    };
    state.snapshot.phase = "installing".into();
    state.snapshot.install_status = None;
    state.snapshot.error = None;
    state.snapshot.error_details = None;
    Ok((bytes, is_debian))
}

pub(crate) fn restore_install_bytes(state: &mut State, bytes: Vec<u8>) {
    state.bytes = Some(bytes);
}

pub(crate) fn update_automatic_checks(
    state: &mut State,
    preferences: &Path,
    enabled: bool,
    persist: impl FnOnce(&Path, bool) -> Result<(), String>,
) -> Result<Snapshot, String> {
    persist(preferences, enabled)?;
    if enabled && !state.snapshot.automatic_checks {
        state.schedule.enable(SystemTime::now());
    }
    state.snapshot.automatic_checks = enabled;
    if state.snapshot.phase == "error"
        && state.snapshot.error.as_deref() == Some(PREFERENCE_READ_ERROR)
    {
        state.snapshot.phase = "idle".into();
        state.snapshot.error = None;
        state.snapshot.error_details = None;
        state.snapshot.retry_action = None;
    }
    Ok(state.snapshot.clone())
}

pub(crate) fn blocks_automatic_check(
    phase: &str,
    retry_action: Option<&str>,
    has_update: bool,
    has_bytes: bool,
) -> bool {
    has_bytes || (has_update && phase != "available") || retry_action == Some("relaunch")
}

/// Apply a successful check. A verified download of the same version survives the
/// check; returns true when the pending update (and its bytes) should be kept.
pub(crate) fn settle_check(state: &mut State, found: Option<UpdateInfo>) -> bool {
    let keep = state.bytes.is_some()
        && found.as_ref().is_some_and(|update| {
            state
                .update
                .as_ref()
                .is_some_and(|pending| pending.version == update.version)
        });
    state.snapshot.retry_action = None;
    if keep {
        state.snapshot.phase = "ready".into();
    } else {
        state.bytes = None;
        state.snapshot.phase = if found.is_some() { "available" } else { "idle" }.into();
        state.snapshot.downloaded_bytes = 0;
        state.snapshot.total_bytes = None;
    }
    state.snapshot.available_version = found.as_ref().map(|update| update.version.clone());
    state.snapshot.release_notes = found.and_then(|update| update.body);
    keep
}

pub(crate) fn validate_download_size(size: u64) -> Result<(), String> {
    if size > MAX_DOWNLOAD_BYTES {
        Err("The update exceeds the supported download size.".into())
    } else {
        Ok(())
    }
}

pub(crate) fn unpacked_size(bytes: &[u8], macos: bool) -> Result<u64, String> {
    if !macos {
        return Ok(bytes.len() as u64);
    }
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    let mut total = 0u64;
    for entry in archive
        .entries()
        .map_err(|_| "The update archive could not be read.")?
    {
        let entry = entry.map_err(|_| "The update archive is invalid.")?;
        let size = entry
            .header()
            .size()
            .map_err(|_| "The update archive size is invalid.")?;
        total = total
            .checked_add(size)
            .filter(|sum| *sum <= 8 * 1024 * 1024 * 1024)
            .ok_or("The unpacked update exceeds the supported size.")?;
    }
    if total == 0 {
        return Err("The update archive is empty.".into());
    }
    Ok(total)
}

pub(crate) fn preflight_destination(
    destination: &Path,
    bytes: &[u8],
    macos: bool,
    free_space: impl FnOnce(&Path) -> Result<u64, String>,
) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or("The installation folder could not be located.")?;
    // Test the actual staging directory, rather than assuming Unix mode bits mean writable.
    let probe = tempfile::NamedTempFile::new_in(parent).map_err(|_| "Silo cannot write to its installation folder. Move it to a writable folder or install the new package manually.")?;
    drop(probe);
    let needed = unpacked_size(bytes, macos)?;
    let free = free_space(parent)?;
    // Existing installation already occupies space. Atomic replacement stages only
    // one new copy beside it; the downloaded archive is held in memory.
    if free < needed {
        return Err(format!(
            "Not enough space to install the update. Free at least {} MiB and retry.",
            needed.saturating_sub(free).div_ceil(1024 * 1024)
        ));
    }
    Ok(())
}

pub(crate) fn available_install_space(parent: &Path) -> Result<u64, String> {
    use std::os::unix::ffi::OsStrExt;
    let encoded = std::ffi::CString::new(parent.as_os_str().as_bytes())
        .map_err(|_| "The installation folder is invalid.")?;
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: CString is NUL terminated and stats is a valid exclusive output pointer.
    if unsafe { libc::statvfs(encoded.as_ptr(), &mut stats) } != 0 {
        return Err("Available installation space could not be checked.".into());
    }
    (stats.f_bavail as u64)
        .checked_mul(stats.f_frsize as u64)
        .ok_or_else(|| "Available installation space is invalid.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(phase: &str) -> Snapshot {
        Snapshot {
            phase: phase.into(),
            last_checked: None,
            retry_action: None,
            current_version: "1.0.0".into(),
            available_version: None,
            release_notes: None,
            downloaded_bytes: 0,
            total_bytes: None,
            automatic_checks: true,
            package_kind: "macos".into(),
            release_url: RELEASE_URL.into(),
            error: None,
            error_details: None,
            running_computers: vec![],
            can_install: false,
            install_block_reason: None,
            install_status: None,
        }
    }

    #[test]
    fn recheck_keeps_a_verified_download_and_replaces_a_superseded_release() {
        let mut state = State::new(snapshot("checking"), SystemTime::now());
        state.snapshot.downloaded_bytes = 3;
        state.snapshot.total_bytes = Some(3);
        state.update = Some(UpdateInfo {
            version: "1.2.0".into(),
            body: None,
        });
        state.bytes = Some(vec![1, 2, 3]);
        assert!(settle_check(
            &mut state,
            Some(UpdateInfo {
                version: "1.2.0".into(),
                body: None,
            })
        ));
        assert_eq!(state.snapshot.phase, "ready");
        assert_eq!(state.bytes.as_deref(), Some(&[1, 2, 3][..]));
        assert_eq!(
            (state.snapshot.downloaded_bytes, state.snapshot.total_bytes),
            (3, Some(3))
        );
        assert_eq!(state.snapshot.available_version.as_deref(), Some("1.2.0"));

        assert!(!settle_check(
            &mut state,
            Some(UpdateInfo {
                version: "1.3.0".into(),
                body: Some("notes".into()),
            })
        ));
        assert_eq!(state.snapshot.phase, "available");
        assert!(state.bytes.is_none());
        assert_eq!(
            (state.snapshot.downloaded_bytes, state.snapshot.total_bytes),
            (0, None)
        );
        assert_eq!(state.snapshot.available_version.as_deref(), Some("1.3.0"));
        assert_eq!(state.snapshot.release_notes.as_deref(), Some("notes"));
        assert!(!settle_check(
            &mut state,
            Some(UpdateInfo {
                version: "1.3.0".into(),
                body: None,
            })
        ));
        assert_eq!(state.snapshot.phase, "available");
        assert!(!settle_check(&mut state, None));
        assert_eq!(state.snapshot.phase, "idle");
        assert!(state.snapshot.available_version.is_none());
    }

    #[test]
    fn automatic_checks_run_while_an_update_is_only_available() {
        assert!(!blocks_automatic_check("available", None, true, false));
        assert!(!blocks_automatic_check("idle", None, false, false));
        assert!(!blocks_automatic_check(
            "error",
            Some("check"),
            false,
            false
        ));
        assert!(blocks_automatic_check("ready", None, true, true));
        assert!(blocks_automatic_check(
            "error",
            Some("download"),
            true,
            false
        ));
        assert!(blocks_automatic_check("error", Some("install"), true, true));
        assert!(blocks_automatic_check(
            "error",
            Some("relaunch"),
            false,
            false
        ));
    }

    #[test]
    fn readiness_probe_never_rejects_concurrent_operations() {
        readiness(|| {
            let _admitted = operation_guard().expect("readiness must not block admission");
            Ok(())
        })
        .unwrap();
        let active = operation_guard().unwrap();
        assert!(readiness(|| Ok(()))
            .unwrap_err()
            .contains("active operations"));
        drop(active);
        readiness(|| Ok(())).unwrap();
    }

    #[test]
    fn concurrent_preference_changes_keep_disk_and_snapshot_in_agreement() {
        let directory = tempfile::tempdir().unwrap();
        let preferences = directory.path().join("prefs.json");
        let state = std::sync::Mutex::new(State::new(snapshot("idle"), SystemTime::now()));
        let (first_saved, saved) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let (second_saved, second) = std::sync::mpsc::channel();
        std::thread::scope(|threads| {
            let state_ref = &state;
            let preferences_ref = &preferences;
            let first = threads.spawn(move || {
                let mut state = state_ref.lock().unwrap();
                update_automatic_checks(&mut state, preferences_ref, false, |path, enabled| {
                    save_preferences(path, enabled)?;
                    first_saved.send(()).unwrap();
                    released.recv().unwrap();
                    Ok(())
                })
            });
            saved.recv().unwrap();
            let state_ref = &state;
            let preferences_ref = &preferences;
            let last = threads.spawn(move || {
                let mut state = state_ref.lock().unwrap();
                update_automatic_checks(&mut state, preferences_ref, true, |path, enabled| {
                    save_preferences(path, enabled)?;
                    second_saved.send(()).unwrap();
                    Ok(())
                })
            });
            let overtook = second.recv_timeout(Duration::from_secs(1)).is_ok();
            release.send(()).unwrap();
            first.join().unwrap().unwrap();
            last.join().unwrap().unwrap();
            assert!(
                !overtook,
                "a second save overtook an unpublished preference"
            );
        });
        assert_eq!(
            read_preferences(&preferences).unwrap(),
            state.lock().unwrap().snapshot.automatic_checks
        );
    }

    #[test]
    fn failed_preference_save_preserves_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let preferences = directory.path().join("prefs.json");
        let mut state = State::new(snapshot("idle"), SystemTime::now());
        let error = update_automatic_checks(&mut state, &preferences, false, |_, _| {
            Err("save failed".into())
        })
        .unwrap_err();
        assert_eq!(error, "save failed");
        assert!(state.snapshot.automatic_checks);
    }

    #[test]
    fn saving_preferences_clears_the_read_error_but_preserves_update_failures() {
        let directory = tempfile::tempdir().unwrap();
        let preferences = directory.path().join("prefs.json");
        fs::write(&preferences, "broken").unwrap();
        let mut initial = snapshot("error");
        initial.error = Some(read_preferences(&preferences).unwrap_err());
        initial.automatic_checks = false;
        let mut state = State::new(initial, SystemTime::now());
        let repaired =
            update_automatic_checks(&mut state, &preferences, false, save_preferences).unwrap();
        assert!(repaired.error.is_none());
        assert_eq!(repaired.phase, "idle");
        assert!(!read_preferences(&preferences).unwrap());
        state.snapshot.phase = "error".into();
        state.snapshot.error = Some("Download failed".into());
        state.snapshot.retry_action = Some("download".into());
        let unrelated =
            update_automatic_checks(&mut state, &preferences, false, save_preferences).unwrap();
        assert_eq!(unrelated.error.as_deref(), Some("Download failed"));
        assert_eq!(unrelated.phase, "error");
        assert_eq!(unrelated.retry_action.as_deref(), Some("download"));
    }

    #[test]
    fn preferences_large_input_memory_is_bounded() {
        const PROBE: &str = "SILO_TEST_UPDATE_PREFERENCES_MEMORY_PROBE";
        if std::env::var_os(PROBE).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "updates::tests::preferences_large_input_memory_is_bounded",
                    "--nocapture",
                ])
                .env(PROBE, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed;"));
            return;
        }

        fn peak_bytes() -> u64 {
            let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
            assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) }, 0);
            #[cfg(target_os = "macos")]
            return usage.ru_maxrss as u64;
            #[cfg(not(target_os = "macos"))]
            return usage.ru_maxrss as u64 * 1024;
        }

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("prefs.json");
        fs::File::create(&path)
            .unwrap()
            .set_len(128 * 1024 * 1024)
            .unwrap();
        let before = peak_bytes();
        assert_eq!(read_preferences(&path).unwrap_err(), PREFERENCE_READ_ERROR);
        let extra = peak_bytes().saturating_sub(before);
        assert!(
            extra < 32 * 1024 * 1024,
            "oversized preferences allocated {extra} bytes"
        );
        assert_eq!(fs::metadata(&path).unwrap().len(), 128 * 1024 * 1024);
        save_preferences(&path, false).unwrap();
        assert!(!read_preferences(&path).unwrap());
    }

    #[test]
    fn preference_limits_and_unknown_fields_are_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("prefs.json");
        let mut bytes = br#"{"automatic_checks":false}"#.to_vec();
        bytes.resize(MAX_PREFERENCE_BYTES as usize, b' ');
        fs::write(&path, &bytes).unwrap();
        assert!(!read_preferences(&path).unwrap());
        bytes.push(b' ');
        fs::write(&path, &bytes).unwrap();
        assert_eq!(read_preferences(&path).unwrap_err(), PREFERENCE_READ_ERROR);
        assert_eq!(fs::read(&path).unwrap(), bytes);

        let saved = serde_json::json!({
            "automatic_checks": false,
            "future_preference": {"channel": "preview", "days": [1, 3, 5]}
        });
        fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        save_preferences(&path, true).unwrap();
        let written: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["automatic_checks"], true);
        assert_eq!(written["future_preference"], saved["future_preference"]);
    }

    #[test]
    fn malformed_known_preferences_are_not_accepted_as_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("prefs.json");
        for bytes in [
            br#"{"automatic_checks":"false","future_preference":true}"#.as_slice(),
            br#"{"automatic_checks":null,"future_preference":true}"#,
            br#"{"future_preference":true}"#,
        ] {
            fs::write(&path, bytes).unwrap();
            assert!(read_preferences(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn missing_preferences_enable_checks_but_corrupt_preferences_do_not() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("prefs.json");
        assert!(read_preferences(&path).unwrap());
        save_preferences(&path, false).unwrap();
        assert!(!read_preferences(&path).unwrap());
        fs::write(&path, "broken").unwrap();
        assert!(read_preferences(&path).is_err());
    }

    #[test]
    fn ordinary_executable_does_not_claim_updatable_package() {
        assert_eq!(
            package_kind(Path::new("/usr/bin/silo-ui"), None, None),
            "manual"
        );
    }

    #[test]
    fn preflight_counts_actual_tar_members_and_rejects_invalid_archive() {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for size in [16usize, 32] {
            let mut header = tar::Header::new_gnu();
            header.set_size(size as u64);
            header.set_mode(0o644);
            header.set_cksum();
            archive
                .append_data(
                    &mut header,
                    format!("Silo.app/file{size}"),
                    vec![0u8; size].as_slice(),
                )
                .unwrap();
        }
        let bytes = archive.into_inner().unwrap().finish().unwrap();
        assert_eq!(unpacked_size(&bytes, true).unwrap(), 48);
        assert_eq!(unpacked_size(&bytes, false).unwrap(), bytes.len() as u64);
        assert!(unpacked_size(b"not an archive", true).is_err());
    }

    #[test]
    fn package_classification_uses_the_bundled_installer_kind() {
        let image = tempfile::NamedTempFile::new().unwrap();
        assert_eq!(
            package_kind(
                Path::new("/usr/bin/silo-ui"),
                Some(image.path()),
                Some(BundleKind::Debian)
            ),
            if cfg!(target_os = "linux") {
                "debian"
            } else {
                "manual"
            }
        );
    }

    #[test]
    fn download_size_limit_rejects_overflow() {
        assert!(validate_download_size(MAX_DOWNLOAD_BYTES).is_ok());
        assert!(validate_download_size(MAX_DOWNLOAD_BYTES + 1).is_err());
        assert!(validate_download_size(u64::MAX).is_err());
    }

    #[test]
    fn preflight_rejects_insufficient_space_without_touching_installation() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("Silo.AppImage");
        fs::write(&destination, "old app").unwrap();
        let payload = vec![0; 2 * 1024 * 1024];
        let error = preflight_destination(&destination, &payload, false, |_| Ok(0)).unwrap_err();
        assert_eq!(
            error,
            "Not enough space to install the update. Free at least 2 MiB and retry."
        );
        assert_eq!(fs::read(&destination).unwrap(), b"old app");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn preflight_rejects_unwritable_parent_before_measuring_space() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("not-a-directory");
        fs::write(&parent, "preserve").unwrap();
        let error = preflight_destination(&parent.join("Silo.AppImage"), b"new app", false, |_| {
            panic!("must reject unwritable staging first")
        })
        .unwrap_err();
        assert!(error.contains("cannot write"));
        assert_eq!(fs::read(parent).unwrap(), b"preserve");
    }

    #[test]
    fn a_preference_save_that_exceeds_the_limit_preserves_the_original() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("prefs.json");
        let mut preferences = serde_json::json!({
            "automatic_checks": true,
            "future_preference": ""
        });
        let padding =
            MAX_PREFERENCE_BYTES as usize - serde_json::to_vec(&preferences).unwrap().len();
        preferences["future_preference"] = serde_json::Value::String("x".repeat(padding));
        let bytes = serde_json::to_vec(&preferences).unwrap();
        assert_eq!(bytes.len(), MAX_PREFERENCE_BYTES as usize);
        fs::write(&path, &bytes).unwrap();
        assert!(save_preferences(&path, false).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(read_preferences(&path).unwrap());
    }
}
