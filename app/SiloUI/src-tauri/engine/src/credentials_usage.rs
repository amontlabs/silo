//! Which Silo-managed secrets a computer used, read from the proxy's event files.
//!
//! Linux computers: MicroSandbox writes `secret-usage.jsonl` in the sandbox's `logs/`
//! directory. macOS computers: the gateway writes it in the computer's folder. Each line
//! names a secret, a host and a method; a value is never in the file and never parsed here.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
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
    #[serde(default)]
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

/// Counter file the proxy keeps next to the event file; rotation never drops it.
pub const SUMMARY_FILE: &str = "secret-usage-summary.tsv";
/// Host shown when the proxy had none to name.
pub const UNKNOWN_HOST: &str = "(unknown host)";

/// Longest event line read; a longer one is skipped, not parsed.
const MAX_LINE_BYTES: usize = 4096;
/// Most distinct rows kept per list; later keys are not added (existing ones keep counting).
const MAX_ROWS: usize = 512;
/// Longest text kept for a name, host, method or reason.
const MAX_TEXT_CHARS: usize = 253;

/// Text for display: control characters become `?` and the length is capped.
fn tidy(text: &str) -> String {
    text.chars()
        .take(MAX_TEXT_CHARS)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

type UsedKey = (String, String, String);
type BlockedKey = (Option<String>, String, String, String);

/// Running totals while files are read.
#[derive(Default)]
struct Totals {
    used: BTreeMap<UsedKey, (u64, u64)>,
    blocked: BTreeMap<BlockedKey, (u64, u64)>,
}

impl Totals {
    fn add(&mut self, row: Row, count: u64, last_at: u64) {
        let host = if row.host.is_empty() {
            UNKNOWN_HOST.to_owned()
        } else {
            tidy(&row.host)
        };
        let secret = row
            .secret
            .filter(|name| !name.is_empty())
            .map(|name| tidy(&name));
        let method = tidy(&row.method);
        let slot = match row.kind.as_str() {
            "used" => {
                let Some(secret) = secret else { return };
                let key = (secret, host, method);
                if !self.used.contains_key(&key) && self.used.len() >= MAX_ROWS {
                    return;
                }
                self.used.entry(key).or_insert((0, 0))
            }
            "blocked" => {
                let key = (secret, host, method, tidy(&row.reason.unwrap_or_default()));
                if !self.blocked.contains_key(&key) && self.blocked.len() >= MAX_ROWS {
                    return;
                }
                self.blocked.entry(key).or_insert((0, 0))
            }
            _ => return,
        };
        slot.0 += count;
        slot.1 = slot.1.max(last_at);
    }

    fn finish(self) -> CredentialsUsage {
        CredentialsUsage {
            used: self
                .used
                .into_iter()
                .map(|((secret, host, method), (count, last_at))| CredentialUse {
                    secret,
                    host,
                    method,
                    count,
                    last_at,
                })
                .collect(),
            blocked: self
                .blocked
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
}

struct Row {
    kind: String,
    secret: Option<String>,
    host: String,
    method: String,
    reason: Option<String>,
}

/// Aggregates the proxy's files. Each event file gets its counter file when there is one and
/// `since` is not set; otherwise its events (the `.1` generation first) are counted.
pub fn read_usage(files: &[PathBuf], since: Option<u64>) -> CredentialsUsage {
    let mut totals = Totals::default();
    for file in files {
        let summary = file.with_file_name(SUMMARY_FILE);
        if since.is_none() {
            if let Ok(text) = fs::read_to_string(&summary) {
                add_summary(&mut totals, &text);
                continue;
            }
        }
        add_events(&mut totals, &read_generations(file), since);
    }
    totals.finish()
}

/// The older generation followed by the current file. A rotation between the two reads would
/// move the current file to `.1` unseen, so the read is repeated until the current file is
/// the same one before and after.
fn read_generations(file: &Path) -> String {
    let mut older = file.to_path_buf().into_os_string();
    older.push(".1");
    let older = PathBuf::from(older);
    let mut text = String::new();
    for _ in 0..4 {
        let before = identity(file);
        text.clear();
        for path in [&older, &file.to_path_buf()] {
            if let Ok(bytes) = fs::read(path) {
                text.push_str(&String::from_utf8_lossy(&bytes));
                text.push('\n');
            }
        }
        if identity(file) == before {
            break;
        }
    }
    text
}

/// Which file is at `path` right now (a rotation changes it).
fn identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|meta| (meta.dev(), meta.ino()))
}

/// Aggregates JSON-lines text; malformed or incomplete lines are skipped.
pub fn aggregate(text: &str, since: Option<u64>) -> CredentialsUsage {
    let mut totals = Totals::default();
    add_events(&mut totals, text, since);
    totals.finish()
}

fn add_events(totals: &mut Totals, text: &str, since: Option<u64>) {
    for line in text.lines() {
        if line.len() > MAX_LINE_BYTES {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Event>(line) else {
            continue;
        };
        if since.is_some_and(|since| event.at < since) {
            continue;
        }
        totals.add(
            Row {
                kind: event.kind,
                secret: event.secret,
                host: event.host,
                method: event.method,
                reason: event.reason,
            },
            1,
            event.at,
        );
    }
}

/// Counter lines: `kind  secret  host  method  reason  count  lastAt`, tab separated.
fn add_summary(totals: &mut Totals, text: &str) {
    for line in text.lines() {
        if line.len() > MAX_LINE_BYTES {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let [kind, secret, host, method, reason, count, last] = fields[..] else {
            continue;
        };
        let (Ok(count), Ok(last)) = (count.parse::<u64>(), last.parse::<u64>()) else {
            continue;
        };
        totals.add(
            Row {
                kind: kind.into(),
                secret: Some(secret.into()),
                host: host.into(),
                method: method.into(),
                reason: Some(reason.into()),
            },
            count,
            last,
        );
    }
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
    fn an_event_without_a_host_is_kept_as_unknown() {
        let usage = aggregate(
            "{\"at\":1,\"kind\":\"blocked\",\"secret\":\"K\",\"method\":\"CONNECT\",\"reason\":\"proxy_metadata\"}\n",
            None,
        );
        assert_eq!(usage.blocked.len(), 1);
        assert_eq!(usage.blocked[0].host, UNKNOWN_HOST);
        assert_eq!(usage.blocked[0].reason, "proxy_metadata");
    }

    #[test]
    fn the_counter_file_outlives_event_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let events = dir.path().join(USAGE_FILE);
        // The event file lost its older lines; the counters still know all 40 requests.
        fs::write(&events, MACOS).unwrap();
        fs::write(
            dir.path().join(SUMMARY_FILE),
            "used\tOPENAI_API_KEY\tapi.openai.com\tPOST\t\t40\t99\nblocked\tK\tevil.example\tGET\thost_not_allowed\t2\t50\n",
        )
        .unwrap();
        let usage = read_usage(&[events.clone()], None);
        assert_eq!(usage.used[0].count, 40);
        assert_eq!(usage.used[0].last_at, 99);
        assert_eq!(usage.blocked[0].count, 2);
        // With `since`, the retained events answer instead.
        assert_eq!(read_usage(&[events], Some(1)).used[0].count, 1);
    }

    #[test]
    fn long_lines_are_skipped_keys_are_capped_and_text_is_tidied() {
        let long = format!(
            "{{\"at\":1,\"kind\":\"used\",\"secret\":\"K\",\"host\":\"{}\",\"method\":\"GET\"}}\n",
            "a".repeat(5000)
        );
        assert!(aggregate(&long, None).used.is_empty());
        let mut many = String::new();
        for i in 0..(MAX_ROWS + 50) {
            many.push_str(&format!(
                "{{\"at\":1,\"kind\":\"used\",\"secret\":\"K\",\"host\":\"h{i}.example\",\"method\":\"GET\"}}\n"
            ));
        }
        assert_eq!(aggregate(&many, None).used.len(), MAX_ROWS);
        let control = aggregate(
            "{\"at\":1,\"kind\":\"used\",\"secret\":\"K\\u001b[2J\",\"host\":\"h\",\"method\":\"GET\"}\n",
            None,
        );
        assert!(!control.used[0].secret.contains('\u{1b}'));
    }

    #[test]
    fn rotation_between_reads_is_followed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(USAGE_FILE);
        fs::write(&file, LINUX).unwrap();
        let text = read_generations(&file);
        assert_eq!(text.matches("kind").count(), 4);
        assert!(identity(&file).is_some());
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
