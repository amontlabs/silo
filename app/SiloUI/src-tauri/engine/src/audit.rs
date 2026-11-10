//! The audit log: one row for every call that can change something (a command, a job or a
//! stream) and for every call the policy refused or held for approval, with who made it.
//! Parameters and results are never stored. The log is its own SQLite file
//! (`<app data>/audit.sqlite3`) with its own schema version, so the job registry's database is
//! left alone.
//!
//! Rows are written by a background thread, so recording never waits on the disk and a write
//! error never fails a call: the first one is reported once and the row is dropped. Rows older
//! than 90 days, and the oldest beyond 200,000, are trimmed.

use std::{
    collections::HashMap,
    fs,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, LazyLock, Mutex,
    },
    time::Duration,
};

use rusqlite::{params_from_iter, types::Value as SqlValue, Connection, OptionalExtension};
use silo_api::methods::audit::{
    AuditCallerKind, AuditEntry, AuditListParams, AuditListResult, AuditOutcome,
};

pub const FILE_NAME: &str = "audit.sqlite3";
const SCHEMA_VERSION: i64 = 1;
const QUEUE: usize = 4096;
const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 500;
/// Trims run after this many rows are written.
const TRIM_EVERY: u64 = 500;

/// How long and how many rows the log keeps.
#[derive(Clone, Copy, Debug)]
pub struct Retention {
    pub max_age: Duration,
    pub max_rows: u64,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            max_age: Duration::from_secs(90 * 24 * 60 * 60),
            max_rows: 200_000,
        }
    }
}

/// One call to record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Milliseconds since the Unix epoch.
    pub at_ms: i64,
    pub caller_kind: AuditCallerKind,
    pub caller_id: Option<String>,
    pub client: Option<String>,
    pub method: String,
    pub computer_id: Option<String>,
    pub outcome: AuditOutcome,
    pub error_code: Option<String>,
    pub job_id: Option<String>,
    pub approval_id: Option<String>,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

enum Message {
    Row(Box<Row>),
    Flush(SyncSender<()>),
}

/// The calls a reader may see: every row, or only rows about these computers (rows whose
/// target, or whose calling computer, is one of them).
pub enum Scope<'a> {
    All,
    Computers(&'a std::collections::BTreeSet<String>),
}

pub struct AuditLog {
    path: PathBuf,
    sender: SyncSender<Message>,
    warned: Arc<AtomicBool>,
    reader: Mutex<Option<Connection>>,
}

static SHARED: LazyLock<Mutex<HashMap<PathBuf, Arc<AuditLog>>>> = LazyLock::new(Default::default);

fn warn_once(warned: &AtomicBool, message: &str) {
    if !warned.swap(true, Ordering::Relaxed) {
        eprintln!("silo-engine: the audit log could not be written: {message}");
    }
}

fn kind_text(kind: AuditCallerKind) -> &'static str {
    match kind {
        AuditCallerKind::Owner => "owner",
        AuditCallerKind::Device => "device",
        AuditCallerKind::Computer => "computer",
    }
}

fn kind_from(text: &str) -> AuditCallerKind {
    match text {
        "device" => AuditCallerKind::Device,
        "computer" => AuditCallerKind::Computer,
        _ => AuditCallerKind::Owner,
    }
}

fn outcome_text(outcome: AuditOutcome) -> &'static str {
    match outcome {
        AuditOutcome::Ok => "ok",
        AuditOutcome::Error => "error",
        AuditOutcome::Refused => "refused",
        AuditOutcome::Asked => "asked",
    }
}

fn outcome_from(text: &str) -> AuditOutcome {
    match text {
        "ok" => AuditOutcome::Ok,
        "refused" => AuditOutcome::Refused,
        "asked" => AuditOutcome::Asked,
        _ => AuditOutcome::Error,
    }
}

fn rfc3339(at_ms: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(at_ms) * 1_000_000)
        .ok()
        .and_then(|at| {
            at.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_default()
}

/// Opens the database, creating it owner-only with the current schema, and refuses a file a
/// newer engine wrote.
fn open_connection(path: &Path) -> Result<Connection, String> {
    if let Some(directory) = path.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .map_err(|error| error.to_string())?;
    }
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .map_err(|error| error.to_string())?;
    let connection = Connection::open(path).map_err(|error| error.to_string())?;
    connection
        .busy_timeout(Duration::from_secs(2))
        .map_err(|error| error.to_string())?;
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS calls (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 at_ms INTEGER NOT NULL,
                 caller_kind TEXT NOT NULL,
                 caller_id TEXT,
                 client TEXT,
                 method TEXT NOT NULL,
                 computer_id TEXT,
                 outcome TEXT NOT NULL,
                 error_code TEXT,
                 job_id TEXT,
                 approval_id TEXT
             );
             CREATE INDEX IF NOT EXISTS calls_computer ON calls (computer_id, id);
             CREATE INDEX IF NOT EXISTS calls_caller ON calls (caller_id, id);
             CREATE INDEX IF NOT EXISTS calls_at ON calls (at_ms);",
        )
        .map_err(|error| error.to_string())?;
    let version: Option<i64> = connection
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    match version {
        None => {
            connection
                .execute(
                    "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
                    [SCHEMA_VERSION],
                )
                .map_err(|error| error.to_string())?;
        }
        Some(SCHEMA_VERSION) => {}
        Some(other) => {
            return Err(format!(
                "{} has schema version {other}, which this engine does not know.",
                path.display()
            ))
        }
    }
    Ok(connection)
}

fn insert(connection: &Connection, row: &Row) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO calls (at_ms, caller_kind, caller_id, client, method, computer_id,
                 outcome, error_code, job_id, approval_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                row.at_ms,
                kind_text(row.caller_kind),
                row.caller_id,
                row.client,
                row.method,
                row.computer_id,
                outcome_text(row.outcome),
                row.error_code,
                row.job_id,
                row.approval_id,
            ],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn trim(connection: &Connection, retention: Retention) -> Result<(), String> {
    let age = i64::try_from(retention.max_age.as_millis()).unwrap_or(i64::MAX);
    connection
        .execute(
            "DELETE FROM calls WHERE at_ms < ?1",
            [now_ms().saturating_sub(age)],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "DELETE FROM calls WHERE id <= (SELECT MAX(id) FROM calls) - ?1",
            [i64::try_from(retention.max_rows).unwrap_or(i64::MAX)],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn write_loop(path: PathBuf, retention: Retention, warned: Arc<AtomicBool>, queue: Receiver<Message>) {
    let mut connection: Option<Connection> = None;
    let mut written: u64 = 0;
    let mut trimmed = false;
    for message in queue {
        match message {
            Message::Flush(done) => {
                let _ = done.send(());
            }
            Message::Row(row) => {
                if connection.is_none() {
                    match open_connection(&path) {
                        Ok(opened) => connection = Some(opened),
                        Err(error) => {
                            warn_once(&warned, &error);
                            continue;
                        }
                    }
                }
                let Some(open) = connection.as_ref() else {
                    continue;
                };
                if let Err(error) = insert(open, &row) {
                    warn_once(&warned, &error);
                    connection = None;
                    continue;
                }
                written += 1;
                if !trimmed || written % TRIM_EVERY == 0 {
                    trimmed = true;
                    if let Err(error) = trim(open, retention) {
                        warn_once(&warned, &error);
                    }
                }
            }
        }
    }
}

impl AuditLog {
    /// The log of the engine whose data folder is `app_data`, shared by every caller in this
    /// process.
    pub fn shared(app_data: &Path) -> Arc<AuditLog> {
        let path = app_data.join(FILE_NAME);
        let mut shared = SHARED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shared
            .entry(path.clone())
            .or_insert_with(|| Arc::new(Self::open(path, Retention::default())))
            .clone()
    }

    /// A log at `path` with its own writer thread.
    pub fn open(path: PathBuf, retention: Retention) -> Self {
        let (sender, queue) = mpsc::sync_channel(QUEUE);
        let warned = Arc::new(AtomicBool::new(false));
        let writer = {
            let (path, warned) = (path.clone(), warned.clone());
            std::thread::Builder::new()
                .name("silo-audit".into())
                .spawn(move || write_loop(path, retention, warned, queue))
        };
        if let Err(error) = writer {
            warn_once(&warned, &error.to_string());
        }
        Self {
            path,
            sender,
            warned,
            reader: Mutex::new(None),
        }
    }

    /// Queues a row. Never blocks; a full queue or a stopped writer drops the row.
    pub fn record(&self, row: Row) {
        match self.sender.try_send(Message::Row(Box::new(row))) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => warn_once(&self.warned, "too many calls at once"),
            Err(TrySendError::Disconnected(_)) => {
                warn_once(&self.warned, "the writer stopped")
            }
        }
    }

    /// Waits, up to `timeout`, until every row queued so far is written.
    pub fn flush(&self, timeout: Duration) {
        let (done, wait) = mpsc::sync_channel(1);
        if self.sender.try_send(Message::Flush(done)).is_ok() {
            let _ = wait.recv_timeout(timeout);
        }
    }

    /// One page of rows, newest first.
    pub fn list(&self, params: &AuditListParams, scope: Scope<'_>) -> Result<AuditListResult, String> {
        self.flush(Duration::from_secs(2));
        let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let mut conditions: Vec<String> = Vec::new();
        let mut values: Vec<SqlValue> = Vec::new();
        if let Some(computer) = &params.computer_id {
            conditions.push("computer_id = ?".into());
            values.push(SqlValue::Text(computer.clone()));
        }
        if let Some(kind) = params.caller_kind {
            conditions.push("caller_kind = ?".into());
            values.push(SqlValue::Text(kind_text(kind).into()));
        }
        if let Some(before) = params.before_id {
            conditions.push("id < ?".into());
            values.push(SqlValue::Integer(i64::try_from(before).unwrap_or(i64::MAX)));
        }
        if let Scope::Computers(visible) = scope {
            if visible.is_empty() {
                return Ok(AuditListResult {
                    entries: Vec::new(),
                    next_before_id: None,
                });
            }
            let marks = vec!["?"; visible.len()].join(", ");
            conditions.push(format!(
                "(computer_id IN ({marks}) OR (caller_kind = 'computer' AND caller_id IN ({marks})))"
            ));
            for _ in 0..2 {
                values.extend(visible.iter().map(|id| SqlValue::Text(id.clone())));
            }
        }
        let filter = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        values.push(SqlValue::Integer(i64::from(limit) + 1));
        let sql = format!(
            "SELECT id, at_ms, caller_kind, caller_id, client, method, computer_id, outcome,
                    error_code, job_id, approval_id
             FROM calls {filter} ORDER BY id DESC LIMIT ?"
        );
        let mut reader = self
            .reader
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if reader.is_none() {
            *reader = Some(open_connection(&self.path)?);
        }
        let connection = reader.as_ref().expect("opened above");
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| error.to_string())?;
        let mut entries: Vec<AuditEntry> = statement
            .query_map(params_from_iter(values), |row| {
                let id: i64 = row.get(0)?;
                let at_ms: i64 = row.get(1)?;
                let caller_kind: String = row.get(2)?;
                let outcome: String = row.get(7)?;
                Ok(AuditEntry {
                    id: u64::try_from(id).unwrap_or_default(),
                    at: rfc3339(at_ms),
                    caller_kind: kind_from(&caller_kind),
                    caller_id: row.get(3)?,
                    client: row.get(4)?,
                    method: row.get(5)?,
                    computer_id: row.get(6)?,
                    outcome: outcome_from(&outcome),
                    error_code: row.get(8)?,
                    job_id: row.get(9)?,
                    approval_id: row.get(10)?,
                })
            })
            .and_then(Iterator::collect)
            .map_err(|error| error.to_string())?;
        let more = entries.len() > limit as usize;
        entries.truncate(limit as usize);
        let next_before_id = more.then(|| entries.last().map(|entry| entry.id)).flatten();
        Ok(AuditListResult {
            entries,
            next_before_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn row(method: &str, kind: AuditCallerKind, caller: Option<&str>, computer: Option<&str>) -> Row {
        Row {
            at_ms: now_ms(),
            caller_kind: kind,
            caller_id: caller.map(str::to_owned),
            client: Some("test".into()),
            method: method.into(),
            computer_id: computer.map(str::to_owned),
            outcome: AuditOutcome::Ok,
            error_code: None,
            job_id: None,
            approval_id: None,
        }
    }

    fn all() -> AuditListParams {
        AuditListParams {
            computer_id: None,
            caller_kind: None,
            before_id: None,
            limit: None,
        }
    }

    #[test]
    fn rows_are_listed_newest_first_in_pages_and_the_file_is_owner_only() {
        let directory = tempfile::tempdir().unwrap();
        let log = AuditLog::open(directory.path().join(FILE_NAME), Retention::default());
        for number in 0..5 {
            log.record(row(
                &format!("m{number}"),
                AuditCallerKind::Owner,
                None,
                Some("c1"),
            ));
        }
        let first = log
            .list(&AuditListParams { limit: Some(2), ..all() }, Scope::All)
            .unwrap();
        assert_eq!(
            first.entries.iter().map(|e| e.method.as_str()).collect::<Vec<_>>(),
            ["m4", "m3"]
        );
        let next = log
            .list(
                &AuditListParams {
                    limit: Some(2),
                    before_id: first.next_before_id,
                    ..all()
                },
                Scope::All,
            )
            .unwrap();
        assert_eq!(next.entries[0].method, "m2");
        let last = log
            .list(&AuditListParams { limit: Some(10), ..all() }, Scope::All)
            .unwrap();
        assert_eq!(last.entries.len(), 5);
        assert!(last.next_before_id.is_none());
        assert!(!last.entries[0].at.is_empty());
        let mode = fs::metadata(directory.path().join(FILE_NAME)).unwrap();
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o777,
            0o600
        );
    }

    #[test]
    fn filters_and_a_computers_scope_cut_the_rows() {
        let directory = tempfile::tempdir().unwrap();
        let log = AuditLog::open(directory.path().join(FILE_NAME), Retention::default());
        log.record(row("a", AuditCallerKind::Owner, None, Some("main")));
        log.record(row("b", AuditCallerKind::Computer, Some("main"), Some("fork")));
        log.record(row("c", AuditCallerKind::Owner, None, Some("stranger")));
        log.record(row("d", AuditCallerKind::Computer, Some("stranger"), None));
        let visible: BTreeSet<String> = ["main".to_owned(), "fork".to_owned()].into();
        let seen = log.list(&all(), Scope::Computers(&visible)).unwrap();
        assert_eq!(
            seen.entries.iter().map(|e| e.method.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
        let computers = log
            .list(
                &AuditListParams {
                    caller_kind: Some(AuditCallerKind::Computer),
                    ..all()
                },
                Scope::All,
            )
            .unwrap();
        assert_eq!(computers.entries.len(), 2);
        let about = log
            .list(
                &AuditListParams {
                    computer_id: Some("stranger".into()),
                    ..all()
                },
                Scope::Computers(&visible),
            )
            .unwrap();
        assert!(about.entries.is_empty());
    }

    #[test]
    fn old_rows_and_rows_beyond_the_cap_are_trimmed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        let log = AuditLog::open(
            path,
            Retention {
                max_age: Duration::from_secs(3600),
                max_rows: 3,
            },
        );
        let mut old = row("old", AuditCallerKind::Owner, None, None);
        old.at_ms -= 2 * 3600 * 1000;
        log.record(old);
        // The trim runs when the row count reaches a multiple of TRIM_EVERY.
        for number in 0..(TRIM_EVERY as usize - 1) {
            log.record(row(&format!("n{number}"), AuditCallerKind::Owner, None, None));
        }
        let rows = log
            .list(&AuditListParams { limit: Some(500), ..all() }, Scope::All)
            .unwrap();
        assert_eq!(rows.entries.len(), 3);
        assert!(rows.entries.iter().all(|entry| entry.method != "old"));
    }

    #[test]
    fn a_file_from_a_newer_engine_is_left_alone_and_recording_still_returns() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        {
            let connection = open_connection(&path).unwrap();
            connection
                .execute("UPDATE meta SET value = 99 WHERE key = 'schema_version'", [])
                .unwrap();
        }
        let log = AuditLog::open(path, Retention::default());
        log.record(row("x", AuditCallerKind::Owner, None, None));
        log.flush(Duration::from_secs(2));
        assert!(log.list(&all(), Scope::All).is_err());
    }
}
