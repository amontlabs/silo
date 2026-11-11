//! Recipes: a clean machine spec, built once into a hidden computer and a checkpoint that
//! forks start from.
//!
//! The pieces: [`normalize`] and [`hash`] (what makes two specs the same build), [`Cache`]
//! (one JSON file per built recipe), [`BuildEnv`] (what a build needs from the world; the
//! engine supplies the real one, tests supply a fake) and [`build`] (the flow itself).

mod additions;
mod build;
mod cache;
mod library;
mod quiet;
pub mod records;
mod scripts;

use sha2::{Digest, Sha256};
use silo_api::methods::recipes::{RecipeOs, RecipePackages, RecipeService, RecipeSpec};

pub use build::{build, remove, BuildEnv, BuildOutcome, Created, ExecOut, RunAs, BUILDER_PREFIX};
pub(crate) use scripts::Step as BuildStep;
#[cfg(test)]
use build::{builder_name, is_builder_of};
pub use additions::{append, manager_key, record_merge, unrecord_merge, Additions, Recorded};
pub use cache::Cache;
pub use quiet::{quiet_names, quiet_warnings};
pub use library::{
    built_in, built_in_names, hooks_of, validate_definition, Library, DEFAULT_RECIPE,
};

/// Folded into every hash, so a change to how a build runs retires every cached build.
const BUILD_VERSION: &str = "silo-recipe-build-3";

/// A recipe's size when it names none.
pub fn default_resources(os: RecipeOs) -> (u32, u32, u32) {
    match os {
        RecipeOs::Linux => (2, 4, 10),
        RecipeOs::Macos => (4, 8, 64),
    }
}

fn clean(list: &[String]) -> Vec<String> {
    let mut items: Vec<String> = list
        .iter()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect();
    items.sort();
    items.dedup();
    items
}

/// The spec with everything that does not change a build removed: whitespace, package order
/// and repeats, service order, repeated ports, and resources spelled out at their default.
/// Setup commands keep their order, because it is the order they run in.
pub fn normalize(spec: &RecipeSpec) -> RecipeSpec {
    let (cpus, memory, disk) = default_resources(spec.os);
    let size = |value: Option<u32>, default: u32| value.filter(|value| *value != default);
    let mut services: Vec<RecipeService> = spec
        .services
        .iter()
        .map(|service| RecipeService {
            name: service.name.trim().to_owned(),
            run: service.run.trim().to_owned(),
        })
        .collect();
    services.sort_by(|left, right| left.name.cmp(&right.name));
    let mut ports = spec.ports.clone();
    ports.sort_unstable();
    ports.dedup();
    RecipeSpec {
        os: spec.os,
        cpus: size(spec.cpus, cpus),
        memory_gib: size(spec.memory_gib, memory),
        disk_gib: size(spec.disk_gib, disk),
        packages: RecipePackages {
            apt: clean(&spec.packages.apt),
            brew: clean(&spec.packages.brew),
            npm: clean(&spec.packages.npm),
            pip: clean(&spec.packages.pip),
            pipx: clean(&spec.packages.pipx),
            cargo: clean(&spec.packages.cargo),
        },
        setup: spec
            .setup
            .iter()
            .map(|command| command.trim().to_owned())
            .filter(|command| !command.is_empty())
            .collect(),
        services,
        ports,
        // Only an off desktop changes what is built.
        desktop: spec.desktop.filter(|on| !on),
    }
}

/// The content hash of the normalized spec on `image`, hex. Two specs that build the same
/// computer on the same image have the same hash; a new image gives a new hash, so a new
/// cache entry and a new baseline key.
pub fn hash(spec: &RecipeSpec, image: &str) -> String {
    let normalized = serde_json::to_vec(&normalize(spec)).unwrap_or_default();
    let mut digest = Sha256::new();
    digest.update(BUILD_VERSION.as_bytes());
    digest.update([0]);
    digest.update(image.as_bytes());
    digest.update([0]);
    digest.update(&normalized);
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The guest steps that put `spec` on a computer: the ones a build runs, in the same order.
pub(crate) fn steps_for(spec: &RecipeSpec) -> Vec<BuildStep> {
    scripts::all_steps(&normalize(spec))
}

/// A recipe name: lowercase letters, digits, `-` and `_`, as a file name.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub(crate) fn plain_word(word: &str) -> bool {
    !word.is_empty()
        && word.len() <= 200
        && !word.starts_with('-')
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@._+:/=~-^".contains(c))
}

/// Why a spec cannot be built, or nothing. Package names are words the package managers
/// understand; they are never run as shell text.
pub fn validate(spec: &RecipeSpec) -> Result<(), String> {
    let (linux, macos) = (spec.os == RecipeOs::Linux, spec.os == RecipeOs::Macos);
    let managers: [(&str, &Vec<String>, bool); 6] = [
        ("apt", &spec.packages.apt, linux),
        ("brew", &spec.packages.brew, macos),
        ("npm", &spec.packages.npm, true),
        ("pip", &spec.packages.pip, true),
        ("pipx", &spec.packages.pipx, true),
        ("cargo", &spec.packages.cargo, true),
    ];
    for (manager, packages, allowed) in managers {
        if !packages.is_empty() && !allowed {
            let os = if linux { "a Linux" } else { "a macOS" };
            return Err(format!("packages.{manager} cannot be used in {os} recipe."));
        }
        if let Some(bad) = packages.iter().find(|package| !plain_word(package.trim())) {
            return Err(format!(
                "packages.{manager}: `{bad}` is not a package name."
            ));
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for service in &spec.services {
        let name = service.name.trim();
        if !valid_name(name) {
            return Err(format!(
                "services: `{name}` is not a service name (lowercase letters, digits, - and _)."
            ));
        }
        if !seen.insert(name.to_owned()) {
            return Err(format!("services: `{name}` is listed twice."));
        }
        if service.run.trim().is_empty() || service.run.contains(['\n', '\0']) {
            return Err(format!(
                "services.{name}: `run` must be one command on one line."
            ));
        }
    }
    if spec.setup.iter().any(|command| command.contains('\0')) {
        return Err("setup: a command may not contain a NUL byte.".into());
    }
    if spec.ports.contains(&0) {
        return Err("ports: 0 is not a port.".into());
    }
    if macos && spec.desktop == Some(false) {
        return Err("desktop: false applies to a Linux recipe; a macOS computer is its own display and always has one.".into());
    }
    if linux && spec.desktop == Some(false) && !spec.services.is_empty() {
        return Err("desktop: false cannot be combined with services: a Linux service starts with the desktop session.".into());
    }
    for (what, value) in [
        ("cpus", spec.cpus),
        ("memory", spec.memory_gib),
        ("disk", spec.disk_gib),
    ] {
        if value == Some(0) {
            return Err(format!("{what}: must be at least 1."));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
