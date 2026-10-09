//! The setup log of one macOS computer: what each step did, command exit statuses and the
//! tails of their output.
//!
//! The file is `<app log dir>/macos-computers/<id>/runtime.log`, plain text with an RFC 3339
//! time before each line, which is the format Silo's Logs page already reads for a computer's
//! `runtime` source. It is append-only and rotates to `runtime.log.1` and so on, like the
//! other retained logs. The account password is replaced wherever it appears; command input
//! is never logged at all.
use super::store::Layout;
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{AppHandle, Manager};

const FILE: &str = "runtime.log";
/// A file past this size is rotated before the next line is written.
const MAX_BYTES: u64 = 1024 * 1024;
/// How many rotated files stay.
const ROTATED: u32 = 3;
/// The most output of a command that one line keeps.
const TAIL_BYTES: usize = 2000;
const REDACTED: &str = "[redacted]";

pub(super) struct SetupLog {
    dir: PathBuf,
    /// Every form of every password the log hides.
    secrets: Mutex<Vec<String>>,
}

/// The folder of one computer's retained logs.
pub(super) fn directory(app: &AppHandle, id: &str) -> Option<PathBuf> {
    Some(
        app.path()
            .app_log_dir()
            .ok()?
            .join("macos-computers")
            .join(id),
    )
}

impl SetupLog {
    pub(super) fn new(dir: PathBuf, passwords: &[String]) -> Self {
        let log = Self {
            dir,
            secrets: Mutex::new(Vec::new()),
        };
        for password in passwords {
            log.hide(password);
        }
        log
    }

    /// Hides `password` and the encodings it travels in (the base64 and hex of the password
    /// and of the `/etc/kcpassword` content made from it) from every later line.
    pub(super) fn hide(&self, password: &str) {
        if password.is_empty() {
            return;
        }
        let kcpassword = super::offline_setup::kcpassword(password);
        let forms = [
            password.to_string(),
            base64_encode(password.as_bytes()),
            base64_encode(&kcpassword),
            hex(password.as_bytes(), false),
            hex(password.as_bytes(), true),
            hex(&kcpassword, false),
            hex(&kcpassword, true),
        ];
        if let Ok(mut secrets) = self.secrets.lock() {
            for form in forms {
                if !secrets.contains(&form) {
                    secrets.push(form);
                }
            }
        }
    }

    /// The log of computer `id`, hiding the password of its account. `None` when the
    /// folder is unknown; logging is best effort and never fails a setup.
    pub(super) fn open(app: &AppHandle, id: &str) -> Option<Self> {
        let layout = Layout::new(&super::app_data(app).ok()?, id);
        let passwords = super::guest_access::account(&layout)
            .map(|account| vec![account.password])
            .unwrap_or_default();
        Some(Self::new(directory(app, id)?, &passwords))
    }

    pub(super) fn dir(&self) -> &Path {
        &self.dir
    }

    /// Appends one line.
    pub(super) fn line(&self, message: &str) {
        let _ = self.write(message, time::OffsetDateTime::now_utc());
    }

    /// Appends the result of a command: its status and the tails of its output.
    pub(super) fn command(&self, what: &str, status: i32, stdout: &str, stderr: &str) {
        // The whole output is cleaned first: a secret cut by the tail would no longer match.
        let secrets = self.secrets();
        self.line(&format!(
            "{what}: status {status}; stdout: {}; stderr: {}",
            tail(&sanitize(stdout, &secrets)),
            tail(&sanitize(stderr, &secrets))
        ));
    }

    fn secrets(&self) -> Vec<String> {
        self.secrets.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn write(&self, message: &str, now: time::OffsetDateTime) -> std::io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        let path = self.dir.join(FILE);
        rotate(&path, MAX_BYTES, ROTATED);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(format_line(now, message, &self.secrets()).as_bytes())
    }
}

/// The last `TAIL_BYTES` of command output on one line.
fn tail(text: &str) -> String {
    let text = text.trim();
    if text.len() <= TAIL_BYTES {
        return text.to_string();
    }
    let mut start = text.len() - TAIL_BYTES;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &text[start..])
}

/// `text` without private key blocks (an unfinished block is dropped to its end) and with
/// every secret replaced.
fn sanitize(text: &str, secrets: &[String]) -> String {
    let mut kept = Vec::new();
    let mut in_key = false;
    for line in text.lines() {
        if line.contains("-----BEGIN") && line.contains("PRIVATE KEY") {
            in_key = true;
        }
        if in_key {
            if line.contains("-----END") {
                in_key = false;
                kept.push(REDACTED);
            }
            continue;
        }
        kept.push(line);
    }
    if in_key {
        kept.push(REDACTED);
    }
    let mut text = kept.join("\n");
    // Longest first, so a form that contains another is replaced whole.
    let mut ordered: Vec<&String> = secrets.iter().filter(|secret| !secret.is_empty()).collect();
    ordered.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    for secret in ordered {
        text = text.replace(secret.as_str(), REDACTED);
    }
    text
}

/// One log line: the time, then the sanitized message on a single line.
fn format_line(now: time::OffsetDateTime, message: &str, secrets: &[String]) -> String {
    let stamp = now
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let text = sanitize(message, secrets)
        .lines()
        .collect::<Vec<_>>()
        .join(" | ");
    format!("{stamp} {text}\n")
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn hex(bytes: &[u8], upper: bool) -> String {
    bytes
        .iter()
        .map(|byte| {
            if upper {
                format!("{byte:02X}")
            } else {
                format!("{byte:02x}")
            }
        })
        .collect()
}

/// Moves `path` to `path.1` (and older ones up) once it is past `max` bytes.
fn rotate(path: &Path, max: u64, keep: u32) {
    if fs::metadata(path).map_or(true, |meta| meta.len() < max) {
        return;
    }
    let numbered = |n: u32| PathBuf::from(format!("{}.{n}", path.display()));
    let _ = fs::remove_file(numbered(keep));
    for n in (1..keep).rev() {
        let _ = fs::rename(numbered(n), numbered(n + 1));
    }
    let _ = fs::rename(path, numbered(1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn a_line_has_a_time_and_hides_secrets_and_whole_key_blocks() {
        let now = time::OffsetDateTime::UNIX_EPOCH;
        let line = format_line(
            now,
            "login with hunter2\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk\n-----END OPENSSH PRIVATE KEY-----\nafter",
            &["hunter2".into(), String::new()],
        );
        assert_eq!(
            line,
            "1970-01-01T00:00:00Z login with [redacted] | [redacted] | after\n"
        );
        assert!(!line.contains("b3BlbnNzaC1rZXk"));
        let open = sanitize("a\n-----BEGIN PRIVATE KEY-----\nsecretbody", &[]);
        assert_eq!(open, "a\n[redacted]");
    }

    #[test]
    fn every_encoding_of_a_password_is_hidden_even_across_the_tail_cut() {
        let dir = tempfile::tempdir().unwrap();
        let log = SetupLog::new(dir.path().join("id"), &["Zx3kQ9mTt2pL".to_string()]);
        let kc = super::super::offline_setup::kcpassword("Zx3kQ9mTt2pL");
        let forms = [
            "Zx3kQ9mTt2pL".to_string(),
            base64_encode(b"Zx3kQ9mTt2pL"),
            base64_encode(&kc),
            hex(&kc, false),
            hex(&kc, true),
        ];
        // The secret starts just before the last TAIL_BYTES of the output.
        for form in forms {
            let output = format!("{}{form}{}", "a".repeat(TAIL_BYTES - 3), "b".repeat(20));
            log.command("x", 0, &output, "");
            let text = fs::read_to_string(log.dir().join(FILE)).unwrap();
            assert!(!text.contains(&form), "{form}");
            assert!(!text.contains(&form[form.len() / 2..]), "{form}");
            fs::remove_file(log.dir().join(FILE)).unwrap();
        }
        // A password added later (the template's) is hidden too.
        log.hide("TemplatePw-77");
        log.line("old TemplatePw-77");
        let text = fs::read_to_string(log.dir().join(FILE)).unwrap();
        assert!(!text.contains("TemplatePw-77"));
    }

    #[test]
    fn output_is_cut_to_its_tail() {
        let long = "é".repeat(TAIL_BYTES);
        let tail = tail(&long);
        assert!(tail.starts_with('…'));
        assert!(tail.len() <= TAIL_BYTES + 4);
        assert_eq!(super::tail("  short \n"), "short");
    }

    #[test]
    fn the_log_is_private_append_only_and_rotates() {
        let dir = tempfile::tempdir().unwrap();
        let log = SetupLog::new(dir.path().join("id"), &["pw".to_string()]);
        log.line("first pw");
        log.line("second");
        let path = log.dir().join(FILE);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.lines().next().unwrap().ends_with("first [redacted]"));
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(log.dir()), 0o700);
        rotate(&path, 1, 2);
        assert!(!path.exists());
        assert!(log.dir().join("runtime.log.1").exists());
        log.line("third");
        rotate(&path, 1, 2);
        rotate(&path, 1, 2);
        assert!(log.dir().join("runtime.log.2").exists());
        assert!(!log.dir().join("runtime.log.3").exists());
    }
}
