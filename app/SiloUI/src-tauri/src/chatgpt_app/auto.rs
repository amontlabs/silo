//! The background worker that keeps the pinned ChatGPT app published.
//!
//! Every device running Silo prepares its own copy without asking: at app start, and
//! again after a retryable failure, with a growing delay. The pieces here are pure so
//! the schedule, the single-worker rule and the ready hook can be tested without an app.

use super::Status;
use std::{
    sync::{Condvar, Mutex},
    time::Duration,
};

/// How long after the app starts before the first download begins.
pub const START_DELAY: Duration = Duration::from_secs(10);

/// The wait before retry number `failures` (1 for the first retry): 30 s, 1 min, 2 min,
/// 5 min, 10 min, 30 min, then hourly. A download attempt already retries a broken
/// connection a few times with its own short backoff.
pub(super) fn delay_for(failures: u32) -> Duration {
    const MINUTES: [u64; 6] = [1, 2, 5, 10, 30, 60];
    match failures {
        0 | 1 => Duration::from_secs(30),
        n => Duration::from_secs(MINUTES[(n as usize - 2).min(MINUTES.len() - 1)] * 60),
    }
}

/// Runs `attempt` until the app is ready or a failure that retrying cannot fix. Between
/// attempts it calls `wait(delay)`, which returns true when woken early (the user chose
/// Retry), restarting the schedule. `on_ready` runs once when the app becomes ready.
pub(super) fn settle(
    mut attempt: impl FnMut() -> Status,
    mut wait: impl FnMut(Duration) -> bool,
    on_ready: impl FnOnce(),
) -> Status {
    let mut failures = 0u32;
    loop {
        match attempt() {
            status @ Status::Ready { .. } => {
                on_ready();
                return status;
            }
            // A hash mismatch or a pinned version OpenAI no longer serves: retrying
            // unprompted changes nothing. A manual Retry requests another attempt.
            status @ Status::Failed {
                retryable: false, ..
            } => return status,
            // Retryable failure (offline, firewall, disk space), or a call that found
            // another run still going: wait, then try again.
            _ => {
                failures += 1;
                if wait(delay_for(failures)) {
                    failures = 0;
                }
            }
        }
    }
}

/// At most one worker per process.
pub(super) struct Slot(Mutex<bool>);

pub(super) struct Claim<'a>(Option<&'a Slot>);

impl Claim<'_> {
    /// A Retry racing with terminal failure either keeps this worker running or
    /// claims its successor. Claiming and releasing share the slot lock.
    pub(super) fn run(mut self, wake: &Wake, mut work: impl FnMut() -> Status) -> Status {
        loop {
            let status = work();
            let slot = self.0.expect("worker owns its slot");
            let mut claimed = slot.0.lock().unwrap_or_else(|p| p.into_inner());
            let mut woken = wake.woken.lock().unwrap_or_else(|p| p.into_inner());
            let retry = std::mem::take(&mut *woken);
            if !retry || matches!(status, Status::Ready { .. }) {
                *claimed = false;
                self.0 = None;
                return status;
            }
        }
    }
}

impl Slot {
    pub(super) const fn new() -> Self {
        Self(Mutex::new(false))
    }
    pub(super) fn claim(&self, wake: &Wake) -> Option<Claim<'_>> {
        let mut claimed = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if *claimed {
            wake.wake();
            None
        } else {
            *claimed = true;
            Some(Claim(Some(self)))
        }
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        if let Some(slot) = self.0 {
            *slot.0.lock().unwrap_or_else(|p| p.into_inner()) = false;
        }
    }
}

pub(super) static WORKER: Slot = Slot::new();
pub(super) static RETRY: Wake = Wake::new();

/// A sleep that "Retry" can cut short.
pub(super) struct Wake {
    woken: Mutex<bool>,
    signal: Condvar,
}

impl Wake {
    pub(super) const fn new() -> Self {
        Self {
            woken: Mutex::new(false),
            signal: Condvar::new(),
        }
    }
    pub(super) fn wake(&self) {
        *self.woken.lock().unwrap_or_else(|p| p.into_inner()) = true;
        self.signal.notify_all();
    }
    /// Sleeps up to `delay`; returns whether it was woken.
    pub(super) fn wait(&self, delay: Duration) -> bool {
        let guard = self.woken.lock().unwrap_or_else(|p| p.into_inner());
        let (mut guard, _) = self
            .signal
            .wait_timeout_while(guard, delay, |woken| !*woken)
            .unwrap_or_else(|p| p.into_inner());
        std::mem::take(&mut *guard)
    }
}

/// Lowers the calling thread's priority so a 450 MB download and the unpacking never
/// compete with the app, computers or the user's work. Child tools inherit it.
pub(super) fn lower_priority() {
    #[cfg(target_os = "macos")]
    // SAFETY: sets the quality of service of the calling thread only.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
    #[cfg(target_os = "linux")]
    // SAFETY: on Linux a nice value set with a thread id applies to that thread only.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, tid, 10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn ready() -> Status {
        Status::Ready {
            path: "/chatgpt/1.2.3-arm64".into(),
            version: "1.2.3".into(),
        }
    }
    fn offline() -> Status {
        Status::Failed {
            reason: "offline".into(),
            retryable: true,
        }
    }

    #[test]
    fn the_schedule_grows_and_stops_at_an_hour() {
        let seconds: Vec<u64> = (1..=9).map(|n| delay_for(n).as_secs()).collect();
        assert_eq!(seconds, [30, 60, 120, 300, 600, 1800, 3600, 3600, 3600]);
        assert_eq!(delay_for(0), delay_for(1));
    }

    #[test]
    fn retryable_failures_are_retried_with_backoff_until_ready() {
        let calls = Cell::new(0);
        let waits = RefCell::new(Vec::new());
        let readied = Cell::new(0);
        let status = settle(
            || {
                calls.set(calls.get() + 1);
                if calls.get() < 4 {
                    offline()
                } else {
                    ready()
                }
            },
            |delay| {
                waits.borrow_mut().push(delay.as_secs());
                false
            },
            || readied.set(readied.get() + 1),
        );
        assert_eq!(status, ready());
        assert_eq!(calls.get(), 4);
        assert_eq!(*waits.borrow(), [30, 60, 120]);
        // The ready hook (the sync of running computers) runs exactly once, at the end.
        assert_eq!(readied.get(), 1);
    }

    #[test]
    fn a_failure_retrying_cannot_fix_stops_the_worker_without_the_ready_hook() {
        let calls = Cell::new(0);
        let fatal = Status::Failed {
            reason: "checksum".into(),
            retryable: false,
        };
        let status = settle(
            || {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    offline()
                } else {
                    fatal.clone()
                }
            },
            |_| false,
            || panic!("not ready"),
        );
        assert_eq!(status, fatal);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn an_early_wake_restarts_the_schedule() {
        let calls = Cell::new(0);
        let waits = RefCell::new(Vec::new());
        settle(
            || {
                calls.set(calls.get() + 1);
                if calls.get() < 5 {
                    offline()
                } else {
                    ready()
                }
            },
            |delay| {
                waits.borrow_mut().push(delay.as_secs());
                // The user chooses Retry during the third wait.
                waits.borrow().len() == 3
            },
            || {},
        );
        assert_eq!(*waits.borrow(), [30, 60, 120, 30]);
    }

    #[test]
    fn a_run_found_in_progress_is_waited_out_not_duplicated() {
        // A call that finds another download running reports progress: not ready, not
        // failed. The worker waits and tries again.
        let calls = Cell::new(0);
        let status = settle(
            || {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    Status::Verifying
                } else {
                    ready()
                }
            },
            |_| false,
            || {},
        );
        assert_eq!(status, ready());
    }

    #[test]
    fn only_one_worker_runs_at_a_time() {
        let slot = Slot::new();
        let wake = Wake::new();
        let first = slot.claim(&wake);
        assert!(first.is_some());
        assert!(slot.claim(&wake).is_none());
        drop(first);
        assert!(slot.claim(&wake).is_some());
    }

    #[test]
    fn retry_during_terminal_failure_starts_another_attempt() {
        let slot = Slot::new();
        let wake = Wake::new();
        let claim = slot.claim(&wake).unwrap();
        std::thread::scope(|threads| {
            let (failed, failure_seen) = std::sync::mpsc::channel();
            let (proceed, proceed_seen) = std::sync::mpsc::channel();
            let wake_ref = &wake;
            let worker = threads.spawn(move || {
                let mut attempts = 0;
                let status = claim.run(wake_ref, || {
                    attempts += 1;
                    if attempts == 1 {
                        failed.send(()).unwrap();
                        proceed_seen.recv().unwrap();
                        Status::Failed {
                            reason: "checksum".into(),
                            retryable: false,
                        }
                    } else {
                        ready()
                    }
                });
                (status, attempts)
            });
            // Retry after failure publication, before the worker releases its slot.
            failure_seen.recv().unwrap();
            assert!(slot.claim(&wake).is_none());
            proceed.send(()).unwrap();
            let (status, attempts) = worker.join().unwrap();
            assert_eq!(status, ready());
            assert_eq!(attempts, 2);
        });
        assert!(slot.claim(&wake).is_some());
    }

    #[test]
    fn a_terminal_worker_releases_its_slot_when_no_retry_was_requested() {
        let slot = Slot::new();
        let wake = Wake::new();
        let attempts = Cell::new(0);
        let status = slot.claim(&wake).unwrap().run(&wake, || {
            attempts.set(attempts.get() + 1);
            Status::Failed {
                reason: "checksum".into(),
                retryable: false,
            }
        });
        assert!(matches!(
            status,
            Status::Failed {
                retryable: false,
                ..
            }
        ));
        assert_eq!(attempts.get(), 1);
        let successor = slot.claim(&wake).unwrap();
        assert_eq!(successor.run(&wake, ready), ready());
        assert!(slot.claim(&wake).is_some());
    }

    #[test]
    fn a_retry_consumed_by_backoff_does_not_repeat_the_ready_hook() {
        let slot = Slot::new();
        let wake = Wake::new();
        let attempts = Cell::new(0);
        let readied = Cell::new(0);
        let status = slot.claim(&wake).unwrap().run(&wake, || {
            settle(
                || {
                    attempts.set(attempts.get() + 1);
                    if attempts.get() == 1 {
                        assert!(slot.claim(&wake).is_none());
                        offline()
                    } else {
                        ready()
                    }
                },
                |delay| wake.wait(delay),
                || readied.set(readied.get() + 1),
            )
        });
        assert_eq!(status, ready());
        assert_eq!(attempts.get(), 2);
        assert_eq!(readied.get(), 1);
        assert!(slot.claim(&wake).is_some());
    }

    #[test]
    fn a_wake_cuts_a_wait_short_and_is_consumed() {
        let wake = Wake::new();
        assert!(!wake.wait(Duration::from_millis(10)));
        wake.wake();
        assert!(wake.wait(Duration::from_secs(30)));
        assert!(!wake.wait(Duration::from_millis(10)));
    }

    #[test]
    fn no_consent_paths_remain() {
        let source = include_str!("../chatgpt_app.rs");
        let code = source.split("#[cfg(test)]").next().unwrap();
        for forbidden in [
            "consent",
            "accept_notice",
            "NotConsented",
            "chatgpt.accept",
            "chatgpt.prepare",
            "chatgpt_app_prepare",
        ] {
            assert!(!code.contains(forbidden), "{forbidden} is still present");
        }
        let build = include_str!("../../build.rs");
        let capabilities = include_str!("../../capabilities/preview.json");
        for removed in ["chatgpt_app_accept_notice", "chatgpt_app_prepare"] {
            assert!(!build.contains(removed));
        }
        assert!(!capabilities.contains("chatgpt-app-accept-notice"));
        assert!(!capabilities.contains("chatgpt-app-prepare"));
        assert!(build.contains("chatgpt_app_retry"));
        assert!(capabilities.contains("allow-chatgpt-app-retry"));
    }
}
