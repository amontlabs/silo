//! What a merge adds to the recipe of the computer it merged into: the packages and settings
//! files it brought over, so a rebuild from the recipe gets the same machine. Only the stored
//! recipe and the computer's record change; nothing is written on the device.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use silo_api::methods::recipes::{RecipeDefinition, RecipeOs};

use super::{plain_word, validate};

/// Packages by recipe manager key, and settings file paths.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Additions {
    #[serde(default)]
    pub packages: BTreeMap<String, Vec<String>>,
    #[serde(default)]
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
        for name in names {
            if plain_word(name) {
                let Some(list) = packages_of(&mut out, key) else {
                    break;
                };
                if list.contains(name) || list.len() >= super::library::MAX_LIST {
                    continue;
                }
                list.push(name.clone());
                let size = serde_json::to_vec(&out).map(|b| b.len()).unwrap_or(usize::MAX);
                if size > super::library::MAX_BYTES {
                    if let Some(list) = packages_of(&mut out, key) {
                        list.pop();
                    }
                    continue;
                }
                fresh.add_package(key, name);
            }
        }
    }
    for path in &adds.files {
        let absolute = path.starts_with('/') && !path.contains(['\0', '\n', '\r']);
        if absolute && !out.files.contains(path) {
            if out.files.len() >= super::library::MAX_LIST {
                continue;
            }
            out.files.push(path.clone());
            let size = serde_json::to_vec(&out).map(|b| b.len()).unwrap_or(usize::MAX);
            if size > super::library::MAX_BYTES {
                out.files.pop();
                continue;
            }
            fresh.add_file(path);
        }
    }
    if fresh.is_empty() || validate(&out.spec).is_err() {
        return (recipe.clone(), Additions::default());
    }
    (out, fresh)
}

/// What one merge appended: to the computer's record, and to the saved recipe of the same
/// name when the merge was allowed to change it. Each holds only entries that were not there
/// before, so an undo can take back exactly them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recorded {
    /// The recipe's name.
    pub name: String,
    #[serde(default)]
    pub record: Additions,
    #[serde(default)]
    pub saved: Additions,
}

/// After a merge into `target`: appends what it brought to the recipe the computer was built
/// from. The computer's record always gets it. When `save_recipe` is true, the stored recipe
/// of the same name gets it when the owner saved one; a recipe that ships with Silo is never
/// edited. Returns what was new to each; `None` when the computer has no recipe or nothing
/// was new.
pub fn record_merge(
    library: &super::Library,
    target: &str,
    adds: &Additions,
    save_recipe: bool,
) -> Result<Option<Recorded>, String> {
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
    super::records::amend(target, |stored| stored.recipe = updated)?;
    let mut saved_fresh = Additions::default();
    if save_recipe {
        let _held = super::library::lock();
        if let Some(saved) = library.saved(&record.name)? {
            let (appended, added) = append(&saved, adds);
            if !added.is_empty() {
                library.save_locked(&record.name, &appended)?;
                saved_fresh = added;
            }
        }
    }
    Ok(Some(Recorded {
        name: record.name,
        record: fresh,
        saved: saved_fresh,
    }))
}

/// Takes `adds` out of `recipe`; whether anything was there to take.
fn remove_from(recipe: &mut RecipeDefinition, adds: &Additions) -> bool {
    let mut changed = false;
    for (key, names) in &adds.packages {
        if let Some(list) = packages_of(recipe, key) {
            let before = list.len();
            list.retain(|known| !names.contains(known));
            changed |= list.len() != before;
        }
    }
    let before = recipe.files.len();
    recipe.files.retain(|known| !adds.files.contains(known));
    changed | (recipe.files.len() != before)
}

/// Takes back what [`record_merge`] appended for the merge into `target`: exactly those
/// entries, and only while they are still listed. The computer's record is touched only while
/// it is still the recipe's; the saved recipe only when the merge changed it.
pub fn unrecord_merge(
    library: &super::Library,
    target: &str,
    recorded: &Recorded,
) -> Result<(), String> {
    super::records::amend(target, |stored| {
        if stored.name == recorded.name {
            remove_from(&mut stored.recipe, &recorded.record);
        }
    })?;
    if !recorded.saved.is_empty() {
        let _held = super::library::lock();
        if let Some(mut saved) = library.saved(&recorded.name)? {
            if remove_from(&mut saved, &recorded.saved) {
                library.save_locked(&recorded.name, &saved)?;
            }
        }
    }
    Ok(())
}
