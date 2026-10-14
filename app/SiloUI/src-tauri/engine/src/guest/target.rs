//! Which computer a `computerId` names, and how a command reaches its guest.
use super::{process::Plan, quote, GuestError};
use crate::{
    macos_computers::GuestSsh,
    runtime::{self, RuntimePaths},
};
use silo_api::jobs::ErrorCode;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

/// The guest working account as the wire reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub user: String,
    pub uid: u32,
    pub home: String,
}

/// A computer on this device, resolved by its stable id.
#[derive(Clone)]
pub enum Target {
    Linux {
        paths: RuntimePaths,
        id: String,
        name: String,
    },
    Macos(GuestSsh),
}

/// The working account of a Linux computer. Checking that the computer is managed and that the
/// bundled runtime supports the account launches two runtime processes, so a positive answer is
/// reused for a few seconds; a failure is never cached.
fn linux_user(paths: &RuntimePaths, name: &str) -> Result<&'static str, GuestError> {
    static VERIFIED: OnceLock<Mutex<HashMap<(PathBuf, String), Instant>>> = OnceLock::new();
    const REUSE: Duration = Duration::from_secs(5);
    let key = (paths.home.clone(), name.to_owned());
    let verified = VERIFIED.get_or_init(Default::default);
    let recent = verified
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&key)
        .is_some_and(|at| at.elapsed() < REUSE);
    if recent {
        return Ok(crate::working_account::USER);
    }
    let user = crate::working_account::inspect_user(paths, name)?;
    verified
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key, Instant::now());
    Ok(user)
}

/// The one command line SSH carries to a macOS guest: every word single-quoted, so caller
/// text is data and never shell source. Root work goes through `sudo -n`.
pub(crate) fn command_line(argv: &[String], env: &[(String, String)], root: bool) -> String {
    let mut words = Vec::new();
    if root {
        words.extend(["/usr/bin/sudo".to_owned(), "-n".into()]);
    }
    words.push("/usr/bin/env".into());
    for (key, value) in env {
        words.push(format!("{key}={value}"));
    }
    words.extend(argv.iter().cloned());
    words
        .iter()
        .map(|word| quote(word))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolves `computer_id` against this device's Linux inventory, then its macOS computers.
///
/// `runtime` is the Linux runtime when the device has one; a device whose runtime is not
/// available can still resolve macOS computers.
pub fn resolve(
    runtime: Result<RuntimePaths, String>,
    app_data: &Path,
    computer_id: &str,
) -> Result<Target, GuestError> {
    if let Ok(paths) = &runtime {
        let inventory = runtime::read_metadata(&paths.metadata)?;
        if let Some(computer) = inventory
            .computers
            .iter()
            .find(|computer| computer.id() == computer_id)
        {
            return Ok(Target::Linux {
                paths: paths.clone(),
                id: computer.id().to_owned(),
                name: computer.name().to_owned(),
            });
        }
    }
    if let Some(shell) = crate::macos_computers::guest_ssh(app_data, computer_id)? {
        return Ok(Target::Macos(shell));
    }
    match runtime {
        Err(message) => Err(message.into()),
        Ok(_) => Err(GuestError::not_found(
            "This computer no longer exists on this device.",
        )),
    }
}

impl Target {
    pub fn id(&self) -> &str {
        match self {
            Self::Linux { id, .. } => id,
            Self::Macos(shell) => shell.id(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Linux { name, .. } => name,
            Self::Macos(shell) => shell.name(),
        }
    }

    pub fn is_macos(&self) -> bool {
        matches!(self, Self::Macos(_))
    }

    pub fn runtime_paths(&self) -> Option<&RuntimePaths> {
        match self {
            Self::Linux { paths, .. } => Some(paths),
            Self::Macos(_) => None,
        }
    }

    /// The working account's login name, once the computer can open it.
    pub fn account_user(&self) -> Result<&'static str, GuestError> {
        match self {
            Self::Linux { paths, name, .. } => linux_user(paths, name),
            Self::Macos(shell) => Ok(shell.user()),
        }
    }

    /// The working account: login name, numeric id and home folder, read from the running
    /// guest. Never derived from the host user.
    pub fn account(&self) -> Result<Account, GuestError> {
        use super::exec::{run_bounded, ExecRequest, GuestUser};
        let output = run_bounded(
            self,
            &ExecRequest {
                argv: vec![
                    "perl".into(),
                    "-e".into(),
                    "my @account = getpwuid($<); print \"$account[2]\\n$account[7]\\n\";".into(),
                ],
                cwd: None,
                env: Vec::new(),
                user: GuestUser::Account,
                stdin: None,
                timeout: Duration::from_secs(10),
                max_stdout: 8192,
            },
        )?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let mut lines = text.lines();
        let uid = lines
            .next()
            .and_then(|line| line.trim().parse::<u32>().ok());
        let home = lines
            .next()
            .map(str::trim)
            .filter(|home| home.starts_with('/'));
        match (output.exit_code, uid, home) {
            (Some(0), Some(uid), Some(home)) => Ok(Account {
                user: self.account_user()?.to_owned(),
                uid,
                home: home.to_owned(),
            }),
            _ => Err(GuestError::internal(
                "The computer did not report its account.",
            )),
        }
    }

    /// "Start <name> first." (`not_running`) unless the guest is up. Never starts a computer.
    pub fn ensure_running(&self) -> Result<(), GuestError> {
        match self {
            Self::Linux { paths, name, .. } => {
                crate::terminal::running_computer(paths, name)?;
                Ok(())
            }
            Self::Macos(shell) => Ok(shell.ensure_running()?),
        }
    }

    /// The command that runs `argv` in the guest and nothing else: no supervision, no cwd.
    ///
    /// `root` runs as root; otherwise as the working account with `USER` and `LOGNAME` set.
    /// `env` is passed to the process. `stdin` says whether the caller feeds standard input;
    /// otherwise the guest sees end-of-file immediately.
    pub(crate) fn plan(
        &self,
        argv: &[String],
        env: &[(String, String)],
        root: bool,
        stdin: bool,
    ) -> Result<Plan, GuestError> {
        match self {
            Self::Linux { paths, name, .. } => {
                let user = if root {
                    "root"
                } else {
                    linux_user(paths, name)?
                };
                let mut args = vec![
                    "exec".to_owned(),
                    name.clone(),
                    "--no-start".into(),
                    "--user".into(),
                    user.into(),
                    "--env".into(),
                    format!("USER={user}"),
                    "--env".into(),
                    format!("LOGNAME={user}"),
                ];
                for (key, value) in env {
                    args.push("--env".into());
                    args.push(format!("{key}={value}"));
                }
                if stdin {
                    // Byte-faithful bidirectional streaming without a terminal.
                    args.push("--stream".into());
                } else {
                    args.extend(["--no-tty".into(), "--no-stdin".into()]);
                }
                args.push("--quiet".into());
                args.push("--".into());
                args.extend(argv.iter().cloned());
                Ok(Plan {
                    program: paths.executable.clone(),
                    args,
                    envs: vec![
                        ("MSB_HOME".into(), paths.home.to_string_lossy().into_owned()),
                        (
                            "MSB_PATH".into(),
                            paths.executable.to_string_lossy().into_owned(),
                        ),
                        (
                            "MSB_LIBKRUNFW_PATH".into(),
                            paths.library.to_string_lossy().into_owned(),
                        ),
                    ],
                })
            }
            Self::Macos(shell) => {
                let command = command_line(argv, env, root);
                let (program, args) = shell.invocation(&command)?;
                Ok(Plan {
                    program: PathBuf::from(program),
                    args,
                    envs: Vec::new(),
                })
            }
        }
    }

    /// Turns a transport-level failure of the command channel itself (not of the guest
    /// command) into the code a client should see.
    pub(crate) fn channel_error(&self, exit_code: i32, stderr: &str) -> Option<GuestError> {
        let detail = stderr.trim();
        match self {
            // ssh reserves 255 for its own failures.
            Self::Macos(_) if exit_code == 255 => Some(GuestError::new(
                ErrorCode::NotRunning,
                if detail.is_empty() {
                    format!("Could not connect to {}. Start it first.", self.name())
                } else {
                    format!("Could not connect to {}: {detail}", self.name())
                },
            )),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guest::test_support;

    #[test]
    fn the_macos_command_line_quotes_every_word_so_arguments_stay_data() {
        let words = |list: &[&str]| {
            list.iter()
                .map(|word| (*word).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            command_line(
                &words(&["echo", "it's $(unsafe); rm -rf /"]),
                &[("A".into(), "b c".into())],
                false
            ),
            "'/usr/bin/env' 'A=b c' 'echo' 'it'\\''s $(unsafe); rm -rf /'"
        );
        assert!(command_line(&words(&["true"]), &[], true)
            .starts_with("'/usr/bin/sudo' '-n' '/usr/bin/env' 'true'"));
    }

    #[test]
    fn the_account_is_read_from_the_guest_not_derived_from_the_host() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let account = target.account().unwrap();
        // The fake guest is this machine: its uid and home come from the password database.
        assert_eq!(account.user, "silo");
        assert_eq!(account.uid, unsafe { libc::getuid() });
        assert!(account.home.starts_with('/'));
    }

    #[test]
    fn ids_resolve_to_the_computer_and_unknown_ids_are_not_found() {
        let directory = tempfile::tempdir().unwrap();
        let paths = test_support::runtime(directory.path());
        let target = resolve(
            Ok(paths.clone()),
            directory.path(),
            test_support::COMPUTER_ID,
        )
        .unwrap();
        assert_eq!(target.id(), test_support::COMPUTER_ID);
        assert_eq!(target.name(), test_support::COMPUTER_NAME);
        assert!(!target.is_macos());
        let error = resolve(
            Ok(paths),
            directory.path(),
            "00000000-0000-4000-8000-00000000ffff",
        )
        .err()
        .unwrap();
        assert_eq!(error.code, ErrorCode::NotFound);
        // A device without a Linux runtime still resolves macOS computers, and says why
        // it cannot find anything else.
        let error = resolve(
            Err("The runtime is unavailable.".into()),
            directory.path(),
            test_support::COMPUTER_ID,
        )
        .err()
        .unwrap();
        assert_eq!(error.message, "The runtime is unavailable.");
    }
}
