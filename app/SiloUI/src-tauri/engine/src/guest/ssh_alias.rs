//! SSH aliases for external clients (`guest.sshAlias.ensure/remove`), keyed by `computerId`.
//!
//! The alias, its `*.conf` and its pinned host key are named after the computer's id, so
//! renaming a computer never breaks a saved connection. Aliases are written on the engine's
//! own device; with a `deviceId` the controller's engine writes its own files and asks the
//! owner only to authorize its key (`guest.prepare`).
use super::{
    exec::{run_bounded, ExecRequest, GuestUser},
    target::Target,
    GuestError,
};
use std::{path::Path, time::Duration};

/// What `guest.sshAlias.ensure` asks for.
#[derive(Clone, Debug, Default)]
pub struct AliasEnsure {
    /// Make the alias visible in `~/.ssh/config`. Defaults to true at the wire.
    pub install_include: bool,
    /// The Linux folder the client will open; `/workspace` when omitted.
    pub path: Option<String>,
}

/// The alias and where it lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AliasOutcome {
    pub alias: String,
    pub user: String,
    pub home: String,
    pub config_path: String,
    pub include_line: String,
    pub include_installed: bool,
}

const MACOS_ADAPTER: &str = "SSH aliases for macOS computers are written by guest::macos.";

fn linux<'a>(
    target: &'a Target,
) -> Result<(&'a crate::runtime::RuntimePaths, &'a str, &'a str), GuestError> {
    match target {
        Target::Linux { paths, id, name } => Ok((paths, id, name)),
        Target::Macos(_) => Err(GuestError::internal(MACOS_ADAPTER)),
    }
}

/// Writes the computer's alias and returns what a client needs to use it.
///
/// Refuses before any effect when the folder is invalid, OpenSSH is missing or the computer is
/// not running; verifies the folder exists in the guest and that the alias connects without
/// prompting. A requested `Include` that cannot be installed fails with
/// `include_not_writable` and the exact line to add.
pub fn ensure_alias(
    target: &Target,
    user_home: &Path,
    request: &AliasEnsure,
) -> Result<AliasOutcome, GuestError> {
    ensure_alias_probing(target, user_home, request, &|config, alias| {
        crate::editor::probe_alias(config, alias)
    })
}

/// `ensure_alias` with the final connection check supplied by the caller.
pub(crate) fn ensure_alias_probing(
    target: &Target,
    user_home: &Path,
    request: &AliasEnsure,
    probe_connection: &dyn Fn(&Path, &str) -> Result<(), String>,
) -> Result<AliasOutcome, GuestError> {
    let (paths, id, name) = linux(target)?;
    crate::editor::require_openssh("connect to computers over SSH")?;
    let path = request.path.as_deref().unwrap_or("/workspace");
    crate::editor::validate_path(path)?;
    target.ensure_running()?;
    let probe = run_bounded(
        target,
        &ExecRequest {
            argv: vec!["test".into(), "-d".into(), path.into()],
            cwd: None,
            env: Vec::new(),
            user: GuestUser::Account,
            stdin: None,
            timeout: Duration::from_secs(5),
            max_stdout: 1024,
        },
    );
    if !matches!(probe, Ok(ref output) if output.exit_code == Some(0)) {
        return Err(GuestError::not_found(
            "This folder is unavailable inside the computer.",
        ));
    }
    let account = target.account()?;
    let written =
        crate::editor::ensure_id_alias(paths, user_home, name, id, request.install_include)?;
    probe_connection(&written.config, &written.alias)?;
    Ok(AliasOutcome {
        alias: written.alias,
        user: account.user,
        home: account.home,
        config_path: written.config.to_string_lossy().into_owned(),
        include_line: written.include,
        include_installed: written.include_installed,
    })
}

/// Removes the computer's alias files, and the shared `Include` once no alias is left.
/// Idempotent; it never revokes guest login keys or touches another computer's alias.
pub fn remove_alias(target: &Target, user_home: &Path) -> Result<(), GuestError> {
    let (paths, id, _) = linux(target)?;
    Ok(crate::editor::remove_id_alias(paths, user_home, id)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guest::test_support;
    use silo_api::jobs::ErrorCode;

    #[test]
    fn invalid_folders_and_missing_folders_are_refused_before_anything_is_written() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let home = directory.path().join("user");
        std::fs::create_dir_all(&home).unwrap();
        for path in [
            "relative",
            "/etc",
            "/workspace/../etc",
            "/workspace/a\u{1}b",
        ] {
            let error = ensure_alias(
                &target,
                &home,
                &AliasEnsure {
                    install_include: true,
                    path: Some(path.into()),
                },
            )
            .unwrap_err();
            assert!(
                error.message.contains("Choose a folder inside /workspace"),
                "{path}"
            );
        }
        // The folder check runs in the guest; the fake guest is this machine, which has none.
        let error = ensure_alias(
            &target,
            &home,
            &AliasEnsure {
                install_include: true,
                path: Some("/workspace/does-not-exist-here".into()),
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(
            error.message,
            "This folder is unavailable inside the computer."
        );
        assert!(
            !home.join(".ssh").exists(),
            "no SSH configuration may be touched"
        );
    }

    #[test]
    fn an_alias_reports_the_account_the_include_and_where_the_entry_lives() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let home = directory.path().join("user");
        std::fs::create_dir_all(&home).unwrap();
        let probed = std::cell::RefCell::new(Vec::new());
        let ensure = |install_include: bool| {
            ensure_alias_probing(
                &target,
                &home,
                &AliasEnsure {
                    install_include,
                    path: Some("/workspace/project".into()),
                },
                &|config, alias| {
                    probed
                        .borrow_mut()
                        .push((config.to_owned(), alias.to_owned()));
                    Ok(())
                },
            )
            .unwrap()
        };
        let skipped = ensure(false);
        assert!(!skipped.include_installed);
        assert!(!home.join(".ssh/config").exists());
        let outcome = ensure(true);
        assert_eq!(outcome.alias, format!("silo-{}", test_support::COMPUTER_ID));
        assert_eq!(outcome.user, "silo");
        assert!(outcome.home.starts_with('/'));
        assert!(outcome.include_installed);
        assert!(outcome
            .config_path
            .ends_with(&format!("{}.conf", test_support::COMPUTER_ID)));
        assert!(std::fs::read_to_string(home.join(".ssh/config"))
            .unwrap()
            .contains(&outcome.include_line));
        assert_eq!(probed.borrow().len(), 2);
        // A failing connection check is an error, and the entry stays for the next try.
        let error = ensure_alias_probing(
            &target,
            &home,
            &AliasEnsure {
                install_include: true,
                path: None,
            },
            &|_, _| Err("Could not connect to this computer over SSH.".into()),
        )
        .unwrap_err();
        assert!(error.message.starts_with("Could not connect"));
        remove_alias(&target, &home).unwrap();
        assert!(!std::path::Path::new(&outcome.config_path).exists());
        assert_eq!(
            std::fs::read_to_string(home.join(".ssh/config")).unwrap(),
            ""
        );
    }

    #[test]
    fn removal_is_idempotent_and_a_computer_without_aliases_is_fine() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let home = directory.path().join("user");
        std::fs::create_dir_all(&home).unwrap();
        remove_alias(&target, &home).unwrap();
        remove_alias(&target, &home).unwrap();
    }
}
