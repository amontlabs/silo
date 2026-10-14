use crate::{editor, remote_access, runtime, terminal};
use tauri::AppHandle;

/// Open the selected terminal against an already running local or remote computer.
pub(crate) fn open(app: &AppHandle, name: &str) -> Result<(), String> {
    if let Some((device, computer)) = remote_access::target(name)? {
        // ssh gets `-F`, so the user's ~/.ssh/config needs no Include (G-11).
        let (alias, config) = editor::prepare_remote_private(&device, &computer, "/workspace")?;
        let application = crate::commands::applications::selected_terminal(app)?;
        let command = [
            "/usr/bin/ssh",
            "-F",
            config.to_str().ok_or("Invalid SSH configuration path.")?,
            "-t",
            &alias,
        ]
        .iter()
        .map(|argument| terminal::quote(argument))
        .collect::<Vec<_>>()
        .join(" ");
        return crate::commands::applications::open_terminal(app, &application, &command);
    }

    runtime::validate_name(name).map_err(|error| error.to_string())?;
    let application = crate::commands::applications::selected_terminal(app)?;
    let paths = runtime::runtime_paths(&crate::commands::runtime::host(app))?;
    let metadata = runtime::read_metadata(&paths.metadata).map_err(|error| error.to_string())?;
    if !metadata
        .computers
        .iter()
        .any(|configuration| configuration.name() == name)
    {
        return Err("Choose a local Silo computer.".into());
    }
    terminal::running_computer(&paths, name)?;
    crate::commands::applications::open_terminal(
        app,
        &application,
        &terminal::command(&paths, name)?,
    )
}
