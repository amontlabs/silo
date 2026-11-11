//! The recipe file format: the YAML an owner writes and uploads with `silo recipe add`, and the
//! same text `silo recipe show` prints. Unknown keys are errors, and parse errors carry the
//! line. The engine stores the recipe; this module only converts between the file and the
//! engine's [`RecipeDefinition`].

use std::{collections::BTreeMap, fs, path::Path};

use serde::{Deserialize, Serialize};
use silo_api::methods::{
    keep::KeepHooks,
    recipes::{RecipeDefinition, RecipeOs, RecipePackages, RecipeService, RecipeSpec},
};

use crate::error::CliError;

/// The recipe `silo up` and `silo fork --recipe` use when none is named.
pub const DEFAULT_RECIPE: &str = "linux-dev";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeFile {
    /// The kind of computer the recipe builds; `linux` when left out.
    #[serde(default, skip_serializing_if = "is_default")]
    pub os: FileOs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpus: Option<u32>,
    /// Memory in GiB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<u32>,
    /// Disk in GiB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk: Option<u32>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub packages: Packages,
    /// Commands run once while the recipe is built: as root on Linux, as the account on macOS.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub setup: Vec<String>,
    /// Commands started at boot, by service name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub services: BTreeMap<String, String>,
    /// Ports a fork publishes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<u16>,
    /// `false`: the computer and its forks start no desktop (Linux; on when left out).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop: Option<bool>,
    /// Process names stopped in a fork.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quiet: Vec<String>,
    /// Absolute paths removed from a fork.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scrub: Vec<String>,
    /// What happens when changes are merged into a computer built from this recipe.
    #[serde(default, skip_serializing_if = "is_default")]
    pub keep: Keep,
    /// The command that tests the project. A note for people and agents: Silo never runs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,
    /// Settings files merged into the computer from forks. A merge appends to it; the build
    /// does not create these files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileOs {
    #[default]
    #[serde(rename = "linux")]
    Linux,
    #[serde(rename = "macos")]
    Macos,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packages {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apt: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub brew: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub npm: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pip: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pipx: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cargo: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keep {
    /// Services restarted after a merge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restart: Vec<String>,
    /// Commands run before the changes are applied (database migrations).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migrate: Vec<String>,
    /// Commands run after the changes are applied.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
}

fn blank(names: &[String]) -> bool {
    names.iter().any(|name| name.trim().is_empty())
}

impl RecipeFile {
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
        if let Some(path) = self.scrub.iter().find(|path| !path.starts_with('/')) {
            return Err(format!(
                "scrub: {path} must be an absolute path (the engine does not expand ~)"
            ));
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
        let (wrong, os, used) = match self.os {
            FileOs::Linux => ("brew", "linux", !self.packages.brew.is_empty()),
            FileOs::Macos => ("apt", "macos", !self.packages.apt.is_empty()),
        };
        if used {
            return Err(format!("packages.{wrong}: not available when os is {os}"));
        }
        Ok(())
    }

    /// The recipe as the engine stores it.
    pub fn definition(&self) -> RecipeDefinition {
        RecipeDefinition {
            spec: RecipeSpec {
                os: match self.os {
                    FileOs::Linux => RecipeOs::Linux,
                    FileOs::Macos => RecipeOs::Macos,
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
                desktop: self.desktop,
            },
            quiet: self.quiet.clone(),
            scrub: self.scrub.clone(),
            keep: (self.keep != Keep::default()).then(|| KeepHooks {
                restart: self.keep.restart.clone(),
                migrate: self.keep.migrate.clone(),
                after: self.keep.after.clone(),
            }),
            test: self.test.clone(),
            files: self.files.clone(),
            commands_hidden: None,
        }
    }

    /// The file form of a stored recipe.
    pub fn of(definition: &RecipeDefinition) -> Self {
        let spec = &definition.spec;
        let keep = definition.keep.clone();
        Self {
            os: match spec.os {
                RecipeOs::Linux => FileOs::Linux,
                RecipeOs::Macos => FileOs::Macos,
            },
            cpus: spec.cpus,
            memory: spec.memory_gib,
            disk: spec.disk_gib,
            packages: Packages {
                apt: spec.packages.apt.clone(),
                brew: spec.packages.brew.clone(),
                npm: spec.packages.npm.clone(),
                pip: spec.packages.pip.clone(),
                pipx: spec.packages.pipx.clone(),
                cargo: spec.packages.cargo.clone(),
            },
            setup: spec.setup.clone(),
            services: spec
                .services
                .iter()
                .map(|service| (service.name.clone(), service.run.clone()))
                .collect(),
            ports: spec.ports.clone(),
            desktop: spec.desktop,
            quiet: definition.quiet.clone(),
            scrub: definition.scrub.clone(),
            keep: keep.map_or_else(Keep::default, |keep| Keep {
                restart: keep.restart,
                migrate: keep.migrate,
                after: keep.after,
            }),
            test: definition.test.clone(),
            files: definition.files.clone(),
        }
    }

    pub fn to_yaml(&self) -> String {
        serde_yaml_ng::to_string(self).unwrap_or_default()
    }
}

/// Parses recipe text. An empty file is an empty recipe.
pub fn parse(text: &str) -> Result<RecipeFile, String> {
    let source = if text.trim().is_empty() { "{}" } else { text };
    let recipe: RecipeFile = serde_yaml_ng::from_str(source).map_err(|error| error.to_string())?;
    recipe.validate()?;
    Ok(recipe)
}

/// Reads a recipe file; errors name the file.
pub fn load(path: &Path) -> Result<RecipeFile, CliError> {
    let text = fs::read_to_string(path)
        .map_err(|error| CliError::new(format!("Cannot read {}: {error}", path.display())))?;
    parse(&text).map_err(|reason| CliError::new(format!("{}: {reason}", path.display())))
}

/// Whether `name` can name a recipe: lowercase letters, digits, `-` and `_`.
pub fn check_name(name: &str) -> Result<(), CliError> {
    let fine = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if fine {
        Ok(())
    } else {
        Err(CliError::new(format!(
            "`{name}` is not a recipe name (lowercase letters, digits, - and _)."
        )))
    }
}

/// A recipe's name from its file name when none is given: `web.yaml` is `web`.
pub fn name_from_path(path: &Path) -> Result<String, CliError> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    check_name(stem).map_err(|_| {
        CliError::new(format!(
            "Cannot name the recipe from {}: use --name with lowercase letters, digits, - and _.",
            path.display()
        ))
    })?;
    Ok(stem.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_recipe_parses_and_round_trips_through_the_engine_form() {
        let text = "os: linux\ncpus: 4\nmemory: 8\npackages:\n  apt: [jq]\n  npm: [typescript]\nsetup: [make deps]\nservices:\n  web: python3 -m http.server\nports: [8000]\nquiet: [poller, cron]\nscrub: [/root/.env]\nkeep:\n  restart: [web]\n  migrate: [make migrate]\n  after: [make test]\ntest: make test\nfiles: [/etc/app.conf]\n";
        let recipe = parse(text).unwrap();
        let definition = recipe.definition();
        assert_eq!(definition.spec.cpus, Some(4));
        assert_eq!(definition.spec.packages.apt, ["jq"]);
        assert_eq!(definition.spec.services[0].name, "web");
        assert_eq!(definition.quiet, ["poller", "cron"]);
        assert_eq!(definition.scrub, ["/root/.env"]);
        assert_eq!(definition.keep.as_ref().unwrap().migrate, ["make migrate"]);
        assert_eq!(definition.test.as_deref(), Some("make test"));
        assert_eq!(definition.files, ["/etc/app.conf"]);
        assert_eq!(RecipeFile::of(&definition), recipe);
        assert_eq!(parse(&recipe.to_yaml()).unwrap(), recipe);
    }

    #[test]
    fn an_empty_file_is_a_valid_linux_recipe() {
        let recipe = parse("").unwrap();
        assert_eq!(recipe, RecipeFile::default());
        assert_eq!(recipe.definition().spec.os, RecipeOs::Linux);
        assert_eq!(recipe.definition().keep, None);
    }

    #[test]
    fn mistakes_are_named_with_their_line() {
        let error = parse("quiet: [a]\nsrub: [b]\n").unwrap_err();
        assert!(
            error.contains("srub") && error.contains("line 2"),
            "{error}"
        );
        let error = parse("os: linux\npackages:\n  apk: [git]\n").unwrap_err();
        assert!(error.contains("apk") && error.contains("line 3"), "{error}");
        assert!(parse("os: windows\n").is_err());
        assert!(parse("os: linux\npackages:\n  brew: [jq]\n")
            .unwrap_err()
            .contains("brew"));
        assert!(parse("scrub: [~/.env]\n").unwrap_err().contains("absolute"));
        assert!(parse("services:\n  Web: x\n").is_err());
        assert!(parse("setup: ['']\n").is_err());
        assert!(parse("test: ' '\n").unwrap_err().contains("test"));
    }

    #[test]
    fn names_come_from_the_file_unless_given() {
        assert_eq!(name_from_path(Path::new("/x/web.yaml")).unwrap(), "web");
        assert!(name_from_path(Path::new("/x/My Recipe.yaml")).is_err());
        assert!(check_name("linux-dev").is_ok());
        assert!(check_name("Linux").is_err());
    }
}
