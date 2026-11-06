//! Recipe write-back: after a keep that brought packages or settings files, the recipe the
//! computer came from learns about them, so a rebuild gets the same machine.
//!
//! The recipe is edited as text, not parsed and written again: `serde_yaml_ng` does not keep
//! comments or order, and a recipe is a file people read. Only lines are added or one flow
//! list is extended, and the result is parsed again and compared with what was meant before
//! it replaces the file; when the layout is one this code does not understand, nothing is
//! written and the lists to add are reported instead.

use std::{collections::BTreeMap, fs, path::Path};

use serde_json::{json, Value};

use crate::{
    config::{parse_recipe, Recipe},
    error::CliError,
};

/// What a keep adds to a recipe: packages by recipe manager key, and settings file paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Additions {
    pub packages: BTreeMap<&'static str, Vec<String>>,
    pub files: Vec<String>,
    /// Packages of a manager recipes have no list for.
    pub skipped: Vec<String>,
}

impl Additions {
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty() && self.files.is_empty()
    }
}

/// The recipe key of a package manager as the diff names it, or none.
fn recipe_manager(manager: &str) -> Option<&'static str> {
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

/// Packages added and settings files changed in a diff document (`silo diff --json`).
/// Only what is in the recipe's reach: added packages the user installed (not their
/// dependencies), and files that were added or modified.
pub fn additions(diff: &Value) -> Additions {
    let mut found = Additions::default();
    for entry in diff["packages"].as_array().into_iter().flatten() {
        let manager = entry["manager"].as_str().unwrap_or_default();
        let added: Vec<&str> = entry["added"]
            .as_object()
            .map(|added| added.keys().map(String::as_str).collect())
            .unwrap_or_default();
        if added.is_empty() {
            continue;
        }
        let listed = |key: &str| -> Vec<String> {
            entry[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|name| added.contains(name))
                .map(str::to_owned)
                .collect()
        };
        let mut names = listed("top_level");
        if names.is_empty() {
            names = listed("manual");
        }
        if names.is_empty() {
            names = added.iter().map(|name| (*name).to_owned()).collect();
        }
        match recipe_manager(manager) {
            Some(key) => found.packages.entry(key).or_default().extend(names),
            None => found
                .skipped
                .extend(names.into_iter().map(|name| format!("{manager}: {name}"))),
        }
    }
    for names in found.packages.values_mut() {
        names.sort();
        names.dedup();
    }
    for file in diff["settings"]["files"].as_array().into_iter().flatten() {
        let path = file["path"].as_str().unwrap_or_default();
        if !path.is_empty() && file["change"] != "deleted" {
            found.files.push(path.to_owned());
        }
    }
    found.files.sort();
    found.files.dedup();
    found
}

/// Keeps what the recipe does not list yet, in the order given.
fn missing(existing: &[String], wanted: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in wanted {
        if !existing.contains(name) && !out.contains(name) {
            out.push(name.clone());
        }
    }
    out
}

/// The additions the recipe does not already have.
pub fn new_in(recipe: &Recipe, wanted: &Additions) -> Additions {
    let mut out = Additions {
        skipped: wanted.skipped.clone(),
        files: missing(&recipe.files, &wanted.files),
        ..Additions::default()
    };
    for (manager, names) in &wanted.packages {
        let have = match *manager {
            "apt" => &recipe.packages.apt,
            "brew" => &recipe.packages.brew,
            "npm" => &recipe.packages.npm,
            "pip" => &recipe.packages.pip,
            "pipx" => &recipe.packages.pipx,
            _ => &recipe.packages.cargo,
        };
        let fresh = missing(have, names);
        if !fresh.is_empty() {
            out.packages.insert(manager, fresh);
        }
    }
    out
}

fn scalar(text: &str) -> String {
    let plain = text
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '/' || c == '~')
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._+@=/~:-".contains(c));
    if plain {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', "''"))
    }
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn is_blank_or_comment(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || trimmed.starts_with('#')
}

/// The lines after `start` that belong to the entry at `start` (indented deeper than it, with
/// blank and comment lines in between), as an exclusive end index that stops after the last
/// content line.
fn entry_end(lines: &[String], start: usize) -> usize {
    let base = indent_of(&lines[start]);
    let mut end = start + 1;
    for (offset, line) in lines.iter().enumerate().skip(start + 1) {
        if is_blank_or_comment(line) {
            continue;
        }
        if indent_of(line) <= base {
            break;
        }
        end = offset + 1;
    }
    end
}

fn key_line(
    lines: &[String],
    from: usize,
    to: usize,
    indent: Option<usize>,
    key: &str,
) -> Option<usize> {
    let prefix = format!("{key}:");
    (from..to).find(|&index| {
        let line = &lines[index];
        let trimmed = line.trim_start();
        !is_blank_or_comment(line)
            && indent.is_none_or(|wanted| indent_of(line) == wanted)
            && (trimmed == prefix || trimmed.starts_with(&format!("{prefix} ")))
    })
}

/// Adds `items` to the list at `key` below the mapping entry at `parent` (`None`: the top
/// level). Returns the new lines of the file.
fn add_to_list(
    mut lines: Vec<String>,
    parent: Option<&str>,
    key: &str,
    items: &[String],
) -> Result<Vec<String>, String> {
    let words: Vec<String> = items.iter().map(|item| scalar(item)).collect();
    let (from, to, depth) = match parent {
        None => (0, lines.len(), 0),
        Some(parent) => {
            let Some(at) = key_line(&lines, 0, lines.len(), Some(0), parent) else {
                // The parent mapping is new: add it with the list.
                while lines.last().is_some_and(|line| line.trim().is_empty()) {
                    lines.pop();
                }
                lines.push(format!("{parent}:"));
                lines.push(format!("  {key}: [{}]", words.join(", ")));
                return Ok(lines);
            };
            if !lines[at].trim_end().ends_with(':') {
                return Err(format!("{parent} is not written as a block"));
            }
            let end = entry_end(&lines, at);
            let depth = lines[at + 1..end]
                .iter()
                .find(|line| !is_blank_or_comment(line))
                .map_or(2, |line| indent_of(line));
            (at + 1, end, depth)
        }
    };
    match key_line(&lines, from, to, Some(depth), key) {
        None => {
            let line = format!("{}{key}: [{}]", " ".repeat(depth), words.join(", "));
            match parent {
                None => {
                    while lines.last().is_some_and(|line| line.trim().is_empty()) {
                        lines.pop();
                    }
                    lines.push(line);
                }
                Some(_) => lines.insert(to, line),
            }
            Ok(lines)
        }
        Some(at) => {
            let line = lines[at].clone();
            let (head, value) = line.split_once(':').unwrap_or((&line, ""));
            let (value, comment) = match value.find(" #") {
                Some(pos) => (&value[..pos], &value[pos..]),
                None => (value, ""),
            };
            let gap = if comment.is_empty() {
                ""
            } else {
                &value[value.trim_end().len()..]
            };
            let comment = format!("{gap}{comment}");
            let value = value.trim();
            if value.starts_with('[') {
                let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) else {
                    return Err(format!("{key} is a list that spans several lines"));
                };
                let joined = if inner.trim().is_empty() {
                    words.join(", ")
                } else {
                    format!("{}, {}", inner.trim_end(), words.join(", "))
                };
                lines[at] = format!("{head}: [{joined}]{comment}");
                Ok(lines)
            } else if value.is_empty() {
                let end = entry_end(&lines, at);
                let dash = lines[at + 1..end]
                    .iter()
                    .find(|line| line.trim_start().starts_with("- "))
                    .map(|line| indent_of(line));
                match dash {
                    Some(dash) => {
                        let last = (at + 1..end)
                            .rev()
                            .find(|&index| lines[index].trim_start().starts_with("- "))
                            .unwrap_or(end - 1);
                        for (offset, word) in words.iter().enumerate() {
                            lines
                                .insert(last + 1 + offset, format!("{}- {word}", " ".repeat(dash)));
                        }
                        Ok(lines)
                    }
                    None if end == at + 1 => {
                        lines[at] = format!("{head}: [{}]{comment}", words.join(", "));
                        Ok(lines)
                    }
                    None => Err(format!("{key} is not a list of plain entries")),
                }
            } else {
                Err(format!("{key} is not written as a list"))
            }
        }
    }
}

/// Applies `wanted` (what the recipe does not have yet) to the recipe text, or says why the
/// text cannot be edited safely. The result parses and holds what was added.
pub fn edit(text: &str, wanted: &Additions) -> Result<String, String> {
    let had_newline = text.ends_with('\n');
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    for (manager, names) in &wanted.packages {
        lines = add_to_list(lines, Some("packages"), manager, names)?;
    }
    if !wanted.files.is_empty() {
        lines = add_to_list(lines, None, "files", &wanted.files)?;
    }
    let mut edited = lines.join("\n");
    if had_newline || text.is_empty() {
        edited.push('\n');
    }
    let recipe = parse_recipe(&edited)
        .map_err(|error| format!("the edited recipe does not parse: {error}"))?;
    let still = new_in(&recipe, wanted);
    if !still.is_empty() {
        return Err("the edit did not put the entries where the recipe reads them".into());
    }
    Ok(edited)
}

/// Appends what a keep brought to the recipe file and says what happened:
/// `{file, packages, files, skipped}` or `{file, error, packages, files}` (nothing written).
pub fn apply(path: &Path, wanted: &Additions) -> Result<Value, CliError> {
    let text = fs::read_to_string(path)
        .map_err(|error| CliError::new(format!("Cannot read {}: {error}", path.display())))?;
    let recipe = parse_recipe(&text)
        .map_err(|reason| CliError::new(format!("{}: {reason}", path.display())))?;
    let fresh = new_in(&recipe, wanted);
    let mut report = json!({
        "file": path.display().to_string(),
        "packages": fresh.packages,
        "files": fresh.files,
        "skipped": fresh.skipped,
    });
    if fresh.packages.is_empty() && fresh.files.is_empty() {
        return Ok(report);
    }
    match edit(&text, &fresh) {
        Ok(edited) => {
            fs::write(path, edited).map_err(|error| {
                CliError::new(format!("Cannot write {}: {error}", path.display()))
            })?;
        }
        Err(reason) => {
            report["error"] = json!(reason);
            report["written"] = json!(false);
            return Ok(report);
        }
    }
    report["written"] = json!(true);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted(apt: &[&str], files: &[&str]) -> Additions {
        let mut additions = Additions::default();
        if !apt.is_empty() {
            additions
                .packages
                .insert("apt", apt.iter().map(|s| (*s).to_owned()).collect());
        }
        additions.files = files.iter().map(|s| (*s).to_owned()).collect();
        additions
    }

    #[test]
    fn packages_and_files_come_from_the_diff() {
        let diff = json!({
            "packages": [
                {"manager": "dpkg", "added": {"jq": "1", "libjq1": "1"}, "top_level": ["jq"], "manual": ["jq"]},
                {"manager": "npm -g", "added": {"typescript": "5"}, "manual": [], "top_level": []},
                {"manager": "brew cask", "added": {"slack": "1"}},
                {"manager": "pip", "added": {}, "upgraded": {"x": ["1", "2"]}}
            ],
            "settings": {"files": [
                {"path": "/etc/app.conf", "change": "added"},
                {"path": "/etc/gone.conf", "change": "deleted"}
            ]}
        });
        let found = additions(&diff);
        assert_eq!(found.packages["apt"], ["jq"]);
        assert_eq!(found.packages["npm"], ["typescript"]);
        assert_eq!(found.skipped, ["brew cask: slack"]);
        assert_eq!(found.files, ["/etc/app.conf"]);
        assert!(!found.is_empty() && additions(&json!({})).is_empty());
    }

    #[test]
    fn what_the_recipe_has_is_not_added_again() {
        let recipe = parse_recipe("packages:\n  apt: [jq]\nfiles: [/etc/a]\n").unwrap();
        let fresh = new_in(&recipe, &wanted(&["jq", "ripgrep"], &["/etc/a", "/etc/b"]));
        assert_eq!(fresh.packages["apt"], ["ripgrep"]);
        assert_eq!(fresh.files, ["/etc/b"]);
        assert!(new_in(&recipe, &wanted(&["jq"], &["/etc/a"])).is_empty());
    }

    #[test]
    fn a_flow_list_grows_and_keeps_its_comment_and_neighbours() {
        let text = "# the web box\nos: linux\npackages:\n  apt: [git, curl]   # basics\n  npm: [x]\nsetup: [make]\n";
        let edited = edit(text, &wanted(&["jq"], &[])).unwrap();
        assert_eq!(
            edited,
            "# the web box\nos: linux\npackages:\n  apt: [git, curl, jq]   # basics\n  npm: [x]\nsetup: [make]\n"
        );
    }

    #[test]
    fn a_block_list_gets_dashes_after_its_last_entry() {
        let text = "packages:\n  apt:\n    - git\n    # more soon\n    - curl\n  npm: [x]\n";
        let edited = edit(text, &wanted(&["jq", "ripgrep"], &[])).unwrap();
        assert_eq!(
            edited,
            "packages:\n  apt:\n    - git\n    # more soon\n    - curl\n    - jq\n    - ripgrep\n  npm: [x]\n"
        );
    }

    #[test]
    fn missing_keys_are_added_where_they_belong() {
        let text = "os: linux\npackages:\n  npm: [x]\n\nsetup: [make]\n";
        let edited = edit(text, &wanted(&["jq"], &["/etc/app.conf", "~/.bashrc"])).unwrap();
        assert_eq!(
            edited,
            "os: linux\npackages:\n  npm: [x]\n  apt: [jq]\n\nsetup: [make]\nfiles: [/etc/app.conf, ~/.bashrc]\n"
        );
        assert_eq!(
            edit("os: linux\n", &wanted(&["jq"], &[])).unwrap(),
            "os: linux\npackages:\n  apt: [jq]\n"
        );
        assert_eq!(
            edit("", &wanted(&[], &["/etc/a"])).unwrap(),
            "files: [/etc/a]\n"
        );
        assert_eq!(
            edit("packages:\n  apt: []\n", &wanted(&["jq"], &[])).unwrap(),
            "packages:\n  apt: [jq]\n"
        );
        assert_eq!(
            edit("packages:\n  apt:\n", &wanted(&["jq"], &[])).unwrap(),
            "packages:\n  apt: [jq]\n"
        );
    }

    #[test]
    fn odd_names_are_quoted() {
        let edited = edit("os: linux\n", &wanted(&[], &["/etc/my app.conf"])).unwrap();
        assert_eq!(edited, "os: linux\nfiles: ['/etc/my app.conf']\n");
    }

    #[test]
    fn a_layout_it_does_not_understand_is_left_alone() {
        let multi = "packages:\n  apt: [git,\n    curl]\n";
        assert!(edit(multi, &wanted(&["jq"], &[])).is_err());
        let inline = "packages: {apt: [git]}\n";
        assert!(edit(inline, &wanted(&["jq"], &[])).is_err());
    }

    #[test]
    fn apply_writes_the_file_and_reports_what_it_added() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("web.yaml");
        fs::write(&path, "packages:\n  apt: [git]\n").unwrap();
        let report = apply(&path, &wanted(&["git", "jq"], &["/etc/a"])).unwrap();
        assert_eq!(report["written"], true);
        assert_eq!(report["packages"]["apt"], json!(["jq"]));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "packages:\n  apt: [git, jq]\nfiles: [/etc/a]\n"
        );
        // Nothing new: the file is not touched.
        let again = apply(&path, &wanted(&["jq"], &["/etc/a"])).unwrap();
        assert!(again["packages"].as_object().unwrap().is_empty() && again["written"].is_null());
        // A layout that cannot be edited is reported with nothing written.
        fs::write(&path, "packages: {apt: [git]}\n").unwrap();
        let stuck = apply(&path, &wanted(&["jq"], &[])).unwrap();
        assert_eq!(stuck["written"], false);
        assert!(stuck["error"].as_str().unwrap().contains("packages"));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "packages: {apt: [git]}\n"
        );
    }
}
