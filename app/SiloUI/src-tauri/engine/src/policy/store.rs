//! Which computer made which, and which jobs a computer started. A computer's agent may touch
//! only the computers it made (directly, or through a computer it made), so this record, and
//! not the fork tree, is what the policy reads. One owner-only file under
//! `<app data>/policy/`, replaced atomically.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;

/// A computer made by another computer's agent.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Created {
    pub id: String,
    pub created_by: String,
    pub at: u64,
}

/// A computer being made by a computer's agent, recorded as made once its job has finished.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub created_by: String,
    pub name: String,
    /// Known up front when the engine chose the id (a Linux create).
    #[serde(default)]
    pub id: Option<String>,
    /// The computer a fork is made from; the new computer's parent must be this one.
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub job_id: Option<String>,
    pub at: u64,
}

/// A job a computer's agent started and has not acknowledged.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedJob {
    pub caller: String,
    pub id: String,
    pub at: u64,
    /// The computer the call named, when it named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computer: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub created: Vec<Created>,
    #[serde(default)]
    pub pending: Vec<Pending>,
    #[serde(default)]
    pub jobs: Vec<OwnedJob>,
}

impl Document {
    /// Every computer `creator` made, directly or through a computer it made.
    pub fn made_by(&self, creator: &str) -> HashSet<String> {
        let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
        for record in &self.created {
            children
                .entry(record.created_by.as_str())
                .or_default()
                .push(record.id.as_str());
        }
        let mut made = HashSet::new();
        let mut frontier = vec![creator];
        while let Some(parent) = frontier.pop() {
            for child in children.get(parent).into_iter().flatten() {
                if *child != creator && made.insert((*child).to_owned()) {
                    frontier.push(child);
                }
            }
        }
        made
    }

    /// The computers `creator` made itself, not through another.
    pub fn made_directly(&self, creator: &str) -> HashSet<String> {
        self.created
            .iter()
            .filter(|record| record.created_by == creator)
            .map(|record| record.id.clone())
            .collect()
    }

    pub fn pending_of(&self, creator: &str) -> usize {
        self.pending
            .iter()
            .filter(|pending| pending.created_by == creator)
            .count()
    }

    pub fn owns_job(&self, caller: &str, job_id: &str) -> bool {
        self.jobs
            .iter()
            .any(|job| job.caller == caller && job.id == job_id)
    }

    pub fn jobs_of(&self, caller: &str) -> Vec<OwnedJob> {
        self.jobs
            .iter()
            .filter(|job| job.caller == caller)
            .cloned()
            .collect()
    }

    fn record(&mut self, id: &str, created_by: &str) {
        if id == created_by {
            return;
        }
        self.created.retain(|record| record.id != id);
        self.created.push(Created {
            id: id.to_owned(),
            created_by: created_by.to_owned(),
            at: now_secs(),
        });
    }
}

/// How a pending computer's job stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobState {
    Running,
    Succeeded,
    Failed,
    /// The engine does not know the job, or could not be asked.
    Unknown,
}

/// A pending computer without a job after this long is dropped, and so is one whose job is not
/// confirmed this long after it was asked for.
const NO_JOB: u64 = 120;
const UNCONFIRMED: u64 = 1800;

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct PolicyStore {
    directory: PathBuf,
}

impl PolicyStore {
    pub fn new(app_data: &Path) -> Self {
        Self {
            directory: app_data.join("policy"),
        }
    }

    fn file(&self) -> PathBuf {
        self.directory.join("computers.json")
    }

    fn read(&self) -> Result<Document, String> {
        match fs::read(self.file()) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("The record of who made which computer is unreadable: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Document {
                version: VERSION,
                ..Document::default()
            }),
            Err(error) => Err(format!(
                "Cannot read the record of who made which computer: {error}"
            )),
        }
    }

    fn write(&self, document: &Document) -> Result<(), String> {
        let io = |error: std::io::Error| {
            format!("Cannot save the record of who made which computer: {error}")
        };
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.directory)
            .map_err(io)?;
        let temporary = self
            .directory
            .join(format!("computers-{}.tmp", uuid::Uuid::new_v4().simple()));
        let mut document = document.clone();
        document.version = VERSION;
        let bytes = serde_json::to_vec(&document).map_err(|error| error.to_string())?;
        let written = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, self.file())?;
            fs::File::open(&self.directory)?.sync_all()
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        written.map_err(io)
    }

    /// The record as it is now.
    pub fn snapshot(&self) -> Result<Document, String> {
        let _held = lock();
        self.read()
    }

    /// Changes the record under the store's lock and saves it when `change` succeeds.
    pub fn update<R>(
        &self,
        change: impl FnOnce(&mut Document) -> Result<R, String>,
    ) -> Result<R, String> {
        let _held = lock();
        let mut document = self.read()?;
        let before = document.clone();
        let result = change(&mut document)?;
        if document != before {
            self.write(&document)?;
        }
        Ok(result)
    }

    /// Notes a computer `creator` asked for. A second one by the same name is refused while the
    /// first is pending.
    pub fn begin(&self, creator: &str, pending: Pending) -> Result<(), String> {
        self.update(|document| {
            if document
                .pending
                .iter()
                .any(|other| other.name.eq_ignore_ascii_case(&pending.name))
            {
                return Err(format!("A computer named {} is already being made.", pending.name));
            }
            document.pending.push(Pending {
                created_by: creator.to_owned(),
                at: now_secs(),
                ..pending
            });
            Ok(())
        })
    }

    /// The call that asked for `name` started `job_id`.
    pub fn attach_job(&self, creator: &str, name: &str, job_id: &str) -> Result<(), String> {
        self.update(|document| {
            if let Some(pending) = document
                .pending
                .iter_mut()
                .find(|pending| pending.created_by == creator && pending.name == name)
            {
                pending.job_id = Some(job_id.to_owned());
            }
            Ok(())
        })
    }

    /// The computer `name` is made and its id is known: it is `creator`'s.
    pub fn confirm(&self, creator: &str, name: &str, id: &str) -> Result<(), String> {
        self.update(|document| {
            document
                .pending
                .retain(|pending| !(pending.created_by == creator && pending.name == name));
            document.record(id, creator);
            Ok(())
        })
    }

    /// The call that asked for `name` failed.
    pub fn withdraw(&self, creator: &str, name: &str) -> Result<(), String> {
        self.update(|document| {
            document
                .pending
                .retain(|pending| !(pending.created_by == creator && pending.name == name));
            Ok(())
        })
    }

    /// Records the pending computers whose jobs finished, using `resolve` to find the id of one
    /// the engine chose, and drops the ones that failed or were never confirmed.
    pub fn settle(
        &self,
        job: &dyn Fn(&str) -> JobState,
        resolve: &dyn Fn(&Pending) -> Option<String>,
    ) -> Result<Document, String> {
        let snapshot = self.snapshot()?;
        if snapshot.pending.is_empty() {
            return Ok(snapshot);
        }
        let now = now_secs();
        let mut made: Vec<(Pending, String)> = Vec::new();
        let mut dropped: Vec<Pending> = Vec::new();
        for pending in &snapshot.pending {
            let age = now.saturating_sub(pending.at);
            let Some(job_id) = &pending.job_id else {
                if age > NO_JOB {
                    dropped.push(pending.clone());
                }
                continue;
            };
            match job(job_id) {
                JobState::Running => {}
                JobState::Failed => dropped.push(pending.clone()),
                JobState::Succeeded => match resolve(pending) {
                    Some(id) => made.push((pending.clone(), id)),
                    None if age > UNCONFIRMED => dropped.push(pending.clone()),
                    None => {}
                },
                JobState::Unknown if age > UNCONFIRMED => dropped.push(pending.clone()),
                JobState::Unknown => {}
            }
        }
        if made.is_empty() && dropped.is_empty() {
            return Ok(snapshot);
        }
        self.update(|document| {
            for (pending, id) in &made {
                document.pending.retain(|other| other != pending);
                document.record(id, &pending.created_by);
            }
            document.pending.retain(|other| !dropped.contains(other));
            Ok(document.clone())
        })
    }

    /// Forgets computers that no longer exist. A computer made by a removed one becomes its
    /// maker's, so the agent that could reach it before still can; one whose maker was not
    /// made by a computer is forgotten with it.
    pub fn remove_computers(&self, ids: &[&str]) -> Result<(), String> {
        if ids.is_empty() {
            return Ok(());
        }
        self.update(|document| {
            for id in ids {
                let maker = document
                    .created
                    .iter()
                    .find(|record| record.id == *id)
                    .map(|record| record.created_by.clone());
                document.created.retain(|record| record.id != *id);
                match &maker {
                    Some(maker) => {
                        for record in &mut document.created {
                            if record.created_by == *id {
                                record.created_by = maker.clone();
                            }
                        }
                    }
                    None => document.created.retain(|record| record.created_by != *id),
                }
                document.created.retain(|record| record.id != record.created_by);
                document.pending.retain(|pending| {
                    pending.created_by != *id && pending.id.as_deref() != Some(id)
                });
                document.jobs.retain(|job| job.caller != *id);
            }
            Ok(())
        })
    }

    pub fn note_job(&self, caller: &str, job_id: &str) -> Result<(), String> {
        self.note_job_about(caller, job_id, None)
    }

    /// Notes a job `caller` started, and the computer its call named.
    pub fn note_job_about(
        &self,
        caller: &str,
        job_id: &str,
        computer: Option<&str>,
    ) -> Result<(), String> {
        self.update(|document| {
            if !document.owns_job(caller, job_id) {
                document.jobs.push(OwnedJob {
                    caller: caller.to_owned(),
                    id: job_id.to_owned(),
                    at: now_secs(),
                    computer: computer.map(str::to_owned),
                });
            }
            Ok(())
        })
    }

    pub fn drop_job(&self, caller: &str, job_id: &str) -> Result<(), String> {
        self.update(|document| {
            document
                .jobs
                .retain(|job| !(job.caller == caller && job.id == job_id));
            Ok(())
        })
    }

    /// Takes over what an in-guest token recorded before this store existed: the forks it made
    /// and the jobs it started.
    pub fn import_legacy(
        &self,
        creator: &str,
        forks: &[String],
        jobs: &[(String, u64)],
    ) -> Result<(), String> {
        if forks.is_empty() && jobs.is_empty() {
            return Ok(());
        }
        self.update(|document| {
            for fork in forks {
                if !document.created.iter().any(|record| record.id == *fork) {
                    document.record(fork, creator);
                }
            }
            for (job, at) in jobs {
                if !document.owns_job(creator, job) {
                    document.jobs.push(OwnedJob {
                        caller: creator.to_owned(),
                        id: job.clone(),
                        at: *at,
                        computer: None,
                    });
                }
            }
            Ok(())
        })
    }
}

/// Forgets a computer the engine removed, in the running engine's store. Nothing happens when
/// no engine is installed (unit tests of other modules).
pub fn computer_removed(id: &str) {
    let Some(context) = crate::context::current() else {
        return;
    };
    if let Err(error) = PolicyStore::new(&context.paths().app_data).remove_computers(&[id]) {
        eprintln!("silo-engine: {error}");
    }
    crate::approvals::computer_removed(id);
    crate::recipes::records::computer_removed(id);
}
