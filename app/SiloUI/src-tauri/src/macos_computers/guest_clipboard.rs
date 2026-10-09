//! Explicit clipboard transfers between this Mac and a macOS computer.
//!
//! "Paste into computer" and "Copy from computer" reuse the orchestration, size
//! limits and messages of the desktop viewer (`viewer_clipboard`); only the
//! computer's side differs. It runs over SSH as the logged-in `silo` user: text
//! moves through `pbcopy` and `pbpaste`, PNG images through `osascript`
//! (`«class PNGf»`), and payloads travel on standard input and base64 output,
//! never in the command line. Nothing syncs on its own.
//!
//! The transfer commands follow Lume's `ClipboardWatcher.swift`
//! (https://github.com/trycua/cua/blob/ba4c6369660ab4a9c4d3d8af942bc53ad376615f/libs/lume/src/Clipboard/ClipboardWatcher.swift,
//! MIT License, Copyright (c) trycua).
use super::guest_access as access;
use super::store::{Layout, State};
use super::{app_data, computer, engine};
use crate::{
    desktop_bridge::{ClipboardSupport, GuestClipboard, MAX_IMAGE_BYTES},
    viewer_clipboard::{self, Action, ActiveGuard, Guest, Report, Status},
};
use base64::Engine as _;
use serde::Deserialize;
use std::time::Duration;
use tauri::AppHandle;

const TEXT_TIMEOUT: Duration = Duration::from_secs(10);
const IMAGE_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a transfer's outcome stays in the screen window's subtitle.
const FEEDBACK_SHOWN: Duration = Duration::from_secs(5);

const WRITE_TEXT: &str = "LANG=en_US.UTF-8 /usr/bin/pbcopy";

const WRITE_IMAGE: &str = r#"f=$(/usr/bin/mktemp /tmp/silo-clipboard.XXXXXX) || exit 1
trap '/bin/rm -f "$f"' EXIT
/bin/cat > "$f" || exit 1
/usr/bin/osascript -e "set the clipboard to (read (POSIX file \"$f\") as «class PNGf»)""#;

const READ: &str = r#"f=$(/usr/bin/mktemp /tmp/silo-clipboard.XXXXXX) || exit 1
trap '/bin/rm -f "$f"' EXIT
if /usr/bin/osascript \
  -e 'set imageData to the clipboard as «class PNGf»' \
  -e "set fileRef to open for access POSIX file \"$f\" with write permission" \
  -e 'set eof fileRef to 0' \
  -e 'write imageData to fileRef' \
  -e 'close access fileRef' >/dev/null 2>&1; then
  printf 'IMAGE\n'
  /usr/bin/base64 < "$f"
else
  printf 'TEXT\n'
  LANG=en_US.UTF-8 /usr/bin/pbpaste | /usr/bin/base64
fi"#;

/// Which way a transfer goes, as the frontend names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub(crate) enum Direction {
    #[serde(rename = "paste-into")]
    PasteInto,
    #[serde(rename = "copy-from")]
    CopyFrom,
}

impl Direction {
    fn action(self) -> Action {
        match self {
            Self::PasteInto => Action::Paste,
            Self::CopyFrom => Action::Copy,
        }
    }
}

/// `script` as a single argument of `/bin/sh -c`, whatever the login shell is.
fn sh_command(script: &str) -> String {
    format!("/bin/sh -c '{}'", script.replace('\'', r"'\''"))
}

/// Splits the output of `READ` into the computer's clipboard.
fn parse_read(output: &str) -> Result<GuestClipboard, String> {
    let (kind, encoded) = output
        .split_once('\n')
        .ok_or("The computer's clipboard could not be read.")?;
    let encoded: String = encoded.split_whitespace().collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "The computer's clipboard could not be read.".to_string())?;
    if bytes.is_empty() {
        return Ok(GuestClipboard::Empty);
    }
    match kind.trim() {
        "IMAGE" if bytes.len() > MAX_IMAGE_BYTES => Ok(GuestClipboard::TooLarge),
        "IMAGE" => Ok(GuestClipboard::Image {
            mime: "image/png".into(),
            bytes,
        }),
        "TEXT" => String::from_utf8(bytes)
            .map(GuestClipboard::Text)
            .map_err(|_| "The computer's clipboard is not text.".to_string()),
        _ => Err("The computer's clipboard could not be read.".into()),
    }
}

/// The running computer reached over SSH.
struct MacGuest {
    layout: Layout,
    record: super::store::Record,
}

impl MacGuest {
    fn run(
        &self,
        command: &str,
        stdin: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<String, String> {
        let output = access::run(&self.layout, &self.record, command, stdin, timeout)?;
        if output.status == 0 {
            Ok(output.stdout)
        } else {
            Err(format!(
                "The computer's clipboard failed: {}",
                output.stderr.trim()
            ))
        }
    }
}

impl Guest for MacGuest {
    fn clipboard_support(&self, _writing: bool) -> Result<ClipboardSupport, String> {
        Ok(ClipboardSupport::Supported)
    }

    fn send_text(&self, text: &str) -> Result<(), String> {
        self.run(&sh_command(WRITE_TEXT), Some(text.as_bytes()), TEXT_TIMEOUT)
            .map(drop)
    }

    fn send_image(&self, _mime: &str, bytes: &[u8]) -> Result<(), String> {
        self.run(&sh_command(WRITE_IMAGE), Some(bytes), IMAGE_TIMEOUT)
            .map(drop)
    }

    /// The computer's clipboard is set; the user pastes where they want it.
    fn press_paste(&self) -> Result<(), String> {
        Ok(())
    }

    fn request_clipboard(&self, _timeout: Duration) -> Result<GuestClipboard, String> {
        parse_read(&self.run(&sh_command(READ), None, IMAGE_TIMEOUT)?)
    }
}

/// Runs one transfer in the running computer `id`. Blocks, so call it from a worker thread.
pub(super) fn run(app: &AppHandle, id: &str, direction: Direction) -> Result<Report, String> {
    let action = direction.action();
    let Some(_guard) = ActiveGuard::acquire(&format!("macos-computer-{id}")) else {
        return Ok(Report::new(action, Status::Busy, None));
    };
    let (record, state) = computer(id)?;
    if state != State::Running {
        return Err("Start the computer to use its clipboard.".into());
    }
    let guest = MacGuest {
        layout: Layout::new(&app_data(app)?, id),
        record,
    };
    let device = crate::clipboard::system();
    Ok(match action {
        Action::Paste => viewer_clipboard::paste(&guest, device),
        Action::Copy => viewer_clipboard::copy(&guest, device),
    })
}

/// The words for a transfer's outcome, as the desktop viewer's toolbar shows them.
pub(super) fn feedback(report: &Report, name: &str) -> String {
    use crate::viewer_clipboard::Content;
    let image = report.content == Some(Content::Image);
    let noun = if image { "image" } else { "text" };
    let verb = if report.action == Action::Paste {
        "paste"
    } else {
        "copy"
    };
    match report.status {
        Status::Pasted => format!("Pasted {}into {name}", if image { "image " } else { "" }),
        Status::Copied => format!("Copied {}from {name}", if image { "image " } else { "" }),
        Status::DeviceEmpty => "This device's clipboard has no text or image".into(),
        Status::ComputerEmpty => format!("Nothing to copy from {name}"),
        Status::TooLarge if report.content.is_some() => {
            format!("That {noun} is too large to {verb}")
        }
        Status::TooLarge => format!("{name}'s clipboard is too large to copy"),
        Status::NotConnected => "The computer is not connected".into(),
        Status::Unsupported => "This computer does not support clipboard transfer".into(),
        Status::Busy => "A clipboard transfer is already running".into(),
        Status::Failed => report
            .message
            .clone()
            .unwrap_or_else(|| "The clipboard transfer failed".into()),
    }
}

/// Starts a transfer from the screen window's toolbar and shows its outcome in
/// that window's subtitle.
pub(super) fn spawn_from_display(app: &AppHandle, id: &str, direction: Direction) {
    let (app, id) = (app.clone(), id.to_string());
    tauri::async_runtime::spawn_blocking(move || {
        let name = computer(&id)
            .map(|(record, _)| record.name)
            .unwrap_or_default();
        let text = match run(&app, &id, direction) {
            Ok(report) => feedback(&report, &name),
            Err(message) => message,
        };
        engine::set_display_subtitle(&app, &id, &text);
        std::thread::sleep(FEEDBACK_SHOWN);
        engine::set_display_subtitle(&app, &id, "");
    });
}

/// Nothing needs installing in the computer: its clipboard is reached with the
/// tools macOS ships. Kept so provisioning can call it as a step.
pub(super) fn install(_app: &AppHandle, _id: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer_clipboard::Content;

    fn encoded(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn directions_parse_from_the_frontend_names() {
        let parse = |name: &str| serde_json::from_value::<Direction>(name.into());
        assert_eq!(parse("paste-into").unwrap(), Direction::PasteInto);
        assert_eq!(parse("copy-from").unwrap(), Direction::CopyFrom);
        assert!(parse("paste").is_err());
        assert_eq!(Direction::PasteInto.action(), Action::Paste);
        assert_eq!(Direction::CopyFrom.action(), Action::Copy);
    }

    #[test]
    fn scripts_become_one_quoted_shell_argument() {
        let command = sh_command("trap '/bin/rm -f \"$f\"' EXIT");
        assert_eq!(command, r#"/bin/sh -c 'trap '\''/bin/rm -f "$f"'\'' EXIT'"#);
        assert!(sh_command(WRITE_TEXT).starts_with("/bin/sh -c 'LANG=en_US.UTF-8 /usr/bin/pbcopy"));
    }

    #[test]
    fn payloads_never_appear_in_the_commands() {
        for script in [WRITE_TEXT, WRITE_IMAGE, READ] {
            assert!(!script.contains("base64 -D"));
        }
        assert!(WRITE_IMAGE.contains("«class PNGf»"));
        assert!(READ.contains("pbpaste"));
    }

    #[test]
    fn text_output_parses_to_text() {
        let output = format!("TEXT\n{}\n", encoded("héllo\nworld".as_bytes()));
        assert_eq!(
            parse_read(&output).unwrap(),
            GuestClipboard::Text("héllo\nworld".into())
        );
    }

    #[test]
    fn image_output_parses_to_a_png() {
        let output = format!("IMAGE\n{}\n", encoded(b"\x89PNG"));
        assert_eq!(
            parse_read(&output).unwrap(),
            GuestClipboard::Image {
                mime: "image/png".into(),
                bytes: b"\x89PNG".to_vec()
            }
        );
    }

    #[test]
    fn wrapped_base64_parses() {
        let all = encoded(&[7u8; 200]);
        let (first, rest) = all.split_at(76);
        let output = format!("IMAGE\n{first}\n{rest}\n");
        let GuestClipboard::Image { bytes, .. } = parse_read(&output).unwrap() else {
            panic!("expected an image");
        };
        assert_eq!(bytes, vec![7u8; 200]);
    }

    #[test]
    fn empty_and_oversized_clipboards_are_reported() {
        assert_eq!(parse_read("TEXT\n").unwrap(), GuestClipboard::Empty);
        let huge = format!("IMAGE\n{}\n", encoded(&vec![0u8; MAX_IMAGE_BYTES + 1]));
        assert_eq!(parse_read(&huge).unwrap(), GuestClipboard::TooLarge);
    }

    #[test]
    fn malformed_output_is_an_error() {
        assert!(parse_read("no newline").is_err());
        assert!(parse_read("TEXT\n!!!").is_err());
        assert!(parse_read(&format!("OTHER\n{}", encoded(b"x"))).is_err());
        assert!(parse_read(&format!("TEXT\n{}", encoded(&[0xff, 0xfe]))).is_err());
    }

    #[test]
    fn feedback_names_the_computer_and_content() {
        let report = |action, status, content| Report::new(action, status, content);
        assert_eq!(
            feedback(
                &report(Action::Paste, Status::Pasted, Some(Content::Image)),
                "mac"
            ),
            "Pasted image into mac"
        );
        assert_eq!(
            feedback(
                &report(Action::Copy, Status::Copied, Some(Content::Text)),
                "mac"
            ),
            "Copied from mac"
        );
        assert_eq!(
            feedback(
                &report(Action::Copy, Status::TooLarge, Some(Content::Image)),
                "mac"
            ),
            "That image is too large to copy"
        );
        assert_eq!(
            feedback(&Report::failed(Action::Copy, "boom"), "mac"),
            "boom"
        );
    }

    #[test]
    fn installing_needs_nothing() {
        // Compiles against the provisioning step's signature.
        let _: fn(&AppHandle, &str) -> Result<(), String> = install;
    }
}
