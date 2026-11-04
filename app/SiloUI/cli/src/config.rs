//! The two project files under `.silo/`: the recipe (`recipe.yaml`, a clean machine spec)
//! and the settings (`silo.yaml`, rules for agents). Unknown keys are errors, and parse
//! errors carry the file path and line.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::error::CliError;

const RECIPE_FILE: &str = "recipe.yaml";
const SETTINGS_FILE: &str = "silo.yaml";

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// Process names stopped in a fork.
    #[serde(default)]
    pub quiet: Vec<String>,
    /// Paths removed from a fork.
    #[serde(default)]
    pub scrub: Vec<String>,
    #[serde(default)]
    pub keep: Keep,
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
    #[serde(default)]
    pub auto_keep: Vec<AutoKeepRule>,
    #[serde(default)]
    pub agents: BTreeMap<String, AgentOverride>,
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

    fn validate(&self) -> Result<(), String> {
        if let Some(text) = &self.fork_ram_budget {
            parse_size(text).map_err(|reason| format!("fork_ram_budget: {reason}"))?;
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
        ] {
            if blank(names) {
                return Err(format!("{key}: entries may not be empty"));
            }
        }
        Ok(())
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
            if recipe.is_file() || settings.is_file() {
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
}
