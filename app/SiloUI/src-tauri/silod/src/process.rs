//! How the engine process ends: the exit code the run loop returns, and the restart that the
//! process supervisor (launchd, or systemd on Linux) turns into a new engine.

use std::sync::atomic::{AtomicI32, Ordering};

use silo_engine::context::ProcessControl;

/// The exit code that asks the supervisor to start the engine again (`EX_TEMPFAIL`).
pub const RESTART_EXIT_CODE: i32 = 75;

/// The exit code of a session end that left computers running (`EX_SOFTWARE`), on Linux.
///
/// Not 0, so systemd records the stop as failed and the journal says why. Not
/// `RESTART_EXIT_CODE`: systemd never restarts a unit whose stop it was asked for (a logout or
/// `systemctl --user stop`), and after a SIGTERM from anywhere else `Restart=on-failure` starts
/// a new engine, which takes the surviving computers back.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const SESSION_END_INCOMPLETE_EXIT_CODE: i32 = 70;

/// The exit code of a session end that stopped computers, from what the runtime still lists
/// as running afterwards, and the line to log when that is not nothing.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn session_end_outcome(running: Result<Vec<String>, String>) -> (i32, Option<String>) {
    match running {
        Ok(names) if names.is_empty() => (0, None),
        Ok(names) => (
            SESSION_END_INCOMPLETE_EXIT_CODE,
            Some(format!(
                "the session ended with computers still running: {}. They keep running without \
                 the engine until it starts again and takes them back.",
                names.join(", ")
            )),
        ),
        Err(error) => (
            SESSION_END_INCOMPLETE_EXIT_CODE,
            Some(format!(
                "the session ended, but whether computers are still running is unknown: {error}"
            )),
        ),
    }
}

/// The panic payload a restart unwinds its worker with; the socket server recognizes it.
#[derive(Debug)]
pub struct RestartScheduled;

/// Records the exit code and asks the run loop (AppKit's on macOS) to stop.
pub struct AppProcessControl {
    exit_code: AtomicI32,
    request_stop: Box<dyn Fn() + Send + Sync>,
}

impl AppProcessControl {
    pub(crate) fn new(request_stop: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            exit_code: AtomicI32::new(0),
            request_stop: Box::new(request_stop),
        }
    }

    /// The code the process exits with once its run loop has stopped.
    pub fn exit_code(&self) -> i32 {
        self.exit_code.load(Ordering::SeqCst)
    }
}

impl ProcessControl for AppProcessControl {
    fn exit(&self, code: i32) {
        self.exit_code.store(code, Ordering::SeqCst);
        (self.request_stop)();
    }

    fn restart(&self) -> ! {
        self.exit(RESTART_EXIT_CODE);
        // ProcessControl's non-returning contract is used from engine worker
        // tasks. Unwind that task after scheduling the run loop's shutdown; parking the
        // Tokio worker would prevent the server from draining and releasing its
        // socket and ownership lock.
        std::panic::resume_unwind(Box::new(RestartScheduled))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicUsize, Arc};

    #[test]
    fn a_session_end_that_left_computers_running_exits_with_a_failure_that_is_not_a_restart() {
        assert_eq!(session_end_outcome(Ok(Vec::new())), (0, None));
        let (code, message) = session_end_outcome(Ok(vec!["alpha".into(), "beta".into()]));
        assert_eq!(code, SESSION_END_INCOMPLETE_EXIT_CODE);
        assert!(message.unwrap().contains("still running: alpha, beta"));
        let (code, message) = session_end_outcome(Err("runtime unavailable".into()));
        assert_eq!(code, SESSION_END_INCOMPLETE_EXIT_CODE);
        assert!(message.unwrap().contains("runtime unavailable"));
        // systemd's Restart=on-failure treats any non-zero code as a failure; the engine's own
        // restart request keeps its own code.
        assert_ne!(SESSION_END_INCOMPLETE_EXIT_CODE, 0);
        assert_ne!(SESSION_END_INCOMPLETE_EXIT_CODE, RESTART_EXIT_CODE);
    }

    #[tokio::test]
    async fn restart_unwinds_the_worker_after_scheduling_appkit_exit() {
        let requested = Arc::new(AtomicUsize::new(0));
        let requested_in_callback = requested.clone();
        let process = Arc::new(AppProcessControl::new(move || {
            requested_in_callback.fetch_add(1, Ordering::SeqCst);
        }));
        let worker_process = process.clone();

        let worker = tokio::spawn(async move {
            worker_process.restart();
        });
        let error = worker.await.unwrap_err();

        assert!(error.is_panic());
        assert_eq!(process.exit_code(), RESTART_EXIT_CODE);
        assert_eq!(requested.load(Ordering::SeqCst), 1);
    }
}
