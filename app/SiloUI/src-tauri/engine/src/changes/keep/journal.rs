//! The keep journal: written before main is changed, removed when the keep ends. A journal
//! found with no keep running means the engine stopped mid-keep; the before-keep checkpoint
//! it names is restored. Also records which before-keep checkpoints an undo may use: one
//! becomes undoable only once its keep succeeded, and is consumed when it is restored or
//! its keep failed or was recovered.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::env::CheckpointRef;
use crate::recipes::Recorded;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub fork: String,
    pub target: String,
    pub checkpoint_id: String,
    pub checkpoint_name: String,
    /// The checkpoint's scope (`full` or `disk`); empty in a journal from before it was kept.
    #[serde(default)]
    pub checkpoint_scope: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Taken for a keep that has not finished.
    Pending,
    /// Its keep succeeded: `silo undo` may restore it.
    Undoable,
    /// Restored, or its keep failed or was recovered.
    Consumed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub checkpoint: CheckpointRef,
    pub state: State,
    /// When the keep succeeded (milliseconds since the epoch); orders it against a promote.
    #[serde(default)]
    pub undoable_at: u128,
    /// The fork whose keep took the checkpoint; empty in a record from before it was kept.
    #[serde(default)]
    pub fork: String,
    /// The review digest the keep was pinned to, when it was.
    #[serde(default)]
    pub review_digest: String,
    /// What the keep appended to the recipes of the target, so an undo takes it back; absent
    /// when it appended nothing, and in a record from before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_update: Option<Recorded>,
}

#[derive(Clone, Debug)]
pub struct Journal {
    dir: PathBuf,
}

fn file_name(target: &str) -> String {
    target
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl Journal {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, target: &str, kind: &str) -> PathBuf {
        self.dir.join(format!("{}.{kind}", file_name(target)))
    }

    pub fn begin(&self, entry: &Entry) -> Result<(), String> {
        write_durable(
            &self.dir,
            &self.path(&entry.target, "keep.json"),
            &serde_json::to_vec(entry).unwrap_or_default(),
        )
    }

    pub fn pending(&self, target: &str) -> Result<Option<Entry>, String> {
        match fs::read(self.path(target, "keep.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| format!("The keep journal is unreadable: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("Cannot read the keep journal: {e}")),
        }
    }

    pub fn clear(&self, target: &str) {
        let _ = fs::remove_file(self.path(target, "keep.json"));
        sync_dir(&self.dir);
    }

    pub fn records(&self, target: &str) -> Vec<Record> {
        fs::read(self.path(target, "checkpoints.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save_records(&self, target: &str, records: &[Record]) -> Result<(), String> {
        write_durable(
            &self.dir,
            &self.path(target, "checkpoints.json"),
            &serde_json::to_vec(records).unwrap_or_default(),
        )
    }

    /// A checkpoint taken for a keep that has not finished.
    pub fn record_pending(&self, target: &str, checkpoint: &CheckpointRef) -> Result<(), String> {
        let mut all = self.records(target);
        all.push(Record {
            checkpoint: checkpoint.clone(),
            state: State::Pending,
            undoable_at: 0,
            fork: String::new(),
            review_digest: String::new(),
            recipe_update: None,
        });
        self.save_records(target, &all)
    }

    pub fn set_state(&self, target: &str, id: &str, state: State) -> Result<(), String> {
        let mut all = self.records(target);
        for record in all.iter_mut().filter(|r| r.checkpoint.id == id) {
            record.state = state;
            if state == State::Undoable {
                record.undoable_at = super::super::promote::journal::now_millis();
            }
        }
        self.save_records(target, &all)
    }

    /// Notes which fork and review digest the keep that took checkpoint `id` was for.
    pub fn tag(&self, target: &str, id: &str, fork: &str, digest: &str) -> Result<(), String> {
        let mut all = self.records(target);
        for record in all.iter_mut().filter(|r| r.checkpoint.id == id) {
            record.fork = fork.to_owned();
            record.review_digest = digest.to_owned();
        }
        self.save_records(target, &all)
    }

    /// Notes what the keep that took checkpoint `id` appended to the recipes of the target; `None`
    /// forgets it.
    pub fn set_recipe_update(
        &self,
        target: &str,
        id: &str,
        update: Option<&Recorded>,
    ) -> Result<(), String> {
        let mut all = self.records(target);
        for record in all.iter_mut().filter(|r| r.checkpoint.id == id) {
            record.recipe_update = update.cloned();
        }
        self.save_records(target, &all)
    }

    /// What the keep that took checkpoint `id` appended to the recipes of the target and an
    /// undo has not yet taken back.
    pub fn recipe_update(&self, target: &str, id: &str) -> Option<Recorded> {
        self.records(target)
            .into_iter()
            .find(|r| r.checkpoint.id == id)
            .and_then(|r| r.recipe_update)
    }

    /// The before-keep checkpoint of a keep of `fork` into `target`, pinned to `digest`, that
    /// succeeded at or after `since_ms`. The keep happened; it may have been undone since (a
    /// record that was undoable and is consumed), which still counts: changes the user undid
    /// are not kept again for the same request. A keep that was rolled back never succeeded
    /// and is not found.
    pub fn finished_keep(
        &self,
        target: &str,
        fork: &str,
        digest: &str,
        since_ms: u128,
    ) -> Option<CheckpointRef> {
        self.records(target)
            .into_iter()
            .rev()
            .find(|r| {
                (r.state == State::Undoable || (r.state == State::Consumed && r.undoable_at > 0))
                    && r.fork == fork
                    && r.review_digest == digest
                    && r.undoable_at >= since_ms
            })
            .map(|r| r.checkpoint)
    }

    /// The newest checkpoint an undo may restore.
    pub fn latest_undoable(&self, target: &str) -> Option<CheckpointRef> {
        self.records(target)
            .into_iter()
            .rev()
            .find(|r| r.state == State::Undoable)
            .map(|r| r.checkpoint)
    }

    /// When the keep that `latest_undoable` would restore succeeded.
    pub fn latest_undoable_at(&self, target: &str) -> Option<u128> {
        self.records(target)
            .into_iter()
            .rev()
            .find(|r| r.state == State::Undoable)
            .map(|r| r.undoable_at)
    }

    /// Whether an interrupted keep of the target is waiting to be put back.
    pub fn has_pending(&self, target: &str) -> bool {
        matches!(self.pending(target), Ok(Some(_)) | Err(_))
    }

    /// The manifest of the target as the last keep left it, to tell what changed after.
    pub fn save_after(
        &self,
        target: &str,
        checkpoint_id: &str,
        manifest: &str,
    ) -> Result<(), String> {
        write_durable(
            &self.dir,
            &self.path(target, &format!("{}.after", file_name(checkpoint_id))),
            manifest.as_bytes(),
        )
    }

    pub fn after(&self, target: &str, checkpoint_id: &str) -> Option<String> {
        fs::read_to_string(self.path(target, &format!("{}.after", file_name(checkpoint_id)))).ok()
    }

    /// Targets with a journal left behind.
    pub fn pending_targets(&self) -> Vec<Entry> {
        let Ok(read) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        read.flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".keep.json"))
            .filter_map(|e| fs::read(e.path()).ok())
            .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
            .collect()
    }
}

fn sync_dir(dir: &Path) {
    if let Ok(handle) = fs::File::open(dir) {
        let _ = handle.sync_all();
    }
}

/// Writes a temp file, syncs it, renames it into place and syncs the folder.
fn write_durable(dir: &Path, path: &Path, bytes: &[u8]) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("Cannot write the keep journal: {e}");
    fs::create_dir_all(dir).map_err(fail)?;
    let temp = path.with_extension("tmp");
    let mut file = fs::File::create(&temp).map_err(fail)?;
    file.write_all(bytes).map_err(fail)?;
    file.sync_all().map_err(fail)?;
    fs::rename(&temp, path).map_err(fail)?;
    sync_dir(dir);
    Ok(())
}
