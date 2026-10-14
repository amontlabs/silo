//! Tauri command and host-window adapters for macOS computers.
use crate::{macos_computers_engine as engine, runtime};
pub(crate) use engine::{Action, CreateRequest};
use engine::{MacosContext, MacosHost};
use std::{path::PathBuf, sync::Arc, time::Instant};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};

const DISPLAY_WINDOW_ERROR: &str = "Silo could not open the display.";

fn confirmed_display_window(original: usize, current: usize) -> Result<usize, String> {
    if original != 0 && original == current {
        Ok(current)
    } else {
        Err(DISPLAY_WINDOW_ERROR.to_string())
    }
}

type NativeWindowLookup = Box<dyn FnOnce() -> Result<usize, String>>;

fn resolve_display_window(
    label: &str,
    created_label: &str,
    created_window: impl FnOnce() -> Result<usize, String>,
    lookup_current: impl FnOnce(&str) -> Option<(String, NativeWindowLookup)>,
) -> Result<usize, String> {
    let (current_label, current_window) =
        lookup_current(label).ok_or_else(|| DISPLAY_WINDOW_ERROR.to_string())?;
    if created_label != label || current_label != label {
        return Err(DISPLAY_WINDOW_ERROR.to_string());
    }
    let original = created_window().map_err(|_| DISPLAY_WINDOW_ERROR.to_string())?;
    let current = current_window().map_err(|_| DISPLAY_WINDOW_ERROR.to_string())?;
    confirmed_display_window(original, current)
}

fn current_display_window(
    app: &AppHandle,
    label: &str,
    created: &tauri::WebviewWindow,
) -> Result<usize, String> {
    resolve_display_window(
        label,
        created.label(),
        || {
            created
                .ns_window()
                .map(|window| window as usize)
                .map_err(|_| DISPLAY_WINDOW_ERROR.to_string())
        },
        |label| {
            let current = app.get_webview_window(label)?;
            let current_label = current.label().to_owned();
            let native_window = Box::new(move || {
                current
                    .ns_window()
                    .map(|window| window as usize)
                    .map_err(|_| DISPLAY_WINDOW_ERROR.to_string())
            });
            Some((current_label, native_window))
        },
    )
}

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

        let (app, label, created) = (self.app.clone(), label.clone(), display.clone());
        // `engine::attach_display` invokes this resolver in the same AppKit closure that
        // attaches the view, so no NSWindow address survives a scheduling boundary.
        if let Err(message) = engine::attach_display(
            &context,
            id,
            move || current_display_window(&app, &label, &created),
            toolbar,
        ) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Mutex,
        },
        thread::ThreadId,
    };

    #[derive(Default)]
    struct RecordingMainThread {
        running: AtomicBool,
        calls: AtomicUsize,
        thread: Mutex<Option<ThreadId>>,
    }

    impl silo_engine::MainThread for RecordingMainThread {
        fn run(&self, work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.thread.lock().unwrap() = Some(std::thread::current().id());
            self.running.store(true, Ordering::SeqCst);
            work();
            self.running.store(false, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn current_window_lookup_and_validation_run_on_the_main_thread_runner() {
        let runner = Arc::new(RecordingMainThread::default());
        let runner_for_work = runner.clone();
        let lookup_thread = Arc::new(Mutex::new(None));
        let thread_for_lookup = lookup_thread.clone();
        let result = Arc::new(Mutex::new(None));
        let result_for_work = result.clone();

        silo_engine::context::dispatch_to_main_thread(
            runner.as_ref(),
            Box::new(move || {
                let runner_for_created = runner_for_work.clone();
                let created_window = move || {
                    assert!(runner_for_created.running.load(Ordering::SeqCst));
                    *thread_for_lookup.lock().unwrap() = Some(std::thread::current().id());
                    Ok(0x10)
                };
                let runner_for_current = runner_for_work.clone();
                let lookup_current = move |label: &str| {
                    assert!(runner_for_current.running.load(Ordering::SeqCst));
                    (label == "display").then(|| {
                        let runner_for_ns_window = runner_for_current.clone();
                        let native_window = Box::new(move || {
                            assert!(runner_for_ns_window.running.load(Ordering::SeqCst));
                            Ok(0x10usize)
                        });
                        ("display".to_owned(), native_window as NativeWindowLookup)
                    })
                };
                *result_for_work.lock().unwrap() = Some(resolve_display_window(
                    "display",
                    "display",
                    created_window,
                    lookup_current,
                ));
            }),
        )
        .unwrap();

        assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
        assert_eq!(*result.lock().unwrap(), Some(Ok(0x10)));
        assert_eq!(
            *lookup_thread.lock().unwrap(),
            *runner.thread.lock().unwrap()
        );
    }

    #[test]
    fn display_attachment_rejects_a_closed_or_replaced_window() {
        assert_eq!(
            confirmed_display_window(0x10, 0),
            Err(DISPLAY_WINDOW_ERROR.to_string())
        );
        assert_eq!(
            confirmed_display_window(0x10, 0x20),
            Err(DISPLAY_WINDOW_ERROR.to_string())
        );
        assert_eq!(confirmed_display_window(0x10, 0x10), Ok(0x10));
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

pub(crate) fn computer_ids(app: &AppHandle) -> Vec<String> {
    new_context(app).map_or_else(|_| Vec::new(), |context| engine::computer_ids(&context))
}

pub(crate) fn start_at_launch(
    app: &AppHandle,
    id: &str,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<bool, String> {
    engine::start_at_launch(&new_context(app)?, id, cancelled)
}

pub(crate) fn log_target(app: &AppHandle, id: &str) -> Option<(String, PathBuf)> {
    engine::log_target(&new_context(app).ok()?, id)
}
