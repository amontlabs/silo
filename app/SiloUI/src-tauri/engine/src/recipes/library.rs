//! The stored recipes: one JSON file per name under `<app data>/recipes/saved/`, and the ones
//! Silo ships. A saved recipe with the name of a shipped one replaces it until it is removed.

use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

use silo_api::methods::{
    keep::KeepHooks,
    recipes::{RecipeDefinition, RecipeOs, RecipePackages, RecipeService, RecipeSpec},
};

use super::{valid_name, validate};

/// The recipe `silo up` builds `main` from when none is named.
pub const DEFAULT_RECIPE: &str = "linux-dev";

/// The most bytes one stored recipe may take.
pub(crate) const MAX_BYTES: usize = 256 * 1024;
/// The most entries one list in a recipe may hold.
pub(crate) const MAX_LIST: usize = 256;
/// The most process names and paths a recipe lists for a fork. A caller adds its own on top,
/// and the engine allows 64 of each in all.
const MAX_FORK_LIST: usize = 24;

fn spec(os: RecipeOs, apt: &[&str], setup: &[&str]) -> RecipeSpec {
    RecipeSpec {
        os,
        cpus: None,
        memory_gib: None,
        disk_gib: None,
        packages: RecipePackages {
            apt: apt.iter().map(|name| (*name).to_owned()).collect(),
            ..RecipePackages::default()
        },
        setup: setup.iter().map(|command| (*command).to_owned()).collect(),
        services: Vec::<RecipeService>::new(),
        ports: Vec::new(),
        desktop: None,
    }
}

fn definition(spec: RecipeSpec) -> RecipeDefinition {
    RecipeDefinition {
        spec,
        quiet: Vec::new(),
        scrub: Vec::new(),
        keep: None,
        test: None,
        files: Vec::new(),
    }
}

/// The names of the recipes Silo ships.
pub fn built_in_names() -> [&'static str; 2] {
    [DEFAULT_RECIPE, "macos-xcode"]
}

/// A recipe Silo ships: minimal on purpose, what a coding agent usually wants and nothing else.
pub fn built_in(name: &str) -> Option<RecipeDefinition> {
    match name {
        "linux-dev" => Some(definition(spec(
            RecipeOs::Linux,
            &["build-essential", "jq", "ripgrep"],
            &[],
        ))),
        "macos-xcode" => Some(definition(spec(
            RecipeOs::Macos,
            &[],
            &["xcode-select -p", "xcodebuild -version"],
        ))),
        _ => None,
    }
}

fn plain_line(text: &str) -> bool {
    !text.trim().is_empty() && !text.contains(['\0', '\n', '\r'])
}

/// Why a recipe cannot be stored, or nothing. The build checks the spec again when it runs.
pub fn validate_definition(name: &str, recipe: &RecipeDefinition) -> Result<(), String> {
    if !valid_name(name) {
        return Err(format!(
            "`{name}` is not a recipe name (lowercase letters, digits, - and _)."
        ));
    }
    validate(&recipe.spec)?;
    let lists: [(&str, usize); 5] = [
        ("setup", recipe.spec.setup.len()),
        ("services", recipe.spec.services.len()),
        ("ports", recipe.spec.ports.len()),
        ("files", recipe.files.len()),
        (
            "keep",
            recipe.keep.as_ref().map_or(0, |keep| {
                keep.restart.len() + keep.migrate.len() + keep.after.len()
            }),
        ),
    ];
    for (what, length) in lists {
        if length > MAX_LIST {
            return Err(format!("{what}: at most {MAX_LIST} entries."));
        }
    }
    for (what, list) in [("quiet", &recipe.quiet), ("scrub", &recipe.scrub)] {
        if list.len() > MAX_FORK_LIST {
            return Err(format!("{what}: at most {MAX_FORK_LIST} entries."));
        }
        if let Some(bad) = list.iter().find(|item| !plain_line(item)) {
            return Err(format!("{what}: `{bad}` is not a usable entry."));
        }
    }
    if let Some(bad) = recipe.scrub.iter().find(|path| !path.starts_with('/')) {
        return Err(format!("scrub: `{bad}` must be an absolute path."));
    }
    if let Some(bad) = recipe
        .files
        .iter()
        .find(|path| !path.starts_with('/') && !path.starts_with("~/") || !plain_line(path))
    {
        return Err(format!("files: `{bad}` must be an absolute path."));
    }
    if let Some(keep) = &recipe.keep {
        for (what, list) in [
            ("keep.restart", &keep.restart),
            ("keep.migrate", &keep.migrate),
            ("keep.after", &keep.after),
        ] {
            if let Some(bad) = list.iter().find(|item| !plain_line(item)) {
                return Err(format!("{what}: `{bad}` is not a usable entry."));
            }
        }
    }
    if recipe.test.as_deref().is_some_and(|test| !plain_line(test)) {
        return Err("test: must be one command on one line.".into());
    }
    let size = serde_json::to_vec(recipe)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX);
    if size > MAX_BYTES {
        return Err("The recipe is too large.".into());
    }
    Ok(())
}

/// The `keep` hooks of a definition, empty when it has none.
pub fn hooks_of(recipe: &RecipeDefinition) -> KeepHooks {
    recipe.keep.clone().unwrap_or(KeepHooks {
        restart: Vec::new(),
        migrate: Vec::new(),
        after: Vec::new(),
    })
}

static LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct Library {
    root: PathBuf,
}

impl Library {
    pub fn new(app_data: &Path) -> Self {
        Self {
            root: app_data.join("recipes").join("saved"),
        }
    }

    fn file(&self, name: &str) -> Result<PathBuf, String> {
        if !valid_name(name) {
            return Err(format!("`{name}` is not a recipe name."));
        }
        Ok(self.root.join(format!("{name}.json")))
    }

    /// The recipe the owner saved under this name.
    pub fn saved(&self, name: &str) -> Result<Option<RecipeDefinition>, String> {
        let path = self.file(name)?;
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|error| format!("The saved recipe {name} is unreadable: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("Cannot read the saved recipe {name}: {error}")),
        }
    }

    /// The recipe by this name: the saved one, else the one Silo ships.
    pub fn get(&self, name: &str) -> Result<Option<(RecipeDefinition, bool)>, String> {
        match self.saved(name)? {
            Some(recipe) => Ok(Some((recipe, false))),
            None => Ok(built_in(name).map(|recipe| (recipe, true))),
        }
    }

    /// Every recipe by name, with whether it is the shipped one. A damaged file is skipped.
    pub fn list(&self) -> Vec<(String, RecipeDefinition, bool)> {
        let mut found: Vec<(String, RecipeDefinition, bool)> = fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_stem()?.to_str()?.to_owned();
                (path.extension()? == "json" && valid_name(&name)).then_some((name, path))
            })
            .filter_map(|(name, path)| {
                let recipe = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
                Some((name, recipe, false))
            })
            .collect();
        for name in built_in_names() {
            if !found.iter().any(|(saved, _, _)| saved == name) {
                if let Some(recipe) = built_in(name) {
                    found.push((name.to_owned(), recipe, true));
                }
            }
        }
        found.sort_by(|left, right| left.0.cmp(&right.0));
        found
    }

    /// Writes the recipe whole: a reader sees the old file or the new one.
    pub fn save(&self, name: &str, recipe: &RecipeDefinition) -> Result<(), String> {
        let _held = lock();
        self.save_locked(name, recipe)
    }

    pub(crate) fn save_locked(&self, name: &str, recipe: &RecipeDefinition) -> Result<(), String> {
        validate_definition(name, recipe)?;
        let path = self.file(name)?;
        let io = |error: std::io::Error| format!("Cannot save the recipe {name}: {error}");
        let directory = &self.root;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .map_err(io)?;
        let temporary = directory.join(format!("{name}-{}.tmp", uuid::Uuid::new_v4().simple()));
        let bytes = serde_json::to_vec_pretty(recipe).map_err(|error| error.to_string())?;
        let written = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, &path)?;
            fs::File::open(directory)?.sync_all()
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        written.map_err(io)
    }

    /// Forgets the saved recipe; the shipped one of the same name comes back. Whether one was
    /// there.
    pub fn remove(&self, name: &str) -> Result<bool, String> {
        let path = self.file(name)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!("Cannot remove the recipe {name}: {error}")),
        }
    }
}
