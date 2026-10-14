//! Commands the engine returns for clients to launch (`terminal.command`, `editor.command`)
//! and the owner-side key authorization (`guest.prepare`). The engine never opens a terminal
//! or an editor itself (95 A-22).
use super::{
    exec::{run_bounded, ExecOutput, ExecRequest, GuestUser},
    ssh_alias::{ensure_alias, AliasEnsure},
    target::Target,
    GuestError,
};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

/// The answer when a macOS computer reaches a Linux-only path: macOS terminals, editors and
/// aliases go through [`super::macos`], which needs the engine's macOS context.
fn macos_adapter(what: &str) -> GuestError {
    GuestError::internal(format!(
        "{what} for macOS computers are built by guest::macos."
    ))
}

/// The argv a Mac terminal runs to open a shell in the computer: the bundled runtime's
/// interactive exec as the working account, in `/workspace`. It never starts the computer.
pub fn terminal_command(target: &Target) -> Result<Vec<String>, GuestError> {
    let Target::Linux { paths, name, .. } = target else {
        return Err(macos_adapter("Terminal commands"));
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
        return Err(macos_adapter("Editor commands"));
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
    let Target::Linux { paths, id, .. } = target else {
        return prepare_macos(target, params);
    };
    let mut request = params.clone();
    request["computerId"] = json!(id);
    Ok(crate::remote_access::authorize_guest(paths, &request)?)
}

/// Appends one client key to the working account's `authorized_keys`, keeping the file
/// private and never adding a key twice. The key is the script's only argument, so it stays
/// data. `$HOME` is the account's home (`/Users/silo`).
const AUTHORIZE_KEY: &str = r#"set -e
umask 077
directory="$HOME/.ssh"
file="$directory/authorized_keys"
/bin/mkdir -p "$directory"
/bin/chmod 700 "$directory"
/usr/bin/touch "$file"
/bin/chmod 600 "$file"
if ! /usr/bin/grep -qxF -- "$1" "$file"; then
  if [ -s "$file" ] && [ -n "$(/usr/bin/tail -c 1 "$file")" ]; then
    printf '\n' >> "$file"
  fi
  printf '%s\n' "$1" >> "$file"
fi
"#;

/// The guest's Ed25519 host key, which a controller pins. macOS generates it at first boot.
const HOST_KEY_FILE: &str = "/etc/ssh/ssh_host_ed25519_key.pub";

fn authorize_key_argv(public_key: &str) -> Vec<String> {
    vec![
        "/bin/sh".into(),
        "-c".into(),
        AUTHORIZE_KEY.into(),
        "silo-authorize".into(),
        public_key.into(),
    ]
}

/// The `ssh-ed25519 <key>` part of a host key file, without its comment.
fn host_key_line(file: &[u8]) -> Result<String, GuestError> {
    let invalid = || GuestError::internal("The computer returned an invalid SSH host key.");
    let text = std::str::from_utf8(file).map_err(|_| invalid())?;
    let mut fields = text.split_whitespace();
    let (kind, value) = (fields.next(), fields.next());
    match (kind, value) {
        (Some("ssh-ed25519"), Some(value)) if value.len() <= 256 => {
            Ok(format!("ssh-ed25519 {value}"))
        }
        _ => Err(invalid()),
    }
}

fn run_in_guest(target: &Target, argv: Vec<String>) -> Result<ExecOutput, GuestError> {
    run_bounded(
        target,
        &ExecRequest {
            argv,
            cwd: None,
            env: Vec::new(),
            user: GuestUser::Account,
            stdin: None,
            timeout: Duration::from_secs(10),
            max_stdout: 8 * 1024,
        },
    )
}

/// `guest.prepare` for a macOS computer: authorizes the controller's key for the working
/// account through the guest's SSH and returns the guest's host key and account. The working
/// account of a macOS computer is fixed (`silo`), so `accountProtocol` has nothing to check.
fn prepare_macos(target: &Target, params: &Value) -> Result<Value, GuestError> {
    let Target::Macos(shell) = target else {
        return Err(GuestError::internal("Not a macOS computer."));
    };
    let text = |name: &str| {
        params[name]
            .as_str()
            .ok_or_else(|| GuestError::internal(format!("Invalid params: {name} is required.")))
    };
    let (public_key, path) = (text("publicKey")?, text("path")?);
    crate::runtime::shutdown::ensure_accepting_operations()?;
    crate::editor::validate_public_key(public_key)?;
    crate::editor::validate_macos_path(path)?;
    target.ensure_running()?;
    let folder = run_in_guest(target, vec!["test".into(), "-d".into(), path.into()])?;
    if folder.exit_code != Some(0) {
        return Err(GuestError::not_found(
            "This folder is unavailable inside the computer.",
        ));
    }
    let authorized = run_in_guest(target, authorize_key_argv(public_key))?;
    if authorized.exit_code != Some(0) {
        return Err(GuestError::internal(
            "Silo could not authorize this key inside the computer.",
        ));
    }
    let host = run_in_guest(target, vec!["/bin/cat".into(), HOST_KEY_FILE.into()])?;
    if host.exit_code != Some(0) {
        return Err(GuestError::internal(
            "Silo could not read the computer's SSH host key.",
        ));
    }
    let guest_address = if params["forwarding"].as_bool() == Some(true) {
        Some(shell.address()?.to_string())
    } else {
        None
    };
    Ok(json!({
        "hostPublicKey": host_key_line(&host.stdout)?,
        "user": shell.user(),
        "guestAddress": guest_address,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guest::test_support;
    use silo_api::jobs::ErrorCode;

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
    fn macos_computers_are_built_by_the_macos_adapters_not_the_linux_paths() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::macos_target(directory.path());
        let home = directory.path().join("user");
        let settings = serde_json::Map::new();
        let errors = [
            terminal_command(&target).unwrap_err(),
            editor_command(&target, &home, None, &settings).unwrap_err(),
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
            assert_eq!(error.code, ErrorCode::Internal);
            assert!(error.message.contains("guest::macos"), "{}", error.message);
        }
    }

    const KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOchaFYV1PlX7OWxqxGXrnZTgG+EqFVeLNIJn42NJJxA";

    fn authorize(home: &std::path::Path, key: &str) -> std::process::ExitStatus {
        let argv = authorize_key_argv(key);
        std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .env("HOME", home)
            .status()
            .unwrap()
    }

    #[test]
    fn authorizing_a_key_appends_it_once_and_keeps_the_files_private() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join(".ssh/authorized_keys");
        // An existing file without a final newline keeps its line.
        std::fs::create_dir_all(home.path().join(".ssh")).unwrap();
        std::fs::write(&file, "ssh-ed25519 AAAAexisting other").unwrap();
        assert!(authorize(home.path(), KEY).success());
        assert!(authorize(home.path(), KEY).success());
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            format!("ssh-ed25519 AAAAexisting other\n{KEY}\n")
        );
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(&home.path().join(".ssh")), 0o700);
    }

    #[test]
    fn a_key_with_shell_syntax_is_data_not_source() {
        let home = tempfile::tempdir().unwrap();
        let hostile = "ssh-ed25519 $(touch pwned); `touch pwned`";
        assert!(authorize(home.path(), hostile).success());
        assert!(!home.path().join("pwned").exists());
        assert_eq!(
            std::fs::read_to_string(home.path().join(".ssh/authorized_keys")).unwrap(),
            format!("{hostile}\n")
        );
    }

    #[test]
    fn the_host_key_line_drops_the_comment_and_refuses_other_key_types() {
        assert_eq!(
            host_key_line(b"ssh-ed25519 AAAAhost root@silo\n").unwrap(),
            "ssh-ed25519 AAAAhost"
        );
        for bad in [&b""[..], b"ssh-rsa AAAA x", b"ssh-ed25519", b"\xff\xfe"] {
            assert_eq!(host_key_line(bad).unwrap_err().code, ErrorCode::Internal);
        }
    }

    #[test]
    fn preparing_a_macos_computer_checks_the_key_and_folder_before_touching_the_guest() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::macos_target(directory.path());
        let call = |key: &str, path: &str| {
            prepare(
                &target,
                &json!({"computerId": target.id(), "publicKey": key, "path": path,
                        "accountProtocol": 1}),
            )
            .unwrap_err()
        };
        assert_eq!(
            call("ssh-rsa AAAA", "/Users/silo/workspace").message,
            "Expected an Ed25519 public key."
        );
        assert!(call(KEY, "/workspace")
            .message
            .contains("Choose a folder inside /Users/silo/workspace"));
        // A valid request reaches the guest, which has no access material here.
        assert_eq!(
            call(KEY, "/Users/silo/workspace").message,
            "This computer's access is missing."
        );
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
