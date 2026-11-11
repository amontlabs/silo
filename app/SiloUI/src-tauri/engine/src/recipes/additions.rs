//! What a merge adds to the recipe of the computer it merged into: the packages and settings
//! files it brought over, so a rebuild from the recipe gets the same machine. Only the stored
//! recipe and the computer's record change; nothing is written on the device.

use std::collections::BTreeMap;

use silo_api::methods::recipes::{RecipeDefinition, RecipeOs};

use super::{plain_word, validate};

/// Packages by recipe manager key, and settings file paths.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Additions {
    pub packages: BTreeMap<String, Vec<String>>,
    pub files: Vec<String>,
}

impl Additions {
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty() && self.files.is_empty()
    }

    pub fn add_package(&mut self, manager: &str, name: &str) {
        let Some(key) = manager_key(manager) else {
            return;
        };
        let names = self.packages.entry(key.to_owned()).or_default();
        if !names.iter().any(|known| known == name) {
            names.push(name.to_owned());
            names.sort();
        }
    }

    pub fn add_file(&mut self, path: &str) {
        if !self.files.iter().any(|known| known == path) {
            self.files.push(path.to_owned());
            self.files.sort();
        }
    }
}

/// The recipe key of a package manager as the diff names it, or none: recipes have no list
/// for the others.
pub fn manager_key(manager: &str) -> Option<&'static str> {
    match manager {
        "dpkg" | "apt" => Some("apt"),
        "brew" => Some("brew"),
        "npm" | "npm -g" => Some("npm"),
        "pip" | "pip3" => Some("pip"),
        "pipx" => Some("pipx"),
        "cargo" => Some("cargo"),
        _ => None,
    }
}

fn packages_of<'a>(recipe: &'a mut RecipeDefinition, key: &str) -> Option<&'a mut Vec<String>> {
    let packages = &mut recipe.spec.packages;
    Some(match key {
        "apt" => &mut packages.apt,
        "brew" => &mut packages.brew,
        "npm" => &mut packages.npm,
        "pip" => &mut packages.pip,
        "pipx" => &mut packages.pipx,
        "cargo" => &mut packages.cargo,
        _ => return None,
    })
}

/// The recipe with `adds` appended, and what was new to it. A package its recipe's system
/// cannot install, or whose name a package manager is not given, is left out. The result is
/// the old recipe when nothing was new.
pub fn append(recipe: &RecipeDefinition, adds: &Additions) -> (RecipeDefinition, Additions) {
    let mut out = recipe.clone();
    let mut fresh = Additions::default();
    let linux = out.spec.os == RecipeOs::Linux;
    for (key, names) in &adds.packages {
        let usable = match key.as_str() {
            "apt" => linux,
            "brew" => !linux,
            _ => true,
        };
        if !usable {
            continue;
        }
        let Some(list) = packages_of(&mut out, key) else {
            continue;
        };
        for name in names {
            if plain_word(name) && !list.contains(name) {
                list.push(name.clone());
                fresh.add_package(key, name);
            }
        }
    }
    for path in &adds.files {
        let absolute = path.starts_with('/') && !path.contains(['\0', '\n', '\r']);
        if absolute && !out.files.contains(path) {
            out.files.push(path.clone());
            fresh.add_file(path);
        }
    }
    if fresh.is_empty() || validate(&out.spec).is_err() {
        return (recipe.clone(), Additions::default());
    }
    (out, fresh)
}

/// After a merge into `target`: appends what it brought to the recipe the computer was built
/// from. The computer's record always gets it. The stored recipe of the same name gets it when
/// the owner saved one; a recipe that ships with Silo is never edited. Returns the recipe's
/// name and what was new; `None` when the computer has no recipe or nothing was new.
pub fn record_merge(
    library: &super::Library,
    target: &str,
    adds: &Additions,
) -> Result<Option<(String, Additions)>, String> {
    if adds.is_empty() {
        return Ok(None);
    }
    let Some(record) = super::records::get(target)? else {
        return Ok(None);
    };
    let (updated, fresh) = append(&record.recipe, adds);
    if fresh.is_empty() {
        return Ok(None);
    }
    if let Some(saved) = library.saved(&record.name)? {
        let (appended, _) = append(&saved, adds);
        library.save(&record.name, &appended)?;
    }
    super::records::amend(target, |stored| stored.recipe = updated)?;
    Ok(Some((record.name, fresh)))
}
