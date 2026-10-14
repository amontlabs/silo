//! Fixed system-owned helper; no command, source URL, or package name comes from the UI.
//!
//! The helper authenticates, checks Silo's source, refreshes and downloads first,
//! then prints `ready` and waits for `install` on stdin. Silo stops its computers
//! only at that point, so a cancelled prompt, a disabled source, a busy package
//! lock or a missing candidate never stops or restarts anything.
use std::{
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant},
};
const HELPER: &str = "/usr/lib/silo/silo-system-update";
/// Authentication, the refresh and the download can each wait on the user or the
/// network while computers keep running and new operations are refused.
pub(super) const PREPARE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const TIMED_OUT: &str = "Silo stopped waiting for the system update to authenticate and download. No computers were stopped.";

pub(super) fn preflight() -> Result<(), String> {
    if !cfg!(target_os = "linux")
        || !Path::new(HELPER).is_file()
        || !Path::new("/usr/bin/pkexec").is_file()
    {
        return Err(
            "The system updater is missing. Reinstall Silo’s Debian package to restore it.".into(),
        );
    }
    Ok(())
}
fn command(version: &str) -> Result<Command, String> {
    if version.len() > 64
        || version.split('.').count() != 3
        || version.split('.').any(|part| {
            part.is_empty()
                || !part.bytes().all(|c| c.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
        })
    {
        return Err("The update version is invalid. Check for updates again.".into());
    }
    let mut command = Command::new("/usr/bin/pkexec");
    command.args([
        "--disable-internal-agent",
        HELPER,
        &std::process::id().to_string(),
        version,
    ]);
    Ok(command)
}
/// `prepare` runs once the package is downloaded and must stop the computers.
/// If it fails, the helper is told to cancel and installs nothing.
pub(super) fn install(
    version: &str,
    status: impl FnMut(&str),
    prepare: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    execute(&mut command(version)?, status, PREPARE_TIMEOUT, prepare)
}
fn execute(
    command: &mut Command,
    mut status: impl FnMut(&str),
    timeout: Duration,
    prepare: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let mut errors = tempfile::tempfile().map_err(|_| "The update log could not be opened.")?;
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(
        errors
            .try_clone()
            .map_err(|_| "The update log could not be opened.")?,
    );
    status("Waiting for administrator authentication…");
    let mut child = command.spawn().map_err(|_| "System authentication could not start. Check that a desktop authentication agent is running, then retry.")?;
    let mut input = child.stdin.take();
    let output = child
        .stdout
        .take()
        .ok_or("System update progress is unavailable.")?;
    // Read progress on a thread so waiting for authentication or the download can time out.
    let (sender, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { break };
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + timeout;
    let mut prepare = Some(prepare);
    loop {
        // After the go-ahead the helper owns the package manager; APT bounds its lock wait.
        let line = if prepare.is_some() {
            match lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(line) => Some(line),
                Err(RecvTimeoutError::Disconnected) => None,
                Err(RecvTimeoutError::Timeout) => {
                    // Closing stdin makes the helper refuse to install. Killing works
                    // until authentication completes; afterwards the helper runs as root.
                    drop(input.take());
                    let _ = child.kill();
                    std::thread::spawn(move || child.wait());
                    return Err(TIMED_OUT.into());
                }
            }
        } else {
            lines.recv().ok()
        };
        let Some(line) = line else { break };
        // The helper emits only these fixed stages. APT diagnostics remain in Details.
        match line.as_str() {
            "refreshing" => status("Refreshing Silo’s package list…"),
            "downloading" => status("Downloading the Silo package…"),
            "ready" => {
                let Some(prepare) = prepare.take() else {
                    continue;
                };
                status("Preparing to install. Running computers stop now…");
                if let Err(error) = prepare() {
                    if let Some(mut input) = input.take() {
                        let _ = input.write_all(b"cancel\n");
                    }
                    let _ = child.wait();
                    return Err(error);
                }
                let sent = match input.take() {
                    Some(mut input) => input.write_all(b"install\n").and_then(|()| input.flush()),
                    None => Err(std::io::ErrorKind::BrokenPipe.into()),
                };
                if sent.is_err() {
                    let _ = child.wait();
                    return Err("The system updater stopped before installing. Nothing was installed. Try again.".into());
                }
            }
            "installing" => status("Installing Silo. It will restart when finished…"),
            _ => (),
        }
    }
    drop(input);
    let result = if prepare.is_some() {
        // A closed progress pipe does not mean authentication or downloading ended.
        loop {
            if let Some(result) = child.try_wait().map_err(|_| "The system updater could not be monitored. Check the system package manager before retrying.")? {
                break result;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let _ = child.kill();
                std::thread::spawn(move || child.wait());
                return Err(TIMED_OUT.into());
            }
            std::thread::sleep(remaining.min(Duration::from_millis(50)));
        }
    } else {
        child.wait().map_err(|_| "The system updater could not be monitored. Check the system package manager before retrying.")?
    };
    if result.success() {
        return if prepare.is_none() {
            Ok(())
        } else {
            Err("The system updater finished without installing. Check for updates again.".into())
        };
    }
    if result.code() == Some(126) {
        return Err("Authentication was cancelled. Click Retry when you are ready.".into());
    }
    errors
        .seek(SeekFrom::End(0))
        .map_err(|_| "The update log could not be read.")?;
    let end = errors.stream_position().unwrap_or(0);
    errors
        .seek(SeekFrom::Start(end.saturating_sub(16384)))
        .map_err(|_| "The update log could not be read.")?;
    let mut bytes = Vec::new();
    errors
        .take(16384)
        .read_to_end(&mut bytes)
        .map_err(|_| "The update log could not be read.")?;
    let mut details = String::from_utf8_lossy(&bytes).into_owned();
    if details.trim().is_empty() {
        details = "System authentication or package installation failed. Check that a desktop authentication agent is running, then retry.".into();
    }
    Err(details)
}
pub(super) fn failure_message(details: &str) -> &'static str {
    if details.contains("Authentication was cancelled") {
        "Update cancelled. Click Retry to authenticate."
    } else if details.starts_with(TIMED_OUT) {
        "The system update took too long. No computers were stopped. Try again."
    } else if details.contains("Software & Updates") {
        "Enable Silo’s software source in Software & Updates, then retry."
    } else if details.contains("Could not get lock") || details.contains("Unable to acquire") {
        "Another package manager is busy. Wait for it to finish, then retry."
    } else if details.contains("Failed to fetch") || details.contains("Could not resolve") {
        "The update could not be fetched. Check your connection, then retry."
    } else {
        "Silo could not finish the system update. Check Details, then retry."
    }
}
/// Returns only if this process could not be replaced.
pub(super) fn restart() -> String {
    // Replace this process, rather than resolving /proc/self/exe after dpkg has
    // unlinked the old binary. Also ignore unrelated inherited AppImage paths.
    use std::os::unix::process::CommandExt;
    let error = Command::new("/usr/bin/silo-ui")
        .env_remove("APPIMAGE")
        .env_remove("APPDIR")
        .exec();
    format!("Silo could not start its updated version: {error}")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_fixed_helper_and_numeric_version_can_be_executed() {
        for version in ["-y", "1.2.3;id", "1.2", "01.2.3", "1.2.3-rc1"] {
            assert!(command(version).is_err());
        }
        let cmd = command("0.5.1").unwrap();
        assert_eq!(cmd.get_program(), "/usr/bin/pkexec");
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args[0], "--disable-internal-agent");
        assert_eq!(args[1], HELPER);
        assert_eq!(args[3], "0.5.1");
    }
    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }
    const PATIENT: Duration = Duration::from_secs(30);
    const INSTALLS_ON_GO_AHEAD: &str = "printf 'refreshing\\ndownloading\\nready\\n'; read reply; [ \"$reply\" = install ] || exit 3; printf 'installing\\n'";
    #[test]
    fn real_subprocess_progress_and_cancelled_authentication_are_reported() {
        let mut stages = vec![];
        execute(
            &mut shell(INSTALLS_ON_GO_AHEAD),
            |stage| stages.push(stage.to_string()),
            PATIENT,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(stages.len(), 5);
        assert!(stages[1].contains("Refreshing"));
        assert!(execute(&mut shell("exit 126"), |_| {}, PATIENT, || Ok(()))
            .unwrap_err()
            .contains("cancelled"));
        assert!(execute(
            &mut shell("printf 'Package lock is busy' >&2; exit 1"),
            |_| {},
            PATIENT,
            || Ok(())
        )
        .unwrap_err()
        .contains("Package lock is busy"));
    }
    #[test]
    fn truncated_utf8_log_tail_preserves_package_manager_details() {
        let sentinel = "E: sentinel package repair failure";
        let tail = format!("{}{}", "x".repeat(16383 - sentinel.len()), sentinel);
        let error = execute(
            &mut shell(&format!("printf '\\303\\251%s' '{tail}' >&2; exit 1")),
            |_| {},
            PATIENT,
            || panic!("a failed helper must not stop computers"),
        )
        .unwrap_err();
        assert!(error.contains(sentinel), "lost diagnostic: {error}");
    }
    #[test]
    fn computers_stop_only_after_authentication_refresh_and_download_succeed() {
        let events = std::cell::RefCell::new(vec![]);
        execute(
            &mut shell(INSTALLS_ON_GO_AHEAD),
            |stage| events.borrow_mut().push(stage.to_string()),
            PATIENT,
            || {
                events.borrow_mut().push("stop computers".into());
                Ok(())
            },
        )
        .unwrap();
        let events = events.into_inner();
        let stopped = events
            .iter()
            .position(|event| event == "stop computers")
            .unwrap();
        assert!(events[..stopped]
            .iter()
            .any(|event| event.contains("Downloading")));
        assert!(events[stopped..]
            .iter()
            .any(|event| event.contains("Installing")));
    }
    #[test]
    fn failures_before_the_install_stage_never_stop_computers() {
        for script in [
            "exit 126",
            "printf 'Enable Silo software source in Software & Updates, then retry.' >&2; exit 1",
            "printf 'refreshing\\n'; printf 'E: Could not get lock /var/lib/apt/lists/lock' >&2; exit 100",
            "printf 'refreshing\\ndownloading\\n'; printf 'E: Version 9.9.9 for silo was not found' >&2; exit 100",
            // A helper that never asks for the go-ahead did not install anything.
            "printf 'refreshing\\ndownloading\\n'",
        ] {
            let error = execute(&mut shell(script), |_| {}, PATIENT, || {
                panic!("computers must keep running: {script}")
            })
            .unwrap_err();
            assert!(!error.is_empty(), "{script}");
        }
    }
    #[test]
    fn a_failed_stop_cancels_the_helper_before_it_installs() {
        let directory = tempfile::tempdir().unwrap();
        let installed = directory.path().join("installed");
        let script = format!(
            "printf 'ready\\n'; read reply; [ \"$reply\" = install ] && touch '{}'; exit 1",
            installed.display()
        );
        let error = execute(
            &mut shell(&script),
            |_| {},
            PATIENT,
            || Err("stop failed".into()),
        )
        .unwrap_err();
        assert_eq!(error, "stop failed");
        assert!(!installed.exists());
    }
    #[test]
    fn a_closed_progress_pipe_does_not_bypass_prepare_timeout() {
        let started = Instant::now();
        let error = execute(
            &mut shell("exec 1>&-; exec sleep 2"),
            |_| {},
            Duration::from_millis(100),
            || panic!("computers must keep running"),
        )
        .unwrap_err();
        assert!(error.starts_with(TIMED_OUT), "{error}");
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn a_stalled_authentication_or_download_times_out_without_stopping_computers() {
        let started = std::time::Instant::now();
        let error = execute(
            &mut shell("printf 'refreshing\\n'; exec sleep 30"),
            |_| {},
            Duration::from_millis(300),
            || panic!("computers must keep running"),
        )
        .unwrap_err();
        assert!(error.contains("No computers were stopped"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(
            failure_message(&error),
            "The system update took too long. No computers were stopped. Try again."
        );
    }
}
