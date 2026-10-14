//! Bounded guest execution (`guest.exec`) and the pieces long-running jobs share: request
//! validation, the guest-side supervisor and the process-group control helper.
use super::{
    process::{self, Stopped},
    target::Target,
    GuestError, MAX_RESPONSE_RAW_BYTES,
};
use silo_api::jobs::ErrorCode;
use std::{collections::HashSet, sync::atomic::AtomicBool, time::Duration};

const SUPERVISOR: &str = include_str!("helpers/exec.pl");
const CONTROL: &str = include_str!("helpers/control.pl");
pub(crate) const FILES: &str = include_str!("helpers/files.pl");
/// How long a stopped guest process tree may take to answer SIGTERM before SIGKILL.
pub(crate) const TERMINATE_GRACE_MS: u64 = 3000;
/// How long `kill` waits for a run that has not recorded its process group yet.
const STARTUP_WAIT_MS: u64 = 2000;
/// The signal-name lookup and the process-group kill are short guest commands.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(15);

/// The account a guest command runs as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuestUser {
    /// The working account, with `USER` and `LOGNAME` set.
    Account,
    Root,
}

/// One bounded guest run.
#[derive(Clone, Debug)]
pub struct ExecRequest {
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub user: GuestUser,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
    /// Standard output beyond this many bytes fails the run (`output_too_large`).
    pub max_stdout: usize,
}

/// How a run ended. A process ended by a signal has no exit code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecOutput {
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Checks a request before any effect: a non-empty argv without NUL, an absolute working
/// directory, and environment names that are portable identifiers (letters, digits and
/// underscores, not starting with a digit) and unique.
pub fn validate_request(
    argv: &[String],
    cwd: Option<&str>,
    env: &[(String, String)],
) -> Result<(), GuestError> {
    let invalid = |message: &str| Err(GuestError::invalid(format!("Invalid params: {message}")));
    if argv.is_empty() || argv[0].is_empty() {
        return invalid("argv must name a program.");
    }
    if argv.iter().any(|word| word.contains('\0')) {
        return invalid("argv must not contain NUL.");
    }
    if let Some(cwd) = cwd {
        if !cwd.starts_with('/') || cwd.contains('\0') {
            return invalid("cwd must be an absolute path without NUL.");
        }
    }
    let mut names = HashSet::new();
    for (name, value) in env {
        let identifier = name.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
        });
        if name.is_empty() || !identifier || value.contains('\0') {
            return invalid(
                "environment names must be letters, digits and underscores, not starting with a digit.",
            );
        }
        if !names.insert(name.as_str()) {
            return invalid("environment names must be unique.");
        }
    }
    Ok(())
}

/// The guest command that runs `argv` under the supervisor: its own process group, a
/// recorded group id for `kill`, and the exit status as the command's.
pub(crate) fn supervised(token: &str, cwd: Option<&str>, argv: &[String]) -> Vec<String> {
    let mut command = vec![
        "perl".to_owned(),
        "-e".into(),
        SUPERVISOR.into(),
        "--".into(),
        token.into(),
        cwd.unwrap_or("").into(),
    ];
    command.extend(argv.iter().cloned());
    command
}

/// The guest command that runs a Perl `script` with `arguments`.
pub(crate) fn perl_argv(script: &str, arguments: &[String]) -> Vec<String> {
    let mut command = vec!["perl".to_owned(), "-e".into(), script.into(), "--".into()];
    command.extend(arguments.iter().cloned());
    command
}

/// Runs the control helper in the guest as the same user that started the run.
fn control(
    target: &Target,
    root: bool,
    arguments: &[&str],
    timeout: Duration,
) -> Result<(i32, String), GuestError> {
    let arguments: Vec<String> = arguments.iter().map(|word| (*word).to_owned()).collect();
    let argv = perl_argv(CONTROL, &arguments);
    let plan = target.plan(&argv, &[], root, false)?;
    match process::run_collect(&plan, None, timeout, 4096, 4096, None, &|| {})? {
        Ok(collected) => Ok((
            collected.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&collected.stdout).trim().to_owned(),
        )),
        Err(Stopped::Timeout) => Err(GuestError::new(
            ErrorCode::TimedOut,
            "The computer did not answer in time.",
        )),
        Err(_) => Err(GuestError::internal("The guest control command failed.")),
    }
}

/// Ends the supervised process tree of run `token`: SIGTERM, a grace period, then SIGKILL,
/// and returns once no member of the group remains. `Ok(false)` means some survived.
pub(crate) fn terminate(target: &Target, root: bool, token: &str) -> Result<bool, GuestError> {
    let grace = TERMINATE_GRACE_MS.to_string();
    let wait = STARTUP_WAIT_MS.to_string();
    let (code, _) = control(
        target,
        root,
        &["kill", token, &grace, &wait],
        CONTROL_TIMEOUT + Duration::from_millis(TERMINATE_GRACE_MS + STARTUP_WAIT_MS),
    )?;
    Ok(code == 0)
}

/// The signal that ended run `token`, if the supervisor recorded one. Also clears the
/// run's leftover state.
pub(crate) fn signal_of(target: &Target, root: bool, token: &str) -> Option<String> {
    match control(target, root, &["status", token], CONTROL_TIMEOUT) {
        Ok((0, name)) if !name.is_empty() => Some(name),
        _ => None,
    }
}

/// The exit code the guest supervisor saved for run `token`. It saves only 255, the one code
/// SSH also uses for its own failures, so a recorded exit proves the command really ran.
pub(crate) fn recorded_exit(target: &Target, root: bool, token: &str) -> Option<i32> {
    match control(target, root, &["exit", token], CONTROL_TIMEOUT) {
        Ok((0, text)) => text.parse().ok(),
        _ => None,
    }
}

/// Classifies an exit status from the channel into `(exit code, signal)`.
///
/// The supervisor reports a signal death as 128+N and records the signal name; only then is
/// the guest asked for it. Over SSH, status 255 is also the transport's own failure: it is a
/// guest exit only when the supervisor recorded it, and a channel failure otherwise.
pub(crate) fn settle_exit(
    target: &Target,
    root: bool,
    token: &str,
    code: i32,
    stderr: &str,
) -> Result<(Option<i32>, Option<String>), GuestError> {
    settle_exit_with(
        target,
        code,
        stderr,
        &|| recorded_exit(target, root, token),
        &|| signal_of(target, root, token),
    )
}

/// `settle_exit` with the guest lookups supplied.
pub(crate) fn settle_exit_with(
    target: &Target,
    code: i32,
    stderr: &str,
    recorded: &dyn Fn() -> Option<i32>,
    signal: &dyn Fn() -> Option<String>,
) -> Result<(Option<i32>, Option<String>), GuestError> {
    if target.is_macos() && code == 255 {
        return match recorded() {
            Some(code) => Ok((Some(code), None)),
            None => Err(target
                .channel_error(code, stderr)
                .unwrap_or_else(|| GuestError::internal("The connection to the computer failed."))),
        };
    }
    if (129..=192).contains(&code) {
        if let Some(signal) = signal() {
            return Ok((None, Some(signal)));
        }
    }
    Ok((Some(code), None))
}

/// Runs a request to completion within its deadline and stdout allowance.
///
/// Errors: `not_running` (the computer is never started), `timed_out`,
/// `output_too_large` (standard output only; standard error is spooled, not capped) and
/// `internal` when the complete result cannot fit one response frame.
pub fn run_bounded(target: &Target, request: &ExecRequest) -> Result<ExecOutput, GuestError> {
    run_bounded_cancellable(target, request, None)
}

pub(crate) fn run_bounded_cancellable(
    target: &Target,
    request: &ExecRequest,
    cancel: Option<&AtomicBool>,
) -> Result<ExecOutput, GuestError> {
    validate_request(&request.argv, request.cwd.as_deref(), &request.env)?;
    target.ensure_running()?;
    let root = request.user == GuestUser::Root;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let argv = supervised(&token, request.cwd.as_deref(), &request.argv);
    let plan = target.plan(&argv, &request.env, root, request.stdin.is_some())?;
    // A host-side kill alone would leave the guest process running (B-12).
    let end_guest = || {
        let _ = terminate(target, root, &token);
    };
    let outcome = process::run_collect(
        &plan,
        request.stdin.as_deref(),
        request.timeout,
        request.max_stdout,
        MAX_RESPONSE_RAW_BYTES + 1,
        cancel,
        &end_guest,
    )?;
    match outcome {
        Err(Stopped::Timeout) => Err(GuestError::new(
            ErrorCode::TimedOut,
            "The command in the computer took too long.",
        )),
        Err(Stopped::Overflow) => Err(GuestError::new(
            ErrorCode::OutputTooLarge,
            "The command in the computer printed too much.",
        )),
        Err(Stopped::Cancelled) => Err(GuestError::new(
            ErrorCode::Cancelled,
            "The command in the computer was cancelled.",
        )),
        Ok(collected) => {
            let code = collected.status.code().ok_or_else(|| {
                GuestError::internal("The connection to the computer ended unexpectedly.")
            })?;
            let stderr_text = String::from_utf8_lossy(&collected.stderr).into_owned();
            if collected.stderr_len as usize > MAX_RESPONSE_RAW_BYTES {
                return Err(GuestError::internal(
                    "The command printed more error output than one response can carry.",
                ));
            }
            let (exit_code, signal) = settle_exit(target, root, &token, code, &stderr_text)?;
            Ok(ExecOutput {
                exit_code,
                signal,
                stdout: collected.stdout,
                stderr: collected.stderr,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guest::test_support;

    fn request(argv: &[&str]) -> ExecRequest {
        ExecRequest {
            argv: argv.iter().map(|word| (*word).to_owned()).collect(),
            cwd: None,
            env: Vec::new(),
            user: GuestUser::Account,
            stdin: None,
            timeout: Duration::from_secs(20),
            max_stdout: 64 * 1024,
        }
    }

    #[test]
    fn requests_are_validated_before_any_effect() {
        let words = |list: &[&str]| {
            list.iter()
                .map(|word| (*word).to_owned())
                .collect::<Vec<_>>()
        };
        assert!(validate_request(&words(&["ls"]), Some("/tmp"), &[]).is_ok());
        assert!(validate_request(&[], None, &[]).is_err());
        assert!(validate_request(&words(&[""]), None, &[]).is_err());
        assert!(validate_request(&words(&["a\0b"]), None, &[]).is_err());
        assert!(validate_request(&words(&["ls"]), Some("relative"), &[]).is_err());
        let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
        assert!(validate_request(&words(&["ls"]), None, &[pair("", "x")]).is_err());
        assert!(validate_request(&words(&["ls"]), None, &[pair("A=B", "x")]).is_err());
        for name in [
            "-S /usr/bin/printf injected #",
            "-i",
            "A B",
            "1A",
            "A-B",
            "é",
            "A\n",
        ] {
            assert!(
                validate_request(&words(&["ls"]), None, &[pair(name, "x")]).is_err(),
                "{name:?} must be refused"
            );
        }
        assert!(validate_request(&words(&["ls"]), None, &[pair("_a1", "x")]).is_ok());
        assert!(
            validate_request(&words(&["ls"]), None, &[pair("A", "x"), pair("A", "y")]).is_err()
        );
        assert!(validate_request(&words(&["ls"]), None, &[pair("A", "x"), pair("B", "y")]).is_ok());
    }

    #[test]
    fn exit_255_is_a_guest_exit_only_when_the_supervisor_recorded_it() {
        let directory = tempfile::tempdir().unwrap();
        let mac = test_support::macos_target(directory.path());
        let never = || -> Option<String> { panic!("no signal lookup for 255") };
        // A recorded 255: the program ran and exited with 255.
        assert_eq!(
            settle_exit_with(&mac, 255, "", &|| Some(255), &never).unwrap(),
            (Some(255), None)
        );
        // Nothing recorded: SSH itself failed.
        let error =
            settle_exit_with(&mac, 255, "Connection refused", &|| None, &never).unwrap_err();
        assert_eq!(error.code, ErrorCode::NotRunning);
        assert!(error.message.contains("Connection refused"));
        // Other codes never ask for a record.
        let unused = || -> Option<i32> { panic!("no exit lookup below 255") };
        assert_eq!(
            settle_exit_with(&mac, 7, "", &unused, &never).unwrap(),
            (Some(7), None)
        );
        assert_eq!(
            settle_exit_with(&mac, 137, "", &unused, &|| Some("KILL".into())).unwrap(),
            (None, Some("KILL".into()))
        );
        // Linux has no such ambiguity: 255 is just a code.
        let linux = test_support::target(directory.path());
        assert_eq!(
            settle_exit_with(&linux, 255, "", &unused, &never).unwrap(),
            (Some(255), None)
        );
    }

    #[test]
    fn the_supervisor_records_exit_255_and_a_signal_death_leaves_its_name() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let token = "exit255token";
        let argv = supervised(token, None, &["sh".into(), "-c".into(), "exit 255".into()]);
        let plan = target.plan(&argv, &[], false, false).unwrap();
        let collected = process::run_collect(
            &plan,
            None,
            Duration::from_secs(20),
            1024,
            1024,
            None,
            &|| {},
        )
        .unwrap()
        .ok()
        .unwrap();
        assert_eq!(collected.status.code(), Some(255));
        assert_eq!(recorded_exit(&target, false, token), Some(255));
        assert_eq!(
            recorded_exit(&target, false, token),
            None,
            "reading the record clears it"
        );
        // Through the whole bounded run, a program that exits 255 is a plain exit on Linux.
        let output = run_bounded(&target, &request(&["sh", "-c", "exit 255"])).unwrap();
        assert_eq!(output.exit_code, Some(255));
    }

    #[test]
    fn bounded_runs_return_bytes_status_and_pass_arguments_as_data() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let mut run = request(&[
            "sh",
            "-c",
            "printf '%s|' \"$1\" \"$FROM_ENV\"; printf 'e1' >&2; exit 7",
            "sh",
            "$(echo shell; injected) 'quoted'",
        ]);
        run.env = vec![("FROM_ENV".into(), "env value".into())];
        let output = run_bounded(&target, &run).unwrap();
        assert_eq!(output.exit_code, Some(7));
        assert_eq!(output.signal, None);
        assert_eq!(
            output.stdout,
            b"$(echo shell; injected) 'quoted'|env value|".to_vec()
        );
        assert_eq!(output.stderr, b"e1".to_vec());
    }

    #[test]
    fn invalid_utf8_stdin_and_stdout_survive_byte_for_byte() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let bytes: Vec<u8> = (0..=255u8).chain([0xff, 0xfe, 0x00, 0x80]).collect();
        let mut run = request(&["cat"]);
        run.stdin = Some(bytes.clone());
        let output = run_bounded(&target, &run).unwrap();
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(output.stdout, bytes);
    }

    #[test]
    fn a_missing_program_and_a_missing_directory_fail_the_run_not_the_engine() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let missing = run_bounded(&target, &request(&["/no/such/program"])).unwrap();
        assert_eq!(missing.exit_code, Some(127));
        let mut run = request(&["true"]);
        run.cwd = Some("/no/such/directory".into());
        assert_eq!(run_bounded(&target, &run).unwrap().exit_code, Some(126));
        let mut run = request(&["pwd"]);
        run.cwd = Some(directory.path().to_string_lossy().into_owned());
        let output = run_bounded(&target, &run).unwrap();
        assert_eq!(output.exit_code, Some(0));
        assert!(String::from_utf8_lossy(&output.stdout)
            .trim()
            .ends_with(directory.path().file_name().unwrap().to_str().unwrap()));
    }

    #[test]
    fn a_signal_death_reports_the_signal_instead_of_an_exit_code() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let output = run_bounded(&target, &request(&["sh", "-c", "kill -KILL $$"])).unwrap();
        assert_eq!(output.exit_code, None);
        assert_eq!(output.signal.as_deref(), Some("KILL"));
    }

    #[test]
    fn stdout_beyond_the_allowance_is_an_error_but_the_exact_cap_succeeds() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let mut run = request(&["sh", "-c", "head -c 1000 /dev/zero"]);
        run.max_stdout = 1000;
        assert_eq!(run_bounded(&target, &run).unwrap().stdout.len(), 1000);
        run.max_stdout = 999;
        let error = run_bounded(&target, &run).unwrap_err();
        assert_eq!(error.code, ErrorCode::OutputTooLarge);
    }

    #[test]
    fn a_deadline_ends_the_whole_guest_process_tree() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let marker = directory.path().join("survivor");
        let script = format!("(sleep 2; touch '{}') & sleep 30", marker.display());
        let mut run = request(&["sh", "-c", &script]);
        run.timeout = Duration::from_millis(500);
        let started = std::time::Instant::now();
        let error = run_bounded(&target, &run).unwrap_err();
        assert_eq!(error.code, ErrorCode::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(2600));
        assert!(
            !marker.exists(),
            "the background child must have been stopped with its group"
        );
    }

    #[test]
    fn root_runs_ask_the_runtime_for_root_and_account_runs_set_the_login_names() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("msb.log");
        let target = test_support::target(directory.path());
        let mut run = request(&["true"]);
        run.user = GuestUser::Root;
        run_bounded(&target, &run).unwrap();
        run.user = GuestUser::Account;
        run_bounded(&target, &run).unwrap();
        let log = std::fs::read_to_string(log).unwrap();
        let runs: Vec<&str> = log.lines().filter(|line| line.contains("--user")).collect();
        assert!(runs
            .iter()
            .any(|line| line.contains("--user root --env USER=root --env LOGNAME=root")));
        assert!(runs
            .iter()
            .any(|line| line.contains("--user silo --env USER=silo --env LOGNAME=silo")));
        assert!(runs.iter().all(|line| line.contains("--no-start")));
    }
}
