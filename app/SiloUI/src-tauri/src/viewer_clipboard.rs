//! Clipboard transfers between this device and a computer's desktop viewer.
//!
//! Both directions run only for an explicit user action (a shortcut, an Edit menu item or a
//! toolbar button) and never touch the device clipboard otherwise. The orchestration is written
//! against two small seams, [`Guest`] and [`DeviceClipboard`], so it is tested without a webview.
use crate::clipboard::{ClipboardError, DeviceClipboard, ImageLimits};
use crate::desktop_bridge::{
    Bridge, ClipboardSupport, GuestClipboard, MAX_IMAGE_BYTES, MAX_TEXT_BYTES,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};

/// How long Copy from Computer waits for the guest to announce a new selection.
const COPY_WAIT: Duration = Duration::from_millis(1000);

/// Event the shell window listens to for the outcome of a shortcut-started transfer.
pub(crate) const EVENT: &str = "desktop-clipboard";

const LIMITS: ImageLimits = ImageLimits {
    max_encoded_bytes: MAX_IMAGE_BYTES,
    max_pixels: ImageLimits::DEFAULT.max_pixels,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Action {
    Paste,
    Copy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Status {
    Pasted,
    Copied,
    /// Paste found nothing on the device clipboard.
    DeviceEmpty,
    /// Copy found nothing on the computer's clipboard.
    ComputerEmpty,
    TooLarge,
    NotConnected,
    /// The computer's desktop has clipboard transfer turned off and needs an update.
    Unsupported,
    Busy,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Content {
    Text,
    Image,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Report {
    pub action: Action,
    pub status: Status,
    pub content: Option<Content>,
    pub message: Option<String>,
}

impl Report {
    pub(crate) fn new(action: Action, status: Status, content: Option<Content>) -> Self {
        Self {
            action,
            status,
            content,
            message: None,
        }
    }

    pub(crate) fn failed(action: Action, message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            ..Self::new(action, Status::Failed, None)
        }
    }

    fn too_large(action: Action, content: Content) -> Self {
        Self::new(action, Status::TooLarge, Some(content))
    }
}

/// The computer's side of a transfer.
pub(crate) trait Guest {
    /// Whether the computer's desktop takes part in clipboard transfer in this direction.
    fn clipboard_support(&self, writing: bool) -> Result<ClipboardSupport, String>;
    fn send_text(&self, text: &str) -> Result<(), String>;
    fn send_image(&self, mime: &str, bytes: &[u8]) -> Result<(), String>;
    fn press_paste(&self) -> Result<(), String>;
    fn request_clipboard(&self, timeout: Duration) -> Result<GuestClipboard, String>;
}

/// How long to wait for the page to report the server's clipboard settings after it connects.
const SETTINGS_WAIT: Duration = Duration::from_secs(4);
const SETTINGS_POLL: Duration = Duration::from_millis(500);

impl Guest for Bridge<'_> {
    fn clipboard_support(&self, writing: bool) -> Result<ClipboardSupport, String> {
        let deadline = Instant::now() + SETTINGS_WAIT;
        loop {
            let support = self
                .clipboard_policy(Duration::from_secs(2))?
                .support(writing);
            if support != ClipboardSupport::Unknown || Instant::now() >= deadline {
                return Ok(support);
            }
            std::thread::sleep(SETTINGS_POLL);
        }
    }
    fn send_text(&self, text: &str) -> Result<(), String> {
        self.send_guest_text(text)
    }
    fn send_image(&self, mime: &str, bytes: &[u8]) -> Result<(), String> {
        self.send_guest_image(mime, bytes)
    }
    fn press_paste(&self) -> Result<(), String> {
        self.press_guest_paste()
    }
    fn request_clipboard(&self, timeout: Duration) -> Result<GuestClipboard, String> {
        self.request_guest_clipboard(timeout, true)
    }
}

fn device_error(action: Action, content: Content, error: ClipboardError) -> Report {
    match error {
        ClipboardError::TooLarge { .. } => Report::too_large(action, content),
        other => Report::failed(action, other.to_string()),
    }
}

/// Ends a transfer the computer's desktop cannot take part in, or `None` when it can. This runs
/// before the device clipboard is read or any key is sent.
fn refuse_unsupported(guest: &dyn Guest, action: Action) -> Option<Report> {
    match guest.clipboard_support(action == Action::Paste) {
        Ok(ClipboardSupport::Supported) => None,
        Ok(ClipboardSupport::Unsupported) => Some(Report::new(action, Status::Unsupported, None)),
        Ok(ClipboardSupport::Disabled) => Some(Report::failed(
            action,
            "The desktop's clipboard is turned off for this direction.",
        )),
        Ok(ClipboardSupport::Unknown) => Some(Report::new(action, Status::NotConnected, None)),
        Err(message) => Some(Report::failed(action, message)),
    }
}

/// Paste into Computer: text if the device clipboard holds any, otherwise an image, then Ctrl+V
/// in the computer.
pub(crate) fn paste(guest: &dyn Guest, device: &dyn DeviceClipboard) -> Report {
    let action = Action::Paste;
    if let Some(refusal) = refuse_unsupported(guest, action) {
        return refusal;
    }
    let text = match device.read_text(MAX_TEXT_BYTES) {
        Ok(text) => text,
        Err(error) => return device_error(action, Content::Text, error),
    };
    let (content, sent) = if let Some(text) = text {
        (Content::Text, guest.send_text(&text))
    } else {
        match device.read_image(LIMITS) {
            Ok(Some(png)) => (Content::Image, guest.send_image("image/png", &png.bytes)),
            Ok(None) => return Report::new(action, Status::DeviceEmpty, None),
            Err(error) => return device_error(action, Content::Image, error),
        }
    };
    match sent.and_then(|()| guest.press_paste()) {
        Ok(()) => Report::new(action, Status::Pasted, Some(content)),
        Err(message) => Report::failed(action, message),
    }
}

/// Copy from Computer: press Ctrl+C there, wait for the new selection, then write it to the
/// device clipboard. An empty answer leaves the device clipboard unchanged.
pub(crate) fn copy(guest: &dyn Guest, device: &dyn DeviceClipboard) -> Report {
    let action = Action::Copy;
    if let Some(refusal) = refuse_unsupported(guest, action) {
        return refusal;
    }
    match guest.request_clipboard(COPY_WAIT) {
        Err(message) => Report::failed(action, message),
        Ok(GuestClipboard::TooLarge) => Report::new(action, Status::TooLarge, None),
        Ok(GuestClipboard::Empty) => Report::new(action, Status::ComputerEmpty, None),
        Ok(GuestClipboard::Text(text)) => {
            if text.len() > MAX_TEXT_BYTES {
                return Report::too_large(action, Content::Text);
            }
            match device.write_text(&text) {
                Ok(()) => Report::new(action, Status::Copied, Some(Content::Text)),
                Err(error) => device_error(action, Content::Text, error),
            }
        }
        Ok(GuestClipboard::Image { mime, bytes }) => {
            if bytes.len() > MAX_IMAGE_BYTES {
                return Report::too_large(action, Content::Image);
            }
            match device.write_image_from_encoded(&bytes, &mime, LIMITS) {
                Ok(()) => Report::new(action, Status::Copied, Some(Content::Image)),
                Err(ClipboardError::TooLarge { .. }) => Report::too_large(action, Content::Image),
                Err(_) => Report::failed(action, "The computer's image could not be copied."),
            }
        }
    }
}

/// Viewers with a transfer in progress; a second request for the same viewer is refused.
static ACTIVE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

pub(crate) struct ActiveGuard(String);
impl ActiveGuard {
    pub(crate) fn acquire(label: &str) -> Option<Self> {
        let mut active = ACTIVE.get_or_init(Default::default).lock().ok()?;
        active
            .insert(label.to_string())
            .then(|| Self(label.to_string()))
    }
}
impl Drop for ActiveGuard {
    fn drop(&mut self) {
        if let Some(Ok(mut active)) = ACTIVE.get().map(|active| active.lock()) {
            active.remove(&self.0);
        }
    }
}

/// Runs one transfer for a viewer. Blocks on the page and the device clipboard, so call it from
/// a worker thread.
pub(crate) fn run_blocking(app: &AppHandle, label: &str, action: Action) -> Report {
    let Some(_guard) = ActiveGuard::acquire(label) else {
        return Report::new(action, Status::Busy, None);
    };
    let device = crate::clipboard::system();
    crate::desktop_viewer::with_bridge(app, label, |bridge| {
        Ok(match action {
            Action::Paste => paste(bridge, device),
            Action::Copy => copy(bridge, device),
        })
    })
    .unwrap_or_else(|_| Report::new(action, Status::NotConnected, None))
}

/// Starts a transfer from a shortcut or menu item and reports the outcome to the viewer's shell
/// window, which shows it in the toolbar.
pub(crate) fn spawn(app: &AppHandle, label: &str, action: Action) {
    let (app, label) = (app.clone(), label.to_string());
    tauri::async_runtime::spawn_blocking(move || {
        let report = run_blocking(&app, &label, action);
        let _ = app.emit_to(label.as_str(), EVENT, report);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::fake::FakeClipboard;
    use crate::desktop_bridge::test_page::{acknowledge_send, invocation};
    use crate::desktop_bridge::{Inbox, Op, Page, Reply};
    use std::{cell::RefCell, time::Instant};

    #[derive(Default)]
    struct FakeGuest {
        calls: RefCell<Vec<String>>,
        answer: RefCell<Option<Result<GuestClipboard, String>>>,
        send_error: RefCell<Option<String>>,
        support: RefCell<Option<Result<ClipboardSupport, String>>>,
    }
    impl FakeGuest {
        fn answering(answer: Result<GuestClipboard, String>) -> Self {
            let guest = Self::default();
            *guest.answer.borrow_mut() = Some(answer);
            guest
        }
        fn record(&self, call: String) -> Result<(), String> {
            self.calls.borrow_mut().push(call);
            match self.send_error.borrow().clone() {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }
    }
    impl Guest for FakeGuest {
        fn clipboard_support(&self, writing: bool) -> Result<ClipboardSupport, String> {
            self.calls.borrow_mut().push(format!("support:{writing}"));
            self.support
                .borrow()
                .clone()
                .unwrap_or(Ok(ClipboardSupport::Supported))
        }
        fn send_text(&self, text: &str) -> Result<(), String> {
            self.record(format!("text:{text}"))
        }
        fn send_image(&self, mime: &str, bytes: &[u8]) -> Result<(), String> {
            self.record(format!("image:{mime}:{}", bytes.len()))
        }
        fn press_paste(&self) -> Result<(), String> {
            self.record("paste".into())
        }
        fn request_clipboard(&self, _: Duration) -> Result<GuestClipboard, String> {
            self.answer.borrow_mut().take().expect("no answer queued")
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn device_with_image(width: u32, height: u32) -> FakeClipboard {
        let device = FakeClipboard::default();
        device
            .write_image_from_encoded(&png(width, height), "image/png", LIMITS)
            .unwrap();
        device
    }

    #[test]
    fn paste_sends_text_then_ctrl_v() {
        let (guest, device) = (FakeGuest::default(), FakeClipboard::default());
        device.set_raw_text("hello");
        let report = paste(&guest, &device);
        assert_eq!(report.status, Status::Pasted);
        assert_eq!(report.content, Some(Content::Text));
        assert_eq!(
            *guest.calls.borrow(),
            ["support:true", "text:hello", "paste"]
        );
    }

    #[test]
    fn paste_sends_a_png_when_the_device_holds_only_an_image() {
        let (guest, device) = (FakeGuest::default(), device_with_image(4, 3));
        let report = paste(&guest, &device);
        assert_eq!(report.status, Status::Pasted);
        assert_eq!(report.content, Some(Content::Image));
        let calls = guest.calls.borrow();
        assert!(calls[1].starts_with("image:image/png:"));
        assert_eq!(calls[2], "paste");
    }

    #[test]
    fn paste_with_nothing_on_the_device_sends_nothing() {
        let (guest, device) = (FakeGuest::default(), FakeClipboard::default());
        let report = paste(&guest, &device);
        assert_eq!(report.status, Status::DeviceEmpty);
        assert_eq!(*guest.calls.borrow(), ["support:true"]);
    }

    #[test]
    fn paste_refuses_oversized_text() {
        let (guest, device) = (FakeGuest::default(), FakeClipboard::default());
        device.set_raw_text(&"a".repeat(MAX_TEXT_BYTES + 1));
        let report = paste(&guest, &device);
        assert_eq!(
            (report.status, report.content),
            (Status::TooLarge, Some(Content::Text))
        );
        assert_eq!(*guest.calls.borrow(), ["support:true"]);
    }

    #[test]
    fn paste_reports_an_unreachable_device_clipboard_and_guest_failures() {
        let (guest, device) = (FakeGuest::default(), FakeClipboard::default());
        device.set_unavailable(true);
        assert_eq!(paste(&guest, &device).status, Status::Failed);
        assert_eq!(*guest.calls.borrow(), ["support:true"]);

        device.set_unavailable(false);
        device.set_raw_text("x");
        *guest.send_error.borrow_mut() = Some("The computer did not answer.".into());
        let report = paste(&guest, &device);
        assert_eq!(report.status, Status::Failed);
        assert_eq!(
            report.message.as_deref(),
            Some("The computer did not answer.")
        );
    }

    #[test]
    fn copy_writes_text_and_images_to_the_device() {
        let device = FakeClipboard::default();
        let report = copy(
            &FakeGuest::answering(Ok(GuestClipboard::Text("from guest".into()))),
            &device,
        );
        assert_eq!(
            (report.status, report.content),
            (Status::Copied, Some(Content::Text))
        );
        assert_eq!(device.text().as_deref(), Some("from guest"));

        let report = copy(
            &FakeGuest::answering(Ok(GuestClipboard::Image {
                mime: "image/png".into(),
                bytes: png(5, 2),
            })),
            &device,
        );
        assert_eq!(
            (report.status, report.content),
            (Status::Copied, Some(Content::Image))
        );
        let (width, height, _) = device.image_rgba().unwrap();
        assert_eq!((width, height), (5, 2));
    }

    #[test]
    fn copy_of_an_empty_computer_clipboard_leaves_the_device_unchanged() {
        let device = FakeClipboard::default();
        device.set_raw_text("keep");
        let report = copy(&FakeGuest::answering(Ok(GuestClipboard::Empty)), &device);
        assert_eq!(report.status, Status::ComputerEmpty);
        assert_eq!(device.text().as_deref(), Some("keep"));
    }

    #[test]
    fn copy_rejects_oversized_or_undecodable_content_without_changing_the_device() {
        let device = FakeClipboard::default();
        device.set_raw_text("keep");
        let oversized_text = GuestClipboard::Text("a".repeat(MAX_TEXT_BYTES + 1));
        assert_eq!(
            copy(&FakeGuest::answering(Ok(oversized_text)), &device).status,
            Status::TooLarge
        );
        let oversized_image = GuestClipboard::Image {
            mime: "image/png".into(),
            bytes: vec![0; MAX_IMAGE_BYTES + 1],
        };
        assert_eq!(
            copy(&FakeGuest::answering(Ok(oversized_image)), &device).status,
            Status::TooLarge
        );
        let garbage = GuestClipboard::Image {
            mime: "image/png".into(),
            bytes: b"not a png".to_vec(),
        };
        assert_eq!(
            copy(&FakeGuest::answering(Ok(garbage)), &device).status,
            Status::Failed
        );
        assert_eq!(device.text().as_deref(), Some("keep"));
    }

    #[test]
    fn copy_surfaces_a_timeout_from_the_page() {
        let device = FakeClipboard::default();
        let report = copy(
            &FakeGuest::answering(Err("The computer did not answer.".into())),
            &device,
        );
        assert_eq!(report.status, Status::Failed);
        assert_eq!(
            report.message.as_deref(),
            Some("The computer did not answer.")
        );
        assert_eq!(device.text(), None);
    }

    /// A page that answers a clipboard request with fixed content.
    struct AnsweringPage<'a> {
        inbox: &'a Inbox,
        kind: &'static str,
        body: Vec<u8>,
        policy: &'static str,
        scripts: RefCell<Vec<String>>,
    }
    const SUPPORTED: &str =
        r#"{"transport":true,"clipboard":true,"clipboardIn":true,"clipboardOut":true}"#;
    impl Page for AnsweringPage<'_> {
        fn eval(&self, script: &str) -> Result<(), String> {
            self.scripts.borrow_mut().push(script.to_string());
            if acknowledge_send(self.inbox, script, "ok") {
                return Ok(());
            }
            let (method, args) = invocation(script);
            if method == "capabilities" {
                self.inbox
                    .claim(Op::Capabilities, args[0].as_str().unwrap(), Instant::now())
                    .unwrap()
                    .deliver(Reply {
                        kind: "application/json".into(),
                        body: self.policy.as_bytes().to_vec(),
                    });
            }
            if method == "requestClipboard" {
                let nonce = args[0].as_str().unwrap().to_string();
                self.inbox
                    .claim(Op::Clipboard, &nonce, Instant::now())
                    .unwrap()
                    .deliver(Reply {
                        kind: self.kind.to_string(),
                        body: self.body.clone(),
                    });
            }
            Ok(())
        }
    }

    #[test]
    fn the_real_bridge_carries_an_image_in_both_directions() {
        let inbox = Inbox::default();
        let page = AnsweringPage {
            inbox: &inbox,
            kind: "image/png",
            body: png(3, 3),
            policy: SUPPORTED,
            scripts: RefCell::default(),
        };
        let bridge = Bridge {
            page: &page,
            inbox: &inbox,
        };
        let device = device_with_image(2, 2);
        assert_eq!(paste(&bridge, &device).status, Status::Pasted);
        {
            let scripts = page.scripts.borrow();
            assert!(scripts[0].contains("\"capabilities\""));
            assert!(scripts[1].contains("\"cb,image/png,"));
            assert!(scripts[2].contains("\"sendShortcut\"") && scripts[2].contains("\"kd,118\""));
        }
        let report = copy(&bridge, &device);
        assert_eq!(
            (report.status, report.content),
            (Status::Copied, Some(Content::Image))
        );
        assert_eq!(device.image_rgba().unwrap().0, 3);
    }

    #[test]
    fn a_desktop_without_clipboard_support_is_refused_before_anything_is_read_or_sent() {
        let device = FakeClipboard::default();
        device.set_raw_text("device");
        let guest = FakeGuest::answering(Ok(GuestClipboard::Text("stale".into())));
        *guest.support.borrow_mut() = Some(Ok(ClipboardSupport::Unsupported));
        assert_eq!(paste(&guest, &device).status, Status::Unsupported);
        assert_eq!(copy(&guest, &device).status, Status::Unsupported);
        assert_eq!(*guest.calls.borrow(), ["support:true", "support:false"]);
        assert_eq!(device.text().as_deref(), Some("device"));

        *guest.support.borrow_mut() = Some(Ok(ClipboardSupport::Unknown));
        assert_eq!(paste(&guest, &device).status, Status::NotConnected);
        *guest.support.borrow_mut() = Some(Ok(ClipboardSupport::Disabled));
        assert_eq!(paste(&guest, &device).status, Status::Failed);
        *guest.support.borrow_mut() = Some(Err("The desktop is not connected.".into()));
        assert_eq!(copy(&guest, &device).status, Status::Failed);
        assert!(!guest.calls.borrow().iter().any(|call| call == "paste"));
    }

    #[test]
    fn the_real_bridge_refuses_a_recipe_2_desktop_without_pressing_keys() {
        let inbox = Inbox::default();
        let page = AnsweringPage {
            inbox: &inbox,
            kind: "text/plain",
            body: b"stale guest text".to_vec(),
            policy: r#"{"transport":true,"clipboard":false,"clipboardIn":false,"clipboardOut":false}"#,
            scripts: RefCell::default(),
        };
        let bridge = Bridge {
            page: &page,
            inbox: &inbox,
        };
        let device = FakeClipboard::default();
        device.set_raw_text("device text");
        assert_eq!(paste(&bridge, &device).status, Status::Unsupported);
        assert_eq!(copy(&bridge, &device).status, Status::Unsupported);
        let scripts = page.scripts.borrow();
        assert!(scripts
            .iter()
            .all(|script| script.contains("\"capabilities\"")));
        assert_eq!(device.text().as_deref(), Some("device text"));
    }

    #[test]
    fn paste_does_not_report_success_when_the_transport_is_closed() {
        let inbox = Inbox::default();
        let page = crate::desktop_bridge::test_page::AckingPage::new(&inbox);
        page.outcome.set("closed");
        let bridge = Bridge {
            page: &page,
            inbox: &inbox,
        };
        struct Supported<'a>(Bridge<'a>);
        impl Guest for Supported<'_> {
            fn clipboard_support(&self, _: bool) -> Result<ClipboardSupport, String> {
                Ok(ClipboardSupport::Supported)
            }
            fn send_text(&self, text: &str) -> Result<(), String> {
                self.0.send_text(text)
            }
            fn send_image(&self, mime: &str, bytes: &[u8]) -> Result<(), String> {
                self.0.send_image(mime, bytes)
            }
            fn press_paste(&self) -> Result<(), String> {
                self.0.press_paste()
            }
            fn request_clipboard(&self, timeout: Duration) -> Result<GuestClipboard, String> {
                self.0.request_clipboard(timeout)
            }
        }
        let device = FakeClipboard::default();
        device.set_raw_text("x");
        let report = paste(&Supported(bridge), &device);
        assert_eq!(report.status, Status::Failed);
        assert_eq!(
            report.message.as_deref(),
            Some("The desktop is not connected.")
        );
    }

    #[test]
    fn an_oversized_computer_selection_is_too_large_not_a_cached_copy() {
        let device = FakeClipboard::default();
        device.set_raw_text("keep");
        let report = copy(&FakeGuest::answering(Ok(GuestClipboard::TooLarge)), &device);
        assert_eq!(report.status, Status::TooLarge);
        assert_eq!(device.text().as_deref(), Some("keep"));
    }

    #[test]
    fn a_second_transfer_for_the_same_viewer_is_refused_until_the_first_ends() {
        let first = ActiveGuard::acquire("desktop-shell-a").unwrap();
        assert!(ActiveGuard::acquire("desktop-shell-a").is_none());
        assert!(ActiveGuard::acquire("desktop-shell-b").is_some());
        drop(first);
        assert!(ActiveGuard::acquire("desktop-shell-a").is_some());
    }

    #[test]
    fn reports_serialize_with_kebab_case_names() {
        let report = Report::new(Action::Copy, Status::ComputerEmpty, Some(Content::Image));
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            serde_json::json!({"action":"copy","status":"computer-empty","content":"image","message":null})
        );
        assert_eq!(
            serde_json::to_value(Report::new(Action::Paste, Status::Unsupported, None)).unwrap()
                ["status"],
            "unsupported"
        );
    }
}
