//! Silo computers work as the `silo` account (UID/GID 1001, home `/home/silo`).
//!
//! The guest records its account in `/var/lib/silo/working-account.json`. After each
//! boot, Silo checks that record and, when it is missing, runs the guest setup as root:
//! a new computer gets a fresh account, and a computer from an older Silo, with agent files under
//! root, has them copied into it. The setup writes the record last and can be repeated,
//! so an interrupted setup finishes at the next boot. The record lives on the computer's disk,
//! so restores, forks and imports carry it. See docs/SiloUI-WORKING-ACCOUNT.md.
use crate::runtime::{self, RuntimeError, RuntimePaths, RuntimeRunner};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime},
};

pub(crate) const USER: &str = "silo";

/// Prints `ready`, `missing` or, for a record this Silo does not know, `unknown`.
const CHECK: &str = r#"record=/var/lib/silo/working-account.json
if [ ! -e "$record" ]; then echo missing
elif [ "$(tr -d ' \n' < "$record")" = '{"schemaVersion":1,"user":"silo","home":"/home/silo"}' ]; then echo ready
else echo unknown
fi"#;
const SET_UP: &str = include_str!("../guest/working-account.sh");
const ACCOUNT: &str = include_str!("../guest/working-account.py");
const DESKTOP_SERVICE: &str = include_str!("../guest/desktop-service.py");
/// Moving an older computer copies its home folders and takes ownership of `/workspace`.
const SET_UP_LIMIT: &str = "30m";
const SET_UP_TIMEOUT: Duration = Duration::from_secs(31 * 60);

fn exec(name: &str, limit: &str, command: &[&str]) -> Vec<String> {
    let options = ["--no-start", "--no-tty", "--quiet", "--timeout", limit];
    ["exec", name]
        .into_iter()
        .chain(options)
        .chain(["--user", "root", "--workdir", "/", "--"])
        .chain(command.iter().copied())
        .map(str::to_owned)
        .collect()
}

/// Make a computer Silo just booted ready for the silo account. A failure leaves the computer
/// running; the caller stops it, so a running computer always has its account.
pub(crate) fn prepare(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
) -> Result<(), RuntimeError> {
    let checked = runner.run(
        paths,
        &exec(name, "30s", &["/bin/sh", "-c", CHECK]),
        Duration::from_secs(45),
    )?;
    match checked.stdout.trim() {
        "ready" => return Ok(()),
        "missing" => {}
        _ => return Err(RuntimeError::Invalid(format!(
            "{name} records an account layout this version of Silo does not know. Update Silo, then start it again."
        ))),
    }
    let step = format!("Setting up the silo account in {name}");
    let command = ["/bin/sh", "-c", SET_UP, "sh", ACCOUNT, DESKTOP_SERVICE];
    runtime::OPERATIONS
        .labelled(&step, || {
            runner.run(paths, &exec(name, SET_UP_LIMIT, &command), SET_UP_TIMEOUT)
        })
        .map(drop)
        .map_err(|error| failure(runner, paths, name, step, error))
}

fn failure(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
    step: String,
    error: RuntimeError,
) -> RuntimeError {
    if !matches!(
        error,
        RuntimeError::Failed { .. } | RuntimeError::TimedOut { .. }
    ) {
        return error;
    }
    // Copying home folders fills the computer's memory with file cache; on a host short of
    // memory the computer is killed and the command only reports its lost session.
    if runtime::inspect_computer(runner, paths, name)
        .is_ok_and(|computer| computer.status == "Crashed")
    {
        return RuntimeError::Unavailable(format!(
            "{name} stopped unexpectedly while Silo set up its silo account. This device may have run out of memory: stop other computers, then start it again."
        ));
    }
    match error {
        RuntimeError::Failed {
            exit_code, detail, ..
        } => match reason(&detail) {
            Some(reason) => RuntimeError::Invalid(format!(
                "Silo could not set up the silo account in {name}: {reason}"
            )),
            None => RuntimeError::Failed {
                operation: step,
                exit_code,
                detail,
            },
        },
        _ => RuntimeError::TimedOut { operation: step },
    }
}

/// The message of a `RuntimeError` the guest setup raised, from its traceback's last line.
fn reason(detail: &str) -> Option<&str> {
    detail
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())?
        .strip_prefix("RuntimeError: ")
        .filter(|message| !message.is_empty())
}

/// The account to open `name`'s guest as, once Silo manages it and the bundled runtime
/// supports the account.
pub(crate) fn inspect_user(paths: &RuntimePaths, name: &str) -> Result<&'static str, String> {
    let inspected = runtime::inspect_computer(&runtime::ProcessRunner, paths, name)
        .map_err(|e| e.to_string())?;
    runtime::ensure_managed(&inspected).map_err(|e| e.to_string())?;
    require_runtime(paths)?;
    Ok(USER)
}

/// The executables (by path, size and modification time) that reported support for the
/// account; only a success is remembered.
fn supported_runtimes() -> &'static Mutex<HashSet<(PathBuf, u64, Option<SystemTime>)>> {
    static SUPPORTED: OnceLock<Mutex<HashSet<(PathBuf, u64, Option<SystemTime>)>>> =
        OnceLock::new();
    SUPPORTED.get_or_init(Default::default)
}

pub(crate) fn require_runtime(paths: &RuntimePaths) -> Result<(), String> {
    const UNSUPPORTED: &str = "The bundled runtime cannot open this computer's Linux account. Relaunch Silo to rerun system checks, then repair or update Silo.";
    let identity = std::fs::metadata(&paths.executable).ok().map(|metadata| {
        (
            paths.executable.clone(),
            metadata.len(),
            metadata.modified().ok(),
        )
    });
    if let Some(identity) = &identity {
        if supported_runtimes()
            .lock()
            .is_ok_and(|supported| supported.contains(identity))
        {
            return Ok(());
        }
    }
    let output = runtime::run_msb(
        paths,
        &["--silo-working-account-protocol".into()],
        Duration::from_secs(10),
    )
    .map_err(|_| UNSUPPORTED)?;
    if output.stdout.trim() != "1" {
        return Err(UNSUPPORTED.into());
    }
    if let (Some(identity), Ok(mut supported)) = (identity, supported_runtimes().lock()) {
        supported.insert(identity);
    }
    Ok(())
}

/// The account another device's Silo reports for its computer. Versions before the
/// silo account report root, or nothing.
pub(crate) fn response_user(response: &Value) -> Result<&'static str, String> {
    match response.get("user") {
        Some(Value::String(user)) if user == USER => Ok(USER),
        _ => Err("The other device opens this computer with an account this Silo does not support. Update Silo on both devices.".into()),
    }
}

pub(crate) fn require_client_protocol(request: &Value) -> Result<(), String> {
    if request.get("accountProtocol").and_then(Value::as_u64) != Some(1) {
        return Err(
            "Update Silo on the connecting device to access this computer's Linux account.".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn test_runtime(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let inspected = serde_json::json!({"name":"dev","status":"Running","config":{"labels":{"silo.managed":"true"}}});
    std::fs::write(
        path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --silo-working-account-protocol ]; then printf '1\\n'; exit; fi\n[ \"$1\" = inspect ] || exit 2\nprintf '%s\\n' '{}'\n",
            inspected
        ),
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::CommandOutput;
    use serde_json::json;
    use std::sync::Mutex;

    /// Answers the account check, the setup and `inspect` like the runtime would.
    struct Guest {
        check: &'static str,
        set_up: Mutex<Option<RuntimeError>>,
        status: &'static str,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl Guest {
        fn new(check: &'static str, set_up: Result<(), RuntimeError>) -> Self {
            Self {
                check,
                set_up: Mutex::new(set_up.err()),
                status: "Running",
                calls: Mutex::new(Vec::new()),
            }
        }

        fn commands(&self) -> Vec<String> {
            let calls = self.calls.lock().unwrap();
            calls
                .iter()
                .map(|args| match args.iter().position(|arg| arg == "--") {
                    Some(at) if args[at + 3] == CHECK => "check".into(),
                    Some(at) if args[at + 3] == SET_UP => "set up".into(),
                    _ => args[0].clone(),
                })
                .collect()
        }
    }

    impl RuntimeRunner for Guest {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let output = |stdout: &str| CommandOutput {
                stdout: stdout.into(),
                stderr: String::new(),
            };
            match self.commands().last().unwrap().as_str() {
                "check" => Ok(output(self.check)),
                "set up" => match self.set_up.lock().unwrap().take() {
                    Some(error) => Err(error),
                    None => Ok(output("")),
                },
                "inspect" => Ok(output(
                    &json!({"name":"dev","status":self.status,"config":{}}).to_string(),
                )),
                other => panic!("unexpected {other}"),
            }
        }
    }

    fn paths() -> RuntimePaths {
        let root = std::path::Path::new("/nonexistent");
        RuntimePaths {
            executable: root.join("msb"),
            library: root.join("msb"),
            home: root.join("home"),
            storage_home: None,
            guest_image: root.join("image"),
            metadata: root.join("computers.json"),
            volumes: root.join("volumes"),
        }
    }

    fn failed(detail: &str) -> RuntimeError {
        RuntimeError::Failed {
            operation: "exec dev".into(),
            exit_code: Some(1),
            detail: detail.into(),
        }
    }

    #[test]
    fn a_computer_with_the_account_is_only_checked() {
        let guest = Guest::new("ready\n", Ok(()));
        prepare(&guest, &paths(), "dev").unwrap();
        assert_eq!(guest.commands(), ["check"]);
        let calls = guest.calls.lock().unwrap();
        assert_eq!(
            calls[0][..12],
            [
                "exec",
                "dev",
                "--no-start",
                "--no-tty",
                "--quiet",
                "--timeout",
                "30s",
                "--user",
                "root",
                "--workdir",
                "/",
                "--"
            ]
        );
    }

    #[test]
    fn a_new_or_older_computer_is_set_up_as_root_with_the_bundled_scripts() {
        let guest = Guest::new("missing\n", Ok(()));
        prepare(&guest, &paths(), "dev").unwrap();
        assert_eq!(guest.commands(), ["check", "set up"]);
        let calls = guest.calls.lock().unwrap();
        let at = calls[1].iter().position(|arg| arg == "--").unwrap();
        assert!(calls[1][..at]
            .windows(2)
            .any(|pair| pair == ["--user", "root"]));
        assert!(calls[1][..at]
            .windows(2)
            .any(|pair| pair == ["--timeout", SET_UP_LIMIT]));
        assert_eq!(calls[1][at + 4..], ["sh", ACCOUNT, DESKTOP_SERVICE]);
    }

    #[test]
    fn an_unknown_account_record_is_never_overwritten() {
        let guest = Guest::new("unknown\n", Ok(()));
        let error = prepare(&guest, &paths(), "dev").unwrap_err().to_string();
        assert!(error.contains("Update Silo"), "{error}");
        assert_eq!(guest.commands(), ["check"]);
    }

    #[test]
    fn the_setups_own_reason_is_shown_and_other_failures_keep_their_details() {
        let traceback = "Traceback (most recent call last):\n  File \"<string>\", line 9\nRuntimeError: The reserved account ID 1001 belongs to another account.\n";
        let guest = Guest::new("missing", Err(failed(traceback)));
        let error = prepare(&guest, &paths(), "dev").unwrap_err();
        assert_eq!(
            error.to_string(),
            "Silo could not set up the silo account in dev: The reserved account ID 1001 belongs to another account."
        );
        let guest = Guest::new("missing", Err(failed("E: Unable to locate package sudo")));
        let RuntimeError::Failed {
            operation, detail, ..
        } = prepare(&guest, &paths(), "dev").unwrap_err()
        else {
            panic!("expected the runtime failure");
        };
        assert_eq!(operation, "Setting up the silo account in dev");
        assert_eq!(detail, "E: Unable to locate package sudo");
        let guest = Guest::new(
            "missing",
            Err(RuntimeError::TimedOut {
                operation: "exec dev".into(),
            }),
        );
        assert!(prepare(&guest, &paths(), "dev")
            .unwrap_err()
            .to_string()
            .starts_with("Setting up the silo account in dev timed out"));
        let guest = Guest::new(
            "missing",
            Err(RuntimeError::Cancelled {
                operation: "exec dev".into(),
            }),
        );
        assert!(matches!(
            prepare(&guest, &paths(), "dev"),
            Err(RuntimeError::Cancelled { .. })
        ));
    }

    #[test]
    fn a_computer_that_crashes_during_setup_points_at_memory() {
        let mut guest = Guest::new("missing", Err(failed("connection reset")));
        guest.status = "Crashed";
        let error = prepare(&guest, &paths(), "dev").unwrap_err().to_string();
        assert!(
            error.contains("stopped unexpectedly") && error.contains("memory"),
            "{error}"
        );
        assert_eq!(guest.commands(), ["check", "set up", "inspect"]);
    }

    #[test]
    fn the_check_accepts_records_from_every_silo_that_wrote_them() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("working-account.json");
        let check = CHECK.replace(
            "/var/lib/silo/working-account.json",
            record.to_str().unwrap(),
        );
        let run = || {
            let output = std::process::Command::new("/bin/sh")
                .args(["-c", &check])
                .output()
                .unwrap();
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        assert_eq!(run(), "missing");
        // The earlier setup script wrote compact JSON; the earlier migration, Python's default.
        for text in [
            "{\"schemaVersion\":1,\"user\":\"silo\",\"home\":\"/home/silo\"}\n",
            "{\"schemaVersion\": 1, \"user\": \"silo\", \"home\": \"/home/silo\"}\n",
        ] {
            std::fs::write(&record, text).unwrap();
            assert_eq!(run(), "ready", "{text}");
        }
        for text in [
            "{}\n",
            "{\"schemaVersion\":2,\"user\":\"silo\",\"home\":\"/home/silo\"}",
        ] {
            std::fs::write(&record, text).unwrap();
            assert_eq!(run(), "unknown", "{text}");
        }
    }

    #[test]
    fn working_account_rejects_old_runtime_before_preparing_ssh() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let paths = RuntimePaths {
            executable: root.join("msb"),
            library: root.join("msb"),
            home: root.join("home"),
            storage_home: None,
            guest_image: root.join("image"),
            metadata: root.join("computers.json"),
            volumes: root.join("volumes"),
        };
        test_runtime(&paths.executable);
        assert_eq!(inspect_user(&paths, "dev").unwrap(), "silo");
        let script = std::fs::read_to_string(&paths.executable).unwrap();
        std::fs::write(&paths.executable, script.replace("printf '1", "printf '0")).unwrap();
        let error = inspect_user(&paths, "dev").unwrap_err();
        assert!(
            error.contains("Relaunch Silo") && error.contains("repair or update Silo"),
            "{error}"
        );
    }

    #[test]
    fn a_supported_runtime_is_probed_once() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let paths = RuntimePaths {
            executable: root.join("msb"),
            library: root.join("msb"),
            home: root.join("home"),
            storage_home: None,
            guest_image: root.join("image"),
            metadata: root.join("computers.json"),
            volumes: root.join("volumes"),
        };
        let probes = root.join("probes");
        std::fs::write(
            &paths.executable,
            format!(
                "#!/bin/sh\necho probe >>'{}'\nprintf '1\\n'\n",
                probes.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&paths.executable, std::fs::Permissions::from_mode(0o700))
            .unwrap();
        for _ in 0..3 {
            require_runtime(&paths).unwrap();
        }
        assert_eq!(std::fs::read_to_string(&probes).unwrap().lines().count(), 1);
    }

    #[test]
    fn working_account_requires_aware_remote_clients() {
        assert!(require_client_protocol(&json!({"accountProtocol":1})).is_ok());
        for request in [
            json!({}),
            json!({"accountProtocol":null}),
            json!({"accountProtocol":2}),
            json!({"accountProtocol":"1"}),
        ] {
            assert!(require_client_protocol(&request).is_err());
        }
    }

    #[test]
    fn working_account_remote_response_requires_silo() {
        assert!(response_user(&json!({})).is_err());
        assert_eq!(response_user(&json!({"user":"silo"})).unwrap(), "silo");
        assert!(response_user(&json!({"user":"root"})).is_err());
        for user in [json!(null), json!("silo;id"), json!("unknown"), json!(3)] {
            assert!(response_user(&json!({"user":user})).is_err());
        }
    }
}
