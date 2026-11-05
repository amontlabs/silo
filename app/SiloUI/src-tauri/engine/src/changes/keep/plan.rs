//! The keep planner: what to bring over, and what stops it. Items come from the diff's
//! buckets (never Not kept or Ignored); what main holds now and held at the fork point comes
//! from the manifests themselves.

use std::collections::BTreeSet;

use serde::Serialize;

use super::super::diff::{Change, Diff};
use super::super::manifest::{FileKind, Manifest};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Conflict {
    /// `file`, `package`, `crontab`, `launch agent` or `git`.
    pub kind: String,
    pub item: String,
    pub detail: String,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PkgOp {
    pub manager: String,
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoOp {
    pub repo: String,
    /// The repo does not exist on main.
    pub new: bool,
    pub commits: Option<(String, String)>,
    /// Uncommitted files (absolute paths) to copy.
    pub dirty: Vec<String>,
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
    pub repos: Vec<RepoOp>,
    pub files: Vec<FileOp>,
    /// Users whose crontab changed, with the crontab file's path.
    pub cron: Vec<(String, String)>,
    pub agents: Vec<AgentOp>,
    pub conflicts: Vec<Conflict>,
    /// Changed databases, as `engine name`.
    pub databases: Vec<String>,
    /// Uncommitted files that main also has uncommitted: compared by content later.
    pub dirty_to_verify: Vec<String>,
    pub notes: Vec<String>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
            && self.repos.is_empty()
            && self.files.is_empty()
            && self.cron.is_empty()
            && self.agents.is_empty()
    }

    pub fn summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.packages.is_empty() {
            out.push(format!(
                "packages: {}",
                self.packages
                    .iter()
                    .map(|p| format!("{} {}", p.name, p.version))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        for repo in &self.repos {
            out.push(format!("git: {}", repo.repo));
        }
        if !self.files.is_empty() {
            out.push(format!("files: {}", self.files.len()));
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

#[derive(Debug, PartialEq, Eq)]
enum Ident {
    Missing,
    Link(String),
    File {
        digest: String,
        size: String,
        mtime: String,
    },
}

impl Ident {
    fn same(&self, other: &Ident) -> bool {
        match (self, other) {
            (Ident::Missing, Ident::Missing) => true,
            (Ident::Link(a), Ident::Link(b)) => a == b,
            (
                Ident::File {
                    digest: da,
                    size: sa,
                    mtime: ma,
                },
                Ident::File {
                    digest: db,
                    size: sb,
                    mtime: mb,
                },
            ) => {
                if da != "-" && db != "-" {
                    da == db
                } else {
                    sa == sb && ma == mb
                }
            }
            _ => false,
        }
    }
}

fn ident(manifest: &Manifest, path: &str) -> Ident {
    if let Some(entry) = manifest.files.get(path) {
        return match entry.kind {
            FileKind::Link => Ident::Link(entry.digest.clone()),
            FileKind::File => Ident::File {
                digest: entry.digest.clone(),
                size: entry.size.clone(),
                mtime: entry.mtime.clone(),
            },
        };
    }
    if let Some(entry) = manifest.gig.get(path) {
        return Ident::File {
            digest: entry.digest.clone(),
            size: entry.size.clone(),
            mtime: entry.mtime.clone(),
        };
    }
    Ident::Missing
}

fn pkg_key(manager: &str, name: &str) -> (String, String) {
    (manager.to_owned(), name.to_owned())
}

const REPLAYED: &[&str] = &["dpkg", "brew", "brew-cask", "npm", "pip", "pipx", "cargo"];

/// Builds the plan from the three manifests: `base` (fork time), `fork` (now) and `main` (now).
/// `diff` is base against fork and names what changed.
pub fn build(diff: &Diff, base: &Manifest, fork: &Manifest, main: &Manifest) -> Plan {
    let mut plan = Plan::default();

    // Packages: by name and version, never by file.
    for change in &diff.packages {
        let manager = change.manager.as_str();
        let mut wanted: BTreeSet<String> = match &change.top_level {
            Some(names) => names.iter().cloned().collect(),
            None => change.added.keys().cloned().collect(),
        };
        wanted.extend(change.upgraded.keys().cloned());
        if !REPLAYED.contains(&manager) {
            if !wanted.is_empty() {
                plan.notes.push(format!(
                    "{manager} packages are not replayed: {}",
                    wanted.iter().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            continue;
        }
        for name in wanted {
            let key = pkg_key(manager, &name);
            let Some(version) = fork.pkg.get(&key) else {
                continue;
            };
            let (before, now) = (base.pkg.get(&key), main.pkg.get(&key));
            if now == Some(version) {
                continue;
            }
            if now != before {
                plan.conflicts.push(Conflict {
                    kind: "package".into(),
                    item: format!("{manager} {name}"),
                    detail: format!(
                        "main has {}, the fork has {version}, they started from {}",
                        now.map_or("none", String::as_str),
                        before.map_or("none", String::as_str)
                    ),
                });
                continue;
            }
            plan.packages.push(PkgOp {
                manager: manager.to_owned(),
                name,
                version: version.clone(),
            });
        }
    }

    // Files.
    let crontab_files: BTreeSet<&str> = fork
        .cronf
        .values()
        .chain(main.cronf.values())
        .map(String::as_str)
        .collect();
    let agent_files: BTreeSet<&str> = fork
        .lagent
        .values()
        .chain(main.lagent.values())
        .map(|a| a.path.as_str())
        .collect();
    let mut candidates: BTreeSet<String> = BTreeSet::new();
    candidates.extend(diff.settings.files.iter().map(|c| c.path.clone()));
    candidates.extend(diff.other.iter().map(|c| c.path.clone()));
    for repo in &diff.code {
        candidates.extend(repo.gitignored.iter().map(|c| c.path.clone()));
    }
    for path in candidates {
        if path.starts_with("/var/spool/cron/")
            || crontab_files.contains(path.as_str())
            || agent_files.contains(path.as_str())
        {
            continue;
        }
        let (b, f, m) = (ident(base, &path), ident(fork, &path), ident(main, &path));
        if f.same(&m) {
            continue;
        }
        if !m.same(&b) {
            plan.conflicts.push(Conflict {
                kind: "file".into(),
                item: path,
                detail: "changed on main since the fork was made, and differently in the fork"
                    .into(),
            });
            continue;
        }
        let action = match f {
            Ident::Missing => FileAction::Delete,
            Ident::Link(target) => FileAction::Link(target),
            Ident::File { .. } => FileAction::Copy,
        };
        plan.files.push(FileOp { path, action });
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
        for rel in repo.dirty_new.keys() {
            let abs = format!("{}/{rel}", repo.repo.trim_end_matches('/'));
            if main
                .gitf
                .get(&repo.repo)
                .is_some_and(|files| files.contains_key(rel))
            {
                plan.dirty_to_verify.push(abs.clone());
            }
            dirty.push(abs);
        }
        if repo.commits.is_none() && !new && dirty.is_empty() {
            continue;
        }
        plan.repos.push(RepoOp {
            repo: repo.repo.clone(),
            new,
            commits: repo.commits.clone(),
            dirty,
        });
    }

    // Crontabs.
    for change in &diff.settings.cron {
        let path = fork
            .cronf
            .get(&change.user)
            .or_else(|| main.cronf.get(&change.user))
            .cloned()
            .unwrap_or_default();
        let _ = Change::Added;
        plan.cron.push((change.user.clone(), path));
    }

    // Launch agents and daemons.
    for change in &diff.settings.launch_agents {
        let key = (change.scope.clone(), change.label.clone());
        let digest = |m: &Manifest| m.lagent.get(&key).map(|a| a.digest.clone());
        let (b, f, m) = (digest(base), digest(fork), digest(main));
        if f == m {
            continue;
        }
        if m != b {
            plan.conflicts.push(Conflict {
                kind: "launch agent".into(),
                item: change.label.clone(),
                detail: "changed on main since the fork was made, and differently in the fork"
                    .into(),
            });
            continue;
        }
        let path = fork
            .lagent
            .get(&key)
            .or_else(|| main.lagent.get(&key))
            .map(|a| a.path.clone())
            .unwrap_or_default();
        plan.agents.push(AgentOp {
            scope: change.scope.clone(),
            label: change.label.clone(),
            path,
            delete: f.is_none(),
        });
    }

    for db in &diff.database {
        plan.databases.push(format!("{} {}", db.engine, db.name));
    }
    plan
}
