//! Make this main: a fork replaces the computer it was made from.
//!
//! Computers have a stable id and a name; the name is what `main` means to a person and to
//! every store keyed by name. A promote checkpoints main, stops both computers, holds both
//! operation lanes, and then moves the identity, one journaled step at a time: the fork takes
//! main's name, main becomes `main-before-<time>`, and everything keyed by name follows the
//! name (see [`journal::Step`]). Each step is idempotent, so a crash is repaired at engine
//! start by [`recover_all`]: rolled forward when every step that matters was applied, rolled
//! back otherwise. `silo undo main` afterwards swaps the names and settings back.

pub mod journal;
#[cfg(test)]
mod tests;

use std::{any::Any, time::Duration};

use serde::Serialize;
use serde_json::{json, Value};

use super::keep::{lane, CheckpointRef, ManifestStore, Os};
pub use journal::{Journal, Record, State, Step, Swap};

/// How long a promote or undo waits for another operation on one of the two computers.
pub const LANE_WAIT: Duration = Duration::from_secs(120);

/// What a promote needs to know about a computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub id: String,
    pub name: String,
    pub os: Os,
    pub running: bool,
    /// Why the computer cannot be taken now (a job, a checkpoint, a keep), when it cannot.
    pub busy: Option<String>,
}

/// The engine side of a promote. Tests supply a fake; the engine supplies
/// `engine_env::EngineEnv`.
pub trait PromoteEnv: Send + Sync {
    fn info(&self, id: &str) -> Result<Info, String>;
    /// Whether any computer already has `name`.
    fn name_in_use(&self, name: &str) -> bool;
    /// A sortable local time stamp for the archive name, such as `20261105-1432`.
    fn stamp(&self) -> String;
    /// Saves the `before-promote` checkpoint of main.
    fn checkpoint(&self, id: &str, name: &str) -> Result<CheckpointRef, String>;
    fn stop(&self, id: &str) -> Result<(), String>;
    /// Holds the computer's operation lane until the value is dropped.
    fn hold(&self, _id: &str, _label: &str) -> Result<Box<dyn Any>, String> {
        Ok(Box::new(()))
    }
    /// Everything the stores keep under the computer's name, as one document.
    fn export_bindings(&self, name: &str) -> Result<Value, String>;
    /// Does one step. Repeating it changes nothing.
    fn apply(&self, step: Step, swap: &Swap) -> Result<(), String>;
    /// Undoes one step, whether or not it was applied. Repeating it changes nothing.
    fn revert(&self, step: Step, swap: &Swap) -> Result<(), String>;
    fn note(&self, _line: &str) {}
}

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub fork: String,
    /// `None` promotes into the fork's source.
    pub target: Option<String>,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub fork: String,
    pub target: String,
    /// The name the old main now has.
    pub archive: String,
    pub steps: Vec<String>,
    pub notes: Vec<String>,
    pub checkpoint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Promoted(Report),
    DryRun(Report),
    /// Nothing was changed.
    Refused {
        report: Report,
        reason: String,
        message: String,
    },
}

impl Outcome {
    pub fn to_json(&self) -> Value {
        match self {
            Outcome::Promoted(report) => json!({"status": "promoted", "report": report}),
            Outcome::DryRun(report) => json!({"status": "dry-run", "report": report}),
            Outcome::Refused {
                report,
                reason,
                message,
            } => json!({
                "status": "refused",
                "report": report,
                "refusal": {"reason": reason, "message": message, "conflicts": [], "hint": ""},
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Undone {
    /// The computer that is main again.
    pub computer: String,
    pub archive: String,
    pub recovered: bool,
}

impl Undone {
    pub fn to_json(&self) -> Value {
        json!({
            "status": "undone",
            "computer": self.computer,
            "recovered": self.recovered,
            "changedSince": [],
            "swapped": {"main": self.computer, "was": self.archive},
        })
    }
}

fn plan_steps(macos: bool) -> Vec<Step> {
    Step::ORDER
        .into_iter()
        .filter(|step| macos || *step != Step::MacosIdentity)
        .collect()
}

/// `main-before-<stamp>`, with a number added when the name is taken.
fn archive_name(env: &dyn PromoteEnv, target_name: &str) -> String {
    let base = format!("{target_name}-before-{}", env.stamp());
    let mut name = base.clone();
    let mut n = 1;
    while env.name_in_use(&name) {
        n += 1;
        name = format!("{base}-{n}");
    }
    name
}

fn refuse(report: Report, reason: &str, message: String) -> Outcome {
    Outcome::Refused {
        report,
        reason: reason.into(),
        message,
    }
}

/// Takes both lanes in a fixed order so two promotes cannot wait on each other.
fn lanes(a: &str, b: &str, wait: Duration) -> Result<(lane::LaneGuard, lane::LaneGuard), String> {
    let (first, second) = if a <= b { (a, b) } else { (b, a) };
    let one = lane::acquire(first, wait)?;
    let two = lane::acquire(second, wait)?;
    Ok((one, two))
}

/// Makes `request.fork` the computer `target` was.
pub fn promote(
    env: &dyn PromoteEnv,
    store: &dyn ManifestStore,
    journal: &Journal,
    request: &Request,
    wait: Duration,
) -> Result<Outcome, String> {
    let target = match &request.target {
        Some(target) => target.clone(),
        None => store.source(&request.fork)?.ok_or_else(|| {
            format!(
                "Silo does not know where {} was forked from. Name the computer it replaces.",
                request.fork
            )
        })?,
    };
    if target == request.fork {
        return Err("A computer cannot replace itself.".into());
    }
    let _lanes = lanes(&target, &request.fork, wait)?;
    recover_ids(env, journal, &[&target, &request.fork])?;

    let main = env.info(&target).map_err(|_| {
        format!("{target} is not a computer on this device. Name the computer to replace.")
    })?;
    let fork = env.info(&request.fork)?;
    let steps = plan_steps(main.os == Os::Macos);
    let archive = archive_name(env, &main.name);
    let mut report = Report {
        fork: fork.name.clone(),
        target: main.name.clone(),
        archive: archive.clone(),
        steps: steps.iter().map(|s| s.label().to_owned()).collect(),
        ..Report::default()
    };
    if main.os != fork.os {
        return Ok(refuse(
            report,
            "os",
            format!(
                "{} and {} run different systems; a fork can only replace its own kind.",
                fork.name, main.name
            ),
        ));
    }
    for info in [&main, &fork] {
        if let Some(why) = &info.busy {
            return Ok(refuse(
                report,
                "busy",
                format!(
                    "{} is busy ({why}). Nothing was changed; retry when it is idle.",
                    info.name
                ),
            ));
        }
    }
    if request.dry_run {
        report.notes.push(format!(
            "{} would become {} and {} would be archived as {archive}.",
            fork.name, main.name, main.name
        ));
        return Ok(Outcome::DryRun(report));
    }

    env.note("Saving a checkpoint of main");
    let checkpoint = env.checkpoint(&target, &format!("Before promote of {}", fork.name))?;
    report.checkpoint = Some(checkpoint.id.clone());
    env.note("Stopping both computers");
    for info in [&main, &fork] {
        if info.running {
            env.stop(&info.id)?;
        }
    }
    let _holds = (
        env.hold(&target, &format!("Replacing it with {}", fork.name))?,
        env.hold(&request.fork, "Becoming main")?,
    );
    for id in [&target, &request.fork] {
        let now = env.info(id)?;
        if now.running || now.busy.is_some() {
            return Ok(refuse(
                report,
                "busy",
                format!(
                    "{} started again while the promote was starting. Nothing was changed.",
                    now.name
                ),
            ));
        }
    }

    let swap = Swap {
        fork_id: fork.id.clone(),
        fork_name: fork.name.clone(),
        target_id: main.id.clone(),
        target_name: main.name.clone(),
        archive_name: archive,
        macos: main.os == Os::Macos,
        fork_bindings: env.export_bindings(&fork.name)?,
        fork_source: store.source(&fork.id)?,
        checkpoint,
    };
    let mut record = Record {
        swap,
        state: State::Promoting,
        steps: Vec::new(),
    };
    journal.save(&record)?;
    for step in &steps {
        env.note(&format!("Moving {}", step.label()));
        match env.apply(*step, &record.swap) {
            Ok(()) => {}
            Err(error) if *step == Step::Quiet => {
                report
                    .notes
                    .push(format!("Quiet parking was not restored: {error}"));
            }
            Err(error) => {
                let back = roll_back(env, journal, &mut record);
                return Err(match back {
                    Ok(()) => format!(
                        "{error} The promote was rolled back; {} is main again.",
                        main.name
                    ),
                    Err(also) => format!(
                        "{error} Rolling back also failed ({also}); restarting Silo retries it."
                    ),
                });
            }
        }
        record.steps.push(*step);
        journal.save(&record)?;
    }
    record.state = State::Promoted;
    journal.save(&record)?;
    report.notes.push(format!(
        "{} is now {}. {} was archived as {}; `silo undo {}` swaps them back.",
        fork.name, main.name, main.name, record.swap.archive_name, main.name
    ));
    Ok(Outcome::Promoted(report))
}

/// Reverts every step in reverse order and marks the record over.
fn roll_back(env: &dyn PromoteEnv, journal: &Journal, record: &mut Record) -> Result<(), String> {
    for step in plan_steps(record.swap.macos).into_iter().rev() {
        env.revert(step, &record.swap)?;
    }
    record.steps.clear();
    record.state = State::Over;
    journal.save(record)
}

/// Swaps a promoted fork and the archived old main back. `Ok(None)` when no promote involves
/// `computer`, so the caller falls back to the keep undo.
pub fn undo(
    env: &dyn PromoteEnv,
    journal: &Journal,
    computer: &str,
    wait: Duration,
) -> Result<Option<Undone>, String> {
    let Some(record) = journal
        .live_for(computer)
        .into_iter()
        .find(|r| matches!(r.state, State::Promoted | State::Undoing | State::Promoting))
    else {
        return Ok(None);
    };
    let (fork, target) = (record.swap.fork_id.clone(), record.swap.target_id.clone());
    let _lanes = lanes(&fork, &target, wait)?;
    // Look again with the lanes held: a recovery or another undo may have finished it.
    let Some(mut record) = journal
        .live_for(&fork)
        .into_iter()
        .find(|r| r.swap.target_id == target)
    else {
        return Ok(None);
    };
    let recovered = record.state != State::Promoted;
    for id in [&fork, &target] {
        let info = env.info(id)?;
        if let Some(why) = info.busy {
            return Err(format!(
                "{} is busy ({why}). Nothing was changed.",
                info.name
            ));
        }
        if info.running {
            env.stop(id)?;
        }
    }
    let _holds = (
        env.hold(&fork, "Swapping back")?,
        env.hold(&target, "Becoming main again")?,
    );
    finish_undo(env, journal, &mut record)?;
    Ok(Some(Undone {
        computer: record.swap.target_name.clone(),
        archive: record.swap.archive_name.clone(),
        recovered,
    }))
}

fn finish_undo(env: &dyn PromoteEnv, journal: &Journal, record: &mut Record) -> Result<(), String> {
    if record.state != State::Undoing {
        record.state = State::Undoing;
        journal.save(record)?;
    }
    env.note("Swapping the computers back");
    roll_back(env, journal, record)
}

/// Finishes the promote or undo that `ids` were part of, if the engine stopped mid-way.
fn recover_ids(env: &dyn PromoteEnv, journal: &Journal, ids: &[&str]) -> Result<(), String> {
    for id in ids {
        for mut record in journal.live_for(id) {
            if matches!(record.state, State::Promoting | State::Undoing) {
                recover_record(env, journal, &mut record)?;
            }
        }
    }
    Ok(())
}

/// A promote is rolled forward when every step but the best-effort one was applied, else
/// rolled back; an undo is always finished.
fn recover_record(
    env: &dyn PromoteEnv,
    journal: &Journal,
    record: &mut Record,
) -> Result<(), String> {
    env.note("A promote was interrupted; finishing it");
    match record.state {
        State::Undoing => finish_undo(env, journal, record),
        State::Promoting => {
            let needed: Vec<Step> = plan_steps(record.swap.macos)
                .into_iter()
                .filter(|s| *s != Step::Quiet)
                .collect();
            if needed.iter().all(|s| record.steps.contains(s)) {
                for step in plan_steps(record.swap.macos) {
                    if !record.steps.contains(&step) {
                        let _ = env.apply(step, &record.swap);
                        record.steps.push(step);
                    }
                }
                record.state = State::Promoted;
                journal.save(record)
            } else {
                roll_back(env, journal, record)
            }
        }
        _ => Ok(()),
    }
}

/// Computers with an interrupted promote or undo still to settle.
pub fn interrupted_ids(journal: &Journal) -> Vec<String> {
    journal
        .interrupted()
        .into_iter()
        .map(|r| r.swap.fork_id)
        .collect()
}

/// Settles one interrupted record, with both lanes held. `held` runs once they are.
pub fn recover_one(
    env: &dyn PromoteEnv,
    journal: &Journal,
    fork_id: &str,
    held: &dyn Fn(),
) -> Result<(), String> {
    let Some(mut record) = journal
        .interrupted()
        .into_iter()
        .find(|r| r.swap.fork_id == fork_id)
    else {
        held();
        return Ok(());
    };
    let _lanes = lanes(
        &record.swap.fork_id,
        &record.swap.target_id,
        Duration::from_secs(30),
    )?;
    held();
    recover_record(env, journal, &mut record)
}

/// Settles every interrupted promote; returns the failures.
pub fn recover_all(env: &dyn PromoteEnv, journal: &Journal) -> Vec<String> {
    interrupted_ids(journal)
        .into_iter()
        .filter_map(|id| {
            recover_one(env, journal, &id, &|| {})
                .err()
                .map(|e| format!("{id}: {e}"))
        })
        .collect()
}
