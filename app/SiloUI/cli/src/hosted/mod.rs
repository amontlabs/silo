//! The hosted reviewer (a Pro feature): sends the diff document of a fork to a service that
//! reads it through three lenses and returns findings. Off until `reviewer.url` of
//! `.silo/silo.yaml` or `SILO_REVIEWER_URL` is set. Only the diff document goes out
//! ([`document::review_document`]); the answer is filtered for wording before it is shown.

pub mod document;
pub mod filter;

use std::time::Duration;

use serde_json::{json, Value};

use crate::{
    config::Settings,
    engine::{Engine, Progress},
    error::CliError,
    ops::{self, DiffArgs},
    review,
};

/// Printed above every review.
pub const BANNER: &str =
    "Advisory review of the diff only. It can miss things; you decide what to keep.";
/// The most the request body may hold.
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
/// How long the service has to answer.
pub const TIMEOUT: Duration = Duration::from_secs(90);
const LENSES: &[&str] = &["general", "packages", "code"];
const MAX_FINDINGS: usize = 50;
const MAX_TEXT: usize = 800;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reviewer {
    pub url: String,
    pub token: Option<String>,
}

/// `general`, `packages`, `code` or `all`, as the lenses to ask for.
pub fn parse_lenses(text: Option<&str>) -> Result<Vec<String>, CliError> {
    match text.unwrap_or("all") {
        "all" => Ok(LENSES.iter().map(|lens| (*lens).to_owned()).collect()),
        one if LENSES.contains(&one) => Ok(vec![one.to_owned()]),
        other => Err(CliError::new(format!(
            "{other} is not a lens. Use general, packages, code or all."
        ))),
    }
}

impl Reviewer {
    /// The reviewer the environment or the project settings name; an error when neither does.
    pub fn resolve(settings: Option<&Settings>) -> Result<Self, CliError> {
        let from_env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let url = from_env("SILO_REVIEWER_URL")
            .or_else(|| settings.and_then(|s| s.reviewer.as_ref()).and_then(|r| r.url.clone()))
            .ok_or_else(|| {
                CliError::new(
                    "The hosted reviewer is off. Set reviewer.url in .silo/silo.yaml or SILO_REVIEWER_URL, and SILO_REVIEWER_TOKEN for its token.",
                )
            })?;
        let url = url.trim().to_owned();
        let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"]
            .iter()
            .any(|prefix| url.starts_with(prefix));
        if !url.starts_with("https://") && !local {
            return Err(CliError::new(
                "The reviewer URL must start with https:// (http:// is only for this device).",
            ));
        }
        Ok(Self {
            url,
            token: from_env("SILO_REVIEWER_TOKEN"),
        })
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/review", self.url.trim_end_matches('/'))
    }
}

/// Sends `document` and returns the raw answer.
fn send(reviewer: &Reviewer, document: &Value, lenses: &[String]) -> Result<Value, CliError> {
    let body = serde_json::to_vec(&json!({"diff": document, "lenses": lenses}))?;
    if body.len() > MAX_REQUEST_BYTES {
        return Err(CliError::new(format!(
            "This diff is too large to review ({} KiB; the limit is {} KiB). Review a smaller change.",
            body.len() / 1024,
            MAX_REQUEST_BYTES / 1024
        )));
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| CliError::new(error.to_string()))?;
    let mut request = client
        .post(reviewer.endpoint())
        .header("Content-Type", "application/json")
        .body(body);
    if let Some(token) = &reviewer.token {
        request = request.bearer_auth(token);
    }
    let response = request.send().map_err(|error| {
        let what = if error.is_timeout() {
            "did not answer in time"
        } else {
            "could not be reached"
        };
        CliError::new(format!("The reviewer {what}: {}", error.without_url()))
    })?;
    let status = response.status();
    if !status.is_success() {
        let hint = match status.as_u16() {
            401 | 403 => " Check SILO_REVIEWER_TOKEN.",
            413 => " The diff is larger than the service accepts.",
            _ => "",
        };
        return Err(CliError::new(format!(
            "The reviewer answered {status}.{hint}"
        )));
    }
    response
        .json::<Value>()
        .map_err(|_| CliError::new("The reviewer's answer was not the expected JSON."))
}

fn cut(text: &str) -> String {
    text.chars().take(MAX_TEXT).collect()
}

/// The answer reduced to the lenses asked for, with the wording filter applied; the number
/// of filtered words rides along as `filtered`.
pub fn shape(answer: &Value, lenses: &[String]) -> Value {
    let mut filtered = 0;
    let mut clean = |text: &str| {
        let (text, found) = filter::clean(&cut(text));
        filtered += found;
        text
    };
    let mut out = serde_json::Map::new();
    for lens in lenses {
        let entry = &answer["lenses"][lens.as_str()];
        if !entry.is_object() {
            out.insert(
                lens.clone(),
                json!({"summary": "No answer for this lens.", "findings": []}),
            );
            continue;
        }
        let findings: Vec<Value> = entry["findings"]
            .as_array()
            .into_iter()
            .flatten()
            .take(MAX_FINDINGS)
            .map(|finding| {
                let severity = match finding["severity"].as_str().map(str::to_lowercase) {
                    Some(s) if ["info", "low", "medium", "high"].contains(&s.as_str()) => s,
                    _ => "info".to_owned(),
                };
                json!({
                    "severity": severity,
                    "file": clean(finding["file"].as_str().unwrap_or_default()),
                    "message": clean(finding["message"].as_str().unwrap_or_default()),
                })
            })
            .collect();
        let summary = clean(entry["summary"].as_str().unwrap_or_default());
        out.insert(
            lens.clone(),
            json!({"summary": summary, "findings": findings}),
        );
    }
    json!({"advisory": BANNER, "lenses": out, "filtered": filtered})
}

/// Reviews `target`: a request id (`fr-` and 8 hex digits) or a computer.
pub async fn review_target(
    engine: &Engine,
    settings: Option<&Settings>,
    target: &str,
    lenses: &[String],
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let reviewer = Reviewer::resolve(settings)?;
    let (document, hidden) = if is_request_id(target) {
        let shown = review::show(engine, target, progress).await?;
        match shown["diff"].as_object() {
            Some(_) => document::review_document(&shown["diff"]),
            None => (
                document::text_document(shown["text"].as_str().unwrap_or_default(), ""),
                0,
            ),
        }
    } else {
        let args = DiffArgs {
            computer: target.to_owned(),
            max_lines: None,
            no_baseline: false,
            compute_baseline: false,
        };
        document::review_document(&ops::diff(engine, &args, progress).await?)
    };
    let (reviewer_for_task, lenses_for_task) = (reviewer.clone(), lenses.to_vec());
    let answer =
        tokio::task::spawn_blocking(move || send(&reviewer_for_task, &document, &lenses_for_task))
            .await
            .map_err(|error| CliError::new(error.to_string()))??;
    let mut result = shape(&answer, lenses);
    result["target"] = json!(target);
    result["credentialFilesLeftOut"] = json!(hidden);
    Ok(result)
}

fn is_request_id(text: &str) -> bool {
    text.strip_prefix("fr-")
        .is_some_and(|rest| rest.len() == 8 && rest.chars().all(|c| c.is_ascii_hexdigit()))
}

/// The review as text: the banner, then each lens with its findings.
pub fn render(value: &Value) -> String {
    let mut out = format!("{}\n", value["advisory"].as_str().unwrap_or(BANNER));
    for lens in LENSES {
        let entry = &value["lenses"][*lens];
        if !entry.is_object() {
            continue;
        }
        out.push_str(&format!(
            "\n{lens}: {}\n",
            entry["summary"].as_str().unwrap_or_default()
        ));
        for finding in entry["findings"].as_array().into_iter().flatten() {
            let file = finding["file"].as_str().filter(|f| !f.is_empty());
            out.push_str(&format!(
                "  [{}] {}{}\n",
                finding["severity"].as_str().unwrap_or("info"),
                file.map(|f| format!("{f}: ")).unwrap_or_default(),
                finding["message"].as_str().unwrap_or_default()
            ));
        }
    }
    match value["filtered"].as_u64().unwrap_or(0) {
        0 => {}
        n => out.push_str(&format!(
            "\n{n} word(s) in the service's answer were filtered out.\n"
        )),
    }
    out.trim_end().to_owned()
}
