//! Which Silo-managed secrets a computer used, read from the proxy's event files.
//!
//! Linux computers: MicroSandbox writes `secret-usage.jsonl` in the sandbox's `logs/`
//! directory. macOS computers: the gateway writes it in the computer's folder. Each line
//! names a secret, a host and a method; a value is never in the file and never parsed here.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use serde::Deserialize;
use silo_api::methods::credentials_used::{CredentialBlocked, CredentialUse, CredentialsUsage};

/// File name both sources use.
pub const USAGE_FILE: &str = "secret-usage.jsonl";
/// Where a macOS gateway's own output goes, next to the events.
pub const GATEWAY_LOG_FILE: &str = "gateway.log";
/// Size at which a log moves to `<name>.1`.
pub const LOG_CAP_BYTES: u64 = 1024 * 1024;

/// One parsed line. Unknown fields are ignored, so nothing else can be stored.
#[derive(Debug, Deserialize)]
struct Event {
    at: u64,
    kind: String,
    #[serde(default)]
    secret: Option<String>,
    host: String,
    #[serde(default)]
    method: String,
    #[serde(default)]
    reason: Option<String>,
}

/// Event files of a Linux computer.
pub fn linux_usage_file(home: &Path, sandbox_name: &str) -> PathBuf {
    home.join("sandboxes")
        .join(sandbox_name)
        .join("logs")
        .join(USAGE_FILE)
}

/// Aggregates the events in `files` (each read with its `.1` generation first).
pub fn read_usage(files: &[PathBuf], since: Option<u64>) -> CredentialsUsage {
    let mut text = String::new();
    for file in files {
        let mut older = file.clone().into_os_string();
        older.push(".1");
        for path in [PathBuf::from(older), file.clone()] {
            if let Ok(bytes) = fs::read(&path) {
                text.push_str(&String::from_utf8_lossy(&bytes));
                text.push('\n');
            }
        }
    }
    aggregate(&text, since)
}

/// Aggregates JSON-lines text; malformed or incomplete lines are skipped.
pub fn aggregate(text: &str, since: Option<u64>) -> CredentialsUsage {
    let mut used: BTreeMap<(String, String, String), (u64, u64)> = BTreeMap::new();
    let mut blocked: BTreeMap<(Option<String>, String, String, String), (u64, u64)> =
        BTreeMap::new();
    for line in text.lines() {
        let Ok(event) = serde_json::from_str::<Event>(line) else {
            continue;
        };
        if event.host.is_empty() || since.is_some_and(|since| event.at < since) {
            continue;
        }
        let secret = event.secret.filter(|name| !name.is_empty());
        let slot = match event.kind.as_str() {
            "used" => {
                let Some(secret) = secret else { continue };
                used.entry((secret, event.host, event.method))
                    .or_insert((0, 0))
            }
            "blocked" => blocked
                .entry((
                    secret,
                    event.host,
                    event.method,
                    event.reason.unwrap_or_default(),
                ))
                .or_insert((0, 0)),
            _ => continue,
        };
        slot.0 += 1;
        slot.1 = slot.1.max(event.at);
    }
    CredentialsUsage {
        used: used
            .into_iter()
            .map(|((secret, host, method), (count, last_at))| CredentialUse {
                secret,
                host,
                method,
                count,
                last_at,
            })
            .collect(),
        blocked: blocked
            .into_iter()
            .map(
                |((secret, host, method, reason), (count, last_at))| CredentialBlocked {
                    secret,
                    host,
                    method,
                    count,
                    reason,
                    last_at,
                },
            )
            .collect(),
    }
}

type Locator = Box<dyn Fn(&str) -> Vec<PathBuf> + Send + Sync>;

fn locator() -> &'static Mutex<Option<Locator>> {
    static LOCATOR: OnceLock<Mutex<Option<Locator>>> = OnceLock::new();
    LOCATOR.get_or_init(|| Mutex::new(None))
}

/// Registers how a computer id maps to its event files.
pub fn set_locator(locate: impl Fn(&str) -> Vec<PathBuf> + Send + Sync + 'static) {
    *locator().lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(locate));
}

/// Credentials a computer used since it was created; empty when nothing is registered.
pub fn credentials_for(computer_id: &str) -> CredentialsUsage {
    let files = locator()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|locate| locate(computer_id))
        .unwrap_or_default();
    read_usage(&files, None)
}

/// Moves `path` to `<path>.1` when it is at least `cap` bytes, then opens it for appending.
pub fn open_capped_log(path: &Path, cap: u64) -> std::io::Result<fs::File> {
    if fs::metadata(path).is_ok_and(|meta| meta.len() >= cap) {
        let mut older = path.as_os_str().to_owned();
        older.push(".1");
        fs::rename(path, older)?;
    }
    fs::OpenOptions::new().create(true).append(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINUX: &str = concat!(
        "{\"v\":1,\"at\":10,\"kind\":\"used\",\"secret\":\"OPENAI_API_KEY\",\"host\":\"api.openai.com\",\"method\":\"POST\"}\n",
        "{\"v\":1,\"at\":30,\"kind\":\"used\",\"secret\":\"OPENAI_API_KEY\",\"host\":\"api.openai.com\",\"method\":\"POST\",\"value\":\"sk-leak\"}\n",
        "not json\n",
        "{\"at\":5,\"kind\":\"used\",\"host\":\"x\"}\n",
        "{\"v\":1,\"at\":20,\"kind\":\"blocked\",\"secret\":\"GITHUB_TOKEN\",\"host\":\"evil.example\",\"method\":\"GET\",\"reason\":\"host_not_allowed\"}\n",
    );
    const MACOS: &str = "{\"v\":1,\"at\":40,\"kind\":\"used\",\"secret\":\"OPENAI_API_KEY\",\"host\":\"api.openai.com\",\"method\":\"POST\"}\n";

    #[test]
    fn counts_per_secret_host_and_method_and_skips_malformed_lines() {
        let usage = aggregate(LINUX, None);
        assert_eq!(usage.used.len(), 1);
        assert_eq!(usage.used[0].count, 2);
        assert_eq!(usage.used[0].last_at, 30);
        assert_eq!(usage.blocked.len(), 1);
        assert_eq!(usage.blocked[0].secret.as_deref(), Some("GITHUB_TOKEN"));
        assert_eq!(usage.blocked[0].reason, "host_not_allowed");
    }

    #[test]
    fn no_value_is_stored_anywhere_in_the_result() {
        let usage = aggregate(LINUX, None);
        assert!(!serde_json::to_string(&usage).unwrap().contains("sk-leak"));
    }

    #[test]
    fn since_drops_older_events() {
        let usage = aggregate(LINUX, Some(25));
        assert_eq!(usage.used[0].count, 1);
        assert!(usage.blocked.is_empty());
    }

    #[test]
    fn both_sources_and_rotated_generations_merge() {
        let dir = tempfile::tempdir().unwrap();
        let linux = dir.path().join("linux.jsonl");
        let macos = dir.path().join("macos.jsonl");
        fs::write(&linux, LINUX).unwrap();
        fs::write(dir.path().join("linux.jsonl.1"), MACOS).unwrap();
        fs::write(&macos, MACOS).unwrap();
        let usage = read_usage(&[linux, macos, dir.path().join("missing.jsonl")], None);
        assert_eq!(usage.used[0].count, 4);
        assert_eq!(usage.used[0].last_at, 40);
    }

    #[test]
    fn capped_log_rotates_once_full() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(GATEWAY_LOG_FILE);
        fs::write(&path, "0123456789").unwrap();
        open_capped_log(&path, 10).unwrap();
        assert!(dir.path().join("gateway.log.1").exists());
        assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    }

    #[test]
    fn linux_file_sits_in_the_sandbox_log_directory() {
        assert!(linux_usage_file(Path::new("/h"), "dev")
            .ends_with("sandboxes/dev/logs/secret-usage.jsonl"));
    }
}
