//! The changing part of a keep, in order: packages, git, files, crontabs and launch agents,
//! then the recipe's hooks (migrate, restart, after). It writes only what `prepare` read from
//! the fork. The first failure stops it; the caller puts main back from the before-keep
//! checkpoint.

use std::{thread, time::Duration};

use super::{
    env::{KeepEnv, Live, Os, RunAs},
    plan::FileAction,
    prepare::Ready,
    scripts, Request,
};

#[derive(Debug, Default)]
pub struct Applied {
    pub steps: Vec<String>,
    pub notes: Vec<String>,
}

/// How long a restarted service gets to show a new process id.
const RESTART_CHECKS: u32 = 20;
const RESTART_PAUSE: Duration = Duration::from_millis(if cfg!(test) { 1 } else { 500 });

pub fn apply(
    env: &dyn KeepEnv,
    ready: &Ready,
    request: &Request,
    target: &str,
) -> Result<Applied, String> {
    let os = ready.os;
    let mut done = Applied::default();
    let plan = &ready.plan;

    // Packages: removals the fork made, then installs, each manager as the user it belongs to.
    let mut managers: Vec<&str> = plan.packages.iter().map(|p| p.manager.as_str()).collect();
    managers.dedup();
    for manager in managers {
        let specs: Vec<String> = plan
            .packages
            .iter()
            .filter(|p| p.manager == manager)
            .map(|p| match manager {
                "dpkg" => format!("{}={}", p.name, p.version),
                "npm" => format!("{}@{}", p.name, p.version),
                "pip" | "pipx" => format!("{}=={}", p.name, p.version),
                "cargo" => format!("{}={}", p.name, p.version),
                _ => p.name.clone(),
            })
            .collect();
        env.note(&format!("Installing {manager} packages"));
        env.run(
            target,
            scripts::pkg_runner(os, manager),
            &scripts::pkg_install(os, manager, &specs)?,
        )?
        .success(&format!("Installing {manager} packages"))?;
        done.steps.push(format!(
            "packages {manager}: {}",
            plan.packages
                .iter()
                .filter(|p| p.manager == manager)
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut removing: Vec<&str> = plan.removals.iter().map(|p| p.manager.as_str()).collect();
    removing.dedup();
    for manager in removing {
        let names: Vec<String> = plan
            .removals
            .iter()
            .filter(|p| p.manager == manager)
            .map(|p| p.name.clone())
            .collect();
        env.run(
            target,
            scripts::pkg_runner(os, manager),
            &scripts::pkg_remove(os, manager, &names)?,
        )?
        .success(&format!("Removing {manager} packages"))?;
        done.steps
            .push(format!("packages {manager}: removed {}", names.join(", ")));
    }

    // Launch agents the fork removed stop first.
    for agent in plan.agents.iter().filter(|a| a.delete) {
        let _ = env.run(
            target,
            RunAs::Root,
            &scripts::agent_unload(&agent.scope, &agent.label),
        );
    }

    // Git.
    for repo in &plan.repos {
        if let Some(bundle) = ready.bundles.iter().find(|b| b.repo == repo.repo) {
            match (&bundle.branch, bundle.state.as_str()) {
                (_, "uptodate") => done.notes.push(format!(
                    "{}: main already has the fork's commits",
                    repo.repo
                )),
                (Some(branch), _) if branch.exists => done.notes.push(format!(
                    "{}: branch {} already holds the fork's commits",
                    repo.repo, branch.name
                )),
                (Some(branch), _) => {
                    let head = env
                        .run(
                            target,
                            RunAs::Account,
                            &scripts::git_branch(&repo.repo, &bundle.tmp_ref, &branch.name),
                        )?
                        .success(&format!("Creating branch {} in {}", branch.name, repo.repo))?;
                    done.steps.push(format!(
                        "git {}: the fork's commits are on branch {} at {}; nothing was merged or checked out",
                        repo.repo,
                        branch.name,
                        head.trim()
                    ));
                }
                (None, "new") => {
                    env.run(
                        target,
                        RunAs::Account,
                        &scripts::git_clone(&repo.repo, &bundle.path),
                    )?
                    .success(&format!("Cloning {}", repo.repo))?;
                    done.steps.push(format!("git {}: cloned", repo.repo));
                }
                (None, _) => {
                    let head = env
                        .run(
                            target,
                            RunAs::Account,
                            &scripts::git_merge(&repo.repo, &bundle.tmp_ref),
                        )?
                        .success(&format!("Merging {}", repo.repo))?;
                    done.steps
                        .push(format!("git {}: merged, now at {}", repo.repo, head.trim()));
                }
            }
        }
        for op in &repo.dirty {
            if op.delete {
                env.run(target, RunAs::Root, &scripts::file_remove(&op.path))?
                    .success("Removing a file")?;
            } else {
                write_staged(env, ready, target, &op.path)?;
            }
        }
        if !repo.dirty.is_empty() {
            done.steps.push(format!(
                "git {}: {} uncommitted changes carried",
                repo.repo,
                repo.dirty.len()
            ));
        }
    }

    // Files. Package replay may already have written some: write only what still differs.
    let copying: Vec<String> = plan
        .files
        .iter()
        .filter(|op| op.action == FileAction::Copy)
        .map(|op| op.path.clone())
        .collect();
    let now = env.inspect(target, &copying)?;
    let (mut changed, mut same) = (0, 0);
    for op in &plan.files {
        match &op.action {
            FileAction::Delete => {
                env.run(target, RunAs::Root, &scripts::file_remove(&op.path))?
                    .success("Removing a file")?;
            }
            FileAction::Link(link) => {
                env.run(target, RunAs::Root, &scripts::symlink(&op.path, link))?
                    .success("Recreating a link")?;
            }
            FileAction::Copy => {
                if now.get(&op.path) == ready.fork_live.get(&op.path) {
                    same += 1;
                    continue;
                }
                write_staged(env, ready, target, &op.path)?;
            }
        }
        changed += 1;
    }
    if !plan.files.is_empty() {
        done.steps.push(format!(
            "files: {changed} written, {same} already identical"
        ));
    }

    // Crontabs.
    for (user, text) in &ready.cron {
        env.run(target, RunAs::Root, &scripts::crontab_write(user, text))?
            .success("Writing the crontab")?;
        done.steps.push(format!("crontab {user}: merged"));
    }

    // Launch agents: a plist that cannot be loaded fails the keep.
    for agent in plan.agents.iter().filter(|a| !a.delete) {
        env.run(
            target,
            RunAs::Root,
            &scripts::agent_load(&agent.scope, &agent.label, &agent.path),
        )?
        .success(&format!("Loading launch agent {}", agent.label))?;
        done.steps
            .push(format!("launch agent {}: loaded", agent.label));
    }
    for agent in plan.agents.iter().filter(|a| a.delete) {
        done.steps
            .push(format!("launch agent {}: removed", agent.label));
    }

    // Hooks run in the one repo the keep touched, else main's only repo, else the home folder.
    let cwd = {
        let touched: Vec<&str> = plan.repos.iter().map(|r| r.repo.as_str()).collect();
        let repos: Vec<&str> = ready.main.git.keys().map(String::as_str).collect();
        match (touched.as_slice(), repos.as_slice()) {
            ([one], _) | ([], [one]) => (*one).to_owned(),
            _ => ready.main.home().unwrap_or("/").to_owned(),
        }
    };
    if request.hooks.migrate.is_empty() && ready.migrations_changed {
        done.notes.push(
            "Migrations changed; no migrate: step is set, so run them on main's database.".into(),
        );
    }
    for (index, command) in request.hooks.migrate.iter().enumerate() {
        run_hook(
            env,
            target,
            &cwd,
            request.hooks_hidden,
            "migrate",
            index,
            command,
            &mut done,
        )?;
    }

    // Restart services and check that each one is a new, running process.
    for service in &request.hooks.restart {
        let before = pid(env, target, os, service)?;
        env.note(&format!("Restarting {service}"));
        env.run(target, RunAs::Root, &scripts::service_restart(os, service))?
            .success(&format!("Restarting {service}"))?;
        let mut after = String::from("0");
        let mut good = false;
        for attempt in 0..RESTART_CHECKS {
            after = pid(env, target, os, service)?;
            let moved = after != "0" && (before == "0" || after != before);
            if moved
                && env
                    .run(target, RunAs::Root, &scripts::pid_alive(&after))?
                    .code
                    == 0
            {
                good = true;
                break;
            }
            if attempt + 1 < RESTART_CHECKS {
                thread::sleep(RESTART_PAUSE);
            }
        }
        if !good {
            return Err(format!(
                "{service} did not come back as a new running process (was {before}, now {after})."
            ));
        }
        done.steps
            .push(format!("restart {service}: pid {before} -> {after}"));
    }

    for (index, command) in request.hooks.after.iter().enumerate() {
        run_hook(
            env,
            target,
            &cwd,
            request.hooks_hidden,
            "after",
            index,
            command,
            &mut done,
        )?;
    }
    Ok(done)
}

/// Runs one `migrate` or `after` hook and records it. A hidden hook (one from a recipe, for a
/// caller that may not read it) is named by its step number, and a failure gives only the exit
/// code, never the command line or its output.
#[allow(clippy::too_many_arguments)]
fn run_hook(
    env: &dyn KeepEnv,
    target: &str,
    cwd: &str,
    hidden: bool,
    kind: &str,
    index: usize,
    command: &str,
    done: &mut Applied,
) -> Result<(), String> {
    let number = index + 1;
    if !hidden {
        let verb = if kind == "migrate" {
            "Migrating"
        } else {
            "Running"
        };
        env.note(&format!("{verb}: {command}"));
        env.run(target, RunAs::Account, &scripts::hook(command, cwd))?
            .success(&format!("{kind} `{command}`"))?;
        done.steps.push(format!("{kind}: {command}"));
        return Ok(());
    }
    env.note(&format!("Running the recipe's {kind} step {number}"));
    let out = env
        .run(target, RunAs::Account, &scripts::hook(command, cwd))
        .map_err(|_| format!("The recipe's {kind} step {number} could not run."))?;
    if out.code != 0 {
        return Err(format!(
            "The recipe's {kind} step {number} failed (exit {}).",
            out.code
        ));
    }
    done.steps
        .push(format!("Ran the recipe's {kind} step {number}"));
    Ok(())
}

fn pid(env: &dyn KeepEnv, computer: &str, os: Os, service: &str) -> Result<String, String> {
    let out = env
        .run(computer, RunAs::Root, &scripts::service_pid(os, service))?
        .success("Reading a process id")?;
    let pid = out.trim();
    Ok(if pid.is_empty() {
        "0".into()
    } else {
        pid.to_owned()
    })
}

/// Writes bytes that `prepare` read from the fork.
fn write_staged(env: &dyn KeepEnv, ready: &Ready, target: &str, path: &str) -> Result<(), String> {
    let data = ready
        .staged
        .get(path)
        .ok_or_else(|| format!("{path} was not read from the fork."))?;
    let mode = match ready.fork_live.get(path) {
        Some(Live::File { mode, .. }) => mode.as_str(),
        _ => "644",
    };
    env.write(target, path, data, mode)
}
