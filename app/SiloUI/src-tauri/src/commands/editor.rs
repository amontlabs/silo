use crate::engine_client::{Engine, EngineClient, EngineError};
use crate::{applications, editor, remote_access, runtime};
use serde_json::json;
use std::path::Path;
use tauri::{AppHandle, Manager, WebviewWindow};

/// Open a computer in the editor selected in this device's application settings.
pub(crate) fn open(app: &AppHandle, name: &str, path: Option<&str>) -> Result<(), String> {
    let path = path.unwrap_or("/workspace");
    if let Some((device, computer)) = remote_access::target(name)? {
        let (alias, _) = editor::prepare_remote(&device, &computer, path)?;
        let application = crate::commands::applications::selected_editor(app)?;
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
    let application = crate::commands::applications::selected_editor(app)?;
    let command = applications::editor_command(&application)?;
    let paths = runtime::runtime_paths(&crate::commands::runtime::host(app))?;
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
        crate::commands::bundled_tools::bundle_kind(bundle.clone()),
        appimage.as_deref(),
    )?
    .join("msb");
    let library = runtime::bundled_runtime_library(
        &msb,
        Path::new(""),
        crate::commands::runtime::package_kind(bundle),
    );
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

/// The line this device's SSH configuration is missing for editors, if any.
pub(crate) async fn include_notice(engine: &EngineClient) -> Result<Option<String>, EngineError> {
    let notice = engine.call("editor.includeNotice", json!({})).await?;
    Ok(notice.as_str().map(str::to_owned))
}

#[tauri::command]
pub(crate) async fn read_editor_include_notice(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<Option<String>, EngineError> {
    if window.label() != "main" {
        return Err(EngineError::internal(
            "Only the main window can read the editor connection notice.",
        ));
    }
    include_notice(&engine).await
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::Value;

    #[tokio::test]
    async fn the_include_notice_is_the_engines_text_or_nothing() {
        let notice =
            std::sync::Arc::new(std::sync::Mutex::new(json!("Add Include to ~/.ssh/config")));
        let current = notice.clone();
        let fake = FakeEngine::start(move |method, _| match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "editor.includeNotice" => Ok(current.lock().unwrap().clone()),
            "engine.state" => Ok(Value::Null),
            _ => Ok(Value::Null),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            include_notice(&engine).await.unwrap().as_deref(),
            Some("Add Include to ~/.ssh/config")
        );
        *notice.lock().unwrap() = Value::Null;
        assert_eq!(include_notice(&engine).await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_unavailable_engine_method_is_typed() {
        let fake = FakeEngine::start(|method, _| match method {
            "editor.includeNotice" => Err(("unsupported_remote_operation", "Not here.".into())),
            _ => Ok(json!({"subscription": "sub"})),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            include_notice(&engine).await.unwrap_err().code,
            "unsupported_remote_operation"
        );
    }
}
