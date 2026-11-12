//! Make this main: a fork replaces the computer it was made from.
//!
//! Computers have a stable id and a name; the name is what `main` means to a person and to
//! every store keyed by name. A promote checkpoints main, stops both computers, holds both
//! operation lanes, and then moves the identity, one journaled step at a time: the fork takes
//! main's name, main becomes `main-before-<time>`, and everything keyed by name follows the
//! name (see [`journal::Step`]). Each step is idempotent, so a crash is repaired at engine
//! start by [`recover_all`]: rolled forward when every step that matters was applied, rolled
//! back otherwise. `silo undo main` afterwards swaps the names and settings back, but only
//! the latest promote of a computer, and only while the two still carry the names the promote
//! gave them.

pub mod engine_env;
pub mod journal;
pub mod lineage;
pub mod linux;
pub mod service;
#[cfg(test)]
mod tests;

use std::{any::Any, time::Duration};

use serde::Serialize;
use serde_json::{json, Value};

use super::keep::{lane, CheckpointRef, ManifestStore, Os, UnderLane};
use crate::policy::handoff::Handoff;
use journal::now_millis;
pub use journal::{Journal, Record, State, Step, Swap};

/// How long a promote or undo waits for another operation on one of the two computers.
pub const LANE_WAIT: Duration = Duration::from_secs(120);

/// How long a computer may take to finish stopping before a promote or undo gives up.
#[cfg(not(test))]
const STOP_WAIT: Duration = Duration::from_secs(120);
#[cfg(test)]
const STOP_WAIT: Duration = Duration::from_millis(50);

/// Stops each running computer and waits until it reports stopped: a computer that is still
/// stopping looks running, and the request to stop it returns before it is done.
fn stop_and_wait(env: &dyn PromoteEnv, ids: &[&str]) -> Result<(), String> {
    for id in ids {
        if env.info(id)?.running {
            env.stop(id)?;
        }
    }
    let begun = std::time::Instant::now();
    for id in ids {
        loop {
            let info = env.info(id)?;
            if !info.running {
                break;
            }
            if begun.elapsed() > STOP_WAIT {
                return Err(format!(
                    "{} did not finish stopping. Nothing was changed; retry in a moment.",
                    info.name
                ));
            }
            env.settle();
        }
    }
    Ok(())
}

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
    /// Why computers of this kind cannot be promoted on this device, when they cannot.
    fn unsupported(&self, _os: Os) -> Option<String> {
        None
    }
    /// Why these two particular computers cannot swap names, when they cannot (a disk stored
    /// under a computer's name, say).
    fn refusal(&self, _main: &Info, _fork: &Info) -> Option<String> {
        None
    }
    /// Whether any computer already has `name`.
    fn name_in_use(&self, name: &str) -> bool;
    /// A sortable local time stamp for the archive name, such as `20261105-1432`.
    fn stamp(&self) -> String;
    /// Saves the `before-promote` checkpoint of main.
    fn checkpoint(&self, id: &str, name: &str) -> Result<CheckpointRef, String>;
    fn stop(&self, id: &str) -> Result<(), String>;
    /// Starts the computer again after a promote or an undo.
    fn start(&self, _id: &str) -> Result<(), String> {
        Ok(())
    }
    /// Waits a moment before a computer that is still stopping is looked at again.
    fn settle(&self) {
        std::thread::sleep(Duration::from_millis(250));
    }
    /// Holds the computer's operation lane until the value is dropped.
    fn hold(&self, _id: &str, _label: &str) -> Result<Box<dyn Any>, String> {
        Ok(Box::new(()))
    }
    /// Holds both computers' operation lanes until the value is dropped. The gate admits one
    /// operation per thread, so an environment backed by it takes both under one admission.
    fn hold_pair(&self, first: &str, second: &str, label: &str) -> Result<Box<dyn Any>, String> {
        Ok(Box::new((
            self.hold(first, label)?,
            self.hold(second, label)?,
        )))
    }
    /// Everything the stores keep under the computer's name, as one document.
    fn export_bindings(&self, id: &str, name: &str) -> Result<Value, String>;
    /// Who made which of the two computers, read before the swap and given back to `apply` and
    /// `revert` in the swap.
    fn export_makers(&self, _fork_id: &str, _target_id: &str) -> Result<Handoff, String> {
        Ok(Handoff::default())
    }
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
    /// The name the promoted fork had before, and has again.
    pub fork: String,
    pub archive: String,
    pub recovered: bool,
}

impl Undone {
    pub fn to_json(&self) -> Value {
        json!({
            "status": "undone",
            "kind": "promote",
            "computer": self.computer,
            "fork": self.fork,
            "archive": self.archive,
            "recovered": self.recovered,
            "changedSince": [],
        })
    }
}

fn plan_steps(macos: bool) -> Vec<Step> {
    Step::ORDER
        .into_iter()
        .filter(|step| macos || *step != Step::MacosIdentity)
        .collect()
}

/// The longest a computer name may be.
const NAME_LIMIT: usize = 32;

/// `main-before-<stamp>`, with a number added when the name is taken. The target's part is
/// shortened as far as it takes, whatever the number, so the result is at most [`NAME_LIMIT`]
/// characters; a target name with nothing left of it (only dashes, or empty) becomes `computer`.
fn archive_name(env: &dyn PromoteEnv, target_name: &str) -> String {
    let tail = format!("-before-{}", env.stamp());
    let build = |suffix: &str| {
        let room = NAME_LIMIT.saturating_sub(tail.len() + suffix.len());
        let shorten = |text: &str| {
            text.chars()
                .take(room)
                .collect::<String>()
                .trim_end_matches('-')
                .to_owned()
        };
        let mut head = shorten(target_name);
        if head.is_empty() {
            head = shorten("computer");
        }
        let name = format!("{head}{tail}{suffix}");
        let name: String = name.chars().take(NAME_LIMIT).collect();
        name.trim_start_matches('-').to_owned()
    };
    let mut name = build("");
    let mut n = 1;
    while env.name_in_use(&name) {
        n += 1;
        name = build(&format!("-{n}"));
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
    promote_checked(env, store, journal, request, wait, &|| Ok(()))
}

/// [`promote`], after `under_lane` accepts the state of the fork once both lanes are held.
pub fn promote_checked(
    env: &dyn PromoteEnv,
    store: &dyn ManifestStore,
    journal: &Journal,
    request: &Request,
    wait: Duration,
    under_lane: UnderLane<'_>,
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
    under_lane()?;
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
    if let Some(why) = env.unsupported(main.os) {
        return Ok(refuse(report, "unsupported", why));
    }
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
    if main.name.eq_ignore_ascii_case(&fork.name) {
        return Ok(refuse(
            report,
            "name",
            format!(
                "{} and {} have the same name; Silo will not leave two computers named {}.",
                fork.name, main.name, main.name
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
    if let Some(why) = env.refusal(&main, &fork) {
        return Ok(refuse(report, "storage", why));
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
    stop_and_wait(env, &[&main.id, &fork.id])?;
    let holds = env.hold_pair(
        &target,
        &request.fork,
        &format!("Replacing {} with {}", main.name, fork.name),
    )?;
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
        fork_bindings: env.export_bindings(&fork.id, &fork.name)?,
        target_bindings: env.export_bindings(&main.id, &main.name)?,
        fork_source: store.source(&fork.id)?,
        target_source: store.source(&main.id)?,
        checkpoint,
        fork_was_running: fork.running,
        target_was_running: main.running,
        makers: env.export_makers(&fork.id, &main.id)?,
    };
    let mut record = Record {
        swap,
        state: State::Promoting,
        at: now_millis(),
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
    record.at = now_millis();
    journal.save(&record)?;
    report.notes.push(format!(
        "{} is now {}. {} was archived as {}; `silo undo {}` swaps them back.",
        fork.name, main.name, main.name, record.swap.archive_name, main.name
    ));
    // The new main runs when the old one did (best effort: the swap is done); the archive stays
    // stopped. A start takes the computer's lane itself, so the swap's hold ends first.
    drop(holds);
    if record.swap.target_was_running {
        env.note("Starting the new main");
        if let Err(error) = env.start(&record.swap.fork_id) {
            report
                .notes
                .push(format!("{} was not started again: {error}", main.name));
        }
    }
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

/// The record `silo undo` would act on for `computer`: the latest promote in force that
/// involves it.
fn latest_for(journal: &Journal, computer: &str) -> Option<Record> {
    journal
        .live_for(computer)
        .into_iter()
        .filter(|r| matches!(r.state, State::Promoted | State::Undoing | State::Promoting))
        .max_by_key(|r| r.at)
}

/// When the latest promote of `computer` finished, if one is in force. A keep made after it
/// is newer, and `silo undo` takes the newest of the two.
pub fn latest_at(journal: &Journal, computer: &str) -> Option<u128> {
    latest_for(journal, computer).map(|r| r.at)
}

/// Whether `silo undo` takes back the promote rather than a keep: the promote is the newest of
/// the two, and no interrupted keep is waiting to be put back.
pub fn promote_is_newest(promoted: Option<u128>, kept: Option<u128>, keep_pending: bool) -> bool {
    match (promoted, kept) {
        (Some(promoted), Some(kept)) => !keep_pending && promoted > kept,
        (Some(_), None) => !keep_pending,
        (None, _) => false,
    }
}

/// Swaps a promoted fork and the archived old main back. `Ok(None)` when no promote involves
/// `computer`, so the caller falls back to the keep undo.
///
/// Only the latest promote that involves either computer is undone, and only while the fork
/// still has the name it was given and the old main still has the archive name; otherwise
/// nothing changes. Ports changed while promoted stay with the computer that holds the main
/// role, which is the old main again afterwards (see `EngineEnv::revert`).
pub fn undo(
    env: &dyn PromoteEnv,
    journal: &Journal,
    computer: &str,
    wait: Duration,
) -> Result<Option<Undone>, String> {
    undo_checked(env, journal, computer, wait, &|| Ok(()))
}

/// [`undo`], after `under_lane` accepts what the undo would take back once the lanes are held.
pub fn undo_checked(
    env: &dyn PromoteEnv,
    journal: &Journal,
    computer: &str,
    wait: Duration,
    under_lane: UnderLane<'_>,
) -> Result<Option<Undone>, String> {
    let Some(record) = latest_for(journal, computer) else {
        return Ok(None);
    };
    let key = (record.swap.fork_id.clone(), record.swap.target_id.clone());
    let _lanes = lanes(&key.0, &key.1, wait)?;
    under_lane()?;
    // Look again with the lanes held: a recovery or another undo may have finished it.
    let Some(mut record) = journal
        .get(&key.0, &key.1)
        .filter(|r| r.state != State::Over)
    else {
        return Ok(None);
    };
    let recovered = record.state != State::Promoted;
    if !recovered {
        // A promote made after this one that involves either computer must be undone first.
        if let Some(later) = journal.in_force().into_iter().find(|other| {
            other.at > record.at
                && [&key.0, &key.1]
                    .iter()
                    .any(|id| other.swap.fork_id == **id || other.swap.target_id == **id)
        }) {
            return Err(format!(
                "{} replaced {} after this promote. Undo that one first (`silo undo {}`). Nothing was changed.",
                later.swap.fork_name, later.swap.target_name, later.swap.fork_name
            ));
        }
    }
    for id in [&key.0, &key.1] {
        let info = env.info(id)?;
        if let Some(why) = info.busy {
            return Err(format!(
                "{} is busy ({why}). Nothing was changed.",
                info.name
            ));
        }
    }
    if !recovered {
        let (fork, main) = (env.info(&key.0)?, env.info(&key.1)?);
        if fork.name != record.swap.target_name || main.name != record.swap.archive_name {
            return Err(format!(
                "{} and {} were renamed after the promote (they are {} and {}; the promote left {} and {}). Rename them back, then undo. Nothing was changed.",
                record.swap.fork_name,
                record.swap.target_name,
                fork.name,
                main.name,
                record.swap.target_name,
                record.swap.archive_name
            ));
        }
        if env.name_in_use(&record.swap.fork_name) {
            return Err(format!(
                "Another computer is named {} now, so {} cannot get its name back. Nothing was changed.",
                record.swap.fork_name, record.swap.target_name
            ));
        }
    }
    stop_and_wait(env, &[&key.0, &key.1])?;
    let holds = env.hold_pair(&key.0, &key.1, "Swapping the computers back")?;
    finish_undo(env, journal, &mut record)?;
    drop(holds);
    // Whatever was running before the promote runs again (best effort: the swap is done).
    for (id, was_running) in [
        (&record.swap.target_id, record.swap.target_was_running),
        (&record.swap.fork_id, record.swap.fork_was_running),
    ] {
        if was_running {
            env.note("Starting the computers that were running");
            let _ = env.start(id);
        }
    }
    Ok(Some(Undone {
        computer: record.swap.target_name.clone(),
        fork: record.swap.fork_name.clone(),
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
                record.at = now_millis();
                journal.save(record)
            } else {
                roll_back(env, journal, record)
            }
        }
        _ => Ok(()),
    }
}

/// The fork and the old main of every interrupted promote or undo.
pub fn interrupted_keys(journal: &Journal) -> Vec<(String, String)> {
    journal
        .interrupted()
        .into_iter()
        .map(|r| (r.swap.fork_id, r.swap.target_id))
        .collect()
}

/// Settles one interrupted record with both computers held from the start. `held` runs once
/// they are, so the caller can let startup go on knowing neither computer can start. The
/// recovery is tried up to `attempts` times, `pause` apart, with the computers still held
/// (the runtime may not be ready yet at engine start).
pub fn recover_one(
    env: &dyn PromoteEnv,
    journal: &Journal,
    key: &(String, String),
    held: &dyn Fn(),
    attempts: u32,
    pause: Duration,
) -> Result<(), String> {
    let _lanes = lanes(&key.0, &key.1, Duration::from_secs(30))?;
    held();
    let mut last = Ok(());
    for attempt in 0..attempts.max(1) {
        let Some(mut record) = journal
            .interrupted()
            .into_iter()
            .find(|r| r.swap.fork_id == key.0 && r.swap.target_id == key.1)
        else {
            return Ok(());
        };
        last = recover_record(env, journal, &mut record);
        if last.is_ok() {
            return Ok(());
        }
        if attempt + 1 < attempts {
            std::thread::sleep(pause);
        }
    }
    last
}

/// Settles every interrupted promote; returns the failures.
pub fn recover_all(env: &dyn PromoteEnv, journal: &Journal) -> Vec<String> {
    interrupted_keys(journal)
        .into_iter()
        .filter_map(|key| {
            recover_one(env, journal, &key, &|| {}, 1, Duration::ZERO)
                .err()
                .map(|e| format!("{}: {e}", key.0))
        })
        .collect()
}
