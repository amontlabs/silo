//! Linux computer rebuild on the newest image.
//!
//! Rebuilds an existing Linux computer on the newest guest image while preserving its
//! workspace disk, its `/home/silo` directory, and its SSH credentials.
//! The operation follows the S2 design:
//! 1. Acquire the computer's operation lane.
//! 2. Check refusal conditions cleanly before making changes (macOS refusal, undo name collision,
//!    home size exceeding new root disk capacity, or missing host reflink when clone is needed).
//! 3. Journal each step to support crash resume and clean rollback.
//! 4. Create an integrity checkpoint of the old computer as the durable undo mechanism.
//! 5. Create `<name>-rebuild-<hex>` on the newest image with the old hardware arguments.
//! 6. Boot the rebuild computer once to initialize its working account, then stop it.
//! 7. Move the workspace disk (preserving layer structure and destination volume ID).
//! 8. Boot both computers, carry `/home/silo` via guest tar and host staging, and copy the host `ssh/` folder.
//! 9. Replay the computer's recipe (if recorded).
//! 10. Stop both computers, swap names (`<name>` -> `<name>-undo`, `<name>-rebuild-<hex>` -> `<name>`),
//!     and relabel `silo.machine-id` on both computers.

pub mod engine_env;
pub mod journal;
#[cfg(test)]
mod tests;

use std::any::Any;

pub use journal::{Journal, RebuildRecord, RebuildState, RebuildStep};
use silo_api::methods::computers::ComputerRebuildResult;
use silo_api::methods::recipes::RecipeSpec;

/// Basic properties of a computer needed for rebuild planning and validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputerInfo {
    pub id: String,
    pub name: String,
    pub is_linux: bool,
    pub running: bool,
    pub cpus: u32,
    pub max_cpus: u32,
    pub memory_gib: u32,
    pub max_memory_gib: u32,
    pub root_disk_gib: u32,
    pub workspace_storage_gib: u32,
    pub desktop: bool,
    pub has_checkpoint_layers: bool,
}

/// Interface for executing rebuild steps.
pub trait RebuildEnv: Send + Sync {
    fn computer_info(&self, computer: &str) -> Result<ComputerInfo, String>;
    fn computer_exists(&self, name: &str) -> bool;
    fn supports_reflink(&self) -> bool;
    fn measure_home_bytes(&self, computer: &str) -> Result<u64, String>;
    fn hold_lane(&self, computer_id: &str) -> Result<Box<dyn Any>, String>;
    fn stop_computer(&self, computer: &str) -> Result<(), String>;
    fn start_computer(&self, computer: &str) -> Result<(), String>;
    fn checkpoint_integrity(&self, computer: &str, name: &str) -> Result<String, String>;
    fn newest_image(&self) -> Result<String, String>;
    fn create_computer(
        &self,
        name: &str,
        id: &str,
        image: &str,
        info: &ComputerInfo,
    ) -> Result<(), String>;
    fn setup_account(&self, computer: &str) -> Result<(), String>;
    fn move_workspace_disk(&self, from: &str, to: &str) -> Result<(), String>;
    fn carry_home(&self, from: &str, to: &str) -> Result<(), String>;
    fn copy_ssh_folder(&self, from: &str, to: &str) -> Result<(), String>;
    fn replay_recipe(&self, computer: &str, spec: &RecipeSpec) -> Result<(), String>;
    fn swap_names_and_relabel(
        &self,
        old_name: &str,
        old_id: &str,
        rebuild_name: &str,
        rebuild_id: &str,
        undo_name: &str,
        undo_id: &str,
    ) -> Result<(), String>;
    fn delete_computer(&self, computer: &str) -> Result<(), String>;
    fn random_hex(&self) -> String {
        let id = uuid::Uuid::new_v4().simple().to_string();
        id[..8].to_owned()
    }
    fn now_millis(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default()
    }
    fn note(&self, _msg: &str) {}
}

/// Rebuilds a Linux computer on the newest image.
pub fn rebuild(
    env: &dyn RebuildEnv,
    journal: &Journal,
    computer: &str,
    recipe: Option<&silo_api::methods::recipes::RecipeRecord>,
) -> Result<ComputerRebuildResult, String> {
    let info = env.computer_info(computer)?;

    // 1. Refusal: only Linux computers can be rebuilt on the guest image.
    if !info.is_linux {
        return Err(
            "Rebuild on the newest image is currently only supported for Linux computers. Nothing was changed."
                .into(),
        );
    }

    let undo_name = format!("{}-undo", info.name);
    // 2. Refusal: undo name must not collide with an existing computer.
    if env.computer_exists(&undo_name) {
        return Err(format!(
            "Computer '{undo_name}' already exists. Drop or rename it before rebuilding '{}'. Nothing was changed.",
            info.name
        ));
    }

    // 3. Refusal: layered disk requires copy-on-write (reflink) support on host.
    if info.has_checkpoint_layers && !env.supports_reflink() {
        return Err(
            "The host filesystem does not support copy-on-write (reflink) cloning, which is required to rebuild a computer with checkpoint layers. Nothing was changed."
                .into(),
        );
    }

    // 4. Refusal: home size must fit on the root disk alongside its transfer tar.
    let home_bytes = env.measure_home_bytes(&info.name)?;
    let max_home_bytes = (info.root_disk_gib as u64)
        .saturating_mul(1024 * 1024 * 1024)
        .saturating_mul(45)
        / 100;
    if home_bytes > max_home_bytes {
        let home_mb = home_bytes / (1024 * 1024);
        return Err(format!(
            "The home folder of '{}' ({} MB) is too large for the new root disk (maximum 1.8 GiB for a 4 GiB root disk). Nothing was changed.",
            info.name, home_mb
        ));
    }

    // Acquire the computer's operation lane.
    let _lane = env.hold_lane(&info.id)?;

    // Settle any previous interrupted rebuild for this computer first.
    if let Some(existing) = journal.get(&info.id) {
        if existing.state == RebuildState::Rebuilding {
            resume_or_rollback(env, journal, &existing)?;
        }
    }

    let hex = env.random_hex();
    let rebuild_name = format!("{}-rebuild-{}", info.name, hex);
    let rebuild_id = format!("{}-rebuild-{}", info.id, hex);
    let undo_id = format!("{}-undo", info.id);
    let image = env.newest_image()?;

    let mut record = RebuildRecord {
        computer_id: info.id.clone(),
        computer_name: info.name.clone(),
        rebuild_id: rebuild_id.clone(),
        rebuild_name: rebuild_name.clone(),
        undo_id: undo_id.clone(),
        undo_name: undo_name.clone(),
        image: image.clone(),
        state: RebuildState::Rebuilding,
        step: RebuildStep::Init,
        checkpoint_id: None,
        was_running: info.running,
        created_at: env.now_millis(),
    };
    journal.save(&record)?;

    // Execution with rollback protection before swap.
    let execute = || -> Result<(), String> {
        // Step 1: Stop old computer if running.
        if info.running {
            env.note(&format!("Stopping {}", info.name));
            env.stop_computer(&info.name)?;
        }
        record.step = RebuildStep::Stopped;
        journal.save(&record)?;

        // Step 2: Checkpoint with integrity as undo.
        env.note("Creating integrity checkpoint");
        let checkpoint_id =
            env.checkpoint_integrity(&info.name, &format!("before-rebuild-{}", hex))?;
        record.checkpoint_id = Some(checkpoint_id);
        record.step = RebuildStep::CheckpointCreated;
        journal.save(&record)?;

        // Step 3: Create rebuild computer on newest image with old arguments.
        env.note(&format!("Creating rebuild sandbox {}", record.rebuild_name));
        env.create_computer(&record.rebuild_name, &record.rebuild_id, &image, &info)?;
        env.start_computer(&record.rebuild_name)?;
        env.setup_account(&record.rebuild_name)?;
        env.stop_computer(&record.rebuild_name)?;
        record.step = RebuildStep::RebuildCreated;
        journal.save(&record)?;

        // Step 4: Move workspace disk.
        env.note("Moving workspace storage");
        env.move_workspace_disk(&info.name, &record.rebuild_name)?;
        record.step = RebuildStep::WorkspaceMoved;
        journal.save(&record)?;

        // Step 5: Carry /home/silo and host ssh folder.
        env.note("Carrying home folder and SSH credentials");
        env.start_computer(&info.name)?;
        env.start_computer(&record.rebuild_name)?;
        env.carry_home(&info.name, &record.rebuild_name)?;
        env.copy_ssh_folder(&info.name, &record.rebuild_name)?;
        record.step = RebuildStep::HomeAndSshCarried;
        journal.save(&record)?;

        // Step 6: Replay recipe.
        if let Some(record_entry) = recipe {
            env.note("Replaying recipe");
            env.replay_recipe(&record.rebuild_name, &record_entry.recipe.spec)?;
        }
        record.step = RebuildStep::RecipeReplayed;
        journal.save(&record)?;

        // Step 7: Swap names and relabel machine-id.
        env.note("Swapping computer identities");
        env.stop_computer(&info.name)?;
        env.stop_computer(&record.rebuild_name)?;
        env.swap_names_and_relabel(
            &info.name,
            &info.id,
            &record.rebuild_name,
            &record.rebuild_id,
            &record.undo_name,
            &record.undo_id,
        )?;
        record.step = RebuildStep::NamesSwapped;
        journal.save(&record)?;

        // Step 8: Restart if it was running.
        if record.was_running {
            env.note(&format!("Starting rebuilt computer {}", info.name));
            env.start_computer(&info.name)?;
        }

        Ok(())
    };

    match execute() {
        Ok(()) => {
            record.state = RebuildState::Finished;
            record.step = RebuildStep::Complete;
            journal.remove(&info.id);
            Ok(ComputerRebuildResult {
                computer_id: info.id,
                name: info.name,
                undo_name: record.undo_name,
                image,
            })
        }
        Err(err) => {
            // Roll back if swap has not occurred.
            if record.step != RebuildStep::NamesSwapped && record.step != RebuildStep::Complete {
                let _ = env.stop_computer(&record.rebuild_name);
                let _ = env.delete_computer(&record.rebuild_name);
                if record.was_running && !env.computer_info(&info.name).map_or(false, |c| c.running)
                {
                    let _ = env.start_computer(&info.name);
                }
                record.state = RebuildState::Failed;
                let _ = journal.save(&record);
            }
            Err(err)
        }
    }
}

/// Resumes or rolls back an interrupted rebuild found in the journal.
pub fn resume_or_rollback(
    env: &dyn RebuildEnv,
    journal: &Journal,
    record: &RebuildRecord,
) -> Result<(), String> {
    match record.step {
        RebuildStep::NamesSwapped | RebuildStep::Complete => {
            // Already swapped: ensure names and labels are consistent.
            if record.was_running
                && env
                    .computer_info(&record.computer_name)
                    .map_or(false, |c| !c.running)
            {
                let _ = env.start_computer(&record.computer_name);
            }
            journal.remove(&record.computer_id);
            Ok(())
        }
        _ => {
            // Not swapped yet: roll back safely.
            if env.computer_exists(&record.rebuild_name) {
                let _ = env.stop_computer(&record.rebuild_name);
                let _ = env.delete_computer(&record.rebuild_name);
            }
            if record.was_running
                && env
                    .computer_info(&record.computer_name)
                    .map_or(false, |c| !c.running)
            {
                let _ = env.start_computer(&record.computer_name);
            }
            journal.remove(&record.computer_id);
            Ok(())
        }
    }
}

/// Scans the journal and recovers all interrupted rebuild operations.
pub fn recover_all(env: &dyn RebuildEnv, journal: &Journal) -> Result<usize, String> {
    let records = journal.all();
    let mut recovered = 0;
    for record in records {
        if record.state == RebuildState::Rebuilding {
            resume_or_rollback(env, journal, &record)?;
            recovered += 1;
        }
    }
    Ok(recovered)
}
