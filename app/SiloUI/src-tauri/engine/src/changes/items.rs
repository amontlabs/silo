//! The items a keep can bring over or conflict on, and the answers a caller can give for them.
//!
//! An item is one file path, one package of a manager, one repository, one crontab, one launch
//! agent or one database. Its id is a string built only from what names it, so it is the same in
//! the diff, in a conflict and in a choice, and does not depend on the order of anything:
//!
//! | Item | Id |
//! | --- | --- |
//! | file (also an uncommitted file in a repository) | `file:<absolute path>` |
//! | package | `package:<manager>:<name>` |
//! | repository | `repo:<absolute path>` |
//! | crontab | `cron:<user>` |
//! | launch agent or daemon | `launch-agent:<scope>:<label>` |
//! | database | `database:<engine>:<name>` |

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::diff::{Diff, PackageChange, PathChange};

pub fn file(path: &str) -> String {
    format!("file:{path}")
}

pub fn package(manager: &str, name: &str) -> String {
    format!("package:{manager}:{name}")
}

pub fn repo(path: &str) -> String {
    format!("repo:{path}")
}

pub fn cron(user: &str) -> String {
    format!("cron:{user}")
}

pub fn launch_agent(scope: &str, label: &str) -> String {
    format!("launch-agent:{scope}:{label}")
}

pub fn database(engine: &str, name: &str) -> String {
    format!("database:{engine}:{name}")
}

/// What an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Package,
    Repo,
    Cron,
    LaunchAgent,
    Database,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::File => "file",
            Kind::Package => "package",
            Kind::Repo => "repository",
            Kind::Cron => "crontab",
            Kind::LaunchAgent => "launch agent",
            Kind::Database => "database",
        }
    }
}

/// The kind an id names, from its prefix.
pub fn kind_of(id: &str) -> Option<Kind> {
    let (prefix, _) = id.split_once(':')?;
    Some(match prefix {
        "file" => Kind::File,
        "package" => Kind::Package,
        "repo" => Kind::Repo,
        "cron" => Kind::Cron,
        "launch-agent" => Kind::LaunchAgent,
        "database" => Kind::Database,
        _ => return None,
    })
}

/// How a caller settles an item both sides changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Answer {
    /// The fork's version is written over main's.
    Fork,
    /// The item is left as main has it.
    Main,
    /// A repository only: the fork's commits arrive as a branch and nothing is merged.
    Branch,
}

impl Answer {
    pub fn word(self) -> &'static str {
        match self {
            Answer::Fork => "fork",
            Answer::Main => "main",
            Answer::Branch => "branch",
        }
    }
}

/// What the caller said about items, by id.
///
/// `skip` and an answer of `main` mean the same thing for the keep: the item stays as main has
/// it. They are kept apart only so the report can say whether an item was skipped or left by
/// choice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choices {
    pub answers: BTreeMap<String, Answer>,
    pub skip: BTreeSet<String>,
}

/// How an item stands against the caller's choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Said {
    Nothing,
    Skipped,
    Answered(Answer),
}

impl Choices {
    pub fn is_empty(&self) -> bool {
        self.answers.is_empty() && self.skip.is_empty()
    }

    pub fn said(&self, id: &str) -> Said {
        if self.skip.contains(id) {
            Said::Skipped
        } else {
            self.answers
                .get(id)
                .map_or(Said::Nothing, |a| Said::Answered(*a))
        }
    }
}

/// One item of a diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    /// Whether a repository has commits that can be put on a branch.
    pub has_commits: bool,
}

fn wanted_packages(change: &PackageChange) -> Vec<String> {
    let mut names: BTreeSet<String> = match &change.top_level {
        Some(names) => names.iter().cloned().collect(),
        None => change.added.keys().cloned().collect(),
    };
    names.extend(change.upgraded.keys().cloned());
    names.extend(change.removed.keys().cloned());
    names.into_iter().collect()
}

fn path_ids(changes: &mut [PathChange]) {
    for change in changes {
        change.id = file(&change.path);
    }
}

impl Diff {
    /// Gives every entry a keep can act on its id. Run once the diff is final.
    pub(super) fn assign_ids(&mut self) {
        path_ids(&mut self.settings.files);
        path_ids(&mut self.other);
        for change in &mut self.code {
            change.id = repo(&change.repo);
            path_ids(&mut change.gitignored);
            let prefix = change.repo.trim_end_matches('/').to_owned();
            change.dirty_ids = change
                .dirty_new
                .keys()
                .chain(change.renamed_from.values())
                .map(|rel| (rel.clone(), file(&format!("{prefix}/{rel}"))))
                .collect();
        }
        for change in &mut self.packages {
            change.ids = wanted_packages(change)
                .iter()
                .map(|name| package(&change.manager, name))
                .collect();
        }
        for change in &mut self.settings.cron {
            change.id = cron(&change.user);
        }
        for change in &mut self.settings.launch_agents {
            change.id = launch_agent(&change.scope, &change.label);
        }
        for change in &mut self.database {
            change.id = database(&change.engine, &change.name);
        }
    }

    /// The diff without ids. Ids add nothing to what a diff says (each is a function of its
    /// entry), so the review digest is taken over this form and stays the same across
    /// versions that did not have them.
    pub fn without_ids(&self) -> Diff {
        let mut bare = self.clone();
        for change in bare
            .settings
            .files
            .iter_mut()
            .chain(bare.other.iter_mut())
            .chain(bare.code.iter_mut().flat_map(|c| c.gitignored.iter_mut()))
        {
            change.id.clear();
        }
        for change in &mut bare.code {
            change.id.clear();
            change.dirty_ids.clear();
        }
        for change in &mut bare.packages {
            change.ids.clear();
        }
        for change in &mut bare.settings.cron {
            change.id.clear();
        }
        for change in &mut bare.settings.launch_agents {
            change.id.clear();
        }
        for change in &mut bare.database {
            change.id.clear();
        }
        bare
    }

    /// Every item the diff holds, by id.
    pub fn items(&self) -> BTreeMap<String, Item> {
        let mut items = BTreeMap::new();
        let mut add = |id: String, kind: Kind, has_commits: bool| {
            items.insert(
                id.clone(),
                Item {
                    id,
                    kind,
                    has_commits,
                },
            );
        };
        for change in self
            .settings
            .files
            .iter()
            .chain(&self.other)
            .chain(self.code.iter().flat_map(|c| &c.gitignored))
        {
            add(file(&change.path), Kind::File, false);
        }
        for change in &self.code {
            add(repo(&change.repo), Kind::Repo, change.commits.is_some());
            let prefix = change.repo.trim_end_matches('/');
            for rel in change.dirty_new.keys().chain(change.renamed_from.values()) {
                add(file(&format!("{prefix}/{rel}")), Kind::File, false);
            }
        }
        for change in &self.packages {
            for name in wanted_packages(change) {
                add(package(&change.manager, &name), Kind::Package, false);
            }
        }
        for change in &self.settings.cron {
            add(cron(&change.user), Kind::Cron, false);
        }
        for change in &self.settings.launch_agents {
            add(
                launch_agent(&change.scope, &change.label),
                Kind::LaunchAgent,
                false,
            );
        }
        for change in &self.database {
            add(
                database(&change.engine, &change.name),
                Kind::Database,
                false,
            );
        }
        items
    }

    /// Whether anything a keep could bring over or conflict on changed. Databases are left
    /// out: a keep never combines them, so they cannot make two sides disagree.
    pub fn has_reviewable_changes(&self) -> bool {
        !(self.code.is_empty()
            && self.packages.is_empty()
            && self.settings.files.is_empty()
            && self.settings.cron.is_empty()
            && self.settings.launch_agents.is_empty()
            && self.other.is_empty())
    }
}

/// Checks the caller's choices against the diff that was reviewed. Anything it rejects names
/// the item and says why.
///
/// A choice is checked against the reviewed diff, and does not join the review digest. The
/// digest pins what the fork changed; a choice answers how those changes meet main, which the
/// digest does not hold (main moves on its own). A real keep reaches this check only after its
/// digest matched, and a dry run checks the fork as it is now, so both judge a choice against
/// the same diff and a dry run with choices says what a real run would do.
pub fn validate(choices: &Choices, diff: &Diff) -> Result<(), String> {
    let items = diff.items();
    for id in &choices.skip {
        if let Some(answer) = choices.answers.get(id) {
            if *answer != Answer::Main {
                return Err(format!(
                    "{id} is in `skip` and also answered `{}`; a skipped item stays as main has it.",
                    answer.word()
                ));
            }
        }
    }
    let named = choices
        .answers
        .iter()
        .map(|(id, answer)| (id, Some(*answer)))
        .chain(choices.skip.iter().map(|id| (id, None)));
    for (id, answer) in named {
        let Some(item) = items.get(id) else {
            return Err(format!(
                "{id} is not in the changes that were reviewed, so it cannot be answered."
            ));
        };
        if item.kind == Kind::Database {
            return Err(format!(
                "{id} is a database. A keep never combines databases, so there is nothing to choose."
            ));
        }
        if answer == Some(Answer::Branch) {
            if item.kind != Kind::Repo {
                return Err(format!(
                    "{id} is a {}. Only a repository can be left as a branch.",
                    item.kind.label()
                ));
            }
            if !item.has_commits {
                return Err(format!(
                    "{id} has no commits in the fork, so there is nothing to put on a branch."
                ));
            }
        }
    }
    Ok(())
}

/// The branch the fork's commits are left on.
pub fn branch_name(fork: &str) -> String {
    let mut clean = String::new();
    for c in fork.chars() {
        let c = if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
            c
        } else {
            '-'
        };
        if c == '.' && clean.ends_with('.') {
            continue;
        }
        clean.push(c);
    }
    let clean = clean.trim_matches(|c| c == '.' || c == '-');
    let clean = clean.strip_suffix(".lock").unwrap_or(clean);
    if clean.is_empty() {
        "silo/fork".to_owned()
    } else {
        format!("silo/{clean}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_built_from_what_names_the_item() {
        assert_eq!(file("/root/.bashrc"), "file:/root/.bashrc");
        assert_eq!(package("dpkg", "jq"), "package:dpkg:jq");
        assert_eq!(repo("/root/app"), "repo:/root/app");
        assert_eq!(cron("root"), "cron:root");
        assert_eq!(launch_agent("user", "dev.x"), "launch-agent:user:dev.x");
        assert_eq!(database("postgres", "app"), "database:postgres:app");
        assert_eq!(kind_of("package:npm:@a/b"), Some(Kind::Package));
        assert_eq!(kind_of("nonsense"), None);
    }

    #[test]
    fn branch_names_are_safe_ref_names() {
        assert_eq!(branch_name("fix-login"), "silo/fix-login");
        assert_eq!(branch_name("my fork: v2"), "silo/my-fork--v2");
        assert_eq!(branch_name("a..b"), "silo/a.b");
        assert_eq!(branch_name("..x.lock"), "silo/x");
        assert_eq!(branch_name(""), "silo/fork");
        assert_eq!(branch_name("///"), "silo/fork");
    }
}
