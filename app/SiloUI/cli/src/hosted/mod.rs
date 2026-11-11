//! The hosted reviewer (a Pro feature): sends the diff document of a fork to a service that
//! reads it through three lenses and returns findings. Off until `reviewer.url` of
//! `.silo/silo.yaml` or `SILO_REVIEWER_URL` is set. Only the diff document goes out
//! ([`document::review_document`]); the answer is filtered for wording before it is shown.

pub mod codediff;
pub mod document;
pub mod filter;

use std::time::Duration;

use serde_json::{json, Value};

use crate::{
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
/// The most of the service's answer that is read.
pub const MAX_ANSWER_BYTES: u64 = 1024 * 1024;
pub const TIMEOUT: Duration = Duration::from_secs(90);
const LENSES: &[&str] = &["general", "packages", "code"];
const MAX_FINDINGS: usize = 50;
const MAX_TEXT: usize = 800;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reviewer {
    pub url: String,
    pub token: Option<String>,
    /// Things worth telling the person, such as a project URL that was ignored.
    pub notes: Vec<String>,
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

/// `https`, or `http` to this device only; no user name or password in the address.
pub fn check_url(text: &str) -> Result<String, CliError> {
    let url = reqwest::Url::parse(text.trim())
        .map_err(|_| CliError::new("The reviewer URL is not a valid address."))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(CliError::new(
            "The reviewer URL must not contain a user name or password.",
        ));
    }
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    match url.scheme() {
        "https" => {}
        "http" if local => {}
        _ => {
            return Err(CliError::new(
                "The reviewer URL must start with https:// (http:// is only for localhost, 127.0.0.1 and [::1]).",
            ))
        }
    }
    Ok(text.trim().to_owned())
}

/// `~/.config/silo/reviewer.yaml`. The token is never kept there: it is SILO_REVIEWER_TOKEN.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewerSettings {
    #[serde(default)]
    url: Option<String>,
}

/// The URL in `~/.config/silo/reviewer.yaml` (`url: https://...`), if the file exists.
fn user_url() -> Result<Option<String>, CliError> {
    let Some(home) = std::env::var_os("HOME") else {
        return Ok(None);
    };
    let path = std::path::Path::new(&home).join(".config/silo/reviewer.yaml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let settings: ReviewerSettings = serde_yaml_ng::from_str(&text)
        .map_err(|error| CliError::new(format!("{}: {error}", path.display())))?;
    Ok(settings.url)
}

impl Reviewer {
    /// The reviewer `SILO_REVIEWER_URL` or the user-level file names.
    pub fn resolve() -> Result<Self, CliError> {
        let from_env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let url = match from_env("SILO_REVIEWER_URL") {
            Some(url) => url,
            None => user_url()?.ok_or_else(|| {
                CliError::new(
                    "The hosted reviewer is off. Set SILO_REVIEWER_URL (or url: in ~/.config/silo/reviewer.yaml) and SILO_REVIEWER_TOKEN.",
                )
            })?,
        };
        Ok(Self {
            url: check_url(&url)?,
            token: from_env("SILO_REVIEWER_TOKEN"),
            notes: Vec::new(),
        })
    }

    /// The review address: the URL's path with `v1/review` added, and its query kept.
    pub fn endpoint(&self) -> Result<reqwest::Url, CliError> {
        let mut url = reqwest::Url::parse(&self.url)
            .map_err(|_| CliError::new("The reviewer URL is not a valid address."))?;
        url.set_fragment(None);
        url.path_segments_mut()
            .map_err(|_| CliError::new("The reviewer URL cannot take a path."))?
            .pop_if_empty()
            .extend(["v1", "review"]);
        Ok(url)
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
        .post(reviewer.endpoint()?)
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
    let mut body = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(response, MAX_ANSWER_BYTES + 1),
        &mut body,
    )
    .map_err(|error| CliError::new(format!("Reading the reviewer's answer failed: {error}")))?;
    if body.len() as u64 > MAX_ANSWER_BYTES {
        return Err(CliError::new("The reviewer's answer is larger than 1 MiB."));
    }
    serde_json::from_slice(&body)
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
        if entry.get("error").is_some_and(|e| !e.is_null()) {
            out.insert(
                lens.clone(),
                json!({"summary": "this lens could not be produced", "findings": [], "error": true}),
            );
            continue;
        }
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
                    "file": cut(finding["file"].as_str().unwrap_or_default()),
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
    target: &str,
    lenses: &[String],
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let reviewer = Reviewer::resolve()?;
    let (mut document, hidden, code_sent) = if is_request_id(target) {
        let shown = review::show(engine, target, progress).await?;
        match shown["diff"].as_object() {
            Some(_) => {
                let name = shown["forkName"].as_str().unwrap_or_default();
                with_code(engine, name, &shown["diff"], lenses).await
            }
            None => {
                let (document, dropped) =
                    document::text_document(shown["text"].as_str().unwrap_or_default(), "");
                (document, dropped, false)
            }
        }
    } else {
        let args = DiffArgs {
            computer: target.to_owned(),
            max_lines: None,
            no_baseline: false,
            compute_baseline: false,
        };
        let diff = ops::diff(engine, &args, progress).await?;
        with_code(engine, target, &diff, lenses).await
    };
    if !document.is_object() {
        document = json!({});
    }
    let (reviewer_for_task, lenses_for_task) = (reviewer.clone(), lenses.to_vec());
    let answer =
        tokio::task::spawn_blocking(move || send(&reviewer_for_task, &document, &lenses_for_task))
            .await
            .map_err(|error| CliError::new(error.to_string()))??;
    let mut result = shape(&answer, lenses);
    result["target"] = json!(target);
    result["codeContentSent"] = json!(code_sent);
    result["notes"] = json!(reviewer.notes);
    result["credentialFilesLeftOut"] = json!(hidden);
    Ok(result)
}

/// The review document of `diff`, with the code lens's diff text added when it is asked for.
async fn with_code(
    engine: &Engine,
    computer: &str,
    diff: &Value,
    lenses: &[String],
) -> (Value, usize, bool) {
    let (mut document, hidden) = document::review_document(diff);
    let mut sent = false;
    if lenses.iter().any(|lens| lens == "code") {
        let home = diff["home"].as_str().unwrap_or_default();
        let text = codediff::read(engine, computer, diff, home).await;
        if !text.is_empty() {
            document["code_diff"] = json!(text);
            sent = true;
        }
    }
    (document, hidden, sent)
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
    if value["codeContentSent"] == true {
        out.push_str("\nCode changes (text of the diff, with credential files and token-shaped strings removed) were sent to the reviewer.\n");
    }
    for note in value["notes"].as_array().into_iter().flatten() {
        out.push_str(&format!("\nNote: {}\n", note.as_str().unwrap_or_default()));
    }
    match value["filtered"].as_u64().unwrap_or(0) {
        0 => {}
        n => out.push_str(&format!(
            "\n{n} word(s) in the service's answer were replaced.\n"
        )),
    }
    out.trim_end().to_owned()
}
