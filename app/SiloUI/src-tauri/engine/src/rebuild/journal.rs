//! The rebuild journal: one file per computer being rebuilt, written before the first change
//! and after every step, so an interrupted rebuild is rolled back (or finished) at the next
//! engine start. Every write is synced.

use std::{fs, io::Write, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::changes::keep::CheckpointRef;

/// What a rebuild is made of: the computer being rebuilt, the sandbox built beside it, and the
/// name and id the old sandbox has afterwards.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// The id the computer keeps; the rebuilt sandbox carries it afterwards.
    pub old_id: String,
    pub old_name: String,
    /// The sandbox built on the newest image while the rebuild runs.
    pub new_id: String,
    pub new_name: String,
    /// What the old sandbox is called afterwards.
    pub undo_id: String,
    pub undo_name: String,
    pub image: String,
    /// Names the home folder's staging file on the host.
    pub stage: String,
}

/// How far a rebuild got. Each step is recorded after it finished, except `Swapping`, which is
/// recorded before the first name moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Planned,
    Stopped,
    Checkpointed,
    Created,
    HomeCarried,
    WorkspaceMoved,
    RecipeReplayed,
    Swapping,
    Swapped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub plan: Plan,
    pub step: Step,
    /// The disk checkpoint of the old computer, taken before anything else changes.
    pub checkpoint: Option<CheckpointRef>,
    /// Whether the computer was running when the rebuild began; it runs again afterwards.
    pub was_running: bool,
    pub at: u64,
}

#[derive(Clone, Debug)]
pub struct Journal {
    dir: PathBuf,
}

impl Journal {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, computer_id: &str) -> PathBuf {
        let safe: String = computer_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(format!("{safe}.rebuild.json"))
    }

    pub fn save(&self, record: &Record) -> Result<(), String> {
        let fail = |e: std::io::Error| format!("Cannot write the rebuild journal: {e}");
        fs::create_dir_all(&self.dir).map_err(fail)?;
        let path = self.path(&record.plan.old_id);
        let temp = path.with_extension("tmp");
        let mut file = fs::File::create(&temp).map_err(fail)?;
        file.write_all(&serde_json::to_vec(record).unwrap_or_default())
            .map_err(fail)?;
        file.sync_all().map_err(fail)?;
        fs::rename(&temp, &path).map_err(fail)?;
        if let Ok(dir) = fs::File::open(&self.dir) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    pub fn get(&self, computer_id: &str) -> Option<Record> {
        let bytes = fs::read(self.path(computer_id)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn remove(&self, computer_id: &str) {
        let _ = fs::remove_file(self.path(computer_id));
        if let Ok(dir) = fs::File::open(&self.dir) {
            let _ = dir.sync_all();
        }
    }

    /// Every rebuild that was neither finished nor rolled back.
    pub fn all(&self) -> Vec<Record> {
        let Ok(read) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut records: Vec<Record> = read
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".rebuild.json"))
            .filter_map(|e| fs::read(e.path()).ok())
            .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
            .collect();
        records.sort_by(|a, b| (a.at, &a.plan.old_id).cmp(&(b.at, &b.plan.old_id)));
        records
    }
}
