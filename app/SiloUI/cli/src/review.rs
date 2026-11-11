//! Review requests as the commands and the MCP tools use them: list, show, merge, decline
//! and open the requests `silo merge --request` records. The engine does the work; a fork made
//! from a host folder is read here, because only the CLI reads the folder's git history.

use serde_json::{json, Value};
use silo_api::methods::{
    keep::KeepAnswer,
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

/// Every device's requests, each row with the device it is on (`deviceId`, `deviceName`), and
/// the devices that could not be read (`unreachable`).
pub async fn list_all(engine: &Engine, status: Option<RequestFilter>) -> Result<Value, CliError> {
    engine
        .on_device(None)
        .call(requests::changes_requests_list(ChangesRequestsListParams {
            status,
            device_id: None,
            all_devices: Some(true),
        }))
        .await
}

/// Where `id` was found: `None` is this device.
type Found = (Option<String>, String);

/// The one place a request is, or an error when two devices have the same id.
pub fn choose(found: &[Found], id: &str) -> Result<Option<String>, CliError> {
    match found {
        [] => Ok(None),
        [(device, _)] => Ok(device.clone()),
        many => Err(CliError::new(format!(
            "Request {id} is on {}. Use --device to say which.",
            many.iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>()
                .join(" and ")
        ))),
    }
}

/// The engine a request's actions go to: the device it is on. This device's own requests
/// and every connected device's are searched; an id found on more than one is refused.
/// Naming a device with `--device` skips the search.
pub async fn locate(engine: &Engine, id: &str) -> Result<Engine, CliError> {
    if engine.device().is_some() || !engine.routes_devices() {
        return Ok(engine.clone());
    }
    let has = |value: &Value| {
        value["requests"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|row| row["request"]["id"] == id)
    };
    let mut found: Vec<Found> = Vec::new();
    if has(&list(engine, Some(RequestFilter::All)).await?) {
        found.push((None, "this device".to_owned()));
    }
    let devices = engine.devices().await.unwrap_or_default();
    if !devices.is_empty() {
        if let Ok(merged) = list_all(engine, Some(RequestFilter::All)).await {
            for row in merged["requests"].as_array().into_iter().flatten() {
                let Some(device) = row["deviceId"].as_str() else {
                    continue;
                };
                let known = devices.iter().find(|(known, _)| known == device);
                let duplicate = found
                    .iter()
                    .any(|(seen, _)| seen.as_deref() == Some(device));
                if row["request"]["id"] == id && !duplicate {
                    if let Some((_, name)) = known {
                        found.push((Some(device.to_owned()), name.clone()));
                    }
                }
            }
        }
    }
    Ok(match choose(&found, id)? {
        Some(device) => engine.on_device(Some(device)),
        None => engine.clone(),
    })
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
    if from_folder && engine.device().is_none() {
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

/// Merges the request's fork into its target, pinned to the request's digest. A refused merge
/// is a result (`status: "refused"`), and the request stays open.
pub async fn merge(
    engine: &Engine,
    id: &str,
    data_ok: bool,
    choices: std::collections::BTreeMap<String, KeepAnswer>,
    skip: Vec<String>,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    engine
        .run_job(
            &format!("Merging {id}"),
            requests::changes_requests_keep(ChangesRequestsKeepParams {
                device_id: None,
                request_id: id.to_owned(),
                data_ok: data_ok.then_some(true),
                keep: None,
                choices: (!choices.is_empty()).then_some(choices),
                skip: (!skip.is_empty()).then_some(skip),
            }),
            progress,
        )
        .await
}

/// Drops the request's fork. Neither main nor the request's target is ever dropped.
pub async fn decline(engine: &Engine, id: &str, progress: Progress<'_>) -> Result<Value, CliError> {
    engine
        .run_job(
            &format!("Declining {id}"),
            requests::changes_requests_throw_away(id_params(id)),
            progress,
        )
        .await
}

/// Starts the request's fork when it is stopped and says how to reach it.
pub async fn open(engine: &Engine, id: &str, progress: Progress<'_>) -> Result<Value, CliError> {
    let mut value = engine
        .run_job(
            &format!("Opening {id}"),
            requests::changes_requests_open(id_params(id)),
            progress,
        )
        .await?;
    // The SSH name and the URLs are the other device's own: a loopback address or a name
    // from its SSH setup does not reach the fork from here.
    if engine.device().is_some() {
        if let Some(object) = value.as_object_mut() {
            object.remove("sshAlias");
            if let Some(urls) = object.remove("urls") {
                object.insert("urls".into(), json!([]));
                object.insert("deviceUrls".into(), urls);
            }
        }
    }
    Ok(value)
}
