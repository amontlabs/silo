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
    /// The command that tests the project, run in a fork by `auto_keep` rules with
    /// `tests: pass`; exit code 0 passes.
    #[serde(default)]
    pub test: Option<String>,
    /// Settings files kept into the computer from forks (`silo fold` appends them). A record
    /// for people: the build does not create these files.
    #[serde(default)]
    pub files: Vec<String>,
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

/// The kinds of change a diff holds, as `auto_keep` rules name them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum Bucket {
    #[serde(rename = "code")]
    Code,
    #[serde(rename = "packages")]
    Packages,
    #[serde(rename = "settings")]
    Settings,
    #[serde(rename = "database")]
    Database,
    #[serde(rename = "other")]
    Other,
}

impl Bucket {
    pub fn name(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Packages => "packages",
            Self::Settings => "settings",
            Self::Database => "database",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum TestsRequirement {
    #[serde(rename = "pass")]
    Pass,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoKeepWhen {
    /// The buckets that may hold changes; every other bucket must be empty.
    #[serde(default)]
    pub only: Vec<Bucket>,
    /// `pass`: the recipe's `test:` command must exit 0 in the fork.
    #[serde(default)]
    pub tests: Option<TestsRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoKeepRule {
    pub when: AutoKeepWhen,
}

impl AutoKeepRule {
    /// What `fold: auto-code` stands for.
    pub fn auto_code() -> Self {
        Self {
            when: AutoKeepWhen {
                only: vec![Bucket::Code],
                tests: Some(TestsRequirement::Pass),
            },
        }
    }

    /// The rule as it is written in `silo.yaml`, for messages.
    pub fn describe(&self) -> String {
        let only: Vec<&str> = self.when.only.iter().map(|bucket| bucket.name()).collect();
        let mut text = format!("only: [{}]", only.join(", "));
        if self.when.tests.is_some() {
            text.push_str(", tests: pass");
        }
        format!("when: {{ {text} }}")
    }
}

/// Per-agent overrides of the top-level settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentOverride {
    #[serde(default)]
    pub recipes: Option<Vec<String>>,
    #[serde(default)]
    pub fork_secrets: Option<Vec<String>>,
    #[serde(default)]
    pub fold: Option<Fold>,
    #[serde(default)]
    pub auto_keep: Option<Vec<AutoKeepRule>>,
    #[serde(default)]
    pub step_checkpoints: Option<bool>,
    #[serde(default)]
    pub step_checkpoints_keep: Option<u32>,
    /// Connected devices (names) the agent may act on through MCP; none by default.
    #[serde(default)]
    pub devices: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The devices the resolved agent may act on through MCP (from its `agents:` entry).
    #[serde(skip)]
    pub devices: Vec<String>,
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
    /// The hosted reviewer (`silo review`); off without a URL.
    #[serde(default)]
    pub reviewer: Option<ReviewerSettings>,
}

/// `reviewer:` of `.silo/silo.yaml`. The token is never kept here: it is SILO_REVIEWER_TOKEN.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerSettings {
    #[serde(default)]
    pub url: Option<String>,
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

/// The name an agent is matched by: trimmed, lowercase, spaces as `-`. `None` when empty.
pub fn normalize_agent(name: Option<&str>) -> Option<String> {
    let name = name?
        .trim()
        .to_lowercase()
        .replace(char::is_whitespace, "-");
    (!name.is_empty()).then_some(name)
}

fn validate_rules(key: &str, rules: &[AutoKeepRule]) -> Result<(), String> {
    for (index, rule) in rules.iter().enumerate() {
        if rule.when.only.is_empty() {
            return Err(format!(
                "{key}[{index}].when.only: list the buckets that may change (code, packages, settings, database, other)"
            ));
        }
    }
    Ok(())
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

    /// The settings that apply to an agent, and the one place per-agent overrides take effect:
    /// the commands and the MCP server resolve the loaded file here once and read the result.
    /// `recipes`, `fork_secrets`, `fold`, `auto_keep` and the step settings of the agent's
    /// entry in `agents:` replace the top-level ones; an unknown agent (or none) gets the
    /// top-level settings. `auto_keep` of the result is the full rule list (`fold: auto-code`
    /// with no rules is expanded), and `agents` is emptied, so resolving again changes nothing.
    pub fn resolve(settings: Option<Settings>, agent: Option<&str>) -> Option<Settings> {
        Self::resolve_as(settings, agent, true)
    }

    /// Like [`Settings::resolve`]. `named` says `--agent` or `SILO_AGENT` named the agent; when
    /// the name is only the MCP client's self-report, `fold` and `auto_keep` of its entry do
    /// not apply (what keeps a fork without asking comes from the top level or a named agent),
    /// while `recipes`, `fork_secrets` and the step settings, which only narrow, still do.
    pub fn resolve_as(
        settings: Option<Settings>,
        agent: Option<&str>,
        named: bool,
    ) -> Option<Settings> {
        let mut settings = settings?;
        let agents = std::mem::take(&mut settings.agents);
        let mut entry = normalize_agent(agent)
            .and_then(|wanted| {
                agents
                    .into_iter()
                    .find(|(name, _)| {
                        normalize_agent(Some(name)).as_deref() == Some(wanted.as_str())
                    })
                    .map(|(_, entry)| entry)
            })
            .unwrap_or_default();
        if !named {
            entry.fold = None;
            entry.auto_keep = None;
            // A device is opened to an agent by the file or the command line, not by what
            // the client calls itself.
            entry.devices = None;
        }
        settings.devices = entry.devices.take().unwrap_or_default();
        let fold = entry.fold.unwrap_or(settings.fold);
        let rules = match (entry.auto_keep, entry.fold) {
            (Some(rules), _) => rules,
            // An agent that asks for every fold gets no automatic keep from the top level.
            (None, Some(Fold::Ask)) => Vec::new(),
            (None, _) => settings.auto_keep,
        };
        settings.auto_keep = if rules.is_empty() && fold == Fold::AutoCode {
            vec![AutoKeepRule::auto_code()]
        } else {
            rules
        };
        settings.fold = fold;
        if let Some(recipes) = entry.recipes {
            settings.recipes = recipes;
        }
        if let Some(secrets) = entry.fork_secrets {
            settings.fork_secrets = secrets;
        }
        if let Some(checkpoints) = entry.step_checkpoints {
            settings.step_checkpoints = checkpoints;
        }
        if let Some(keep) = entry.step_checkpoints_keep {
            settings.step_checkpoints_keep = Some(keep);
        }
        Some(settings)
    }

    /// The rules that keep a fork without asking, after [`Settings::resolve`].
    pub fn auto_keep_rules(settings: Option<&Settings>) -> &[AutoKeepRule] {
        settings.map_or(&[], |settings| settings.auto_keep.as_slice())
    }

    /// The step settings that apply, after [`Settings::resolve`].
    pub fn step_settings(settings: Option<&Settings>) -> StepSettings {
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
        validate_rules("auto_keep", &self.auto_keep)?;
        let mut seen = BTreeMap::new();
        for (agent, entry) in &self.agents {
            let Some(normal) = normalize_agent(Some(agent)) else {
                return Err("agents: an agent name may not be empty".into());
            };
            if let Some(other) = seen.insert(normal, agent) {
                return Err(format!(
                    "agents.{agent}: the same agent as agents.{other} (names are lowercase)"
                ));
            }
            if entry.recipes.as_deref().is_some_and(blank) {
                return Err(format!("agents.{agent}.recipes: names may not be empty"));
            }
            if entry.fork_secrets.as_deref().is_some_and(blank) {
                return Err(format!(
                    "agents.{agent}.fork_secrets: names may not be empty"
                ));
            }
            // An agent's entry may only narrow what the top level allows; the client name an
            // agent is matched by is self-reported, so it must not be a way to widen.
            if let Some(recipes) = &entry.recipes {
                if recipes.is_empty() && !self.recipes.is_empty() {
                    return Err(format!(
                        "agents.{agent}.recipes: an empty list would allow every recipe; list the ones to allow from recipes"
                    ));
                }
                if let Some(name) = recipes
                    .iter()
                    .find(|name| !self.recipes.is_empty() && !self.recipes.contains(name))
                {
                    return Err(format!(
                        "agents.{agent}.recipes: {name} is not in recipes; an agent can only narrow the top-level list"
                    ));
                }
            }
            if let Some(name) = entry
                .fork_secrets
                .iter()
                .flatten()
                .find(|name| !self.fork_secrets.contains(name))
            {
                return Err(format!(
                    "agents.{agent}.fork_secrets: {name} is not in fork_secrets; an agent can only narrow the top-level list"
                ));
            }
            if let Some(rules) = &entry.auto_keep {
                validate_rules(&format!("agents.{agent}.auto_keep"), rules)?;
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
            ("files", &self.files),
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
        if self
            .test
            .as_deref()
            .is_some_and(|test| test.trim().is_empty())
        {
            return Err("test: the command may not be empty".into());
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

    /// The settings file as they apply to `agent` (see [`Settings::resolve`]); the only way
    /// the commands and the MCP server read settings.
    pub fn load_settings(&self, agent: Option<&str>) -> Result<Option<Settings>, CliError> {
        self.load_settings_as(agent, true)
    }

    /// [`Project::load_settings`] for an agent that may be only the MCP client's self-reported
    /// name (`named` false); see [`Settings::resolve_as`].
    pub fn load_settings_as(
        &self,
        agent: Option<&str>,
        named: bool,
    ) -> Result<Option<Settings>, CliError> {
        let loaded = self.settings.as_deref().map(load_settings).transpose()?;
        Ok(Settings::resolve_as(loaded, agent, named))
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
        assert_eq!(settings.auto_keep[0].when.only, [Bucket::Code]);
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

    const AGENTS: &str = "recipes: [linux-dev, web]\nfork_secrets: [A]\nfold: ask\nstep_checkpoints: true\nstep_checkpoints_keep: 5\nauto_keep:\n  - when: { only: [code] }\nagents:\n  openclaw:\n    recipes: [web]\n    fork_secrets: []\n    fold: auto-code\n    step_checkpoints: false\n  claude-code:\n    fold: ask\n";

    #[test]
    fn an_agents_entry_replaces_the_top_level_settings() {
        let loaded = || Some(parse_settings(AGENTS).unwrap());
        let open = Settings::resolve(loaded(), Some("OpenClaw")).unwrap();
        assert_eq!(open.recipes, ["web"]);
        assert!(open.fork_secrets.is_empty());
        assert_eq!(open.fold, Fold::AutoCode);
        // No rules of its own: the top-level ones stay.
        assert_eq!(open.auto_keep.len(), 1);
        assert_eq!(open.auto_keep[0].when.tests, None);
        assert!(!open.step_checkpoints);
        assert_eq!(open.step_checkpoints_keep, Some(5));
        assert!(open.agents.is_empty());

        // `fold: ask` for an agent turns the top-level rules off for it.
        let asks = Settings::resolve(loaded(), Some("claude-code")).unwrap();
        assert!(asks.auto_keep.is_empty());
        assert_eq!(asks.recipes, ["linux-dev", "web"]);

        // Unknown or no agent: the top level, untouched.
        for agent in [Some("nobody"), None, Some("  ")] {
            let top = Settings::resolve(loaded(), agent).unwrap();
            assert_eq!(top.recipes, ["linux-dev", "web"]);
            assert_eq!(top.fork_secrets, ["A"]);
            assert_eq!(top.auto_keep.len(), 1);
            assert!(top.step_checkpoints);
        }
        assert_eq!(Settings::resolve(None, Some("openclaw")), None);
        let again = Settings::resolve(Some(open.clone()), Some("claude-code")).unwrap();
        assert_eq!(again, open);
    }

    #[test]
    fn a_client_reported_agent_name_does_not_pick_fold_or_auto_keep() {
        let loaded = || Some(parse_settings(AGENTS).unwrap());
        // openclaw says `fold: auto-code`, but only a named agent gets it.
        let reported = Settings::resolve_as(loaded(), Some("openclaw"), false).unwrap();
        assert_eq!(reported.fold, Fold::Ask);
        assert_eq!(reported.auto_keep.len(), 1);
        assert_eq!(reported.recipes, ["web"]);
        assert!(reported.fork_secrets.is_empty());
        // claude-code says `fold: ask`, which would drop the top-level rules for a named one.
        let asks = Settings::resolve_as(loaded(), Some("claude-code"), false).unwrap();
        assert_eq!(asks.auto_keep.len(), 1);
        let named = Settings::resolve_as(loaded(), Some("claude-code"), true).unwrap();
        assert!(named.auto_keep.is_empty());
    }

    #[test]
    fn step_settings_and_checks_read_the_resolved_settings() {
        let loaded = Some(parse_settings(AGENTS).unwrap());
        let open = Settings::resolve(loaded, Some("openclaw"));
        assert!(!Settings::step_settings(open.as_ref()).checkpoints);
        assert!(Settings::check_recipe(open.as_ref(), "web").is_ok());
        assert!(Settings::check_recipe(open.as_ref(), "linux-dev").is_err());
        assert!(Settings::check_fork_secrets(open.as_ref(), &["A".into()]).is_err());
    }

    #[test]
    fn an_agents_devices_apply_only_to_a_named_agent() {
        let text = "agents:\n  bot:\n    devices: [Mac mini]\n";
        let named = Settings::resolve_as(Some(parse_settings(text).unwrap()), Some("bot"), true);
        assert_eq!(named.unwrap().devices, vec!["Mac mini".to_owned()]);
        let reported =
            Settings::resolve_as(Some(parse_settings(text).unwrap()), Some("bot"), false);
        assert!(reported.unwrap().devices.is_empty());
        let none = Settings::resolve(Some(parse_settings("recipes: []\n").unwrap()), Some("bot"));
        assert!(none.unwrap().devices.is_empty());
        // `devices:` is an agent setting, not a top-level one.
        assert!(parse_settings("devices: [x]\n").is_err());
    }

    #[test]
    fn an_agents_entry_may_only_narrow_recipes_and_secrets() {
        let top = "recipes: [linux-dev]\nfork_secrets: [A]\nagents:\n";
        for (entry, key) in [
            ("  x: { recipes: [web] }\n", "agents.x.recipes"),
            ("  x: { recipes: [] }\n", "agents.x.recipes"),
            ("  x: { fork_secrets: [B] }\n", "agents.x.fork_secrets"),
        ] {
            let error = parse_settings(&format!("{top}{entry}")).unwrap_err();
            assert!(error.contains(key), "{error}");
        }
        // No top-level secrets: no agent can name one.
        assert!(parse_settings("agents:\n  x: { fork_secrets: [B] }\n").is_err());
        // Narrowing, and rules and fold, are allowed.
        let fine =
            format!("{top}  x: {{ recipes: [linux-dev], fork_secrets: [], fold: auto-code }}\n");
        assert!(parse_settings(&fine).is_ok());
        // With no top-level recipes (any), an agent may list some.
        assert!(parse_settings("agents:\n  x: { recipes: [web] }\n").is_ok());
    }

    #[test]
    fn agent_names_are_matched_in_lowercase() {
        assert_eq!(
            normalize_agent(Some(" Claude Code ")).as_deref(),
            Some("claude-code")
        );
        assert_eq!(normalize_agent(Some("")), None);
        assert_eq!(normalize_agent(None), None);
    }

    #[test]
    fn auto_keep_rules_are_checked_with_their_key() {
        let error =
            parse_settings("auto_keep:\n  - when: { only: [code, secrets] }\n").unwrap_err();
        assert!(error.contains("secrets"), "{error}");
        let error =
            parse_settings("auto_keep:\n  - when: { only: [code], tests: maybe }\n").unwrap_err();
        assert!(error.contains("maybe"), "{error}");
        let error = parse_settings("auto_keep:\n  - when: {}\n").unwrap_err();
        assert!(error.contains("auto_keep[0].when.only"), "{error}");
        let error =
            parse_settings("agents:\n  x:\n    auto_keep:\n      - when: { tests: pass }\n")
                .unwrap_err();
        assert!(error.contains("agents.x.auto_keep[0].when.only"), "{error}");
        let error = parse_settings("agents:\n  x: { recipes: [''] }\n").unwrap_err();
        assert!(error.contains("agents.x.recipes"), "{error}");
        let error = parse_settings("agents:\n  x: { speed: 1 }\n").unwrap_err();
        assert!(error.contains("speed"), "{error}");
        let error = parse_settings("agents:\n  Claude: {}\n  claude: {}\n").unwrap_err();
        assert!(error.contains("agents.claude"), "{error}");
    }

    #[test]
    fn a_recipe_declares_its_test_command_and_files() {
        let recipe = parse_recipe("test: make test\nfiles: [/etc/app.conf]\n").unwrap();
        assert_eq!(recipe.test.as_deref(), Some("make test"));
        assert_eq!(recipe.files, ["/etc/app.conf"]);
        assert!(parse_recipe("test: ' '\n").unwrap_err().contains("test"));
        // Neither is part of the build.
        assert_eq!(recipe.spec(), Recipe::default().spec());
    }
}
