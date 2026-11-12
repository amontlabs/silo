//! Approvals: calls a computer's agent made that the person who runs Silo must allow. One
//! owner-only JSON file per approval under `<app data>/approvals/`, replaced atomically.
//!
//! An approval's kind, computer, method, parameters and summary are written once, when it is
//! made, from what the engine checked; nothing here changes them afterwards. The agent can
//! attach its own note, which is kept apart from the summary. Every change of state goes
//! through this store under one lock, so an approval leaves `pending` once and an allowed
//! call is handed to the engine once.

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use silo_api::{
    methods::{
        approvals::{
            Approval, ApprovalOutcome, ApprovalState, ApprovalsChangedPayload,
            APPROVAL_NOTE_MAX_CHARS,
        },
        notifications::{Notice, NoticeCategory, NoticeComputer},
    },
    plain_text, plain_text_within,
    transport::ApprovalKind,
};

use crate::context::{Audience, EngineContext, Event};

/// Pending approvals one computer may hold at once.
pub const PENDING_LIMIT: usize = 20;
/// A pending approval nobody answered expires after this long.
pub const PENDING_FOR_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// An answered, denied or expired approval is removed this long after it ended; one its asker
/// withdrew is removed at once.
pub const KEPT_FOR_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// Approvals one computer may create in any hour, whatever became of them.
pub const ASKS_PER_HOUR: usize = 30;
const ASK_WINDOW_MS: u64 = 60 * 60 * 1000;
/// The longest computer name an approval keeps.
const NAME_MAX_CHARS: usize = 100;

/// A computer an allowed call makes, recorded as the asking computer's.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Creation {
    pub name: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

/// An approval as it is stored.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    #[serde(flatten)]
    pub approval: Approval,
    /// The digest of the method and the parameters as the computer sent them: the same call
    /// asked again finds this approval.
    pub request_key: String,
    /// The summary a computer reads when the stored one holds what only the owner may see.
    #[serde(default)]
    pub computer_summary: Option<String>,
    #[serde(default)]
    pub creates: Option<Creation>,
    /// What the call acts on as it was when it was asked (the last merge to undo, the state of
    /// the fork to promote); the allowed call is refused when it has moved.
    #[serde(default)]
    pub pin: Option<String>,
    pub created_ms: u64,
    #[serde(default)]
    pub ended_ms: Option<u64>,
}

impl Record {
    pub fn id(&self) -> &str {
        &self.approval.id
    }

    pub fn state(&self) -> ApprovalState {
        self.approval.state
    }

    pub fn computer(&self) -> &str {
        &self.approval.computer_id
    }

    /// A pending approval older than [`PENDING_FOR_MS`], whether or not a sweep marked it yet.
    fn past_due(&self, now: u64) -> bool {
        self.state() == ApprovalState::Pending
            && now.saturating_sub(self.created_ms) >= PENDING_FOR_MS
    }

    /// Drops a secret's value from the stored call; true when there was one.
    pub fn scrub_value(&mut self) -> bool {
        self.approval
            .params
            .get_mut("request")
            .and_then(Value::as_object_mut)
            .is_some_and(|request| request.remove("value").is_some())
    }

    /// The summary the computer that asked may read. A merge saved without one may hold the
    /// commands of a recipe's hooks, so the computer reads only that it waits.
    pub fn summary_for_computer(&self) -> &str {
        match (&self.computer_summary, self.approval.kind) {
            (Some(summary), _) => summary,
            (None, ApprovalKind::Merge) => crate::policy::WAITING_MERGE_SUMMARY,
            (None, _) => &self.approval.summary,
        }
    }

    /// The approval as a computer sees it: the view, with the summary cut for a computer.
    pub fn view_for_computer(&self) -> Approval {
        let mut approval = self.view();
        approval.summary = self.summary_for_computer().to_owned();
        approval
    }

    /// The approval as callers see it: a secret's value never leaves the store.
    pub fn view(&self) -> Approval {
        let mut approval = self.approval.clone();
        if let Some(value) = approval.params.pointer_mut("/request/value") {
            if !value.is_null() {
                *value = json!("(hidden)");
            }
        }
        approval
    }
}

/// What an approval is made from. Everything here is the engine's; the agent writes nothing.
pub struct NewApproval {
    pub kind: ApprovalKind,
    pub computer_id: String,
    pub computer_name: String,
    pub method: String,
    /// The parameters that run on allow.
    pub params: Value,
    /// The parameters as the computer sent them, for finding the same request again.
    pub asked: Value,
    pub summary: String,
    /// What the computer may read of the summary, when the summary holds what only the owner
    /// may see.
    pub computer_summary: Option<String>,
    pub creates: Option<Creation>,
    pub pin: Option<String>,
    pub review_request_id: Option<String>,
    pub review_digest: Option<String>,
}

/// Why a new approval was not made.
#[derive(Debug, PartialEq, Eq)]
pub enum AskError {
    /// The computer already has [`PENDING_LIMIT`] pending approvals.
    Limit,
    /// The computer created [`ASKS_PER_HOUR`] approvals in the last hour.
    Rate,
    Store(String),
}

/// The digest that finds the same request again.
pub fn request_key(method: &str, asked: &Value) -> String {
    let mut asked = match asked {
        Value::Null => json!({}),
        other => other.clone(),
    };
    // A secret's value is not part of what is asked: the same name asked again with another
    // value is the same request.
    if let Some(request) = asked.get_mut("request").and_then(Value::as_object_mut) {
        request.remove("value");
    }
    // serde_json's map keeps keys sorted, so equal parameters give equal text.
    let text = serde_json::to_string(&asked).unwrap_or_default();
    let mut hash = Sha256::new();
    hash.update(method.as_bytes());
    hash.update([0]);
    hash.update(text.as_bytes());
    format!("{:x}", hash.finalize())
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

fn rfc3339(at_ms: u64) -> String {
    time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(at_ms) * 1_000_000)
        .ok()
        .and_then(|at| {
            at.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_default()
}

fn valid_id(id: &str) -> bool {
    id.len() == 19
        && id.starts_with("ap-")
        && id[3..]
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

fn new_id() -> String {
    format!("ap-{}", &uuid::Uuid::new_v4().simple().to_string()[..16])
}

/// Everything the store knows, kept in memory so a call never reads the folder again: the
/// approvals, and when each computer asked, which outlives an approval that was deleted.
#[derive(Default)]
struct Index {
    loaded: bool,
    records: HashMap<String, Record>,
    asks: HashMap<String, Vec<u64>>,
}

/// One index for each approvals folder, all under one lock.
static INDEXES: Mutex<BTreeMap<PathBuf, Index>> = Mutex::new(BTreeMap::new());

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(app_data: &Path) -> Self {
        Self {
            root: app_data.join("approvals"),
        }
    }

    fn file(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err(format!("`{id}` is not an approval id."));
        }
        Ok(self.root.join(format!("{id}.json")))
    }

    /// Where the times of the approvals each computer asked for are kept, beside the folder.
    fn asks_file(&self) -> PathBuf {
        self.root.with_file_name("approval-asks.json")
    }

    /// Runs `body` on the index under the store's lock, reading the folder the first time.
    fn locked<R>(&self, body: impl FnOnce(&mut Index) -> R) -> R {
        let mut indexes = INDEXES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let index = indexes.entry(self.root.clone()).or_default();
        if !index.loaded {
            self.load(index);
            index.loaded = true;
        }
        body(index)
    }

    fn load(&self, index: &mut Index) {
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.filter_map(Result::ok) {
                if entry.path().extension().is_none_or(|ext| ext != "json") {
                    continue;
                }
                let Ok(bytes) = fs::read(entry.path()) else {
                    continue;
                };
                if let Ok(record) = serde_json::from_slice::<Record>(&bytes) {
                    if valid_id(record.id()) {
                        index.records.insert(record.id().to_owned(), record);
                    }
                }
            }
        }
        if let Some(saved) = fs::read(self.asks_file())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<HashMap<String, Vec<u64>>>(&bytes).ok())
        {
            index.asks = saved;
        }
        // An approval that outlived the saved times still counts.
        for record in index.records.values() {
            let times = index.asks.entry(record.computer().to_owned()).or_default();
            if !times.contains(&record.created_ms) {
                times.push(record.created_ms);
            }
        }
    }

    /// Every approval, newest first.
    fn sorted(index: &Index) -> Vec<Record> {
        let mut found: Vec<Record> = index.records.values().cloned().collect();
        found.sort_by(|left, right| {
            (right.created_ms, &right.approval.id).cmp(&(left.created_ms, &left.approval.id))
        });
        found
    }

    /// Writes `bytes` whole to `path` through a temporary file: a reader sees the old file or
    /// the new one.
    fn write_file(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let dir = path.parent().unwrap_or(&self.root);
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let temporary = dir.join(format!(
            "{}.{}.tmp",
            path.file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
            uuid::Uuid::new_v4().simple()
        ));
        let written = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            fs::File::open(dir)?.sync_all()
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        written
    }

    /// Writes the approval whole and remembers it.
    fn put(&self, index: &mut Index, record: Record) -> Result<(), String> {
        let path = self.file(record.id())?;
        let bytes = serde_json::to_vec_pretty(&record).map_err(|error| error.to_string())?;
        self.write_file(&path, &bytes)
            .map_err(|error| format!("Cannot save the approval {}: {error}", record.id()))?;
        index.records.insert(record.id().to_owned(), record);
        Ok(())
    }

    fn remove(&self, index: &mut Index, id: &str) {
        if let Ok(path) = self.file(id) {
            let _ = fs::remove_file(path);
        }
        index.records.remove(id);
    }

    /// Saves the times computers asked, keeping only those still inside the window.
    fn save_asks(&self, index: &mut Index, now: u64) {
        index.asks.retain(|_, times| {
            times.retain(|at| now.saturating_sub(*at) < ASK_WINDOW_MS);
            !times.is_empty()
        });
        if let Ok(bytes) = serde_json::to_vec(&index.asks) {
            if let Err(error) = self.write_file(&self.asks_file(), &bytes) {
                eprintln!("silo-engine: cannot save the approval request times: {error}");
            }
        }
    }

    fn existing(index: &Index, id: &str) -> Result<Record, String> {
        if !valid_id(id) {
            return Err(format!("`{id}` is not an approval id."));
        }
        index
            .records
            .get(id)
            .cloned()
            .ok_or_else(|| format!("There is no approval {id}."))
    }

    /// Ends pending approvals past their time and removes ended ones past theirs. Returns the
    /// approvals that expired.
    fn sweep_locked(&self, index: &mut Index, now: u64) -> Vec<Record> {
        let mut expired = Vec::new();
        let mut ids: Vec<String> = index.records.keys().cloned().collect();
        ids.sort();
        for id in ids {
            let Some(mut record) = index.records.get(&id).cloned() else {
                continue;
            };
            match record.state() {
                ApprovalState::Pending
                    if now.saturating_sub(record.created_ms) >= PENDING_FOR_MS =>
                {
                    record.approval.state = ApprovalState::Expired;
                    record.approval.reason =
                        Some("Nobody answered it within seven days.".to_owned());
                    record.ended_ms = Some(now);
                    record.scrub_value();
                    if self.put(index, record.clone()).is_ok() {
                        expired.push(record);
                    }
                }
                ApprovalState::Pending => {}
                // An allowed call still running has no outcome yet and is kept.
                ApprovalState::Allowed if record.approval.outcome.is_none() => {}
                _ => {
                    let ended = record.ended_ms.unwrap_or(record.created_ms);
                    if now.saturating_sub(ended) >= KEPT_FOR_MS {
                        self.remove(index, &id);
                    } else if record.scrub_value() {
                        // A file written before values were dropped at the end.
                        let _ = self.put(index, record);
                    }
                }
            }
        }
        expired
    }

    /// Ends and removes what is past its time; returns the approvals that expired.
    pub fn sweep(&self, now: u64) -> Vec<Record> {
        self.locked(|index| self.sweep_locked(index, now))
    }

    /// Every approval, newest first, after a sweep.
    pub fn list(&self) -> (Vec<Record>, Vec<Record>) {
        self.locked(|index| {
            let expired = self.sweep_locked(index, now_ms());
            (Self::sorted(index), expired)
        })
    }

    pub fn get(&self, id: &str) -> Result<Option<Record>, String> {
        let _ = self.file(id)?;
        Ok(self.locked(|index| index.records.get(id).cloned()))
    }

    /// The pending approval of `computer` for the same call, if there is one.
    pub fn find_pending(&self, computer: &str, request_key: &str) -> Option<Record> {
        let now = now_ms();
        self.locked(|index| {
            index
                .records
                .values()
                .find(|record| {
                    record.state() == ApprovalState::Pending
                        && !record.past_due(now)
                        && record.computer() == computer
                        && record.request_key == request_key
                })
                .cloned()
        })
    }

    /// Makes an approval, or returns the pending one the same computer made for the same call
    /// (`false`). The same call asked again with another secret value is the same call: the
    /// value kept is the first one's.
    pub fn ask(&self, new: NewApproval) -> Result<(Record, bool), AskError> {
        self.ask_at(new, now_ms())
    }

    pub fn ask_at(&self, new: NewApproval, now: u64) -> Result<(Record, bool), AskError> {
        self.locked(|index| {
            self.sweep_locked(index, now);
            let key = request_key(&new.method, &new.asked);
            let pending: Vec<&Record> = index
                .records
                .values()
                .filter(|record| {
                    record.state() == ApprovalState::Pending && record.computer() == new.computer_id
                })
                .collect();
            if let Some(same) = pending.iter().find(|record| record.request_key == key) {
                return Ok(((*same).clone(), false));
            }
            if pending.len() >= PENDING_LIMIT {
                return Err(AskError::Limit);
            }
            let recent = index.asks.get(&new.computer_id).map_or(0, |times| {
                times
                    .iter()
                    .filter(|at| now.saturating_sub(**at) < ASK_WINDOW_MS)
                    .count()
            });
            if recent >= ASKS_PER_HOUR {
                return Err(AskError::Rate);
            }
            let record = Record {
                approval: Approval {
                    id: new_id(),
                    kind: new.kind,
                    computer_id: new.computer_id.clone(),
                    computer_name: plain_text_within(&new.computer_name, NAME_MAX_CHARS),
                    method: new.method,
                    params: new.params,
                    summary: plain_text(&new.summary),
                    note: None,
                    state: ApprovalState::Pending,
                    created_at: rfc3339(now),
                    expires_at: rfc3339(now + PENDING_FOR_MS),
                    decided_at: None,
                    decided_by: None,
                    decision_note: None,
                    reason: None,
                    outcome: None,
                    review_request_id: new.review_request_id,
                    review_digest: new.review_digest,
                },
                request_key: key,
                computer_summary: new.computer_summary.map(|text| plain_text(&text)),
                creates: new.creates,
                pin: new.pin,
                created_ms: now,
                ended_ms: None,
            };
            self.put(index, record.clone()).map_err(AskError::Store)?;
            index.asks.entry(new.computer_id).or_default().push(now);
            self.save_asks(index, now);
            Ok((record, true))
        })
    }

    /// Changes one approval, which must be pending, and writes it.
    fn change_pending(
        &self,
        id: &str,
        change: impl FnOnce(&mut Record) -> Result<(), String>,
    ) -> Result<Record, String> {
        self.locked(|index| {
            let mut record = Self::existing(index, id)?;
            if record.past_due(now_ms()) {
                record.approval.state = ApprovalState::Expired;
            }
            if record.state() != ApprovalState::Pending {
                return Err(not_pending(&record));
            }
            change(&mut record)?;
            self.put(index, record.clone())?;
            Ok(record)
        })
    }

    /// Attaches or replaces the asking computer's note; an empty note removes it.
    pub fn note(&self, id: &str, computer: &str, note: &str) -> Result<Record, String> {
        let note = plain_text(note);
        let note = note.trim();
        if note.chars().count() > APPROVAL_NOTE_MAX_CHARS {
            return Err(format!(
                "A note is at most {APPROVAL_NOTE_MAX_CHARS} characters."
            ));
        }
        self.change_pending(id, |record| {
            if record.computer() != computer {
                return Err("Only the computer that asked can write its note.".to_owned());
            }
            record.approval.note = (!note.is_empty()).then(|| note.to_owned());
            Ok(())
        })
    }

    /// Withdraws a pending approval. `computer` is the caller when a computer withdraws it,
    /// which must be the one that asked; the approval is then removed at once.
    pub fn cancel(&self, id: &str, computer: Option<&str>, reason: &str) -> Result<Record, String> {
        let record = self.change_pending(id, |record| {
            if computer.is_some_and(|computer| computer != record.computer()) {
                return Err("Only the computer that asked can withdraw it.".to_owned());
            }
            record.approval.state = ApprovalState::Cancelled;
            record.approval.reason = Some(reason.to_owned());
            record.ended_ms = Some(now_ms());
            record.scrub_value();
            Ok(())
        })?;
        if computer.is_some() {
            self.locked(|index| self.remove(index, id));
        }
        Ok(record)
    }

    /// Records the owner's deny.
    pub fn deny(&self, id: &str, by: &str, note: Option<&str>) -> Result<Record, String> {
        self.change_pending(id, |record| {
            let now = now_ms();
            record.approval.state = ApprovalState::Denied;
            record.approval.decided_at = Some(rfc3339(now));
            record.approval.decided_by = Some(by.to_owned());
            record.approval.decision_note = note.map(str::to_owned);
            record.ended_ms = Some(now);
            record.scrub_value();
            Ok(())
        })
    }

    /// Records the owner's allow before the call runs, so the call is handed to the engine
    /// once: a second decision finds it no longer pending, and an engine that stops before the
    /// outcome is stored ends it as interrupted ([`Self::recover`]). A secret's value is kept
    /// until the outcome is stored.
    pub fn allow(&self, id: &str, by: &str, note: Option<&str>) -> Result<Record, String> {
        self.change_pending(id, |record| {
            let now = now_ms();
            record.approval.state = ApprovalState::Allowed;
            record.approval.decided_at = Some(rfc3339(now));
            record.approval.decided_by = Some(by.to_owned());
            record.approval.decision_note = note.map(str::to_owned);
            record.ended_ms = Some(now);
            Ok(())
        })
    }

    /// Stores the outcome of an allowed call, once, and drops a secret's value.
    pub fn finish(&self, id: &str, outcome: ApprovalOutcome) -> Result<Record, String> {
        self.locked(|index| {
            let mut record = Self::existing(index, id)?;
            if record.state() != ApprovalState::Allowed || record.approval.outcome.is_some() {
                return Err(format!(
                    "The approval {id} has no call waiting for its outcome."
                ));
            }
            record.approval.outcome = Some(outcome);
            record.ended_ms = Some(now_ms());
            record.scrub_value();
            self.put(index, record.clone())?;
            Ok(record)
        })
    }

    /// Ends every allowed approval without an outcome: the engine stopped while its call was
    /// being handed over, and it is never run again on its own.
    pub fn recover(&self) -> Vec<Record> {
        self.locked(|index| {
            let waiting: Vec<Record> = Self::sorted(index)
                .into_iter()
                .filter(|record| {
                    record.state() == ApprovalState::Allowed && record.approval.outcome.is_none()
                })
                .collect();
            waiting
                .into_iter()
                .filter_map(|mut record| {
                    record.approval.outcome = Some(ApprovalOutcome {
                        result: None,
                        job_id: None,
                        error: Some(
                            "Silo stopped before the outcome of this call was recorded. It may not have run, and it is not run again; ask again if it is still needed."
                                .to_owned(),
                        ),
                    });
                    record.scrub_value();
                    self.put(index, record.clone()).ok().map(|()| record)
                })
                .collect()
        })
    }

    /// Cancels the pending approvals a deleted computer asked for.
    pub fn cancel_for_computer(&self, computer: &str) -> Vec<Record> {
        let ids: Vec<String> = self.locked(|index| {
            index
                .records
                .values()
                .filter(|record| {
                    record.state() == ApprovalState::Pending && record.computer() == computer
                })
                .map(|record| record.approval.id.clone())
                .collect()
        });
        ids.iter()
            .filter_map(|id| {
                self.cancel(id, None, "The computer that asked was deleted.")
                    .ok()
            })
            .collect()
    }

    /// The pending approvals that wrap the review request `request_id`.
    pub fn pending_for_request(&self, request_id: &str) -> Vec<Record> {
        self.locked(|index| {
            index
                .records
                .values()
                .filter(|record| {
                    record.state() == ApprovalState::Pending
                        && record.approval.review_request_id.as_deref() == Some(request_id)
                })
                .cloned()
                .collect()
        })
    }

    /// Records the outcome of a review request answered outside its approval: kept (allowed,
    /// with the keep's job) or thrown away by the owner (denied).
    pub fn answered_outside(
        &self,
        id: &str,
        allowed: bool,
        by: &str,
        outcome: Option<ApprovalOutcome>,
    ) -> Result<Record, String> {
        self.change_pending(id, |record| {
            let now = now_ms();
            record.approval.state = if allowed {
                ApprovalState::Allowed
            } else {
                ApprovalState::Denied
            };
            record.approval.decided_at = Some(rfc3339(now));
            record.approval.decided_by = Some(by.to_owned());
            record.approval.outcome = if allowed {
                Some(outcome.unwrap_or(ApprovalOutcome {
                    result: None,
                    job_id: None,
                    error: None,
                }))
            } else {
                None
            };
            record.ended_ms = Some(now);
            record.scrub_value();
            Ok(())
        })
    }
}

fn not_pending(record: &Record) -> String {
    let id = record.id();
    match record.state() {
        ApprovalState::Pending => format!("The approval {id} is pending."),
        ApprovalState::Allowed => format!("The approval {id} was already allowed."),
        ApprovalState::Denied => format!("The approval {id} was already denied."),
        ApprovalState::Cancelled => format!("The approval {id} was withdrawn."),
        ApprovalState::Expired => format!("The approval {id} expired."),
    }
}

/// The notice key of an approval.
pub fn notice_key(id: &str) -> String {
    format!("approval:{id}")
}

/// Tells clients an approval changed: `approvals.changed`, and the retained notice, which a
/// pending approval holds and any other state clears.
pub fn announce(context: &dyn EngineContext, record: &Record) {
    let payload = ApprovalsChangedPayload {
        id: record.approval.id.clone(),
        state: record.state(),
    };
    if let Ok(payload) = serde_json::to_value(payload) {
        context.publish(Event::new("approvals.changed", payload), Audience::All);
    }
    let key = notice_key(record.id());
    let noticed = if record.state() == ApprovalState::Pending {
        crate::notices::retain(context, &notice(record), Audience::All)
    } else {
        context.clear_notice(&key, record.computer(), Audience::All)
    };
    if let Err(error) = noticed {
        eprintln!("silo-engine: approval {}: {error}", record.id());
    }
}

/// The notice of a pending approval: the computer's name and the engine's summary. The
/// agent's note is never in it.
pub fn notice(record: &Record) -> Notice {
    let approval = &record.approval;
    let mut title = format!("{} asks: {}", approval.computer_name, approval.summary);
    if title.chars().count() > 300 {
        title = title.chars().take(299).collect::<String>() + "…";
    }
    Notice {
        category: NoticeCategory::Approvals,
        key: notice_key(&approval.id),
        title,
        body: "Allow or deny it in Silo.".to_owned(),
        computer: Some(NoticeComputer {
            id: approval.computer_id.clone(),
            name: approval.computer_name.clone(),
        }),
    }
}

/// Cancels the pending approvals of a computer the engine removed, in the running engine's
/// store. Nothing happens when no engine is installed (unit tests of other modules).
pub fn computer_removed(id: &str) {
    let Some(context) = crate::context::current() else {
        return;
    };
    let store = Store::new(&context.paths().app_data);
    for record in store.cancel_for_computer(id) {
        announce(context.as_ref(), &record);
    }
}

#[cfg(test)]
mod tests;
