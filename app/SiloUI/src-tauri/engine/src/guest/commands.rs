//! Commands the engine returns for clients to launch (`terminal.command`, `editor.command`)
//! and the owner-side key authorization (`guest.prepare`). The engine never opens a terminal
//! or an editor itself (95 A-22).
use super::{
    ssh_alias::{ensure_alias, AliasEnsure},
    target::Target,
    GuestError,
};
use serde_json::{json, Value};
use silo_api::jobs::ErrorCode;
use std::path::Path;

pub(crate) fn pending_parity(what: &str) -> GuestError {
    GuestError::new(
        ErrorCode::UnsupportedRemoteOperation,
        format!(
            "{what} for macOS computers arrives with the macOS access work (pending parity-2)."
        ),
    )
}

/// The argv a Mac terminal runs to open a shell in the computer: the bundled runtime's
/// interactive exec as the working account, in `/workspace`. It never starts the computer.
pub fn terminal_command(target: &Target) -> Result<Vec<String>, GuestError> {
    let Target::Linux { paths, name, .. } = target else {
        return Err(pending_parity("Terminals"));
    };
    target.ensure_running()?;
    let user = target.account_user()?;
    let text = |path: &Path| -> Result<String, GuestError> {
        path.to_str()
            .map(str::to_owned)
            .ok_or_else(|| GuestError::internal("Invalid runtime path."))
    };
    let executable = text(&paths.executable)?;
    Ok(vec![
        "/usr/bin/env".to_owned(),
        format!("MSB_HOME={}", text(&paths.home)?),
        format!("MSB_PATH={executable}"),
        format!("MSB_LIBKRUNFW_PATH={}", text(&paths.library)?),
        executable,
        "exec".into(),
        name.clone(),
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
    ])
}

/// Prepares the computer's alias and the editor's workspace, and returns the argv a Mac
/// launches to open `path` (default `/workspace`) in the editor chosen in `settings`.
pub fn editor_command(
    target: &Target,
    user_home: &Path,
    path: Option<&str>,
    settings: &serde_json::Map<String, Value>,
) -> Result<Vec<String>, GuestError> {
    if target.is_macos() {
        return Err(pending_parity("Editors"));
    }
    let path = path.unwrap_or("/workspace");
    crate::editor::validate_path(path)?;
    let application = crate::applications::selected_editor_from_settings(settings)?;
    let command = crate::applications::editor_command(&application)?;
    let alias = ensure_alias(
        target,
        user_home,
        &AliasEnsure {
            install_include: true,
            path: Some(path.to_owned()),
        },
    )?;
    let launch = crate::editor::editor_launch(&command, &alias.alias, path, user_home)?;
    let mut argv = vec![launch.get_program().to_string_lossy().into_owned()];
    argv.extend(
        launch
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned()),
    );
    Ok(argv)
}

/// `guest.prepare` on the owner: authorizes a controller's Ed25519 key for the working
/// account and returns the pinned host key, the account and (with `forwarding`) the guest's
/// address. Waits its turn on the computer's operation gate, like every key change.
///
/// `params` is the wire request (`computerId`, `publicKey`, `path`, `accountProtocol`,
/// `forwarding?`).
pub fn prepare(target: &Target, params: &Value) -> Result<Value, GuestError> {
    struct NoLocalAccess;
    impl crate::remote_access::LocalAccess for NoLocalAccess {
        fn desktop_connection(&self, _: &str, _: &str) -> Result<Value, String> {
            Err("Desktop access is not part of guest.prepare.".into())
        }
        fn desktop_dispatch(&self, _: &str, _: &Value) -> Result<Value, String> {
            Err("Desktop access is not part of guest.prepare.".into())
        }
        fn chatgpt_status(&self) -> Result<Value, String> {
            Err("ChatGPT access is not part of guest.prepare.".into())
        }
        fn chatgpt_retry(&self) -> Result<Value, String> {
            Err("ChatGPT access is not part of guest.prepare.".into())
        }
    }
    let Target::Linux { paths, id, .. } = target else {
        return Err(pending_parity("Key authorization"));
    };
    let mut request = params.clone();
    request["computerId"] = json!(id);
    let result = crate::remote_access::dispatch(
        paths,
        "guest.prepare",
        &request,
        &|_, _| Err("Checkpoints are not part of guest.prepare.".into()),
        &NoLocalAccess,
    )?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guest::test_support;

    #[test]
    fn the_terminal_command_is_the_runtimes_interactive_exec_as_the_working_account() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let argv = terminal_command(&target).unwrap();
        let paths = test_support::runtime(directory.path());
        assert_eq!(argv[0], "/usr/bin/env");
        assert_eq!(argv[1], format!("MSB_HOME={}", paths.home.display()));
        assert_eq!(argv[2], format!("MSB_PATH={}", paths.executable.display()));
        assert_eq!(argv[4], paths.executable.to_string_lossy());
        assert_eq!(
            argv[5..].join(" "),
            "exec dev --user silo --env USER=silo --env LOGNAME=silo --no-start --workdir /workspace --tty"
        );
    }

    #[test]
    fn macos_commands_wait_for_the_access_work_and_say_so() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::macos_target(directory.path());
        let home = directory.path().join("user");
        let settings = serde_json::Map::new();
        let errors = [
            terminal_command(&target).unwrap_err(),
            editor_command(&target, &home, None, &settings).unwrap_err(),
            prepare(&target, &json!({})).unwrap_err(),
            ensure_alias(
                &target,
                &home,
                &AliasEnsure {
                    install_include: true,
                    path: None,
                },
            )
            .unwrap_err(),
            crate::guest::remove_alias(&target, &home).unwrap_err(),
        ];
        for error in errors {
            assert_eq!(error.code, ErrorCode::UnsupportedRemoteOperation);
            assert!(
                error.message.contains("pending parity-2"),
                "{}",
                error.message
            );
        }
    }

    #[test]
    fn macos_exec_and_files_reach_the_guest_over_ssh_and_fail_cleanly_without_access() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::macos_target(directory.path());
        // The command line SSH carries quotes every word, so arguments stay data.
        let plan = target.plan(
            &["echo".into(), "it's $(unsafe)".into()],
            &[("A".into(), "b c".into())],
            true,
            false,
        );
        // Without the computer's key there is no way to log in: a clear error, no effect.
        assert_eq!(
            plan.err().unwrap().message,
            "This computer's access is missing."
        );
        assert_eq!(
            crate::guest::read(&target, "/tmp/x", None, None)
                .unwrap_err()
                .message,
            "This computer's access is missing."
        );
    }

    #[test]
    fn a_stopped_computer_has_no_terminal_command() {
        let directory = tempfile::tempdir().unwrap();
        let paths = test_support::runtime(directory.path());
        crate::test_support::write_shell_script(
            &paths.executable,
            r#"case "$1" in inspect) printf '%s\n' '{"name":"dev","status":"Stopped","config":{"labels":{"silo.managed":"true"}}}';; esac"#,
        );
        let target =
            crate::guest::resolve(Ok(paths), directory.path(), test_support::COMPUTER_ID).unwrap();
        let error = terminal_command(&target).unwrap_err();
        assert_eq!(error.code, ErrorCode::NotRunning);
        assert_eq!(error.message, "Start dev first.");
    }

    #[test]
    fn editor_commands_check_the_folder_and_the_chosen_editor_before_writing_anything() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let home = directory.path().join("user");
        std::fs::create_dir_all(&home).unwrap();
        let settings = serde_json::Map::new();
        let error = editor_command(&target, &home, Some("/etc"), &settings).unwrap_err();
        assert!(error.message.contains("Choose a folder inside /workspace"));
        let mut settings = serde_json::Map::new();
        settings.insert("editorUseSystemDefault".into(), json!(false));
        settings.insert("editorPath".into(), json!("/nonexistent/Editor.app"));
        let error = editor_command(&target, &home, None, &settings).unwrap_err();
        assert!(error.message.contains("selected editor is unavailable"));
        assert!(!home.join(".ssh").exists());
    }
}
