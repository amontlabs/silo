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
    /// The size of `/home/silo` in bytes. A stopped computer may be started for the
    /// measurement and is stopped again.
    fn home_bytes(&self, id: &str) -> Result<u64, String>;
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
        recover_record(env, journal, &record)?;
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
    let home = env.home_bytes(computer_id)?;
    if home > limit {
        return Err(refuse(format!(
            "The home folder of '{}' ({} MB) is too large to carry to a new {} GiB root disk (up to {} MB fits).",
            info.name,
            home / (1024 * 1024),
            info.root_disk_gib,
            limit / (1024 * 1024)
        )));
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
        return Err(match roll_back(env, journal, &record) {
            Ok(()) => format!(
                "{error} The rebuild was rolled back; {} is as it was.",
                info.name
            ),
            Err(also) => format!(
                "{error} Rolling back also failed ({also}); restarting Silo retries it."
            ),
        });
    }
    Ok(ComputerRebuildResult {
        computer_id: info.id,
        name: info.name,
        undo_name,
        image,
    })
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

/// Runs the steps the record has not done yet.
fn advance(
    env: &dyn RebuildEnv,
    journal: &Journal,
    record: &mut Record,
    recipe: Option<&RecipeSpec>,
) -> Result<(), String> {
    let plan = record.plan.clone();
    if record.step < Step::Stopped {
        env.note(&format!("Stopping {}", plan.old_name));
        stop_and_wait(env, &plan.old_id)?;
        save(env, journal, record, Step::Stopped)?;
    }
    if record.step < Step::Checkpointed {
        env.note("Saving a checkpoint to go back to");
        let checkpoint = env.checkpoint(&plan.old_id, "Before rebuild on the newest image")?;
        record.checkpoint = Some(checkpoint);
        save(env, journal, record, Step::Checkpointed)?;
    }
    if record.step < Step::Created {
        env.note("Building the new computer");
        env.create(&plan)?;
        // The first start sets up the working account before the home folder arrives.
        env.start(&plan.new_id)?;
        stop_and_wait(env, &plan.new_id)?;
        save(env, journal, record, Step::Created)?;
    }
    if record.step < Step::HomeCarried {
        env.note("Carrying the home folder");
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
        env.note("Copying the workspace disk");
        env.move_workspace(&plan)?;
        env.copy_ssh(&plan)?;
        save(env, journal, record, Step::WorkspaceMoved)?;
    }
    if record.step < Step::RecipeReplayed {
        if let Some(spec) = recipe.filter(|spec| spec.os == RecipeOs::Linux) {
            env.note("Installing the recipe on the new image");
            env.start(&plan.new_id)?;
            env.replay_recipe(&plan.new_id, spec)?;
            stop_and_wait(env, &plan.new_id)?;
        }
        save(env, journal, record, Step::RecipeReplayed)?;
    }
    if record.step < Step::Swapping {
        stop_and_wait(env, &plan.old_id)?;
        stop_and_wait(env, &plan.new_id)?;
        save(env, journal, record, Step::Swapping)?;
    }
    if record.step < Step::Swapped {
        env.note("Swapping the computers");
        env.swap_apply(&plan)?;
        save(env, journal, record, Step::Swapped)?;
    }
    finish(env, journal, record);
    Ok(())
}

/// What follows the swap: forget what the build left in the stores, bring the computer back
/// if it was running, and close the journal. None of it undoes the rebuild.
fn finish(env: &dyn RebuildEnv, journal: &Journal, record: &Record) {
    if let Err(error) = env.settle_swapped(&record.plan) {
        env.note(&format!(
            "Some leftovers of the build were not cleaned up: {error}"
        ));
    }
    if record.was_running {
        env.note(&format!("Starting {}", record.plan.old_name));
        if let Err(error) = env.start(&record.plan.old_id) {
            env.note(&format!(
                "{} was not started again: {error}",
                record.plan.old_name
            ));
        }
    }
    journal.remove(&record.plan.old_id);
}

/// Puts everything back: the identities if the swap had begun, then the build sandbox and its
/// staged files go, and the computer runs again only if it did when the rebuild began (it is
/// left stopped otherwise).
fn roll_back(env: &dyn RebuildEnv, journal: &Journal, record: &Record) -> Result<(), String> {
    let plan = &record.plan;
    for id in [&plan.new_id, &plan.old_id] {
        // A running computer cannot be renamed or removed.
        if env.info(id).map(|info| info.running).unwrap_or(false) {
            stop_and_wait(env, id)?;
        }
    }
    if record.step >= Step::Swapping {
        env.swap_revert(plan)?;
    }
    env.remove_build(plan)?;
    if record.was_running {
        if let Err(error) = env.start(&plan.old_id) {
            env.note(&format!("{} was not started again: {error}", plan.old_name));
        }
    }
    journal.remove(&plan.old_id);
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
        finish(env, journal, record);
        Ok(())
    } else {
        roll_back(env, journal, record)
    }
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
/// can start, stop or change the computer meanwhile. It is tried up to `attempts` times, `pause` apart (the
/// runtime may not be ready yet at engine start).
pub fn recover_one(
    env: &dyn RebuildEnv,
    journal: &Journal,
    computer_id: &str,
    held: &dyn Fn(),
    attempts: u32,
    pause: Duration,
) -> Result<(), String> {
    let _lane = lane::acquire(computer_id, Duration::from_secs(30))?;
    let _gate = env.hold(computer_id)?;
    held();
    let mut last = Ok(());
    for attempt in 0..attempts.max(1) {
        let Some(record) = journal.get(computer_id) else {
            return Ok(());
        };
        last = recover_record(env, journal, &record);
        if last.is_ok() {
            return Ok(());
        }
        if attempt + 1 < attempts {
            std::thread::sleep(pause);
        }
    }
    last
}
