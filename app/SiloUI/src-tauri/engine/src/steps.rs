//! Steps: the commands an agent runs on a computer through Silo, one record each.
//!
//! The `silo` command line and the MCP server call `steps.record` after a command finishes
//! (and after the `step` checkpoint they took, when step checkpoints are on). The engine
//! numbers the record, 1-based per computer and never reused, and links it to the step before
//! it. A computer's records are one JSON file under `<engine data>/steps/`. The newest
//! [`KEEP_RECORDS`] are kept; the checkpoints themselves are trimmed by the `step` kind's own
//! retention (`checkpoints::DEFAULT_STEP_RETENTION`).

use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, PoisonError},
};

use serde::{Deserialize, Serialize};
use silo_api::methods::steps::Step;

use crate::{changes::store::Store, context::EngineContext};

/// The longest command text a step keeps, in characters.
pub const COMMAND_LIMIT: usize = 200;

/// How many records a computer keeps; the oldest are dropped past it.
pub const KEEP_RECORDS: usize = 500;

static WRITES: Mutex<()> = Mutex::new(());

#[derive(Default, Serialize, Deserialize)]
struct File {
    /// The number the next step gets.
    next: u32,
    steps: Vec<Step>,
}

/// What `steps.record` is given, with the parent link the caller worked out.
pub struct NewStep {
    pub computer_id: String,
    pub command: Vec<String>,
    pub exit_code: Option<i32>,
    pub started_at: String,
    pub finished_at: String,
    pub checkpoint_id: Option<String>,
    /// For the first step of a fork: the checkpoint the fork was made from.
    pub parent_checkpoint: Option<String>,
}

pub struct StepStore {
    root: PathBuf,
}

/// The argument vector as one line: arguments with spaces or quotes are single-quoted, and
/// the line is cut to [`COMMAND_LIMIT`] characters, ending in `…` when it was cut.
pub fn command_text(argv: &[String]) -> String {
    let joined = argv
        .iter()
        .map(|arg| {
            if !arg.is_empty()
                && !arg
                    .chars()
                    .any(|c| c.is_whitespace() || "'\"\\".contains(c))
            {
                arg.clone()
            } else {
                format!("'{}'", arg.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if joined.chars().count() <= COMMAND_LIMIT {
        return joined;
    }
    let mut cut: String = joined.chars().take(COMMAND_LIMIT - 1).collect();
    cut.push('…');
    cut
}

fn valid_computer(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl StepStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn for_engine(context: &dyn EngineContext) -> Self {
        Self::new(Store::for_engine(context).sibling_dir("steps"))
    }

    fn file(&self, computer: &str) -> Result<PathBuf, String> {
        if !valid_computer(computer) {
            return Err("The computer id is not usable as a file name.".to_owned());
        }
        Ok(self.root.join(format!("{computer}.json")))
    }

    fn read(&self, computer: &str) -> Result<File, String> {
        match fs::read(self.file(computer)?) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("The steps of this computer are unreadable: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(File::default()),
            Err(error) => Err(format!("Cannot read the steps of this computer: {error}")),
        }
    }

    /// The computer's steps, oldest first.
    pub fn list(&self, computer: &str) -> Result<Vec<Step>, String> {
        Ok(self.read(computer)?.steps)
    }

    /// Adds the next step of `new.computer_id`.
    pub fn record(&self, new: NewStep) -> Result<Step, String> {
        let _held = WRITES.lock().unwrap_or_else(PoisonError::into_inner);
        let path = self.file(&new.computer_id)?;
        let mut file = self.read(&new.computer_id)?;
        let number = file
            .next
            .max(file.steps.last().map_or(0, |step| step.number))
            + 1;
        let step = Step {
            id: format!("st-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
            computer_id: new.computer_id,
            number,
            command: command_text(&new.command),
            exit_code: new.exit_code,
            started_at: new.started_at,
            finished_at: new.finished_at,
            checkpoint_id: new.checkpoint_id,
            parent_step: file.steps.last().map(|step| step.number),
            parent_checkpoint: if file.steps.is_empty() && number == 1 {
                new.parent_checkpoint
            } else {
                None
            },
        };
        file.steps.push(step.clone());
        file.next = number;
        if file.steps.len() > KEEP_RECORDS {
            let excess = file.steps.len() - KEEP_RECORDS;
            file.steps.drain(..excess);
        }
        fs::create_dir_all(&self.root).map_err(|error| error.to_string())?;
        let temporary = path.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4().simple()));
        let bytes = serde_json::to_vec_pretty(&file).map_err(|error| error.to_string())?;
        fs::write(&temporary, bytes)
            .and_then(|()| fs::rename(&temporary, &path))
            .map_err(|error| {
                let _ = fs::remove_file(&temporary);
                format!("Cannot save the step: {error}")
            })?;
        Ok(step)
    }

    /// The computer and step number a checkpoint was taken for, if any step names it.
    pub fn step_of_checkpoint(&self, checkpoint_id: &str) -> Option<(String, u32)> {
        let entries = fs::read_dir(&self.root).ok()?;
        for entry in entries.flatten() {
            let Ok(bytes) = fs::read(entry.path()) else {
                continue;
            };
            let Ok(file) = serde_json::from_slice::<File>(&bytes) else {
                continue;
            };
            if let Some(step) = file
                .steps
                .iter()
                .find(|step| step.checkpoint_id.as_deref() == Some(checkpoint_id))
            {
                return Some((step.computer_id.clone(), step.number));
            }
        }
        None
    }

    /// Forgets a deleted computer's steps.
    pub fn remove_computer(&self, computer: &str) -> Result<(), String> {
        match fs::remove_file(self.file(computer)?) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.to_string()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(command: &[&str], checkpoint: Option<&str>) -> NewStep {
        NewStep {
            computer_id: "c1".into(),
            command: command.iter().map(|part| (*part).to_owned()).collect(),
            exit_code: Some(0),
            started_at: "2026-01-01T00:00:00Z".into(),
            finished_at: "2026-01-01T00:00:01Z".into(),
            checkpoint_id: checkpoint.map(str::to_owned),
            parent_checkpoint: Some("cp-fork".into()),
        }
    }

    #[test]
    fn numbers_are_per_computer_monotonic_and_linked() {
        let dir = tempfile::tempdir().unwrap();
        let store = StepStore::new(dir.path());
        let first = store.record(new(&["ls"], Some("cp-1"))).unwrap();
        let second = store.record(new(&["make", "test"], None)).unwrap();
        assert_eq!((first.number, second.number), (1, 2));
        assert_eq!(first.parent_step, None);
        assert_eq!(first.parent_checkpoint.as_deref(), Some("cp-fork"));
        assert_eq!(second.parent_step, Some(1));
        assert_eq!(second.parent_checkpoint, None);
        let mut other = new(&["ls"], None);
        other.computer_id = "c2".into();
        assert_eq!(store.record(other).unwrap().number, 1);
        assert_eq!(store.list("c1").unwrap().len(), 2);
        assert_eq!(store.step_of_checkpoint("cp-1"), Some(("c1".into(), 1)));
        assert_eq!(store.step_of_checkpoint("missing"), None);
    }

    #[test]
    fn numbers_are_not_reused_after_old_records_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let store = StepStore::new(dir.path());
        for _ in 0..KEEP_RECORDS + 3 {
            store.record(new(&["true"], None)).unwrap();
        }
        let steps = store.list("c1").unwrap();
        assert_eq!(steps.len(), KEEP_RECORDS);
        assert_eq!(steps.first().unwrap().number, 4);
        assert_eq!(
            store.record(new(&["true"], None)).unwrap().number,
            KEEP_RECORDS as u32 + 4
        );
    }

    #[test]
    fn commands_are_quoted_and_cut_to_the_limit() {
        let argv = |parts: &[&str]| parts.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            command_text(&argv(&["echo", "a b", "it's", ""])),
            r#"echo 'a b' 'it'\''s' ''"#
        );
        let long = command_text(&argv(&["echo", &"x".repeat(500)]));
        assert_eq!(long.chars().count(), COMMAND_LIMIT);
        assert!(long.ends_with('…'));
        let exact = command_text(&argv(&[&"y".repeat(COMMAND_LIMIT)]));
        assert_eq!(exact.chars().count(), COMMAND_LIMIT);
        assert!(!exact.ends_with('…'));
    }

    #[test]
    fn removing_a_computer_forgets_its_steps_and_a_bad_id_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = StepStore::new(dir.path());
        store.record(new(&["ls"], None)).unwrap();
        store.remove_computer("c1").unwrap();
        assert!(store.list("c1").unwrap().is_empty());
        assert!(store.list("../x").is_err());
    }
}
