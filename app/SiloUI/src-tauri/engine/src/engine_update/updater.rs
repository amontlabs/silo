//! The engine's own update schedule and signed-package state (`updates.*`, 60 §5).
//!
//! This is the Tauri-free counterpart of the app updater: the same policy functions in
//! [`crate::updates`] drive a [`Provider`] instead of the Tauri updater plugin. A Fjord app
//! update never goes through here; only a new *engine* version does, and replacing the engine
//! is the external installer's job (`engine.update.prepare` and `engine.shutdown`).
use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::{Duration, SystemTime},
};

use silo_api::methods::updates::UpdateSnapshot;

use crate::updates::{self, CheckErrorKind, Snapshot, State, UpdateInfo, MAX_DOWNLOAD_BYTES};

/// Why a check failed, already classified for the user-facing message.
#[derive(Clone, Debug)]
pub struct CheckFailure {
    pub kind: CheckErrorKind,
    pub details: String,
}

/// Where engine releases come from. The signed feed itself is undecided (C-9); a provider
/// supplies it without changing the policy around it.
pub trait Provider: Send + Sync {
    /// The newest engine release newer than the running one, if any.
    fn check(&self) -> Result<Option<UpdateInfo>, CheckFailure>;

    /// Downloads and verifies the release found by the last check, reporting
    /// `(received, total)`.
    fn download(&self, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<Vec<u8>, String>;
}

/// The provider of an engine with no configured feed: it never finds an update.
pub struct NoFeed;

impl Provider for NoFeed {
    fn check(&self) -> Result<Option<UpdateInfo>, CheckFailure> {
        Ok(None)
    }

    fn download(&self, _: &mut dyn FnMut(u64, Option<u64>)) -> Result<Vec<u8>, String> {
        Err("Check for an update before downloading.".into())
    }
}

const DOWNLOAD_FAILURE: &str =
    "The update could not be downloaded or verified. Your installation was not changed. Try again.";

pub struct Controller {
    preferences: PathBuf,
    development: bool,
    provider: Box<dyn Provider>,
    state: Mutex<State>,
}

impl Controller {
    pub fn new(
        preferences: PathBuf,
        current_version: String,
        development: bool,
        provider: Box<dyn Provider>,
    ) -> Self {
        let preference = updates::read_preferences(&preferences);
        let error = preference.as_ref().err().cloned();
        // Development builds come from source and never replace themselves with a release.
        let automatic = preference.unwrap_or(false) && !development;
        let snapshot = Snapshot {
            last_checked: None,
            retry_action: None,
            phase: if error.is_some() { "error" } else { "idle" }.into(),
            current_version,
            available_version: None,
            release_notes: None,
            downloaded_bytes: 0,
            total_bytes: None,
            automatic_checks: automatic,
            package_kind: "macos".into(),
            release_url: updates::RELEASE_URL.into(),
            error,
            error_details: None,
            running_computers: Vec::new(),
            can_install: false,
            install_block_reason: None,
            install_status: None,
        };
        Self {
            preferences,
            development,
            provider,
            state: Mutex::new(State::new(snapshot, SystemTime::now())),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        crate::sync::lock_or_recover(&self.state, "engine update state")
    }

    pub fn snapshot(&self) -> UpdateSnapshot {
        to_api(&self.lock().snapshot)
    }

    /// Applies the readiness the engine measured: whether an engine update could be prepared
    /// now, and the computers it would save and resume.
    pub fn apply_readiness(&self, readiness: Result<Vec<String>, String>) -> UpdateSnapshot {
        let mut state = self.lock();
        state.snapshot.can_install = readiness.is_ok();
        match readiness {
            Ok(running) => {
                state.snapshot.install_block_reason = None;
                state.snapshot.running_computers = running;
            }
            Err(reason) => state.snapshot.install_block_reason = Some(reason),
        }
        to_api(&state.snapshot)
    }

    pub fn focused(&self) {
        updates::focused(&mut self.lock(), SystemTime::now());
    }

    pub fn set_automatic_checks(&self, enabled: bool) -> Result<UpdateSnapshot, String> {
        let mut state = self.lock();
        updates::update_automatic_checks(
            &mut state,
            &self.preferences,
            enabled && !self.development,
            updates::save_preferences,
        )
        .map(|snapshot| to_api(&snapshot))
    }

    /// Checks for a release. An automatic check does nothing until one is due.
    pub fn check(&self, automatic: bool) -> Result<UpdateSnapshot, String> {
        if self.development {
            // No network request: the development channel has no update feed.
            let mut state = self.lock();
            updates::no_feed_available(&mut state);
            return Ok(to_api(&state.snapshot));
        }
        let previous = {
            let mut state = self.lock();
            if automatic && !updates::automatic_check_due(&state, SystemTime::now()) {
                return Ok(to_api(&state.snapshot));
            }
            updates::begin_check(&mut state)?
        };
        let result = self.provider.check();
        let mut state = self.lock();
        match result {
            Ok(found) => {
                updates::complete_check(&mut state, SystemTime::now(), true);
                let keep = updates::settle_check(&mut state, found.clone());
                if !keep {
                    state.update = found;
                }
                state.snapshot.last_checked = time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .ok();
            }
            Err(failure) => {
                let message = updates::check_error_message(failure.kind);
                if state.update.is_none() {
                    updates::complete_check(&mut state, SystemTime::now(), false);
                    updates::record_failure(&mut state, message, failure.details);
                } else {
                    // A failed re-check leaves a found or downloaded update actionable.
                    updates::check_failed(
                        &mut state,
                        SystemTime::now(),
                        &previous,
                        message,
                        failure.details,
                    );
                }
            }
        }
        Ok(to_api(&state.snapshot))
    }

    /// Validates a download request and marks it started. Returns the snapshot to publish.
    pub fn begin_download(&self) -> Result<Snapshot, String> {
        let mut state = self.lock();
        updates::begin_download(&mut state)?;
        Ok(state.snapshot.clone())
    }

    /// Downloads the release `begin_download` accepted. `progress` receives the snapshot at
    /// most every 100 ms.
    pub fn download(&self, mut progress: impl FnMut(&Snapshot)) -> Result<UpdateSnapshot, String> {
        let mut last_report = std::time::Instant::now() - Duration::from_secs(1);
        let mut received = 0u64;
        let outcome = self.provider.download(&mut |bytes, total| {
            received = bytes;
            if last_report.elapsed() >= Duration::from_millis(100) {
                last_report = std::time::Instant::now();
                let mut state = self.lock();
                updates::download_progress(&mut state, bytes, total);
                progress(&state.snapshot);
            }
        });
        let outcome = outcome.and_then(|bytes| {
            updates::validate_download_size(bytes.len() as u64)?;
            if received > MAX_DOWNLOAD_BYTES {
                return Err("The update exceeds the supported download size.".into());
            }
            Ok(bytes)
        });
        let mut state = self.lock();
        match outcome {
            Ok(bytes) => {
                updates::download_ready(&mut state, bytes);
                Ok(to_api(&state.snapshot))
            }
            Err(details) => {
                updates::record_failure(&mut state, DOWNLOAD_FAILURE, details);
                Err(DOWNLOAD_FAILURE.to_owned())
            }
        }
    }

    /// The snapshot as of a failed download or any other change, for publication.
    pub fn failure_snapshot(&self) -> UpdateSnapshot {
        to_api(&self.lock().snapshot)
    }

    /// Records a failure of the computers' resumption after an update.
    pub fn recovery_failed(&self, message: &str, details: &str) -> UpdateSnapshot {
        let mut state = self.lock();
        updates::record_failure(&mut state, message, details);
        to_api(&state.snapshot)
    }
}

/// `UpdateSnapshot` omits the obsolete Debian helper field (60 §5.1).
pub fn to_api(snapshot: &Snapshot) -> UpdateSnapshot {
    let mut value = serde_json::to_value(snapshot).expect("update snapshots serialize");
    if let Some(fields) = value.as_object_mut() {
        fields.remove("installStatus");
    }
    serde_json::from_value(value).expect("update snapshots match the API type")
}

/// The job-progress projection of the same state, which keeps every source field.
pub fn to_progress(
    snapshot: &Snapshot,
) -> Result<silo_api::methods::updates::UpdateProgressData, String> {
    let value = serde_json::to_value(snapshot).map_err(|error| error.to_string())?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    type Found = Arc<Mutex<Result<Option<UpdateInfo>, CheckFailure>>>;

    struct Fixture {
        found: Found,
        payload: Vec<u8>,
        checks: Arc<Mutex<u32>>,
    }

    impl Provider for Fixture {
        fn check(&self) -> Result<Option<UpdateInfo>, CheckFailure> {
            *self.checks.lock().unwrap() += 1;
            self.found.lock().unwrap().clone()
        }

        fn download(&self, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<Vec<u8>, String> {
            progress(2, Some(self.payload.len() as u64));
            Ok(self.payload.clone())
        }
    }

    struct Harness {
        controller: Controller,
        checks: Arc<Mutex<u32>>,
        found: Found,
    }

    fn harness(
        directory: &tempfile::TempDir,
        development: bool,
        found: Result<Option<UpdateInfo>, CheckFailure>,
    ) -> Harness {
        let checks = Arc::new(Mutex::new(0));
        let found = Arc::new(Mutex::new(found));
        let controller = Controller::new(
            directory.path().join("update-preferences.json"),
            "1.0.0".into(),
            development,
            Box::new(Fixture {
                found: found.clone(),
                payload: vec![1, 2, 3],
                checks: checks.clone(),
            }),
        );
        Harness {
            controller,
            checks,
            found,
        }
    }

    fn release() -> UpdateInfo {
        UpdateInfo {
            version: "1.1.0".into(),
            body: Some("notes".into()),
        }
    }

    #[test]
    fn a_check_finds_a_release_and_a_download_verifies_it() {
        let directory = tempfile::tempdir().unwrap();
        let harness = harness(&directory, false, Ok(Some(release())));
        let controller = &harness.controller;
        assert!(controller.begin_download().is_err());
        let found = controller.check(false).unwrap();
        assert_eq!(found.phase, "available");
        assert_eq!(found.available_version.as_deref(), Some("1.1.0"));
        assert!(found.last_checked.is_some());
        let started = controller.begin_download().unwrap();
        assert_eq!(started.phase, "downloading");
        let mut reports = 0;
        let done = controller.download(|_| reports += 1).unwrap();
        assert_eq!(done.phase, "ready");
        assert_eq!((done.downloaded_bytes, done.total_bytes), (3, Some(3)));
        assert_eq!(reports, 1);
    }

    #[test]
    fn the_development_channel_never_asks_the_provider() {
        let directory = tempfile::tempdir().unwrap();
        let harness = harness(&directory, true, Ok(Some(release())));
        assert!(!harness.controller.snapshot().automatic_checks);
        let snapshot = harness.controller.check(false).unwrap();
        assert_eq!(snapshot.phase, "idle");
        assert_eq!(*harness.checks.lock().unwrap(), 0);
        // The preference is stored but never enables checks on this channel.
        assert!(
            !harness
                .controller
                .set_automatic_checks(true)
                .unwrap()
                .automatic_checks
        );
    }

    #[test]
    fn a_failed_check_is_a_retryable_error_and_keeps_a_found_release() {
        let directory = tempfile::tempdir().unwrap();
        let harness = harness(&directory, false, Ok(Some(release())));
        harness.controller.check(false).unwrap();
        *harness.found.lock().unwrap() = Err(CheckFailure {
            kind: CheckErrorKind::ReleaseUnavailable,
            details: "offline".into(),
        });
        let after = harness.controller.check(false).unwrap();
        assert_eq!(after.phase, "available");
        assert_eq!(after.retry_action.as_deref(), Some("check"));
        assert!(after.error.unwrap().contains("unavailable"));

        let fresh = tempfile::tempdir().unwrap();
        let harness = self::harness(
            &fresh,
            false,
            Err(CheckFailure {
                kind: CheckErrorKind::Other,
                details: "dns".into(),
            }),
        );
        let failed = harness.controller.check(false).unwrap();
        assert_eq!(failed.phase, "error");
        assert_eq!(failed.error_details.as_deref(), Some("dns"));
    }

    #[test]
    fn automatic_checks_persist_and_run_only_when_due() {
        let directory = tempfile::tempdir().unwrap();
        let harness = harness(&directory, false, Ok(None));
        assert!(harness.controller.snapshot().automatic_checks);
        let off = harness.controller.set_automatic_checks(false).unwrap();
        assert!(!off.automatic_checks);
        harness.controller.check(true).unwrap();
        assert_eq!(
            *harness.checks.lock().unwrap(),
            0,
            "disabled checks never run"
        );
        harness.controller.set_automatic_checks(true).unwrap();
        let reopened = Controller::new(
            directory.path().join("update-preferences.json"),
            "1.0.0".into(),
            false,
            Box::new(NoFeed),
        );
        assert!(reopened.snapshot().automatic_checks);
        harness.controller.focused();
    }

    #[test]
    fn readiness_names_the_computers_an_update_would_save() {
        let directory = tempfile::tempdir().unwrap();
        let harness = harness(&directory, false, Ok(None));
        let ready = harness.controller.apply_readiness(Ok(vec!["dev".into()]));
        assert!(ready.can_install);
        assert_eq!(ready.running_computers, ["dev"]);
        let blocked = harness
            .controller
            .apply_readiness(Err("Wait for computer operations".into()));
        assert!(!blocked.can_install);
        assert_eq!(
            blocked.install_block_reason.as_deref(),
            Some("Wait for computer operations")
        );
    }
}
