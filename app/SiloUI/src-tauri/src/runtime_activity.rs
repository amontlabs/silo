use super::*;

const LIMIT: usize = 200;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Event {
    id: String,
    action: String,
    computer: String,
    #[serde(default)]
    computer_id: String,
    timestamp: u64,
    completed: bool,
    /// One-line summary of a failed action (never raw runtime output).
    failure: Option<String>,
    /// The runtime's own explanation of the failure, for a Details disclosure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diagnostic: Option<String>,
    #[serde(default)]
    dismissed: bool,
    /// The user cancelled the action; it is neither a failure nor a success.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    cancelled: bool,
    process: u32,
    #[serde(default)]
    process_session: String,
}

fn process_session() -> &'static str {
    static SESSION: OnceLock<String> = OnceLock::new();
    SESSION.get_or_init(|| uuid::Uuid::new_v4().to_string())
}

fn path(paths: &RuntimePaths) -> PathBuf {
    paths.metadata.with_file_name("computer-activity.json")
}

fn events(paths: &RuntimePaths) -> Result<Vec<Event>, RuntimeError> {
    let file = match File::open(path(paths)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => {
            return Err(RuntimeError::Unavailable(
                "Computer activity could not be read.".into(),
            ))
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RuntimeError::Unavailable("Computer activity could not be read.".into()))?;
    if bytes.len() as u64 > MAX_OUTPUT_BYTES {
        return Err(RuntimeError::Malformed(
            "Computer activity is too large.".into(),
        ));
    }
    let mut events: Vec<Event> = serde_json::from_slice(&bytes)
        .map_err(|_| RuntimeError::Malformed("Computer activity could not be decoded.".into()))?;
    // Entries from another build (a newer action, an over-long journal) only
    // cost history; they must not stop start/stop from journaling.
    events.retain(|event| {
        validate_name(&event.computer).is_ok()
            && matches!(event.action.as_str(), "start" | "stop" | "restart")
    });
    if events.len() > LIMIT {
        events.drain(..events.len() - LIMIT);
    }
    Ok(events)
}

static ACTIVITY_WRITES: Mutex<()> = Mutex::new(());

fn store(paths: &RuntimePaths, event: &Event) -> Result<(), String> {
    // Shutdown stops several computers concurrently. Keep each read-modify-write atomic
    // so one completed stop cannot erase another computer's activity entry.
    let _write = ACTIVITY_WRITES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut entries = events(paths).map_err(|error| error.to_string())?;
    entries.retain(|old| old.id != event.id);
    entries.push(event.clone());
    if entries.len() > LIMIT {
        entries.remove(0);
    }
    // Detailed failures must not make the journal exceed its own read limit.
    let sizes: Vec<usize> = entries
        .iter()
        .map(|entry| serde_json::to_vec(entry).map(|bytes| bytes.len() + 1))
        .collect::<Result<_, _>>()
        .map_err(|_| "Computer activity could not be saved.")?;
    let mut bytes = 1 + sizes.iter().sum::<usize>();
    let mut drop_count = 0;
    while bytes > MAX_OUTPUT_BYTES as usize && drop_count + 1 < entries.len() {
        bytes -= sizes[drop_count];
        drop_count += 1;
    }
    if bytes > MAX_OUTPUT_BYTES as usize {
        return Err("Computer activity is too large to save.".into());
    }
    entries.drain(..drop_count);
    let target = path(paths);
    let parent = target
        .parent()
        .ok_or("Computer activity storage is unavailable.")?;
    fs::create_dir_all(parent).map_err(|_| "Computer activity storage is unavailable.")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "Computer activity could not be saved.")?;
    serde_json::to_writer(&mut file, &entries)
        .map_err(|_| "Computer activity could not be saved.")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "Computer activity could not be saved.")?;
    file.persist(&target)
        .map_err(|_| "Computer activity could not be saved.")?;
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|_| "Computer activity could not be synced.".to_string())
}

// A failed history write cannot change a lifecycle result. Keep a warning for
// this session even if a later write succeeds, since an outcome may be missing.
static HISTORY_WARNINGS: OnceLock<Mutex<HashMap<PathBuf, Value>>> = OnceLock::new();

fn warn(paths: &RuntimePaths, message: &str) {
    let mut warning = history_warning("computer");
    warning["detail"] = format!("{message} Computer actions can still run.").into();
    HISTORY_WARNINGS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(path(paths), warning);
}

fn record(paths: &RuntimePaths, event: &Event) {
    // store leaves unreadable history untouched rather than overwriting it.
    if let Err(message) = store(paths, event) {
        warn(paths, &message);
    }
}

pub(super) fn begin(
    paths: &RuntimePaths,
    action: &str,
    computer: &str,
    computer_id: &str,
) -> Result<Event, String> {
    validate_name(computer).map_err(|error| error.to_string())?;
    if !matches!(action, "start" | "stop" | "restart") {
        return Err("Unknown computer action.".into());
    }
    let timestamp = activity_timestamp();
    let event = Event {
        // Concurrent shutdown workers can observe the same clock tick.
        id: format!("lifecycle-{}-{}", std::process::id(), uuid::Uuid::new_v4()),
        action: action.into(),
        computer: computer.into(),
        computer_id: computer_id.into(),
        timestamp,
        completed: false,
        failure: None,
        diagnostic: None,
        dismissed: false,
        cancelled: false,
        process: std::process::id(),
        process_session: process_session().into(),
    };
    record(paths, &event);
    Ok(event)
}

pub(super) fn matches(event: &Event, action: &str, computer: &str) -> bool {
    event.action == action && event.computer == computer
}

pub(super) fn resume(paths: &RuntimePaths, event: &mut Event, computer_id: &str) {
    event.computer_id = computer_id.into();
    event.process = std::process::id();
    event.process_session = process_session().into();
    event.completed = false;
    event.failure = None;
    event.diagnostic = None;
    event.dismissed = false;
    event.cancelled = false;
    record(paths, event);
}

pub(super) fn finish(paths: &RuntimePaths, event: &mut Event, result: &Result<(), RuntimeError>) {
    event.completed = true;
    event.cancelled = matches!(result, Err(RuntimeError::Cancelled { .. }));
    let report = result
        .as_ref()
        .err()
        .filter(|_| !event.cancelled)
        .map(failure_report);
    // The summary is the user-facing line; the runtime's explanation (filtered like
    // Logs and bounded) is kept separately for a Details disclosure.
    event.failure = report.as_ref().map(|report| report.summary.clone());
    event.diagnostic = report.and_then(|report| report.diagnostic);
    record(paths, event);
}

/// Settle an action that is being retired without running (D-22): an unfinished
/// entry becomes cancelled; a finished one (for example a failed start kept for
/// Retry) keeps its recorded outcome.
pub(super) fn retire(paths: &RuntimePaths, event: &mut Event) {
    // A saved action holds the entry as it was when saved; the journal has its outcome.
    let journaled = events(paths)
        .unwrap_or_else(|error| {
            warn(paths, &error.to_string());
            Vec::new()
        })
        .into_iter()
        .find(|entry| entry.id == event.id);
    if event.completed || journaled.is_some_and(|entry| entry.completed) {
        return;
    }
    let operation = format!("{} {}", event.action, event.computer);
    finish(paths, event, &Err(RuntimeError::Cancelled { operation }));
}

/// Summary and diagnostic of a recorded failure. Journals written before the
/// diagnostic field existed kept both in `failure`, separated by the first newline.
fn failure_parts(event: &Event) -> Option<(String, Option<String>)> {
    let failure = event.failure.as_deref()?;
    let (summary, legacy) = failure.split_once('\n').unwrap_or((failure, ""));
    let diagnostic = event.diagnostic.as_deref().unwrap_or(legacy);
    let summary = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = summary.to_lowercase();
    if lower.contains("exit code") || lower.contains("worker") {
        let details = format!("{summary}\n{diagnostic}");
        return Some((
            "The computer action did not finish. Check its state and retry.".into(),
            diagnostic_text(&details),
        ));
    }
    Some((summary, diagnostic_text(diagnostic)))
}

/// A computer's latest undismissed lifecycle failure, as shown on its row.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LifecycleFailureView {
    /// One line, for example "Start failed: …". Serialized as `lifecycleFailure`.
    lifecycle_failure: String,
    /// The runtime's explanation, for a Details disclosure. Serialized as
    /// `lifecycleFailureDiagnostic` and omitted when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    lifecycle_failure_diagnostic: Option<String>,
}

#[cfg(test)]
pub(super) fn failures(
    paths: &RuntimePaths,
) -> Result<HashMap<String, LifecycleFailureView>, RuntimeError> {
    Ok(failures_in(&events(paths)?))
}

/// The lifecycle failures and the activity history from one reading of the journal.
pub(super) fn failures_and_read(
    paths: &RuntimePaths,
) -> (
    HashMap<String, LifecycleFailureView>,
    Result<Vec<Value>, RuntimeError>,
) {
    let journal = events(paths);
    let failures = journal
        .as_ref()
        .map(|journal| failures_in(journal))
        .unwrap_or_default();
    (failures, read_journal(paths, journal))
}

fn failures_in(journal: &[Event]) -> HashMap<String, LifecycleFailureView> {
    let mut latest = HashMap::new();
    for event in journal {
        // Legacy records remain in Activity, but cannot be attributed safely to
        // a current computer: names can be reused after deletion or restoration.
        if !event.computer_id.is_empty() {
            latest.insert(event.computer_id.clone(), event);
        }
    }
    latest
        .into_iter()
        .filter_map(|(name, event)| {
            let label = match event.action.as_str() {
                "start" => "Start",
                "stop" => "Stop",
                _ => "Restart",
            };
            let (summary, diagnostic) = failure_parts(event).filter(|_| !event.dismissed)?;
            Some((
                name,
                LifecycleFailureView {
                    lifecycle_failure: format!("{label} failed: {summary}"),
                    lifecycle_failure_diagnostic: diagnostic,
                },
            ))
        })
        .collect()
}

pub(super) fn acknowledge_failure(
    paths: &RuntimePaths,
    computer_id: &str,
) -> Result<(), RuntimeError> {
    if let Some(mut event) = events(paths)
        .unwrap_or_else(|error| {
            warn(paths, &error.to_string());
            Vec::new()
        })
        .into_iter()
        .rev()
        .find(|event| event.computer_id == computer_id)
    {
        event.dismissed = true;
        record(paths, &event);
    }
    Ok(())
}

fn timestamp(value: u64) -> String {
    let date = time::OffsetDateTime::from_unix_timestamp((value / 1000) as i64)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        date.year(),
        u8::from(date.month()),
        date.day(),
        date.hour(),
        date.minute(),
        date.second(),
        value % 1000
    )
}

fn history_warning(kind: &str) -> Value {
    serde_json::json!({"id": format!("{kind}-history-unavailable"), "category": "system", "title": "Activity history unavailable", "detail": format!("Silo could not read its {kind} activity history."), "occurredAt": timestamp(activity_timestamp()), "time": timestamp(activity_timestamp()), "tone": "warning", "status": "completed"})
}

#[cfg(test)]
pub(super) fn read(paths: &RuntimePaths) -> Result<Vec<Value>, RuntimeError> {
    read_journal(paths, events(paths))
}

fn read_journal(
    paths: &RuntimePaths,
    journal: Result<Vec<Event>, RuntimeError>,
) -> Result<Vec<Value>, RuntimeError> {
    let mut warnings = Vec::new();
    let mut result: Vec<Value> = read_activity(paths, false).unwrap_or_else(|_| { warnings.push(history_warning("setup")); Vec::new() }).into_iter().enumerate().map(|(index, event)| {
        let mut entry = serde_json::json!({"id": format!("setup-{}-{}-{}-{index}", event.request_id, event.timestamp, event.step), "category": "computer", "title": event.message, "detail": "Computer setup", "occurredAt": timestamp(event.timestamp), "time": timestamp(event.timestamp), "tone": if event.level == "error" { "danger" } else if event.level == "warning" { "warning" } else { "neutral" }, "status": "completed", "computer": event.computer});
        if let Some(diagnostic) = event.diagnostic {
            entry["diagnostic"] = diagnostic.into();
        }
        if event.partial {
            entry["partial"] = true.into();
        }
        entry
    }).collect();
    result.extend(journal.unwrap_or_else(|_| { warnings.push(history_warning("computer")); Vec::new() }).into_iter().map(|event| {
        let interrupted = !event.completed && (event.process != std::process::id() || event.process_session != process_session());
        let failed = event.failure.is_some();
        if event.cancelled {
            let title = match event.action.as_str() { "start" => "Start cancelled", "stop" => "Stop cancelled", _ => "Restart cancelled" };
            return serde_json::json!({"id": event.id, "category": "computer", "title": title, "detail": "The action was cancelled.", "occurredAt": timestamp(event.timestamp), "time": timestamp(event.timestamp), "tone": "neutral", "status": "completed", "computer": event.computer, "cancelled": true});
        }
        let (failure, diagnostic) = failure_parts(&event).map_or((None, None), |(summary, diagnostic)| (Some(summary), diagnostic));
        let title = match (event.action.as_str(), event.completed, failed) {
            ("start", true, false) => "Computer started", ("stop", true, false) => "Computer stopped", ("restart", true, false) => "Computer restarted",
            ("start", _, _) => "Starting computer", ("stop", _, _) => "Stopping computer", _ => "Restarting computer",
        };
        let mut entry = serde_json::json!({"id": event.id, "category": "computer", "title": if failed { format!("{title} failed") } else { title.into() }, "detail": if interrupted { "Silo closed before the result was verified. Check the computer state.".into() } else { failure.unwrap_or_else(|| if event.completed { "Runtime state verified.".into() } else { "Waiting for the runtime…".into() }) }, "occurredAt": timestamp(event.timestamp), "time": timestamp(event.timestamp), "tone": if interrupted { "warning" } else if failed { "danger" } else if event.completed { "success" } else { "neutral" }, "status": if event.completed || interrupted { "completed" } else { "running" }, "computer": event.computer});
        // A failed entry's runtime explanation, for a Details disclosure (D-39).
        if let Some(diagnostic) = diagnostic.filter(|_| !interrupted) {
            entry["diagnostic"] = diagnostic.into();
        }
        entry
    }));
    if let Some(warning) = HISTORY_WARNINGS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&path(paths))
        .cloned()
    {
        warnings.retain(|entry| entry["id"] != warning["id"]);
        warnings.push(warning);
    }
    result.sort_by(|a, b| b["occurredAt"].as_str().cmp(&a["occurredAt"].as_str()));
    result.truncate(LIMIT - warnings.len());
    result.extend(warnings);
    result.sort_by(|a, b| b["occurredAt"].as_str().cmp(&a["occurredAt"].as_str()));
    Ok(result)
}

/// Remove terminal control sequences (7- and 8-bit CSI, OSC/DCS/APC/PM/SOS
/// strings) and every other C0/C1 control except tab and newline, so exported
/// logs cannot drive a terminal (CR/BS overwrites, colours, titles).
pub(super) fn strip_ansi(text: &str) -> String {
    fn skip_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
        while let Some(ch) = chars.next() {
            if ch == '\u{7}' || ch == '\u{9c}' {
                break;
            }
            if ch == '\u{1b}' && chars.peek() == Some(&'\\') {
                chars.next();
                break;
            }
        }
    }
    let mut chars = text.chars().peekable();
    let mut clean = String::with_capacity(text.len());
    while let Some(ch) = chars.next() {
        let introducer = match ch {
            '\u{1b}' => match chars.next() {
                Some('[') => '\u{9b}',
                Some(']') => '\u{9d}',
                Some('P') => '\u{90}',
                Some('X') => '\u{98}',
                Some('^') => '\u{9e}',
                Some('_') => '\u{9f}',
                _ => continue,
            },
            ch => ch,
        };
        match introducer {
            '\u{9b}' => {
                for ch in chars.by_ref() {
                    if ('@'..='~').contains(&ch) {
                        break;
                    }
                }
            }
            '\u{9d}' | '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => skip_string(&mut chars),
            '\t' | '\n' => clean.push(introducer),
            ch if ch.is_control() => {}
            ch => clean.push(ch),
        }
    }
    clean
}

/// A secret-looking assignment: `secret`, `token`, `key`, `passw` or
/// `credential` followed by optional word characters and quotes, then `:` or
/// `=` (for example `AWS_SECRET_ACCESS_KEY=`, `api_key =`, `"password": `).
fn sensitive_assignment(lower: &str) -> bool {
    ["secret", "token", "key", "passw", "credential"]
        .iter()
        .any(|word| {
            lower.match_indices(word).any(|(at, _)| {
                let rest = lower[at + word.len()..].trim_start_matches(|ch: char| {
                    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
                });
                let rest = rest.trim_start_matches(['"', '\'']).trim_start();
                rest.starts_with(':') || rest.starts_with('=')
            })
        })
}

fn credential_url(line: &str) -> bool {
    line.match_indices("://").any(|(at, _)| {
        let start = line[..at]
            .char_indices()
            .rfind(|(_, ch)| !ch.is_ascii_alphanumeric() && !matches!(ch, '+' | '-' | '.'))
            .map_or(0, |(index, ch)| index + ch.len_utf8());
        let candidate = line[start..].split_whitespace().next().unwrap_or("");
        reqwest::Url::parse(candidate)
            .or_else(|_| {
                reqwest::Url::parse(candidate.trim_end_matches(['"', '\'', '>', ')', ']', '}']))
            })
            .is_ok_and(|url| {
                !url.username().is_empty()
                    || url.password().is_some()
                    || url.query_pairs().any(|(name, _)| {
                        let name = name.to_ascii_lowercase();
                        matches!(
                            name.as_str(),
                            "sig" | "signature" | "x-amz-signature" | "x-goog-signature"
                        ) || sensitive_assignment(&format!("{name}="))
                    })
            })
    })
}

fn sensitive_option(lower: &str) -> bool {
    lower.split_whitespace().any(|word| {
        let word = word.trim_matches(['"', '\'']);
        if word == "-u" {
            return true;
        }
        let Some(option) = word.strip_prefix("--") else {
            return false;
        };
        if matches!(option, "user" | "proxy-user") {
            return true;
        }
        option.split(['-', '_', '=']).any(|part| {
            matches!(
                part,
                "password"
                    | "passwd"
                    | "passphrase"
                    | "token"
                    | "secret"
                    | "key"
                    | "credential"
                    | "credentials"
            )
        })
    })
}

pub(super) fn log_text(body: &str) -> String {
    log_text_with_pem(body, &mut false)
}

pub(super) fn log_text_with_pem(body: &str, in_pem: &mut bool) -> String {
    strip_ansi(body)
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            // Hide whole PEM blocks, not only their BEGIN line.
            if lower.contains("-----begin") {
                *in_pem = true;
            }
            let pem = *in_pem;
            if lower.contains("-----end") {
                *in_pem = false;
            }
            if pem
                || sensitive_assignment(&lower)
                || sensitive_option(&lower)
                || credential_url(line)
                || [
                    "authorization",
                    "bearer ",
                    "ghp_",
                    "ghs_",
                    "ghu_",
                    "ghr_",
                    "github_pat_",
                    "private key",
                    "environment:",
                    "\"env\"",
                ]
                .iter()
                .any(|marker| lower.contains(marker))
            {
                "[Sensitive runtime output hidden]".into()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_migrated_activity_history_loads() {
        let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
        let events = events(&migrated.runtime_paths()).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].computer, "dev");
        assert_eq!(
            events[0].computer_id,
            crate::runtime_migration::vocabulary_tests::ID
        );
    }
    #[test]
    fn command_line_secret_options_stay_out_of_failure_history() {
        let dir = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&dir);
        for line in [
            "curl --user alice:synthetic-password https://example.test",
            "curl -u alice:synthetic-password https://example.test",
            "curl --proxy-user alice:synthetic-password https://example.test",
            "login --password synthetic-password",
            "client --api-key synthetic-key",
            "client --client-secret synthetic-secret",
            "client --access_token synthetic-token",
            "client --passphrase synthetic-passphrase",
        ] {
            let mut event = begin(&paths, "start", "dev", "computer-1").unwrap();
            finish(
                &paths,
                &mut event,
                &Err(RuntimeError::Failed {
                    operation: "Starting the computer".into(),
                    exit_code: Some(1),
                    detail: format!("connection failed\n{line}"),
                }),
            );
            for text in [
                fs::read_to_string(path(&paths)).unwrap(),
                serde_json::to_string(&read(&paths).unwrap()).unwrap(),
                serde_json::to_string(&failures(&paths).unwrap()).unwrap(),
            ] {
                assert!(
                    !text.contains("synthetic"),
                    "Command credentials escaped: {text}"
                );
                assert!(text.contains("connection failed"));
            }
        }
        for line in [
            "client --keyboard-layout us",
            "client --monkey banana",
            "client --output result",
            "curl --user-agent Silo https://example.test",
        ] {
            assert_eq!(log_text(line), line);
        }
    }
    #[test]
    fn log_text_hides_url_credentials_without_hiding_public_urls() {
        for line in [
            "fetch https://alice:synthetic-password@example.test/repo",
            "git clone 'https://synthetic-token@example.test/repo'",
            "connect(postgresql://alice:synthetic-password@localhost/db)",
            "remote=https://alice:synthetic%2Dpassword@example.test/repo",
            "https://:synthetic-password@example.test/repo",
            "connect('postgresql://alice:synthetic-password@localhost')",
            "fetch https://alice:synthetic'password@example.test/repo",
            "fetch https://alice:synthetic)password@example.test/repo",
            "fetch https://alice:synthetic-password@[::1]",
            "download https://example.test/blob?sv=2026-02-06&sp=r&sig=synthetic-signature",
            "fetch https://example.test/?%74oken=synthetic-token",
            "fetch https://example.test/?api%5Fkey=synthetic-key",
            "fetch https://example.test/?X-Amz-Signature=synthetic-signature",
            "fetch https://example.test/?X-Goog-Signature=synthetic-signature",
            "🚨https://example.test/?sig=synthetic-signature",
        ] {
            assert_eq!(
                log_text(line),
                "[Sensitive runtime output hidden]",
                "{line}"
            );
        }
        for line in [
            "fetch https://example.test/repo",
            "fetch https://example.test/team@main/repo",
            "fetch https://example.test/?contact=alice@example.test",
            "connection failed for alice@example.test",
            "fetch https://example.test/?signature_status=valid",
            "🚨https://example.test/public",
        ] {
            assert_eq!(log_text(line), line);
        }
    }
    #[test]
    fn log_text_hides_common_secret_assignments_and_pem_blocks() {
        for line in [
            "AWS_SECRET_ACCESS_KEY=abc",
            "api_key = abc",
            "Password: hunter2",
            "PASSWORD =x",
            "\"client_secret\": \"abc\"",
            "export GH_TOKEN=abc",
        ] {
            assert_eq!(
                log_text(line),
                "[Sensitive runtime output hidden]",
                "{line}"
            );
        }
        let pem = "before\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk\n-----END OPENSSH PRIVATE KEY-----\nafter";
        let text = log_text(pem);
        assert!(!text.contains("b3BlbnNzaC1rZXk"));
        assert!(text.starts_with("before\n") && text.ends_with("\nafter"));
        assert_eq!(log_text("Computer started in 2s"), "Computer started in 2s");
    }

    #[test]
    fn strip_ansi_removes_8bit_and_string_controls() {
        assert_eq!(strip_ansi("a\u{9b}31mb"), "ab");
        assert_eq!(strip_ansi("a\u{1b}P1;2|payload\u{1b}\\b"), "ab");
        assert_eq!(strip_ansi("a\u{1b}_apc\u{9c}b\u{1b}]0;title\u{7}c"), "abc");
        assert_eq!(
            strip_ansi("safe\rhidden\u{8}\u{8}x\tt\nn"),
            "safehiddenx\tt\nn"
        );
    }
    #[test]
    fn cancelled_action_is_persisted_as_cancelled_not_failed() {
        let dir = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&dir);
        let mut event = begin(&paths, "start", "dev", "computer-1").unwrap();
        finish(
            &paths,
            &mut event,
            &Err(RuntimeError::Cancelled {
                operation: "start dev".into(),
            }),
        );
        assert!(failures(&paths).unwrap().is_empty());
        let entry = &read(&paths).unwrap()[0];
        assert_eq!(entry["title"], "Start cancelled");
        assert_eq!(entry["tone"], "neutral");
        assert_eq!(entry["cancelled"], true);
    }

    #[test]
    fn unknown_or_excess_entries_do_not_block_journaling() {
        let dir = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&dir);
        let mut first = begin(&paths, "start", "dev", "computer-1").unwrap();
        finish(&paths, &mut first, &Ok(()));
        let mut entries: Vec<Value> =
            serde_json::from_slice(&fs::read(path(&paths)).unwrap()).unwrap();
        let mut unknown = entries[0].clone();
        unknown["action"] = "hibernate".into();
        unknown["id"] = "future".into();
        entries.push(unknown);
        while entries.len() <= LIMIT + 5 {
            let mut copy = entries[0].clone();
            copy["id"] = format!("old-{}", entries.len()).into();
            entries.push(copy);
        }
        fs::write(path(&paths), serde_json::to_vec(&entries).unwrap()).unwrap();
        let mut next = begin(&paths, "stop", "dev", "computer-1").unwrap();
        finish(&paths, &mut next, &Ok(()));
        let stored = events(&paths).unwrap();
        assert!(stored.len() <= LIMIT);
        assert!(stored.iter().all(|event| event.action != "hibernate"));
        assert!(stored.iter().any(|event| event.id == next.id));
    }
    #[test]
    fn lifecycle_failure_keeps_diagnostics_and_survives_read_until_success() {
        let dir = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&dir);
        let mut event = begin(&paths, "start", "dev", "computer-1").unwrap();
        finish(&paths, &mut event, &Err(RuntimeError::Failed {
            operation: "Starting the computer".into(),
            exit_code: Some(1),
            detail: "\u{1b}[31mlibkrunfw could not load: different Team IDs\u{1b}[0m\nTOKEN=private-value".into(),
        }));
        // The summary is one actionable line; the runtime's explanation is separate.
        let entry = &read(&paths).unwrap()[0];
        let detail = entry["detail"].as_str().unwrap();
        assert_eq!(detail, "Starting the computer: The runtime did not complete the operation. Check the computer state and retry.");
        let diagnostic = entry["diagnostic"].as_str().unwrap();
        assert!(diagnostic.starts_with("Exit code 1\n"), "{diagnostic}");
        assert!(diagnostic.contains("libkrunfw could not load: different Team IDs"));
        assert!(!diagnostic.contains("private-value"));
        assert!(!diagnostic.contains('\u{1b}'));
        let failure = serde_json::to_value(&failures(&paths).unwrap()["computer-1"]).unwrap();
        assert_eq!(
            failure["lifecycleFailure"],
            format!("Start failed: {detail}")
        );
        assert!(failure["lifecycleFailureDiagnostic"]
            .as_str()
            .unwrap()
            .contains("different Team IDs"));
        assert!(!failures(&paths)
            .unwrap()
            .contains_key("replacement-computer"));
        let mut retry = begin(&paths, "start", "dev", "computer-1").unwrap();
        finish(&paths, &mut retry, &Ok(()));
        assert!(!failures(&paths).unwrap().contains_key("computer-1"));
        assert!(read(&paths).unwrap().iter().any(|entry| entry["diagnostic"]
            .as_str()
            .is_some_and(|text| text.contains("different Team IDs"))));
    }

    #[test]
    fn failures_recorded_before_the_diagnostic_field_are_split_into_summary_and_detail() {
        let dir = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&dir);
        let mut event = begin(&paths, "stop", "dev", "computer-1").unwrap();
        event.completed = true;
        event.failure = Some("Stopping the computer (exit code 2): The runtime did not complete the operation.\nruntime said no".into());
        store(&paths, &event).unwrap();
        let failure = serde_json::to_value(&failures(&paths).unwrap()["computer-1"]).unwrap();
        assert_eq!(
            failure["lifecycleFailure"],
            "Stop failed: The computer action did not finish. Check its state and retry."
        );
        let diagnostic = failure["lifecycleFailureDiagnostic"].as_str().unwrap();
        assert!(diagnostic.contains("exit code 2") && diagnostic.contains("runtime said no"));
        assert_eq!(read(&paths).unwrap()[0]["diagnostic"], diagnostic);
    }

    #[test]
    fn durable_lifecycle_records_verified_results_without_raw_failure_output() {
        let dir = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&dir);
        let mut event = begin(&paths, "restart", "dev", "computer-1").unwrap();
        assert_eq!(read(&paths).unwrap()[0]["status"], "running");
        finish(
            &paths,
            &mut event,
            &Err(RuntimeError::Failed {
                operation: "Restarting the computer".into(),
                exit_code: Some(1),
                detail: "TOKEN=private-value".into(),
            }),
        );
        let values = read(&paths).unwrap();
        assert_eq!(values[0]["tone"], "danger");
        assert!(!serde_json::to_string(&values)
            .unwrap()
            .contains("private-value"));
        assert_eq!(values[0]["status"], "completed");
    }
    #[test]
    fn unfinished_activity_from_a_reused_pid_is_interrupted_until_resumed() {
        for legacy in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let paths = super::super::tests::paths(&dir);
            let mut event = begin(&paths, "start", "dev", "computer-1").unwrap();
            assert_eq!(read(&paths).unwrap()[0]["status"], "running");
            let mut saved = serde_json::to_value(&event).unwrap();
            if legacy {
                saved.as_object_mut().unwrap().remove("processSession");
            } else {
                saved["processSession"] = uuid::Uuid::nil().to_string().into();
            }
            fs::write(
                path(&paths),
                serde_json::to_vec(&vec![saved.clone()]).unwrap(),
            )
            .unwrap();
            let interrupted = read(&paths).unwrap();
            assert_eq!(interrupted[0]["status"], "completed");
            assert_eq!(interrupted[0]["tone"], "warning");
            event = serde_json::from_value(saved).unwrap();
            resume(&paths, &mut event, "computer-1");
            assert_eq!(read(&paths).unwrap()[0]["status"], "running");
            finish(&paths, &mut event, &Ok(()));
            assert_eq!(read(&paths).unwrap()[0]["tone"], "success");
        }
    }

    #[test]
    fn unfinished_previous_process_is_not_reported_running_or_successful() {
        let dir = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&dir);
        let mut event = begin(&paths, "stop", "dev", "computer-1").unwrap();
        event.process = 0;
        store(&paths, &event).unwrap();
        let values = read(&paths).unwrap();
        assert_eq!(values[0]["tone"], "warning");
        assert_eq!(values[0]["status"], "completed");
    }

    #[test]
    fn oversized_history_is_preserved_when_its_bounded_prefix_is_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&dir);
        let history = path(&paths);
        let mut original = b"[]".to_vec();
        original.resize(MAX_OUTPUT_BYTES as usize, b' ');
        original.extend_from_slice(b"unfinished trailing history");
        fs::write(&history, &original).unwrap();

        let mut event = begin(&paths, "stop", "dev", "computer-1").unwrap();
        finish(&paths, &mut event, &Ok(()));
        assert!(fs::read(&history).unwrap() == original);
        assert!(read(&paths).unwrap().iter().any(|entry| {
            entry["id"] == "computer-history-unavailable" && entry["tone"] == "warning"
        }));
        // A complete valid file exactly at the limit remains writable.
        fs::write(&history, &original[..MAX_OUTPUT_BYTES as usize]).unwrap();
        let next = begin(&paths, "start", "dev", "computer-1").unwrap();
        assert!(events(&paths)
            .unwrap()
            .iter()
            .any(|event| event.id == next.id));
    }

    #[test]
    fn acknowledging_a_crash_does_not_fail_when_activity_history_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&dir);
        let history = path(&paths);
        let original = b"{unfinished activity history";
        fs::write(&history, original).unwrap();

        acknowledge_failure(&paths, "computer-1").unwrap();
        assert_eq!(fs::read(&history).unwrap(), original);
        assert!(read(&paths).unwrap().iter().any(|entry| {
            entry["id"] == "computer-history-unavailable" && entry["tone"] == "warning"
        }));
    }

    #[test]
    fn acknowledging_a_failure_preserves_activity_but_clears_overview() {
        let dir = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&dir);
        let mut event = begin(&paths, "start", "dev", "computer-1").unwrap();
        finish(
            &paths,
            &mut event,
            &Err(RuntimeError::Invalid("Boot failed".into())),
        );
        acknowledge_failure(&paths, "replacement-computer").unwrap();
        assert!(failures(&paths).unwrap().contains_key("computer-1"));
        acknowledge_failure(&paths, "computer-1").unwrap();
        assert!(failures(&paths).unwrap().is_empty());
        assert_eq!(read(&paths).unwrap()[0]["detail"], "Boot failed");
    }

    #[test]
    fn detailed_failure_retention_stays_within_the_journal_read_limit() {
        let dir = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&dir);
        let mut event = begin(&paths, "start", "dev", "computer-1").unwrap();
        event.completed = true;
        event.failure = Some("failure detail ".repeat(500));
        let entries: Vec<_> = (0..133)
            .map(|index| {
                let mut entry = event.clone();
                entry.id = index.to_string();
                entry
            })
            .collect();
        fs::write(path(&paths), serde_json::to_vec(&entries).unwrap()).unwrap();
        finish(
            &paths,
            &mut event,
            &Err(RuntimeError::Failed {
                operation: "Starting the computer".into(),
                exit_code: Some(1),
                detail: "💥".repeat(20_000),
            }),
        );
        assert!(fs::metadata(path(&paths)).unwrap().len() <= MAX_OUTPUT_BYTES);
        assert!(events(&paths).unwrap().len() < 134);
        let stored = events(&paths).unwrap().last().unwrap().clone();
        assert!(!stored.failure.as_ref().unwrap().contains('💥'));
        assert!(stored
            .diagnostic
            .as_ref()
            .unwrap()
            .ends_with("[Diagnostic truncated]"));
    }
}
