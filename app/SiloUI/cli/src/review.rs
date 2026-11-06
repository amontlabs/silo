//! Review requests as the commands and the MCP tools use them: list, show, keep, throw away
//! and open the requests `silo fold --request` records. The engine does the work; a fork made
//! from a host folder is read here, because only the CLI reads the folder's git history.

use serde_json::{json, Value};
use silo_api::methods::{
    keep::KeepHooks,
    requests,
    review_requests::{
        ChangesRequestsKeepParams, ChangesRequestsListParams, RequestFilter, RequestIdParams,
    },
};

use crate::{
    engine::{Engine, Progress},
    error::CliError,
    folder,
};

/// `open`, `kept`, `thrown-away` or `all`.
pub fn parse_filter(text: &str) -> Result<RequestFilter, CliError> {
    match text {
        "open" => Ok(RequestFilter::Open),
        "kept" => Ok(RequestFilter::Kept),
        "thrown-away" => Ok(RequestFilter::ThrownAway),
        "all" => Ok(RequestFilter::All),
        other => Err(CliError::new(format!(
            "{other} is not a status. Use open, kept, thrown-away or all."
        ))),
    }
}

fn id_params(id: &str) -> RequestIdParams {
    RequestIdParams {
        device_id: None,
        request_id: id.to_owned(),
    }
}

/// The requests with the given status (default open), each with its fork's name and state.
pub async fn list(engine: &Engine, status: Option<RequestFilter>) -> Result<Value, CliError> {
    engine
        .call(requests::changes_requests_list(ChangesRequestsListParams {
            status,
            device_id: None,
            all_devices: None,
        }))
        .await
}

/// A request with a fresh diff of its fork and the credentials the fork used.
pub async fn show(engine: &Engine, id: &str, progress: Progress<'_>) -> Result<Value, CliError> {
    let mut value = engine
        .run_job(
            &format!("Looking at {id}"),
            requests::changes_requests_show(id_params(id)),
            progress,
        )
        .await?;
    let from_folder = value["request"]["target"]
        .as_str()
        .is_some_and(|target| target.starts_with("folder:"));
    if from_folder {
        let name = value["forkName"].as_str().unwrap_or_default().to_owned();
        let fork = engine.computer(&name).await?;
        if let Some(record) = folder::record_for(engine, &fork.id) {
            let digest = folder::review_digest(engine, &fork, &record).await?;
            let asked = value["request"]["reviewDigest"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            value["text"] = json!(folder::review_summary(engine, &fork, &record).await?);
            value["stale"] = json!(!asked.is_empty() && asked != digest);
            value["reviewDigest"] = json!(digest);
            value["fresh"] = json!(true);
        }
    }
    Ok(value)
}

/// Keeps the request's fork into its target, pinned to the request's digest. A refused keep is
/// a result (`status: "refused"`), and the request stays open.
pub async fn keep(
    engine: &Engine,
    id: &str,
    hooks: Option<KeepHooks>,
    data_ok: bool,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    engine
        .run_job(
            &format!("Keeping {id}"),
            requests::changes_requests_keep(ChangesRequestsKeepParams {
                device_id: None,
                request_id: id.to_owned(),
                data_ok: data_ok.then_some(true),
                keep: hooks,
            }),
            progress,
        )
        .await
}

/// Drops the request's fork. Neither main nor the request's target is ever dropped.
pub async fn throw_away(
    engine: &Engine,
    id: &str,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    engine
        .run_job(
            &format!("Throwing away {id}"),
            requests::changes_requests_throw_away(id_params(id)),
            progress,
        )
        .await
}

/// Starts the request's fork when it is stopped and says how to reach it.
pub async fn open(engine: &Engine, id: &str, progress: Progress<'_>) -> Result<Value, CliError> {
    engine
        .run_job(
            &format!("Opening {id}"),
            requests::changes_requests_open(id_params(id)),
            progress,
        )
        .await
}
