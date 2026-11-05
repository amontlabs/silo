//! The build flow: create a hidden computer, run the recipe's steps, check the result, save a
//! checkpoint, and record the build in the cache. A build that fails anywhere leaves no cache
//! entry and no builder computer behind.

use silo_api::methods::recipes::{RecipeEntry, RecipeOs, RecipeSpec};
use time::format_description::well_known::Rfc3339;

use super::{cache::Cache, default_resources, hash, normalize, scripts, validate, valid_name};

/// Builder computers are named `silo-build-<name>`. `silo list` leaves them out.
pub const BUILDER_PREFIX: &str = "silo-build-";

/// The most macOS virtual machines a Mac runs at once.
const MACOS_LIMIT: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunAs {
    Root,
    Account,
}

#[derive(Clone, Debug, Default)]
pub struct ExecOut {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl ExecOut {
    /// The last 400 characters of what the command said on stderr, else on stdout.
    fn tail(&self) -> String {
        let text = if self.stderr.is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        let text = String::from_utf8_lossy(text);
        let text = text.trim();
        let skip = text.chars().count().saturating_sub(400);
        text.chars().skip(skip).collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    pub id: String,
    pub name: String,
}

/// What a build needs from the world. The engine supplies [`crate::recipes`]' real one in
/// `silod`; tests supply a fake.
pub trait BuildEnv {
    /// The guest image the computer will be made from.
    fn image_version(&self) -> String;
    /// How many macOS virtual machines are running now.
    fn macos_running(&self) -> Result<usize, String>;
    /// The id of the computer with this name, if there is one.
    fn find(&self, name: &str) -> Result<Option<String>, String>;
    /// Creates a computer and starts it.
    fn create(
        &self,
        name: &str,
        os: RecipeOs,
        cpus: u32,
        memory_gib: u32,
        disk_gib: u32,
    ) -> Result<Created, String>;
    fn run(
        &self,
        computer: &str,
        who: RunAs,
        script: &str,
        args: &[(String, String)],
    ) -> Result<ExecOut, String>;
    /// Saves a checkpoint and returns its id.
    fn checkpoint(&self, computer: &str, name: &str) -> Result<String, String>;
    fn checkpoint_exists(&self, computer: &str, checkpoint: &str) -> bool;
    fn stop(&self, computer: &str) -> Result<(), String>;
    /// Stops the computer if it runs, then deletes it.
    fn delete(&self, computer: &str) -> Result<(), String>;
    fn note(&self, line: &str);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildOutcome {
    pub entry: RecipeEntry,
    /// False when the cache already held this build.
    pub built: bool,
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

/// Builds `spec` as the recipe `name`, or returns the cached build when it is the same spec
/// on the same image and its checkpoint is still there.
pub fn build(
    env: &dyn BuildEnv,
    cache: &Cache,
    name: &str,
    spec: &RecipeSpec,
    force: bool,
) -> Result<BuildOutcome, String> {
    if !valid_name(name) {
        return Err(format!(
            "`{name}` is not a recipe name (lowercase letters, digits, - and _)."
        ));
    }
    validate(spec)?;
    let spec = normalize(spec);
    let spec_hash = hash(&spec);
    let image = env.image_version();
    let previous = cache.get(name)?;
    if let Some(entry) = previous.as_ref().filter(|entry| {
        !force
            && entry.hash == spec_hash
            && entry.image_version == image
            && env.checkpoint_exists(&entry.computer_id, &entry.checkpoint_id)
    }) {
        return Ok(BuildOutcome {
            entry: entry.clone(),
            built: false,
        });
    }
    if spec.os == RecipeOs::Macos && env.macos_running()? >= MACOS_LIMIT {
        return Err("Two macOS virtual machines are already running, and building a recipe needs a third. Stop one and try again.".into());
    }
    let builder_name = format!("{BUILDER_PREFIX}{name}");
    // A builder an interrupted build left behind is dropped first.
    if let Some(stale) = env.find(&builder_name)? {
        env.delete(&stale)
            .map_err(|error| format!("The old builder {builder_name} could not be dropped: {error}"))?;
    }
    let (cpus, memory, disk) = default_resources(spec.os);
    env.note(&format!("Creating the builder computer for {name}"));
    let created = env.create(
        &builder_name,
        spec.os,
        spec.cpus.unwrap_or(cpus),
        spec.memory_gib.unwrap_or(memory),
        spec.disk_gib.unwrap_or(disk),
    );
    let created = match created {
        Ok(created) => created,
        Err(error) => {
            // The create may have left a half-made computer.
            if let Ok(Some(leftover)) = env.find(&builder_name) {
                let _ = env.delete(&leftover);
            }
            return Err(format!("The builder could not be created: {error}"));
        }
    };
    let finished = (|| -> Result<String, String> {
        for step in scripts::all_steps(&spec) {
            env.note(&step.label);
            let out = env
                .run(&created.id, step.who, &step.script, &step.args)
                .map_err(|error| format!("{} failed: {error}", step.label))?;
            if out.code != 0 {
                return Err(format!(
                    "{} failed (exit {}): {}",
                    step.label,
                    out.code,
                    out.tail()
                ));
            }
        }
        env.note("Saving the checkpoint");
        let checkpoint = env.checkpoint(&created.id, &format!("recipe {name} {}", &spec_hash[..12]))?;
        env.stop(&created.id)?;
        Ok(checkpoint)
    })();
    let checkpoint_id = match finished {
        Ok(id) => id,
        Err(error) => {
            if let Err(cleanup) = env.delete(&created.id) {
                return Err(format!(
                    "{error} The builder {builder_name} could not be dropped either: {cleanup}"
                ));
            }
            return Err(error);
        }
    };
    let entry = RecipeEntry {
        name: name.to_owned(),
        hash: spec_hash,
        image_version: image,
        os: spec.os,
        computer_id: created.id.clone(),
        computer_name: created.name,
        checkpoint_id,
        built_at: now(),
        ports: spec.ports.clone(),
    };
    if let Err(error) = cache.put(&entry) {
        let _ = env.delete(&created.id);
        return Err(error);
    }
    // The rebuilt recipe replaces the old builder, which forks may no longer start from.
    if let Some(old) = previous.filter(|old| old.computer_id != entry.computer_id) {
        let _ = env.delete(&old.computer_id);
    }
    Ok(BuildOutcome { entry, built: true })
}

/// Removes a recipe from the cache and drops its builder computer.
pub fn remove(env: &dyn BuildEnv, cache: &Cache, name: &str) -> Result<(), String> {
    let entry = cache
        .get(name)?
        .ok_or_else(|| format!("There is no built recipe named {name}."))?;
    env.delete(&entry.computer_id)
        .map_err(|error| format!("The builder of {name} could not be dropped: {error}"))?;
    cache.remove(name)?;
    Ok(())
}
