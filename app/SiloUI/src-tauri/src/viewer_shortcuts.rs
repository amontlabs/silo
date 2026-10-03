//! Native Paste and Copy triggers for desktop viewer windows.
//!
//! The guest page can forge any DOM event, so key presses inside it never start
//! a clipboard transfer. The triggers here come from the operating system before
//! the page sees the key: an `NSEvent` local monitor on macOS, a GTK key handler
//! on the viewer window on Linux, and the Edit menu on macOS. All three call
//! the same handlers below, which reach the computer only through the bridge.
use crate::viewer_clipboard;
use tauri::AppHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shortcut {
    PasteIntoComputer,
    CopyFromComputer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Platform {
    Mac,
    Linux,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Keys {
    /// Command on macOS, Super on Linux.
    pub command: bool,
    pub control: bool,
    pub option: bool,
    pub shift: bool,
}

/// Command+C and Command+V on macOS. Ctrl+C and Ctrl+V are ordinary guest
/// shortcuts on Linux, so Silo's triggers there are Ctrl+Shift+C and Ctrl+Shift+V.
/// `letter` is the layout-resolved character, so other keyboard layouts work.
pub(crate) fn classify(platform: Platform, keys: Keys, letter: Option<char>) -> Option<Shortcut> {
    let exact = match platform {
        Platform::Mac => keys.command && !keys.control && !keys.option && !keys.shift,
        Platform::Linux => keys.control && keys.shift && !keys.option && !keys.command,
    };
    if !exact {
        return None;
    }
    match letter?.to_ascii_lowercase() {
        'v' => Some(Shortcut::PasteIntoComputer),
        'c' => Some(Shortcut::CopyFromComputer),
        _ => None,
    }
}

pub(crate) fn clipboard_shortcuts_enabled() -> bool {
    true
}

/// Where keyboard input goes inside a viewer window, as far as the native layer can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) enum Focus {
    /// An editable native text control, such as a field editor.
    EditableText,
    /// The shell's own web content, whose inputs keep ordinary copy and paste.
    Shell,
    /// Anything else, which includes the guest's web content.
    Other,
}

/// Whether a viewer shortcut is taken for the computer rather than left to the focused control.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn takes_shortcut(focus: Focus) -> bool {
    focus == Focus::Other
}

/// Tracks the physical keys that started a shortcut, so holding one down starts one transfer.
#[derive(Debug, Default)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct HeldKeys {
    codes: Vec<u16>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl HeldKeys {
    /// A press of `code`; true unless that key is still down from an earlier press.
    pub(crate) fn press(&mut self, code: u16) -> bool {
        if self.codes.contains(&code) {
            return false;
        }
        self.codes.push(code);
        true
    }

    /// A release of `code`; true when it ends a tracked press.
    pub(crate) fn release(&mut self, code: u16) -> bool {
        let before = self.codes.len();
        self.codes.retain(|held| *held != code);
        self.codes.len() != before
    }

    /// The window lost focus, so no release will be seen.
    pub(crate) fn clear(&mut self) {
        self.codes.clear();
    }
}

/// Starts a shortcut's transfer for one viewer on a worker thread; the bridge waits for the
/// page and must never block the main thread.
pub(crate) fn run(app: &AppHandle, label: &str, shortcut: Shortcut) {
    let action = match shortcut {
        Shortcut::PasteIntoComputer => viewer_clipboard::Action::Paste,
        Shortcut::CopyFromComputer => viewer_clipboard::Action::Copy,
    };
    viewer_clipboard::spawn(app, label, action);
}

/// Runs a shortcut for the viewer window that has focus, if any.
#[cfg(target_os = "macos")]
pub(crate) fn run_focused(app: &AppHandle, shortcut: Shortcut) {
    use tauri::Manager;
    let focused = app
        .webview_windows()
        .into_iter()
        .find(|(label, window)| {
            crate::desktop_viewer::is_viewer_label(label) && window.is_focused().unwrap_or(false)
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
            match classify(Platform::Linux, keys, event.keyval().to_unicode()) {
                Some(shortcut) => {
                    // Autorepeat sends further presses of the same physical key.
                    if held.borrow_mut().press(event.hardware_keycode()) {
                        run(&app, &label, shortcut);
                    }
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac(command: bool, control: bool, option: bool, shift: bool) -> Keys {
        Keys {
            command,
            control,
            option,
            shift,
        }
    }

    #[test]
    fn macos_triggers_are_exactly_command_c_and_command_v() {
        let cmd = mac(true, false, false, false);
        assert_eq!(
            classify(Platform::Mac, cmd, Some('v')),
            Some(Shortcut::PasteIntoComputer)
        );
        assert_eq!(
            classify(Platform::Mac, cmd, Some('C')),
            Some(Shortcut::CopyFromComputer)
        );
        assert_eq!(classify(Platform::Mac, cmd, Some('x')), None);
        assert_eq!(classify(Platform::Mac, cmd, None), None);
        for keys in [
            mac(false, false, false, false),
            mac(true, true, false, false),
            mac(true, false, true, false),
            mac(true, false, false, true),
            mac(false, true, false, false),
        ] {
            assert_eq!(classify(Platform::Mac, keys, Some('v')), None, "{keys:?}");
        }
    }

    #[test]
    fn a_held_shortcut_key_starts_one_transfer_until_it_is_released_or_focus_is_lost() {
        let mut held = HeldKeys::default();
        assert!(held.press(54));
        assert!(!held.press(54), "autorepeat");
        assert!(!held.press(54));
        assert!(!held.release(55), "another key");
        assert!(!held.press(54));
        assert!(held.release(54));
        assert!(!held.release(54));
        assert!(held.press(54), "a new press after the release");
        held.clear();
        assert!(held.press(54), "a press after focus returns");
        held.clear();
        // A second shortcut key pressed while the first is down is a new transfer, and the
        // first one's autorepeat stays suppressed.
        assert!(held.press(54));
        assert!(held.press(55));
        assert!(!held.press(54));
    }

    #[test]
    fn only_the_guest_side_of_a_viewer_window_gives_up_its_shortcuts() {
        assert!(takes_shortcut(Focus::Other));
        assert!(!takes_shortcut(Focus::Shell));
        assert!(!takes_shortcut(Focus::EditableText));
    }

    #[test]
    fn linux_triggers_are_ctrl_shift_c_and_ctrl_shift_v_so_plain_ctrl_v_reaches_the_guest() {
        let chord = mac(false, true, false, true);
        assert_eq!(
            classify(Platform::Linux, chord, Some('V')),
            Some(Shortcut::PasteIntoComputer)
        );
        assert_eq!(
            classify(Platform::Linux, chord, Some('c')),
            Some(Shortcut::CopyFromComputer)
        );
        for keys in [
            mac(false, true, false, false),
            mac(false, true, true, true),
            mac(true, true, false, true),
            mac(false, false, false, true),
        ] {
            assert_eq!(classify(Platform::Linux, keys, Some('v')), None, "{keys:?}");
        }
    }
}
