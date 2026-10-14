use std::collections::BTreeMap;

use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::applications::{self, Application, ApplicationCatalog, ApplicationKind};

/// Opens `url` in the browser chosen in the settings (the engine's, read through the client).
pub(crate) fn open_browser(app: &AppHandle, url: &str) -> Result<(), String> {
    let url = applications::validated_browser_url(url)?;
    let settings = crate::engine_client::of(app).cached_settings();
    applications::open_browser_with_settings(&settings, &url)
}

#[tauri::command]
pub(crate) async fn list_applications(
    app: AppHandle,
    window: WebviewWindow,
    selections: BTreeMap<String, String>,
) -> Result<ApplicationCatalog, String> {
    if !matches!(window.label(), "main" | "status") {
        return Err("This window cannot discover applications".into());
    }
    tokio::task::spawn_blocking(move || {
        let catalog = applications::discover_with_selections(selections)?;
        #[cfg(target_os = "linux")]
        let catalog = {
            let mut catalog = catalog;
            let (send, receive) = std::sync::mpsc::sync_channel(1);
            app.run_on_main_thread(move || {
                applications::load_icons(&mut catalog);
                let _ = send.send(catalog);
            })
            .map_err(|error| error.to_string())?;
            receive.recv().map_err(|error| error.to_string())?
        };
        #[cfg(not(target_os = "linux"))]
        let _ = app;
        Ok(catalog)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn choose_application(
    app: AppHandle,
    window: WebviewWindow,
    kind: ApplicationKind,
) -> Result<Option<Application>, String> {
    if window.label() != "main" {
        return Err("Only the main window can choose applications".into());
    }
    tokio::task::spawn_blocking(move || {
        let title = match kind {
            ApplicationKind::Terminal => "Choose a terminal",
            ApplicationKind::Editor => "Choose a code editor",
            ApplicationKind::Browser => "Choose a browser",
        };
        let dialog = app.dialog().file().set_title(title).set_parent(&window);
        #[cfg(target_os = "macos")]
        let dialog = dialog
            .set_directory("/Applications")
            .add_filter("Applications", &["app"]);
        #[cfg(target_os = "linux")]
        let dialog = {
            let dialog = dialog.set_directory("/usr/share/applications");
            if matches!(kind, ApplicationKind::Browser) {
                dialog.add_filter("Desktop entries", &["desktop"])
            } else {
                dialog
            }
        };
        let Some(file) = dialog.blocking_pick_file() else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|error| error.to_string())?;
        let application = applications::chosen_application(&path, kind)?;
        #[cfg(target_os = "linux")]
        let application = {
            let (send, receive) = std::sync::mpsc::sync_channel(1);
            app.run_on_main_thread(move || {
                let mut application = application;
                applications::load_icon(&mut application);
                let _ = send.send(application);
            })
            .map_err(|error| error.to_string())?;
            receive.recv().map_err(|error| error.to_string())?
        };
        Ok(Some(application))
    })
    .await
    .map_err(|error| error.to_string())?
}
