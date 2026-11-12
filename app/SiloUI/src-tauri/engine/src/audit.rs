//! The audit log: one row for every call that can change something (a command, a job or a
//! stream) and for every call the policy refused or held for approval, with who made it.
//! Parameters and results are never stored. The log is its own SQLite file
//! (`<app data>/audit.sqlite3`) with its own schema version, so the job registry's database is
//! left alone.
//!
//! Rows are written by a background thread, so recording never waits on the disk and a write
//! error never fails a call: the first one is reported once and the row is dropped. A call that
//! repeats the caller's newest row (same method, outcome, error and computer) adds to that
//! row's count instead of making a new one. Rows older than 90 days are trimmed; beyond that
//! the owner's and devices' rows keep the newest 200,000, and each computer's own calls the
//! newest 20,000, so a computer can never push out anyone else's rows.
//!
//! A computer's rows wait in a small buffer of their own, apart from the queue of the owner's
//! and devices' rows. A repeat of the computer's newest waiting row is counted there before
//! anything is queued, each computer may have only so many rows waiting, and the rows that
//! find the buffer full are counted into one summary row per computer, so computer traffic
//! cannot take a place in the owner's queue.

use std::{
    collections::{BTreeMap, HashMap},
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
const SCHEMA_VERSION: i64 = 2;
const QUEUE: usize = 4096;
/// Rows of all computers together that may wait to be written.
const COMPUTER_WAITING: usize = 1024;
/// Rows of one computer that may wait to be written.
const PER_COMPUTER_WAITING: usize = 64;
/// How long the writer waits for a message before it looks at the computers' rows.
const POLL: Duration = Duration::from_millis(250);
/// The method of the row that counts the rows of a computer that were not recorded.
pub const DROPPED_METHOD: &str = "audit.dropped";
pub const DROPPED_ERROR: &str = "audit_rows_dropped";
const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 500;
/// Trims run after this many rows are written.
const TRIM_EVERY: u64 = 500;

/// How long and how many rows the log keeps.
#[derive(Clone, Copy, Debug)]
pub struct Retention {
    pub max_age: Duration,
    /// Rows kept for the owner and for devices.
    pub max_rows: u64,
    /// Rows kept for each computer's own calls.
    pub max_computer_rows: u64,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            max_age: Duration::from_secs(90 * 24 * 60 * 60),
            max_rows: 200_000,
            max_computer_rows: 20_000,
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
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

enum Message {
    Row(Box<Row>),
    /// Computer rows are waiting in the buffer.
    Wake,
    Flush(SyncSender<()>),
}

/// A computer's row waiting to be written, with the calls it stands for.
struct Waiting {
    row: Row,
    count: u64,
    last_at: i64,
}

/// What the rows of a computer's agent wait in: a bounded buffer in which a repeat of a
/// computer's newest waiting row is counted instead of added.
#[derive(Default)]
struct ComputerRows {
    next: u64,
    waiting: BTreeMap<u64, Waiting>,
    newest: HashMap<Option<String>, u64>,
    per_computer: HashMap<Option<String>, usize>,
    /// The rows each computer lost to a full buffer and the time of the latest.
    dropped: HashMap<Option<String>, (u64, i64)>,
}

fn same_call(a: &Row, b: &Row) -> bool {
    a.method == b.method
        && a.outcome == b.outcome
        && a.error_code == b.error_code
        && a.computer_id == b.computer_id
        && a.client == b.client
        && a.approval_id == b.approval_id
}

impl ComputerRows {
    fn add(&mut self, row: Row) {
        let caller = row.caller_id.clone();
        if let Some(waiting) = self
            .newest
            .get(&caller)
            .and_then(|seq| self.waiting.get_mut(seq))
        {
            if same_call(&waiting.row, &row) {
                waiting.count += 1;
                waiting.last_at = row.at_ms;
                return;
            }
        }
        let held = self.per_computer.get(&caller).copied().unwrap_or(0);
        if self.waiting.len() >= COMPUTER_WAITING || held >= PER_COMPUTER_WAITING {
            // A flood of ids must not grow the summary either: past one entry per place in
            // the buffer the rest are counted together.
            let key = if self.dropped.contains_key(&caller) || self.dropped.len() < COMPUTER_WAITING
            {
                caller
            } else {
                None
            };
            let lost = self.dropped.entry(key).or_insert((0, row.at_ms));
            lost.0 += 1;
            lost.1 = row.at_ms;
            return;
        }
        let seq = self.next;
        self.next += 1;
        *self.per_computer.entry(caller.clone()).or_default() += 1;
        self.newest.insert(caller, seq);
        let last_at = row.at_ms;
        self.waiting.insert(
            seq,
            Waiting {
                row,
                count: 1,
                last_at,
            },
        );
    }

    /// Everything waiting, oldest first, then a summary row for each computer that lost rows.
    fn take(&mut self) -> Vec<Waiting> {
        let mut rows: Vec<Waiting> = std::mem::take(&mut self.waiting).into_values().collect();
        self.newest.clear();
        self.per_computer.clear();
        for (caller, (count, last_at)) in std::mem::take(&mut self.dropped) {
            rows.push(Waiting {
                row: Row {
                    at_ms: last_at,
                    caller_kind: AuditCallerKind::Computer,
                    caller_id: caller,
                    client: None,
                    method: DROPPED_METHOD.to_owned(),
                    computer_id: None,
                    outcome: AuditOutcome::Error,
                    error_code: Some(DROPPED_ERROR.to_owned()),
                    job_id: None,
                    approval_id: None,
                },
                count,
                last_at,
            });
        }
        rows
    }
}

/// What the recording side and the writer thread share.
#[derive(Default)]
struct Shared {
    computers: Mutex<ComputerRows>,
    /// A wake message is queued and the writer has not looked at the buffer since.
    woken: AtomicBool,
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
    shared: Arc<Shared>,
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
                 approval_id TEXT,
                 count INTEGER NOT NULL DEFAULT 1,
                 last_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS calls_computer ON calls (computer_id, id);
             CREATE INDEX IF NOT EXISTS calls_caller ON calls (caller_id, id);
             CREATE INDEX IF NOT EXISTS calls_at ON calls (at_ms);",
        )
        .map_err(|error| error.to_string())?;
    let read_version = || -> Result<Option<i64>, String> {
        connection
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())
    };
    match read_version()? {
        None => {
            connection
                .execute(
                    "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
                    [SCHEMA_VERSION],
                )
                .map_err(|error| error.to_string())?;
        }
        Some(SCHEMA_VERSION) => {}
        // Version 1 had no count of repeated calls.
        Some(1) => {
            let upgraded = connection.execute_batch(
                "BEGIN IMMEDIATE;
                 ALTER TABLE calls ADD COLUMN count INTEGER NOT NULL DEFAULT 1;
                 ALTER TABLE calls ADD COLUMN last_at INTEGER;
                 UPDATE meta SET value = 2 WHERE key = 'schema_version';
                 COMMIT;",
            );
            if let Err(error) = upgraded {
                let _ = connection.execute_batch("ROLLBACK");
                // The reader and the writer may both open the old file; one of them upgraded it.
                if read_version()? != Some(SCHEMA_VERSION) {
                    return Err(error.to_string());
                }
            }
        }
        Some(other) => {
            return Err(format!(
                "{} has schema version {other}, which this engine does not know.",
                path.display()
            ))
        }
    }
    Ok(connection)
}

/// Counts `count` calls, the latest at `last_at`, in the caller's newest row when `row` repeats
/// it, else adds a row.
fn insert(connection: &Connection, row: &Row, count: u64, last_at: i64) -> Result<(), String> {
    let count = i64::try_from(count).unwrap_or(i64::MAX);
    let caller = rusqlite::params![kind_text(row.caller_kind), row.caller_id];
    let newest: Option<(
        i64,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = connection
        .query_row(
            "SELECT id, method, outcome, error_code, computer_id, client, approval_id
                 FROM calls WHERE caller_kind = ?1 AND caller_id IS ?2
                 ORDER BY id DESC LIMIT 1",
            caller,
            |found| {
                Ok((
                    found.get(0)?,
                    found.get(1)?,
                    found.get(2)?,
                    found.get(3)?,
                    found.get(4)?,
                    found.get(5)?,
                    found.get(6)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some((id, method, outcome, error_code, computer_id, client, approval_id)) = newest {
        let repeats = method == row.method
            && outcome == outcome_text(row.outcome)
            && error_code == row.error_code
            && computer_id == row.computer_id
            && client == row.client
            && approval_id == row.approval_id;
        if repeats {
            return connection
                .execute(
                    "UPDATE calls SET count = count + ?3, last_at = ?2 WHERE id = ?1",
                    rusqlite::params![id, last_at, count],
                )
                .map(|_| ())
                .map_err(|error| error.to_string());
        }
    }
    connection
        .execute(
            "INSERT INTO calls (at_ms, caller_kind, caller_id, client, method, computer_id,
                 outcome, error_code, job_id, approval_id, count, last_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
                count,
                last_at,
            ],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn trim(connection: &Connection, retention: Retention) -> Result<(), String> {
    let failed = |error: rusqlite::Error| error.to_string();
    let age = i64::try_from(retention.max_age.as_millis()).unwrap_or(i64::MAX);
    connection
        .execute(
            "DELETE FROM calls WHERE at_ms < ?1",
            [now_ms().saturating_sub(age)],
        )
        .map_err(failed)?;
    // The owner's and devices' rows are counted apart from every computer's.
    connection
        .execute(
            "DELETE FROM calls WHERE caller_kind <> 'computer' AND id <= COALESCE(
                 (SELECT id FROM calls WHERE caller_kind <> 'computer'
                  ORDER BY id DESC LIMIT 1 OFFSET ?1), 0)",
            [i64::try_from(retention.max_rows).unwrap_or(i64::MAX)],
        )
        .map_err(failed)?;
    let cap = i64::try_from(retention.max_computer_rows).unwrap_or(i64::MAX);
    let mut over = connection
        .prepare(
            "SELECT caller_id FROM calls WHERE caller_kind = 'computer'
             GROUP BY caller_id HAVING COUNT(*) > ?1",
        )
        .map_err(failed)?;
    let computers: Vec<Option<String>> = over
        .query_map([cap], |found| found.get(0))
        .and_then(Iterator::collect)
        .map_err(failed)?;
    for computer in computers {
        connection
            .execute(
                "DELETE FROM calls WHERE caller_kind = 'computer' AND caller_id IS ?1 AND id <= COALESCE(
                     (SELECT id FROM calls WHERE caller_kind = 'computer' AND caller_id IS ?1
                      ORDER BY id DESC LIMIT 1 OFFSET ?2), 0)",
                rusqlite::params![computer, cap],
            )
            .map_err(failed)?;
    }
    Ok(())
}

/// The thread that writes the rows, owner and device rows from the queue and computer rows
/// from the buffer.
struct Writer {
    path: PathBuf,
    retention: Retention,
    warned: Arc<AtomicBool>,
    shared: Arc<Shared>,
    connection: Option<Connection>,
    written: u64,
    trimmed: bool,
}

impl Writer {
    fn write(&mut self, row: &Row, count: u64, last_at: i64) {
        if self.connection.is_none() {
            match open_connection(&self.path) {
                Ok(opened) => self.connection = Some(opened),
                Err(error) => {
                    warn_once(&self.warned, &error);
                    return;
                }
            }
        }
        let Some(open) = self.connection.as_ref() else {
            return;
        };
        if let Err(error) = insert(open, row, count, last_at) {
            warn_once(&self.warned, &error);
            self.connection = None;
            return;
        }
        self.written += 1;
        if !self.trimmed || self.written % TRIM_EVERY == 0 {
            self.trimmed = true;
            if let Err(error) = trim(open, self.retention) {
                warn_once(&self.warned, &error);
            }
        }
    }

    /// Writes the computers' waiting rows.
    fn drain_computers(&mut self) {
        self.shared.woken.store(false, Ordering::SeqCst);
        let rows = self
            .shared
            .computers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        for waiting in rows {
            self.write(&waiting.row, waiting.count, waiting.last_at);
        }
    }
}

fn write_loop(
    path: PathBuf,
    retention: Retention,
    warned: Arc<AtomicBool>,
    shared: Arc<Shared>,
    queue: Receiver<Message>,
) {
    let mut writer = Writer {
        path,
        retention,
        warned,
        shared,
        connection: None,
        written: 0,
        trimmed: false,
    };
    loop {
        match queue.recv_timeout(POLL) {
            Ok(Message::Row(row)) => writer.write(&row, 1, row.at_ms),
            Ok(Message::Wake) | Err(mpsc::RecvTimeoutError::Timeout) => writer.drain_computers(),
            Ok(Message::Flush(done)) => {
                writer.drain_computers();
                let _ = done.send(());
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                writer.drain_computers();
                return;
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
        let shared = Arc::new(Shared::default());
        let writer = {
            let (path, warned, shared) = (path.clone(), warned.clone(), shared.clone());
            std::thread::Builder::new()
                .name("silo-audit".into())
                .spawn(move || write_loop(path, retention, warned, shared, queue))
        };
        if let Err(error) = writer {
            warn_once(&warned, &error.to_string());
        }
        Self {
            path,
            sender,
            shared,
            warned,
            reader: Mutex::new(None),
        }
    }

    /// Queues a row. Never blocks. A computer's row waits in the computers' buffer, where a
    /// repeat is counted and a row that finds the buffer full is counted in the computer's
    /// summary row; an owner's or device's row that finds the queue full, or a stopped
    /// writer, is dropped.
    pub fn record(&self, row: Row) {
        if row.caller_kind == AuditCallerKind::Computer {
            self.shared
                .computers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .add(row);
            if !self.shared.woken.swap(true, Ordering::SeqCst)
                && self.sender.try_send(Message::Wake).is_err()
            {
                // The writer looks at the buffer again within a moment of its own accord.
                self.shared.woken.store(false, Ordering::SeqCst);
            }
            return;
        }
        match self.sender.try_send(Message::Row(Box::new(row))) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => warn_once(&self.warned, "too many calls at once"),
            Err(TrySendError::Disconnected(_)) => warn_once(&self.warned, "the writer stopped"),
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
    pub fn list(
        &self,
        params: &AuditListParams,
        scope: Scope<'_>,
    ) -> Result<AuditListResult, String> {
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
        let visible_only = match scope {
            Scope::Computers(visible) => Some(visible),
            Scope::All => None,
        };
        if let Some(visible) = visible_only {
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
                    error_code, job_id, approval_id, count, last_at
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
                    count: u64::try_from(row.get::<_, i64>(11)?).unwrap_or(1).max(1),
                    last_at: row.get::<_, Option<i64>>(12)?.map(rfc3339),
                })
            })
            .and_then(Iterator::collect)
            .map_err(|error| error.to_string())?;
        // What a computer reads says nothing about who else called: no program names, no
        // device keys, and no other computer's id.
        if let Some(visible) = visible_only {
            for entry in &mut entries {
                entry.client = None;
                let known = entry
                    .caller_id
                    .as_ref()
                    .is_some_and(|id| visible.contains(id));
                if entry.caller_kind != AuditCallerKind::Computer || !known {
                    entry.caller_id = None;
                }
            }
        }
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

    fn row(
        method: &str,
        kind: AuditCallerKind,
        caller: Option<&str>,
        computer: Option<&str>,
    ) -> Row {
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
            .list(
                &AuditListParams {
                    limit: Some(2),
                    ..all()
                },
                Scope::All,
            )
            .unwrap();
        assert_eq!(
            first
                .entries
                .iter()
                .map(|e| e.method.as_str())
                .collect::<Vec<_>>(),
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
            .list(
                &AuditListParams {
                    limit: Some(10),
                    ..all()
                },
                Scope::All,
            )
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
        log.record(row(
            "b",
            AuditCallerKind::Computer,
            Some("main"),
            Some("fork"),
        ));
        log.record(row("c", AuditCallerKind::Owner, None, Some("stranger")));
        log.record(row("d", AuditCallerKind::Computer, Some("stranger"), None));
        let visible: BTreeSet<String> = ["main".to_owned(), "fork".to_owned()].into();
        let seen = log.list(&all(), Scope::Computers(&visible)).unwrap();
        assert_eq!(
            seen.entries
                .iter()
                .map(|e| e.method.as_str())
                .collect::<Vec<_>>(),
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
                max_computer_rows: 3,
            },
        );
        let mut old = row("old", AuditCallerKind::Owner, None, None);
        old.at_ms -= 2 * 3600 * 1000;
        log.record(old);
        // The trim runs when the row count reaches a multiple of TRIM_EVERY.
        for number in 0..(TRIM_EVERY as usize - 1) {
            log.record(row(
                &format!("n{number}"),
                AuditCallerKind::Owner,
                None,
                None,
            ));
        }
        let rows = log
            .list(
                &AuditListParams {
                    limit: Some(500),
                    ..all()
                },
                Scope::All,
            )
            .unwrap();
        assert_eq!(rows.entries.len(), 3);
        assert!(rows.entries.iter().all(|entry| entry.method != "old"));
    }

    fn kinds() -> AuditListParams {
        AuditListParams {
            limit: Some(500),
            ..all()
        }
    }

    fn trimmed_log(owner: u64, computer: u64) -> (tempfile::TempDir, AuditLog) {
        let directory = tempfile::tempdir().unwrap();
        let log = AuditLog::open(
            directory.path().join(FILE_NAME),
            Retention {
                max_age: Duration::from_secs(3600),
                max_rows: owner,
                max_computer_rows: computer,
            },
        );
        (directory, log)
    }

    #[test]
    fn a_computer_can_never_push_out_the_owners_rows() {
        let (_directory, log) = trimmed_log(4, 4);
        for number in 0..3 {
            log.record(row(
                &format!("owner{number}"),
                AuditCallerKind::Owner,
                None,
                None,
            ));
        }
        log.record(row("device", AuditCallerKind::Device, Some("key"), None));
        // The trim runs when the count of rows written reaches a multiple of TRIM_EVERY.
        for number in 0..TRIM_EVERY * 2 - 14 {
            log.record(row(
                &format!("noise{number}"),
                AuditCallerKind::Computer,
                Some("a"),
                None,
            ));
            if number % 16 == 15 {
                log.flush(Duration::from_secs(5));
            }
        }
        for number in 0..10 {
            log.record(row(
                &format!("other{number}"),
                AuditCallerKind::Computer,
                Some("b"),
                None,
            ));
        }
        let rows = log.list(&kinds(), Scope::All).unwrap();
        let methods = |kind: AuditCallerKind, caller: Option<&str>| {
            rows.entries
                .iter()
                .filter(|entry| entry.caller_kind == kind && entry.caller_id.as_deref() == caller)
                .map(|entry| entry.method.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(methods(AuditCallerKind::Owner, None).len(), 3);
        assert_eq!(methods(AuditCallerKind::Device, Some("key")).len(), 1);
        assert_eq!(methods(AuditCallerKind::Computer, Some("a")).len(), 4);
        assert_eq!(methods(AuditCallerKind::Computer, Some("b")).len(), 4);
        assert_eq!(
            methods(AuditCallerKind::Computer, Some("b"))[0],
            "other9",
            "each computer keeps its newest"
        );
    }

    #[test]
    fn owner_and_device_rows_are_trimmed_among_themselves() {
        let (_directory, log) = trimmed_log(3, 100);
        for number in 0..TRIM_EVERY {
            log.record(row(
                &format!("m{number}"),
                AuditCallerKind::Owner,
                None,
                None,
            ));
        }
        log.record(row("computer", AuditCallerKind::Computer, Some("a"), None));
        let rows = log.list(&kinds(), Scope::All).unwrap();
        assert_eq!(rows.entries.len(), 4);
        assert_eq!(rows.entries[1].method, format!("m{}", TRIM_EVERY - 1));
    }

    #[test]
    fn a_call_that_repeats_the_callers_newest_row_is_counted_in_it() {
        let (_directory, log) = trimmed_log(100, 100);
        let mut first = row("exec", AuditCallerKind::Computer, Some("a"), Some("a"));
        let begun = now_ms() - 60_000;
        first.at_ms = begun;
        let mut again = first.clone();
        again.at_ms = begun + 4_000;
        log.record(first);
        log.record(again.clone());
        log.record(again.clone());
        // Another caller's row between does not break a caller's run; its own different row does.
        log.record(row("exec", AuditCallerKind::Computer, Some("b"), Some("b")));
        log.record(again.clone());
        let mut refused = again.clone();
        refused.outcome = AuditOutcome::Refused;
        refused.error_code = Some("permission_denied".into());
        log.record(refused.clone());
        log.record(refused);
        let mut elsewhere = again.clone();
        elsewhere.computer_id = Some("c".into());
        log.record(elsewhere);
        let rows = log.list(&kinds(), Scope::All).unwrap();
        let shown: Vec<(String, u64)> = rows
            .entries
            .iter()
            .map(|entry| {
                (
                    format!(
                        "{}:{:?}:{:?}",
                        entry.method, entry.outcome, entry.computer_id
                    ),
                    entry.count,
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("exec:Ok:Some(\"c\")".to_owned(), 1),
                ("exec:Refused:Some(\"a\")".to_owned(), 2),
                ("exec:Ok:Some(\"b\")".to_owned(), 1),
                ("exec:Ok:Some(\"a\")".to_owned(), 4),
            ]
        );
        let counted = rows.entries.last().unwrap();
        assert_eq!(counted.at, rfc3339(begun));
        assert_eq!(
            counted.last_at.as_deref(),
            Some(rfc3339(begun + 4_000).as_str())
        );
    }

    #[test]
    fn a_version_one_file_gains_the_count_columns() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    &"CREATE TABLE meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
                     INSERT INTO meta VALUES ('schema_version', 1);
                     CREATE TABLE calls (
                         id INTEGER PRIMARY KEY AUTOINCREMENT, at_ms INTEGER NOT NULL,
                         caller_kind TEXT NOT NULL, caller_id TEXT, client TEXT,
                         method TEXT NOT NULL, computer_id TEXT, outcome TEXT NOT NULL,
                         error_code TEXT, job_id TEXT, approval_id TEXT);
                     INSERT INTO calls (at_ms, caller_kind, method, outcome)
                         VALUES ({}, 'owner', 'old', 'ok');"
                        .replace("{}", &(now_ms() - 1000).to_string()),
                )
                .unwrap();
        }
        let log = AuditLog::open(path, Retention::default());
        log.record(row("new", AuditCallerKind::Owner, None, None));
        let rows = log.list(&all(), Scope::All).unwrap();
        assert_eq!(
            rows.entries
                .iter()
                .map(|e| (e.method.as_str(), e.count))
                .collect::<Vec<_>>(),
            [("new", 1), ("old", 1)]
        );
    }

    #[test]
    fn what_a_computer_reads_names_no_program_and_no_device_key() {
        let (_directory, log) = trimmed_log(100, 100);
        log.record(row(
            "a",
            AuditCallerKind::Device,
            Some("SHA256:key"),
            Some("main"),
        ));
        log.record(row("b", AuditCallerKind::Owner, None, Some("main")));
        log.record(row(
            "c",
            AuditCallerKind::Computer,
            Some("main"),
            Some("main"),
        ));
        let visible: BTreeSet<String> = ["main".to_owned()].into();
        let seen = log.list(&all(), Scope::Computers(&visible)).unwrap();
        assert_eq!(seen.entries.len(), 3);
        for entry in &seen.entries {
            assert_eq!(entry.client, None, "{}", entry.method);
            assert_eq!(
                entry.caller_id.as_deref(),
                (entry.caller_kind == AuditCallerKind::Computer).then_some("main")
            );
        }
        let owner = log.list(&all(), Scope::All).unwrap();
        assert!(owner.entries.iter().all(|entry| entry.client.is_some()));
        assert!(owner
            .entries
            .iter()
            .any(|entry| entry.caller_id.as_deref() == Some("SHA256:key")));
    }

    #[test]
    fn a_file_from_a_newer_engine_is_left_alone_and_recording_still_returns() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        {
            let connection = open_connection(&path).unwrap();
            connection
                .execute(
                    "UPDATE meta SET value = 99 WHERE key = 'schema_version'",
                    [],
                )
                .unwrap();
        }
        let log = AuditLog::open(path, Retention::default());
        log.record(row("x", AuditCallerKind::Owner, None, None));
        log.flush(Duration::from_secs(2));
        assert!(log.list(&all(), Scope::All).is_err());
    }

    #[test]
    fn a_flood_of_computer_calls_cannot_push_out_the_owners_rows() {
        let (_directory, log) = trimmed_log(10_000, 100_000);
        let mut owner = 0;
        let flood = 20_000;
        for number in 0..flood {
            log.record(row(
                &format!("flood{number}"),
                AuditCallerKind::Computer,
                Some("a"),
                None,
            ));
            if number % 100 == 0 {
                log.record(row(
                    &format!("owner{owner}"),
                    AuditCallerKind::Owner,
                    None,
                    None,
                ));
                owner += 1;
            }
        }
        log.flush(Duration::from_secs(30));
        let mut kept_owner = 0;
        let mut counted = 0;
        let mut before = None;
        loop {
            let page = log
                .list(
                    &AuditListParams {
                        limit: Some(500),
                        before_id: before,
                        ..all()
                    },
                    Scope::All,
                )
                .unwrap();
            for entry in &page.entries {
                match entry.caller_kind {
                    AuditCallerKind::Owner => kept_owner += 1,
                    _ if entry.method == DROPPED_METHOD => {
                        counted += entry.count;
                        assert_eq!(entry.error_code.as_deref(), Some(DROPPED_ERROR));
                        assert_eq!(entry.caller_id.as_deref(), Some("a"));
                    }
                    _ => counted += entry.count,
                }
            }
            before = page.next_before_id;
            if before.is_none() {
                break;
            }
        }
        assert_eq!(kept_owner, owner, "every owner row is kept");
        assert_eq!(
            counted, flood,
            "every computer call is a row, a count or in the summary"
        );
    }

    #[test]
    fn repeats_of_a_computers_call_are_counted_before_they_are_queued() {
        let (_directory, log) = trimmed_log(100, 100);
        let repeated = 30_000;
        for _ in 0..repeated {
            log.record(row(
                "guest.exec",
                AuditCallerKind::Computer,
                Some("a"),
                Some("a"),
            ));
        }
        log.record(row("owner", AuditCallerKind::Owner, None, None));
        let rows = log.list(&kinds(), Scope::All).unwrap();
        let computer: Vec<&AuditEntry> = rows
            .entries
            .iter()
            .filter(|entry| entry.caller_kind == AuditCallerKind::Computer)
            .collect();
        assert_eq!(
            computer.iter().map(|entry| entry.count).sum::<u64>(),
            repeated
        );
        assert!(computer.iter().all(|entry| entry.method == "guest.exec"));
        assert!(rows.entries.iter().any(|entry| entry.method == "owner"));
    }
}
