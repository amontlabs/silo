//! The programs a container computer needs: Silo's own helpers beside the engine and the
//! system's crun, skopeo, umoci and systemd-run. Missing system programs are reported through
//! the host prerequisite check (`silo host init` names the same ones).

use crate::runtime::{RuntimeError, RuntimePaths};
use std::path::{Path, PathBuf};

pub const CTR: &str = "silo-ctr";
pub const INIT: &str = "silo-init";

#[derive(Clone, Debug)]
pub struct Tools {
    pub ctr: PathBuf,
    pub init: PathBuf,
    pub gateway: PathBuf,
    pub crun: PathBuf,
    pub skopeo: PathBuf,
    pub umoci: PathBuf,
    pub systemd_run: PathBuf,
    pub systemctl: PathBuf,
}

pub fn find_in_path(name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|directory| directory.join(name))
        .find(|candidate| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(candidate)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

impl Tools {
    /// Locates every program, naming the first one that is missing.
    pub fn locate(paths: &RuntimePaths) -> Result<Self, RuntimeError> {
        let beside = |name: &str| -> Result<PathBuf, RuntimeError> {
            let path = paths.executable.with_file_name(name);
            if path.is_file() {
                Ok(path)
            } else {
                Err(RuntimeError::Unavailable(format!(
                    "{name} is missing from this Silo engine. Reinstall Silo."
                )))
            }
        };
        let system_path = std::env::var_os("PATH").unwrap_or_default();
        let system = |name: &str| -> Result<PathBuf, RuntimeError> {
            find_in_path(name, &system_path).ok_or_else(|| {
                RuntimeError::Unavailable(format!(
                    "{name} is not installed on this host. Run silo host init to see what container computers need."
                ))
            })
        };
        Ok(Self {
            ctr: beside(CTR)?,
            init: beside(INIT)?,
            gateway: beside(crate::computer_gateway::GATEWAY_BINARY)?,
            crun: system("crun")?,
            skopeo: system("skopeo")?,
            umoci: system("umoci")?,
            systemd_run: system("systemd-run")?,
            systemctl: system("systemctl")?,
        })
    }
}

/// Runs a short program to completion and returns its standard output, or its error text.
pub fn run_checked(program: &Path, args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let output = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("{} could not start: {error}", program.display()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let text = String::from_utf8_lossy(&output.stderr);
        let last = text
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        Err(format!(
            "{} failed ({}): {last}",
            program
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("program"),
            output.status
        ))
    }
}

/// What a program run with [`run_bounded`] did.
#[derive(Debug)]
pub struct Finished {
    pub success: bool,
    pub stdout: String,
    /// The last non-empty line of its error output.
    pub last_error: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BoundedError {
    /// The program was still running when the limit passed; it and its children were killed.
    TimedOut,
    CouldNotStart(String),
}

/// Runs a program for at most `limit`. A program that is still running then is killed together
/// with the processes it started (it runs in a process group of its own), so a copy or a
/// measurement inside a user namespace does not outlive the wait.
pub fn run_bounded(
    program: &Path,
    args: &[&std::ffi::OsStr],
    limit: std::time::Duration,
) -> Result<Finished, BoundedError> {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|error| {
            BoundedError::CouldNotStart(format!("{} could not start: {error}", program.display()))
        })?;
    let read = |stream: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut stream) = stream {
                let mut bytes = Vec::new();
                let _ = stream.read_to_end(&mut bytes);
                text = String::from_utf8_lossy(&bytes).into_owned();
            }
            text
        })
    };
    let stdout = read(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let stderr = read(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let deadline = std::time::Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Ok(None) | Err(_) => break None,
        }
    };
    let Some(status) = status else {
        // SAFETY: a negative pid addresses the process group the child leads; SIGKILL has no
        // arguments to get wrong.
        unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
        let _ = child.kill();
        let _ = child.wait();
        // The readers end by themselves when the pipes close; a process that left the group would
        // keep them open, so they are not waited for.
        return Err(BoundedError::TimedOut);
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    Ok(Finished {
        success: status.success(),
        stdout,
        last_error: stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_are_found_on_the_given_path_only_when_executable() {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("crun");
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        let path = std::env::join_paths([directory.path()]).unwrap();
        assert_eq!(find_in_path("crun", &path), None);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(find_in_path("crun", &path), Some(program));
        assert_eq!(find_in_path("skopeo", &path), None);
    }

    #[test]
    fn a_failing_tool_reports_its_last_error_line() {
        let error = run_checked(
            Path::new("/bin/sh"),
            &["-c".as_ref(), "echo a >&2; echo b >&2; exit 3".as_ref()],
        )
        .unwrap_err();
        assert!(
            error.contains("sh failed") && error.ends_with(": b"),
            "{error}"
        );
    }

    #[test]
    fn a_bounded_run_returns_what_the_program_printed() {
        let done = run_bounded(
            Path::new("/bin/sh"),
            &["-c".as_ref(), "echo out; echo oops >&2; exit 2".as_ref()],
            std::time::Duration::from_secs(10),
        )
        .unwrap();
        assert!(!done.success);
        assert_eq!(done.stdout, "out\n");
        assert_eq!(done.last_error, "oops");
    }

    #[test]
    fn a_bounded_run_kills_the_program_and_what_it_started_at_the_limit() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("late");
        let script = format!("(sleep 2; touch {}) & wait", marker.display());
        let started = std::time::Instant::now();
        let outcome = run_bounded(
            Path::new("/bin/sh"),
            &["-c".as_ref(), script.as_ref()],
            std::time::Duration::from_millis(200),
        );
        assert_eq!(outcome.unwrap_err(), BoundedError::TimedOut);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        std::thread::sleep(std::time::Duration::from_millis(2300));
        assert!(
            !marker.exists(),
            "the child of the killed program kept running"
        );
    }
}
