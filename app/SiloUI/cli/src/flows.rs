//! Keep, undo and review requests as the commands and the MCP tools use them: they tell a
//! fork made from a host folder from any other fork.

use std::path::Path;

use serde_json::{json, Value};
use silo_api::methods::{keep::KeepHooks, requests};

use crate::{
    autokeep::{self, Outcome},
    config::{Project, Recipe, Settings},
    engine::{Engine, Progress},
    error::CliError,
    folder,
    ops::{self, DiffArgs, FoldOptions},
    pr::{self, PrArgs, PrEnv},
    summary, writeback,
};

/// The memory file that records which recipe a computer was made from.
const RECIPE_MEMORY: &str = "cli-recipes.json";

/// Remembers that a computer was made from the named recipe (`silo up`, `silo fork --recipe`),
/// so a later keep into it knows which recipe file to write back to.
pub fn remember_recipe(engine: &Engine, computer_id: &str, recipe: &str) {
    let _ = engine.remember(RECIPE_MEMORY, computer_id, json!(recipe));
}

/// A fork comes from the same recipe as the computer it was copied from.
pub fn inherit_recipe(engine: &Engine, source_id: &str, fork_id: &str) {
    if let Some(name) = recipe_of(engine, source_id) {
        remember_recipe(engine, fork_id, &name);
    }
}

fn recipe_of(engine: &Engine, computer_id: &str) -> Option<String> {
    engine
        .recall(RECIPE_MEMORY, computer_id)
        .and_then(|value| value.as_str().map(str::to_owned))
}

/// What a command knows about the project when it keeps, asks or opens a pull request.
pub struct Context<'a> {
    pub project: &'a Project,
    /// The settings as they apply to the calling agent.
    pub settings: Option<&'a Settings>,
    /// The recipe whose `keep:` and `test:` apply.
    pub recipe: Option<&'a Recipe>,
    /// Append kept packages and settings files to the recipe the computer came from.
    pub write_back: bool,
}

pub enum Mode {
    /// Keep the changes (or check them, with `dry_run`).
    Keep,
    /// Ask for a review, keeping at once when an `auto_keep` rule covers every change.
    Request,
    /// Push the code to a new branch and open a pull request.
    PullRequest(PrArgs, PrEnv),
}

/// Runs `silo fold` / the MCP `fold` tool for one fork in the given mode.
pub async fn run(
    engine: &Engine,
    fork: &str,
    into: Option<&str>,
    context: &Context<'_>,
    options: FoldOptions,
    mode: Mode,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    match mode {
        Mode::PullRequest(args, env) => {
            if into.is_some() || options.dry_run {
                return Err(CliError::new(
                    "--pr opens a pull request from the fork's code; it cannot be combined with --into or --dry-run.",
                ));
            }
            pr::open(engine, fork, &args, &env, progress).await
        }
        Mode::Request => ask_or_keep(engine, fork, into, context, progress).await,
        Mode::Keep => {
            let id = engine.computer(fork).await?.id;
            let pinned = options
                .reviewed
                .clone()
                .or_else(|| engine.recall_review(&id));
            let dry_run = options.dry_run;
            let mut value = fold(
                engine,
                fork,
                into,
                ops::keep_hooks(context.recipe),
                options,
                false,
                progress,
            )
            .await?;
            if context.write_back && !dry_run && value["status"] == "kept" {
                if let Some(report) =
                    write_back(engine, fork, &value, context, pinned, progress).await
                {
                    value["writeBack"] = report;
                }
            }
            Ok(value)
        }
    }
}

/// A review request, unless an `auto_keep` rule covers every change: then the changes are
/// kept at once and the result says which rule matched.
async fn ask_or_keep(
    engine: &Engine,
    fork: &str,
    into: Option<&str>,
    context: &Context<'_>,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    if Settings::auto_keep_rules(context.settings).is_empty() {
        return request(engine, fork, into, progress).await;
    }
    match autokeep::attempt(
        engine,
        fork,
        into,
        context.settings,
        context.recipe,
        progress,
    )
    .await?
    {
        Outcome::Kept(mut value) => {
            if context.write_back {
                if let Some(report) =
                    write_back(engine, fork, &value, context, None, progress).await
                {
                    value["writeBack"] = report;
                }
            }
            Ok(value)
        }
        Outcome::Nothing => {
            let mut value = request(engine, fork, into, progress).await?;
            value["autoKeep"] = json!({ "declined": "nothing changed" });
            Ok(value)
        }
        Outcome::Declined(reason) => {
            let mut value = request(engine, fork, into, progress).await?;
            value["autoKeep"] = json!({ "declined": reason });
            Ok(value)
        }
    }
}

/// At the end of `silo fork ... -- cmd` with `auto_keep` rules set: keep a fork whose command
/// succeeded when a rule covers every change, else leave a review request. `None` when no
/// rules are set or nothing changed.
pub async fn after_command(
    engine: &Engine,
    fork: &str,
    command_ok: bool,
    context: &Context<'_>,
    progress: Progress<'_>,
) -> Option<Value> {
    if Settings::auto_keep_rules(context.settings).is_empty() {
        return None;
    }
    let outcome = if command_ok {
        ask_or_keep(engine, fork, None, context, progress).await
    } else {
        request(engine, fork, None, progress)
            .await
            .map(|mut value| {
                value["autoKeep"] = json!({ "declined": "the command failed" });
                value
            })
    };
    let mut value = match outcome {
        Ok(value) => value,
        Err(error) => return Some(json!({ "fork": fork, "error": error.message })),
    };
    if value["autoKeep"]["declined"] == "nothing changed" {
        return None;
    }
    value["fork"] = json!(fork);
    Some(value)
}

/// After a successful keep: appends the packages and settings files it brought to the recipe
/// file the target came from. The report says what was added; `None` when the keep brought
/// neither or the computer has no recipe file.
async fn write_back(
    engine: &Engine,
    fork: &str,
    kept: &Value,
    context: &Context<'_>,
    pinned: Option<String>,
    progress: Progress<'_>,
) -> Option<Value> {
    let directory = context.project.directory.as_ref()?;
    let computer = engine.computer(fork).await.ok()?;
    if folder::record_for(engine, &computer.id).is_some() {
        return None;
    }
    let target = kept["report"]["target"].as_str().unwrap_or_default();
    let named = recipe_of(engine, target).or_else(|| recipe_of(engine, &computer.id));
    let path = match named {
        Some(name) => Some(directory.join("recipes").join(format!("{name}.yaml"))),
        None => context.project.recipe.clone(),
    }
    .filter(|path| path.is_file())?;
    let seen = ops::diff(
        engine,
        &DiffArgs {
            computer: computer.name.clone(),
            max_lines: None,
            no_baseline: false,
            compute_baseline: false,
        },
        progress,
    )
    .await
    .ok()?;
    let wanted = writeback::additions(&seen);
    if wanted.is_empty() {
        return None;
    }
    if let Some(pinned) = pinned {
        if seen["reviewDigest"].as_str() != Some(pinned.as_str()) {
            return Some(json!({
                "file": path.display().to_string(),
                "error": "the fork changed after it was reviewed; the recipe was not changed",
                "written": false,
            }));
        }
    }
    Some(match writeback::apply(&path, &wanted) {
        Ok(report) => report,
        Err(error) => json!({
            "file": path.display().to_string(),
            "error": error.message,
            "written": false,
        }),
    })
}

/// Keeps a fork's changes. A fork made from a host folder brings its git commits back into
/// that folder; any other fork is kept into its source computer. With `request_review`,
/// nothing is kept: the fork is recorded as waiting for a review.
pub async fn fold(
    engine: &Engine,
    fork: &str,
    into: Option<&str>,
    keep: Option<KeepHooks>,
    options: FoldOptions,
    request_review: bool,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    if request_review {
        return request(engine, fork, into, progress).await;
    }
    let computer = engine.computer(fork).await?;
    if let Some(record) = folder::record_for(engine, &computer.id) {
        if into.is_some() {
            return Err(CliError::new(format!(
                "{fork} was made from {}; it keeps into that folder, so --into does not apply.",
                record.path
            )));
        }
        return folder::fold_folder(engine, &computer, record, options.dry_run, progress).await;
    }
    ops::fold(engine, fork, into, keep, options, progress).await
}

/// Puts back what the latest keep changed: a fork's host folder (named by the fork or by the
/// folder's path), else a computer.
pub async fn undo(
    engine: &Engine,
    target: &str,
    base: &Path,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    if let Some(record) = folder::undo_target(engine, target, base).await {
        return folder::undo_folder(engine, record).await;
    }
    if folder::is_folder(target) {
        return Err(CliError::new(format!(
            "No fork of {target} has been folded, so there is nothing to undo."
        )));
    }
    ops::undo(engine, target, progress).await
}

/// Records that a fork waits for a review, without keeping it, and returns the request. The
/// request carries the digest and the readable summary of the fork's changes now. A fork made
/// from a host folder has the folder as its target (`folder:<path>`).
pub async fn request(
    engine: &Engine,
    fork: &str,
    into: Option<&str>,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let computer = engine.computer(fork).await?;
    let (digest, summary, target_id) = match folder::record_for(engine, &computer.id) {
        Some(record) => (
            folder::review_digest(engine, &computer, &record).await?,
            folder::review_summary(engine, &computer, &record).await?,
            Some(format!("folder:{}", record.path)),
        ),
        None => {
            let seen = ops::diff(
                engine,
                &DiffArgs {
                    computer: computer.name.clone(),
                    max_lines: None,
                    no_baseline: false,
                    compute_baseline: false,
                },
                progress,
            )
            .await?;
            let target_id = match into {
                Some(name) => Some(engine.computer(name).await?.id),
                None => None,
            };
            (
                seen["reviewDigest"].as_str().unwrap_or_default().to_owned(),
                summary::review(&seen),
                target_id,
            )
        }
    };
    let mut value = engine
        .call(requests::changes_request(
            silo_api::methods::keep::ChangesRequestParams {
                fork_id: computer.id.clone(),
                target_id,
                review_digest: (!digest.is_empty()).then_some(digest),
                summary: Some(summary),
            },
        ))
        .await?;
    value["fork"] = json!(computer.name);
    value["status"] = json!("requested");
    Ok(value)
}
