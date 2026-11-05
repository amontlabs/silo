//! The promote journal: one file per promoted fork, written before the first identity is
//! swapped and kept afterwards so `silo undo` can swap back. Every write is synced.

use std::{fs, io::Write, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::changes::keep::CheckpointRef;

/// One identity move. Each is idempotent in both directions, so a crash can repeat it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// The computers' names: the fork takes main's, main takes the archive name.
    Names,
    /// Everything keyed by computer name: secret and GitHub assignments, published ports,
    /// URL names and SSH access. Main's stay under the name; the fork's own are dropped.
    Bindings,
    /// The fork's lineage: it no longer has a source; the archive records what replaced it.
    Lineage,
    /// macOS only: the proxy CA and machine identity the network bootstrap expects.
    MacosIdentity,
    /// Quiet parking restored on the promoted fork. Best effort; it never fails a promote.
    Quiet,
}

impl Step {
    pub const ORDER: [Step; 5] = [
        Step::Names,
        Step::Bindings,
        Step::Lineage,
        Step::MacosIdentity,
        Step::Quiet,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Step::Names => "names",
            Step::Bindings => "name-keyed settings",
            Step::Lineage => "lineage",
            Step::MacosIdentity => "macOS network identity",
            Step::Quiet => "quiet parking",
        }
    }
}

/// What a promote needs to do or undo every step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Swap {
    pub fork_id: String,
    pub fork_name: String,
    pub target_id: String,
    pub target_name: String,
    pub archive_name: String,
    pub macos: bool,
    /// The fork's own name-keyed settings before the swap, put back by an undo.
    pub fork_bindings: Value,
    /// Where the fork was forked from before it replaced `target_id`.
    pub fork_source: Option<String>,
    pub checkpoint: CheckpointRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Steps are being applied.
    Promoting,
    /// Every step applied: `silo undo` may swap back.
    Promoted,
    /// An undo is reverting the steps.
    Undoing,
    /// Swapped back, or rolled back.
    Over,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub swap: Swap,
    pub state: State,
    /// Steps applied (while promoting) or still to revert (while undoing).
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug)]
pub struct Journal {
    dir: PathBuf,
}

impl Journal {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, fork_id: &str) -> PathBuf {
        let safe: String = fork_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(format!("{safe}.promote.json"))
    }

    pub fn save(&self, record: &Record) -> Result<(), String> {
        let fail = |e: std::io::Error| format!("Cannot write the promote journal: {e}");
        fs::create_dir_all(&self.dir).map_err(fail)?;
        let path = self.path(&record.swap.fork_id);
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

    pub fn all(&self) -> Vec<Record> {
        let Ok(read) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut records: Vec<Record> = read
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".promote.json"))
            .filter_map(|e| fs::read(e.path()).ok())
            .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
            .collect();
        records.sort_by(|a, b| a.swap.fork_id.cmp(&b.swap.fork_id));
        records
    }

    /// Records that are not over and involve `id` as the fork or the old main.
    pub fn live_for(&self, id: &str) -> Vec<Record> {
        self.all()
            .into_iter()
            .filter(|r| r.state != State::Over)
            .filter(|r| r.swap.fork_id == id || r.swap.target_id == id)
            .collect()
    }

    /// Records whose steps were interrupted.
    pub fn interrupted(&self) -> Vec<Record> {
        self.all()
            .into_iter()
            .filter(|r| matches!(r.state, State::Promoting | State::Undoing))
            .collect()
    }
}
