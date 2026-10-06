use std::{
    io,
    process::{Child, ExitStatus},
    thread,
    time::{Duration, Instant},
};

const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Ask a child to stop with SIGTERM so it can clean up, and SIGKILL it only if
/// it is still running after `grace`. Returns the result of reaping the child.
pub(crate) fn terminate_child(child: &mut Child, grace: Duration) -> io::Result<ExitStatus> {
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        // SAFETY: the child has not been reaped (no successful wait yet), so
        // its PID still names this process's own child.
        unsafe { libc::kill(pid, libc::SIGTERM) };
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) => thread::sleep(POLL_INTERVAL),
                Err(_) => break,
            }
        }
    }
    let _ = child.kill();
    child.wait()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn a_child_that_handles_sigterm_exits_without_being_killed() {
        let mut child = Command::new("sh").args(["-c", "sleep 30"]).spawn().unwrap();
        let started = Instant::now();
        let status = terminate_child(&mut child, Duration::from_secs(5)).unwrap();
        assert!(started.elapsed() < Duration::from_secs(4));
        assert!(!status.success());
    }

    #[test]
    fn a_child_that_ignores_sigterm_is_killed_after_the_grace() {
        let mut child = Command::new("sh")
            .args(["-c", "trap '' TERM; while :; do sleep 1; done"])
            .spawn()
            .unwrap();
        thread::sleep(Duration::from_millis(200));
        let started = Instant::now();
        terminate_child(&mut child, Duration::from_millis(300)).unwrap();
        assert!(started.elapsed() >= Duration::from_millis(300));
        assert!(child.try_wait().unwrap().is_some());
    }
}
