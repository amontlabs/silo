//! The two project files under `.silo/`: the recipe (`recipe.yaml`, a clean machine spec)
//! and the settings (`silo.yaml`, rules for agents). Unknown keys are errors, and parse
//! errors carry the file path and line.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use silo_api::methods::recipes::{RecipePackages, RecipeService, RecipeSpec};

use crate::error::CliError;

const RECIPE_FILE: &str = "recipe.yaml";
const SETTINGS_FILE: &str = "silo.yaml";

/// How old `main`'s warm checkpoint may be for a fork to start from it. The engine sees what
/// it does to a computer, not what happens over a plain `ssh` session, so the age is capped.
pub const DEFAULT_WARM_MAX_AGE_SECS: u64 = 60;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// The kind of computer the recipe builds; `linux` when left out.
    #[serde(default)]
    pub os: RecipeOs,
    #[serde(default)]
    pub cpus: Option<u32>,
    /// Memory in GiB.
    #[serde(default)]
    pub memory: Option<u32>,
    /// Disk in GiB.
    #[serde(default)]
    pub disk: Option<u32>,
    #[serde(default)]
    pub packages: Packages,
    /// Commands run once while the recipe is built: as root on Linux, as the account on macOS.
    #[serde(default)]
    pub setup: Vec<String>,
    /// Commands started at boot, by service name.
    #[serde(default)]
    pub services: BTreeMap<String, String>,
    /// Ports a fork publishes.
    #[serde(default)]
    pub ports: Vec<u16>,
    /// Process names stopped in a fork.
    #[serde(default)]
    pub quiet: Vec<String>,
    /// Paths removed from a fork.
    #[serde(default)]
    pub scrub: Vec<String>,
    #[serde(default)]
    pub keep: Keep,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum RecipeOs {
    #[default]
    #[serde(rename = "linux")]
    Linux,
    #[serde(rename = "macos")]
    Macos,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packages {
    #[serde(default)]
    pub apt: Vec<String>,
    #[serde(default)]
    pub brew: Vec<String>,
    #[serde(default)]
    pub npm: Vec<String>,
    #[serde(default)]
    pub pip: Vec<String>,
    #[serde(default)]
    pub pipx: Vec<String>,
    #[serde(default)]
    pub cargo: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keep {
    /// Services restarted after a keep.
    #[serde(default)]
    pub restart: Vec<String>,
    /// Commands run before the changes are applied (database migrations).
    #[serde(default)]
    pub migrate: Vec<String>,
    /// Commands run after the changes are applied.
    #[serde(default)]
    pub after: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum Fold {
    #[default]
    #[serde(rename = "ask")]
    Ask,
    #[serde(rename = "auto-code")]
    AutoCode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoKeepWhen {
    #[serde(default)]
    pub only: Vec<String>,
    #[serde(default)]
    pub tests: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoKeepRule {
    pub when: AutoKeepWhen,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentOverride {
    #[serde(default)]
    pub recipes: Option<Vec<String>>,
    #[serde(default)]
    pub fork_secrets: Option<Vec<String>>,
    #[serde(default)]
    pub fold: Option<Fold>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default)]
    pub recipes: Vec<String>,
    #[serde(default)]
    pub fork_secrets: Vec<String>,
    #[serde(default)]
    pub fold: Fold,
    /// Cap on running forks' footprint, like `8G`; read with
    /// [`Settings::fork_ram_budget_bytes`].
    #[serde(default)]
    pub fork_ram_budget: Option<String>,
    /// How old `main`'s warm checkpoint may be for a fork to start from it, like `90s`.
    #[serde(default)]
    pub warm_max_age: Option<String>,
    /// Save a checkpoint of a computer after each step (each command run through Silo).
    #[serde(default)]
    pub step_checkpoints: bool,
    /// How many step checkpoints a computer keeps (the newest ones; 0 keeps every one).
    #[serde(default)]
    pub step_checkpoints_keep: Option<u32>,
    #[serde(default)]
    pub auto_keep: Vec<AutoKeepRule>,
    #[serde(default)]
    pub agents: BTreeMap<String, AgentOverride>,
}

/// How many step checkpoints a computer keeps when `step_checkpoints_keep` is not set.
pub const DEFAULT_STEP_CHECKPOINTS_KEEP: u32 = 20;

/// What to do after a step, resolved from the project settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepSettings {
    pub checkpoints: bool,
    pub keep: u32,
}

impl Default for StepSettings {
    fn default() -> Self {
        Self {
            checkpoints: false,
            keep: DEFAULT_STEP_CHECKPOINTS_KEEP,
        }
    }
}

fn blank(names: &[String]) -> bool {
    names.iter().any(|name| name.trim().is_empty())
}

impl Settings {
    pub fn fork_ram_budget_bytes(&self) -> Option<u64> {
        self.fork_ram_budget
            .as_deref()
            .and_then(|text| parse_size(text).ok())
    }

    /// The longest a warm checkpoint may be and still be used by a fork that names no
    /// checkpoint: `warm_max_age`, else one minute.
    pub fn warm_max_age_secs(settings: Option<&Settings>) -> u64 {
        settings
            .and_then(|settings| settings.warm_max_age.as_deref())
            .and_then(|text| crate::clock::parse_duration_secs(text).ok())
            .unwrap_or(DEFAULT_WARM_MAX_AGE_SECS)
    }

    /// The step settings that apply: every caller resolves them here, so a per-agent override
    /// has one place to take effect.
    pub fn step_settings(settings: Option<&Settings>, _agent: Option<&str>) -> StepSettings {
        settings.map_or_else(StepSettings::default, |settings| StepSettings {
            checkpoints: settings.step_checkpoints,
            keep: settings
                .step_checkpoints_keep
                .unwrap_or(DEFAULT_STEP_CHECKPOINTS_KEEP),
        })
    }

    fn validate(&self) -> Result<(), String> {
        if let Some(text) = &self.fork_ram_budget {
            parse_size(text).map_err(|reason| format!("fork_ram_budget: {reason}"))?;
        }
        if let Some(text) = &self.warm_max_age {
            crate::clock::parse_duration_secs(text)
                .map_err(|reason| format!("warm_max_age: {reason}"))?;
        }
        if blank(&self.recipes) {
            return Err("recipes: names may not be empty".into());
        }
        if blank(&self.fork_secrets) {
            return Err("fork_secrets: names may not be empty".into());
        }
        for (agent, entry) in &self.agents {
            if agent.trim().is_empty() {
                return Err("agents: an agent name may not be empty".into());
            }
            if entry.fork_secrets.as_deref().is_some_and(blank) {
                return Err(format!(
                    "agents.{agent}.fork_secrets: names may not be empty"
                ));
            }
        }
        Ok(())
    }

    /// Whether a recipe may be forked from: any, unless the settings list some.
    pub fn check_recipe(settings: Option<&Settings>, name: &str) -> Result<(), CliError> {
        match settings {
            Some(settings)
                if !settings.recipes.is_empty()
                    && !settings.recipes.iter().any(|allowed| allowed == name) =>
            {
                Err(CliError::new(format!(
                    "The recipe {name} is not in recipes in .silo/{SETTINGS_FILE}."
                )))
            }
            _ => Ok(()),
        }
    }

    /// Whether `--secret name` may be used: any name when no settings file exists, else only
    /// the names `fork_secrets` lists.
    pub fn check_fork_secrets(
        settings: Option<&Settings>,
        names: &[String],
    ) -> Result<(), CliError> {
        let Some(settings) = settings else {
            return Ok(());
        };
        for name in names {
            if !settings.fork_secrets.iter().any(|allowed| allowed == name) {
                return Err(CliError::new(format!(
                    "The secret {name} is not in fork_secrets in .silo/{SETTINGS_FILE}."
                )));
            }
        }
        Ok(())
    }
}

impl Recipe {
    fn validate(&self) -> Result<(), String> {
        for (key, names) in [
            ("quiet", &self.quiet),
            ("scrub", &self.scrub),
            ("keep.restart", &self.keep.restart),
            ("keep.migrate", &self.keep.migrate),
            ("keep.after", &self.keep.after),
            ("setup", &self.setup),
            ("packages.apt", &self.packages.apt),
            ("packages.brew", &self.packages.brew),
            ("packages.npm", &self.packages.npm),
            ("packages.pip", &self.packages.pip),
            ("packages.pipx", &self.packages.pipx),
            ("packages.cargo", &self.packages.cargo),
        ] {
            if blank(names) {
                return Err(format!("{key}: entries may not be empty"));
            }
        }
        for (name, command) in &self.services {
            if command.trim().is_empty() {
                return Err(format!("services.{name}: the command may not be empty"));
            }
        }
        let (wrong, os) = match self.os {
            RecipeOs::Linux => ("brew", "linux"),
            RecipeOs::Macos => ("apt", "macos"),
        };
        let used = match self.os {
            RecipeOs::Linux => !self.packages.brew.is_empty(),
            RecipeOs::Macos => !self.packages.apt.is_empty(),
        };
        if used {
            return Err(format!("packages.{wrong}: not available when os is {os}"));
        }
        for name in self.services.keys() {
            let fine = !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
            if !fine {
                return Err(format!(
                    "services.{name}: a service name is lowercase letters, digits, - and _"
                ));
            }
        }
        Ok(())
    }

    /// The build inputs, as the engine takes them.
    pub fn spec(&self) -> RecipeSpec {
        RecipeSpec {
            os: match self.os {
                RecipeOs::Linux => silo_api::methods::recipes::RecipeOs::Linux,
                RecipeOs::Macos => silo_api::methods::recipes::RecipeOs::Macos,
            },
            cpus: self.cpus,
            memory_gib: self.memory,
            disk_gib: self.disk,
            packages: RecipePackages {
                apt: self.packages.apt.clone(),
                brew: self.packages.brew.clone(),
                npm: self.packages.npm.clone(),
                pip: self.packages.pip.clone(),
                pipx: self.packages.pipx.clone(),
                cargo: self.packages.cargo.clone(),
            },
            setup: self.setup.clone(),
            services: self
                .services
                .iter()
                .map(|(name, run)| RecipeService {
                    name: name.clone(),
                    run: run.clone(),
                })
                .collect(),
            ports: self.ports.clone(),
        }
    }
}

/// `8G`, `512M`, `1T` (binary multiples), or a plain number of bytes.
pub fn parse_size(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, unit) = text.split_at(split);
    let amount: u64 = digits
        .parse()
        .map_err(|_| format!("`{text}` is not a size like 8G"))?;
    let multiplier: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1 << 10,
        "m" | "mb" | "mib" => 1 << 20,
        "g" | "gb" | "gib" => 1 << 30,
        "t" | "tb" | "tib" => 1 << 40,
        _ => return Err(format!("`{text}` is not a size like 8G")),
    };
    amount
        .checked_mul(multiplier)
        .ok_or_else(|| format!("`{text}` is too large"))
}

fn parse_text<T: for<'de> Deserialize<'de>>(
    text: &str,
    validate: impl FnOnce(&T) -> Result<(), String>,
) -> Result<T, String> {
    // An empty file is an empty document, not an error.
    let source = if text.trim().is_empty() { "{}" } else { text };
    let value: T = serde_yaml_ng::from_str(source).map_err(|error| error.to_string())?;
    validate(&value)?;
    Ok(value)
}

fn parse_file<T: for<'de> Deserialize<'de>>(
    path: &Path,
    validate: impl FnOnce(&T) -> Result<(), String>,
) -> Result<T, CliError> {
    let text = fs::read_to_string(path)
        .map_err(|error| CliError::new(format!("Cannot read {}: {error}", path.display())))?;
    parse_text(&text, validate)
        .map_err(|reason| CliError::new(format!("{}: {reason}", path.display())))
}

pub fn parse_recipe(text: &str) -> Result<Recipe, String> {
    parse_text(text, Recipe::validate)
}

pub fn parse_settings(text: &str) -> Result<Settings, String> {
    parse_text(text, Settings::validate)
}

pub fn load_recipe(path: &Path) -> Result<Recipe, CliError> {
    parse_file(path, Recipe::validate)
}

pub fn load_settings(path: &Path) -> Result<Settings, CliError> {
    parse_file(path, Settings::validate)
}

/// Recipes shipped with the CLI. Minimal on purpose: they add what a coding agent usually
/// wants and nothing else.
const BUILT_IN: &[(&str, &str)] = &[
    (
        "linux-dev",
        "os: linux\npackages:\n  apt: [build-essential, jq, ripgrep]\n",
    ),
    (
        "macos-xcode",
        "os: macos\nsetup:\n  - xcode-select -p\n  - xcodebuild -version\n",
    ),
];

pub const DEFAULT_RECIPE: &str = "linux-dev";

pub fn built_in_names() -> Vec<&'static str> {
    BUILT_IN.iter().map(|(name, _)| *name).collect()
}

fn built_in(name: &str) -> Option<Recipe> {
    BUILT_IN
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, text)| parse_recipe(text).expect("a built-in recipe parses"))
}

/// The `.silo/` files found from a directory upward.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    pub directory: Option<PathBuf>,
    pub recipe: Option<PathBuf>,
    pub settings: Option<PathBuf>,
}

impl Project {
    /// Walks up from `start` to the nearest `.silo/` holding a recipe or settings file. The
    /// account directory `~/.silo` holds neither, so it is never taken for a project.
    pub fn discover(start: &Path) -> Self {
        for directory in start.ancestors() {
            let silo = directory.join(".silo");
            let recipe = silo.join(RECIPE_FILE);
            let settings = silo.join(SETTINGS_FILE);
            if recipe.is_file() || settings.is_file() || silo.join("recipes").is_dir() {
                return Self {
                    directory: Some(silo),
                    recipe: recipe.is_file().then_some(recipe),
                    settings: settings.is_file().then_some(settings),
                };
            }
        }
        Self::default()
    }

    pub fn load_settings(&self) -> Result<Option<Settings>, CliError> {
        self.settings.as_deref().map(load_settings).transpose()
    }

    /// A named recipe: `.silo/recipes/<name>.yaml` of the project, else a built-in one.
    pub fn named_recipe(&self, name: &str) -> Result<Recipe, CliError> {
        let fine = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if !fine {
            return Err(CliError::new(format!(
                "`{name}` is not a recipe name (lowercase letters, digits, - and _)."
            )));
        }
        if let Some(directory) = &self.directory {
            let path = directory.join("recipes").join(format!("{name}.yaml"));
            if path.is_file() {
                return load_recipe(&path);
            }
        }
        built_in(name).ok_or_else(|| {
            CliError::new(format!(
                "No recipe is called {name}: there is no .silo/recipes/{name}.yaml and the built-in recipes are {}.",
                built_in_names().join(", ")
            ))
        })
    }

    /// Every recipe the project and the CLI offer, by name, with where it comes from.
    pub fn recipe_names(&self) -> Vec<(String, &'static str)> {
        let mut found: BTreeMap<String, &'static str> = built_in_names()
            .into_iter()
            .map(|name| (name.to_owned(), "built-in"))
            .collect();
        if let Some(directory) = &self.directory {
            if let Ok(entries) = fs::read_dir(directory.join("recipes")) {
                for entry in entries.filter_map(Result::ok) {
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "yaml") {
                        if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                            found.insert(stem.to_owned(), "project");
                        }
                    }
                }
            }
        }
        found.into_iter().collect()
    }

    pub fn load_recipe(&self) -> Result<Option<Recipe>, CliError> {
        self.recipe.as_deref().map(load_recipe).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_recipe_parses() {
        let recipe = parse_recipe(
            "quiet: [poller, cron]\nscrub: [~/.env]\nkeep:\n  restart: [web]\n  migrate: [make migrate]\n  after: [make test]\n",
        )
        .unwrap();
        assert_eq!(recipe.quiet, ["poller", "cron"]);
        assert_eq!(recipe.scrub, ["~/.env"]);
        assert_eq!(recipe.keep.restart, ["web"]);
        assert_eq!(recipe.keep.migrate, ["make migrate"]);
        assert_eq!(recipe.keep.after, ["make test"]);
    }

    #[test]
    fn an_empty_file_is_valid() {
        assert_eq!(parse_recipe("").unwrap(), Recipe::default());
        assert_eq!(parse_settings("# nothing\n").unwrap(), Settings::default());
    }

    #[test]
    fn an_unknown_recipe_key_names_its_line() {
        let error = parse_recipe("quiet: [a]\nsrub: [b]\n").unwrap_err();
        assert!(error.contains("srub"), "{error}");
        assert!(error.contains("line 2"), "{error}");
        let nested = parse_recipe("keep:\n  restart: [a]\n  later: [b]\n").unwrap_err();
        assert!(
            nested.contains("later") && nested.contains("line 3"),
            "{nested}"
        );
    }

    #[test]
    fn a_wrong_type_is_an_error() {
        assert!(parse_recipe("quiet: poller\n").is_err());
        assert!(parse_recipe("quiet: ['']\n").unwrap_err().contains("quiet"));
    }

    #[test]
    fn settings_parse_with_every_key() {
        let settings = parse_settings(
            "recipes: [linux-dev]\nfork_secrets: [OPENAI_API_KEY]\nfold: auto-code\nfork_ram_budget: 8G\nauto_keep:\n  - when: { only: [code], tests: pass }\nagents:\n  openclaw: { fold: ask, fork_secrets: [] }\n",
        )
        .unwrap();
        assert_eq!(settings.fold, Fold::AutoCode);
        assert_eq!(settings.fork_ram_budget_bytes(), Some(8 << 30));
        assert_eq!(settings.auto_keep[0].when.only, ["code"]);
        assert_eq!(settings.agents["openclaw"].fold, Some(Fold::Ask));
        assert_eq!(settings.agents["openclaw"].fork_secrets, Some(vec![]));
        assert_eq!(parse_settings("").unwrap().fold, Fold::Ask);
    }

    #[test]
    fn warm_max_age_defaults_to_a_minute_and_reads_units() {
        assert_eq!(Settings::warm_max_age_secs(None), 60);
        let empty = parse_settings("").unwrap();
        assert_eq!(Settings::warm_max_age_secs(Some(&empty)), 60);
        let set = parse_settings("warm_max_age: 90s\n").unwrap();
        assert_eq!(Settings::warm_max_age_secs(Some(&set)), 90);
        let set = parse_settings("warm_max_age: 1h\n").unwrap();
        assert_eq!(Settings::warm_max_age_secs(Some(&set)), 3600);
        assert!(parse_settings("warm_max_age: soon\n")
            .unwrap_err()
            .contains("warm_max_age"));
    }

    #[test]
    fn settings_reject_unknown_keys_and_bad_values() {
        assert!(parse_settings("fold: sometimes\n").is_err());
        assert!(parse_settings("fork_ram_budget: lots\n")
            .unwrap_err()
            .contains("fork_ram_budget"));
        let unknown = parse_settings("recipes: []\nfork_secret: [X]\n").unwrap_err();
        assert!(
            unknown.contains("fork_secret") && unknown.contains("line 2"),
            "{unknown}"
        );
        assert!(parse_settings("agents:\n  a: { speed: 1 }\n").is_err());
    }

    #[test]
    fn sizes_read_binary_units() {
        assert_eq!(parse_size("8G"), Ok(8 << 30));
        assert_eq!(parse_size("512m"), Ok(512 << 20));
        assert_eq!(parse_size("1024"), Ok(1024));
        assert!(parse_size("G").is_err());
        assert!(parse_size("8X").is_err());
    }

    #[test]
    fn fork_secrets_limit_what_may_be_named() {
        let settings = parse_settings("fork_secrets: [OPENAI_API_KEY]\n").unwrap();
        let name = |value: &str| vec![value.to_owned()];
        assert!(Settings::check_fork_secrets(Some(&settings), &name("OPENAI_API_KEY")).is_ok());
        let error = Settings::check_fork_secrets(Some(&settings), &name("github")).unwrap_err();
        assert!(error.message.contains("github"));
        assert!(Settings::check_fork_secrets(None, &name("github")).is_ok());
    }

    #[test]
    fn discovery_walks_up_and_skips_account_directories() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("repo");
        let nested = project.join("src/deep");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(project.join(".silo")).unwrap();
        fs::write(project.join(".silo/recipe.yaml"), "quiet: [a]\n").unwrap();
        let found = Project::discover(&nested);
        assert_eq!(found.directory, Some(project.join(".silo")));
        assert!(found.settings.is_none());
        assert_eq!(found.load_recipe().unwrap().unwrap().quiet, ["a"]);

        let account = root.path().join("home");
        fs::create_dir_all(account.join(".silo/engine")).unwrap();
        assert_eq!(Project::discover(&account), Project::default());
    }

    #[test]
    fn load_errors_name_the_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recipe.yaml");
        fs::write(&path, "bogus: 1\n").unwrap();
        let error = load_recipe(&path).unwrap_err();
        assert!(error.message.contains("recipe.yaml") && error.message.contains("bogus"));
        assert!(load_recipe(&root.path().join("missing.yaml")).is_err());
    }

    #[test]
    fn a_build_recipe_parses_and_becomes_a_spec() {
        let recipe = parse_recipe(
            "os: macos\ncpus: 6\nmemory: 12\npackages:\n  brew: [jq]\n  npm: [typescript]\nsetup: [make deps]\nservices:\n  web: python3 -m http.server\nports: [8000]\nquiet: [x]\n",
        )
        .unwrap();
        let spec = recipe.spec();
        assert_eq!(spec.cpus, Some(6));
        assert_eq!(spec.memory_gib, Some(12));
        assert_eq!(spec.packages.brew, ["jq"]);
        assert_eq!(spec.services[0].name, "web");
        assert_eq!(spec.ports, [8000]);
        assert_eq!(parse_recipe("quiet: [a]\n").unwrap().os, RecipeOs::Linux);
    }

    #[test]
    fn recipe_build_keys_are_checked_with_their_line() {
        let error = parse_recipe("os: linux\npackages:\n  apk: [git]\n").unwrap_err();
        assert!(error.contains("apk") && error.contains("line 3"), "{error}");
        assert!(parse_recipe("os: windows\n").is_err());
        assert!(parse_recipe("os: linux\npackages:\n  brew: [jq]\n")
            .unwrap_err()
            .contains("brew"));
        assert!(parse_recipe("services:\n  Web: x\n").is_err());
        assert!(parse_recipe("setup: ['']\n").is_err());
    }

    #[test]
    fn named_recipes_come_from_the_project_then_the_built_ins() {
        let root = tempfile::tempdir().unwrap();
        let silo = root.path().join(".silo");
        fs::create_dir_all(silo.join("recipes")).unwrap();
        fs::write(silo.join("recipes/web.yaml"), "packages:\n  apt: [nginx]\n").unwrap();
        fs::write(silo.join("recipes/linux-dev.yaml"), "setup: [true]\n").unwrap();
        let project = Project::discover(root.path());
        assert_eq!(project.named_recipe("web").unwrap().packages.apt, ["nginx"]);
        assert_eq!(project.named_recipe("linux-dev").unwrap().setup, ["true"]);
        assert_eq!(
            project.named_recipe("macos-xcode").unwrap().os,
            RecipeOs::Macos
        );
        assert!(project
            .named_recipe("nope")
            .unwrap_err()
            .message
            .contains("linux-dev"));
        assert!(project.named_recipe("../x").is_err());
        let names = project.recipe_names();
        assert!(names.contains(&("web".to_owned(), "project")));
        assert!(names.contains(&("macos-xcode".to_owned(), "built-in")));
        assert_eq!(
            Project::default().named_recipe("linux-dev").unwrap().os,
            RecipeOs::Linux
        );
    }

    #[test]
    fn settings_limit_which_recipes_may_be_forked() {
        let settings = parse_settings("recipes: [linux-dev]\n").unwrap();
        assert!(Settings::check_recipe(Some(&settings), "linux-dev").is_ok());
        assert!(Settings::check_recipe(Some(&settings), "macos-xcode").is_err());
        assert!(Settings::check_recipe(None, "macos-xcode").is_ok());
        let open = parse_settings("fold: ask\n").unwrap();
        assert!(Settings::check_recipe(Some(&open), "any").is_ok());
    }
}
