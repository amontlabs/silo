//! The keep journal: written before main is changed, removed when the keep ends. A journal
//! found with no keep running means the engine stopped mid-keep; the before-keep checkpoint
//! it names is restored. Also remembers which before-keep checkpoints an undo has used.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub fork: String,
    pub target: String,
    pub checkpoint_id: String,
    pub checkpoint_name: String,
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

    fn path(&self, target: &str) -> PathBuf {
        self.dir.join(format!("{}.keep.json", file_name(target)))
    }

    fn used_path(&self, target: &str) -> PathBuf {
        self.dir.join(format!("{}.undone.json", file_name(target)))
    }

    pub fn begin(&self, entry: &Entry) -> Result<(), String> {
        fs::create_dir_all(&self.dir).map_err(|e| format!("Cannot write the keep journal: {e}"))?;
        write_atomic(
            &self.path(&entry.target),
            &serde_json::to_vec(entry).unwrap_or_default(),
        )
    }

    pub fn pending(&self, target: &str) -> Result<Option<Entry>, String> {
        match fs::read(self.path(target)) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| format!("The keep journal is unreadable: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("Cannot read the keep journal: {e}")),
        }
    }

    pub fn clear(&self, target: &str) {
        let _ = fs::remove_file(self.path(target));
    }

    pub fn mark_undone(&self, target: &str, checkpoint_id: &str) -> Result<(), String> {
        let mut ids = self.undone(target);
        ids.push(checkpoint_id.to_owned());
        fs::create_dir_all(&self.dir).map_err(|e| format!("Cannot write the keep journal: {e}"))?;
        write_atomic(
            &self.used_path(target),
            &serde_json::to_vec(&ids).unwrap_or_default(),
        )
    }

    pub fn record_checkpoint(
        &self,
        target: &str,
        checkpoint: &super::env::CheckpointRef,
    ) -> Result<(), String> {
        let mut all = self.checkpoints(target);
        all.push(checkpoint.clone());
        fs::create_dir_all(&self.dir).map_err(|e| format!("Cannot write the keep journal: {e}"))?;
        write_atomic(
            &self
                .dir
                .join(format!("{}.checkpoints.json", file_name(target))),
            &serde_json::to_vec(&all).unwrap_or_default(),
        )
    }

    /// The before-keep checkpoints keeps made of this computer, oldest first.
    pub fn checkpoints(&self, target: &str) -> Vec<super::env::CheckpointRef> {
        fs::read(
            self.dir
                .join(format!("{}.checkpoints.json", file_name(target))),
        )
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
    }

    pub fn undone(&self, target: &str) -> Vec<String> {
        fs::read(self.used_path(target))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
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

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes)
        .and_then(|()| fs::rename(&temp, path))
        .map_err(|e| format!("Cannot write the keep journal: {e}"))
}
