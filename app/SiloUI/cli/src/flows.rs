//! Keep, undo and review requests as the commands and the MCP tools use them: they tell a
//! fork made from a host folder from any other fork.

use std::path::Path;

use serde_json::{json, Value};
use silo_api::methods::{requests, review_requests::bucket_counts};

use crate::{
    engine::{Engine, Progress},
    error::CliError,
    folder,
    ops::{self, DiffArgs, MergeOptions},
    pr::{self, PrArgs, PrEnv},
    summary,
};

pub enum Mode {
    /// Merge the changes (or check them, with `dry_run`).
    Merge,
    /// Ask for a review instead of merging.
    Request,
    /// Push the code to a new branch and open a pull request.
    PullRequest(PrArgs, PrEnv),
}

/// Runs `silo merge` / the MCP `merge` tool for one fork in the given mode.
pub async fn run(
    engine: &Engine,
    fork: &str,
    into: Option<&str>,
    options: MergeOptions,
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
        Mode::Request => request(engine, fork, into, progress).await,
        Mode::Merge => merge(engine, fork, into, options, progress).await,
    }
}

/// Merges a fork's changes. A fork made from a host folder brings its git commits back into
/// that folder; any other fork is merged into its source computer (or `into`).
pub async fn merge(
    engine: &Engine,
    fork: &str,
    into: Option<&str>,
    options: MergeOptions,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let computer = engine.computer(fork).await?;
    if let Some(record) = folder::record_for(engine, &computer.id) {
        if into.is_some() {
            return Err(CliError::new(format!(
                "{fork} was made from {}; it merges into that folder, so --into does not apply.",
                record.path
            )));
        }
        if !options.choices.is_empty() || !options.skip.is_empty() {
            return Err(CliError::new(format!(
                "{fork} was made from {}; its commits are merged into the folder by Git, so --choose and --skip do not apply.",
                record.path
            )));
        }
        let mut value = folder::fold_folder(
            engine,
            &computer,
            record,
            options.dry_run,
            options.reviewed.as_deref(),
            progress,
        )
        .await?;
        if value["status"] == "kept" {
            close_requests(engine, &computer.id, &mut value).await;
        }
        return Ok(value);
    }
    ops::merge(engine, fork, into, options, progress).await
}

/// After a folder merge, closes the review requests for the fork and its folder. The merge is
/// done whether or not this works, so a failure is a note in the result.
async fn close_requests(engine: &Engine, fork_id: &str, value: &mut Value) {
    let Some(record) = folder::record_for(engine, fork_id) else {
        return;
    };
    let closed = engine
        .call(requests::changes_requests_mark_kept(
            silo_api::methods::review_requests::ChangesRequestsMarkKeptParams {
                device_id: None,
                fork_id: fork_id.to_owned(),
                target: format!("folder:{}", record.path),
                keep_id: record.backup.map(|backup| backup.reference),
            },
        ))
        .await;
    if let Err(error) = closed {
        if let Some(notes) = value["report"]["notes"].as_array_mut() {
            notes.push(json!(format!(
                "The review request for this fork could not be closed: {error}"
            )));
        }
    }
}

/// Puts back what the latest merge changed: a fork's host folder (named by the fork or by the
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
            "No fork of {target} has been merged, so there is nothing to undo."
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
    let (digest, summary, target_id, counts) = match folder::record_for(engine, &computer.id) {
        Some(record) => (
            folder::review_digest(engine, &computer, &record).await?,
            folder::review_summary(engine, &computer, &record).await?,
            Some(format!("folder:{}", record.path)),
            None,
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
                Some(bucket_counts(&seen)),
            )
        }
    };
    let mut value = engine
        .call(requests::changes_request(
            silo_api::methods::keep::ChangesRequestParams {
                device_id: None,
                fork_id: computer.id.clone(),
                target_id,
                review_digest: (!digest.is_empty()).then_some(digest),
                summary: Some(summary),
                counts,
            },
        ))
        .await?;
    value["fork"] = json!(computer.name);
    value["status"] = json!("requested");
    Ok(value)
}
