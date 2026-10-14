use crate::{applications, editor, remote_access, runtime};
use std::path::Path;
use tauri::{AppHandle, Manager, WebviewWindow};

/// Open a computer in the editor selected in this device's application settings.
pub(crate) fn open(app: &AppHandle, name: &str, path: Option<&str>) -> Result<(), String> {
    let path = path.unwrap_or("/workspace");
    if let Some((device, computer)) = remote_access::target(name)? {
        let (alias, _) = editor::prepare_remote(&device, &computer, path)?;
        let application = applications::selected_editor(app)?;
        let command = applications::editor_command(&application)?;
        let home = app
            .path()
            .home_dir()
            .map_err(|_| "Could not prepare the editor connection.")?;
        return editor::launch_prepared(&command, &alias, path, &home);
    }

    editor::require_openssh("open computer folders in your editor")?;
    runtime::validate_name(name).map_err(|error| error.to_string())?;
    editor::validate_path(path)?;
    let application = applications::selected_editor(app)?;
    let command = applications::editor_command(&application)?;
    let paths = runtime::runtime_paths(app)?;
    let home = app
        .path()
        .home_dir()
        .map_err(|_| "Could not prepare the editor connection.")?;
    editor::open_local(&paths, &home, name, path, &command)
}

/// Runs the bundled `msb ssh serve` in this AppImage's mount for an editor.
pub(crate) fn run_transport(args: &[String]) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let (home, name) = editor::transport_args(args)?;
    let home = Path::new(home);
    let executable =
        std::env::current_exe().map_err(|_| "Could not prepare the editor connection.")?;
    let bundle = tauri::utils::platform::bundle_type();
    let appimage = std::env::var_os("APPDIR").map(std::path::PathBuf::from);
    let msb = crate::bundled_tools::resolve(
        &executable,
        Path::new(""),
        bundle.clone(),
        appimage.as_deref(),
    )?
    .join("msb");
    let library = runtime::bundled_runtime_library(&msb, Path::new(""), bundle);
    let error = std::process::Command::new(&msb)
        .args([
            "ssh",
            "serve",
            name,
            "--stdio",
            "--no-start",
            "--no-inactivity-timeout",
        ])
        .env("MSB_HOME", home)
        .env("MSB_PATH", &msb)
        .env("MSB_LIBKRUNFW_PATH", &library)
        .exec();
    Err(format!("Could not start the computer connection: {error}"))
}

#[tauri::command]
pub(crate) async fn read_editor_include_notice(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<Option<String>, String> {
    if window.label() != "main" {
        return Err("Only the main window can read the editor connection notice.".into());
    }
    let home = app
        .path()
        .home_dir()
        .map_err(|_| "Could not prepare the editor connection.")?;
    tokio::task::spawn_blocking(move || editor::read_editor_include_notice(&home))
        .await
        .map_err(|_| "Could not prepare the editor connection.".to_owned())?
}

/// Preserve the startup repair and AppImage refresh as background work.
pub(crate) fn refresh_transports(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let path = app.path();
        let Ok(home) = path.home_dir() else {
            return;
        };
        let app_data = path.app_data_dir().unwrap_or_default();
        let paths = runtime::runtime_paths(&app).ok();
        let failures = editor::refresh_transports(paths.as_ref(), &app_data, &home);
        if !failures.is_empty() {
            crate::notifications::notify(
                &app,
                crate::notifications::failure(
                    "startup:editor-connections",
                    "Editor connections need attention",
                    &failures.join(" "),
                    None,
                ),
            );
        }
    });
}
