//! Keep and undo: bring a fork's reviewed changes onto its source (main), or put main back.
//!
//! A keep pins to the diff the user reviewed (a digest), takes a `before-keep` checkpoint of
//! main, holds main's operation lane, and only then compares the fork with main and the
//! fork-time base (three-way). Everything it will write is read from the fork first. An item
//! both sides changed is a question: the request's `choices` answer it (the fork's version,
//! main's, or for a repository a branch), and a conflict without an answer stops the keep
//! before main is touched with the list of questions. A keep that would bring credentials or
//! carry a database change nothing migrates stops the same way. A journal written before the
//! first change lets a restart put main back from the checkpoint.

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

pub use crate::changes::items::{Answer, Choices};
pub use env::{CheckpointRef, ExecOut, GuestCmd, KeepEnv, ManifestStore, Os, RunAs};
pub use journal::Journal;
pub use plan::{Conflict, Plan, ReportItem};
pub use prepare::review_digest;

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

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub fork: String,
    /// `None` keeps into the fork's source.
    pub target: Option<String>,
    pub hooks: Hooks,
    pub dry_run: bool,
    /// Carry database changes even though no migration covers them.
    pub data_ok: bool,
    /// The review digest the user saw (`silo diff --json`); the keep refuses if the fork moved.
    pub reviewed: Option<String>,
    /// How the caller settles items both sides changed, and which items to leave out.
    pub choices: Choices,
}

/// How long a keep or undo waits for another one on the same target.
pub const LANE_WAIT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Refusal {
    /// `choices-needed`, `database`, `credentials`, `changed` or `main-changed`.
    pub reason: String,
    pub message: String,
    pub conflicts: Vec<Conflict>,
    pub hint: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub fork: String,
    pub target: String,
    pub summary: Vec<String>,
    pub steps: Vec<String>,
    pub notes: Vec<String>,
    pub checkpoint: Option<String>,
    /// The digest of the fork's changes this keep acted on.
    pub review_digest: Option<String>,
    /// What was done with each item, by id; empty when the keep was refused.
    pub items: Vec<ReportItem>,
    /// Whether main was unchanged since the fork was made, so nothing could conflict. Unknown
    /// (`None`) when the keep stopped before main was looked at.
    pub fast_forward: Option<bool>,
    /// The packages and settings files this keep brings over, as the recipe of the target would
    /// list them.
    #[serde(skip)]
    pub additions: crate::recipes::Additions,
    /// What was appended to the recipe of the target after the keep; absent when nothing was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe: Option<RecipeUpdate>,
}

/// What a keep added to the recipe of the computer it merged into.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeUpdate {
    /// The recipe's name.
    pub name: String,
    pub packages: std::collections::BTreeMap<String, Vec<String>>,
    pub files: Vec<String>,
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
        /// What changed on the computer after the keep; the undo discards it.
        changed_since: Vec<String>,
        /// How main came back, and anything the undo could not do.
        resumed: Resumed,
        notes: Vec<String>,
    },
    NothingToUndo {
        computer: String,
    },
}

/// What state main is in once an undo restored it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resumed {
    /// Running again from the checkpoint's memory, processes included.
    Memory,
    /// Left stopped: it was stopped when the keep started, or it could not be started.
    Stopped,
}

impl Resumed {
    fn label(self) -> &'static str {
        match self {
            Resumed::Memory => "memory",
            Resumed::Stopped => "stopped",
        }
    }
}

impl UndoOutcome {
    pub fn to_json(&self) -> Value {
        match self {
            UndoOutcome::Undone {
                computer,
                checkpoint,
                recovered,
                changed_since,
                resumed,
                notes,
            } => json!({
                "status": "undone",
                "computer": computer,
                "recovered": recovered,
                "changedSince": changed_since,
                "main": if *resumed == Resumed::Stopped { "stopped" } else { "running" },
                "resumedFrom": resumed.label(),
                "notes": notes,
                "checkpoint": {"id": checkpoint.id, "name": checkpoint.name, "createdAt": checkpoint.created_at},
            }),
            UndoOutcome::NothingToUndo { computer } => {
                json!({"status": "nothing-to-undo", "computer": computer})
            }
        }
    }
}

fn refused_early(request: &Request, target: &str, digest: &str) -> Outcome {
    Outcome::Refused {
        report: Report {
            fork: request.fork.clone(),
            target: target.to_owned(),
            review_digest: Some(digest.to_owned()),
            ..Report::default()
        },
        refusal: Refusal {
            reason: "changed".into(),
            message: format!(
                "{} changed since it was reviewed. {target} was not changed.",
                request.fork
            ),
            conflicts: vec![Conflict::new(
                "fork",
                request.fork.clone(),
                "its changes differ from the reviewed diff",
            )],
            hint: "Review it again with `silo diff --json` and merge with the new digest.".into(),
        },
    }
}

fn main_changed(request: &Request, target: &str, digest: &str, lines: Vec<String>) -> Outcome {
    Outcome::Refused {
        report: Report {
            fork: request.fork.clone(),
            target: target.to_owned(),
            review_digest: Some(digest.to_owned()),
            ..Report::default()
        },
        refusal: Refusal {
            reason: "main-changed".into(),
            message: format!(
                "{target} changed while the keep was starting. {target} was not changed by the keep."
            ),
            conflicts: lines
                .into_iter()
                .map(|line| Conflict::new("main", target.to_owned(), line))
                .collect(),
            hint: "Merge again once nothing else is changing it.".into(),
        },
    }
}

/// What differs between two manifests of the same computer, as report lines; `None` when
/// nothing the keep will write or check moved: its files, packages, crontabs, agents and the
/// repos it merges. Processes, ports and background churn elsewhere do not count.
fn main_moved(
    view: &prepare::ForkView,
    os: Os,
    request: &Request,
    before: &str,
    now: &str,
) -> Option<Vec<String>> {
    use crate::changes::manifest::Manifest;
    if before == now {
        return None;
    }
    let (Ok(before), Ok(now)) = (Manifest::parse(before), Manifest::parse(now)) else {
        return Some(vec![
            "Silo could not compare main before and after the checkpoint.".into(),
        ]);
    };
    let items = |main: &Manifest| {
        let plan = plan::build(&view.diff, &view.base, &view.fork, main, os, request);
        prepare::read_signature(&plan, main)
    };
    let (first, second) = (items(&before), items(&now));
    if first == second {
        return None;
    }
    let mut lines: Vec<String> = second
        .iter()
        .filter(|item| !first.contains(item))
        .cloned()
        .collect();
    if lines.is_empty() {
        lines.push("what the keep would write changed".into());
    }
    Some(lines)
}

/// The request with the `keep` hooks of the recipe `target` was built from, when it has one:
/// the engine applies the computer's own rules, whatever the caller sent. A computer built
/// from no recipe runs the hooks the caller sent.
fn with_recipe_hooks(request: &Request, target: &str) -> Request {
    let mut effective = request.clone();
    if let Ok(Some(record)) = crate::recipes::records::get(target) {
        let keep = crate::recipes::hooks_of(&record.recipe);
        effective.hooks = Hooks {
            restart: keep.restart,
            migrate: keep.migrate,
            after: keep.after,
        };
    }
    effective
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
    let effective = with_recipe_hooks(request, &target);
    let request = &effective;
    let os = env.os(&target).map_err(|_| {
        format!("{target} is not a computer on this device. Name the computer to keep into.")
    })?;
    let _lane = lane::acquire(&target, wait)?;
    recover_locked(env, journal, &target, &|| {})?;

    let view = prepare::look_at_fork(env, store, &request.fork)?;
    if request
        .reviewed
        .as_deref()
        .is_some_and(|reviewed| reviewed != view.digest)
    {
        return Ok(refused_early(request, &target, &view.digest));
    }
    // The digest matched (or none was sent), so the diff in hand is the reviewed one. The
    // choices are checked against it and are not part of the digest: it pins what the fork
    // changed, and a choice answers how that meets main, which moves on its own.
    crate::changes::items::validate(&request.choices, &view.diff)?;
    let mut tracker = prepare::Tracker {
        fork: request.fork.clone(),
        target: target.clone(),
        ..Default::default()
    };

    if request.dry_run {
        let prepared = prepare::prepare(env, store, request, &target, &view, &mut tracker, true);
        tracker.cleanup(env);
        return Ok(match prepared? {
            prepare::Prepared::Refused { report, refusal } => Outcome::Refused { report, refusal },
            prepare::Prepared::Ready(ready) => Outcome::DryRun(ready.report),
        });
    }

    // Every refusal that does not need main held is decided before anything is created, so a
    // refused keep is quick and leaves no checkpoint. The same checks run again once main
    // is held, because main may have moved since.
    let preflight = prepare::prepare(env, store, request, &target, &view, &mut tracker, false);
    tracker.cleanup(env);
    if let prepare::Prepared::Refused { report, refusal } = preflight? {
        return Ok(Outcome::Refused { report, refusal });
    }

    let main_before = store.capture(&target)?;
    env.note("Saving a checkpoint of main");
    let checkpoint =
        env.checkpoint_before_keep(&target, &format!("Before keep of {}", request.fork))?;
    journal.record_pending(&target, &checkpoint)?;
    journal.tag(
        &target,
        &checkpoint.id,
        &request.fork,
        request.reviewed.as_deref().unwrap_or_default(),
    )?;
    journal.begin(&journal::Entry {
        fork: request.fork.clone(),
        target: target.clone(),
        checkpoint_id: checkpoint.id.clone(),
        checkpoint_name: checkpoint.name.clone(),
        checkpoint_scope: checkpoint.scope.clone(),
    })?;
    let consume = |journal: &Journal| {
        let _ = journal.set_state(&target, &checkpoint.id, journal::State::Consumed);
        journal.clear(&target);
    };

    // From here to the end of the apply, main's lane is held.
    let applied = {
        let _gate = match env.hold(&target, &format!("Keeping {}", request.fork)) {
            Ok(gate) => gate,
            Err(error) => {
                consume(journal);
                return Err(error);
            }
        };
        // A restore after a failure goes back to the checkpoint, so main must still be what
        // the checkpoint saved now that nothing else can change it.
        let moved = store
            .capture(&target)
            .map(|now| main_moved(&view, os, request, &main_before, &now));
        match moved {
            Ok(None) => {}
            Ok(Some(lines)) => {
                consume(journal);
                return Ok(main_changed(request, &target, &view.digest, lines));
            }
            Err(error) => {
                consume(journal);
                return Err(error);
            }
        }
        let prepared = prepare::prepare(env, store, request, &target, &view, &mut tracker, true);
        let result = match prepared {
            Ok(prepare::Prepared::Refused { report, refusal }) => {
                Ok(Err(Outcome::Refused { report, refusal }))
            }
            Ok(prepare::Prepared::Ready(ready)) => {
                apply::apply(env, &ready, request, &target).map(|applied| Ok((ready, applied)))
            }
            // Looking at main and the fork changed nothing on main, so there is nothing to
            // put back.
            Err(error) => {
                tracker.cleanup(env);
                consume(journal);
                return Err(error);
            }
        };
        tracker.cleanup(env);
        result
    };
    match applied {
        Ok(Err(refused)) => {
            consume(journal);
            Ok(refused)
        }
        Ok(Ok((ready, applied))) => {
            let mut report = ready.report;
            report.checkpoint = Some(checkpoint.id.clone());
            report.steps = applied.steps;
            report.notes.extend(applied.notes);
            match store.capture(&target) {
                Ok(after) => {
                    if let Err(error) = journal.save_after(&target, &checkpoint.id, &after) {
                        report.notes.push(format!(
                            "Undo will not be able to tell what changes after this keep: {error}"
                        ));
                    }
                }
                Err(error) => report.notes.push(format!(
                    "Undo will not be able to tell what changes after this keep: {error}"
                )),
            }
            journal.set_state(&target, &checkpoint.id, journal::State::Undoable)?;
            journal.clear(&target);
            env.after_kept(&target);
            Ok(Outcome::Kept(report))
        }
        Err(error) => {
            env.note("Putting main back from the checkpoint");
            let restored = env.restore(&target, &checkpoint.id);
            if restored.is_ok() {
                consume(journal);
            }
            Err(match restored {
                Ok(()) => {
                    let (_, notes) = bring_back(env, &target, &checkpoint);
                    format!(
                        "{error} Main was put back from the checkpoint taken before the keep. {}",
                        notes.join(" ")
                    )
                    .trim_end()
                    .to_owned()
                }
                Err(restore) => format!(
                    "{error} Putting main back also failed ({restore}); `silo undo {target}` retries it."
                ),
            })
        }
    }
}

/// Puts main back from the latest before-keep checkpoint a successful keep made.
pub fn undo(
    env: &dyn KeepEnv,
    store: &dyn ManifestStore,
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
            scope: entry.checkpoint_scope.clone(),
        };
        env.restore(computer, &entry.checkpoint_id)?;
        journal.set_state(computer, &entry.checkpoint_id, journal::State::Consumed)?;
        journal.clear(computer);
        let (resumed, notes) = bring_back(env, computer, &checkpoint);
        return Ok(UndoOutcome::Undone {
            computer: computer.to_owned(),
            checkpoint,
            recovered: true,
            changed_since: Vec::new(),
            resumed,
            notes,
        });
    }
    let Some(checkpoint) = journal.latest_undoable(computer) else {
        return Ok(UndoOutcome::NothingToUndo {
            computer: computer.to_owned(),
        });
    };
    let changed_since = changed_after(store, journal, computer, &checkpoint.id);
    env.restore(computer, &checkpoint.id)?;
    journal.set_state(computer, &checkpoint.id, journal::State::Consumed)?;
    let (resumed, notes) = bring_back(env, computer, &checkpoint);
    Ok(UndoOutcome::Undone {
        computer: computer.to_owned(),
        checkpoint,
        recovered: false,
        changed_since,
        resumed,
        notes,
    })
}

/// A restore leaves main stopped. Main goes back the way it was when the keep started: a
/// checkpoint with memory means main was running, and starting it resumes that memory (its
/// processes and an open database as they were); a disk checkpoint means it was stopped.
fn bring_back(
    env: &dyn KeepEnv,
    computer: &str,
    checkpoint: &CheckpointRef,
) -> (Resumed, Vec<String>) {
    match checkpoint.scope.as_str() {
        "full" => {
            env.note("Starting main from the checkpoint's memory");
            match env.start(computer) {
                Ok(()) => (Resumed::Memory, Vec::new()),
                Err(error) => (
                    Resumed::Stopped,
                    vec![format!(
                        "Main was put back but did not start: {error} Start it with `silo start {computer}`."
                    )],
                ),
            }
        }
        "disk" => (Resumed::Stopped, Vec::new()),
        _ => (
            Resumed::Stopped,
            vec![format!(
                "Silo could not tell whether {computer} was running before the keep, so it was left stopped. Start it with `silo start {computer}` if it was."
            )],
        ),
    }
}

/// What changed on the computer since the keep left it, as report lines.
fn changed_after(
    store: &dyn ManifestStore,
    journal: &Journal,
    computer: &str,
    checkpoint_id: &str,
) -> Vec<String> {
    use crate::changes::{diff::diff, ignore::IgnoreList, manifest::Manifest};
    let Some(after) = journal.after(computer, checkpoint_id) else {
        return vec!["Silo could not tell whether the computer changed after the keep.".into()];
    };
    let (Ok(before), Ok(now)) = (
        Manifest::parse(&after),
        store
            .capture(computer)
            .and_then(|t| Manifest::parse(&t).map_err(|e| e.to_string())),
    ) else {
        return vec!["Silo could not tell whether the computer changed after the keep.".into()];
    };
    let changes = diff(&before, &now, None, &IgnoreList::builtin());
    let lines = changes.render_lines(15);
    if changes
        .summary
        .iter()
        .all(|s| s.lines.is_empty() || s.bucket == "Not kept")
    {
        return Vec::new();
    }
    lines
}

/// A journal found while no keep runs means the engine stopped mid-keep.
fn recover_locked(
    env: &dyn KeepEnv,
    journal: &Journal,
    target: &str,
    held: &dyn Fn(),
) -> Result<(), String> {
    let Some(entry) = journal.pending(target)? else {
        held();
        return Ok(());
    };
    env.note("A keep was interrupted; putting main back first");
    env.recover(target, &entry.checkpoint_id, held)?;
    let _ = journal.set_state(target, &entry.checkpoint_id, journal::State::Consumed);
    journal.clear(target);
    Ok(())
}

/// Restores the interrupted keep of one computer. `held` is called once its lane is held,
/// before the restore starts, with "recovering an interrupted keep" shown to anything
/// that queues for it meanwhile.
pub fn recover_one(
    env: &dyn KeepEnv,
    journal: &Journal,
    target: &str,
    held: &dyn Fn(),
) -> Result<(), String> {
    let _lane = lane::acquire(target, Duration::from_secs(30))?;
    recover_locked(env, journal, target, held)
}

/// Computers with an interrupted keep still to restore.
pub fn interrupted_targets(journal: &Journal) -> Vec<String> {
    journal
        .pending_targets()
        .into_iter()
        .map(|entry| entry.target)
        .collect()
}

/// Restores every interrupted keep.
pub fn recover_all(env: &dyn KeepEnv, journal: &Journal) -> Vec<String> {
    let mut failures = Vec::new();
    for target in interrupted_targets(journal) {
        if let Err(error) = recover_one(env, journal, &target, &|| {}) {
            failures.push(format!("{target}: {error}"));
        }
    }
    failures
}
