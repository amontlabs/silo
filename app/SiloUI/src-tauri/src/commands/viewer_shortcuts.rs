//! Native window hooks for desktop viewer Paste and Copy shortcuts.
use crate::viewer_shortcuts::*;
use silo_api::methods::clipboard::ClipboardAction as Action;
use tauri::AppHandle;

/// Starts a shortcut's transfer for one viewer on a worker thread; the bridge waits for the
/// page and must never block the main thread.
pub(crate) fn run(app: &AppHandle, label: &str, shortcut: Shortcut) {
    let action = match shortcut {
        Shortcut::PasteIntoComputer => Action::Paste,
        Shortcut::CopyFromComputer => Action::Copy,
    };
    crate::commands::viewer_clipboard::spawn(app, label, action);
}

/// Runs a shortcut for the viewer window that has focus, if any.
#[cfg(target_os = "macos")]
pub(crate) fn run_focused(app: &AppHandle, shortcut: Shortcut) {
    use tauri::Manager;
    let focused = app
        .webview_windows()
        .into_iter()
        .find(|(label, window)| {
            crate::commands::desktop_viewer::is_shell(label) && window.is_focused().unwrap_or(false)
        })
        .map(|(label, _)| label);
    if let Some(label) = focused {
        run(app, &label, shortcut);
    }
}

/// Makes `window`'s Paste and Copy shortcuts native. Call from the main thread.
pub(crate) fn install(app: &AppHandle, window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    mac::install(app, window);
    #[cfg(target_os = "linux")]
    if let Err(error) = linux::install(app, window) {
        eprintln!("Silo desktop viewer shortcuts unavailable: {error}");
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = (app, window);
}

pub(crate) fn uninstall(window_label: &str) {
    #[cfg(target_os = "macos")]
    mac::unregister(window_label);
    #[cfg(not(target_os = "macos"))]
    let _ = window_label;
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use block2::RcBlock;
    use objc2::{rc::Retained, MainThreadMarker};
    use objc2_app_kit::{
        NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSText, NSView, NSWindow,
    };
    use std::{
        collections::HashMap,
        ptr::{self, NonNull},
        sync::{Mutex, Once, OnceLock},
    };

    struct Registered {
        label: String,
        app: AppHandle,
        /// Address of the shell's `WKWebView`, compared but never dereferenced.
        shell: Option<usize>,
    }
    /// NSWindow addresses of the viewer windows, by Tauri label.
    static WINDOWS: OnceLock<Mutex<HashMap<usize, Registered>>> = OnceLock::new();
    static MONITOR: Once = Once::new();

    fn windows() -> &'static Mutex<HashMap<usize, Registered>> {
        WINDOWS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn keys(flags: NSEventModifierFlags) -> Keys {
        Keys {
            command: flags.contains(NSEventModifierFlags::Command),
            control: flags.contains(NSEventModifierFlags::Control),
            option: flags.contains(NSEventModifierFlags::Option),
            shift: flags.contains(NSEventModifierFlags::Shift),
        }
    }

    fn address<T: objc2::Message>(object: &Retained<T>) -> usize {
        Retained::as_ptr(object) as usize
    }

    /// What the window's first responder is: an editable text control, part of the shell's
    /// web view, or something else (the guest's web view).
    fn focus(window: &NSWindow, shell: Option<usize>) -> Focus {
        let Some(responder) = window.firstResponder() else {
            return Focus::Other;
        };
        if let Ok(text) = responder.clone().downcast::<NSText>() {
            if text.isEditable() {
                return Focus::EditableText;
            }
        }
        let mut view = responder.downcast::<NSView>().ok();
        while let Some(current) = view {
            if Some(address(&current)) == shell {
                return Focus::Shell;
            }
            view = unsafe { current.superview() };
        }
        Focus::Other
    }

    /// Returns the event to let AppKit continue, or null to consume it.
    fn handle(event: NonNull<NSEvent>) -> *mut NSEvent {
        let pass = event.as_ptr();
        // SAFETY: AppKit passes a live event for the duration of the callback.
        let event = unsafe { event.as_ref() };
        if event.r#type() != NSEventType::KeyDown || !clipboard_shortcuts_enabled() {
            return pass;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            return pass;
        };
        let Some(window) = event.window(mtm) else {
            return pass;
        };
        let letter = event
            .charactersIgnoringModifiers()
            .and_then(|text| text.to_string().chars().next());
        let flags = event.modifierFlags() & NSEventModifierFlags::DeviceIndependentFlagsMask;
        let Some(shortcut) = classify(Platform::Mac, keys(flags), letter) else {
            return pass;
        };
        let target = windows().lock().ok().and_then(|map| {
            map.get(&address(&window))
                .map(|r| (r.app.clone(), r.label.clone(), r.shell))
        });
        let Some((app, label, shell)) = target else {
            return pass;
        };
        if !takes_shortcut(focus(&window, shell)) {
            return pass;
        }
        if !event.isARepeat() {
            run(&app, &label, shortcut);
        }
        ptr::null_mut()
    }

    pub(super) fn install(app: &AppHandle, window: &tauri::WebviewWindow) {
        let Ok(native) = window.ns_window() else {
            return;
        };
        if let Ok(mut map) = windows().lock() {
            map.insert(
                native as usize,
                Registered {
                    label: window.label().to_string(),
                    app: app.clone(),
                    shell: None,
                },
            );
        }
        let label = window.label().to_string();
        let _ = window.with_webview(move |webview| {
            let shell = webview.inner() as usize;
            if let Ok(mut map) = windows().lock() {
                for registered in map.values_mut().filter(|r| r.label == label) {
                    registered.shell = Some(shell);
                }
            }
        });
        MONITOR.call_once(|| {
            let block = RcBlock::new(handle);
            // SAFETY: the block returns either the event it was given or null.
            let monitor = unsafe {
                NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
            };
            // The monitor lives as long as the process.
            std::mem::forget(monitor);
        });
    }

    pub(super) fn unregister(label: &str) {
        if let Ok(mut map) = windows().lock() {
            map.retain(|_, registered| registered.label != label);
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use gtk::{gdk, glib, prelude::*};
    use std::{cell::RefCell, rc::Rc};

    pub(super) fn install(app: &AppHandle, window: &tauri::WebviewWindow) -> tauri::Result<()> {
        let gtk_window = window.gtk_window()?;
        let (app, label) = (app.clone(), window.label().to_string());
        let held = Rc::new(RefCell::new(HeldKeys::default()));
        // A release the handler below swallowed must not reach the guest either.
        let release_held = Rc::clone(&held);
        gtk_window.connect_key_release_event(move |_, event| {
            if release_held.borrow_mut().release(event.hardware_keycode()) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        let focus_held = Rc::clone(&held);
        gtk_window.connect_focus_out_event(move |_, _| {
            focus_held.borrow_mut().clear();
            glib::Propagation::Proceed
        });
        // A handler on the toplevel runs before the focused guest webview sees the key.
        gtk_window.connect_key_press_event(move |_, event| {
            if !clipboard_shortcuts_enabled() {
                return glib::Propagation::Proceed;
            }
            let state = event.state();
            let keys = Keys {
                command: state.contains(gdk::ModifierType::SUPER_MASK),
                control: state.contains(gdk::ModifierType::CONTROL_MASK),
                option: state.contains(gdk::ModifierType::MOD1_MASK),
                shift: state.contains(gdk::ModifierType::SHIFT_MASK),
            };
            let shortcut = classify(Platform::Linux, keys, event.keyval().to_unicode());
            match held.borrow_mut().decide(event.hardware_keycode(), shortcut) {
                Press::Start(shortcut) => {
                    run(&app, &label, shortcut);
                    glib::Propagation::Stop
                }
                Press::Swallow => glib::Propagation::Stop,
                Press::Pass => glib::Propagation::Proceed,
            }
        });
        Ok(())
    }
}
