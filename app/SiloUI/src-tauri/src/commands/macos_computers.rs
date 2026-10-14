//! Tauri command and host-window adapters for macOS computers.
use crate::{macos_computers_engine as engine, runtime};
pub(crate) use engine::{Action, CreateRequest};
use engine::{MacosContext, MacosHost};
use std::{path::PathBuf, sync::Arc, time::Instant};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};

struct TauriMacosHost {
    app: AppHandle,
}

impl MacosHost for TauriMacosHost {
    fn show_display(
        &self,
        context: &MacosContext,
        id: &str,
        title: &str,
        toolbar: bool,
    ) -> Result<(), String> {
        let label = engine::display_label(id);
        if let Some(existing) = self.app.get_webview_window(&label) {
            let _ = existing.show();
            return existing
                .set_focus()
                .map_err(|_| "Could not focus the display.".to_string());
        }

        // The blank page has no matching capability; the VM view covers its content.
        let blank = tauri::Url::parse("about:blank").map_err(|error| error.to_string())?;
        let display = WebviewWindowBuilder::new(&self.app, &label, WebviewUrl::External(blank))
            .title(title)
            .inner_size(1440., 900.)
            .min_inner_size(640., 400.)
            .resizable(true)
            .build()
            .map_err(|_| "Silo could not open the display.".to_string())?;

        let (close_context, closed_id) = (context.clone(), id.to_string());
        display.on_window_event(move |event| {
            if matches!(event, WindowEvent::Destroyed) {
                engine::display_window_closed(&close_context, &closed_id);
            }
        });

        let native_window = display
            .ns_window()
            .map_err(|_| "Silo could not open the display.".to_string())?
            as usize;
        if let Err(message) = engine::attach_display(&context, id, native_window, toolbar) {
            let _ = display.destroy();
            return Err(message);
        }
        Ok(())
    }

    fn close_display(&self, id: &str) {
        if let Some(display) = self.app.get_webview_window(&engine::display_label(id)) {
            // Window destruction waits for the main thread, which may be the caller.
            std::thread::spawn(move || {
                let _ = display.destroy();
            });
        }
    }

    fn linux_computer_names(&self) -> Vec<String> {
        runtime::computer_names(&crate::commands::runtime::host(&self.app))
    }
}

/// Builds the per-operation context used by the macOS computer engine.
pub(crate) fn new_context(app: &AppHandle) -> Result<MacosContext, String> {
    let engine = silo_engine::context::current()
        .ok_or_else(|| "The engine context is not installed.".to_string())?;
    Ok(MacosContext::new(
        engine,
        Arc::new(TauriMacosHost { app: app.clone() }),
    ))
}

fn require_main(window: &Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Manage macOS computers from the main Silo window.".into())
    }
}

#[tauri::command]
pub(crate) async fn read_macos_computers(
    app: AppHandle,
    window: Window,
) -> Result<engine::MacosComputersState, String> {
    require_main(&window)?;
    engine::read_macos_computers(new_context(&app)?).await
}

#[tauri::command]
pub(crate) async fn delete_macos_template(app: AppHandle, window: Window) -> Result<(), String> {
    require_main(&window)?;
    engine::delete_macos_template(new_context(&app)?).await
}

#[tauri::command]
pub(crate) async fn create_macos_computer(
    app: AppHandle,
    window: Window,
    request: CreateRequest,
) -> Result<engine::MacosComputer, String> {
    require_main(&window)?;
    engine::create_macos_computer(new_context(&app)?, request).await
}

#[tauri::command]
pub(crate) async fn macos_computer_action(
    app: AppHandle,
    window: Window,
    id: String,
    action: Action,
) -> Result<(), String> {
    require_main(&window)?;
    engine::macos_computer_action(new_context(&app)?, id, action).await
}

#[tauri::command]
pub(crate) async fn open_macos_display(
    app: AppHandle,
    window: Window,
    id: String,
) -> Result<(), String> {
    require_main(&window)?;
    engine::open_macos_display(new_context(&app)?, id).await
}

#[tauri::command]
pub(crate) async fn create_macos_checkpoint(
    app: AppHandle,
    window: Window,
    id: String,
    name: String,
) -> Result<(), String> {
    require_main(&window)?;
    engine::create_macos_checkpoint(new_context(&app)?, id, name).await
}

#[tauri::command]
pub(crate) async fn restore_macos_checkpoint(
    app: AppHandle,
    window: Window,
    id: String,
    checkpoint_id: String,
) -> Result<(), String> {
    require_main(&window)?;
    engine::restore_macos_checkpoint(new_context(&app)?, id, checkpoint_id).await
}

#[tauri::command]
pub(crate) async fn fork_macos_checkpoint(
    app: AppHandle,
    window: Window,
    id: String,
    checkpoint_id: String,
    new_name: String,
) -> Result<(), String> {
    require_main(&window)?;
    engine::fork_macos_checkpoint(new_context(&app)?, id, checkpoint_id, new_name).await
}

#[tauri::command]
pub(crate) async fn delete_macos_checkpoint(
    app: AppHandle,
    window: Window,
    id: String,
    checkpoint_id: String,
) -> Result<(), String> {
    require_main(&window)?;
    engine::delete_macos_checkpoint(new_context(&app)?, id, checkpoint_id).await
}

#[tauri::command]
pub(crate) async fn macos_computer_clipboard(
    app: AppHandle,
    window: Window,
    id: String,
    direction: engine::ClipboardDirection,
) -> Result<crate::viewer_clipboard::Report, String> {
    require_main(&window)?;
    engine::macos_computer_clipboard(new_context(&app)?, id, direction).await
}

// Compatibility adapters for call sites owned by the runtime, logs, and remote tracks.
pub(crate) use engine::{close_for_update, reopen_after_update};

pub(crate) fn stop_all(app: &AppHandle, deadline: Option<Instant>) -> Result<(), String> {
    engine::stop_all(&new_context(app)?, deadline)
}

pub(crate) fn names(app: &AppHandle) -> Vec<String> {
    new_context(app).map_or_else(|_| Vec::new(), |context| engine::names(&context))
}

pub(crate) fn log_target(app: &AppHandle, id: &str) -> Option<(String, PathBuf)> {
    engine::log_target(&new_context(app).ok()?, id)
}
