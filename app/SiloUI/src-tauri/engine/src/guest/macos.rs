//! The macOS side of the alias, terminal and editor methods. These go through the macOS
//! access adapters (`macos_computers::ssh_alias`), which write ID-keyed aliases that reach the
//! guest through `silod --remote-guest`, so they need the engine's [`MacosContext`] for its
//! paths and the live inventory. The adapters report plain messages; this module gives them the
//! stable wire codes.
use super::{
    ssh_alias::{AliasEnsure, AliasOutcome},
    target::Target,
    GuestError,
};
use crate::macos_computers::{self, MacosContext};
use serde_json::Value;

fn computer<'a>(target: &'a Target, what: &str) -> Result<&'a str, GuestError> {
    match target {
        Target::Macos(shell) => Ok(shell.id()),
        Target::Linux { .. } => Err(GuestError::internal(format!(
            "{what} for Linux computers does not use the macOS adapters."
        ))),
    }
}

/// `guest.sshAlias.ensure` for a macOS computer: writes `<app data>/ssh/macos/<id>.conf`,
/// probes the connection and, when asked, installs the shared `Include`.
pub fn ensure_alias(
    context: &MacosContext,
    target: &Target,
    request: &AliasEnsure,
) -> Result<AliasOutcome, GuestError> {
    let id = computer(target, "SSH aliases")?;
    let result = macos_computers::ensure_ssh_alias(
        context,
        id,
        request.path.as_deref(),
        Some(request.install_include),
    )?;
    Ok(AliasOutcome {
        alias: result.alias,
        user: result.user,
        home: result.home,
        config_path: result.config_path,
        include_line: result.include_line,
        include_installed: result.include_installed,
    })
}

/// `guest.sshAlias.remove` for a macOS computer. Idempotent.
pub fn remove_alias(context: &MacosContext, target: &Target) -> Result<(), GuestError> {
    let id = computer(target, "SSH aliases")?;
    Ok(macos_computers::remove_ssh_alias(context, id)?)
}

/// `terminal.command` for a macOS computer: `ssh -F <alias config> -t <alias>`.
pub fn terminal_command(
    context: &MacosContext,
    target: &Target,
) -> Result<Vec<String>, GuestError> {
    let id = computer(target, "Terminals")?;
    Ok(macos_computers::terminal_command(context, id)?.argv)
}

/// `editor.command` for a macOS computer: prepares the alias and returns the argv that opens
/// `path` (default the workspace) in the editor chosen in `settings`.
pub fn editor_command(
    context: &MacosContext,
    target: &Target,
    path: Option<&str>,
    settings: &serde_json::Map<String, Value>,
) -> Result<Vec<String>, GuestError> {
    let id = computer(target, "Editors")?;
    if let Some(path) = path {
        crate::editor::validate_macos_path(path)?;
    }
    let application = crate::applications::selected_editor_from_settings(settings)?;
    let command = crate::applications::editor_command(&application)?;
    Ok(macos_computers::editor_command(context, id, path, &command)?.argv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guest::test_support;
    use silo_api::jobs::ErrorCode;

    struct Host;
    impl macos_computers::MacosHost for Host {
        fn show_display(&self, _: &MacosContext, _: &str, _: &str, _: bool) -> Result<(), String> {
            Err("no display in tests".into())
        }
        fn close_display(&self, _: &str) {}
        fn linux_computer_names(&self) -> Vec<String> {
            Vec::new()
        }
    }

    fn context(directory: &std::path::Path) -> MacosContext {
        macos_computers::MacosContext::new(
            test_support::RecordingContext::new(directory),
            std::sync::Arc::new(Host),
        )
    }

    #[test]
    fn linux_targets_are_not_served_by_the_macos_adapters() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let context = context(directory.path());
        let error = terminal_command(&context, &target).unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(error.message.contains("does not use the macOS adapters"));
        assert!(remove_alias(&context, &target).is_err());
    }

    #[test]
    fn a_macos_computer_the_inventory_does_not_hold_is_reported_as_gone() {
        // The target was found on disk, but this engine's live inventory has no such computer
        // (it was never loaded here), so the adapters answer with their own words.
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::macos_target(directory.path());
        let context = context(directory.path());
        let error = terminal_command(&context, &target).unwrap_err();
        assert!(
            !error.message.contains("pending parity-2"),
            "{}",
            error.message
        );
    }

    #[test]
    fn editor_paths_outside_the_macos_workspace_are_refused_before_anything_is_written() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::macos_target(directory.path());
        let context = context(directory.path());
        let settings = serde_json::Map::new();
        for path in ["/workspace", "/etc", "/Users/silo/workspace/../x"] {
            let error = editor_command(&context, &target, Some(path), &settings).unwrap_err();
            assert!(
                error
                    .message
                    .contains("Choose a folder inside /Users/silo/workspace"),
                "{path}: {}",
                error.message
            );
        }
        assert!(!directory.path().join("app-data/ssh").exists());
    }
}
