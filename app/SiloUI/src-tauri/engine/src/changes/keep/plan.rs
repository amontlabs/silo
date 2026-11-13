//! The keep planner: what to bring over, and what stops it. Items come from the diff's
//! buckets (never Not kept or Ignored); what main held at the fork point comes from the
//! base manifest and what main and the fork hold now is looked at live in `prepare`.

use std::collections::BTreeSet;

use serde::Serialize;

use super::{
    env::{Live, Os},
    scripts::{normalize, valid_package_word},
    Request,
};
use crate::changes::{
    diff::Diff,
    items::{self, Answer, Said},
    manifest::{FileKind, Manifest},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    /// `file`, `package`, `crontab`, `git`, `credentials`, `destination` or `fork`.
    pub kind: String,
    pub item: String,
    pub detail: String,
    /// The id of the item, the key of its answer; empty for a conflict no answer settles.
    pub id: String,
    /// The answers that settle it.
    pub allowed: Vec<Answer>,
    /// What changed in the fork, in a few words.
    pub fork_change: String,
    /// What changed on main, in a few words.
    pub main_change: String,
}

impl Conflict {
    pub fn new(kind: &str, item: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            item: item.into(),
            detail: detail.into(),
            id: String::new(),
            allowed: Vec::new(),
            fork_change: String::new(),
            main_change: String::new(),
        }
    }

    /// Makes the conflict an item the caller can answer, with the answers that settle it.
    pub fn answerable(
        mut self,
        id: &str,
        allowed: &[Answer],
        fork_change: impl Into<String>,
        main_change: impl Into<String>,
    ) -> Self {
        self.id = id.to_owned();
        self.allowed = allowed.to_vec();
        self.fork_change = fork_change.into();
        self.main_change = main_change.into();
        self
    }
}

/// What happened to one item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportItem {
    pub id: String,
    /// `file`, `package`, `repository`, `crontab`, `launch agent` or `database`.
    pub kind: String,
    pub label: String,
    /// One of the `outcome` words.
    pub outcome: String,
    pub detail: String,
}

/// The words `ReportItem::outcome` takes.
pub mod outcome {
    /// The fork's version is in main.
    pub const BROUGHT_OVER: &str = "brought-over";
    /// Left as main has it, because the caller answered `main`.
    pub const LEFT_AS_MAIN: &str = "left-as-main";
    /// The fork's commits are a branch in main's repository.
    pub const BRANCH: &str = "branch";
    /// Not brought over because the caller listed it in `skip`.
    pub const SKIPPED: &str = "skipped";
    /// Main already holds what the fork holds.
    pub const ALREADY_THERE: &str = "already-there";
    /// A database: never combined with main's.
    pub const NOT_COMBINED: &str = "not-combined";
}

impl ReportItem {
    pub fn new(id: &str, label: &str, outcome: &str, detail: impl Into<String>) -> Self {
        Self {
            id: id.to_owned(),
            kind: items::kind_of(id)
                .map_or("item", items::Kind::label)
                .to_owned(),
            label: label.to_owned(),
            outcome: outcome.to_owned(),
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileAction {
    Copy,
    Link(String),
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileOp {
    pub id: String,
    pub path: String,
    pub action: FileAction,
}

/// What a path held at the fork point, from the base manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Base {
    Missing,
    /// A hashed file (`-` when the file was too large to hash).
    File(String),
    Link(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileCand {
    pub id: String,
    pub path: String,
    pub base: Base,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PkgOp {
    pub manager: String,
    pub name: String,
    pub version: String,
}

/// An uncommitted change in a repo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirtyOp {
    pub id: String,
    pub rel: String,
    pub path: String,
    pub delete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoOp {
    pub id: String,
    pub repo: String,
    /// The repo does not exist on main.
    pub new: bool,
    pub commits: Option<(String, String)>,
    /// Main's head when the fork was made.
    pub base_head: Option<String>,
    pub dirty: Vec<DirtyOp>,
    /// The branch the fork's commits are left on instead of being merged.
    pub branch: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentOp {
    pub id: String,
    pub scope: String,
    pub label: String,
    pub path: String,
    pub delete: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub packages: Vec<PkgOp>,
    pub removals: Vec<PkgOp>,
    /// Transitive dependencies the package managers will bring along: (manager, names).
    pub dependencies: Vec<(String, Vec<String>)>,
    pub repos: Vec<RepoOp>,
    pub file_cands: Vec<FileCand>,
    /// Decided in `prepare` from live looks at both computers.
    pub files: Vec<FileOp>,
    /// Users whose crontab changed, with the crontab file's path.
    pub cron: Vec<(String, String)>,
    pub agents: Vec<AgentOp>,
    pub conflicts: Vec<Conflict>,
    /// Changed databases, as `engine name`.
    pub databases: Vec<String>,
    /// The ids of `databases`, in the same order.
    pub database_ids: Vec<String>,
    /// Items the caller's choices left alone, and items main already holds.
    pub items: Vec<ReportItem>,
    pub notes: Vec<String>,
    /// The paths of the settings files the fork added or changed, as the diff names them.
    pub settings_files: BTreeSet<String>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
            && self.removals.is_empty()
            && self.repos.is_empty()
            && self.files.is_empty()
            && self.cron.is_empty()
            && self.agents.is_empty()
    }

    /// Whether the caller said to leave the item as main has it (`skip`, or the answer
    /// `main`); if so it is recorded and the planner goes no further with it.
    fn left_by_choice(&mut self, request: &Request, id: &str, label: &str) -> bool {
        match request.choices.said(id) {
            Said::Skipped => self.items.push(ReportItem::new(
                id,
                label,
                outcome::SKIPPED,
                "skipped; it stays as main has it",
            )),
            Said::Answered(Answer::Main) => self.items.push(ReportItem::new(
                id,
                label,
                outcome::LEFT_AS_MAIN,
                "left as main has it, as answered",
            )),
            _ => return false,
        }
        true
    }

    pub fn summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.packages.is_empty() {
            let mut line = format!(
                "packages: {}",
                self.packages
                    .iter()
                    .map(|p| format!("{} {}", p.name, p.version))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let deps: usize = self.dependencies.iter().map(|(_, names)| names.len()).sum();
            if deps > 0 {
                line.push_str(&format!(
                    " (+{deps} dependencies: {})",
                    self.dependencies
                        .iter()
                        .flat_map(|(_, n)| n.iter().cloned())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            out.push(line);
        }
        if !self.removals.is_empty() {
            out.push(format!(
                "remove packages: {}",
                self.removals
                    .iter()
                    .map(|p| p.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        for repo in &self.repos {
            out.push(format!("git: {}", repo.repo));
        }
        if !self.file_cands.is_empty() {
            out.push(format!("files: {}", self.file_cands.len()));
        }
        for (user, _) in &self.cron {
            out.push(format!("crontab: {user}"));
        }
        for agent in &self.agents {
            out.push(format!("launch agent: {}", agent.label));
        }
        out
    }
}

pub fn live_of(manifest: &Manifest, path: &str) -> Base {
    if let Some(entry) = manifest.files.get(path) {
        return match entry.kind {
            FileKind::Link => Base::Link(entry.digest.clone()),
            FileKind::File => Base::File(entry.digest.clone()),
        };
    }
    if let Some(entry) = manifest.gig.get(path) {
        return Base::File(entry.digest.clone());
    }
    Base::Missing
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// The target already holds what the fork holds.
    Skip,
    Write,
    Conflict(String),
}

/// Whether main still holds what it held at the fork point.
fn main_unchanged(base: &Base, main: &Live) -> bool {
    match (base, main) {
        (Base::Missing, Live::Missing) => true,
        (Base::File(digest), Live::File { sha, .. }) => digest != "-" && digest == sha,
        (Base::Link(target), Live::Link(now)) => target == now,
        _ => false,
    }
}

/// The three-way rule for one path: base (fork point), fork now and main now.
pub fn decide(base: &Base, fork: &Live, main: &Live) -> Decision {
    if matches!(fork, Live::Unreadable) || matches!(main, Live::Unreadable) {
        return Decision::Conflict("it cannot be read".into());
    }
    if matches!(fork, Live::Dir) {
        return Decision::Skip;
    }
    if fork == main {
        return Decision::Skip;
    }
    // Same bytes with another mode is a change the keep carries.
    if main_unchanged(base, main) {
        return Decision::Write;
    }
    if let (Live::File { sha: a, .. }, Live::File { sha: b, mode: _ }) = (fork, main) {
        if a == b {
            return Decision::Write;
        }
    }
    Decision::Conflict(
        "changed on main since the fork was made, and differently in the fork".into(),
    )
}

/// The three-way rule for an uncommitted file: the blob at the fork point, and the hashes the
/// fork's and main's working trees hold now.
pub fn decide_dirty(base_blob: &str, fork: &str, main: &str) -> Decision {
    if fork == main {
        return Decision::Skip;
    }
    let main_changed = if base_blob == "-" {
        main != "-"
    } else {
        main != base_blob
    };
    if main_changed {
        Decision::Conflict(
            "changed on main since the fork was made, and differently in the fork".into(),
        )
    } else {
        Decision::Write
    }
}

const REPLAYED: &[&str] = &["dpkg", "brew", "brew-cask", "npm", "pip", "pipx", "cargo"];

fn pkg_key(manager: &str, name: &str) -> (String, String) {
    (manager.to_owned(), name.to_owned())
}

/// What happened to a file in a few words, from its state at the fork point and now.
pub fn file_change(base: &Base, now: &Live) -> &'static str {
    match (base, now) {
        (Base::Missing, Live::Missing) => "absent",
        (Base::Missing, _) => "added",
        (_, Live::Missing) => "deleted",
        (_, Live::Unreadable) => "cannot be read",
        _ if main_unchanged(base, now) => "unchanged",
        _ => "changed",
    }
}

/// The same for an uncommitted file, from the blob at the fork point and the hash now
/// (`-` for none).
pub fn dirty_change(blob: &str, now: &str) -> &'static str {
    match (blob, now) {
        ("-", "-") => "absent",
        ("-", _) => "added",
        (_, "-") => "deleted",
        _ if blob == now => "unchanged",
        _ => "changed",
    }
}

fn version_change(before: Option<&String>, now: Option<&String>) -> String {
    match (before, now) {
        (_, None) => "removed".to_owned(),
        (None, Some(v)) => format!("added {v}"),
        (Some(a), Some(b)) if a == b => "unchanged".to_owned(),
        (Some(_), Some(v)) => format!("now {v}"),
    }
}

/// Builds the plan from the three manifests: `base` (fork time), `fork` (now) and `main`
/// (now). `diff` is base against fork and names what changed. The request's choices settle
/// what they can here: an item left as main has it never enters the plan, and a package the
/// fork may overwrite is planned like any other.
pub fn build(
    diff: &Diff,
    base: &Manifest,
    fork: &Manifest,
    main: &Manifest,
    os: Os,
    request: &Request,
) -> Plan {
    let mut plan = Plan::default();

    // Packages: by name and version, never by file.
    for change in &diff.packages {
        let manager = change.manager.as_str();
        let mut wanted: BTreeSet<String> = match &change.top_level {
            Some(names) => names.iter().cloned().collect(),
            None => change.added.keys().cloned().collect(),
        };
        wanted.extend(change.upgraded.keys().cloned());
        let gone: BTreeSet<String> = change.removed.keys().cloned().collect();
        if !REPLAYED.contains(&manager) {
            if !wanted.is_empty() || !gone.is_empty() {
                plan.notes.push(format!(
                    "{manager} packages are not replayed: {}",
                    wanted.union(&gone).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            continue;
        }
        let scoped = manager == "npm";
        for name in wanted {
            let key = pkg_key(manager, &name);
            let Some(version) = fork.pkg.get(&key) else {
                continue;
            };
            let id = items::package(manager, &name);
            let label = format!("{manager} {name}");
            let (before, now) = (base.pkg.get(&key), main.pkg.get(&key));
            if plan.left_by_choice(request, &id, &label) {
                continue;
            }
            if !valid_package_word(&name, scoped) || !valid_package_word(version, false) {
                plan.conflicts.push(
                    Conflict::new(
                        "package",
                        label,
                        "the name or version has characters a package manager is not given",
                    )
                    .answerable(
                        &id,
                        &[Answer::Main],
                        format!("version {version}"),
                        version_change(before, now),
                    ),
                );
                continue;
            }
            if now == Some(version) {
                plan.items.push(ReportItem::new(
                    &id,
                    &label,
                    outcome::ALREADY_THERE,
                    format!("main already has {version}"),
                ));
                continue;
            }
            let forced = request.choices.said(&id) == Said::Answered(Answer::Fork);
            if now != before && !forced {
                plan.conflicts.push(
                    Conflict::new(
                        "package",
                        label,
                        format!(
                            "main has {}, the fork has {version}, they started from {}",
                            now.map_or("none", String::as_str),
                            before.map_or("none", String::as_str)
                        ),
                    )
                    .answerable(
                        &id,
                        &[Answer::Fork, Answer::Main],
                        version_change(before, Some(version)),
                        version_change(before, now),
                    ),
                );
                continue;
            }
            plan.items.push(ReportItem::new(
                &id,
                &label,
                outcome::BROUGHT_OVER,
                format!("installs {version}"),
            ));
            plan.packages.push(PkgOp {
                manager: manager.to_owned(),
                name,
                version: version.clone(),
            });
        }
        for name in gone {
            let key = pkg_key(manager, &name);
            let id = items::package(manager, &name);
            let label = format!("{manager} {name}");
            let (before, now) = (base.pkg.get(&key), main.pkg.get(&key));
            if plan.left_by_choice(request, &id, &label) {
                continue;
            }
            if now.is_none() {
                plan.items.push(ReportItem::new(
                    &id,
                    &label,
                    outcome::ALREADY_THERE,
                    "main does not have it",
                ));
                continue;
            }
            if !valid_package_word(&name, scoped) {
                plan.conflicts.push(
                    Conflict::new(
                        "package",
                        label,
                        "the name has characters a package manager is not given",
                    )
                    .answerable(
                        &id,
                        &[Answer::Main],
                        "removed",
                        version_change(before, now),
                    ),
                );
                continue;
            }
            let forced = request.choices.said(&id) == Said::Answered(Answer::Fork);
            if now != before && !forced {
                plan.conflicts.push(
                    Conflict::new(
                        "package",
                        label,
                        "the fork removed it, but main changed it since the fork was made",
                    )
                    .answerable(
                        &id,
                        &[Answer::Fork, Answer::Main],
                        "removed",
                        version_change(before, now),
                    ),
                );
                continue;
            }
            plan.items.push(ReportItem::new(
                &id,
                &label,
                outcome::BROUGHT_OVER,
                "removes it",
            ));
            plan.removals.push(PkgOp {
                manager: manager.to_owned(),
                name,
                version: before.cloned().unwrap_or_default(),
            });
        }
        if !change.dependencies.is_empty() {
            plan.dependencies
                .push((manager.to_owned(), change.dependencies.clone()));
        }
    }

    // Files (decided against live content in `prepare`).
    let crontab_files: BTreeSet<&str> = fork
        .cronf
        .values()
        .chain(main.cronf.values())
        .map(String::as_str)
        .collect();
    let mut candidates: BTreeSet<String> = BTreeSet::new();
    candidates.extend(diff.settings.files.iter().map(|c| c.path.clone()));
    plan.settings_files.extend(
        diff.settings
            .files
            .iter()
            .filter(|c| c.change != crate::changes::diff::Change::Deleted)
            .map(|c| normalize(os, &c.path)),
    );
    candidates.extend(diff.other.iter().map(|c| c.path.clone()));
    for repo in &diff.code {
        candidates.extend(repo.gitignored.iter().map(|c| c.path.clone()));
    }
    for path in candidates {
        if path.starts_with("/var/spool/cron/") || crontab_files.contains(path.as_str()) {
            continue;
        }
        let id = items::file(&path);
        if plan.left_by_choice(request, &id, &path) {
            continue;
        }
        plan.file_cands.push(FileCand {
            id,
            base: live_of(base, &path),
            path: normalize(os, &path),
        });
    }

    // Git.
    for repo in &diff.code {
        let new = repo.new.is_some() && !main.git.contains_key(&repo.repo);
        if repo.removed {
            plan.notes.push(format!(
                "{} was removed in the fork; main keeps it",
                repo.repo
            ));
        }
        let mut dirty = Vec::new();
        for (rel, code) in &repo.dirty_new {
            let prefix = repo.repo.trim_end_matches('/');
            if let Some(from) = repo.renamed_from.get(rel).filter(|_| !code.contains('C')) {
                dirty.push(DirtyOp {
                    id: items::file(&format!("{prefix}/{from}")),
                    rel: from.clone(),
                    path: format!("{prefix}/{from}"),
                    delete: true,
                });
            }
            dirty.push(DirtyOp {
                id: items::file(&format!("{prefix}/{rel}")),
                rel: rel.clone(),
                path: format!("{prefix}/{rel}"),
                delete: code.contains('D'),
            });
        }
        let id = items::repo(&repo.repo);
        let left = plan.left_by_choice(request, &id, &repo.repo);
        let branch = (!left
            && request.choices.said(&id) == Said::Answered(Answer::Branch)
            && repo.commits.is_some())
        .then(|| items::branch_name(&request.fork));
        if left || branch.is_some() {
            // The repository's working tree stays as main has it, ignored files included.
            // Only what belongs to the repository goes: ignored files and uncommitted files,
            // not a settings or other file that happens to lie under it, and not a file the
            // caller answered `fork` on its own.
            let prefix = repo.repo.trim_end_matches('/');
            let elsewhere: BTreeSet<String> = diff
                .settings
                .files
                .iter()
                .chain(&diff.other)
                .map(|c| items::file(&c.path))
                .collect();
            let ignored: BTreeSet<String> = repo
                .gitignored
                .iter()
                .map(|c| items::file(&c.path))
                .chain(
                    repo.dirty_new
                        .keys()
                        .chain(repo.renamed_from.values())
                        .map(|rel| items::file(&format!("{prefix}/{rel}"))),
                )
                .filter(|id| {
                    !elsewhere.contains(id)
                        && request.choices.said(id) != Said::Answered(Answer::Fork)
                })
                .collect();
            let (dropped, kept): (Vec<FileCand>, Vec<FileCand>) =
                std::mem::take(&mut plan.file_cands)
                    .into_iter()
                    .partition(|c| ignored.contains(&c.id));
            plan.file_cands = kept;
            for cand in dropped {
                plan.items.push(ReportItem::new(
                    &cand.id,
                    &cand.path,
                    outcome::LEFT_AS_MAIN,
                    format!("not carried: {} stays as main has it", repo.repo),
                ));
            }
        }
        if left || (repo.commits.is_none() && !new && dirty.is_empty()) {
            continue;
        }
        plan.repos.push(RepoOp {
            id,
            repo: repo.repo.clone(),
            new,
            commits: repo.commits.clone(),
            base_head: base
                .git
                .get(&repo.repo)
                .map(|g| g.head.clone())
                .filter(|h| !h.is_empty()),
            dirty,
            branch,
        });
    }

    // Crontabs.
    for change in &diff.settings.cron {
        let id = items::cron(&change.user);
        if plan.left_by_choice(request, &id, &change.user) {
            continue;
        }
        let path = fork
            .cronf
            .get(&change.user)
            .or_else(|| main.cronf.get(&change.user))
            .cloned()
            .unwrap_or_default();
        plan.cron.push((change.user.clone(), path));
    }

    // Launch agents and daemons: the plist is a file candidate; the keep loads it after.
    for change in &diff.settings.launch_agents {
        let key = (change.scope.clone(), change.label.clone());
        let delete = !fork.lagent.contains_key(&key);
        let Some(entry) = fork
            .lagent
            .get(&key)
            .or_else(|| main.lagent.get(&key))
            .or_else(|| base.lagent.get(&key))
        else {
            continue;
        };
        let id = items::launch_agent(&change.scope, &change.label);
        let path = normalize(os, &entry.path);
        if plan.left_by_choice(request, &id, &change.label) {
            plan.file_cands.retain(|c| c.path != path);
            continue;
        }
        let base_ident = match base.lagent.get(&key) {
            Some(a) => Base::File(a.digest.clone()),
            None => Base::Missing,
        };
        plan.file_cands.retain(|c| c.path != path);
        plan.file_cands.push(FileCand {
            id: id.clone(),
            path: path.clone(),
            base: base_ident,
        });
        plan.agents.push(AgentOp {
            id,
            scope: change.scope.clone(),
            label: change.label.clone(),
            path,
            delete,
        });
    }

    for db in &diff.database {
        plan.databases.push(format!("{} {}", db.engine, db.name));
        plan.database_ids
            .push(items::database(&db.engine, &db.name));
    }
    plan
}
