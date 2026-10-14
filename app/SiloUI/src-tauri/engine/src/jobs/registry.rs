use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use silo_api::{
    jobs::{ErrorCode, JobFinished, JobId, JobLookupResult, JobOutcome, JobState, JobSpace},
    methods::notifications::Notice,
    transport::ErrorObject,
};

pub const DISCOVERY_ID_BUDGET_BYTES: usize = 3 * 1024 * 1024;
const MAX_JOB_ID_BYTES: usize = 512;
const MAX_NOTICE_BYTES: usize = 64 * 1024;
const MAX_NOTICE_LIST_BYTES: usize = 3 * 1024 * 1024;
const INTERRUPTED_MESSAGE: &str = "The engine restarted before this job completed.";
const NOT_STARTED_MESSAGE: &str = "The engine restarted before this job started.";

type CancelHandler = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelRoute {
    SetupGate,
    Transfer,
    PushGate,
    Export,
    Inspection,
    JobGate,
    LogExport,
    Github,
    UpdateDownload,
    MacosGate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Acceptance {
    pub job_id: JobId,
    pub created: bool,
    pub acknowledged: bool,
}

#[derive(Debug)]
pub struct RegistryError {
    message: String,
}

impl RegistryError {
    pub(super) fn invalid(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub(super) fn serialization(error: serde_json::Error) -> Self {
        Self::invalid(format!("Could not encode the job registry value: {error}"))
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RegistryError {}

impl From<rusqlite::Error> for RegistryError {
    fn from(error: rusqlite::Error) -> Self {
        Self::invalid(format!("Job registry storage failed: {error}"))
    }
}

impl From<String> for RegistryError {
    fn from(message: String) -> Self {
        Self::invalid(message)
    }
}

#[derive(Clone)]
pub struct JobRegistry {
    path: PathBuf,
    connection: Arc<Mutex<Connection>>,
    cancel_handlers: Arc<Mutex<HashMap<JobId, (CancelRoute, CancelHandler)>>>,
    active_keys: Arc<Mutex<HashMap<String, JobId>>>,
    gate_jobs: Arc<Mutex<HashMap<u64, JobId>>>,
}

impl JobRegistry {
    pub fn open(engine_data_dir: &Path) -> Result<Self, RegistryError> {
        create_private_directory(engine_data_dir)?;
        let path = engine_data_dir.join("registry.sqlite3");
        let mut connection = Connection::open(&path)?;
        configure_database(&connection)?;
        initialize_schema(&mut connection)?;
        set_private_database_files(&path)?;
        let registry = Self {
            path,
            connection: Arc::new(Mutex::new(connection)),
            cancel_handlers: Arc::new(Mutex::new(HashMap::new())),
            active_keys: Arc::new(Mutex::new(HashMap::new())),
            gate_jobs: Arc::new(Mutex::new(HashMap::new())),
        };
        registry.recover_interrupted_jobs()?;
        Ok(registry)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn accept(&self, job_id: JobId, method: &str) -> Result<Acceptance, RegistryError> {
        if job_id.as_str().len() > MAX_JOB_ID_BYTES {
            return Err(RegistryError::invalid("Job id exceeds the registry limit."));
        }
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT state FROM jobs WHERE job_id = ?1",
                [job_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(state) = existing {
            transaction.commit()?;
            return Ok(Acceptance {
                job_id,
                created: false,
                acknowledged: state == "acknowledged",
            });
        }
        let discovery_bytes = job_id_json_bytes(job_id.as_str())?;
        ensure_discovery_budget(&transaction, discovery_bytes)?;
        let accepted_order = next_value(&transaction, "accepted_order")?;
        transaction.execute(
            "INSERT INTO jobs(job_id, method, state, discovery_bytes, accepted_order, accepted_at_ms) \
             VALUES (?1, ?2, 'waiting', ?3, ?4, ?5)",
            params![job_id.as_str(), method, discovery_bytes, accepted_order, now_ms()],
        )?;
        transaction.commit()?;
        Ok(Acceptance {
            job_id,
            created: true,
            acknowledged: false,
        })
    }

    pub fn allocate(&self, method: &str) -> Result<Acceptance, RegistryError> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let next = next_value(&transaction, "next_job_id")?;
        let job_id = JobId::new(format!("job:{next}"))
            .map_err(|error| RegistryError::invalid(error.to_string()))?;
        let discovery_bytes = job_id_json_bytes(job_id.as_str())?;
        ensure_discovery_budget(&transaction, discovery_bytes)?;
        let accepted_order = next_value(&transaction, "accepted_order")?;
        transaction.execute(
            "INSERT INTO jobs(job_id, method, state, discovery_bytes, accepted_order, accepted_at_ms) \
             VALUES (?1, ?2, 'waiting', ?3, ?4, ?5)",
            params![job_id.as_str(), method, discovery_bytes, accepted_order, now_ms()],
        )?;
        transaction.commit()?;
        Ok(Acceptance {
            job_id,
            created: true,
            acknowledged: false,
        })
    }

    pub fn mark_running(&self, job_id: &JobId) -> Result<(), RegistryError> {
        let connection = self.lock_connection()?;
        connection.execute(
            "UPDATE jobs SET state = 'running', started_at_ms = COALESCE(started_at_ms, ?2) \
             WHERE job_id = ?1 AND state = 'waiting'",
            params![job_id.as_str(), now_ms()],
        )?;
        Ok(())
    }

    pub fn record_progress(
        &self,
        job_id: &JobId,
        progress: &serde_json::Value,
    ) -> Result<(), RegistryError> {
        let progress = serde_json::to_string(progress).map_err(RegistryError::serialization)?;
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: Option<String> = transaction
            .query_row(
                "SELECT state FROM jobs WHERE job_id = ?1",
                [job_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        match exists.as_deref() {
            Some("acknowledged") => return Ok(()),
            Some("finished") => return Ok(()),
            Some(_) => {
                transaction.execute(
                    "UPDATE jobs SET state = 'running', progress_json = ?2, \
                     started_at_ms = COALESCE(started_at_ms, ?3) WHERE job_id = ?1",
                    params![job_id.as_str(), progress, now_ms()],
                )?;
            }
            None => {
                let discovery_bytes = job_id_json_bytes(job_id.as_str())?;
                ensure_discovery_budget(&transaction, discovery_bytes)?;
                let accepted_order = next_value(&transaction, "accepted_order")?;
                transaction.execute(
                    "INSERT INTO jobs(job_id, method, state, progress_json, discovery_bytes, accepted_order, accepted_at_ms, started_at_ms) \
                     VALUES (?1, 'unspecified', 'running', ?2, ?3, ?4, ?5, ?5)",
                    params![job_id.as_str(), progress, discovery_bytes, accepted_order, now_ms()],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Commits a terminal result before its event is published.
    /// Returns false for a repeated identical finish or an acknowledged tombstone.
    pub fn finish(&self, job_id: &JobId, finished: &JobFinished) -> Result<bool, RegistryError> {
        let outcome = JobOutcome {
            result: finished.result.clone(),
            error: finished.error.clone(),
        };
        let outcome_json = serde_json::to_string(&outcome).map_err(RegistryError::serialization)?;
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, Option<String>)> = transaction
            .query_row(
                "SELECT state, outcome_json FROM jobs WHERE job_id = ?1",
                [job_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match existing {
            Some((state, _)) if state == "acknowledged" => {
                transaction.commit()?;
                return Ok(false);
            }
            Some((state, Some(previous))) if state == "finished" => {
                transaction.commit()?;
                if previous == outcome_json {
                    return Ok(false);
                }
                return Err(RegistryError::invalid(format!(
                    "Job {job_id} already has a different terminal outcome."
                )));
            }
            Some((state, _)) if state == "finished" => {
                transaction.commit()?;
                return Ok(false);
            }
            Some(_) => {
                transaction.execute(
                    "UPDATE jobs SET state = 'finished', outcome_json = ?2, finished_at_ms = ?3 \
                     WHERE job_id = ?1",
                    params![job_id.as_str(), outcome_json, now_ms()],
                )?;
            }
            None => {
                let discovery_bytes = job_id_json_bytes(job_id.as_str())?;
                ensure_discovery_budget(&transaction, discovery_bytes)?;
                let accepted_order = next_value(&transaction, "accepted_order")?;
                transaction.execute(
                    "INSERT INTO jobs(job_id, method, state, outcome_json, discovery_bytes, accepted_order, accepted_at_ms, finished_at_ms) \
                     VALUES (?1, 'unspecified', 'finished', ?2, ?3, ?4, ?5, ?5)",
                    params![job_id.as_str(), outcome_json, discovery_bytes, accepted_order, now_ms()],
                )?;
            }
        }
        transaction.commit()?;
        self.active_keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, active_job| active_job != job_id);
        self.gate_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, active_job| active_job != job_id);
        Ok(true)
    }

    /// Reconciles a terminal result from an authoritative domain journal after restart.
    /// A previously committed terminal result remains immutable; this only replaces the
    /// generic interruption written for an unfinished job.
    pub fn reconcile_interrupted(
        &self,
        job_id: &JobId,
        finished: &JobFinished,
    ) -> Result<bool, RegistryError> {
        let outcome = JobOutcome {
            result: finished.result.clone(),
            error: finished.error.clone(),
        };
        let outcome_json = serde_json::to_string(&outcome).map_err(RegistryError::serialization)?;
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, Option<String>)> = transaction
            .query_row(
                "SELECT state, outcome_json FROM jobs WHERE job_id = ?1",
                [job_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match existing {
            Some((state, _)) if state == "acknowledged" => {
                transaction.commit()?;
                return Ok(false);
            }
            Some((state, Some(previous))) if state == "finished" => {
                let previous: JobOutcome = serde_json::from_str(&previous)
                    .map_err(RegistryError::serialization)?;
                let is_interrupted = previous.error.as_ref().is_some_and(|error| {
                    error.code == ErrorCode::Internal && error.message == INTERRUPTED_MESSAGE
                });
                if !is_interrupted {
                    transaction.commit()?;
                    return if previous == outcome {
                        Ok(false)
                    } else {
                        Err(RegistryError::invalid(format!(
                            "Job {job_id} already has a different terminal outcome."
                        )))
                    };
                }
                transaction.execute(
                    "UPDATE jobs SET outcome_json = ?2, finished_at_ms = ?3 WHERE job_id = ?1",
                    params![job_id.as_str(), outcome_json, now_ms()],
                )?;
            }
            Some((state, _)) if state == "finished" => {
                transaction.commit()?;
                return Ok(false);
            }
            Some(_) => {
                transaction.execute(
                    "UPDATE jobs SET state = 'finished', outcome_json = ?2, finished_at_ms = ?3 \
                     WHERE job_id = ?1",
                    params![job_id.as_str(), outcome_json, now_ms()],
                )?;
            }
            None => {
                let discovery_bytes = job_id_json_bytes(job_id.as_str())?;
                ensure_discovery_budget(&transaction, discovery_bytes)?;
                let accepted_order = next_value(&transaction, "accepted_order")?;
                transaction.execute(
                    "INSERT INTO jobs(job_id, method, state, outcome_json, discovery_bytes, accepted_order, accepted_at_ms, finished_at_ms) \
                     VALUES (?1, 'recovered', 'finished', ?2, ?3, ?4, ?5, ?5)",
                    params![job_id.as_str(), outcome_json, discovery_bytes, accepted_order, now_ms()],
                )?;
            }
        }
        transaction.commit()?;
        self.active_keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, active_job| active_job != job_id);
        self.gate_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, active_job| active_job != job_id);
        Ok(true)
    }

    pub fn get(&self, job_id: &JobId) -> Result<Option<JobLookupResult>, RegistryError> {
        let connection = self.lock_connection()?;
        let row: Option<(String, Option<String>)> = connection
            .query_row(
                "SELECT state, outcome_json FROM jobs WHERE job_id = ?1",
                [job_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((state, outcome_json)) = row else {
            return Ok(None);
        };
        if state == "acknowledged" {
            return Ok(None);
        }
        let state = match state.as_str() {
            "waiting" => JobState::Waiting,
            "running" => JobState::Running,
            "finished" => JobState::Finished,
            _ => return Err(RegistryError::invalid("Job registry contains an invalid state.")),
        };
        let outcome = outcome_json
            .map(|value| serde_json::from_str(&value).map_err(RegistryError::serialization))
            .transpose()?;
        Ok(Some(JobLookupResult {
            job_id: job_id.clone(),
            state,
            outcome,
        }))
    }

    pub fn list_unacknowledged(&self) -> Result<Vec<JobId>, RegistryError> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT job_id FROM jobs WHERE state = 'finished' ORDER BY accepted_order",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let value = row?;
            JobId::new(value).map_err(|error| RegistryError::invalid(error.to_string()))
        })
        .collect()
    }

    pub fn acknowledge(&self, job_id: &JobId) -> Result<(), RegistryError> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: Option<String> = transaction
            .query_row(
                "SELECT state FROM jobs WHERE job_id = ?1",
                [job_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        match state.as_deref() {
            Some("waiting" | "running") => {
                return Err(RegistryError::invalid("A running job cannot be acknowledged."));
            }
            Some("finished") => {
                transaction.execute(
                    "UPDATE jobs SET state = 'acknowledged', outcome_json = NULL, \
                     progress_json = NULL, requester_client_id = NULL WHERE job_id = ?1",
                    [job_id.as_str()],
                )?;
            }
            Some("acknowledged") | None => {}
            Some(_) => return Err(RegistryError::invalid("Job registry contains an invalid state.")),
        }
        transaction.commit()?;
        self.cancel_handlers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(job_id);
        self.active_keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, active_job| active_job != job_id);
        self.gate_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, active_job| active_job != job_id);
        Ok(())
    }

    pub fn register_cancel<F>(
        &self,
        job_id: JobId,
        route: CancelRoute,
        cancel: F,
    ) where
        F: Fn() -> Result<(), String> + Send + Sync + 'static,
    {
        self.cancel_handlers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(job_id, (route, Arc::new(cancel)));
    }

    pub fn associate_active_key(&self, key: impl Into<String>, job_id: JobId) {
        self.active_keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key.into(), job_id);
    }

    pub fn active_job_for_key(&self, key: &str) -> Option<JobId> {
        self.active_keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .cloned()
    }

    pub fn associate_gate(&self, gate_id: u64, job_id: JobId) {
        self.gate_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(gate_id, job_id);
    }

    pub fn job_for_gate(&self, gate_id: u64) -> Option<JobId> {
        self.gate_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&gate_id)
            .cloned()
    }

    pub fn cancel(&self, job_id: &JobId) -> Result<(), RegistryError> {
        let Some(record) = self.get(job_id)? else {
            return Err(RegistryError::invalid("not_found"));
        };
        if record.state == JobState::Finished {
            return Ok(());
        }
        let (route, handler) = self
            .cancel_handlers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(job_id)
            .cloned()
            .ok_or_else(|| RegistryError::invalid("This operation can't be cancelled."))?;
        if !route_matches(job_id.space(), route) {
            return Err(RegistryError::invalid(
                "The job cancellation route does not match its id space.",
            ));
        }
        handler().map_err(RegistryError::invalid)
    }

    pub fn upsert_notice(&self, notice: &Notice) -> Result<(), RegistryError> {
        let notice_json = serde_json::to_string(notice).map_err(RegistryError::serialization)?;
        if notice_json.len() > MAX_NOTICE_BYTES {
            return Err(RegistryError::invalid(
                "Notice exceeds the retained notice size limit.",
            ));
        }
        let computer_id = notice.computer.as_ref().map(|computer| computer.id.clone());
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (other_notice_bytes, other_notice_count): (i64, i64) = transaction.query_row(
            "SELECT COALESCE(SUM(length(CAST(notice_json AS BLOB))), 0), COUNT(*) FROM notices WHERE key != ?1",
            [&notice.key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        // Match list()'s conservative accounting: JSON brackets plus one byte
        // per row for a separator (including the first row).
        let listed_bytes = other_notice_bytes.max(0) as usize
            + notice_json.len()
            + other_notice_count.max(0) as usize
            + 3;
        if listed_bytes > MAX_NOTICE_LIST_BYTES {
            return Err(RegistryError::invalid(
                "Retained notices reached the response size limit.",
            ));
        }
        let order = next_value(&transaction, "notice_order")?;
        transaction.execute(
            "INSERT INTO notices(key, computer_id, notice_json, updated_order) VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(key) DO UPDATE SET computer_id = excluded.computer_id, \
             notice_json = excluded.notice_json, updated_order = excluded.updated_order",
            params![&notice.key, computer_id, notice_json, order],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list_notices(&self) -> Result<Vec<Notice>, RegistryError> {
        let connection = self.lock_connection()?;
        let mut statement =
            connection.prepare("SELECT notice_json FROM notices ORDER BY updated_order")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            serde_json::from_str(&row?).map_err(RegistryError::serialization)
        })
        .collect()
    }

    pub fn clear_notices_for_computer(&self, computer_id: &str) -> Result<(), RegistryError> {
        let connection = self.lock_connection()?;
        connection.execute(
            "DELETE FROM notices WHERE computer_id = ?1",
            [computer_id],
        )?;
        Ok(())
    }

    fn recover_interrupted_jobs(&self) -> Result<(), RegistryError> {
        let interrupted = JobOutcome {
            result: None,
            error: Some(ErrorObject {
                code: ErrorCode::Internal,
                message: INTERRUPTED_MESSAGE.to_owned(),
                diagnostics: None,
            }),
        };
        let not_started = JobOutcome {
            result: None,
            error: Some(ErrorObject {
                code: ErrorCode::Cancelled,
                message: NOT_STARTED_MESSAGE.to_owned(),
                diagnostics: None,
            }),
        };
        let interrupted_json = serde_json::to_string(&interrupted)
            .map_err(RegistryError::serialization)?;
        let not_started_json = serde_json::to_string(&not_started)
            .map_err(RegistryError::serialization)?;
        let connection = self.lock_connection()?;
        connection.execute(
            "UPDATE jobs SET state = 'finished', \
             outcome_json = CASE state WHEN 'waiting' THEN ?1 ELSE ?2 END, finished_at_ms = ?3 \
             WHERE state IN ('waiting', 'running')",
            params![not_started_json, interrupted_json, now_ms()],
        )?;
        Ok(())
    }

    fn lock_connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>, RegistryError> {
        self.connection
            .lock()
            .map_err(|_| RegistryError::invalid("Job registry storage lock failed."))
    }
}

pub(super) fn registry_for_path(path: &Path) -> Result<Arc<JobRegistry>, RegistryError> {
    static REGISTRIES: OnceLock<Mutex<HashMap<PathBuf, Arc<JobRegistry>>>> = OnceLock::new();
    create_private_directory(path)?;
    let canonical = fs::canonicalize(path).map_err(|error| {
        RegistryError::invalid(format!("Could not resolve engine data directory: {error}"))
    })?;
    let registries = REGISTRIES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut registries = registries
        .lock()
        .map_err(|_| RegistryError::invalid("Job registry cache lock failed."))?;
    if let Some(registry) = registries.get(&canonical) {
        return Ok(registry.clone());
    }
    let registry = Arc::new(JobRegistry::open(&canonical)?);
    registries.insert(canonical, registry.clone());
    Ok(registry)
}

fn route_matches(space: JobSpace, route: CancelRoute) -> bool {
    matches!(
        (space, route),
        (JobSpace::Setup, CancelRoute::SetupGate)
            | (JobSpace::Transfer, CancelRoute::Transfer)
            | (JobSpace::Push, CancelRoute::PushGate)
            | (JobSpace::Exports, CancelRoute::Export)
            | (JobSpace::Inspect, CancelRoute::Inspection)
            | (JobSpace::Job, CancelRoute::JobGate)
            | (JobSpace::Job, CancelRoute::LogExport)
            | (JobSpace::Job, CancelRoute::Github)
            | (JobSpace::Job, CancelRoute::UpdateDownload)
            | (JobSpace::Job, CancelRoute::MacosGate)
    )
}

fn configure_database(connection: &Connection) -> Result<(), RegistryError> {
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    let journal_mode: String = connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(RegistryError::invalid("SQLite WAL mode was not enabled."));
    }
    connection.execute_batch(
        "PRAGMA synchronous=FULL; PRAGMA fullfsync=ON; PRAGMA foreign_keys=ON;",
    )?;
    let synchronous: i64 = connection.query_row("PRAGMA synchronous", [], |row| row.get(0))?;
    let foreign_keys: i64 = connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    if synchronous != 2 || foreign_keys != 1 {
        return Err(RegistryError::invalid(
            "SQLite durability or foreign key settings could not be verified.",
        ));
    }
    let fullfsync: i64 = connection.query_row("PRAGMA fullfsync", [], |row| row.get(0))?;
    if cfg!(target_os = "macos") && fullfsync != 1 {
        return Err(RegistryError::invalid("SQLite fullfsync could not be enabled."));
    }
    Ok(())
}

fn initialize_schema(connection: &mut Connection) -> Result<(), RegistryError> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > 2 {
        return Err(RegistryError::invalid(format!(
            "Job registry schema {version} is newer than this engine supports."
        )));
    }
    if version == 0 {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(
            "CREATE TABLE jobs (
                 job_id TEXT PRIMARY KEY,
                 method TEXT NOT NULL,
                 state TEXT NOT NULL CHECK(state IN ('waiting','running','finished','acknowledged')),
                 progress_json TEXT,
                 outcome_json TEXT,
                 discovery_bytes INTEGER NOT NULL,
                 accepted_order INTEGER NOT NULL UNIQUE,
                 accepted_at_ms INTEGER NOT NULL,
                 started_at_ms INTEGER,
                 finished_at_ms INTEGER,
                 requester_client_id TEXT
             );
             CREATE TABLE notices (
                 key TEXT PRIMARY KEY,
                 computer_id TEXT,
                 notice_json TEXT NOT NULL,
                 updated_order INTEGER NOT NULL
             );
             CREATE TABLE registry_meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
             INSERT INTO registry_meta(key, value) VALUES
                 ('next_job_id', 1), ('accepted_order', 0), ('notice_order', 0);
             PRAGMA user_version=2;",
        )?;
        transaction.commit()?;
    } else if version == 1 {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(
            "ALTER TABLE jobs ADD COLUMN discovery_bytes INTEGER NOT NULL DEFAULT 0;",
        )?;
        let job_ids = {
            let mut statement = transaction.prepare("SELECT job_id FROM jobs")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for job_id in job_ids {
            let discovery_bytes = job_id_json_bytes(&job_id)?;
            transaction.execute(
                "UPDATE jobs SET discovery_bytes = ?2 WHERE job_id = ?1",
                params![job_id, discovery_bytes],
            )?;
        }
        transaction.execute_batch("PRAGMA user_version=2;")?;
        transaction.commit()?;
    }
    Ok(())
}

fn next_value(
    transaction: &rusqlite::Transaction<'_>,
    key: &str,
) -> Result<i64, RegistryError> {
    let current: i64 = transaction.query_row(
        "SELECT value FROM registry_meta WHERE key = ?1",
        [key],
        |row| row.get(0),
    )?;
    let next = current
        .checked_add(1)
        .ok_or_else(|| RegistryError::invalid("Job registry counter is exhausted."))?;
    transaction.execute(
        "UPDATE registry_meta SET value = ?2 WHERE key = ?1",
        params![key, next],
    )?;
    Ok(if key == "next_job_id" { current } else { next })
}

fn job_id_json_bytes(job_id: &str) -> Result<i64, RegistryError> {
    if job_id.len() > MAX_JOB_ID_BYTES {
        return Err(RegistryError::invalid("Job id exceeds the registry limit."));
    }
    let bytes = serde_json::to_vec(job_id).map_err(RegistryError::serialization)?;
    i64::try_from(bytes.len())
        .map_err(|error| RegistryError::invalid(format!("Job id size is invalid: {error}")))
}

fn ensure_discovery_budget(
    transaction: &rusqlite::Transaction<'_>,
    new_id_bytes: i64,
) -> Result<(), RegistryError> {
    let (current_bytes, count): (i64, i64) = transaction.query_row(
        "SELECT COALESCE(SUM(discovery_bytes), 0), COUNT(*) FROM jobs WHERE state != 'acknowledged'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    // Count the JSON array brackets and commas as well as each quoted ID.
    let list_bytes = if count == 0 {
        2_i64.checked_add(new_id_bytes)
    } else {
        2_i64
            .checked_add(current_bytes)
            .and_then(|bytes| bytes.checked_add(count))
            .and_then(|bytes| bytes.checked_add(new_id_bytes))
    }
    .ok_or_else(|| RegistryError::invalid("Job discovery size is exhausted."))?;
    if usize::try_from(list_bytes).unwrap_or(usize::MAX) > DISCOVERY_ID_BUDGET_BYTES {
        return Err(RegistryError::invalid(
            "busy: retained job discovery is at capacity; acknowledge displayed outcomes first.",
        ));
    }
    Ok(())
}

fn create_private_directory(path: &Path) -> Result<(), RegistryError> {
    fs::create_dir_all(path).map_err(|error| {
        RegistryError::invalid(format!("Could not create engine data directory: {error}"))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            RegistryError::invalid(format!("Could not secure engine data directory: {error}"))
        })?;
    }
    Ok(())
}

fn set_private_file(path: &Path) -> Result<(), RegistryError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
            RegistryError::invalid(format!("Could not secure job registry file: {error}"))
        })?;
    }
    Ok(())
}

fn set_private_database_files(path: &Path) -> Result<(), RegistryError> {
    set_private_file(path)?;
    for suffix in ["-wal", "-shm"] {
        let mut companion = path.as_os_str().to_os_string();
        companion.push(suffix);
        let companion = PathBuf::from(companion);
        if companion.exists() {
            set_private_file(&companion)?;
        }
    }
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn finished(job_id: JobId, result: serde_json::Value) -> JobFinished {
        JobFinished {
            job_id,
            result: Some(result),
            error: None,
        }
    }

    #[test]
    fn completed_outcomes_survive_restart_and_unfinished_jobs_are_recovered() {
        let temporary = tempfile::tempdir().unwrap();
        let completed_id = JobId::new("push:operation-7").unwrap();
        let waiting_id;
        let interrupted_id;
        {
            let registry = JobRegistry::open(temporary.path()).unwrap();
            let accepted = registry.accept(completed_id.clone(), "push.start").unwrap();
            assert!(accepted.created);
            assert!(registry
                .finish(&completed_id, &finished(completed_id.clone(), serde_json::json!({"status":"succeeded"})))
                .unwrap());

            waiting_id = registry.allocate("computers.lifecycle").unwrap().job_id;
            interrupted_id = registry.allocate("computers.lifecycle").unwrap().job_id;
            registry.mark_running(&interrupted_id).unwrap();
        }

        let restarted = JobRegistry::open(temporary.path()).unwrap();
        let retained = restarted.get(&completed_id).unwrap().unwrap();
        assert_eq!(retained.state, JobState::Finished);
        assert_eq!(
            retained.outcome.unwrap().result,
            Some(serde_json::json!({"status":"succeeded"}))
        );
        let interrupted = restarted.get(&interrupted_id).unwrap().unwrap();
        assert_eq!(interrupted.state, JobState::Finished);
        let error = interrupted.outcome.unwrap().error.unwrap();
        assert_eq!(error.code, ErrorCode::Internal);
        assert_eq!(error.message, INTERRUPTED_MESSAGE);
        let waiting = restarted.get(&waiting_id).unwrap().unwrap();
        assert_eq!(waiting.state, JobState::Finished);
        let error = waiting.outcome.unwrap().error.unwrap();
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert_eq!(error.message, NOT_STARTED_MESSAGE);
        assert_eq!(
            restarted.list_unacknowledged().unwrap(),
            vec![completed_id, waiting_id, interrupted_id]
        );
    }

    #[test]
    fn domain_journal_can_reconcile_a_generic_restart_interruption() {
        let temporary = tempfile::tempdir().unwrap();
        let job_id;
        {
            let registry = JobRegistry::open(temporary.path()).unwrap();
            job_id = registry.accept(JobId::new("push:operation-8").unwrap(), "push.start").unwrap().job_id;
            registry.mark_running(&job_id).unwrap();
        }
        let restarted = JobRegistry::open(temporary.path()).unwrap();
        let recovered = finished(job_id.clone(), serde_json::json!({"status":"unknown"}));
        assert!(restarted.reconcile_interrupted(&job_id, &recovered).unwrap());
        assert!(!restarted.reconcile_interrupted(&job_id, &recovered).unwrap());
        let record = restarted.get(&job_id).unwrap().unwrap();
        assert_eq!(record.outcome.unwrap().result, recovered.result);
    }

    #[test]
    fn acknowledgement_drops_the_outcome_and_keeps_a_non_reusable_tombstone() {
        let temporary = tempfile::tempdir().unwrap();
        let job_id = JobId::new("transfer:transfer-4").unwrap();
        {
            let registry = JobRegistry::open(temporary.path()).unwrap();
            registry.accept(job_id.clone(), "files.upload").unwrap();
            registry
                .finish(&job_id, &finished(job_id.clone(), serde_json::json!({"status":"cancelled"})))
                .unwrap();
            registry.acknowledge(&job_id).unwrap();
            registry.acknowledge(&job_id).unwrap();
            assert!(registry.get(&job_id).unwrap().is_none());
            assert!(registry.list_unacknowledged().unwrap().is_empty());
        }
        let restarted = JobRegistry::open(temporary.path()).unwrap();
        let accepted_again = restarted.accept(job_id.clone(), "files.upload").unwrap();
        assert!(!accepted_again.created);
        assert!(accepted_again.acknowledged);
        assert!(restarted.get(&job_id).unwrap().is_none());
    }

    #[test]
    fn discovery_budget_counts_serialized_utf8_ids_and_array_separators() {
        let temporary = tempfile::tempdir().unwrap();
        let registry = JobRegistry::open(temporary.path()).unwrap();
        let mut connection = registry.lock_connection().unwrap();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let payload = "é".repeat(240);
        let id = format!("setup:{payload}");
        let encoded_bytes = job_id_json_bytes(&id).unwrap();
        let count = (DISCOVERY_ID_BUDGET_BYTES / (encoded_bytes as usize + 1)) + 1;
        let old_character_estimate = count * (id.chars().count() + 3);
        assert!(old_character_estimate < DISCOVERY_ID_BUDGET_BYTES);

        for index in 0..count {
            let job_id = format!("{id}-{index}");
            let discovery_bytes = job_id_json_bytes(&job_id).unwrap();
            transaction
                .execute(
                    "INSERT INTO jobs(job_id, method, state, discovery_bytes, accepted_order, accepted_at_ms) \
                     VALUES (?1, 'test', 'finished', ?2, ?3, 0)",
                    params![job_id, discovery_bytes, index as i64 + 1],
                )
                .unwrap();
        }

        let next = JobId::new("setup:next").unwrap();
        let error = ensure_discovery_budget(
            &transaction,
            job_id_json_bytes(next.as_str()).unwrap(),
        )
        .unwrap_err();
        assert!(error.message().contains("at capacity"));
    }

    #[test]
    fn cancellation_routes_by_job_space_and_rejects_a_mismatched_route() {
        let temporary = tempfile::tempdir().unwrap();
        let registry = JobRegistry::open(temporary.path()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let routes = [
            ("setup:req", CancelRoute::SetupGate),
            ("transfer:xfer", CancelRoute::Transfer),
            ("push:push", CancelRoute::PushGate),
            ("exports:backup", CancelRoute::Export),
            ("inspect:archive", CancelRoute::Inspection),
            ("job:1", CancelRoute::JobGate),
        ];
        for (raw_id, route) in routes {
            let id = JobId::new(raw_id).unwrap();
            registry.accept(id.clone(), "test.long-operation").unwrap();
            let calls = calls.clone();
            registry.register_cancel(id.clone(), route, move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
            registry.cancel(&id).unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), routes.len());

        let mismatched = JobId::new("setup:wrong-route").unwrap();
        registry.accept(mismatched.clone(), "test.long-operation").unwrap();
        registry.register_cancel(mismatched.clone(), CancelRoute::Transfer, || Ok(()));
        assert!(registry.cancel(&mismatched).is_err());
    }

    #[test]
    fn notices_replace_by_key_survive_restart_and_clear_by_computer() {
        let temporary = tempfile::tempdir().unwrap();
        let notice = |key: &str, body: &str, computer_id: &str| Notice {
            category: silo_api::methods::notifications::NoticeCategory::Failures,
            key: key.to_owned(),
            title: "Setup failed".to_owned(),
            body: body.to_owned(),
            computer: Some(silo_api::methods::notifications::NoticeComputer {
                id: computer_id.to_owned(),
                name: "Dev".to_owned(),
            }),
        };
        {
            let registry = JobRegistry::open(temporary.path()).unwrap();
            registry.upsert_notice(&notice("setup:dev", "old", "dev")).unwrap();
            registry.upsert_notice(&notice("setup:dev", "latest", "dev")).unwrap();
            registry.upsert_notice(&notice("setup:other", "keep", "other")).unwrap();
        }
        let restarted = JobRegistry::open(temporary.path()).unwrap();
        let notices = restarted.list_notices().unwrap();
        assert_eq!(notices.len(), 2);
        assert_eq!(notices[0].body, "latest");
        restarted.clear_notices_for_computer("dev").unwrap();
        let remaining = restarted.list_notices().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].key, "setup:other");
    }
}
