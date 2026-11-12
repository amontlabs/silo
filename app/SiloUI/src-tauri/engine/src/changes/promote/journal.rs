//! The promote journal: one file per promoted fork, written before the first identity is
//! swapped and kept afterwards so `silo undo` can swap back. Every write is synced.

use std::{fs, io::Write, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::changes::keep::CheckpointRef;

/// Milliseconds since the epoch, never the same twice in one process, so two records made
/// within a millisecond still order.
pub fn now_millis() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();
    let mut last = LAST.load(Ordering::SeqCst);
    loop {
        let next = now.max(last + 1);
        match LAST.compare_exchange(last, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return u128::from(next),
            Err(seen) => last = seen,
        }
    }
}

/// One identity move. Each is idempotent in both directions, so a crash can repeat it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// The computers' names: the fork takes main's, main takes the archive name.
    Names,
    /// Everything keyed by computer name: secret and GitHub assignments, published ports,
    /// URL names and SSH access. Main's stay under the name; the fork's own are dropped.
    Bindings,
    /// SSH access: main's saved connection reaches the new main (and the archive's reaches
    /// the archive), because the alias entries are keyed by computer id.
    Alias,
    /// The fork's lineage. The record itself is the lineage: forks of the old main resolve to
    /// the new main and the new main's source is the old main's (see `lineage`). The record of
    /// which computer's agent made which moves with it: the fork is no longer made by the old
    /// main, which loses what it made to the fork.
    Lineage,
    /// macOS only: main's proxy CA and hostname go to the new main, and the archive gets the
    /// fork's hostname.
    MacosIdentity,
    /// Quiet parking restored on the promoted fork. Best effort; it never fails a promote.
    Quiet,
}

impl Step {
    pub const ORDER: [Step; 6] = [
        Step::Names,
        Step::Bindings,
        Step::Alias,
        Step::Lineage,
        Step::MacosIdentity,
        Step::Quiet,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Step::Names => "names",
            Step::Bindings => "name-keyed settings",
            Step::Alias => "SSH access",
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
    /// Main's, which move to the fork when ports are keyed by computer id.
    pub target_bindings: Value,
    /// Where the fork was forked from before it replaced `target_id`.
    pub fork_source: Option<String>,
    /// Where main was forked from; the promoted fork's source from now on.
    #[serde(default)]
    pub target_source: Option<String>,
    pub checkpoint: CheckpointRef,
    /// Whether the fork was running when the promote began; an undo starts it again.
    #[serde(default)]
    pub fork_was_running: bool,
    /// The same for main.
    #[serde(default)]
    pub target_was_running: bool,
    /// Who made which of the two before the swap (see `policy::handoff`).
    #[serde(default)]
    pub makers: crate::policy::handoff::Handoff,
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
    /// When the record was made, and again when the promote finished (milliseconds since the
    /// epoch): orders the promotes and keeps of one computer.
    #[serde(default)]
    pub at: u128,
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

    fn path(&self, fork_id: &str, target_id: &str) -> PathBuf {
        let safe: String = format!("{fork_id}.{target_id}")
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
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
        let path = self.path(&record.swap.fork_id, &record.swap.target_id);
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
        records.sort_by(|a, b| {
            (a.at, &a.swap.fork_id, &a.swap.target_id).cmp(&(
                b.at,
                &b.swap.fork_id,
                &b.swap.target_id,
            ))
        });
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

    /// The record of this fork over this old main, when one is on file.
    pub fn get(&self, fork_id: &str, target_id: &str) -> Option<Record> {
        self.all()
            .into_iter()
            .find(|r| r.swap.fork_id == fork_id && r.swap.target_id == target_id)
    }

    /// Records that are in force: promoted, or still being promoted or undone.
    pub fn in_force(&self) -> Vec<Record> {
        self.all()
            .into_iter()
            .filter(|r| r.state != State::Over)
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
