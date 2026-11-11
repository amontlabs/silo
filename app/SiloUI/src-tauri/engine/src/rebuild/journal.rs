//! The rebuild journal: records each step of a computer rebuild so an interrupted
//! rebuild resumes or rolls back cleanly.

use std::{fs, io::Write, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RebuildStep {
    Init,
    Stopped,
    CheckpointCreated,
    RebuildCreated,
    WorkspaceMoved,
    HomeAndSshCarried,
    RecipeReplayed,
    NamesSwapped,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RebuildState {
    Rebuilding,
    Finished,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebuildRecord {
    pub computer_id: String,
    pub computer_name: String,
    pub rebuild_id: String,
    pub rebuild_name: String,
    pub undo_id: String,
    pub undo_name: String,
    pub image: String,
    pub state: RebuildState,
    pub step: RebuildStep,
    pub checkpoint_id: Option<String>,
    pub was_running: bool,
    pub created_at: u64,
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

    pub fn save(&self, record: &RebuildRecord) -> Result<(), String> {
        let fail = |e: std::io::Error| format!("Cannot write the rebuild journal: {e}");
        fs::create_dir_all(&self.dir).map_err(fail)?;
        let path = self.path(&record.computer_id);
        let temp = path.with_extension("tmp");
        let mut file = fs::File::create(&temp).map_err(fail)?;
        file.write_all(&serde_json::to_vec_pretty(record).unwrap_or_default())
            .map_err(fail)?;
        file.sync_all().map_err(fail)?;
        fs::rename(&temp, &path).map_err(fail)?;
        if let Ok(dir) = fs::File::open(&self.dir) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    pub fn get(&self, computer_id: &str) -> Option<RebuildRecord> {
        let bytes = fs::read(self.path(computer_id)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn remove(&self, computer_id: &str) {
        let _ = fs::remove_file(self.path(computer_id));
    }

    pub fn all(&self) -> Vec<RebuildRecord> {
        let Ok(read) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        read.flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".rebuild.json"))
            .filter_map(|e| fs::read(e.path()).ok())
            .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
            .collect()
    }
}
