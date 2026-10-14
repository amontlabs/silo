//! Launching host applications from a command the engine built: the terminal and editor the
//! user chose. The engine decides *what* runs (`terminal.command`, `editor.command`); this
//! client decides nothing about computers and only starts the program.

use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use serde_json::{Map, Value};

use crate::applications;

const EDITOR_EXIT_WINDOW: Duration = Duration::from_secs(10);
const FAILED_EDITOR: &str =
    "The editor could not be opened. Check its installation and Remote SSH support.";

/// Opens `argv` (an ssh session built by the engine) in the terminal chosen in `settings`.
pub(crate) fn open_terminal(settings: &Map<String, Value>, argv: &[String]) -> Result<(), String> {
    if argv.is_empty() {
        return Err("The engine returned no terminal command.".into());
    }
    let application = applications::selected_terminal_from_settings(settings)?;
    let command = argv
        .iter()
        .map(|argument| crate::terminal::quote(argument))
        .collect::<Vec<_>>()
        .join(" ");
    applications::open_terminal_with_application(&application, &command)
}

/// Starts the editor the engine prepared, and reports a launch that exits at once with failure.
pub(crate) fn launch_editor(argv: &[String]) -> Result<(), String> {
    launch_editor_within(argv, EDITOR_EXIT_WINDOW)
}

fn launch_editor_within(argv: &[String], window: Duration) -> Result<(), String> {
    let (program, arguments) = argv
        .split_first()
        .ok_or_else(|| "The engine returned no editor command.".to_owned())?;
    let mut launch = Command::new(program);
    launch.args(arguments);
    let mut child = applications::launch::sanitize_child(&mut launch)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| FAILED_EDITOR.to_owned())?;
    let deadline = Instant::now() + window;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) | Err(_) => return Err(FAILED_EDITOR.to_owned()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_editor_that_exits_successfully_or_keeps_running_is_launched() {
        assert!(launch_editor_within(&["/usr/bin/true".into()], Duration::from_secs(2)).is_ok());
        assert!(launch_editor_within(
            &["/bin/sleep".into(), "2".into()],
            Duration::from_millis(100)
        )
        .is_ok());
    }

    #[test]
    fn a_failing_or_missing_editor_is_an_error() {
        assert_eq!(
            launch_editor_within(&["/usr/bin/false".into()], Duration::from_secs(2)),
            Err(FAILED_EDITOR.to_owned())
        );
        assert!(launch_editor_within(&["/no/such/editor".into()], Duration::from_secs(1)).is_err());
        assert!(launch_editor_within(&[], Duration::from_secs(1)).is_err());
    }
}
