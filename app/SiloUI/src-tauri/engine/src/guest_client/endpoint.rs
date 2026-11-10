//! `/etc/silo/endpoint.json` in a computer: the host name, the port, the token and the
//! computer's id. The engine writes it as root through the guest agent (SSH for a macOS
//! computer), readable by the `silo` account's group, and checks it at every start: a file that
//! is missing, or whose token does not hash to the stored one, is replaced with a new token.

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use silo_api::methods::guest_client::{GUEST_ENDPOINT_HOST, GUEST_ENDPOINT_PATH};

use super::tokens::{hash_of, same, Record, TokenStore, MAX_TOKEN_BYTES};
use crate::guest::{self, ExecRequest, GuestUser, Target};

/// The file's format version.
pub const VERSION: u32 = 1;
const EXEC_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_FILE_BYTES: usize = 4096;

/// Writes standard input to `$1` atomically: root-owned, mode 0640 with the `silo` account's
/// group when the computer has that account, else 0600.
pub const WRITE_SCRIPT: &str = "set -e; umask 077; dir=$(dirname \"$1\"); mkdir -p \"$dir\"; chmod 755 \"$dir\"; \
     tmp=\"$1.new.$$\"; cat > \"$tmp\"; chown 0 \"$tmp\"; \
     group=$(id -gn silo 2>/dev/null || true); \
     if [ -n \"$group\" ]; then chgrp \"$group\" \"$tmp\"; chmod 640 \"$tmp\"; else chmod 600 \"$tmp\"; fi; \
     mv -f \"$tmp\" \"$1\"";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EndpointFile {
    host: String,
    port: u16,
    token: String,
    computer_id: String,
}

/// The file's text for a computer's record and its token.
pub fn file_text(record: &Record, token: &str) -> Result<String, String> {
    let port = record
        .port
        .ok_or("Silo's tools are not on in that computer.")?;
    Ok(json!({
        "version": VERSION,
        "host": GUEST_ENDPOINT_HOST,
        "port": port,
        "token": token,
        "computerId": record.computer_id,
    })
    .to_string())
}

/// Whether `text`, as read from the computer, names this record's channel with the token whose
/// hash the record keeps.
pub fn matches(text: &[u8], record: &Record) -> bool {
    let Ok(file) = serde_json::from_slice::<EndpointFile>(text) else {
        return false;
    };
    record.port == Some(file.port)
        && file.host == GUEST_ENDPOINT_HOST
        && file.computer_id == record.computer_id
        && !record.hash.is_empty()
        && file.token.len() <= MAX_TOKEN_BYTES
        && same(&hash_of(&file.token), &record.hash)
}

fn run_as_root(
    target: &Target,
    argv: &[&str],
    stdin: Option<Vec<u8>>,
) -> Result<guest::ExecOutput, String> {
    guest::run_bounded(
        target,
        &ExecRequest {
            argv: argv.iter().map(|word| (*word).to_owned()).collect(),
            cwd: None,
            env: Vec::new(),
            user: GuestUser::Root,
            stdin,
            timeout: EXEC_TIMEOUT,
            max_stdout: MAX_FILE_BYTES,
        },
    )
    .map_err(|error| error.message)
}

/// The file as the computer has it; `None` when it cannot be read.
fn read(target: &Target) -> Option<Vec<u8>> {
    run_as_root(target, &["cat", "--", GUEST_ENDPOINT_PATH], None)
        .ok()
        .filter(|output| output.exit_code == Some(0))
        .map(|output| output.stdout)
}

/// Mints a new token for the computer and writes the file into it. The computer must be
/// running.
pub fn write(target: &Target, store: &TokenStore) -> Result<(), String> {
    let minted = store.mint(target.id())?;
    let text = file_text(&minted.record, &minted.token)?;
    let output = run_as_root(
        target,
        &[
            "sh",
            "-c",
            WRITE_SCRIPT,
            "silo-endpoint",
            GUEST_ENDPOINT_PATH,
        ],
        Some(text.into_bytes()),
    )?;
    if output.exit_code != Some(0) {
        return Err(format!(
            "Silo could not write {GUEST_ENDPOINT_PATH} in the computer: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// Checks the computer's file against its record and writes a new one when it is missing or
/// does not match. Returns whether it wrote. The computer must be running.
pub fn ensure(target: &Target, store: &TokenStore) -> Result<bool, String> {
    let record = store
        .status(target.id())?
        .ok_or("Silo's tools are not on in that computer.")?;
    if read(target).is_some_and(|text| matches(&text, &record)) {
        return Ok(false);
    }
    write(target, store)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(dir: &std::path::Path) -> (TokenStore, Record, String) {
        let store = TokenStore::new(dir);
        store.enable("main", 40001).unwrap();
        let minted = store.mint("main").unwrap();
        (store, minted.record, minted.token)
    }

    #[test]
    fn the_file_names_the_host_the_port_the_token_and_the_computer() {
        let dir = tempfile::tempdir().unwrap();
        let (_, record, token) = record(dir.path());
        let text = file_text(&record, &token).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["host"], "host.microsandbox.internal");
        assert_eq!(value["port"], 40001);
        assert_eq!(value["token"], token.as_str());
        assert_eq!(value["computerId"], "main");
        assert!(matches(text.as_bytes(), &record));
    }

    #[test]
    fn a_file_with_another_token_port_or_computer_does_not_match() {
        let dir = tempfile::tempdir().unwrap();
        let (store, record, token) = record(dir.path());
        let text = file_text(&record, &token).unwrap();
        let fresh = store.mint("main").unwrap().record;
        assert!(!matches(text.as_bytes(), &fresh), "an old token");
        let mut moved = fresh.clone();
        moved.port = Some(40002);
        let current = store.mint("main").unwrap();
        let text = file_text(&current.record, &current.token).unwrap();
        assert!(matches(text.as_bytes(), &current.record));
        assert!(!matches(
            text.as_bytes(),
            &Record {
                port: Some(40002),
                ..current.record.clone()
            }
        ));
        assert!(!matches(
            text.as_bytes(),
            &Record {
                computer_id: "fork".into(),
                ..current.record.clone()
            }
        ));
        assert!(!matches(
            text.as_bytes(),
            &Record {
                hash: String::new(),
                ..current.record.clone()
            }
        ));
        assert!(!matches(b"not json", &current.record));
        assert!(!matches(b"", &current.record));
    }

    #[test]
    fn the_write_script_writes_atomically_and_keeps_the_file_private_without_the_account() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("etc/silo/endpoint.json");
        // As a normal user `chown 0` fails; the script stops before the file is replaced.
        let script = WRITE_SCRIPT.replace("chown 0 \"$tmp\"; ", "");
        let mut child = std::process::Command::new("sh")
            .args(["-c", &script, "silo-endpoint"])
            .arg(&path)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write as _;
        child.stdin.take().unwrap().write_all(b"{\"a\":1}").unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"a\":1}");
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(&path).unwrap().permissions(),
        ) & 0o777;
        assert!(mode == 0o600 || mode == 0o640, "{mode:o}");
        assert!(WRITE_SCRIPT.contains("chown 0 \"$tmp\""));
        assert!(WRITE_SCRIPT.contains("mv -f \"$tmp\" \"$1\""));
    }
}
