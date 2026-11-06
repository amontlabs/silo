//! How the engine process ends: the exit code the run loop returns, and the restart that the
//! process supervisor (launchd, or systemd on Linux) turns into a new engine.

use std::sync::atomic::{AtomicI32, Ordering};

use silo_engine::context::ProcessControl;

/// The exit code that asks the supervisor to start the engine again (`EX_TEMPFAIL`).
pub const RESTART_EXIT_CODE: i32 = 75;

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
