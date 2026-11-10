//! Bucketed comparison of two manifests. See `docs/SiloUI-CHANGES.md`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::ignore::{credential_group, credential_group_owned, IgnoreList};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

/// What a path was (or became) in a manifest: enough to tell two different contents apart.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Identity {
    /// `file` or `link`.
    pub kind: String,
    pub size: String,
    /// sha256 of a file; `None` when it was too large to hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Link target of a symlink.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Never set: a timestamp is not part of what a file is, so it stays out of the JSON and
    /// out of the review digest. Kept so older stored documents still read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime: Option<String>,
}

impl Identity {
    fn of(e: &FileEntry) -> Identity {
        match e.kind {
            FileKind::Link => Identity {
                kind: "link".into(),
                size: e.size.clone(),
                hash: None,
                target: Some(e.digest.clone()),
                mtime: None,
            },
            FileKind::File => {
                let hashed = e.digest != "-";
                Identity {
                    kind: "file".into(),
                    size: e.size.clone(),
                    hash: hashed.then(|| e.digest.clone()),
                    target: None,
                    mtime: None,
                }
            }
        }
    }
}

/// A changed path with its content before and after (`None` on the side where it does not
/// exist).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PathChange {
    /// The item id of a file a keep can bring over; empty on entries that are not items.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub path: String,
    pub change: Change,
    #[serde(default)]
    pub before: Option<Identity>,
    #[serde(default)]
    pub after: Option<Identity>,
}

/// Code (git) changes for one repo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoChange {
    /// The item id of the repository.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub repo: String,
    /// Set when the repo did not exist in the base: (head, branch).
    pub new: Option<(String, String)>,
    pub removed: bool,
    /// Set when the repo no longer exists: (head, branch) it had in the base.
    #[serde(default)]
    pub removed_from: Option<(String, String)>,
    /// (old head, new head)
    pub commits: Option<(String, String)>,
    /// (old branch, new branch)
    pub branch: Option<(String, String)>,
    /// Dirty or untracked files that are new or changed: repo-relative path -> status code.
    pub dirty_new: BTreeMap<String, String>,
    pub dirty_gone: Vec<String>,
    /// Content of the files in `dirty_new` as they are now (when the manifest has them).
    #[serde(default)]
    pub dirty_identity: BTreeMap<String, Identity>,
    /// Renamed files in `dirty_new`: new path -> old path.
    #[serde(default)]
    pub renamed_from: BTreeMap<String, String>,
    /// The item id of each uncommitted file (and of each renamed file's old path), by
    /// repo-relative path.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dirty_ids: BTreeMap<String, String>,
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
    /// The item id of each package a keep acts on: the top-level added packages, and the
    /// upgraded and removed ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,
    pub added: BTreeMap<String, String>,
    pub removed: BTreeMap<String, String>,
    pub upgraded: BTreeMap<String, (String, String)>,
    /// Added packages the manager marks as manually installed (empty when it has no marker).
    pub manual: Vec<String>,
    /// Direct requirements of the added packages, when the manifest has dependency data.
    pub requires: BTreeMap<String, Vec<String>>,
    /// Files changed by a package, attributed through `OWN` records: package -> paths.
    pub owned_files: BTreeMap<String, Vec<String>>,
    /// The same files with the operation (added, modified, deleted) and content.
    #[serde(default)]
    pub owned_changes: BTreeMap<String, Vec<PathChange>>,
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
    /// The item id of the crontab.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub user: String,
    pub change: Change,
    /// The crontab file, when the manifest names it.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub before: Option<String>,
    #[serde(default)]
    pub after: Option<String>,
    /// Lines in the crontab now that it did not have, and lines it lost, when both texts are
    /// known. A crontab the quiet step set aside is compared with its parked original.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines_added: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines_removed: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceChange {
    pub name: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchAgentChange {
    /// The item id of the launch agent or daemon.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub scope: String,
    pub label: String,
    pub change: Change,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub before: Option<String>,
    #[serde(default)]
    pub after: Option<String>,
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
    /// Data digests before and after.
    #[serde(default)]
    pub digest_before: Option<String>,
    #[serde(default)]
    pub digest_after: Option<String>,
}

/// One changed database: a Postgres database or a SQLite file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbChange {
    /// The item id of the database.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
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
    /// Set when one side of the comparison could not be read (a database that was not running
    /// when the fork-time base was captured): the database is listed, not called changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_compared: Option<String>,
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
    /// The changes behind `credentials`, with content: credential group -> changes.
    #[serde(default)]
    pub credential_details: BTreeMap<String, Vec<PathChange>>,
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
        self.pruned_trees
            .iter()
            .fold(self.paths.len() as u64, |n, t| n.saturating_add(t.delta))
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
    /// Files that change when the computer sits idle but ended with other content here: not
    /// removed as background, and not listed in a bucket; the text shows one collapsed line.
    #[serde(default)]
    pub idle_noise: Vec<PathChange>,
    pub not_kept: NotKept,
    /// Changed files that live in git repos; reported through Code only.
    pub repo_files: Vec<String>,
    /// The same files with the change and content.
    #[serde(default)]
    pub repo_changes: Vec<PathChange>,
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

fn file_changes(
    a: &BTreeMap<String, FileEntry>,
    b: &BTreeMap<String, FileEntry>,
) -> Vec<PathChange> {
    let mut out = Vec::new();
    let paths: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    for p in paths {
        let (x, y) = (a.get(p), b.get(p));
        let change = match (x, y) {
            (None, Some(_)) => Change::Added,
            (Some(_), None) => Change::Deleted,
            (Some(x), Some(y)) if !same_entry(x, y) => Change::Modified,
            _ => continue,
        };
        out.push(PathChange {
            id: String::new(),
            path: p.clone(),
            change,
            before: x.map(Identity::of),
            after: y.map(Identity::of),
        });
    }
    out
}

fn gig_changes(
    a: &BTreeMap<String, GigEntry>,
    b: &BTreeMap<String, GigEntry>,
) -> Vec<(String, PathChange)> {
    let as_entry = |g: &GigEntry| FileEntry {
        kind: FileKind::File,
        size: g.size.clone(),
        mtime: g.mtime.clone(),
        digest: g.digest.clone(),
    };
    let mut out = Vec::new();
    let paths: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    for p in paths {
        let (x, y) = (a.get(p), b.get(p));
        let (change, repo) = match (x, y) {
            (None, Some(y)) => (Change::Added, &y.repo),
            (Some(x), None) => (Change::Deleted, &x.repo),
            (Some(x), Some(y)) if !same_entry(&as_entry(x), &as_entry(y)) => {
                (Change::Modified, &y.repo)
            }
            _ => continue,
        };
        let pc = PathChange {
            id: String::new(),
            path: p.clone(),
            change,
            before: x.map(|g| Identity::of(&as_entry(g))),
            after: y.map(|g| Identity::of(&as_entry(g))),
        };
        out.push((repo.clone(), pc));
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
        let base = if !ctx.home.is_empty() && h == ctx.home {
            "~".to_owned()
        } else {
            h
        };
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

/// One line per changed file, then one line per directory with more than three changed
/// files. Lines for single files come first so a cut at the end drops the rollups first.
fn rollup(ctx: &Ctx, chs: &[PathChange], key: impl Fn(&str) -> String) -> Vec<String> {
    let (mut singles, rolled) = rollup_parts(ctx, chs, key);
    singles.extend(rolled);
    singles
}

fn rollup_parts(
    ctx: &Ctx,
    chs: &[PathChange],
    key: impl Fn(&str) -> String,
) -> (Vec<String>, Vec<String>) {
    let mut groups: BTreeMap<String, Vec<&PathChange>> = BTreeMap::new();
    for c in chs {
        groups.entry(key(&c.path)).or_default().push(c);
    }
    let (mut singles, mut rolled) = (Vec::new(), Vec::new());
    for (k, g) in groups {
        if g.len() > 3 {
            rolled.push(format!(
                "{}/ ({} files: {})",
                ctx.short(&k),
                g.len(),
                counts(&g)
            ));
        } else {
            for c in g {
                singles.push(format!("{} {}", c.change.sym(), ctx.short(&c.path)));
            }
        }
    }
    (singles, rolled)
}

fn cap(mut items: Vec<String>, n: usize, noun: &str) -> Vec<String> {
    if items.len() > n {
        let rest = items.len() - n;
        items.truncate(n);
        items.push(format!("+{rest} {noun}"));
    }
    items
}

/// Non-blank lines `after` has beyond `before`, and lines it lost (repeats counted).
fn line_delta(before: &str, after: &str) -> (u32, u32) {
    let mut counts: BTreeMap<&str, i64> = BTreeMap::new();
    for line in before.lines().map(str::trim_end).filter(|l| !l.is_empty()) {
        *counts.entry(line).or_default() -= 1;
    }
    for line in after.lines().map(str::trim_end).filter(|l| !l.is_empty()) {
        *counts.entry(line).or_default() += 1;
    }
    let added: i64 = counts.values().filter(|n| **n > 0).sum();
    let removed: i64 = counts.values().filter(|n| **n < 0).map(|n| -n).sum();
    (
        u32::try_from(added).unwrap_or(u32::MAX),
        u32::try_from(removed).unwrap_or(u32::MAX),
    )
}

fn lines_text(n: u32) -> String {
    format!("{n} line{}", if n == 1 { "" } else { "s" })
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

/// Removes from `items` everything whose key also appears among `base`'s keys.
fn subtract_by<T, K: std::hash::Hash + Eq>(items: &mut Vec<T>, base: &[T], key: impl Fn(&T) -> K) {
    if base.is_empty() {
        return;
    }
    let seen: HashSet<K> = base.iter().map(&key).collect();
    items.retain(|i| !seen.contains(&key(i)));
}

fn subtract<T: std::hash::Hash + Eq>(items: &mut Vec<T>, base: &[T]) {
    let seen: HashSet<&T> = base.iter().collect();
    items.retain(|i| !seen.contains(i));
}

/// Baseline identity of a path change: the path, the kind of change and the resulting
/// content.
fn subtract_paths(items: &mut Vec<PathChange>, base: &[PathChange]) {
    subtract_by(items, base, |c| (c.path.clone(), c.change, c.after.clone()));
}

/// Unquotes one git C-style quoted name (`"a\303\251"`). Returns the name and the rest.
fn git_unquote(s: &str) -> Option<(String, &str)> {
    let rest = s.strip_prefix('"')?;
    let bytes = rest.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Some((String::from_utf8_lossy(&out).into_owned(), &rest[i + 1..])),
            b'\\' if i + 1 < bytes.len() => {
                i += 1;
                match bytes[i] {
                    b'a' => out.push(7),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'v' => out.push(11),
                    d @ b'0'..=b'7' => {
                        let mut v = u32::from(d - b'0');
                        let mut n = 1;
                        while n < 3 && i + 1 < bytes.len() && (b'0'..=b'7').contains(&bytes[i + 1])
                        {
                            i += 1;
                            v = v * 8 + u32::from(bytes[i] - b'0');
                            n += 1;
                        }
                        out.push(v as u8);
                    }
                    other => out.push(other),
                }
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    None
}

fn git_name(s: &str) -> (String, &str) {
    match git_unquote(s) {
        Some(x) => x,
        None => (s.to_owned(), ""),
    }
}

/// A `git status --porcelain` path field: (new path, old path for renames and copies).
/// The `old -> new` arrow is read only for rename or copy status codes (`R`, `C`); other
/// records are a single, possibly quoted, path.
fn porcelain_path(code: &str, raw: &str) -> (String, Option<String>) {
    if code.contains(['R', 'C']) {
        let (old, rest) = git_name(raw);
        if let Some(new) = rest.strip_prefix(" -> ") {
            return (git_name(new).0, Some(old));
        }
        if rest.is_empty() {
            if let Some((o, n)) = raw.split_once(" -> ") {
                return (git_name(n).0, Some(git_name(o).0));
            }
        }
    }
    (git_name(raw).0, None)
}

fn rel_to<'a>(repo: &str, path: &'a str) -> &'a str {
    path.strip_prefix(repo)
        .map(|r| r.trim_start_matches('/'))
        .unwrap_or(path)
}

struct Builder<'a> {
    base: &'a Manifest,
    cur: &'a Manifest,
    ctx: Ctx,
    d: Diff,
}

/// The deepest repo in `repos` that contains `p`.
fn repo_of<'r>(repos: &HashSet<&'r str>, p: &str) -> Option<&'r str> {
    let mut end = p.len();
    while let Some(i) = p[..end].rfind('/') {
        if let Some(r) = repos.get(&p[..i]) {
            return Some(*r);
        }
        end = i;
    }
    None
}

fn db_identity(digest: &str) -> Identity {
    Identity {
        kind: "file".into(),
        size: String::new(),
        hash: Some(digest.to_owned()),
        target: None,
        mtime: None,
    }
}

impl<'a> Builder<'a> {
    fn run(mut self, ignore: &IgnoreList) -> Diff {
        self.d.format = DIFF_FORMAT;
        self.d.home = self.ctx.home.clone();
        let repos = self.classify_files(ignore);
        self.packages();
        self.code(repos);
        self.settings_extras();
        self.database(ignore);
        self.not_kept();
        self.pruned_trees();
        self.d.ignored.paths.sort();
        self.d.ignored.paths.dedup();
        self.d
    }

    fn credential(&mut self, group: String, change: PathChange) {
        self.d
            .not_kept
            .credential_details
            .entry(group)
            .or_default()
            .push(change);
    }

    /// Splits the changed files into buckets. Returns the gitignored changes per repo and
    /// leaves credential routing of those to `code`.
    fn classify_files(&mut self, ignore: &IgnoreList) -> BTreeMap<String, Vec<PathChange>> {
        let (a, b) = (self.base, self.cur);
        let repos: HashSet<&str> = a
            .git
            .keys()
            .chain(b.git.keys())
            .map(String::as_str)
            .collect();
        let sqlite: HashSet<&str> =
            a.db.sqlite
                .keys()
                .chain(b.db.sqlite.keys())
                .map(String::as_str)
                .collect();
        let cronf: HashSet<&str> = a
            .cronf
            .values()
            .chain(b.cronf.values())
            .map(String::as_str)
            .collect();
        let lagent: HashSet<&str> = a
            .lagent
            .values()
            .chain(b.lagent.values())
            .map(|l| l.path.as_str())
            .collect();
        let owner = |p: &str| b.own.get(p).or_else(|| a.own.get(p)).cloned();

        let mut owned: BTreeMap<(String, String), Vec<PathChange>> = BTreeMap::new();
        for c in file_changes(&a.files, &b.files) {
            let p = c.path.clone();
            let own = owner(&p);
            if ignore.matches(&p) {
                self.d.ignored.paths.push(p);
            } else if let Some(g) = credential_group_owned(&p, own.is_some()) {
                self.credential(g, c);
            } else if repo_of(&repos, &p).is_some() {
                self.d.repo_changes.push(c);
            } else if sqlite.contains(p.as_str())
                || cronf.contains(p.as_str())
                || lagent.contains(p.as_str())
            {
                // reported as a database, crontab or launch agent change
            } else if let Some(o) = own {
                owned.entry(o).or_default().push(c);
            } else if is_settings(&self.ctx, &p) {
                self.d.settings.files.push(c);
            } else {
                self.d.other.push(c);
            }
        }
        for ((mgr, name), changes) in owned {
            let pc = self.package_entry(&mgr);
            pc.owned_files.insert(
                name.clone(),
                changes.iter().map(|c| c.path.clone()).collect(),
            );
            pc.owned_changes.insert(name, changes);
        }

        let mut gigs: BTreeMap<String, Vec<PathChange>> = BTreeMap::new();
        for (repo, c) in gig_changes(&a.gig, &b.gig) {
            let own = owner(&c.path).is_some();
            if ignore.matches(&c.path) {
                self.d.ignored.paths.push(c.path);
            } else if let Some(g) = credential_group_owned(&c.path, own) {
                self.credential(g, c);
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
        self.d.packages.push(PackageChange {
            manager: mgr.to_owned(),
            ..Default::default()
        });
        self.d.packages.last_mut().expect("just pushed")
    }

    fn packages(&mut self) {
        let (a, b) = (self.base, self.cur);
        let group = |m: &'a Manifest| {
            let mut out: BTreeMap<&'a String, BTreeMap<&'a String, &'a String>> = BTreeMap::new();
            for (k, v) in &m.pkg {
                out.entry(&k.0).or_default().insert(&k.1, v);
            }
            out
        };
        let (ga, gb) = (group(a), group(b));
        let empty = BTreeMap::new();
        let mgrs: BTreeSet<&&String> = ga.keys().chain(gb.keys()).collect();
        for mgr in mgrs {
            let mgr: &String = mgr;
            let pa = ga.get(mgr).unwrap_or(&empty);
            let pb = gb.get(mgr).unwrap_or(&empty);
            let mut pc = PackageChange {
                manager: mgr.clone(),
                ..Default::default()
            };
            for (n, v) in pb {
                match pa.get(n) {
                    None => {
                        pc.added.insert((*n).clone(), (*v).clone());
                    }
                    Some(old) if old != v => {
                        pc.upgraded
                            .insert((*n).clone(), ((*old).clone(), (*v).clone()));
                    }
                    _ => {}
                }
            }
            for (n, v) in pa {
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
                let changes = std::mem::take(&mut existing.owned_changes);
                *existing = pc;
                existing.owned_files = owned;
                existing.owned_changes = changes;
            } else {
                self.d.packages.push(pc);
            }
        }
        self.d.packages.sort_by(|x, y| x.manager.cmp(&y.manager));
    }

    fn code(&mut self, mut gigs: BTreeMap<String, Vec<PathChange>>) {
        let (a, b) = (self.base, self.cur);
        let repos: BTreeSet<String> = a
            .git
            .keys()
            .chain(b.git.keys())
            .chain(gigs.keys())
            .cloned()
            .collect();
        let repo_set: HashSet<&str> = a
            .git
            .keys()
            .chain(b.git.keys())
            .map(String::as_str)
            .collect();
        // Changed files under each repo (their content changed since the base).
        let mut changed: HashMap<&str, HashSet<&str>> = HashMap::new();
        for c in &self.d.repo_changes {
            if let Some(r) = repo_of(&repo_set, &c.path) {
                changed.entry(r).or_default().insert(&c.path);
            }
        }
        let normalize =
            |m: Option<&BTreeMap<String, String>>| -> BTreeMap<String, (String, Option<String>)> {
                let mut out = BTreeMap::new();
                for (raw, code) in m.into_iter().flatten() {
                    let (new, old) = porcelain_path(code, raw);
                    out.insert(new, (code.clone(), old));
                }
                out
            };
        let mut creds: Vec<(String, PathChange)> = Vec::new();
        let mut found: Vec<RepoChange> = Vec::new();
        for r in &repos {
            let mut rc = RepoChange {
                repo: r.clone(),
                ..Default::default()
            };
            let (x, y) = (a.git.get(r), b.git.get(r));
            match (x, y) {
                (None, Some(y)) => rc.new = Some((y.head.clone(), y.branch.clone())),
                (Some(x), None) => {
                    rc.removed = true;
                    rc.removed_from = Some((x.head.clone(), x.branch.clone()));
                }
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
                let fa = if x.is_some() {
                    normalize(a.gitf.get(r))
                } else {
                    BTreeMap::new()
                };
                let fb = normalize(b.gitf.get(r));
                let touched = changed.get(r.as_str());
                let abs_of = |rel: &str| format!("{}/{}", r.trim_end_matches('/'), rel);
                for (p, (code, old)) in &fb {
                    let abs = abs_of(p);
                    let content_changed = touched.is_some_and(|t| t.contains(abs.as_str()));
                    if fa.get(p).map(|x| &x.0) == Some(code) && !content_changed {
                        continue;
                    }
                    let after = b.files.get(&abs).map(Identity::of);
                    let cred = credential_group(&abs)
                        .map(|g| (g, abs.clone()))
                        .or_else(|| {
                            old.as_ref()
                                .and_then(|o| credential_group(&abs_of(o)).map(|g| (g, abs_of(o))))
                        });
                    if let Some((g, path)) = cred {
                        let change = if code.starts_with('?') {
                            Change::Added
                        } else {
                            Change::Modified
                        };
                        creds.push((
                            g,
                            PathChange {
                                id: String::new(),
                                path,
                                change,
                                before: None,
                                after,
                            },
                        ));
                        continue;
                    }
                    rc.dirty_new.insert(p.clone(), code.clone());
                    if let Some(i) = after {
                        rc.dirty_identity.insert(p.clone(), i);
                    }
                    if let Some(o) = old {
                        rc.renamed_from.insert(p.clone(), o.clone());
                    }
                }
                for p in fa.keys() {
                    if !fb.contains_key(p) {
                        rc.dirty_gone.push(p.clone());
                    }
                }
                if let Some(x) = x {
                    if rc.dirty_new.is_empty() && rc.dirty_gone.is_empty() {
                        let total = |g: &super::manifest::GitRepo| {
                            i128::from(g.dirty) + i128::from(g.untracked)
                        };
                        let dd = total(y) - total(x);
                        if dd > 0 {
                            rc.dirty_delta = i64::try_from(dd).unwrap_or(i64::MAX);
                        }
                    }
                }
            }
            if let Some(g) = gigs.remove(r) {
                rc.gitignored = g;
            }
            if !rc.is_empty() {
                found.push(rc);
            }
        }
        self.d.code.extend(found);
        for (g, c) in creds {
            self.credential(g, c);
        }
    }

    fn settings_extras(&mut self) {
        let (a, b) = (self.base, self.cur);
        let users: BTreeSet<&String> = a
            .cron
            .keys()
            .chain(b.cron.keys())
            .chain(a.qcron.keys())
            .collect();
        for u in users {
            let (mut x, y) = (
                a.cron.get(u).map(String::as_str).unwrap_or("-"),
                b.cron.get(u).map(String::as_str).unwrap_or("-"),
            );
            // A crontab that could not be read says nothing either way.
            if x == "?" || y == "?" {
                continue;
            }
            let mut before_text = a.cront.get(u).map(String::as_str);
            // The quiet step parked main's crontab before the base was taken, so the base shows
            // none. What the fork started from is the parked original.
            if x == "-" {
                if let Some((hash, text)) = a.qcron.get(u) {
                    // Parked and still absent: the quiet step, not a change.
                    if y == "-" {
                        continue;
                    }
                    x = hash;
                    before_text = Some(text);
                }
            }
            if x == y {
                continue;
            }
            let change = if x == "-" {
                Change::Added
            } else if y == "-" {
                Change::Deleted
            } else {
                Change::Modified
            };
            let side = |v: &str| (v != "-").then(|| v.to_owned());
            let path = b.cronf.get(u).or_else(|| a.cronf.get(u)).cloned();
            let (lines_added, lines_removed) = match (change, before_text, b.cront.get(u)) {
                (Change::Modified, Some(before), Some(after)) => {
                    let (added, removed) = line_delta(before, after);
                    (Some(added), Some(removed))
                }
                _ => (None, None),
            };
            self.d.settings.cron.push(CronChange {
                id: String::new(),
                user: u.clone(),
                change,
                path,
                before: side(x),
                after: side(y),
                lines_added,
                lines_removed,
            });
        }
        for (s, from) in &a.svc {
            if let Some(to) = b.svc.get(s) {
                if from != to {
                    self.d.settings.services.push(ServiceChange {
                        name: s.clone(),
                        from: from.clone(),
                        to: to.clone(),
                    });
                }
            }
        }
        let keys: BTreeSet<&(String, String)> = a.lagent.keys().chain(b.lagent.keys()).collect();
        for k in keys {
            let (x, y) = (a.lagent.get(k), b.lagent.get(k));
            let change = match (x, y) {
                (None, Some(_)) => Change::Added,
                (Some(_), None) => Change::Deleted,
                (Some(x), Some(y)) if x.digest != y.digest => Change::Modified,
                _ => continue,
            };
            self.d.settings.launch_agents.push(LaunchAgentChange {
                id: String::new(),
                scope: k.0.clone(),
                label: k.1.clone(),
                change,
                path: y.or(x).map(|l| l.path.clone()),
                before: x.map(|l| l.digest.clone()),
                after: y.map(|l| l.digest.clone()),
            });
        }
    }

    fn database(&mut self, ignore: &IgnoreList) {
        let (a, b) = (&self.base.db, &self.cur.db);
        let mut tabs: BTreeMap<&String, BTreeSet<&String>> = BTreeMap::new();
        for k in a.pg_tables.keys().chain(b.pg_tables.keys()) {
            tabs.entry(&k.0).or_default().insert(&k.1);
        }
        let tabs_a: HashSet<&String> = a.pg_tables.keys().map(|k| &k.0).collect();
        let tabs_b: HashSet<&String> = b.pg_tables.keys().map(|k| &k.0).collect();
        let dbs: BTreeSet<&String> = a
            .pg_schema
            .keys()
            .chain(b.pg_schema.keys())
            .chain(tabs.keys().copied())
            .collect();
        for db in dbs {
            let (sa, sb) = (a.pg_schema.get(db), b.pg_schema.get(db));
            let mut tcs = Vec::new();
            for t in tabs.get(db).into_iter().flatten() {
                let key = (db.clone(), (*t).clone());
                let (ta, tb) = (a.pg_tables.get(&key), b.pg_tables.get(&key));
                let change = match (ta, tb) {
                    (None, Some(_)) => Change::Added,
                    (Some(_), None) => Change::Deleted,
                    (Some(x), Some(y)) if x.digest != y.digest => Change::Modified,
                    _ => continue,
                };
                tcs.push(TableChange {
                    table: (*t).clone(),
                    change,
                    rows_before: ta.and_then(|t| t.rows),
                    rows_after: tb.and_then(|t| t.rows),
                    digest_before: ta.map(|t| t.digest.clone()),
                    digest_after: tb.map(|t| t.digest.clone()),
                });
            }
            let in_a = sa.is_some() || tabs_a.contains(db);
            let in_b = sb.is_some() || tabs_b.contains(db);
            let change = match (in_a, in_b) {
                (false, true) => Change::Added,
                (true, false) => Change::Deleted,
                _ => Change::Modified,
            };
            let schema = if sa != sb {
                Some((sa.cloned(), sb.cloned()))
            } else {
                None
            };
            if change == Change::Modified && schema.is_none() && tcs.is_empty() {
                continue;
            }
            // A server that was not readable on one side has no databases there: what the
            // other side lists is not a creation or a drop.
            let not_compared = match (change, &a.pg_unreadable, &b.pg_unreadable) {
                (Change::Added, Some(why), _) => Some(format!("base unreadable ({why})")),
                (Change::Deleted, _, Some(why)) => Some(format!("now unreadable ({why})")),
                _ => None,
            };
            self.d.database.push(DbChange {
                id: String::new(),
                engine: "postgres".into(),
                name: db.clone(),
                change,
                schema,
                tables: tcs,
                digest: None,
                not_compared,
            });
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
            if ignore.matches(f) {
                self.d.ignored.paths.push(f.clone());
                continue;
            }
            let owned = self.cur.own.contains_key(f) || self.base.own.contains_key(f);
            if let Some(g) = credential_group_owned(f, owned) {
                let pc = PathChange {
                    id: String::new(),
                    path: f.clone(),
                    change,
                    before: x.map(|d| db_identity(d)),
                    after: y.map(|d| db_identity(d)),
                };
                self.credential(g, pc);
                continue;
            }
            self.d.database.push(DbChange {
                id: String::new(),
                engine: "sqlite".into(),
                name: f.clone(),
                change,
                schema: None,
                tables: Vec::new(),
                digest: Some((x.cloned(), y.cloned())),
                not_compared: None,
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
            .map(|(_, k, v)| Listener {
                proto: k.0.clone(),
                port: k.1.clone(),
                process: v.clone(),
            })
            .collect();
        self.d.not_kept.processes = b.proc.difference(&a.proc).cloned().collect();
        self.d.not_kept.services = b
            .svc
            .keys()
            .filter(|s| !a.svc.contains_key(*s))
            .cloned()
            .collect();
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
            self.d.ignored.pruned_trees.push(PrunedTree {
                path: p.clone(),
                delta,
            });
        }
    }
}

/// Compares `base` (the fork-time state) with `cur`. `baseline` is the diff of a fork of
/// the same source left idle for the same time; whatever it contains is background noise
/// and is removed from the result.
pub fn diff(base: &Manifest, cur: &Manifest, baseline: Option<&Diff>, ignore: &IgnoreList) -> Diff {
    let home = cur
        .home()
        .or_else(|| base.home())
        .unwrap_or("")
        .trim_end_matches('/')
        .to_owned();
    let mut d = Builder {
        base,
        cur,
        ctx: Ctx { home },
        d: Diff::default(),
    }
    .run(ignore);
    if let Some(bl) = baseline {
        d.subtract_baseline(bl);
    }
    d.finish();
    d
}

impl Diff {
    fn subtract_baseline(&mut self, bl: &Diff) {
        let bcode: HashMap<&str, &RepoChange> =
            bl.code.iter().map(|b| (b.repo.as_str(), b)).collect();
        for rc in &mut self.code {
            let Some(b) = bcode.get(rc.repo.as_str()) else {
                continue;
            };
            if rc.new == b.new {
                rc.new = None;
            }
            if rc.removed && b.removed && rc.removed_from == b.removed_from {
                rc.removed = false;
                rc.removed_from = None;
            }
            if rc.commits == b.commits {
                rc.commits = None;
            }
            if rc.branch == b.branch {
                rc.branch = None;
            }
            let ident = &rc.dirty_identity;
            rc.dirty_new.retain(|p, c| {
                !(b.dirty_new.get(p) == Some(c) && b.dirty_identity.get(p) == ident.get(p))
            });
            let kept = &rc.dirty_new;
            rc.dirty_identity.retain(|p, _| kept.contains_key(p));
            rc.renamed_from.retain(|p, _| kept.contains_key(p));
            subtract(&mut rc.dirty_gone, &b.dirty_gone);
            if rc.dirty_delta == b.dirty_delta {
                rc.dirty_delta = 0;
            }
            subtract_paths(&mut rc.gitignored, &b.gitignored);
        }
        self.code.retain(|r| !r.is_empty());

        let bpk: HashMap<&str, &PackageChange> = bl
            .packages
            .iter()
            .map(|b| (b.manager.as_str(), b))
            .collect();
        for pc in &mut self.packages {
            let Some(b) = bpk.get(pc.manager.as_str()) else {
                continue;
            };
            pc.added.retain(|n, v| b.added.get(n) != Some(v));
            pc.removed.retain(|n, v| b.removed.get(n) != Some(v));
            pc.upgraded.retain(|n, v| b.upgraded.get(n) != Some(v));
            pc.manual.retain(|n| pc.added.contains_key(n));
            pc.requires.retain(|n, _| pc.added.contains_key(n));
            for (n, changes) in &mut pc.owned_changes {
                if let Some(bp) = b.owned_changes.get(n) {
                    subtract_paths(changes, bp);
                }
            }
        }
        for pc in &mut self.packages {
            pc.owned_changes.retain(|_, v| !v.is_empty());
            pc.owned_files = pc
                .owned_changes
                .iter()
                .map(|(n, v)| (n.clone(), v.iter().map(|c| c.path.clone()).collect()))
                .collect();
        }
        self.packages.retain(|p| !p.is_empty());

        subtract_paths(&mut self.settings.files, &bl.settings.files);
        subtract_by(&mut self.settings.cron, &bl.settings.cron, |c| {
            (c.user.clone(), c.change, c.after.clone(), c.path.clone())
        });
        subtract_by(&mut self.settings.services, &bl.settings.services, |s| {
            (s.name.clone(), s.from.clone(), s.to.clone())
        });
        subtract_by(
            &mut self.settings.launch_agents,
            &bl.settings.launch_agents,
            |l| {
                (
                    l.scope.clone(),
                    l.label.clone(),
                    l.change,
                    l.after.clone(),
                    l.path.clone(),
                )
            },
        );
        subtract_paths(&mut self.other, &bl.other);
        // A file outside Code, Packages and Database that the idle computer also touched
        // (whatever it holds now) is background churn: listed by path, not in a bucket.
        let idle: HashSet<&str> = bl
            .settings
            .files
            .iter()
            .chain(&bl.other)
            .map(|c| c.path.as_str())
            .collect();
        let mut noisy = Vec::new();
        for list in [&mut self.settings.files, &mut self.other] {
            let (n, rest): (Vec<PathChange>, Vec<PathChange>) = std::mem::take(list)
                .into_iter()
                .partition(|c| idle.contains(c.path.as_str()));
            *list = rest;
            noisy.extend(n);
        }
        self.idle_noise = noisy;
        subtract_paths(&mut self.repo_changes, &bl.repo_changes);

        let bdb: HashMap<(&str, &str), &DbChange> = bl
            .database
            .iter()
            .map(|b| ((b.engine.as_str(), b.name.as_str()), b))
            .collect();
        let mut drop = vec![false; self.database.len()];
        for (i, db) in self.database.iter_mut().enumerate() {
            let Some(b) = bdb.get(&(db.engine.as_str(), db.name.as_str())) else {
                continue;
            };
            match db.change {
                Change::Added | Change::Deleted => {
                    let same = DbChange {
                        id: db.id.clone(),
                        ..(**b).clone()
                    };
                    drop[i] = db.change == b.change && *db == same;
                }
                Change::Modified if b.change == Change::Modified => {
                    if db.schema.as_ref().map(|s| &s.1) == b.schema.as_ref().map(|s| &s.1) {
                        db.schema = None;
                    }
                    subtract_by(&mut db.tables, &b.tables, |t| {
                        (t.table.clone(), t.change, t.digest_after.clone())
                    });
                    if db.digest.as_ref().map(|s| &s.1) == b.digest.as_ref().map(|s| &s.1) {
                        db.digest = None;
                    }
                    drop[i] = db.schema.is_none() && db.tables.is_empty() && db.digest.is_none();
                }
                Change::Modified => {}
            }
        }
        let mut i = 0;
        self.database.retain(|_| {
            i += 1;
            !drop[i - 1]
        });

        subtract_by(&mut self.not_kept.listening, &bl.not_kept.listening, |l| {
            (l.proto.clone(), l.port.clone(), l.process.clone())
        });
        subtract(&mut self.not_kept.processes, &bl.not_kept.processes);
        subtract(&mut self.not_kept.services, &bl.not_kept.services);
        for (g, changes) in &mut self.not_kept.credential_details {
            if let Some(bp) = bl.not_kept.credential_details.get(g) {
                subtract_paths(changes, bp);
            }
        }
        self.not_kept
            .credential_details
            .retain(|_, v| !v.is_empty());
        subtract(&mut self.ignored.paths, &bl.ignored.paths);
        let btrees: HashMap<&str, u64> = bl
            .ignored
            .pruned_trees
            .iter()
            .map(|b| (b.path.as_str(), b.delta))
            .collect();
        for t in &mut self.ignored.pruned_trees {
            if let Some(b) = btrees.get(t.path.as_str()) {
                t.delta = t.delta.saturating_sub(*b);
            }
        }
        self.ignored.pruned_trees.retain(|t| t.delta > 0);
    }

    /// Derives the dependency collapse, the path lists and the text lines from the detail.
    fn finish(&mut self) {
        for pc in &mut self.packages {
            collapse(pc);
        }
        self.settings.files.sort();
        self.other.sort();
        self.idle_noise.sort();
        self.repo_changes.sort();
        self.repo_files = self.repo_changes.iter().map(|c| c.path.clone()).collect();
        for v in self.not_kept.credential_details.values_mut() {
            v.sort_by(|x, y| x.path.cmp(&y.path));
            v.dedup_by(|y, x| x.path == y.path);
        }
        self.not_kept.credentials = self
            .not_kept
            .credential_details
            .iter()
            .map(|(g, v)| (g.clone(), v.iter().map(|c| c.path.clone()).collect()))
            .collect();
        self.assign_ids();
        self.summary = self.sections();
    }

    pub fn is_empty(&self) -> bool {
        self.summary.iter().all(|s| s.lines.is_empty()) && self.ignored.count() == 0
    }

    fn sections(&self) -> Vec<Section> {
        let ctx = Ctx {
            home: self.home.clone(),
        };
        let mut out: Vec<Section> = Vec::new();
        let mut push = |bucket: &str, lines: Vec<String>| {
            if !lines.is_empty() {
                out.push(Section {
                    bucket: bucket.to_owned(),
                    lines,
                });
            }
        };
        push("Code", self.code_lines(&ctx));
        push("Packages", self.package_lines());
        push("Settings & files", self.settings_lines(&ctx));
        push("Database changed", self.database_lines(&ctx, false));
        push("Database not compared", self.database_lines(&ctx, true));
        push(
            "Other changed",
            rollup(&ctx, &self.other, |p| other_group(&ctx, p)),
        );
        push("Not kept", self.not_kept_lines(&ctx));
        push("Also changes when idle", self.idle_noise_lines(&ctx));
        out
    }

    fn idle_noise_lines(&self, ctx: &Ctx) -> Vec<String> {
        if self.idle_noise.is_empty() {
            return Vec::new();
        }
        let names: Vec<String> = self
            .idle_noise
            .iter()
            .take(3)
            .map(|c| ctx.short(&c.path))
            .collect();
        let more = self.idle_noise.len().saturating_sub(3);
        vec![format!(
            "{} files ({}{})",
            self.idle_noise.len(),
            names.join(", "),
            if more > 0 {
                format!(", +{more} more")
            } else {
                String::new()
            }
        )]
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
                let tail = if names.len() > 3 {
                    format!(" +{} more", names.len() - 3)
                } else {
                    String::new()
                };
                out.push(format!(
                    "{r}: {} dirty/untracked: {}{tail}",
                    names.len(),
                    names[..names.len().min(3)].join(", ")
                ));
            }
            if !rc.dirty_gone.is_empty() {
                out.push(format!(
                    "{r}: {} dirty/untracked files now clean",
                    rc.dirty_gone.len()
                ));
            }
            if rc.dirty_delta > 0 {
                out.push(format!(
                    "{r}: {} more dirty/untracked files",
                    rc.dirty_delta
                ));
            }
            if !rc.gitignored.is_empty() {
                let names: Vec<&str> = rc
                    .gitignored
                    .iter()
                    .map(|c| rel_to(&rc.repo, &c.path))
                    .collect();
                let tail = if names.len() > 3 {
                    format!(" +{} more", names.len() - 3)
                } else {
                    String::new()
                };
                out.push(format!(
                    "{r}: {}{tail} changed (gitignored)",
                    names[..names.len().min(3)].join(", ")
                ));
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
                let items = pc
                    .upgraded
                    .iter()
                    .map(|(n, (x, y))| format!("{n} {x}→{y}"))
                    .collect();
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
                .map(|(n, p)| {
                    format!(
                        "{n} ({} file{})",
                        p.len(),
                        if p.len() == 1 { "" } else { "s" }
                    )
                })
                .collect();
            if !files_only.is_empty() {
                parts.push(format!(
                    "files changed in {}",
                    cap(files_only, 4, "more").join(", ")
                ));
            }
            if !parts.is_empty() {
                out.push(format!("{}: {}", pkg_label(&pc.manager), parts.join("; ")));
            }
        }
        out
    }

    fn settings_lines(&self, ctx: &Ctx) -> Vec<String> {
        let s = &self.settings;
        let (mut lines, rolled) = rollup_parts(ctx, &s.files, |p| {
            p.rsplit_once('/').map(|x| x.0).unwrap_or("").to_owned()
        });
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
            let verb = match (c.change, c.lines_added, c.lines_removed) {
                (Change::Modified, Some(added), Some(removed)) if added + removed > 0 => {
                    match (added, removed) {
                        (a, 0) => format!("{} added", lines_text(a)),
                        (0, r) => format!("{} removed", lines_text(r)),
                        (a, r) => format!("{} added, {r} removed", lines_text(a)),
                    }
                }
                _ => verb.to_owned(),
            };
            lines.push(format!("cron {}: {verb}", c.user));
        }
        for v in &s.services {
            lines.push(format!("service {}: {}→{}", v.name, v.from, v.to));
        }
        // Directory counts last: a line limit cuts them before any named change.
        lines.extend(rolled);
        lines
    }

    fn database_lines(&self, ctx: &Ctx, uncompared: bool) -> Vec<String> {
        let mut out = Vec::new();
        for d in &self.database {
            if d.not_compared.is_some() != uncompared {
                continue;
            }
            if let Some(why) = &d.not_compared {
                out.push(format!(
                    "{} {}: database not compared: {why}",
                    d.engine,
                    ctx.short(&d.name)
                ));
                continue;
            }
            if d.engine == "sqlite" {
                let name = ctx.short(&d.name);
                let (x, y) = d.digest.clone().unwrap_or((None, None));
                let digest =
                    |o: &Option<String>| o.as_deref().map(short7).unwrap_or_else(|| "-".into());
                out.push(match d.change {
                    Change::Added => format!("sqlite {name}: added ({})", digest(&y)),
                    Change::Deleted => format!("sqlite {name}: removed"),
                    Change::Modified => {
                        format!("sqlite {name}: changed ({}→{})", digest(&x), digest(&y))
                    }
                });
                continue;
            }
            let head = format!("postgres {}", d.name);
            match d.change {
                Change::Added => {
                    let t = d.tables.len();
                    out.push(format!(
                        "{head}: new database ({t} table{})",
                        if t == 1 { "" } else { "s" }
                    ));
                }
                Change::Deleted => out.push(format!("{head}: dropped")),
                Change::Modified => {
                    let mut parts = Vec::new();
                    if d.schema.is_some() {
                        parts.push("schema changed".to_owned());
                    }
                    if !d.tables.is_empty() {
                        let items = d.tables.iter().map(table_text).collect();
                        parts.push(format!(
                            "data changed in {}",
                            cap(items, 3, "more tables").join(", ")
                        ));
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
                .map(|l| {
                    if l.process == "-" {
                        format!(":{}", l.port)
                    } else {
                        format!(":{} ({})", l.port, l.process)
                    }
                })
                .collect();
            out.push(format!("listening: {}", cap(items, 6, "more").join(", ")));
        }
        if !nk.processes.is_empty() {
            out.push(format!(
                "processes: {}",
                cap(nk.processes.clone(), 6, "more").join(", ")
            ));
        }
        if !nk.services.is_empty() {
            out.push(format!(
                "services: {}",
                cap(nk.services.clone(), 6, "more").join(", ")
            ));
        }
        if !nk.credentials.is_empty() {
            let items = nk
                .credentials
                .iter()
                .map(|(g, paths)| {
                    if g.ends_with('/') {
                        let n = paths.len();
                        format!(
                            "{} ({n} file{})",
                            ctx.short(g),
                            if n == 1 { "" } else { "s" }
                        )
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
        let mut secs: Vec<(String, Vec<String>, usize)> = self
            .summary
            .iter()
            .filter(|s| !s.lines.is_empty())
            .map(|s| (s.bucket.clone(), s.lines.clone(), 0))
            .collect();
        let total = |secs: &Vec<(String, Vec<String>, usize)>| -> usize {
            secs.iter()
                .map(|s| 1 + s.1.len() + usize::from(s.2 > 0))
                .sum::<usize>()
                + usize::from(ignored > 0)
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
            lines.push(format!(
                "Ignored: {ignored} paths (caches, logs, build output)"
            ));
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
        Change::Modified => format!(
            "{} ({}→{} rows)",
            t.table,
            n(t.rows_before),
            n(t.rows_after)
        ),
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
        pc.added
            .keys()
            .filter(|n| !required.contains(n))
            .cloned()
            .collect()
    } else {
        return;
    };
    if top.is_empty() {
        return;
    }
    let tops: BTreeSet<&String> = top.iter().collect();
    pc.dependencies = added
        .iter()
        .filter(|n| !tops.contains(**n))
        .map(|n| (*n).clone())
        .collect();
    pc.top_level = Some(top);
}

fn deps_of(pc: &PackageChange, deps: &BTreeSet<&String>, name: &str) -> usize {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut stack = vec![name];
    while let Some(n) = stack.pop() {
        for d in pc.requires.get(n).into_iter().flatten() {
            if deps.contains(d) && seen.insert(d) {
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
            let deps: BTreeSet<&String> = pc.dependencies.iter().collect();
            let items: Vec<String> = top
                .iter()
                .map(|n| {
                    let k = if per_parent { deps_of(pc, &deps, n) } else { 0 };
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
            let n = if pc.manager == "pip" {
                CAP_PIP
            } else {
                CAP_DEFAULT
            };
            let names: Vec<&String> = pc.added.keys().collect();
            let mut s = names
                .iter()
                .take(n)
                .map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            if names.len() > n {
                s.push_str(&format!(" (+{} more)", names.len() - n));
            }
            s
        }
    }
}
