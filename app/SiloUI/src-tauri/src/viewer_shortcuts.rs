//! Native Paste and Copy triggers for desktop viewer windows.
//!
//! The guest page can forge any DOM event, so key presses inside it never start
//! a clipboard transfer. The triggers here come from the operating system before
//! the page sees the key: an `NSEvent` local monitor on macOS, a GTK key handler
//! on the viewer window on Linux, and the Edit menu on macOS. All three call
//! the same handlers below, which reach the computer only through the bridge.
use crate::desktop_bridge::{Bridge, GuestClipboard};
use std::time::Duration;
use tauri::AppHandle;

/// How long Copy from Computer waits for the guest to announce a new selection.
const COPY_WAIT: Duration = Duration::from_millis(1000);

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

/// The device clipboard as the shortcut handlers need it.
pub(crate) trait HostClipboard {
    fn read_text(&self) -> Result<Option<String>, String>;
    fn write_text(&self, text: &str) -> Result<(), String>;
}

/// The device clipboard the shortcuts use. `None` until the clipboard phase
/// provides one; while it is `None` the shortcuts and menu items stay inert and
/// the viewer behaves as before.
fn host_clipboard() -> Option<&'static (dyn HostClipboard + Sync)> {
    None
}

pub(crate) fn clipboard_shortcuts_enabled() -> bool {
    host_clipboard().is_some()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CopyOutcome {
    Copied,
    NothingToCopy,
    /// The guest holds an image; images arrive with the image phase.
    Unsupported,
}

/// Paste into Computer: put the device clipboard text on the computer's
/// clipboard, then press Ctrl+V there.
pub(crate) fn paste_into_computer(
    bridge: &Bridge,
    clipboard: &dyn HostClipboard,
) -> Result<(), String> {
    let Some(text) = clipboard.read_text()?.filter(|text| !text.is_empty()) else {
        return Ok(());
    };
    bridge.send_guest_text(&text)?;
    bridge.press_guest_paste()
}

/// Copy from Computer: press Ctrl+C there, wait for the new selection, then
/// write it to the device clipboard.
pub(crate) fn copy_from_computer(
    bridge: &Bridge,
    clipboard: &dyn HostClipboard,
) -> Result<CopyOutcome, String> {
    match bridge.request_guest_clipboard(COPY_WAIT, true)? {
        GuestClipboard::Text(text) => {
            clipboard.write_text(&text)?;
            Ok(CopyOutcome::Copied)
        }
        GuestClipboard::Empty => Ok(CopyOutcome::NothingToCopy),
        GuestClipboard::Image { .. } => Ok(CopyOutcome::Unsupported),
    }
}

/// Runs a shortcut for one viewer on a worker thread; the bridge waits for the
/// page and must never block the main thread.
pub(crate) fn run(app: &AppHandle, label: &str, shortcut: Shortcut) {
    let Some(clipboard) = host_clipboard() else {
        return;
    };
    let (app, label) = (app.clone(), label.to_string());
    tauri::async_runtime::spawn_blocking(move || {
        let result = crate::desktop_viewer::with_bridge(&app, &label, |bridge| match shortcut {
            Shortcut::PasteIntoComputer => paste_into_computer(bridge, clipboard),
            Shortcut::CopyFromComputer => copy_from_computer(bridge, clipboard).map(|_| ()),
        });
        if let Err(message) = result {
            eprintln!("Silo desktop viewer shortcut: {message}");
        }
    });
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
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType};
    use std::{
        collections::HashMap,
        ptr::{self, NonNull},
        sync::{Mutex, Once, OnceLock},
    };

    struct Registered {
        label: String,
        app: AppHandle,
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
        let target = {
            let address = objc2::rc::Retained::as_ptr(&window) as usize;
            windows()
                .lock()
                .ok()
                .and_then(|map| map.get(&address).map(|r| (r.app.clone(), r.label.clone())))
        };
        let Some((app, label)) = target else {
            return pass;
        };
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
                },
            );
        }
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

    pub(super) fn install(app: &AppHandle, window: &tauri::WebviewWindow) -> tauri::Result<()> {
        let gtk_window = window.gtk_window()?;
        let (app, label) = (app.clone(), window.label().to_string());
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
                    run(&app, &label, shortcut);
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
    use crate::desktop_bridge::{Inbox, Op, Page};
    use std::{cell::RefCell, time::Instant};

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

    #[test]
    fn the_shortcuts_stay_inert_until_a_device_clipboard_is_provided() {
        assert!(!clipboard_shortcuts_enabled());
    }

    #[derive(Default)]
    struct FakeClipboard {
        text: RefCell<Option<String>>,
        written: RefCell<Vec<String>>,
    }
    impl HostClipboard for FakeClipboard {
        fn read_text(&self) -> Result<Option<String>, String> {
            Ok(self.text.borrow().clone())
        }
        fn write_text(&self, text: &str) -> Result<(), String> {
            self.written.borrow_mut().push(text.to_string());
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingPage {
        scripts: RefCell<Vec<String>>,
    }
    impl Page for RecordingPage {
        fn eval(&self, script: &str) -> Result<(), String> {
            self.scripts.borrow_mut().push(script.to_string());
            Ok(())
        }
    }

    #[test]
    fn paste_sends_the_device_text_then_ctrl_v() {
        let page = RecordingPage::default();
        let inbox = Inbox::default();
        let bridge = Bridge {
            page: &page,
            inbox: &inbox,
        };
        let clipboard = FakeClipboard::default();
        paste_into_computer(&bridge, &clipboard).unwrap();
        assert!(
            page.scripts.borrow().is_empty(),
            "an empty clipboard sends nothing"
        );
        *clipboard.text.borrow_mut() = Some("hello".into());
        paste_into_computer(&bridge, &clipboard).unwrap();
        let scripts = page.scripts.borrow();
        assert_eq!(scripts.len(), 2);
        assert!(scripts[0].contains("\"cw,aGVsbG8=\""));
        assert!(scripts[1].contains("\"kd,118\""));
    }

    /// A page that answers a clipboard request with fixed content.
    struct AnsweringPage<'a> {
        inbox: &'a Inbox,
        kind: &'static str,
        body: &'static [u8],
    }
    impl Page for AnsweringPage<'_> {
        fn eval(&self, script: &str) -> Result<(), String> {
            let start = script.find("[\"").unwrap() + 2;
            let nonce: String = script[start..].chars().take_while(|c| *c != '"').collect();
            let claim = self
                .inbox
                .claim(Op::Clipboard, &nonce, Instant::now())
                .unwrap();
            claim.deliver(crate::desktop_bridge::Reply {
                kind: self.kind.to_string(),
                body: self.body.to_vec(),
            });
            Ok(())
        }
    }

    #[test]
    fn copy_writes_only_text_the_user_asked_for() {
        for (kind, body, outcome, written) in [
            (
                "text/plain",
                &b"from guest"[..],
                CopyOutcome::Copied,
                vec!["from guest"],
            ),
            ("none", &b""[..], CopyOutcome::NothingToCopy, vec![]),
            ("image/png", &b"png"[..], CopyOutcome::Unsupported, vec![]),
        ] {
            let inbox = Inbox::default();
            let page = AnsweringPage {
                inbox: &inbox,
                kind,
                body,
            };
            let bridge = Bridge {
                page: &page,
                inbox: &inbox,
            };
            let clipboard = FakeClipboard::default();
            assert_eq!(copy_from_computer(&bridge, &clipboard).unwrap(), outcome);
            assert_eq!(*clipboard.written.borrow(), written);
        }
    }
}
