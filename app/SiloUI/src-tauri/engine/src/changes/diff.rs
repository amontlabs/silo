//! Bucketed comparison of two manifests. See `docs/SiloUI-CHANGES.md`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::ignore::{credential_group, IgnoreList};
use super::manifest::{FileEntry, FileKind, GigEntry, Manifest};

/// Version of the JSON shape produced by `Diff`'s serialization.
pub const DIFF_FORMAT: u32 = 1;

/// Bucket titles in display order (Ignored is a count line after them).
pub const BUCKETS: [&str; 6] = [
    "Code",
    "Packages",
    "Settings & files",
    "Database changed",
    "Other changed",
    "Not kept",
];

const TOOL_DIRS: &[&str] = &[
    ".local/bin/",
    ".cargo/bin/",
    ".bun/",
    ".deno/",
    ".pyenv/",
    ".nvm/",
    ".volta/",
    ".local/pipx/",
];
/// Longest first, so `/opt/homebrew` wins over `/opt`.
const GROUP_ROOTS: &[&str] = &[
    "/opt/homebrew",
    "/usr/local",
    "/Library",
    "/opt",
    "/srv",
    "/usr",
    "/var",
];
/// Packages shown for a manager that has no manual-install marker and no dependency data.
const CAP_DEFAULT: usize = 6;
/// Same, for pip: its installed set is mostly transitive dependencies.
const CAP_PIP: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Added,
    Modified,
    Deleted,
}

impl Change {
    fn sym(self) -> char {
        match self {
            Change::Added => '+',
            Change::Modified => '~',
            Change::Deleted => '-',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PathChange {
    pub path: String,
    pub change: Change,
}

/// Code (git) changes for one repo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoChange {
    pub repo: String,
    /// Set when the repo did not exist in the base: (head, branch).
    pub new: Option<(String, String)>,
    pub removed: bool,
    /// (old head, new head)
    pub commits: Option<(String, String)>,
    /// (old branch, new branch)
    pub branch: Option<(String, String)>,
    /// Dirty or untracked files that are new or changed: repo-relative path -> status code.
    pub dirty_new: BTreeMap<String, String>,
    pub dirty_gone: Vec<String>,
    /// Growth of the dirty+untracked count when the file lists do not explain it.
    pub dirty_delta: i64,
    /// Changed gitignored files (`.env`).
    pub gitignored: Vec<PathChange>,
}

impl RepoChange {
    fn is_empty(&self) -> bool {
        self.new.is_none()
            && !self.removed
            && self.commits.is_none()
            && self.branch.is_none()
            && self.dirty_new.is_empty()
            && self.dirty_gone.is_empty()
            && self.dirty_delta == 0
            && self.gitignored.is_empty()
    }
}

/// Package changes of one manager.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageChange {
    /// Manager name as in the manifest (`dpkg`, `brew`, `pip`, ...).
    pub manager: String,
    pub added: BTreeMap<String, String>,
    pub removed: BTreeMap<String, String>,
    pub upgraded: BTreeMap<String, (String, String)>,
    /// Added packages the manager marks as manually installed (empty when it has no marker).
    pub manual: Vec<String>,
    /// Direct requirements of the added packages, when the manifest has dependency data.
    pub requires: BTreeMap<String, Vec<String>>,
    /// Files changed by a package, attributed through `OWN` records: package -> paths.
    pub owned_files: BTreeMap<String, Vec<String>>,
    /// Added packages shown by name; `None` when nothing identifies them.
    pub top_level: Option<Vec<String>>,
    /// Added packages folded into a top-level one.
    pub dependencies: Vec<String>,
}

impl PackageChange {
    fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.upgraded.is_empty()
            && self.owned_files.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronChange {
    pub user: String,
    pub change: Change,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceChange {
    pub name: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchAgentChange {
    pub scope: String,
    pub label: String,
    pub change: Change,
}

/// Settings & files bucket detail.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsChanges {
    pub files: Vec<PathChange>,
    pub cron: Vec<CronChange>,
    pub services: Vec<ServiceChange>,
    pub launch_agents: Vec<LaunchAgentChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableChange {
    pub table: String,
    pub change: Change,
    pub rows_before: Option<u64>,
    pub rows_after: Option<u64>,
}

/// One changed database: a Postgres database or a SQLite file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbChange {
    /// `postgres` or `sqlite`.
    pub engine: String,
    /// Database name (Postgres) or file path (SQLite).
    pub name: String,
    pub change: Change,
    /// Schema digests before and after (Postgres), present when the schema differs.
    pub schema: Option<(Option<String>, Option<String>)>,
    pub tables: Vec<TableChange>,
    /// File digests before and after (SQLite).
    pub digest: Option<(Option<String>, Option<String>)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listener {
    pub proto: String,
    pub port: String,
    pub process: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotKept {
    pub listening: Vec<Listener>,
    pub processes: Vec<String>,
    pub services: Vec<String>,
    /// Credential group (directory ending in `/`, or a single file) -> changed paths.
    pub credentials: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrunedTree {
    pub path: String,
    pub delta: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ignored {
    pub paths: Vec<String>,
    pub pruned_trees: Vec<PrunedTree>,
}

impl Ignored {
    pub fn count(&self) -> u64 {
        self.paths.len() as u64 + self.pruned_trees.iter().map(|t| t.delta).sum::<u64>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub bucket: String,
    pub lines: Vec<String>,
}

/// The reviewable difference between two manifests. Serializes to the `silo diff --json`
/// shape (documented in `docs/SiloUI-CHANGES.md`); `summary` holds the text lines before
/// any line limit, derived from the detail fields.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diff {
    pub format: u32,
    pub home: String,
    pub summary: Vec<Section>,
    pub code: Vec<RepoChange>,
    pub packages: Vec<PackageChange>,
    pub settings: SettingsChanges,
    pub database: Vec<DbChange>,
    pub other: Vec<PathChange>,
    pub not_kept: NotKept,
    /// Changed files that live in git repos; reported through Code only.
    pub repo_files: Vec<String>,
    pub ignored: Ignored,
}

struct Ctx {
    home: String,
}

impl Ctx {
    fn home_rel<'a>(&self, p: &'a str) -> Option<(String, &'a str)> {
        if !self.home.is_empty() {
            if let Some(rest) = p.strip_prefix(&format!("{}/", self.home)) {
                return Some((self.home.clone(), rest));
            }
        }
        if let Some(rest) = p.strip_prefix("/root/") {
            return Some(("/root".to_owned(), rest));
        }
        for prefix in ["/home/", "/Users/"] {
            if let Some(rest) = p.strip_prefix(prefix) {
                if let Some((user, rel)) = rest.split_once('/') {
                    if !user.is_empty() {
                        return Some((format!("{prefix}{user}"), rel));
                    }
                }
            }
        }
        None
    }

    fn short(&self, p: &str) -> String {
        if !self.home.is_empty() {
            if let Some(rest) = p.strip_prefix(&self.home) {
                if rest.starts_with('/') {
                    return format!("~{rest}");
                }
            }
        }
        p.to_owned()
    }
}

fn same_entry(x: &FileEntry, y: &FileEntry) -> bool {
    if x.kind != y.kind {
        return false;
    }
    if x.kind == FileKind::Link {
        return x.digest == y.digest;
    }
    if x.digest != "-" && y.digest != "-" {
        return x.digest == y.digest;
    }
    x.size == y.size && x.mtime == y.mtime
}

fn file_changes(a: &BTreeMap<String, FileEntry>, b: &BTreeMap<String, FileEntry>) -> Vec<PathChange> {
    let mut out = Vec::new();
    let paths: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    for p in paths {
        let change = match (a.get(p), b.get(p)) {
            (None, Some(_)) => Change::Added,
            (Some(_), None) => Change::Deleted,
            (Some(x), Some(y)) if !same_entry(x, y) => Change::Modified,
            _ => continue,
        };
        out.push(PathChange { path: p.clone(), change });
    }
    out
}

fn gig_changes(a: &BTreeMap<String, GigEntry>, b: &BTreeMap<String, GigEntry>) -> Vec<(String, PathChange)> {
    let as_entry = |g: &GigEntry| FileEntry {
        kind: FileKind::File,
        size: g.size.clone(),
        mtime: g.mtime.clone(),
        digest: g.digest.clone(),
    };
    let mut out = Vec::new();
    let paths: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    for p in paths {
        let (change, repo) = match (a.get(p), b.get(p)) {
            (None, Some(y)) => (Change::Added, &y.repo),
            (Some(x), None) => (Change::Deleted, &x.repo),
            (Some(x), Some(y)) if !same_entry(&as_entry(x), &as_entry(y)) => (Change::Modified, &y.repo),
            _ => continue,
        };
        out.push((repo.clone(), PathChange { path: p.clone(), change }));
    }
    out
}

/// settings or other (credentials are routed before this).
fn is_settings(ctx: &Ctx, p: &str) -> bool {
    const SETTINGS_PREFIXES: &[&str] = &[
        "/etc/",
        "/var/spool/cron/",
        "/Library/LaunchAgents/",
        "/Library/LaunchDaemons/",
        "/opt/homebrew/etc/",
        "/usr/local/etc/",
    ];
    if SETTINGS_PREFIXES.iter().any(|x| p.starts_with(x)) {
        return true;
    }
    if let Some((_, rel)) = ctx.home_rel(p) {
        if TOOL_DIRS.iter().any(|t| rel.starts_with(t)) {
            return false;
        }
        if rel.starts_with("Library/Preferences/") || rel.starts_with("Library/LaunchAgents/") {
            return true;
        }
        return rel.split('/').any(|c| c.starts_with('.'));
    }
    false
}

fn other_group(ctx: &Ctx, p: &str) -> String {
    if let Some((h, rel)) = ctx.home_rel(p) {
        let base = if !ctx.home.is_empty() && h == ctx.home { "~".to_owned() } else { h };
        return format!("{}/{}", base, rel.split('/').next().unwrap_or(""));
    }
    for r in GROUP_ROOTS {
        if let Some(rest) = p.strip_prefix(&format!("{r}/")) {
            return format!("{}/{}", r, rest.split('/').next().unwrap_or(""));
        }
    }
    let sp = ctx.short(p);
    let parts: Vec<&str> = sp.trim_matches('/').split('/').take(2).collect();
    format!("/{}", parts.join("/"))
}

fn counts(chs: &[&PathChange]) -> String {
    let n = |c: Change| chs.iter().filter(|x| x.change == c).count();
    [Change::Added, Change::Modified, Change::Deleted]
        .iter()
        .filter(|c| n(**c) > 0)
        .map(|c| format!("{}{}", c.sym(), n(*c)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn rollup(ctx: &Ctx, chs: &[PathChange], key: impl Fn(&str) -> String) -> Vec<String> {
    let mut groups: BTreeMap<String, Vec<&PathChange>> = BTreeMap::new();
    for c in chs {
        groups.entry(key(&c.path)).or_default().push(c);
    }
    let mut lines = Vec::new();
    for (k, g) in groups {
        if g.len() > 3 {
            lines.push(format!("{}/ ({} files: {})", ctx.short(&k), g.len(), counts(&g)));
        } else {
            for c in g {
                lines.push(format!("{} {}", c.change.sym(), ctx.short(&c.path)));
            }
        }
    }
    lines
}

fn cap(mut items: Vec<String>, n: usize, noun: &str) -> Vec<String> {
    if items.len() > n {
        let rest = items.len() - n;
        items.truncate(n);
        items.push(format!("+{rest} {noun}"));
    }
    items
}

fn short7(s: &str) -> String {
    s.chars().take(7).collect()
}

fn base_name(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

fn pkg_label(mgr: &str) -> &str {
    match mgr {
        "dpkg" => "apt",
        "brew-cask" => "brew cask",
        "npm" => "npm -g",
        other => other,
    }
}

/// Removes from `items` everything that also appears in `base`.
fn subtract<T: PartialEq>(items: &mut Vec<T>, base: &[T]) {
    items.retain(|i| !base.contains(i));
}

fn rel_to<'a>(repo: &str, path: &'a str) -> &'a str {
    path.strip_prefix(repo).map(|r| r.trim_start_matches('/')).unwrap_or(path)
}

struct Builder<'a> {
    base: &'a Manifest,
    cur: &'a Manifest,
    ctx: Ctx,
    d: Diff,
}

impl<'a> Builder<'a> {
    fn run(mut self, ignore: &IgnoreList) -> Diff {
        self.d.format = DIFF_FORMAT;
        self.d.home = self.ctx.home.clone();
        let repos = self.classify_files(ignore);
        self.packages();
        self.code(repos, ignore);
        self.settings_extras();
        self.database();
        self.not_kept();
        self.pruned_trees();
        for v in self.d.not_kept.credentials.values_mut() {
            v.sort();
            v.dedup();
        }
        self.d
    }

    /// Splits the changed files into buckets. Returns the gitignored changes per repo and
    /// leaves credential routing of those to `code`.
    fn classify_files(&mut self, ignore: &IgnoreList) -> BTreeMap<String, Vec<PathChange>> {
        let (a, b) = (self.base, self.cur);
        let mut repos: Vec<&String> = a.git.keys().chain(b.git.keys()).collect::<BTreeSet<_>>().into_iter().collect();
        repos.sort_by_key(|r| std::cmp::Reverse(r.len()));
        let sqlite: BTreeSet<&String> = a.db.sqlite.keys().chain(b.db.sqlite.keys()).collect();
        let cronf: BTreeSet<&String> = a.cronf.values().chain(b.cronf.values()).collect();
        let lagent: BTreeSet<&String> = a
            .lagent
            .values()
            .chain(b.lagent.values())
            .map(|l| &l.path)
            .collect();
        let owner = |p: &str| b.own.get(p).or_else(|| a.own.get(p)).cloned();

        let mut owned: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for c in file_changes(&a.files, &b.files) {
            let p = &c.path;
            if ignore.matches(p) {
                self.d.ignored.paths.push(p.clone());
            } else if let Some(g) = credential_group(p) {
                self.d.not_kept.credentials.entry(g).or_default().push(p.clone());
            } else if repos.iter().any(|r| p.starts_with(&format!("{r}/"))) {
                self.d.repo_files.push(p.clone());
            } else if sqlite.contains(p) || cronf.contains(p) || lagent.contains(p) {
                // reported as a database, crontab or launch agent change
            } else if let Some(o) = owner(p) {
                owned.entry(o).or_default().push(p.clone());
            } else if is_settings(&self.ctx, p) {
                self.d.settings.files.push(c);
            } else {
                self.d.other.push(c);
            }
        }
        for ((mgr, name), paths) in owned {
            let pc = self.package_entry(&mgr);
            pc.owned_files.insert(name, paths);
        }

        let mut gigs: BTreeMap<String, Vec<PathChange>> = BTreeMap::new();
        for (repo, c) in gig_changes(&a.gig, &b.gig) {
            if ignore.matches(&c.path) {
                self.d.ignored.paths.push(c.path);
            } else if let Some(g) = credential_group(&c.path) {
                self.d.not_kept.credentials.entry(g).or_default().push(c.path);
            } else {
                gigs.entry(repo).or_default().push(c);
            }
        }
        gigs
    }

    fn package_entry(&mut self, mgr: &str) -> &mut PackageChange {
        if let Some(i) = self.d.packages.iter().position(|p| p.manager == mgr) {
            return &mut self.d.packages[i];
        }
        self.d.packages.push(PackageChange { manager: mgr.to_owned(), ..Default::default() });
        self.d.packages.last_mut().expect("just pushed")
    }

    fn packages(&mut self) {
        let (a, b) = (self.base, self.cur);
        let mgrs: BTreeSet<&String> = a.pkg.keys().chain(b.pkg.keys()).map(|k| &k.0).collect();
        for mgr in mgrs {
            let pa: BTreeMap<&String, &String> = a.pkg.iter().filter(|(k, _)| &k.0 == mgr).map(|(k, v)| (&k.1, v)).collect();
            let pb: BTreeMap<&String, &String> = b.pkg.iter().filter(|(k, _)| &k.0 == mgr).map(|(k, v)| (&k.1, v)).collect();
            let mut pc = PackageChange { manager: mgr.clone(), ..Default::default() };
            for (n, v) in &pb {
                match pa.get(n) {
                    None => {
                        pc.added.insert((*n).clone(), (*v).clone());
                    }
                    Some(old) if old != v => {
                        pc.upgraded.insert((*n).clone(), ((*old).clone(), (*v).clone()));
                    }
                    _ => {}
                }
            }
            for (n, v) in &pa {
                if !pb.contains_key(n) {
                    pc.removed.insert((*n).clone(), (*v).clone());
                }
            }
            if pc.added.is_empty() && pc.removed.is_empty() && pc.upgraded.is_empty() {
                continue;
            }
            pc.manual = pc
                .added
                .keys()
                .filter(|n| b.pkgm.contains(&(mgr.clone(), (*n).clone())))
                .cloned()
                .collect();
            for n in pc.added.keys() {
                if let Some(r) = b.pkgd.get(&(mgr.clone(), n.clone())) {
                    pc.requires.insert(n.clone(), r.clone());
                }
            }
            if let Some(existing) = self.d.packages.iter_mut().find(|p| &p.manager == mgr) {
                let owned = std::mem::take(&mut existing.owned_files);
                *existing = pc;
                existing.owned_files = owned;
            } else {
                self.d.packages.push(pc);
            }
        }
        self.d.packages.sort_by(|x, y| x.manager.cmp(&y.manager));
    }

    fn code(&mut self, mut gigs: BTreeMap<String, Vec<PathChange>>, _ignore: &IgnoreList) {
        let (a, b) = (self.base, self.cur);
        let repos: BTreeSet<String> = a.git.keys().chain(b.git.keys()).chain(gigs.keys()).cloned().collect();
        for r in &repos {
            let mut rc = RepoChange { repo: r.clone(), ..Default::default() };
            let (x, y) = (a.git.get(r), b.git.get(r));
            match (x, y) {
                (None, Some(y)) => rc.new = Some((y.head.clone(), y.branch.clone())),
                (Some(_), None) => rc.removed = true,
                (Some(x), Some(y)) => {
                    if x.head != y.head {
                        rc.commits = Some((x.head.clone(), y.head.clone()));
                    }
                    if x.branch != y.branch {
                        rc.branch = Some((x.branch.clone(), y.branch.clone()));
                    }
                }
                (None, None) => {}
            }
            if let Some(y) = y {
                let empty = BTreeMap::new();
                let fa = if x.is_some() { a.gitf.get(r).unwrap_or(&empty) } else { &empty };
                let fb = b.gitf.get(r).unwrap_or(&empty);
                let cred = |rel: &str, nk: &mut NotKept| -> bool {
                    let target = rel.rsplit(" -> ").next().unwrap_or(rel);
                    let abs = format!("{}/{}", r.trim_end_matches('/'), target);
                    match credential_group(&abs) {
                        Some(g) => {
                            nk.credentials.entry(g).or_default().push(abs);
                            true
                        }
                        None => false,
                    }
                };
                for (p, code) in fb {
                    if fa.get(p) != Some(code) && !cred(p, &mut self.d.not_kept) {
                        rc.dirty_new.insert(p.clone(), code.clone());
                    }
                }
                for p in fa.keys() {
                    if !fb.contains_key(p) {
                        rc.dirty_gone.push(p.clone());
                    }
                }
                if let Some(x) = x {
                    if rc.dirty_new.is_empty() && rc.dirty_gone.is_empty() {
                        let dd = (y.dirty + y.untracked) as i64 - (x.dirty + x.untracked) as i64;
                        if dd > 0 {
                            rc.dirty_delta = dd;
                        }
                    }
                }
            }
            if let Some(g) = gigs.remove(r) {
                rc.gitignored = g;
            }
            if !rc.is_empty() {
                self.d.code.push(rc);
            }
        }
    }

    fn settings_extras(&mut self) {
        let (a, b) = (self.base, self.cur);
        let users: BTreeSet<&String> = a.cron.keys().chain(b.cron.keys()).collect();
        for u in users {
            let (x, y) = (a.cron.get(u).map(String::as_str).unwrap_or("-"), b.cron.get(u).map(String::as_str).unwrap_or("-"));
            if x != y {
                let change = if x == "-" {
                    Change::Added
                } else if y == "-" {
                    Change::Deleted
                } else {
                    Change::Modified
                };
                self.d.settings.cron.push(CronChange { user: u.clone(), change });
            }
        }
        for (s, from) in &a.svc {
            if let Some(to) = b.svc.get(s) {
                if from != to {
                    self.d.settings.services.push(ServiceChange { name: s.clone(), from: from.clone(), to: to.clone() });
                }
            }
        }
        let keys: BTreeSet<&(String, String)> = a.lagent.keys().chain(b.lagent.keys()).collect();
        for k in keys {
            let change = match (a.lagent.get(k), b.lagent.get(k)) {
                (None, Some(_)) => Change::Added,
                (Some(_), None) => Change::Deleted,
                (Some(x), Some(y)) if x.digest != y.digest => Change::Modified,
                _ => continue,
            };
            self.d.settings.launch_agents.push(LaunchAgentChange { scope: k.0.clone(), label: k.1.clone(), change });
        }
    }

    fn database(&mut self) {
        let (a, b) = (&self.base.db, &self.cur.db);
        let dbs: BTreeSet<&String> = a.pg_schema.keys().chain(b.pg_schema.keys()).chain(a.pg_tables.keys().map(|k| &k.0)).chain(b.pg_tables.keys().map(|k| &k.0)).collect();
        for db in dbs {
            let (sa, sb) = (a.pg_schema.get(db), b.pg_schema.get(db));
            let tables: BTreeSet<&String> = a.pg_tables.keys().chain(b.pg_tables.keys()).filter(|k| &k.0 == db).map(|k| &k.1).collect();
            let mut tcs = Vec::new();
            for t in tables {
                let key = (db.clone(), t.clone());
                let (ta, tb) = (a.pg_tables.get(&key), b.pg_tables.get(&key));
                let change = match (ta, tb) {
                    (None, Some(_)) => Change::Added,
                    (Some(_), None) => Change::Deleted,
                    (Some(x), Some(y)) if x.digest != y.digest => Change::Modified,
                    _ => continue,
                };
                tcs.push(TableChange {
                    table: t.clone(),
                    change,
                    rows_before: ta.and_then(|t| t.rows),
                    rows_after: tb.and_then(|t| t.rows),
                });
            }
            let in_a = sa.is_some() || a.pg_tables.keys().any(|k| &k.0 == db);
            let in_b = sb.is_some() || b.pg_tables.keys().any(|k| &k.0 == db);
            let change = match (in_a, in_b) {
                (false, true) => Change::Added,
                (true, false) => Change::Deleted,
                _ => Change::Modified,
            };
            let schema = if sa != sb { Some((sa.cloned(), sb.cloned())) } else { None };
            if change == Change::Modified && schema.is_none() && tcs.is_empty() {
                continue;
            }
            self.d.database.push(DbChange { engine: "postgres".into(), name: db.clone(), change, schema, tables: tcs, digest: None });
        }
        let files: BTreeSet<&String> = a.sqlite.keys().chain(b.sqlite.keys()).collect();
        for f in files {
            let (x, y) = (a.sqlite.get(f), b.sqlite.get(f));
            let change = match (x, y) {
                (None, Some(_)) => Change::Added,
                (Some(_), None) => Change::Deleted,
                (Some(x), Some(y)) if x != y => Change::Modified,
                _ => continue,
            };
            self.d.database.push(DbChange {
                engine: "sqlite".into(),
                name: f.clone(),
                change,
                schema: None,
                tables: Vec::new(),
                digest: Some((x.cloned(), y.cloned())),
            });
        }
    }

    fn not_kept(&mut self) {
        let (a, b) = (self.base, self.cur);
        let mut ports: Vec<(u64, &(String, String), &String)> = b
            .listen
            .iter()
            .filter(|(k, _)| !a.listen.contains_key(*k))
            .map(|(k, v)| (k.1.parse().unwrap_or(0), k, v))
            .collect();
        ports.sort();
        self.d.not_kept.listening = ports
            .into_iter()
            .map(|(_, k, v)| Listener { proto: k.0.clone(), port: k.1.clone(), process: v.clone() })
            .collect();
        self.d.not_kept.processes = b.proc.difference(&a.proc).cloned().collect();
        self.d.not_kept.services = b.svc.keys().filter(|s| !a.svc.contains_key(*s)).cloned().collect();
    }

    fn pruned_trees(&mut self) {
        let (a, b) = (self.base, self.cur);
        let paths: BTreeSet<&String> = a.pdirs.keys().chain(b.pdirs.keys()).collect();
        for p in paths {
            let (x, y) = (a.pdirs.get(p), b.pdirs.get(p));
            if x == y {
                continue;
            }
            let delta = match (x, y) {
                (None, Some(n)) | (Some(n), None) => n.unwrap_or(0).max(1),
                (Some(Some(x)), Some(Some(y))) => x.abs_diff(*y),
                _ => 1,
            };
            self.d.ignored.pruned_trees.push(PrunedTree { path: p.clone(), delta });
        }
    }
}

/// Compares `base` (the fork-time state) with `cur`. `baseline` is the diff of a fork of
/// the same source left idle for the same time; whatever it contains is background noise
/// and is removed from the result.
pub fn diff(base: &Manifest, cur: &Manifest, baseline: Option<&Diff>, ignore: &IgnoreList) -> Diff {
    let home = cur.home().or_else(|| base.home()).unwrap_or("").trim_end_matches('/').to_owned();
    let mut d = Builder { base, cur, ctx: Ctx { home }, d: Diff::default() }.run(ignore);
    if let Some(bl) = baseline {
        d.subtract_baseline(bl);
    }
    d.finish();
    d
}

impl Diff {
    fn subtract_baseline(&mut self, bl: &Diff) {
        for rc in &mut self.code {
            let Some(b) = bl.code.iter().find(|b| b.repo == rc.repo) else { continue };
            if rc.new == b.new {
                rc.new = None;
            }
            if rc.removed && b.removed {
                rc.removed = false;
            }
            if rc.commits == b.commits {
                rc.commits = None;
            }
            if rc.branch == b.branch {
                rc.branch = None;
            }
            rc.dirty_new.retain(|p, c| b.dirty_new.get(p) != Some(c));
            subtract(&mut rc.dirty_gone, &b.dirty_gone);
            if rc.dirty_delta == b.dirty_delta {
                rc.dirty_delta = 0;
            }
            subtract(&mut rc.gitignored, &b.gitignored);
        }
        self.code.retain(|r| !r.is_empty());

        for pc in &mut self.packages {
            let Some(b) = bl.packages.iter().find(|b| b.manager == pc.manager) else { continue };
            pc.added.retain(|n, v| b.added.get(n) != Some(v));
            pc.removed.retain(|n, v| b.removed.get(n) != Some(v));
            pc.upgraded.retain(|n, v| b.upgraded.get(n) != Some(v));
            pc.manual.retain(|n| pc.added.contains_key(n));
            pc.requires.retain(|n, _| pc.added.contains_key(n));
            for (n, paths) in &mut pc.owned_files {
                if let Some(bp) = b.owned_files.get(n) {
                    paths.retain(|p| !bp.contains(p));
                }
            }
            pc.owned_files.retain(|_, v| !v.is_empty());
        }
        self.packages.retain(|p| !p.is_empty());

        subtract(&mut self.settings.files, &bl.settings.files);
        subtract(&mut self.settings.cron, &bl.settings.cron);
        subtract(&mut self.settings.services, &bl.settings.services);
        subtract(&mut self.settings.launch_agents, &bl.settings.launch_agents);
        subtract(&mut self.other, &bl.other);
        subtract(&mut self.repo_files, &bl.repo_files);

        for db in &mut self.database {
            if let Some(b) = bl.database.iter().find(|b| b.engine == db.engine && b.name == db.name) {
                if db.schema == b.schema {
                    db.schema = None;
                }
                subtract(&mut db.tables, &b.tables);
                if db.digest == b.digest {
                    db.digest = None;
                }
                if db.change == Change::Modified && b.change == Change::Modified {
                    // nothing else to compare
                } else if db.change == b.change {
                    db.schema = None;
                    db.tables.clear();
                    db.digest = None;
                    db.change = Change::Modified;
                }
            }
        }
        self.database.retain(|d| match d.engine.as_str() {
            "sqlite" => d.digest.is_some() || d.change != Change::Modified,
            _ => d.schema.is_some() || !d.tables.is_empty() || d.change != Change::Modified,
        });

        subtract(&mut self.not_kept.listening, &bl.not_kept.listening);
        subtract(&mut self.not_kept.processes, &bl.not_kept.processes);
        subtract(&mut self.not_kept.services, &bl.not_kept.services);
        for (g, paths) in &mut self.not_kept.credentials {
            if let Some(bp) = bl.not_kept.credentials.get(g) {
                paths.retain(|p| !bp.contains(p));
            }
        }
        self.not_kept.credentials.retain(|_, v| !v.is_empty());
        subtract(&mut self.ignored.paths, &bl.ignored.paths);
        for t in &mut self.ignored.pruned_trees {
            if let Some(b) = bl.ignored.pruned_trees.iter().find(|b| b.path == t.path) {
                t.delta = t.delta.saturating_sub(b.delta);
            }
        }
        self.ignored.pruned_trees.retain(|t| t.delta > 0);
    }

    /// Derives the dependency collapse and the text lines from the detail.
    fn finish(&mut self) {
        for pc in &mut self.packages {
            collapse(pc);
        }
        self.settings.files.sort();
        self.other.sort();
        self.summary = self.sections();
    }

    pub fn is_empty(&self) -> bool {
        self.summary.iter().all(|s| s.lines.is_empty()) && self.ignored.count() == 0
    }

    fn sections(&self) -> Vec<Section> {
        let ctx = Ctx { home: self.home.clone() };
        let mut out: Vec<Section> = Vec::new();
        let mut push = |bucket: &str, lines: Vec<String>| {
            if !lines.is_empty() {
                out.push(Section { bucket: bucket.to_owned(), lines });
            }
        };
        push("Code", self.code_lines(&ctx));
        push("Packages", self.package_lines());
        push("Settings & files", self.settings_lines(&ctx));
        push("Database changed", self.database_lines(&ctx));
        push("Other changed", rollup(&ctx, &self.other, |p| other_group(&ctx, p)));
        push("Not kept", self.not_kept_lines(&ctx));
        out
    }

    fn code_lines(&self, ctx: &Ctx) -> Vec<String> {
        let mut out = Vec::new();
        for rc in &self.code {
            let r = ctx.short(&rc.repo);
            if let Some((head, branch)) = &rc.new {
                out.push(format!("new repo {r} @ {} ({branch})", short7(head)));
            } else if rc.removed {
                out.push(format!("repo removed: {r}"));
            } else if let Some((x, y)) = &rc.commits {
                let extra = match &rc.branch {
                    Some((bx, by)) => format!(" ({bx}→{by})"),
                    None => String::new(),
                };
                out.push(format!("{r}: {}..{}{extra}", short7(x), short7(y)));
            } else if let Some((bx, by)) = &rc.branch {
                out.push(format!("{r}: branch {bx}→{by}"));
            }
            if !rc.dirty_new.is_empty() {
                let names: Vec<&str> = rc.dirty_new.keys().map(|p| base_name(p)).collect();
                let tail = if names.len() > 3 { format!(" +{} more", names.len() - 3) } else { String::new() };
                out.push(format!("{r}: {} dirty/untracked: {}{tail}", names.len(), names[..names.len().min(3)].join(", ")));
            }
            if !rc.dirty_gone.is_empty() {
                out.push(format!("{r}: {} dirty/untracked files now clean", rc.dirty_gone.len()));
            }
            if rc.dirty_delta > 0 {
                out.push(format!("{r}: {} more dirty/untracked files", rc.dirty_delta));
            }
            if !rc.gitignored.is_empty() {
                let names: Vec<&str> = rc.gitignored.iter().map(|c| rel_to(&rc.repo, &c.path)).collect();
                let tail = if names.len() > 3 { format!(" +{} more", names.len() - 3) } else { String::new() };
                out.push(format!("{r}: {}{tail} changed (gitignored)", names[..names.len().min(3)].join(", ")));
            }
        }
        out
    }

    fn package_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for pc in &self.packages {
            let mut parts: Vec<String> = Vec::new();
            if !pc.added.is_empty() {
                parts.push(added_text(pc));
            }
            if !pc.upgraded.is_empty() {
                let items = pc.upgraded.iter().map(|(n, (x, y))| format!("{n} {x}→{y}")).collect();
                parts.push(format!("upgraded {}", cap(items, 4, "more").join(", ")));
            }
            if !pc.removed.is_empty() {
                let items = pc.removed.keys().cloned().collect();
                parts.push(format!("removed {}", cap(items, 4, "more").join(", ")));
            }
            let files_only: Vec<String> = pc
                .owned_files
                .iter()
                .filter(|(n, _)| !pc.added.contains_key(*n) && !pc.upgraded.contains_key(*n))
                .map(|(n, p)| format!("{n} ({} file{})", p.len(), if p.len() == 1 { "" } else { "s" }))
                .collect();
            if !files_only.is_empty() {
                parts.push(format!("files changed in {}", cap(files_only, 4, "more").join(", ")));
            }
            if !parts.is_empty() {
                out.push(format!("{}: {}", pkg_label(&pc.manager), parts.join("; ")));
            }
        }
        out
    }

    fn settings_lines(&self, ctx: &Ctx) -> Vec<String> {
        let s = &self.settings;
        let mut lines = rollup(ctx, &s.files, |p| p.rsplit_once('/').map(|x| x.0).unwrap_or("").to_owned());
        for l in &s.launch_agents {
            let verb = match l.change {
                Change::Added => "added",
                Change::Deleted => "removed",
                Change::Modified => "changed",
            };
            lines.push(format!("launch agent {} ({}): {verb}", l.label, l.scope));
        }
        for c in &s.cron {
            let verb = match c.change {
                Change::Added => "added",
                Change::Deleted => "removed",
                Change::Modified => "changed",
            };
            lines.push(format!("cron {}: {verb}", c.user));
        }
        for v in &s.services {
            lines.push(format!("service {}: {}→{}", v.name, v.from, v.to));
        }
        lines
    }

    fn database_lines(&self, ctx: &Ctx) -> Vec<String> {
        let mut out = Vec::new();
        for d in &self.database {
            if d.engine == "sqlite" {
                let name = ctx.short(&d.name);
                let (x, y) = d.digest.clone().unwrap_or((None, None));
                let digest = |o: &Option<String>| o.as_deref().map(short7).unwrap_or_else(|| "-".into());
                out.push(match d.change {
                    Change::Added => format!("sqlite {name}: added ({})", digest(&y)),
                    Change::Deleted => format!("sqlite {name}: removed"),
                    Change::Modified => format!("sqlite {name}: changed ({}→{})", digest(&x), digest(&y)),
                });
                continue;
            }
            let head = format!("postgres {}", d.name);
            match d.change {
                Change::Added => {
                    let t = d.tables.len();
                    out.push(format!("{head}: new database ({t} table{})", if t == 1 { "" } else { "s" }));
                }
                Change::Deleted => out.push(format!("{head}: dropped")),
                Change::Modified => {
                    let mut parts = Vec::new();
                    if d.schema.is_some() {
                        parts.push("schema changed".to_owned());
                    }
                    if !d.tables.is_empty() {
                        let items = d.tables.iter().map(table_text).collect();
                        parts.push(format!("data changed in {}", cap(items, 3, "more tables").join(", ")));
                    }
                    out.push(format!("{head}: {}", parts.join("; ")));
                }
            }
        }
        out
    }

    fn not_kept_lines(&self, ctx: &Ctx) -> Vec<String> {
        let nk = &self.not_kept;
        let mut out = Vec::new();
        if !nk.listening.is_empty() {
            let items = nk
                .listening
                .iter()
                .map(|l| if l.process == "-" { format!(":{}", l.port) } else { format!(":{} ({})", l.port, l.process) })
                .collect();
            out.push(format!("listening: {}", cap(items, 6, "more").join(", ")));
        }
        if !nk.processes.is_empty() {
            out.push(format!("processes: {}", cap(nk.processes.clone(), 6, "more").join(", ")));
        }
        if !nk.services.is_empty() {
            out.push(format!("services: {}", cap(nk.services.clone(), 6, "more").join(", ")));
        }
        if !nk.credentials.is_empty() {
            let items = nk
                .credentials
                .iter()
                .map(|(g, paths)| {
                    if g.ends_with('/') {
                        let n = paths.len();
                        format!("{} ({n} file{})", ctx.short(g), if n == 1 { "" } else { "s" })
                    } else {
                        ctx.short(g)
                    }
                })
                .collect();
            out.push(format!("credentials: {}", cap(items, 6, "more").join(", ")));
        }
        out
    }

    /// Readable text, at most `max_lines` lines when the sections can be cut that far
    /// (every section keeps its header and one item). Cut items become "… +N more".
    pub fn render(&self, max_lines: usize) -> String {
        self.render_lines(max_lines).join("\n")
    }

    pub fn render_lines(&self, max_lines: usize) -> Vec<String> {
        let ignored = self.ignored.count();
        let mut secs: Vec<(String, Vec<String>, usize)> =
            self.summary.iter().filter(|s| !s.lines.is_empty()).map(|s| (s.bucket.clone(), s.lines.clone(), 0)).collect();
        let total = |secs: &Vec<(String, Vec<String>, usize)>| -> usize {
            secs.iter().map(|s| 1 + s.1.len() + usize::from(s.2 > 0)).sum::<usize>() + usize::from(ignored > 0)
        };
        while total(&secs) > max_lines {
            let Some(idx) = secs
                .iter()
                .enumerate()
                .filter(|(_, s)| s.1.len() >= if s.2 > 0 { 2 } else { 3 })
                .max_by_key(|(i, s)| (s.1.len(), std::cmp::Reverse(*i)))
                .map(|(i, _)| i)
            else {
                break;
            };
            let s = &mut secs[idx];
            let k = if s.2 > 0 { 1 } else { 2 }; // the first cut also pays for the "more" line
            let keep = s.1.len() - k;
            s.1.truncate(keep);
            s.2 += k;
        }
        let mut lines = Vec::new();
        for (title, items, hidden) in secs {
            lines.push(title);
            lines.extend(items.into_iter().map(|i| format!("  {i}")));
            if hidden > 0 {
                lines.push(format!("  … +{hidden} more (see --json)"));
            }
        }
        if ignored > 0 {
            lines.push(format!("Ignored: {ignored} paths (caches, logs, build output)"));
        }
        if lines.is_empty() {
            lines.push("No changes".to_owned());
        }
        lines
    }
}

fn table_text(t: &TableChange) -> String {
    let n = |o: Option<u64>| o.map(|v| v.to_string()).unwrap_or_else(|| "?".into());
    match t.change {
        Change::Added => format!("{} (new, {} rows)", t.table, n(t.rows_after)),
        Change::Deleted => format!("{} (dropped)", t.table),
        Change::Modified => format!("{} ({}→{} rows)", t.table, n(t.rows_before), n(t.rows_after)),
    }
}

/// Decides which added packages are shown by name and which fold into them.
fn collapse(pc: &mut PackageChange) {
    pc.top_level = None;
    pc.dependencies = Vec::new();
    if pc.added.is_empty() {
        return;
    }
    let added: BTreeSet<&String> = pc.added.keys().collect();
    let has_deps = !pc.requires.is_empty();
    let top: Vec<String> = if !pc.manual.is_empty() {
        pc.manual.clone()
    } else if has_deps {
        // no manual marker (pip): packages no other added package requires
        let required: BTreeSet<&String> = pc.requires.values().flatten().collect();
        pc.added.keys().filter(|n| !required.contains(n)).cloned().collect()
    } else {
        return;
    };
    if top.is_empty() {
        return;
    }
    let tops: BTreeSet<&String> = top.iter().collect();
    pc.dependencies = added.iter().filter(|n| !tops.contains(**n)).map(|n| (*n).clone()).collect();
    pc.top_level = Some(top);
}

fn deps_of(pc: &PackageChange, name: &str) -> usize {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut stack = vec![name];
    while let Some(n) = stack.pop() {
        for d in pc.requires.get(n).into_iter().flatten() {
            if pc.dependencies.contains(d) && seen.insert(d) {
                stack.push(d);
            }
        }
    }
    seen.len()
}

fn added_text(pc: &PackageChange) -> String {
    match &pc.top_level {
        Some(top) => {
            let per_parent = !pc.requires.is_empty();
            let items: Vec<String> = top
                .iter()
                .map(|n| {
                    let k = if per_parent { deps_of(pc, n) } else { 0 };
                    if k > 0 {
                        format!("{n} (+{k} dependencies)")
                    } else {
                        n.clone()
                    }
                })
                .collect();
            let mut s = cap(items, CAP_DEFAULT, "more").join(", ");
            if !per_parent && !pc.dependencies.is_empty() {
                s.push_str(&format!(" (+{} dependencies)", pc.dependencies.len()));
            }
            s
        }
        None => {
            let n = if pc.manager == "pip" { CAP_PIP } else { CAP_DEFAULT };
            let names: Vec<&String> = pc.added.keys().collect();
            let mut s = names.iter().take(n).map(|x| x.as_str()).collect::<Vec<_>>().join(", ");
            if names.len() > n {
                s.push_str(&format!(" (+{} more)", names.len() - n));
            }
            s
        }
    }
}
