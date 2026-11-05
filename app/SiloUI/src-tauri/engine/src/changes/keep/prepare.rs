//! The read-only part of a keep: look at both computers, build the plan, read what the fork
//! will give, and decide whether the keep may start. Everything the keep writes is read here
//! once, so the apply step works from one frozen view of the fork. Main gets only a temporary
//! git ref and temp bundle files, both removed on every path.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::{
    cron::{self, Merge},
    env::{KeepEnv, Live, ManifestStore, Os, RunAs},
    plan::{self, Base, Conflict, Decision, FileAction, FileOp, Plan, PkgOp},
    scripts, Refusal, Report, Request,
};
use crate::changes::{
    diff::{diff, Diff},
    ignore::{credential_group, IgnoreList},
    manifest::Manifest,
};

pub struct Bundle {
    pub repo: String,
    pub path: String,
    pub tmp_ref: String,
    /// `uptodate`, `ff`, `merge` or `new`.
    pub state: String,
}

/// Temporary things a keep leaves on the computers until it ends.
#[derive(Default)]
pub struct Tracker {
    pub fork: String,
    pub target: String,
    /// (repo, bundle path, temporary ref), on both computers.
    pub temps: Vec<(String, String, String)>,
}

impl Tracker {
    pub fn cleanup(&mut self, env: &dyn KeepEnv) {
        for (repo, path, tmp_ref) in std::mem::take(&mut self.temps) {
            for computer in [&self.fork, &self.target] {
                let _ = env.run(computer, RunAs::Root, &scripts::file_remove(&path));
            }
            let _ = env.run(
                &self.target,
                RunAs::Account,
                &scripts::git_drop_ref(&repo, &tmp_ref),
            );
        }
    }
}

/// The fork as the user reviewed it.
pub struct ForkView {
    pub base: Manifest,
    pub fork: Manifest,
    pub diff: Diff,
    pub digest: String,
}

pub struct Ready {
    pub plan: Plan,
    pub main: Manifest,
    pub os: Os,
    pub report: Report,
    /// (user, the crontab main ends up with)
    pub cron: Vec<(String, String)>,
    pub bundles: Vec<Bundle>,
    pub migrations_changed: bool,
    /// Path -> what the fork holds, for the files the keep writes.
    pub fork_live: BTreeMap<String, Live>,
    /// Path -> bytes read from the fork.
    pub staged: BTreeMap<String, Vec<u8>>,
    /// bundle path -> bytes
    pub bundle_data: BTreeMap<String, Vec<u8>>,
}

#[allow(clippy::large_enum_variant)]
pub enum Prepared {
    Ready(Ready),
    Refused { report: Report, refusal: Refusal },
}

/// The digest of what a reviewer saw: the buckets a keep acts on, without noise counts.
pub fn review_digest(diff: &Diff) -> String {
    let view = serde_json::json!({
        "code": diff.code, "packages": diff.packages, "settings": diff.settings,
        "database": diff.database, "other": diff.other,
    });
    let digest = Sha256::digest(serde_json::to_vec(&view).unwrap_or_default());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse(text: &str) -> Result<Manifest, String> {
    Manifest::parse(text).map_err(|e| e.to_string())
}

fn compare(
    store: &dyn ManifestStore,
    fork_name: &str,
    base: &Manifest,
    fork_text: &str,
) -> Result<(Manifest, Diff), String> {
    let fork = parse(fork_text)?;
    let noise = match store.baseline(fork_name) {
        Some((before, idle)) => Some(diff(
            &parse(&before)?,
            &parse(&idle)?,
            None,
            &IgnoreList::builtin(),
        )),
        None => None,
    };
    let changes = diff(base, &fork, noise.as_ref(), &IgnoreList::builtin());
    Ok((fork, changes))
}

/// Captures the fork and compares it with its fork-time base.
pub fn look_at_fork(
    env: &dyn KeepEnv,
    store: &dyn ManifestStore,
    fork_name: &str,
) -> Result<ForkView, String> {
    env.note("Looking at the fork");
    let base = parse(&store.fork_time(fork_name)?)?;
    let (fork, changes) = compare(store, fork_name, &base, &store.capture(fork_name)?)?;
    let digest = review_digest(&changes);
    Ok(ForkView {
        base,
        fork,
        diff: changes,
        digest,
    })
}

/// Migration folders and files, not any path that happens to contain the word.
pub fn is_migration(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let dirs = &parts[..parts.len().saturating_sub(1)];
    dirs.iter().any(|p| {
        let p = p.to_ascii_lowercase();
        matches!(p.as_str(), "migrations" | "migrate" | "alembic")
    })
}

/// Paths a git history must not carry into main: credentials, env files, ignored paths.
pub fn sensitive_path(path: &str) -> bool {
    let abs = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    };
    let base = abs.rsplit('/').next().unwrap_or("");
    credential_group(&abs).is_some()
        || IgnoreList::builtin().matches(&abs)
        || base == ".env"
        || (base.starts_with(".env.")
            && !matches!(base, ".env.example" | ".env.sample" | ".env.template"))
}

fn refuse(
    report: Report,
    reason: &str,
    message: String,
    conflicts: Vec<Conflict>,
    hint: &str,
) -> Prepared {
    Prepared::Refused {
        report,
        refusal: Refusal {
            reason: reason.to_owned(),
            message,
            conflicts,
            hint: hint.to_owned(),
        },
    }
}

fn nul_names(text: &str) -> Vec<String> {
    text.split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[allow(clippy::too_many_lines)]
pub fn prepare(
    env: &dyn KeepEnv,
    store: &dyn ManifestStore,
    request: &Request,
    target: &str,
    view: &ForkView,
    tracker: &mut Tracker,
    read_files: bool,
) -> Result<Prepared, String> {
    let fork_name = request.fork.as_str();
    let os = env.os(target)?;
    if env.os(fork_name)? != os {
        return Err("The fork and the computer it is kept into run different systems.".into());
    }
    env.note("Looking at the target");
    let main = parse(&store.capture(target)?)?;
    let mut plan = plan::build(&view.diff, &view.base, &view.fork, &main, os);
    let first_plan = plan.clone();
    let first_read = read_signature(&first_plan, &view.fork);
    let mut report = Report {
        fork: fork_name.to_owned(),
        target: target.to_owned(),
        summary: view.diff.render_lines(15),
        review_digest: Some(view.digest.clone()),
        ..Report::default()
    };
    let id = uuid::Uuid::new_v4().simple().to_string();

    // Files: the three-way rule against what both computers hold right now.
    let cand_paths: Vec<String> = plan.file_cands.iter().map(|c| c.path.clone()).collect();
    let fork_live = env.inspect(fork_name, &cand_paths)?;
    let main_live = env.inspect(target, &cand_paths)?;
    let mut staged = BTreeMap::new();
    let mut writes: Vec<String> = Vec::new();
    let mut fork_final: BTreeMap<String, Live> = BTreeMap::new();
    for cand in &plan.file_cands.clone() {
        let (f, m) = (&fork_live[&cand.path], &main_live[&cand.path]);
        match plan::decide(&cand.base, f, m) {
            Decision::Skip => {}
            Decision::Conflict(why) => {
                plan.conflicts
                    .push(Conflict::new("file", cand.path.clone(), why))
            }
            Decision::Write => {
                let action = match f {
                    Live::Missing => FileAction::Delete,
                    Live::Link(t) => {
                        if link_is_sensitive(&cand.path, t) {
                            plan.notes.push(format!(
                                "{} links to a credential path and is not kept",
                                cand.path
                            ));
                            continue;
                        }
                        FileAction::Link(t.clone())
                    }
                    _ => FileAction::Copy,
                };
                if credential_group(&cand.path).is_some() {
                    continue;
                }
                if action != FileAction::Delete {
                    writes.push(cand.path.clone());
                }
                fork_final.insert(cand.path.clone(), f.clone());
                plan.files.push(FileOp {
                    path: cand.path.clone(),
                    action,
                });
            }
        }
    }

    // Git: history first, so nothing sensitive reaches main.
    let mut bundles = Vec::new();
    let mut bundle_data = BTreeMap::new();
    let mut dirty_checked: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut migrations_changed = plan.files.iter().any(|op| is_migration(&op.path));
    for (index, repo) in plan.repos.clone().iter().enumerate() {
        let old = repo
            .commits
            .as_ref()
            .map(|(old, _)| old.clone())
            .unwrap_or_default();
        if let Some((_, new)) = &repo.commits {
            let names = env
                .run(
                    fork_name,
                    RunAs::Account,
                    &scripts::git_changed(&repo.repo, if repo.new { "" } else { &old }, new),
                )?
                .code_ok(&format!(
                    "Listing the fork's changed files in {}",
                    repo.repo
                ))?;
            for name in nul_names(&String::from_utf8_lossy(&names)) {
                if is_migration(&name) {
                    migrations_changed = true;
                }
                if sensitive_path(&name) {
                    plan.conflicts.push(Conflict::new(
                        "credentials",
                        format!("{}/{name}", repo.repo.trim_end_matches('/')),
                        "a commit in the fork adds it; credentials and ignored paths are not kept into main",
                    ));
                }
            }
        }
        if repo.commits.is_none() && !repo.new {
            continue;
        }
        if plan.conflicts.iter().any(|c| c.kind == "credentials") {
            continue;
        }
        let path = format!("/tmp/silo-keep-{id}-{index}.bundle");
        let tmp_ref = format!("refs/silo-tmp/{id}-{index}");
        tracker
            .temps
            .push((repo.repo.clone(), path.clone(), tmp_ref.clone()));
        env.run(
            fork_name,
            RunAs::Account,
            &scripts::git_bundle(&repo.repo, &path),
        )?
        .success("Bundling the fork's commits")?;
        let data = env.read(fork_name, &path)?;
        env.write(target, &path, &data, "644")?;
        bundle_data.insert(path.clone(), data);
        let state = if repo.new {
            "new".to_owned()
        } else {
            let out = env
                .run(
                    target,
                    RunAs::Account,
                    &scripts::git_precheck(&repo.repo, &path, &tmp_ref),
                )?
                .success(&format!("Checking the merge in {}", repo.repo))?;
            let mut lines = out.lines();
            let state = lines.next().unwrap_or("unsupported").trim().to_owned();
            match state.as_str() {
                "conflict" => {
                    for line in lines {
                        plan.conflicts.push(Conflict::new("git", repo.repo.clone(), line.trim()));
                    }
                }
                "unsupported" => plan.conflicts.push(Conflict::new(
                    "git",
                    repo.repo.clone(),
                    "this git cannot test a merge before it happens (git merge-tree needs git 2.38)",
                )),
                _ => {}
            }
            state
        };
        bundles.push(Bundle {
            repo: repo.repo.clone(),
            path,
            tmp_ref,
            state,
        });
    }

    // Uncommitted files and deletes: three-way against the blob at the fork point.
    for (index, repo) in plan.repos.clone().iter().enumerate() {
        let mut keep_ops = Vec::new();
        let base_head = repo.base_head.clone().unwrap_or_default();
        let paths: Vec<String> = repo
            .dirty
            .iter()
            .filter(|d| !d.delete)
            .map(|d| d.path.clone())
            .collect();
        let fork_paths = env.inspect(fork_name, &paths)?;
        for (n, op) in repo.dirty.iter().enumerate() {
            if sensitive_path(&op.rel) {
                plan.conflicts.push(Conflict::new("credentials", op.path.clone(), "it is uncommitted in the fork; credentials and ignored paths are not kept into main"));
                continue;
            }
            if is_migration(&op.rel) {
                migrations_changed = true;
            }
            let state = |computer: &str, repo_dir: &str| -> Result<(String, String), String> {
                let out = env
                    .run(
                        computer,
                        RunAs::Account,
                        &scripts::git_file_state(repo_dir, &op.rel, &base_head),
                    )?
                    .success("Comparing an uncommitted file")?;
                let mut words = out.split_whitespace();
                Ok((
                    words.next().unwrap_or("-").to_owned(),
                    words.next().unwrap_or("-").to_owned(),
                ))
            };
            let on_main = !repo.new && main.git.contains_key(&repo.repo);
            let (blob, main_m) = if on_main {
                state(target, &repo.repo)?
            } else {
                ("-".to_owned(), "-".to_owned())
            };
            let fork_m = if op.delete {
                "-".to_owned()
            } else {
                state(fork_name, &repo.repo)?.1
            };
            if !op.delete {
                match fork_paths.get(&op.path) {
                    Some(Live::File { .. }) => {}
                    Some(Live::Link(_)) | Some(Live::Dir) => {
                        plan.notes
                            .push(format!("{} is a link or folder and not kept", op.path));
                        continue;
                    }
                    Some(Live::Missing) | None => continue,
                    Some(Live::Unreadable) => {
                        plan.conflicts.push(Conflict::new(
                            "file",
                            op.path.clone(),
                            "it cannot be read",
                        ));
                        continue;
                    }
                }
            }
            match plan::decide_dirty(&blob, &fork_m, &main_m) {
                Decision::Skip => {}
                Decision::Conflict(why) => {
                    plan.conflicts
                        .push(Conflict::new("file", op.path.clone(), why))
                }
                Decision::Write => {
                    if !op.delete {
                        writes.push(op.path.clone());
                        if let Some(live) = fork_paths.get(&op.path) {
                            fork_final.insert(op.path.clone(), live.clone());
                        }
                    }
                    keep_ops.push(n);
                }
            }
        }
        dirty_checked.push((index, keep_ops));
    }
    for (index, keep) in dirty_checked {
        let mut n = 0;
        plan.repos[index].dirty.retain(|_| {
            n += 1;
            keep.contains(&(n - 1))
        });
    }
    plan.repos
        .retain(|r| r.commits.is_some() || r.new || !r.dirty.is_empty());

    // Destinations: no link or file in the way on main.
    let mut dests: Vec<String> = plan
        .files
        .iter()
        .filter(|op| op.action != FileAction::Delete)
        .map(|op| op.path.clone())
        .collect();
    dests.extend(
        plan.repos
            .iter()
            .flat_map(|r| r.dirty.iter().filter(|d| !d.delete).map(|d| d.path.clone())),
    );
    if !dests.is_empty() {
        let out = env
            .run(target, RunAs::Root, &scripts::dest_check(&dests))?
            .code_ok("Checking destinations")?;
        for (path, verdict) in scripts::parse_pairs(&out) {
            if verdict != "ok" {
                plan.conflicts
                    .push(Conflict::new("destination", path, verdict));
            }
        }
    }

    // Read what the fork will give. A look ahead of the checkpoint only decides refusals, so
    // the fork's files are read once, for the keep itself.
    if read_files && plan.conflicts.is_empty() {
        for path in &writes {
            if matches!(fork_final.get(path), Some(Live::File { .. })) {
                staged.insert(path.clone(), env.read(fork_name, path)?);
            }
        }
    }

    // Brew installs whatever it currently ships: it must be the fork's version.
    for pkg in plan
        .packages
        .clone()
        .iter()
        .filter(|p| p.manager.starts_with("brew"))
    {
        let shipped = env
            .run(
                target,
                RunAs::Account,
                &scripts::brew_version(&pkg.name, pkg.manager == "brew-cask"),
            )?
            .success("Asking brew for a version")?;
        let shipped = shipped.trim();
        let same = !shipped.is_empty()
            && (shipped == pkg.version
                || pkg.version.starts_with(&format!("{shipped}_"))
                || shipped.starts_with(&format!("{}_", pkg.version)));
        if !same {
            let detail = if shipped.is_empty() {
                format!(
                    "brew did not report a version for it; the fork has {}",
                    pkg.version
                )
            } else {
                format!("brew now offers {shipped}; the fork has {}", pkg.version)
            };
            plan.conflicts.push(Conflict::new(
                "package",
                format!("{} {}", pkg.manager, pkg.name),
                detail,
            ));
        }
    }

    // Crontabs: by line against the fork-time text.
    let mut cron_texts = Vec::new();
    for (user, _) in &plan.cron {
        let theirs = env
            .run(fork_name, RunAs::Root, &scripts::crontab_read(user))?
            .success("Reading the fork's crontab")?;
        let ours = env
            .run(target, RunAs::Root, &scripts::crontab_read(user))?
            .success("Reading main's crontab")?;
        // A quiet fork's own crontab was parked: the parked original is main's at fork time.
        let (base_digest, base_text) = match view.base.qcron.get(user) {
            Some((digest, text)) => (Some(digest), Some(text)),
            None => (view.base.cron.get(user), view.base.cront.get(user)),
        };
        let untouched = base_digest == main.cron.get(user);
        let merged = if untouched {
            if theirs.lines().eq(ours.lines()) {
                Merge::Unchanged
            } else {
                Merge::Merged(theirs.clone())
            }
        } else {
            cron::merge(base_text.map_or("", String::as_str), &theirs, &ours)
        };
        match merged {
            Merge::Unchanged => {}
            Merge::Merged(text) => cron_texts.push((user.clone(), text)),
            Merge::Conflict(lines) => plan.conflicts.push(Conflict::new(
                "crontab",
                user.clone(),
                format!("main and the fork both changed: {}", lines.join(" | ")),
            )),
        }
    }
    plan.cron
        .retain(|(user, _)| cron_texts.iter().any(|(u, _)| u == user));

    // The fork must not have moved while it was read: only what the keep uses is compared, so
    // background churn elsewhere on the fork does not refuse a keep.
    let again = parse(&store.capture(fork_name)?)?;
    if read_signature(&first_plan, &again) != first_read {
        return Ok(refuse(
            report,
            "changed",
            format!("{fork_name} changed while it was being read. {target} was not changed."),
            vec![Conflict::new(
                "fork",
                fork_name,
                "its changes differ from the first look",
            )],
            "Let the fork finish, review it again with `silo diff`, and keep again.",
        ));
    }

    let ready =
        |plan: Plan, cron: Vec<(String, String)>, bundles: Vec<Bundle>, mut report: Report| {
            report.summary.extend(
                plan.summary()
                    .into_iter()
                    .map(|line| format!("will keep {line}")),
            );
            report.notes.extend(plan.notes.clone());
            Ready {
                plan,
                main: main.clone(),
                os,
                report,
                cron,
                bundles,
                migrations_changed,
                fork_live: fork_final.clone(),
                staged: staged.clone(),
                bundle_data: bundle_data.clone(),
            }
        };

    if !plan.conflicts.is_empty() {
        let conflicts = plan.conflicts.clone();
        let reason = if conflicts.iter().any(|c| c.kind == "credentials") {
            "credentials"
        } else if conflicts.iter().all(|c| c.kind == "git") {
            "git"
        } else {
            "divergent"
        };
        let message = if conflicts.iter().all(|c| c.kind == "package") {
            format!(
                "Keep stopped: package version: {}. {target} was not changed.",
                conflicts
                    .iter()
                    .map(|c| format!("{} ({})", c.item, c.detail))
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        } else {
            format!(
                "Keep stopped: {} {} on {target} and in {fork_name}. {target} was not changed.",
                conflicts
                    .iter()
                    .map(|c| c.item.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
                if reason == "credentials" {
                    "would bring credentials or ignored files"
                } else {
                    "clash"
                },
            )
        };
        report.notes.extend(plan.notes.clone());
        return Ok(refuse(
            report,
            reason,
            message,
            conflicts,
            "Bring main's change into the fork (or remove the file from the fork's commits) and keep again.",
        ));
    }
    let covered = request.data_ok || (!request.hooks.migrate.is_empty() && migrations_changed);
    if !plan.databases.is_empty() && !covered {
        let message = format!(
            "Database changed ({}) and no migration covers it. {target} was not changed.",
            plan.databases.join(", ")
        );
        return Ok(refuse(
            report,
            "database",
            message,
            Vec::new(),
            "A keep carries a database change only when the recipe has a migrate: step and the fork changed migration files. Otherwise use \"Make this main\": the fork becomes main with its database.",
        ));
    }
    Ok(Prepared::Ready(ready(plan, cron_texts, bundles, report)))
}

/// The identity, in `fork`, of every item `plan` reads: file identities (kind, size, content
/// digest; no timestamps), package versions, repo state and crontab digests. Anything else the
/// fork changes while it is read is not part of the keep.
pub(super) fn read_signature(plan: &Plan, fork: &Manifest) -> Vec<String> {
    let mut paths: Vec<&str> = plan.file_cands.iter().map(|c| c.path.as_str()).collect();
    paths.extend(plan.agents.iter().map(|a| a.path.as_str()));
    for repo in &plan.repos {
        paths.extend(repo.dirty.iter().map(|d| d.path.as_str()));
    }
    paths.sort_unstable();
    paths.dedup();
    let mut items: Vec<String> = paths
        .into_iter()
        .map(|p| {
            let id = fork
                .files
                .get(p)
                .map(|e| (e.kind, e.size.as_str(), e.digest.as_str()));
            format!("file {p}: {id:?}")
        })
        .collect();
    let package = |p: &PkgOp| fork.pkg.get(&(p.manager.clone(), p.name.clone()));
    items.extend(
        plan.packages
            .iter()
            .chain(&plan.removals)
            .map(|p| format!("package {} {}: {:?}", p.manager, p.name, package(p))),
    );
    items.extend(plan.repos.iter().map(|r| {
        format!(
            "repo {}: {:?} {:?}",
            r.repo,
            fork.git.get(&r.repo),
            fork.gitf.get(&r.repo)
        )
    }));
    items.extend(
        plan.cron
            .iter()
            .map(|(user, _)| format!("crontab {user}: {:?}", fork.cron.get(user))),
    );
    items
}

fn link_is_sensitive(path: &str, target: &str) -> bool {
    let abs = if target.starts_with('/') {
        target.to_owned()
    } else {
        let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
        format!("{dir}/{target}")
    };
    sensitive_path(&abs)
}
