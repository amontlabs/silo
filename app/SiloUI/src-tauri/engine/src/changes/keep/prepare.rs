//! The read-only part of a keep: look at both computers, build the plan, read what the fork
//! will give, and decide whether the keep may start. Everything the keep writes is read here
//! once, so the apply step works from one frozen view of the fork. Main gets only a temporary
//! git ref and temp bundle files, both removed on every path.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::{
    cron::{self, Merge},
    env::{KeepEnv, Live, ManifestStore, Os, RunAs},
    plan::{self, outcome, Conflict, Decision, FileAction, FileOp, PkgOp, Plan, ReportItem},
    scripts, Refusal, Report, Request,
};
use crate::changes::{
    diff::{diff, Diff},
    ignore::{credential_group, IgnoreList},
    items::{self, Answer, Said},
    manifest::Manifest,
};

/// The branch a repository's fork commits are left on.
pub struct BranchPlan {
    /// The name the branch gets: the wanted one, or the first free one after it.
    pub name: String,
    /// The branch already exists and points at the fork's commits.
    pub exists: bool,
}

pub struct Bundle {
    pub repo: String,
    pub path: String,
    pub tmp_ref: String,
    /// `uptodate`, `ff`, `merge`, `new`, or `conflict` / `unsupported` when the caller
    /// answered `branch`.
    pub state: String,
    /// Set when the commits are left on a branch instead of being merged.
    pub branch: Option<BranchPlan>,
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
    /// What a computer left idle changes; taken out of the fork's diff and of main's.
    pub noise: Option<Diff>,
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
    let diff = diff.without_ids();
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
) -> Result<(Manifest, Option<Diff>, Diff), String> {
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
    Ok((fork, noise, changes))
}

/// Captures the fork and compares it with its fork-time base.
pub fn look_at_fork(
    env: &dyn KeepEnv,
    store: &dyn ManifestStore,
    fork_name: &str,
) -> Result<ForkView, String> {
    env.note("Looking at the fork");
    let base = parse(&store.fork_time(fork_name)?)?;
    let (fork, noise, changes) = compare(store, fork_name, &base, &store.capture(fork_name)?)?;
    let digest = review_digest(&changes);
    Ok(ForkView {
        base,
        fork,
        diff: changes,
        digest,
        noise,
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

/// Whether a path inside a repository is a credential or an env file, judged by its path in
/// the repository and by its full path (a repository can sit where the credentials are).
fn sensitive_in(repo: &str, rel: &str) -> bool {
    let full = format!("{}/{rel}", repo.trim_end_matches('/'));
    sensitive_path(rel) || credential_group(&full).is_some()
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
    let (fork_label, target_label) = (env.name_of(fork_name), env.name_of(target));
    let os = env.os(target)?;
    if env.os(fork_name)? != os {
        return Err("The fork and the computer it is kept into run different systems.".into());
    }
    env.note("Looking at the target");
    let main = parse(&store.capture(target)?)?;
    let mut plan = plan::build(&view.diff, &view.base, &view.fork, &main, os, request);
    let first_plan = plan.clone();
    let first_read = read_signature(&first_plan, &view.fork);
    let mut report = Report {
        fork: fork_name.to_owned(),
        target: target.to_owned(),
        summary: view.diff.render_lines(15),
        review_digest: Some(view.digest.clone()),
        fast_forward: Some(main_unchanged_since_fork(view, &main)),
        ..Report::default()
    };
    let mut items = std::mem::take(&mut plan.items);
    let forced = |id: &str| request.choices.said(id) == Said::Answered(Answer::Fork);
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
        let unreadable = matches!(f, Live::Unreadable) || matches!(m, Live::Unreadable);
        let mut decision = plan::decide(&cand.base, f, m);
        if matches!(decision, Decision::Conflict(_)) && forced(&cand.id) && !unreadable {
            decision = Decision::Write;
        }
        match decision {
            Decision::Skip => {
                if f == m {
                    items.push(ReportItem::new(
                        &cand.id,
                        &cand.path,
                        outcome::ALREADY_THERE,
                        "main already holds what the fork holds",
                    ));
                }
            }
            Decision::Conflict(why) => {
                let allowed: &[Answer] = if unreadable {
                    &[Answer::Main]
                } else {
                    &[Answer::Fork, Answer::Main]
                };
                plan.conflicts
                    .push(Conflict::new("file", cand.path.clone(), why).answerable(
                        &cand.id,
                        allowed,
                        plan::file_change(&cand.base, f),
                        plan::file_change(&cand.base, m),
                    ));
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
                items.push(ReportItem::new(
                    &cand.id,
                    &cand.path,
                    outcome::BROUGHT_OVER,
                    match &action {
                        FileAction::Copy => "written from the fork",
                        FileAction::Link(_) => "link recreated",
                        FileAction::Delete => "deleted, as in the fork",
                    },
                ));
                plan.files.push(FileOp {
                    id: cand.id.clone(),
                    path: cand.path.clone(),
                    action,
                });
            }
        }
    }

    for op in &plan.packages {
        report.additions.add_package(&op.manager, &op.name);
    }
    for op in &plan.files {
        if op.action != FileAction::Delete && plan.settings_files.contains(&op.path) {
            report.additions.add_file(&op.path);
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
                if sensitive_in(&repo.repo, &name) {
                    let path = format!("{}/{name}", repo.repo.trim_end_matches('/'));
                    plan.conflicts.push(
                        Conflict::new(
                            "credentials",
                            path.clone(),
                            "a commit in the fork adds it; credentials and ignored paths are not kept into main",
                        )
                        .answerable(&format!("credentials:{path}"), &[], "added", ""),
                    );
                }
            }
        }
        if repo.commits.is_none() && !repo.new {
            items.push(ReportItem::new(
                &repo.id,
                &repo.repo,
                outcome::ALREADY_THERE,
                "the fork has no new commits here; its uncommitted files are listed on their own",
            ));
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
        let mut branch = None;
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
            let stuck = matches!(state.as_str(), "conflict" | "unsupported");
            if stuck && forced(&repo.id) {
                return Err(format!(
                    "{} cannot be forced over main's repository, because both sides changed it. Answer `branch` to keep the fork's commits as a branch, or `main` to leave it.",
                    repo.id
                ));
            }
            if let (Some(wanted), true) = (&repo.branch, state != "uptodate") {
                let out = env
                    .run(
                        target,
                        RunAs::Account,
                        &scripts::git_branch_plan(&repo.repo, &tmp_ref, wanted),
                    )?
                    .success(&format!("Choosing a branch name in {}", repo.repo))?;
                let mut lines = out.lines();
                let name = lines.next().unwrap_or("").trim().to_owned();
                if name.is_empty() {
                    return Err(format!(
                        "Silo could not choose a branch name in {}.",
                        repo.repo
                    ));
                }
                branch = Some(BranchPlan {
                    name,
                    exists: lines.next().is_some_and(|l| l.trim() == "same"),
                });
            } else if state == "conflict" {
                let lines: Vec<&str> = lines.map(str::trim).collect();
                plan.conflicts
                    .push(git_conflict(&repo, &main, &lines.join(" | ")));
            } else if state == "unsupported" {
                plan.conflicts.push(git_conflict(
                    &repo,
                    &main,
                    "this git cannot test a merge before it happens (git merge-tree needs git 2.38)",
                ));
            }
            state
        };
        items.push(repo_item(&repo, &state, branch.as_ref()));
        bundles.push(Bundle {
            repo: repo.repo.clone(),
            path,
            tmp_ref,
            state,
            branch,
        });
    }

    // Uncommitted files and deletes: three-way against the blob at the fork point.
    for (index, repo) in plan.repos.clone().iter().enumerate() {
        let mut keep_ops = Vec::new();
        if repo.branch.is_some() {
            if !repo.dirty.is_empty() {
                plan.notes.push(format!(
                    "Uncommitted changes in {} are not carried while its commits are left on a branch",
                    repo.repo
                ));
            }
            for op in &repo.dirty {
                items.push(ReportItem::new(
                    &op.id,
                    &op.path,
                    outcome::LEFT_AS_MAIN,
                    "not carried: the repository's commits are left on a branch",
                ));
            }
            dirty_checked.push((index, keep_ops));
            continue;
        }
        let base_head = repo.base_head.clone().unwrap_or_default();
        let paths: Vec<String> = repo
            .dirty
            .iter()
            .filter(|d| !d.delete)
            .map(|d| d.path.clone())
            .collect();
        let fork_paths = env.inspect(fork_name, &paths)?;
        for (n, op) in repo.dirty.iter().enumerate() {
            match request.choices.said(&op.id) {
                Said::Skipped => {
                    items.push(ReportItem::new(
                        &op.id,
                        &op.path,
                        outcome::SKIPPED,
                        "skipped; it stays as main has it",
                    ));
                    continue;
                }
                Said::Answered(Answer::Main) => {
                    items.push(ReportItem::new(
                        &op.id,
                        &op.path,
                        outcome::LEFT_AS_MAIN,
                        "left as main has it, as answered",
                    ));
                    continue;
                }
                _ => {}
            }
            if sensitive_in(&repo.repo, &op.rel) {
                plan.conflicts.push(
                    Conflict::new("credentials", op.path.clone(), "it is uncommitted in the fork; credentials and ignored paths are not kept into main")
                        .answerable(&format!("credentials:{}", op.path), &[], "added", ""),
                );
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
                        plan.conflicts.push(
                            Conflict::new("file", op.path.clone(), "it cannot be read").answerable(
                                &op.id,
                                &[Answer::Main],
                                "cannot be read",
                                "",
                            ),
                        );
                        continue;
                    }
                }
            }
            let mut decision = plan::decide_dirty(&blob, &fork_m, &main_m);
            if matches!(decision, Decision::Conflict(_)) && forced(&op.id) {
                decision = Decision::Write;
            }
            match decision {
                Decision::Skip => items.push(ReportItem::new(
                    &op.id,
                    &op.path,
                    outcome::ALREADY_THERE,
                    "main already holds what the fork holds",
                )),
                Decision::Conflict(why) => {
                    plan.conflicts
                        .push(Conflict::new("file", op.path.clone(), why).answerable(
                            &op.id,
                            &[Answer::Fork, Answer::Main],
                            plan::dirty_change(&blob, &fork_m),
                            plan::dirty_change(&blob, &main_m),
                        ))
                }
                Decision::Write => {
                    items.push(ReportItem::new(
                        &op.id,
                        &op.path,
                        outcome::BROUGHT_OVER,
                        if op.delete {
                            "deleted, as in the fork"
                        } else {
                            "uncommitted change carried"
                        },
                    ));
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
        let ids: BTreeMap<&str, &str> = plan
            .files
            .iter()
            .map(|op| (op.path.as_str(), op.id.as_str()))
            .chain(
                plan.repos
                    .iter()
                    .flat_map(|r| r.dirty.iter().map(|d| (d.path.as_str(), d.id.as_str()))),
            )
            .collect();
        let out = env
            .run(target, RunAs::Root, &scripts::dest_check(&dests))?
            .code_ok("Checking destinations")?;
        let mut blocked = Vec::new();
        for (path, verdict) in scripts::parse_pairs(&out) {
            if verdict != "ok" {
                let id = ids.get(path.as_str()).copied().unwrap_or_default();
                blocked.push(
                    Conflict::new("destination", path, verdict.clone()).answerable(
                        id,
                        &[Answer::Main],
                        "to be written here",
                        verdict,
                    ),
                );
            }
        }
        plan.conflicts.extend(blocked);
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
            plan.conflicts.push(
                Conflict::new("package", format!("{} {}", pkg.manager, pkg.name), detail)
                    .answerable(
                        &items::package(&pkg.manager, &pkg.name),
                        &[Answer::Main],
                        format!("version {}", pkg.version),
                        if shipped.is_empty() {
                            "no version offered".to_owned()
                        } else {
                            format!("brew offers {shipped}")
                        },
                    ),
            );
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
        let id = items::cron(user);
        let mut merged = if untouched {
            if theirs.lines().eq(ours.lines()) {
                Merge::Unchanged
            } else {
                Merge::Merged(theirs.clone())
            }
        } else {
            cron::merge(base_text.map_or("", String::as_str), &theirs, &ours)
        };
        if matches!(merged, Merge::Conflict(_)) && forced(&id) {
            merged = Merge::Merged(theirs.clone());
        }
        match merged {
            Merge::Unchanged => items.push(ReportItem::new(
                &id,
                user,
                outcome::ALREADY_THERE,
                "main's crontab already holds what the fork's does",
            )),
            Merge::Merged(text) => {
                items.push(ReportItem::new(
                    &id,
                    user,
                    outcome::BROUGHT_OVER,
                    if untouched || text == theirs {
                        "the fork's crontab is written"
                    } else {
                        "the fork's lines are merged into main's crontab"
                    },
                ));
                cron_texts.push((user.clone(), text));
            }
            Merge::Conflict(lines) => plan.conflicts.push(
                Conflict::new(
                    "crontab",
                    user.clone(),
                    format!("main and the fork both changed: {}", lines.join(" | ")),
                )
                .answerable(
                    &id,
                    &[Answer::Fork, Answer::Main],
                    "changed lines",
                    "changed lines",
                ),
            ),
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
            format!(
                "{fork_label} changed while it was being read. {target_label} was not changed."
            ),
            vec![Conflict::new(
                "fork",
                fork_label.as_str(),
                "its changes differ from the first look",
            )],
            "Let the fork finish, review it again with `silo diff`, and merge again.",
        ));
    }

    if !plan.conflicts.is_empty() {
        let conflicts = plan.conflicts.clone();
        report.notes.extend(plan.notes.clone());
        if conflicts.iter().any(|c| c.kind == "credentials") {
            let message = format!(
                "The merge stopped: {} would bring credentials or ignored files. {target_label} was not changed.",
                conflicts
                    .iter()
                    .filter(|c| c.kind == "credentials")
                    .map(|c| c.item.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return Ok(refuse(
                report,
                "credentials",
                message,
                conflicts,
                "Credentials are never merged into main. Remove them from the fork's commits and files and merge again.",
            ));
        }
        let message = format!(
            "{} changed on both sides and needs an answer before {fork_label} can be merged into {target_label}: {}. {target_label} was not changed.",
            if conflicts.len() == 1 {
                "One item".to_owned()
            } else {
                format!("{} items", conflicts.len())
            },
            conflicts
                .iter()
                .map(|c| c.item.clone())
                .collect::<Vec<_>>()
                .join(", "),
        );
        return Ok(refuse(
            report,
            "choices-needed",
            message,
            conflicts,
            "Merge again with `choices`: an answer for each listed id, one of the answers it allows (`fork`, `main`, or `branch` for a repository). Add an id to `skip` to leave an item that does not conflict.",
        ));
    }
    let migrating = !request.hooks.migrate.is_empty() && migrations_changed;
    let covered = request.data_ok || migrating;
    if !plan.databases.is_empty() && !covered {
        let message = format!(
            "Database changed ({}) and no migration covers it. {target_label} was not changed.",
            plan.databases.join(", ")
        );
        let hint = format!(
            "A merge carries a database change only when the recipe has a migrate: step and the fork changed migration files. Otherwise replace main with the fork: run `silo replace-main {}` and the fork becomes main with its database.",
            report.fork
        );
        return Ok(refuse(report, "database", message, Vec::new(), &hint));
    }
    for (id, label) in plan.database_ids.iter().zip(&plan.databases) {
        items.push(ReportItem::new(
            id,
            label,
            outcome::NOT_COMBINED,
            if migrating {
                "main's database is not combined with the fork's; the recipe's migrate step runs on main's own database"
            } else {
                "main's database is left as it is; the fork's database changes are not combined (kept without a migration step, as asked)"
            },
        ));
    }
    items.sort_by(|a, b| a.id.cmp(&b.id));
    items.dedup_by(|later, first| later.id == first.id);
    report.items = items;
    report.summary.extend(
        plan.summary()
            .into_iter()
            .map(|line| format!("will merge {line}")),
    );
    report.notes.extend(plan.notes.clone());
    Ok(Prepared::Ready(Ready {
        plan,
        main,
        os,
        report,
        cron: cron_texts,
        bundles,
        migrations_changed,
        fork_live: fork_final,
        staged,
        bundle_data,
    }))
}

/// Whether main still holds everything the fork started from: what main changed since the
/// fork was made, compared with the fork-time record the same way the fork is. Databases do
/// not count, since a keep never combines them. When this is true nothing can conflict.
fn main_unchanged_since_fork(view: &ForkView, main: &Manifest) -> bool {
    !diff(
        &view.base,
        main,
        view.noise.as_ref(),
        &IgnoreList::builtin(),
    )
    .has_reviewable_changes()
}

fn short(head: &str) -> &str {
    &head[..head.len().min(7)]
}

/// A repository both sides changed, as a conflict only `branch` and `main` settle.
fn git_conflict(repo: &plan::RepoOp, main: &Manifest, detail: &str) -> Conflict {
    let fork_change = match &repo.commits {
        Some((_, new)) => format!("new commits, now at {}", short(new)),
        None => "changed".to_owned(),
    };
    let main_change = match (&repo.base_head, main.git.get(&repo.repo)) {
        (Some(base), Some(now)) if *base != now.head => {
            format!("moved from {} to {}", short(base), short(&now.head))
        }
        (_, Some(_)) => "its own changes".to_owned(),
        _ => "changed".to_owned(),
    };
    Conflict::new("git", repo.repo.clone(), detail).answerable(
        &repo.id,
        &[Answer::Branch, Answer::Main],
        fork_change,
        main_change,
    )
}

/// What happened to a repository's commits.
fn repo_item(repo: &plan::RepoOp, state: &str, branch: Option<&BranchPlan>) -> ReportItem {
    let (result, detail) = match (state, branch) {
        ("uptodate", _) => (
            outcome::ALREADY_THERE,
            "main already has the fork's commits".to_owned(),
        ),
        (_, Some(branch)) if branch.exists => (
            outcome::BRANCH,
            format!(
                "branch {} already holds the fork's commits; nothing was merged or checked out",
                branch.name
            ),
        ),
        (_, Some(branch)) => (
            outcome::BRANCH,
            format!(
                "the fork's commits are left on branch {}; nothing was merged or checked out",
                branch.name
            ),
        ),
        ("new", _) => (
            outcome::BROUGHT_OVER,
            "the repository is cloned into main".to_owned(),
        ),
        ("ff", _) => (
            outcome::BROUGHT_OVER,
            "main moves forward to the fork's commits".to_owned(),
        ),
        _ => (
            outcome::BROUGHT_OVER,
            "the fork's commits are merged into main's".to_owned(),
        ),
    };
    ReportItem::new(&repo.id, &repo.repo, result, detail)
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
