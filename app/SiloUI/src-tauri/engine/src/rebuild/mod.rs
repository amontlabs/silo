//! Rebuild a Linux computer on the newest guest image, keeping its workspace, its home folder
//! and its SSH identity.
//!
//! The computer is stopped and checkpointed, a second (hidden) sandbox is built beside it on the
//! newest image with the same size, `/home/silo` is carried over as a tar file while both run,
//! then both are stopped for good, the workspace disk is cloned into the new one (so nothing
//! the old computer does can be missed, and it never runs again), the recipe the computer was
//! built from is replayed on the new root disk, and then the two sandboxes swap: the rebuilt
//! one takes the computer's name and id, the old one becomes `<name>-undo` (stopped, on its own
//! id). Every step is journaled. The computer's operation gate and its lane are held from the
//! first step to the last, so nothing can start, stop, drop or checkpoint either sandbox
//! meanwhile.
//!
//! An interrupted rebuild is rolled back at the next engine start while the swap has not
//! begun, and finished once it has (see [`recover_one`]).
//!
//! Whatever is not copied stays behind in `<name>-undo`: things installed by hand outside the
//! home folder and the recipe. The `Before rebuild` checkpoint of the old computer is kept (a
//! rolled-back rebuild keeps it too): it is the way back to the old image on the same computer,
//! and the checkpoint retention removes it in time.

pub mod engine_env;
pub mod journal;
pub mod service;
pub mod swap;
#[cfg(test)]
mod tests;

use std::time::Duration;

pub use journal::{Journal, Plan, Record, Step};
use silo_api::methods::{
    computers::ComputerRebuildResult,
    recipes::{RecipeOs, RecipeSpec},
};

use crate::changes::keep::{lane, CheckpointRef, Os};

/// How long a rebuild waits for another operation on the same computer.
pub const LANE_WAIT: Duration = Duration::from_secs(120);

/// How long a computer may take to finish stopping before a rebuild gives up.
#[cfg(not(test))]
const STOP_WAIT: Duration = Duration::from_secs(120);
#[cfg(test)]
const STOP_WAIT: Duration = Duration::from_millis(50);

/// The longest a computer name may be.
const NAME_LIMIT: usize = 32;

/// The sandbox built beside a computer is called `silo-rebuild-<key>`; like Silo's other
/// working computers it stays out of lists and trees (see `computer_tree::hidden`).
pub const BUILD_PREFIX: &str = "silo-rebuild-";

/// How much of the new root disk the home folder may fill: the tar file is on that disk too,
/// next to the unpacked folder.
const HOME_SHARE_PERCENT: u64 = 45;

/// What a rebuild needs to know about a computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub id: String,
    pub name: String,
    pub os: Os,
    /// A container computer runs on another runtime and has no sandbox to rebuild.
    pub container: bool,
    pub running: bool,
    /// Why the computer cannot be taken now (a checkpoint restore is pending), when it cannot.
    pub busy: Option<String>,
    /// The size of its root disk, in GiB.
    pub root_disk_gib: u32,
    /// The image reference it was built on, when the runtime reports one.
    pub image: Option<String>,
}

/// The engine side of a rebuild. Tests supply a fake; the engine supplies
/// [`engine_env::EngineRebuildEnv`]. Computers are named by id; the real environment looks the
/// sandbox up.
pub trait RebuildEnv: Send + Sync {
    /// Takes the computer's operation gate, which the caller keeps until it drops the guard.
    /// Every other method of the environment runs under it and takes no gate itself.
    fn hold(&self, _id: &str) -> Result<Box<dyn std::any::Any>, String> {
        Ok(Box::new(()))
    }
    /// Shows the step the rebuild is on wherever the operation is listed.
    fn relabel(&self, _label: &str) {}
    /// Fails when the person asked for the rebuild to stop. Asked between the steps that can
    /// still be rolled back, never after the swap begins.
    fn check_cancelled(&self) -> Result<(), String> {
        Ok(())
    }
    /// Runs `work` without letting a request to cancel cut it short: the rollback and the steps
    /// after the swap began must run to the end.
    fn shielded(&self, work: &mut dyn FnMut()) {
        work();
    }
    fn info(&self, id: &str) -> Result<Info, String>;
    /// Why this computer cannot be rebuilt on this device, when it cannot (no copy-on-write
    /// cloning on the host, a disk kept under the computer's name, a resized workspace).
    fn refusal(&self, _info: &Info) -> Option<String> {
        None
    }
    /// Whether any computer already has `name`.
    fn name_in_use(&self, name: &str) -> bool;
    /// The recipe the computer was built from, as recorded when it was built.
    fn recipe(&self, _id: &str) -> Result<Option<RecipeSpec>, String> {
        Ok(None)
    }
    /// The size of `/home/silo` in bytes, or `None` when it cannot be known without starting a
    /// stopped computer (the packing checks the size then, once the rebuild is journaled).
    fn home_bytes(&self, id: &str) -> Result<Option<u64>, String>;
    /// The reference of the newest guest image, ready to build computers from.
    fn newest_image(&self) -> Result<String, String>;
    fn fresh_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
    fn stage_key(&self) -> String {
        uuid::Uuid::new_v4().simple().to_string()[..8].to_owned()
    }
    fn now_millis(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default()
    }
    fn start(&self, id: &str) -> Result<(), String>;
    fn stop(&self, id: &str) -> Result<(), String>;
    /// Saves the disk checkpoint of the stopped computer (with its integrity record).
    fn checkpoint(&self, id: &str, name: &str) -> Result<CheckpointRef, String>;
    /// Builds the new sandbox on `plan.image` with the old computer's size and settings and
    /// leaves it stopped.
    fn create(&self, plan: &Plan) -> Result<(), String>;
    /// Copies `/home/silo` of the running old computer into the running new one.
    fn carry_home(&self, plan: &Plan) -> Result<(), String>;
    /// Gives the new sandbox a clone of the old one's workspace disk. Both are stopped.
    fn move_workspace(&self, plan: &Plan) -> Result<(), String>;
    /// Copies the old sandbox's host-side SSH folder (its host key) to the new one.
    fn copy_ssh(&self, plan: &Plan) -> Result<(), String>;
    /// Runs the recipe's install, setup and service steps in the running new computer.
    fn replay_recipe(&self, id: &str, spec: &RecipeSpec) -> Result<(), String>;
    /// Moves the identities (see [`swap`]). Both sandboxes are stopped. Repeating it changes
    /// nothing.
    fn swap_apply(&self, plan: &Plan) -> Result<(), String>;
    /// Moves them back, whether or not the swap was applied.
    fn swap_revert(&self, plan: &Plan) -> Result<(), String>;
    /// Forgets what the build id and the build name had in the stores, after the swap. Best
    /// effort.
    fn settle_swapped(&self, plan: &Plan) -> Result<(), String>;
    /// Removes the new sandbox and the staged home folder, whatever exists of them.
    fn remove_build(&self, plan: &Plan) -> Result<(), String>;
    /// Removes the home folder's tar file from the old computer, which a stop in the middle of
    /// carrying the home folder leaves there. Best effort.
    fn clean_old_computer(&self, _plan: &Plan) {}
    /// Puts the old sandbox back under the computer's name and id when the identities are in a
    /// state the swap does not know (an abandoned rebuild). `swapping` is whether the swap had
    /// begun.
    fn restore_original(&self, plan: &Plan, _swapping: bool) -> Result<(), String> {
        self.swap_revert(plan)
    }
    /// Tells the owner that the rebuild of this computer could not be settled, or (with
    /// `None`) that nothing waits any more.
    fn stuck_notice(&self, _plan: &Plan, _problem: Option<&str>) {}
    fn note(&self, _line: &str) {}
}

/// `head` shortened so that `head + tail` fits a computer name.
fn fitted(head: &str, tail: &str) -> String {
    let room = NAME_LIMIT.saturating_sub(tail.len());
    let head: String = head.chars().take(room).collect();
    format!("{}{tail}", head.trim_end_matches('-'))
}

/// `<name>-undo`, with the name shortened as far as it takes.
pub fn undo_name(name: &str) -> String {
    fitted(name, "-undo")
}

/// `silo-rebuild-<key>`: the sandbox built beside the computer.
fn build_name(key: &str) -> String {
    format!("{BUILD_PREFIX}{key}")
}

fn stop_and_wait(env: &dyn RebuildEnv, id: &str) -> Result<(), String> {
    if env.info(id)?.running {
        env.stop(id)?;
    }
    let begun = std::time::Instant::now();
    loop {
        let info = env.info(id)?;
        if !info.running {
            return Ok(());
        }
        if begun.elapsed() > STOP_WAIT {
            return Err(format!(
                "{} did not finish stopping. Nothing more was changed.",
                info.name
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn refuse(message: String) -> String {
    format!("{message} Nothing was changed.")
}

/// The name of the host file the home folder of the rebuild staged as `stage` waits in.
pub(crate) fn stage_file_name(stage: &str) -> String {
    format!("home-{stage}.tar")
}

/// Removes the staged home folders in `stage_dir` that no rebuild in the journal waits for: what
/// a rebuild left behind when the engine stopped in the middle of carrying the home folder.
pub fn sweep_stage(journal: &Journal, stage_dir: &std::path::Path) {
    let wanted: std::collections::HashSet<String> = journal
        .all()
        .iter()
        .map(|record| stage_file_name(&record.plan.stage))
        .collect();
    let Ok(read) = std::fs::read_dir(stage_dir) else {
        return;
    };
    for entry in read.flatten() {
        if !wanted.contains(entry.file_name().to_string_lossy().as_ref()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The home folder's size limit on a root disk of `root_disk_gib`.
pub fn home_limit(root_disk_gib: u32) -> u64 {
    u64::from(root_disk_gib) * 1024 * 1024 * 1024 * HOME_SHARE_PERCENT / 100
}

/// Runs `work` where a request to cancel cannot cut it short.
fn shielded<T>(env: &dyn RebuildEnv, work: impl FnOnce() -> T) -> T {
    let mut work = Some(work);
    let mut out = None;
    env.shielded(&mut || {
        if let Some(work) = work.take() {
            out = Some(work());
        }
    });
    out.expect("the environment ran the work")
}

/// The message the owner sees when a rebuild cannot be settled by itself.
fn stuck_message(name: &str, error: &str) -> String {
    format!(
        "The rebuild of {name} was interrupted and could not be settled: {error} Ask Silo to abandon it (silo rebuild {name} --abandon) to put the old computer back."
    )
}

/// Rebuilds the Linux computer `computer_id` on the newest guest image. A computer already on
/// it is refused unless `force` is set.
pub fn rebuild(
    env: &dyn RebuildEnv,
    journal: &Journal,
    computer_id: &str,
    force: bool,
    wait: Duration,
) -> Result<ComputerRebuildResult, String> {
    let _lane = lane::acquire(computer_id, wait)?;
    let _gate = env.hold(computer_id)?;
    // A rebuild this one finds half done is settled first.
    if let Some(record) = journal.get(computer_id) {
        env.note("A rebuild was interrupted; settling it first");
        env.relabel("Settling an interrupted rebuild");
        shielded(env, || recover_record(env, journal, &record))
            .map_err(|error| stuck_message(&record.plan.old_name, &error))?;
    }

    // Every refusal comes before the first change.
    let info = env
        .info(computer_id)
        .map_err(|_| "This computer no longer exists on this device.".to_owned())?;
    if info.os != Os::Linux {
        return Err(refuse(format!(
            "{} is a Mac. Rebuilding on the newest image works for Linux computers only.",
            info.name
        )));
    }
    if info.container {
        return Err(refuse(format!(
            "{} runs as a container, which has no separate image to rebuild on.",
            info.name
        )));
    }
    if let Some(why) = &info.busy {
        return Err(refuse(format!(
            "{} is busy ({why}). Retry when it is idle.",
            info.name
        )));
    }
    if let Some(why) = env.refusal(&info) {
        return Err(refuse(why));
    }
    let undo_name = undo_name(&info.name);
    if env.name_in_use(&undo_name) {
        return Err(refuse(format!(
            "Computer '{undo_name}' already exists. Drop or rename it before rebuilding '{}'.",
            info.name
        )));
    }
    let recipe = env.recipe(computer_id)?;
    let image = env.newest_image()?;
    if !force && info.image.as_deref() == Some(image.as_str()) {
        return Err(refuse(format!(
            "{} is already on the newest image. Ask for a forced rebuild (silo rebuild --force) to rebuild it anyway.",
            info.name
        )));
    }
    let limit = home_limit(info.root_disk_gib);
    if let Some(home) = env.home_bytes(computer_id)? {
        if home > limit {
            return Err(refuse(home_too_large(&info.name, home, info.root_disk_gib)));
        }
    }

    let key = env.stage_key();
    let plan = Plan {
        old_id: info.id.clone(),
        old_name: info.name.clone(),
        new_id: env.fresh_id(),
        new_name: build_name(&key),
        undo_id: env.fresh_id(),
        undo_name: undo_name.clone(),
        image: image.clone(),
        stage: key,
    };
    let mut record = Record {
        plan,
        step: Step::Planned,
        checkpoint: None,
        was_running: info.running,
        at: env.now_millis(),
    };
    journal.save(&record)?;

    if let Err(error) = advance(env, journal, &mut record, recipe.as_ref()) {
        return Err(match shielded(env, || roll_back(env, journal, &record)) {
            Ok(()) => format!(
                "{error} The rebuild was rolled back; {} is as it was.",
                info.name
            ),
            Err(also) => format!(
                "{error} Rolling back also failed ({also}); restarting Silo retries it."
            ),
        });
    }
    if let Err(error) = finish(env, journal, &record) {
        return Err(format!(
            "{} was rebuilt, but cleaning up after it did not finish: {error} Silo finishes it the next time it starts.",
            info.name
        ));
    }
    Ok(ComputerRebuildResult {
        computer_id: info.id,
        name: info.name,
        undo_name,
        image,
    })
}

fn home_too_large(name: &str, home: u64, root_disk_gib: u32) -> String {
    format!(
        "The home folder of '{name}' ({} MB) is too large to carry to a new {root_disk_gib} GiB root disk (up to {} MB fits).",
        home / (1024 * 1024),
        home_limit(root_disk_gib) / (1024 * 1024)
    )
}

fn save(
    env: &dyn RebuildEnv,
    journal: &Journal,
    record: &mut Record,
    step: Step,
) -> Result<(), String> {
    record.step = step;
    record.at = env.now_millis();
    journal.save(record)
}

/// Starts a step: shows it, and stops here if the person cancelled.
fn begin(env: &dyn RebuildEnv, name: &str, what: &str) -> Result<(), String> {
    env.note(what);
    env.relabel(&format!("Rebuilding {name}: {what}"));
    env.check_cancelled()
}

/// Runs the steps the record has not done yet, up to the swap. A request to cancel is honoured
/// between steps until the swap begins; from then on the rebuild runs to the end.
fn advance(
    env: &dyn RebuildEnv,
    journal: &Journal,
    record: &mut Record,
    recipe: Option<&RecipeSpec>,
) -> Result<(), String> {
    let plan = record.plan.clone();
    let name = plan.old_name.as_str();
    if record.step < Step::Stopped {
        begin(env, name, "stopping the computer")?;
        stop_and_wait(env, &plan.old_id)?;
        save(env, journal, record, Step::Stopped)?;
    }
    if record.step < Step::Checkpointed {
        begin(env, name, "saving a checkpoint to go back to")?;
        let checkpoint = env.checkpoint(&plan.old_id, "Before rebuild on the newest image")?;
        record.checkpoint = Some(checkpoint);
        save(env, journal, record, Step::Checkpointed)?;
    }
    if record.step < Step::Created {
        begin(env, name, "building the new computer")?;
        env.create(&plan)?;
        // The first start sets up the working account before the home folder arrives.
        env.start(&plan.new_id)?;
        stop_and_wait(env, &plan.new_id)?;
        save(env, journal, record, Step::Created)?;
    }
    if record.step < Step::HomeCarried {
        begin(env, name, "carrying the home folder")?;
        env.start(&plan.old_id)?;
        env.start(&plan.new_id)?;
        env.carry_home(&plan)?;
        // The old computer never runs again after this: the workspace disk is cloned from it
        // stopped, so every write to the workspace is in the clone.
        stop_and_wait(env, &plan.old_id)?;
        stop_and_wait(env, &plan.new_id)?;
        save(env, journal, record, Step::HomeCarried)?;
    }
    if record.step < Step::WorkspaceMoved {
        begin(env, name, "copying the workspace disk")?;
        // Nothing may run while the disk is cloned, whoever started it meanwhile.
        stop_and_wait(env, &plan.old_id)?;
        stop_and_wait(env, &plan.new_id)?;
        env.move_workspace(&plan)?;
        env.copy_ssh(&plan)?;
        save(env, journal, record, Step::WorkspaceMoved)?;
    }
    if record.step < Step::RecipeReplayed {
        begin(env, name, "installing the recipe on the new image")?;
        if let Some(spec) = recipe.filter(|spec| spec.os == RecipeOs::Linux) {
            env.start(&plan.new_id)?;
            env.replay_recipe(&plan.new_id, spec)?;
            stop_and_wait(env, &plan.new_id)?;
        }
        save(env, journal, record, Step::RecipeReplayed)?;
    }
    if record.step < Step::Swapping {
        begin(env, name, "swapping the computers")?;
        stop_and_wait(env, &plan.old_id)?;
        stop_and_wait(env, &plan.new_id)?;
        save(env, journal, record, Step::Swapping)?;
    }
    if record.step < Step::Swapped {
        shielded(env, || {
            env.swap_apply(&plan)?;
            save(env, journal, record, Step::Swapped)
        })?;
    }
    Ok(())
}

/// What follows the swap: forget what the build left in the stores, bring the computer back
/// if it was running, and close the journal once the stores are settled. Bringing the computer
/// back does not depend on the cleanup, and neither undoes the rebuild. When the cleanup fails
/// the journal stays, so the next start (or the next rebuild) runs it again.
fn finish(env: &dyn RebuildEnv, journal: &Journal, record: &Record) -> Result<(), String> {
    shielded(env, || {
        let settled = env.settle_swapped(&record.plan);
        if record.was_running {
            env.note(&format!("Starting {}", record.plan.old_name));
            if let Err(error) = env.start(&record.plan.old_id) {
                env.note(&format!(
                    "{} was not started again: {error}",
                    record.plan.old_name
                ));
            }
        }
        if settled.is_ok() {
            journal.remove(&record.plan.old_id);
            env.stuck_notice(&record.plan, None);
        }
        settled
    })
}

/// Puts everything back: the identities if the swap had begun, then the build sandbox and its
/// staged files go, and the computer runs again only if it did when the rebuild began (it is
/// left stopped otherwise).
fn roll_back(env: &dyn RebuildEnv, journal: &Journal, record: &Record) -> Result<(), String> {
    undo_rebuild(env, journal, record, false)
}

/// `roll_back`, or with `abandon` also the way out of identities in a state the swap does not
/// know: the old sandbox is found by the Silo identities it carries.
fn undo_rebuild(
    env: &dyn RebuildEnv,
    journal: &Journal,
    record: &Record,
    abandon: bool,
) -> Result<(), String> {
    let plan = &record.plan;
    if record.step == Step::Created {
        // The engine may have stopped while the home folder was being carried.
        env.clean_old_computer(plan);
    }
    for id in [&plan.new_id, &plan.old_id] {
        // A running computer cannot be renamed or removed.
        if env.info(id).map(|info| info.running).unwrap_or(false) {
            stop_and_wait(env, id)?;
        }
    }
    let swapping = record.step >= Step::Swapping;
    if swapping || abandon {
        if let Err(error) = env.swap_revert(plan) {
            if !abandon {
                return Err(error);
            }
            env.restore_original(plan, swapping)?;
        }
    }
    env.remove_build(plan)?;
    if record.was_running {
        if let Err(error) = env.start(&plan.old_id) {
            env.note(&format!("{} was not started again: {error}", plan.old_name));
        }
    }
    journal.remove(&plan.old_id);
    env.stuck_notice(plan, None);
    Ok(())
}

/// Finishes or rolls back a rebuild the engine stopped in the middle of. Once the swap has
/// begun it is finished (the swap repeats harmlessly from wherever it stopped, and both
/// sandboxes are fully prepared by then); before that the build is thrown away.
fn recover_record(env: &dyn RebuildEnv, journal: &Journal, record: &Record) -> Result<(), String> {
    if record.step >= Step::Swapping {
        if record.step < Step::Swapped {
            env.swap_apply(&record.plan)?;
        }
        finish(env, journal, record)
    } else {
        roll_back(env, journal, record)
    }
}

/// Throws an interrupted rebuild away and puts the old computer back, whatever state the
/// identities were left in. The owner asks for it when the next start cannot settle the
/// rebuild by itself.
pub fn abandon(
    env: &dyn RebuildEnv,
    journal: &Journal,
    computer_id: &str,
    wait: Duration,
) -> Result<ComputerRebuildResult, String> {
    let _lane = lane::acquire(computer_id, wait)?;
    let _gate = env.hold(computer_id)?;
    let record = journal.get(computer_id).ok_or_else(|| {
        "No rebuild of this computer is waiting. Nothing was changed.".to_owned()
    })?;
    env.relabel(&format!("Abandoning the rebuild of {}", record.plan.old_name));
    shielded(env, || undo_rebuild(env, journal, &record, true))
        .map_err(|error| format!("The rebuild could not be abandoned: {error}"))?;
    Ok(ComputerRebuildResult {
        computer_id: record.plan.old_id.clone(),
        name: record.plan.old_name.clone(),
        undo_name: String::new(),
        image: String::new(),
    })
}

/// The computers with a rebuild to settle.
pub fn interrupted_ids(journal: &Journal) -> Vec<String> {
    journal
        .all()
        .into_iter()
        .map(|record| record.plan.old_id)
        .collect()
}

/// Settles the interrupted rebuild of `computer_id` with its lane and operation gate held from
/// the start. `held` runs once they are, so the caller can let startup go on knowing nothing
/// can start, stop or change the computer meanwhile. It is tried up to `attempts` times, `pause`
/// apart (the runtime may not be ready yet at engine start), and the lane and the gate are
/// taken again on each attempt that does not have them. When the last attempt fails the owner
/// is told, and can abandon the rebuild.
pub fn recover_one(
    env: &dyn RebuildEnv,
    journal: &Journal,
    computer_id: &str,
    held: &dyn Fn(),
    attempts: u32,
    pause: Duration,
) -> Result<(), String> {
    let mut holding: Option<(lane::LaneGuard, Box<dyn std::any::Any>)> = None;
    let mut last = Err("The rebuild was not attempted.".to_owned());
    for attempt in 0..attempts.max(1) {
        if journal.get(computer_id).is_none() {
            return Ok(());
        }
        if holding.is_none() {
            last = lane::acquire(computer_id, Duration::from_secs(30)).and_then(|lane| {
                let gate = env.hold(computer_id)?;
                holding = Some((lane, gate));
                held();
                Ok(())
            });
        }
        if holding.is_some() {
            let Some(record) = journal.get(computer_id) else {
                return Ok(());
            };
            last = shielded(env, || recover_record(env, journal, &record));
            if last.is_ok() {
                return Ok(());
            }
        }
        if attempt + 1 < attempts {
            std::thread::sleep(pause);
        }
    }
    if let (Err(error), Some(record)) = (&last, journal.get(computer_id)) {
        env.stuck_notice(&record.plan, Some(&stuck_message(&record.plan.old_name, error)));
    }
    last
}
