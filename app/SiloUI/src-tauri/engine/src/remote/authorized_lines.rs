//! Reads `authorized_keys` lines for Connections: the key's fingerprint, the forced command and
//! whether that command is this channel's bridge, with or without the `--key` argument that tells
//! the owner which key authenticated.
use base64::{engine::general_purpose, Engine as _};
use sha2::{Digest, Sha256};
use std::path::Path;

/// The bridge argument that names the key a forced command belongs to.
pub(crate) const KEY_FLAG: &str = "--key";

/// `SHA256:` and the 43 unpadded base64 characters of a SHA-256 digest, as `ssh-keygen -l` prints.
pub fn is_key_fingerprint(text: &str) -> bool {
    text.strip_prefix("SHA256:").is_some_and(|digest| {
        digest.len() == 43
            && digest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
            && general_purpose::STANDARD_NO_PAD.decode(digest).is_ok()
    })
}

/// The OpenSSH fingerprint of a base64 key blob, or `None` when it is not base64.
pub fn blob_fingerprint(blob: &str) -> Option<String> {
    let bytes = general_purpose::STANDARD.decode(blob).ok()?;
    Some(format!(
        "SHA256:{}",
        general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(bytes))
    ))
}

/// Whether `blob` is an Ed25519 public key in OpenSSH wire form: the length-prefixed type
/// string `ssh-ed25519` and a length-prefixed 32-byte key, and nothing else.
pub fn is_ed25519_blob(blob: &str) -> bool {
    let Ok(bytes) = general_purpose::STANDARD.decode(blob) else {
        return false;
    };
    let field = |at: usize| -> Option<(&[u8], usize)> {
        let length = u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize;
        let start = at + 4;
        Some((bytes.get(start..start + length)?, start + length))
    };
    let Some((kind, next)) = field(0) else {
        return false;
    };
    let Some((key, end)) = field(next) else {
        return false;
    };
    kind == b"ssh-ed25519" && key.len() == 32 && end == bytes.len()
}

/// A line whose forced command is this channel's bridge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeLine {
    /// The key type, such as `ssh-ed25519`.
    pub kind: String,
    pub blob: String,
    /// What follows the key, possibly empty.
    pub comment: String,
    /// The `--key` argument of the forced command, when it has one.
    pub key_argument: Option<String>,
}

impl BridgeLine {
    pub fn public_key(&self) -> String {
        if self.comment.is_empty() {
            format!("{} {}", self.kind, self.blob)
        } else {
            format!("{} {} {}", self.kind, self.blob, self.comment)
        }
    }

    pub fn fingerprint(&self) -> Option<String> {
        blob_fingerprint(&self.blob)
    }

    /// The forced command names this line's own key, so the bridge reports it correctly.
    pub fn identified(&self) -> bool {
        self.key_argument.is_some() && self.key_argument == self.fingerprint()
    }
}

/// Splits `line` into its options (empty when it has none) and the rest, honoring quotes.
fn split_options(line: &str) -> (&str, &str) {
    let first = line.split_whitespace().next().unwrap_or_default();
    if is_key_type(first) {
        return ("", line);
    }
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        match character {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                return (&line[..index], line[index..].trim_start());
            }
            _ => {}
        }
    }
    (line, "")
}

fn is_key_type(word: &str) -> bool {
    word.starts_with("ssh-") || word.starts_with("ecdsa-") || word.starts_with("sk-")
}

/// The unescaped value of the `command="..."` option, if there is one.
fn forced_command(options: &str) -> Option<String> {
    let mut quoted = false;
    let mut escaped = false;
    let mut start = 0;
    let mut parts = Vec::new();
    for (index, character) in options.char_indices() {
        match character {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ',' if !quoted => {
                parts.push(&options[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&options[start..]);
    let value = parts.into_iter().find_map(|part| {
        let (name, value) = part.split_once('=')?;
        name.eq_ignore_ascii_case("command").then_some(value)
    })?;
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.replace("\\\"", "\""))
}

/// `Some(key argument)` when `command` runs `bridge` (the link's file name) from `~/.local/bin`
/// with `--remote-bridge`, written as Silo writes it or with `$HOME` or the home's path.
fn bridge_invocation(command: &str, bridge: &str, home: Option<&Path>) -> Option<Option<String>> {
    let mut words: Vec<&str> = command.split_whitespace().collect();
    if words.first() == Some(&"exec") {
        words.remove(0);
    }
    let program = *words.first()?;
    let relative = format!(".local/bin/{bridge}");
    let mut programs = vec![
        format!("~/{relative}"),
        format!("$HOME/{relative}"),
        format!("${{HOME}}/{relative}"),
    ];
    if let Some(home) = home {
        programs.push(home.join(&relative).to_string_lossy().into_owned());
    }
    if !programs.iter().any(|known| known == program) || words.get(1) != Some(&"--remote-bridge") {
        return None;
    }
    match &words[2..] {
        [] => Some(None),
        [flag, fingerprint] if *flag == KEY_FLAG => Some(Some((*fingerprint).to_owned())),
        _ => None,
    }
}

/// The line, when its forced command is the bridge named `bridge`.
pub fn bridge_line(line: &str, bridge: &str, home: Option<&Path>) -> Option<BridgeLine> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (options, rest) = split_options(line);
    let key_argument = bridge_invocation(&forced_command(options)?, bridge, home)?;
    let mut words = rest.splitn(3, char::is_whitespace);
    let kind = words.next().filter(|kind| is_key_type(kind))?;
    let blob = words.next().filter(|blob| !blob.is_empty())?;
    Some(BridgeLine {
        kind: kind.to_owned(),
        blob: blob.to_owned(),
        comment: words.next().unwrap_or_default().trim().to_owned(),
        key_argument,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOB: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIKVyZDGnacgUkbjXHi/TsIZ+GxIgprlkfONi9t33BGz3";
    // `ssh-keygen -lf` prints it for the key above.
    const PRINT: &str = "SHA256:LH9ifkGkEurhEj7pgiSwRedJtKQomUQeevYOdCTeGWw";

    #[test]
    fn fingerprints_and_ed25519_blobs() {
        assert_eq!(blob_fingerprint(BLOB).as_deref(), Some(PRINT));
        assert!(is_key_fingerprint(PRINT));
        for wrong in [
            "",
            "SHA256:",
            "MD5:LH9ifkGkEurhEj7pgiSwRedJtKQomUQeevYOdCTeGWw",
            "SHA256:LH9ifkGkEurhEj7pgiSwRedJtKQomUQeevYOdCTeGW",
            "SHA256:LH9ifkGkEurhEj7pgiSwRedJtKQomUQeevYOdCTeGWw=",
            "SHA256:LH9ifkGkEurhEj7pgiSwRedJtKQomUQeevYOdCTe;Ww",
        ] {
            assert!(!is_key_fingerprint(wrong), "{wrong}");
        }
        assert!(is_ed25519_blob(BLOB));
        // An RSA key, a truncated key and text that is not base64.
        assert!(!is_ed25519_blob("AAAAB3NzaC1yc2EAAAADAQABAAAAgQC0"));
        assert!(!is_ed25519_blob(&BLOB[..60]));
        assert!(!is_ed25519_blob("AAAAnot-a-key"));
    }

    #[test]
    fn bridge_lines_in_any_form_silo_or_a_person_wrote() {
        let bridge = "silo-remote";
        let home = Path::new("/home/ada");
        let line = |text: &str| bridge_line(text, bridge, Some(home));
        let current = line(&format!(
            r#"restrict,command="exec ~/.local/bin/silo-remote --remote-bridge --key {PRINT}" ssh-ed25519 {BLOB} Silo remote management"#
        ))
        .unwrap();
        assert_eq!(current.key_argument.as_deref(), Some(PRINT));
        assert!(current.identified());
        assert_eq!(
            current.public_key(),
            format!("ssh-ed25519 {BLOB} Silo remote management")
        );
        let legacy = line(&format!(
            r#"restrict,command="exec ~/.local/bin/silo-remote --remote-bridge" ssh-ed25519 {BLOB} Silo remote management"#
        ))
        .unwrap();
        assert_eq!(legacy.key_argument, None);
        assert!(!legacy.identified());
        let custom = line(&format!(
            r#"from="10.0.0.1,10.0.0.2",no-pty,command="/home/ada/.local/bin/silo-remote --remote-bridge" ssh-ed25519 {BLOB}"#
        ))
        .unwrap();
        assert_eq!(custom.comment, "");
        assert!(line(&format!(
            r#"command="$HOME/.local/bin/silo-remote --remote-bridge" ssh-ed25519 {BLOB} x"#
        ))
        .is_some());
        // Another key's fingerprint is read but does not identify this line.
        let wrong = line(&format!(
            r#"command="exec ~/.local/bin/silo-remote --remote-bridge --key SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" ssh-ed25519 {BLOB}"#
        ))
        .unwrap();
        assert!(!wrong.identified());
        for other in [
            format!("ssh-ed25519 {BLOB} personal"),
            format!(
                r#"command="exec ~/.local/bin/silo-remote-dev --remote-bridge" ssh-ed25519 {BLOB}"#
            ),
            format!(
                r#"command="exec ~/.local/bin/silo-remote --remote-bridge; sh" ssh-ed25519 {BLOB}"#
            ),
            format!(
                r#"command="exec ~/.local/bin/silo-remote --remote-bridge --other" ssh-ed25519 {BLOB}"#
            ),
            format!(r#"command="sh" ssh-ed25519 {BLOB}"#),
            format!(
                r#"# command="exec ~/.local/bin/silo-remote --remote-bridge" ssh-ed25519 {BLOB}"#
            ),
            String::new(),
        ] {
            assert_eq!(line(&other), None, "{other}");
        }
    }
}
