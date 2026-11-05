//! The keep planner: what to bring over, and what stops it. Items come from the diff's
//! buckets (never Not kept or Ignored); what main held at the fork point comes from the
//! base manifest and what main and the fork hold now is looked at live in `prepare`.

use std::collections::BTreeSet;

use serde::Serialize;

use super::{
    env::{Live, Os},
    scripts::{normalize, valid_package_word},
};
use crate::changes::{
    diff::Diff,
    manifest::{FileKind, Manifest},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Conflict {
    /// `file`, `package`, `crontab`, `git`, `credentials`, `destination` or `fork`.
    pub kind: String,
    pub item: String,
    pub detail: String,
}

impl Conflict {
    pub fn new(kind: &str, item: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { kind: kind.into(), item: item.into(), detail: detail.into() }
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
    pub rel: String,
    pub path: String,
    pub delete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoOp {
    pub repo: String,
    /// The repo does not exist on main.
    pub new: bool,
    pub commits: Option<(String, String)>,
    /// Main's head when the fork was made.
    pub base_head: Option<String>,
    pub dirty: Vec<DirtyOp>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentOp {
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
    pub notes: Vec<String>,
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

    pub fn summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.packages.is_empty() {
            let mut line = format!(
                "packages: {}",
                self.packages.iter().map(|p| format!("{} {}", p.name, p.version)).collect::<Vec<_>>().join(", ")
            );
            let deps: usize = self.dependencies.iter().map(|(_, names)| names.len()).sum();
            if deps > 0 {
                line.push_str(&format!(
                    " (+{deps} dependencies: {})",
                    self.dependencies.iter().flat_map(|(_, n)| n.iter().cloned()).collect::<Vec<_>>().join(", ")
                ));
            }
            out.push(line);
        }
        if !self.removals.is_empty() {
            out.push(format!("remove packages: {}", self.removals.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", ")));
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
    Decision::Conflict("changed on main since the fork was made, and differently in the fork".into())
}

/// The three-way rule for an uncommitted file: the blob at the fork point, and the hashes the
/// fork's and main's working trees hold now.
pub fn decide_dirty(base_blob: &str, fork: &str, main: &str) -> Decision {
    if fork == main {
        return Decision::Skip;
    }
    let main_changed = if base_blob == "-" { main != "-" } else { main != base_blob };
    if main_changed {
        Decision::Conflict("changed on main since the fork was made, and differently in the fork".into())
    } else {
        Decision::Write
    }
}

const REPLAYED: &[&str] = &["dpkg", "brew", "brew-cask", "npm", "pip", "pipx", "cargo"];

fn pkg_key(manager: &str, name: &str) -> (String, String) {
    (manager.to_owned(), name.to_owned())
}

/// Builds the plan from the three manifests: `base` (fork time), `fork` (now) and `main`
/// (now). `diff` is base against fork and names what changed.
pub fn build(diff: &Diff, base: &Manifest, fork: &Manifest, main: &Manifest, os: Os) -> Plan {
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
                plan.notes.push(format!("{manager} packages are not replayed: {}", wanted.union(&gone).cloned().collect::<Vec<_>>().join(", ")));
            }
            continue;
        }
        let scoped = manager == "npm";
        for name in wanted {
            let key = pkg_key(manager, &name);
            let Some(version) = fork.pkg.get(&key) else { continue };
            if !valid_package_word(&name, scoped) || !valid_package_word(version, false) {
                plan.conflicts.push(Conflict::new("package", format!("{manager} {name}"), "the name or version has characters a package manager is not given"));
                continue;
            }
            let (before, now) = (base.pkg.get(&key), main.pkg.get(&key));
            if now == Some(version) {
                continue;
            }
            if now != before {
                plan.conflicts.push(Conflict::new(
                    "package",
                    format!("{manager} {name}"),
                    format!(
                        "main has {}, the fork has {version}, they started from {}",
                        now.map_or("none", String::as_str),
                        before.map_or("none", String::as_str)
                    ),
                ));
                continue;
            }
            plan.packages.push(PkgOp { manager: manager.to_owned(), name, version: version.clone() });
        }
        for name in gone {
            let key = pkg_key(manager, &name);
            let (before, now) = (base.pkg.get(&key), main.pkg.get(&key));
            if now.is_none() {
                continue;
            }
            if !valid_package_word(&name, scoped) {
                plan.conflicts.push(Conflict::new("package", format!("{manager} {name}"), "the name has characters a package manager is not given"));
                continue;
            }
            if now != before {
                plan.conflicts.push(Conflict::new("package", format!("{manager} {name}"), "the fork removed it, but main changed it since the fork was made"));
                continue;
            }
            plan.removals.push(PkgOp { manager: manager.to_owned(), name, version: before.cloned().unwrap_or_default() });
        }
        if !change.dependencies.is_empty() {
            plan.dependencies.push((manager.to_owned(), change.dependencies.clone()));
        }
    }

    // Files (decided against live content in `prepare`).
    let crontab_files: BTreeSet<&str> = fork.cronf.values().chain(main.cronf.values()).map(String::as_str).collect();
    let mut candidates: BTreeSet<String> = BTreeSet::new();
    candidates.extend(diff.settings.files.iter().map(|c| c.path.clone()));
    candidates.extend(diff.other.iter().map(|c| c.path.clone()));
    for repo in &diff.code {
        candidates.extend(repo.gitignored.iter().map(|c| c.path.clone()));
    }
    for path in candidates {
        if path.starts_with("/var/spool/cron/") || crontab_files.contains(path.as_str()) {
            continue;
        }
        plan.file_cands.push(FileCand { base: live_of(base, &path), path: normalize(os, &path) });
    }

    // Git.
    for repo in &diff.code {
        let new = repo.new.is_some() && !main.git.contains_key(&repo.repo);
        if repo.removed {
            plan.notes.push(format!("{} was removed in the fork; main keeps it", repo.repo));
        }
        let mut dirty = Vec::new();
        for (rel, code) in &repo.dirty_new {
            let prefix = repo.repo.trim_end_matches('/');
            if let Some(from) = repo.renamed_from.get(rel).filter(|_| !code.contains('C')) {
                dirty.push(DirtyOp { rel: from.clone(), path: format!("{prefix}/{from}"), delete: true });
            }
            dirty.push(DirtyOp { rel: rel.clone(), path: format!("{prefix}/{rel}"), delete: code.contains('D') });
        }
        if repo.commits.is_none() && !new && dirty.is_empty() {
            continue;
        }
        plan.repos.push(RepoOp {
            repo: repo.repo.clone(),
            new,
            commits: repo.commits.clone(),
            base_head: base.git.get(&repo.repo).map(|g| g.head.clone()).filter(|h| !h.is_empty()),
            dirty,
        });
    }

    // Crontabs.
    for change in &diff.settings.cron {
        let path = fork.cronf.get(&change.user).or_else(|| main.cronf.get(&change.user)).cloned().unwrap_or_default();
        plan.cron.push((change.user.clone(), path));
    }

    // Launch agents and daemons: the plist is a file candidate; the keep loads it after.
    for change in &diff.settings.launch_agents {
        let key = (change.scope.clone(), change.label.clone());
        let delete = !fork.lagent.contains_key(&key);
        let Some(entry) = fork.lagent.get(&key).or_else(|| main.lagent.get(&key)).or_else(|| base.lagent.get(&key)) else {
            continue;
        };
        let path = normalize(os, &entry.path);
        let base_ident = match base.lagent.get(&key) {
            Some(a) => Base::File(a.digest.clone()),
            None => Base::Missing,
        };
        plan.file_cands.retain(|c| c.path != path);
        plan.file_cands.push(FileCand { path: path.clone(), base: base_ident });
        plan.agents.push(AgentOp { scope: change.scope.clone(), label: change.label.clone(), path, delete });
    }

    for db in &diff.database {
        plan.databases.push(format!("{} {}", db.engine, db.name));
    }
    plan
}
