//! Open the selected terminal on MicroSandbox's existing interactive exec path.
use crate::runtime::{self, RuntimePaths};
pub use silo_host::terminal::quote;
use std::process::Command;

pub fn command(paths: &RuntimePaths, name: &str) -> Result<String, String> {
    let user = crate::working_account::USER;
    runtime::validate_name(name).map_err(|e| e.to_string())?;
    let home = paths.home.to_str().ok_or("Invalid runtime home path.")?;
    let executable = paths.executable.to_str().ok_or("Invalid runtime path.")?;
    let library = paths
        .library
        .to_str()
        .ok_or("Invalid runtime library path.")?;
    let args = [
        "/usr/bin/env".to_string(),
        format!("MSB_HOME={home}"),
        format!("MSB_PATH={executable}"),
        format!("MSB_LIBKRUNFW_PATH={library}"),
        executable.into(),
        "exec".into(),
        name.into(),
        "--user".into(),
        user.into(),
        "--env".into(),
        format!("USER={user}"),
        "--env".into(),
        format!("LOGNAME={user}"),
        "--no-start".into(),
        "--workdir".into(),
        "/workspace".into(),
        "--tty".into(),
    ];
    Ok(args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" "))
}
/// The inspected computer, or "Start <name> first." when it is stopped or has no runtime computer yet.
pub fn running_computer(
    paths: &RuntimePaths,
    name: &str,
) -> Result<runtime::InspectedSandbox, String> {
    running_computer_with(&runtime::ProcessRunner, paths, name)
}
pub(crate) fn running_computer_with(
    runner: &dyn runtime::RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
) -> Result<runtime::InspectedSandbox, String> {
    let inspected =
        match runtime::observe_computer(runner, paths, name).map_err(|e| e.to_string())? {
            runtime::ComputerRuntime::Absent => return Err(start_first(name)),
            runtime::ComputerRuntime::Present(inspected) => inspected,
        };
    runtime::ensure_managed(&inspected).map_err(|e| e.to_string())?;
    if inspected.status != "Running" {
        return Err(start_first(name));
    }
    Ok(inspected)
}
pub(crate) fn start_first(name: &str) -> String {
    format!("Start {name} first.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::runner::{ExpectedCommand, ScriptedRunner};

    #[test]
    fn only_a_running_managed_computer_can_open_a_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(dir.path());
        for status in [
            "Running", "Created", "Stopped", "Starting", "Draining", "Paused", "Crashed", "unknown",
        ] {
            let runner = ScriptedRunner::new([ExpectedCommand::ok(
                ["inspect", "dev", "--format", "json"],
                serde_json::json!({
                    "name": "dev", "status": status,
                    "config": {"labels": {"silo.managed": "true"}},
                    "runtime_instance_id": "instance-1"
                })
                .to_string(),
            )]);
            let result = running_computer_with(&runner, &paths, "dev");
            if status == "Running" {
                let inspected = result.unwrap();
                assert_eq!(inspected.name, "dev");
                assert_eq!(inspected.runtime_instance_id.as_deref(), Some("instance-1"));
            } else {
                assert_eq!(result.unwrap_err(), "Start dev first.", "{status}");
            }
            runner.assert_finished();
        }
    }

    #[test]
    fn an_unmanaged_running_computer_is_rejected_without_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let runner = ScriptedRunner::new([ExpectedCommand::ok(
            ["inspect", "dev", "--format", "json"],
            r#"{"name":"dev","status":"Running","config":{"labels":{}}}"#,
        )]);
        assert_eq!(
            running_computer_with(&runner, &crate::test_support::paths(dir.path()), "dev")
                .unwrap_err(),
            "Computer 'dev' is not owned by Silo. No computer operation was performed."
        );
        runner.assert_finished();
    }

    #[test]
    fn missing_computer_and_failed_inspection_have_distinct_recovery_messages() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(dir.path());
        let missing = ScriptedRunner::new([ExpectedCommand::error(
            ["inspect", "dev", "--format", "json"],
            runtime::RuntimeError::Failed {
                operation: "Inspect".into(),
                exit_code: Some(1),
                detail: "no such sandbox".into(),
            },
        )]);
        assert_eq!(
            running_computer_with(&missing, &paths, "dev").unwrap_err(),
            "Start dev first."
        );
        missing.assert_finished();

        let timed_out = ScriptedRunner::new([ExpectedCommand::error(
            ["inspect", "dev", "--format", "json"],
            runtime::RuntimeError::TimedOut {
                operation: "Inspect".into(),
            },
        )]);
        assert_eq!(
            running_computer_with(&timed_out, &paths, "dev").unwrap_err(),
            "Inspect timed out. Check the computer state, then retry."
        );
        timed_out.assert_finished();

        let malformed = ScriptedRunner::new([ExpectedCommand::ok(
            ["inspect", "dev", "--format", "json"],
            "{broken",
        )]);
        assert_eq!(
            running_computer_with(&malformed, &paths, "dev").unwrap_err(),
            "The bundled runtime returned invalid state for computer 'dev'."
        );
        malformed.assert_finished();
    }

    #[test]
    fn terminal_command_rejects_non_utf8_runtime_paths() {
        use std::os::unix::ffi::OsStringExt;

        let dir = tempfile::tempdir().unwrap();
        let invalid =
            std::path::PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/runtime-\xff".to_vec()));
        for field in ["home", "executable", "library"] {
            let mut paths = crate::test_support::paths(dir.path());
            match field {
                "home" => paths.home = invalid.clone(),
                "executable" => paths.executable = invalid.clone(),
                "library" => paths.library = invalid.clone(),
                _ => unreachable!(),
            }
            assert!(command(&paths, "dev").is_err(), "accepted lossy {field}");
        }
    }

    #[test]
    fn terminal_command_passes_literal_paths_environment_and_arguments_to_the_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = crate::test_support::paths(dir.path());
        paths.executable = dir.path().join("msb ' $(false) `false`");
        paths.home = dir.path().join("home ' $HOME `false`");
        paths.library = dir.path().join("library ' $(false)");
        crate::test_support::write_shell_script(
            &paths.executable,
            "printf '%s\\n' \"$MSB_HOME\" \"$MSB_PATH\" \"$MSB_LIBKRUNFW_PATH\" \"$@\"",
        );
        let output = Command::new("/bin/sh")
            .args(["-c", &command(&paths, "dev").unwrap()])
            .env("HOME", dir.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        let mut expected = vec![
            paths.home.to_str().unwrap(),
            paths.executable.to_str().unwrap(),
            paths.library.to_str().unwrap(),
        ];
        expected.extend([
            "exec",
            "dev",
            "--user",
            "silo",
            "--env",
            "USER=silo",
            "--env",
            "LOGNAME=silo",
            "--no-start",
            "--workdir",
            "/workspace",
            "--tty",
        ]);
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{}\n", expected.join("\n"))
        );
    }

    #[test]
    fn opens_in_computer_without_starting_a_stopped_computer() {
        let paths = RuntimePaths {
            guest_image: crate::test_support::manifest_root().join("runtime/guest-image"),
            executable: "/tmp/Silo app/msb".into(),
            home: "/tmp/runtime home".into(),
            storage_home: None,
            library: "/tmp/lib.dylib".into(),
            metadata: "/tmp/meta".into(),
            volumes: "/tmp/volumes".into(),
        };
        let user = "silo";
        let text = command(&paths, "dev").unwrap();
        assert!(text.contains("'--no-start' '--workdir' '/workspace' '--tty'"));
        assert!(text.contains("'MSB_HOME=/tmp/runtime home'"));
        assert!(text.contains(&format!(
            "'--user' '{user}' '--env' 'USER={user}' '--env' 'LOGNAME={user}'"
        )));
        assert!(command(&paths, "bad;name").is_err());
    }
}
