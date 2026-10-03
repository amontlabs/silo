//! Parent-lifetime ownership for SSH forwards and their process groups.
use std::{
    os::unix::process::CommandExt,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

/// Runs the forward in its own process group and ends the whole group (ssh
/// and its ProxyCommand) when Silo's end of stdin closes: on Drop, and also
/// when Silo crashes or is force-quit, since the kernel closes it then.
/// `kill 0` is safe only because the group is the tunnel's own.
const WATCHDOG: &str = r#"stop_group() {
  trap '' TERM
  exec 3<&-
  kill -s TERM 0
  /bin/sleep 0.5
  kill -s KILL 0
}
exec 3<&0 </dev/null
"$@" 3<&- &
child=$!
{ read -r _ <&3; stop_group; } &
exec 3<&-
wait "$child"
stop_group
"#;

/// The same ownership for a command that is waited for: its output is captured and the
/// leader exits with the command's own status. Stragglers stay in the group until the
/// owner drops it or calls `kill_group`.
const TASK_WATCHDOG: &str = r#"stop_group() {
  trap '' TERM
  exec 3<&-
  kill -s TERM 0
  /bin/sleep 0.5
  kill -s KILL 0
}
exec 3<&0 </dev/null
"$@" 3<&- &
child=$!
{ read -r _ <&3; stop_group; } >/dev/null 2>&1 &
exec 3<&-
wait "$child"
exit "$?"
"#;

/// The ssh forward and the private directory holding its Unix socket.
pub(crate) struct Tunnel {
    /// The watchdog shell, leader of the tunnel's process group.
    child: Child,
    /// Silo's end of the watchdog pipe; closing it ends the tunnel.
    stdin: Option<ChildStdin>,
    /// Set once the leader is reaped: its group id may then be reused.
    exited: bool,
    /// Removed after the child is reaped (fields drop after `drop`).
    _directory: Option<tempfile::TempDir>,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
}
impl Tunnel {
    /// Runs `command` like `spawn`, capturing its output and, with `max_file`, bounding the
    /// size of any file it writes (the limit is inherited by everything in the group).
    pub(crate) fn spawn_task(command: &Command, max_file: Option<u64>) -> std::io::Result<Self> {
        let mut shell = Command::new("/bin/sh");
        crate::applications::launch::sanitize_child(&mut shell)
            .arg("-c")
            .arg(TASK_WATCHDOG)
            .arg("silo-task")
            .arg(command.get_program())
            .args(command.get_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        if let Some(bytes) = max_file {
            // SAFETY: setrlimit is async-signal-safe and allocates nothing.
            unsafe {
                shell.pre_exec(move || {
                    let limit = libc::rlimit {
                        rlim_cur: bytes as libc::rlim_t,
                        rlim_max: bytes as libc::rlim_t,
                    };
                    if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) == 0 {
                        Ok(())
                    } else {
                        Err(std::io::Error::last_os_error())
                    }
                });
            }
        }
        let mut child = shell.spawn()?;
        let stdin = child.stdin.take();
        let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
        Ok(Self {
            child,
            stdin,
            exited: false,
            _directory: None,
            stdout,
            stderr,
        })
    }

    pub(crate) fn take_output(&mut self) -> (Option<ChildStdout>, Option<ChildStderr>) {
        (self.stdout.take(), self.stderr.take())
    }

    /// The command's exit status once it has finished.
    pub(crate) fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.exited = true;
        }
        Ok(status)
    }

    /// Kills what a finished command left in its group. The watchdog that still holds the
    /// group keeps its id from being reused until this value drops.
    pub(crate) fn kill_group(&self) {
        unsafe { libc::killpg(self.child.id() as i32, libc::SIGKILL) };
    }

    pub(crate) fn spawn(
        command: &Command,
        directory: Option<tempfile::TempDir>,
    ) -> std::io::Result<Self> {
        let mut child = crate::applications::launch::sanitize_child(&mut Command::new("/bin/sh"))
            .arg("-c")
            .arg(WATCHDOG)
            .arg("silo-tunnel")
            .arg(command.get_program())
            .args(command.get_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        let stdin = child.stdin.take();
        Ok(Self {
            child,
            stdin,
            exited: false,
            _directory: directory,
            stdout: None,
            stderr: None,
        })
    }
    #[cfg(test)]
    pub(crate) fn close_lifetime_pipe(&mut self) {
        drop(self.stdin.take());
    }

    #[cfg(test)]
    pub(crate) fn group_id(&self) -> i32 {
        self.child.id() as i32
    }

    pub(crate) fn running(&mut self) -> bool {
        if !self.exited && !matches!(self.child.try_wait(), Ok(None)) {
            self.exited = true;
        }
        !self.exited
    }
}
impl Drop for Tunnel {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if !self.running() {
            return;
        }
        // Closing stdin asks the watchdog to send TERM, then KILL after 500 ms.
        // Let its cleanup process survive the leader so stubborn children also
        // stop after a crash. The unreaped leader reserves the id for our fallback.
        let group = self.child.id() as i32;
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if !self.running() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        unsafe { libc::killpg(group, libc::SIGKILL) };
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
