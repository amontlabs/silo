//! The read-only part of a keep: look at both computers, build the plan, and decide whether
//! the keep may start. Nothing here changes main except a `refs/silo/keep` ref in its repos.

use std::collections::BTreeMap;

use super::{
    cron::{self, Merge},
    env::{KeepEnv, ManifestStore, Os, RunAs},
    plan::{self, Conflict, Plan},
    scripts, Refusal, Report, Request,
};
use crate::changes::{diff::diff, ignore::IgnoreList, manifest::Manifest};

pub struct Bundle {
    pub repo: String,
    pub path: String,
    /// `uptodate`, `ff`, `merge`, `unsupported` or `new`.
    pub state: String,
}

pub struct Ready {
    pub plan: Plan,
    pub fork: Manifest,
    pub main: Manifest,
    pub os: Os,
    pub report: Report,
    /// (user, the crontab main ends up with)
    pub cron: Vec<(String, String)>,
    pub bundles: Vec<Bundle>,
    pub migrations_changed: bool,
}

#[allow(clippy::large_enum_variant)]
pub enum Prepared {
    Ready(Ready),
    Refused { report: Report, refusal: Refusal },
}

fn is_migration(path: &str) -> bool {
    path.split('/').any(|part| {
        let part = part.to_ascii_lowercase();
        part.contains("migrat") || part == "alembic"
    })
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

pub fn prepare(
    env: &dyn KeepEnv,
    store: &dyn ManifestStore,
    request: &Request,
    target: &str,
) -> Result<Prepared, String> {
    let fork_name = request.fork.as_str();
    let os = env.os(target)?;
    if env.os(fork_name)? != os {
        return Err("The fork and the computer it is kept into run different systems.".into());
    }
    env.note("Looking at both computers");
    let base = store.fork_time(fork_name)?;
    let fork = store.capture(fork_name)?;
    let main = store.capture(target)?;
    let baseline = store.baseline(fork_name);
    let changes = diff(&base, &fork, baseline.as_ref(), &IgnoreList::builtin());
    let mut plan = plan::build(&changes, &base, &fork, &main);
    let mut report = Report {
        fork: fork_name.to_owned(),
        target: target.to_owned(),
        summary: changes.render_lines(15),
        ..Report::default()
    };

    // Did the keep carry a migration? Looked up in the fork's commits and files.
    let mut migrations_changed = changes.repo_files.iter().any(|path| is_migration(path))
        || plan
            .repos
            .iter()
            .any(|repo| repo.dirty.iter().any(|path| is_migration(path)))
        || plan.files.iter().any(|op| is_migration(&op.path));
    for repo in &plan.repos {
        if let Some((old, new)) = &repo.commits {
            let names = env
                .run(
                    fork_name,
                    RunAs::Account,
                    &scripts::git_changed(&repo.repo, old, new),
                )?
                .success("Listing the fork's changed files")?;
            if names.lines().any(is_migration) {
                migrations_changed = true;
            }
        }
    }

    // Uncommitted files main also has uncommitted: equal content is fine.
    if !plan.dirty_to_verify.is_empty() {
        let theirs = fingerprints(env, fork_name, os, &plan.dirty_to_verify)?;
        let ours = fingerprints(env, target, os, &plan.dirty_to_verify)?;
        for path in &plan.dirty_to_verify {
            if theirs.get(path) != ours.get(path) {
                plan.conflicts.push(Conflict {
                    kind: "file".into(),
                    item: path.clone(),
                    detail: "uncommitted on main and in the fork, with different content".into(),
                });
            }
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
        let untouched = base.cron.get(user) == main.cron.get(user);
        let merged = if untouched {
            if theirs.lines().eq(ours.lines()) {
                Merge::Unchanged
            } else {
                Merge::Merged(theirs.clone())
            }
        } else {
            match store.fork_time_crontab(fork_name, user)? {
                Some(before) => cron::merge(&before, &theirs, &ours),
                None => cron::merge("", &theirs, &ours),
            }
        };
        match merged {
            Merge::Unchanged => {}
            Merge::Merged(text) => cron_texts.push((user.clone(), text)),
            Merge::Conflict(lines) => plan.conflicts.push(Conflict {
                kind: "crontab".into(),
                item: user.clone(),
                detail: format!("main and the fork both changed: {}", lines.join(" | ")),
            }),
        }
    }
    plan.cron
        .retain(|(user, _)| cron_texts.iter().any(|(u, _)| u == user));

    // Git: move each fork's history to main as a bundle and test the merge there.
    let mut bundles = Vec::new();
    for (index, repo) in plan.repos.iter().enumerate() {
        if repo.commits.is_none() && !repo.new {
            continue;
        }
        let path = format!("/tmp/silo-keep-{index}.bundle");
        env.run(
            fork_name,
            RunAs::Account,
            &scripts::git_bundle(&repo.repo, &path),
        )?
        .success("Bundling the fork's commits")?;
        let (data, _) = env.read(fork_name, &path)?;
        env.write(target, &path, &data, "644")?;
        let state = if repo.new {
            "new".to_owned()
        } else {
            let out = env
                .run(
                    target,
                    RunAs::Account,
                    &scripts::git_precheck(&repo.repo, &path),
                )?
                .success(&format!("Checking the merge in {}", repo.repo))?;
            let mut lines = out.lines();
            let state = lines.next().unwrap_or("unsupported").trim().to_owned();
            if state == "conflict" {
                for line in lines {
                    plan.conflicts.push(Conflict {
                        kind: "git".into(),
                        item: repo.repo.clone(),
                        detail: line.trim().to_owned(),
                    });
                }
            }
            state
        };
        bundles.push(Bundle {
            repo: repo.repo.clone(),
            path,
            state,
        });
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
                fork: fork.clone(),
                main: main.clone(),
                os,
                report,
                cron,
                bundles,
                migrations_changed,
            }
        };

    if !plan.conflicts.is_empty() {
        let conflicts = plan.conflicts.clone();
        let message =
            format!(
            "Divergent keep: {} changed on {target} and in {fork_name}. {target} was not changed.",
            conflicts.iter().map(|c| c.item.clone()).collect::<Vec<_>>().join(", ")
        );
        let prepared = ready(plan, cron_texts, bundles, report.clone());
        cleanup(env, &prepared);
        report.notes.extend(prepared.report.notes);
        return Ok(refuse(
            report,
            "divergent",
            message,
            conflicts,
            "Bring main's change into the fork and keep again, or drop the part of the fork that clashes.",
        ));
    }
    if !plan.databases.is_empty() && !migrations_changed && request.hooks.migrate.is_empty() {
        let message = format!(
            "Database changed ({}) and nothing migrates it. {target} was not changed.",
            plan.databases.join(", ")
        );
        let prepared = ready(plan, cron_texts, bundles, report.clone());
        cleanup(env, &prepared);
        return Ok(refuse(
            report,
            "database",
            message,
            Vec::new(),
            "Add a migration, or a migrate: step in the recipe, so the keep can carry the database change. Otherwise use \"Make this main\": the fork becomes main with its database.",
        ));
    }
    Ok(Prepared::Ready(ready(plan, cron_texts, bundles, report)))
}

/// Removes the bundle files the preparation left on both computers.
pub fn cleanup(env: &dyn KeepEnv, prepared: &Ready) {
    for bundle in &prepared.bundles {
        for computer in [&prepared.report.fork, &prepared.report.target] {
            let _ = env.run(computer, RunAs::Root, &scripts::file_remove(&bundle.path));
        }
    }
}

/// path -> sha256 or `L:target`, for the paths that exist.
pub fn fingerprints(
    env: &dyn KeepEnv,
    computer: &str,
    os: Os,
    paths: &[String],
) -> Result<BTreeMap<String, String>, String> {
    if paths.is_empty() {
        return Ok(BTreeMap::new());
    }
    let out = env
        .run(computer, RunAs::Root, &scripts::fingerprint(os, paths))?
        .success("Comparing files")?;
    Ok(out
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(path, value)| (path.to_owned(), value.trim().to_owned()))
        .collect())
}
