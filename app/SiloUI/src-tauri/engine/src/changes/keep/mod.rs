//! Keep and undo: bring a fork's reviewed changes onto its source (main), or put main back.
//!
//! A keep takes a `before-keep` checkpoint of main, checks the fork against main and the
//! fork-time base (three-way), and only then changes anything: packages by name and version,
//! git merge, files, crontabs and launch agents, then the recipe's `keep:` hooks. A keep that
//! would clash, or that carries a database change nothing migrates, stops before main is
//! touched and returns a structured refusal. One keep at a time runs per target; a journal
//! written before the first change lets a restart put main back from the checkpoint.

pub mod apply;
pub mod cron;
pub mod env;
pub mod guest_env;
pub mod journal;
pub mod lane;
pub mod plan;
mod prepare;
pub mod scripts;
pub mod service;
pub mod store;

#[cfg(test)]
mod tests;

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use env::{CheckpointRef, ExecOut, GuestCmd, KeepEnv, ManifestStore, Os, RunAs};
pub use journal::Journal;
pub use plan::{Conflict, Plan};

/// The recipe's `keep:` section.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hooks {
    #[serde(default)]
    pub restart: Vec<String>,
    #[serde(default)]
    pub migrate: Vec<String>,
    #[serde(default)]
    pub after: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub fork: String,
    /// `None` keeps into the fork's source.
    pub target: Option<String>,
    pub hooks: Hooks,
    pub dry_run: bool,
}

/// How long a keep or undo waits for another one on the same target.
pub const LANE_WAIT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Refusal {
    /// `divergent`, `database` or `git`.
    pub reason: String,
    pub message: String,
    pub conflicts: Vec<Conflict>,
    pub hint: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub fork: String,
    pub target: String,
    pub summary: Vec<String>,
    pub steps: Vec<String>,
    pub notes: Vec<String>,
    pub checkpoint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Kept(Report),
    DryRun(Report),
    /// Main was not changed.
    Refused {
        report: Report,
        refusal: Refusal,
    },
}

impl Outcome {
    pub fn to_json(&self) -> Value {
        match self {
            Outcome::Kept(report) => json!({"status": "kept", "report": report}),
            Outcome::DryRun(report) => json!({"status": "dry-run", "report": report}),
            Outcome::Refused { report, refusal } => {
                json!({"status": "refused", "report": report, "refusal": refusal})
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UndoOutcome {
    Undone {
        computer: String,
        checkpoint: CheckpointRef,
        /// The checkpoint came from a keep that was interrupted.
        recovered: bool,
    },
    NothingToUndo {
        computer: String,
    },
}

impl UndoOutcome {
    pub fn to_json(&self) -> Value {
        match self {
            UndoOutcome::Undone {
                computer,
                checkpoint,
                recovered,
            } => json!({
                "status": "undone",
                "computer": computer,
                "recovered": recovered,
                "checkpoint": {"id": checkpoint.id, "name": checkpoint.name, "createdAt": checkpoint.created_at},
            }),
            UndoOutcome::NothingToUndo { computer } => {
                json!({"status": "nothing-to-undo", "computer": computer})
            }
        }
    }
}

/// Keeps `request.fork` into its target.
pub fn keep(
    env: &dyn KeepEnv,
    store: &dyn ManifestStore,
    journal: &Journal,
    request: &Request,
    wait: Duration,
) -> Result<Outcome, String> {
    let target = match &request.target {
        Some(target) => target.clone(),
        None => store.source(&request.fork)?.ok_or_else(|| {
            format!(
                "Silo does not know where {} was forked from. Name the computer to keep into.",
                request.fork
            )
        })?,
    };
    if target == request.fork {
        return Err("A computer cannot be kept into itself.".into());
    }
    let _lane = lane::acquire(&target, wait)?;
    recover_locked(env, journal, &target)?;

    let prepared = match prepare::prepare(env, store, request, &target)? {
        prepare::Prepared::Refused { report, refusal } => {
            return Ok(Outcome::Refused { report, refusal });
        }
        prepare::Prepared::Ready(prepared) => prepared,
    };
    let mut report = prepared.report.clone();
    if request.dry_run {
        prepare::cleanup(env, &prepared);
        return Ok(Outcome::DryRun(report));
    }

    env.note("Saving a checkpoint of main");
    let checkpoint =
        env.checkpoint_before_keep(&target, &format!("Before keep of {}", request.fork))?;
    report.checkpoint = Some(checkpoint.id.clone());
    journal.begin(&journal::Entry {
        fork: request.fork.clone(),
        target: target.clone(),
        checkpoint_id: checkpoint.id.clone(),
        checkpoint_name: checkpoint.name.clone(),
    })?;
    match apply::apply(env, &prepared, request, &target) {
        Ok(applied) => {
            prepare::cleanup(env, &prepared);
            journal.clear(&target);
            report.steps = applied.steps;
            report.notes.extend(applied.notes);
            Ok(Outcome::Kept(report))
        }
        Err(error) => {
            env.note("Putting main back from the checkpoint");
            let restored = env.restore(&target, &checkpoint.id);
            if restored.is_ok() {
                journal.clear(&target);
            }
            Err(match restored {
                Ok(()) => format!("{error} Main was put back from the checkpoint taken before the keep."),
                Err(restore) => format!(
                    "{error} Putting main back also failed ({restore}); `silo undo {target}` retries it."
                ),
            })
        }
    }
}

/// Puts main back from the latest before-keep checkpoint.
pub fn undo(
    env: &dyn KeepEnv,
    journal: &Journal,
    computer: &str,
    wait: Duration,
) -> Result<UndoOutcome, String> {
    let _lane = lane::acquire(computer, wait)?;
    if let Some(entry) = journal.pending(computer)? {
        let checkpoint = CheckpointRef {
            id: entry.checkpoint_id.clone(),
            name: entry.checkpoint_name.clone(),
            created_at: String::new(),
        };
        env.restore(computer, &entry.checkpoint_id)?;
        journal.clear(computer);
        return Ok(UndoOutcome::Undone {
            computer: computer.to_owned(),
            checkpoint,
            recovered: true,
        });
    }
    let used = journal.undone(computer);
    let latest = env
        .before_keep_checkpoints(computer)?
        .into_iter()
        .rev()
        .find(|checkpoint| !used.contains(&checkpoint.id));
    let Some(checkpoint) = latest else {
        return Ok(UndoOutcome::NothingToUndo {
            computer: computer.to_owned(),
        });
    };
    env.restore(computer, &checkpoint.id)?;
    journal.mark_undone(computer, &checkpoint.id)?;
    Ok(UndoOutcome::Undone {
        computer: computer.to_owned(),
        checkpoint,
        recovered: false,
    })
}

/// A journal found while no keep runs means the engine stopped mid-keep.
fn recover_locked(env: &dyn KeepEnv, journal: &Journal, target: &str) -> Result<(), String> {
    if let Some(entry) = journal.pending(target)? {
        env.note("A keep was interrupted; putting main back first");
        env.restore(target, &entry.checkpoint_id)?;
        journal.clear(target);
    }
    Ok(())
}

/// Restores every interrupted keep. Run when the engine starts.
pub fn recover_all(env: &dyn KeepEnv, journal: &Journal) -> Vec<String> {
    let mut failures = Vec::new();
    for entry in journal.pending_targets() {
        let result = lane::acquire(&entry.target, Duration::from_secs(1))
            .and_then(|_lane| recover_locked(env, journal, &entry.target));
        if let Err(error) = result {
            failures.push(format!("{}: {error}", entry.target));
        }
    }
    failures
}
