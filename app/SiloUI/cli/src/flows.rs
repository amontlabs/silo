//! Keep, undo and review requests as the commands and the MCP tools use them: they tell a
//! fork made from a host folder from any other fork.

use std::path::Path;

use serde_json::{json, Value};
use silo_api::methods::{keep::KeepHooks, requests, review_requests::bucket_counts};

use crate::{
    engine::{Engine, Progress},
    error::CliError,
    folder,
    ops::{self, DiffArgs, FoldOptions},
};

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
                seen["text"].as_str().unwrap_or_default().to_owned(),
                target_id,
                Some(bucket_counts(&seen)),
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
                counts,
            },
        ))
        .await?;
    value["fork"] = json!(computer.name);
    value["status"] = json!("requested");
    Ok(value)
}
