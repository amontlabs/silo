//! `silo approvals`: the requests computers' agents made that wait for your answer. Listing,
//! showing, allowing and denying them; allowing runs the request's call in the engine.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use silo_api::methods::{
    approvals::{
        ApprovalDecision, ApprovalIdParams, ApprovalState, ApprovalsDecideParams,
        ApprovalsListParams, ApprovalsNoteParams,
    },
    requests,
};

use crate::{
    engine::Engine,
    error::{exit, CliError},
};

/// How long a command waits for the owner to answer a request it made, unless told otherwise.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(120);

/// How a command that gets `approval_required` behaves: it says what was asked, then waits.
#[derive(Clone, Debug)]
pub struct WaitOptions {
    /// The longest to wait; zero does not wait at all.
    pub wait: Duration,
    /// Added to the request as the caller's own note right after asking.
    pub reason: Option<String>,
    /// Say on standard error what was asked and that the command is waiting.
    pub announce: bool,
    /// How often the request is looked at.
    pub poll: Duration,
}

impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            wait: DEFAULT_WAIT,
            reason: None,
            announce: true,
            poll: Duration::from_millis(1500),
        }
    }
}

/// Parses `90`, `90s`, `2m` or `1h30m` into a duration.
pub fn parse_duration(text: &str) -> Result<Duration, CliError> {
    let bad = || CliError::new(format!("`{text}` is not a duration like 90s, 2m or 1h."));
    let text = text.trim();
    if text.is_empty() {
        return Err(bad());
    }
    if let Ok(seconds) = text.parse::<u64>() {
        return Ok(Duration::from_secs(seconds));
    }
    let mut total = 0u64;
    let mut digits = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let amount: u64 = digits.parse().map_err(|_| bad())?;
        digits.clear();
        total += amount
            * match c {
                's' => 1,
                'm' => 60,
                'h' => 3600,
                _ => return Err(bad()),
            };
    }
    if !digits.is_empty() {
        return Err(bad());
    }
    Ok(Duration::from_secs(total))
}

fn state_of(approval: &Value) -> &str {
    approval
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// The exit code that stands for where a request is: done, denied, waiting or ended.
pub fn exit_for(state: &str) -> i32 {
    match state {
        "allowed" => 0,
        "denied" => exit::DENIED,
        "pending" => exit::WAITING,
        _ => exit::ENDED,
    }
}

/// What a call that asked for an answer returns once the owner allowed it: the job it started,
/// or its result.
fn allowed_result(approval: &Value) -> Result<Option<Value>, CliError> {
    let outcome = &approval["outcome"];
    if !outcome.is_object() {
        return Ok(None);
    }
    if let Some(error) = outcome.get("error").and_then(Value::as_str) {
        return Err(CliError::new(format!(
            "You allowed it, but it did not run: {}",
            silo_api::plain_text(error)
        )));
    }
    Ok(Some(match outcome.get("jobId").and_then(Value::as_str) {
        Some(job) => json!({ "jobId": job }),
        None => outcome.get("result").cloned().unwrap_or(Value::Null),
    }))
}

fn waiting_error(id: &str, summary: &str) -> CliError {
    CliError::new(format!(
        "Still waiting for the owner to answer request {id} ({summary}). Nothing has run. Check it later with `silo approvals status {id}`, or wait for it with `silo approvals wait {id}`."
    ))
    .with_exit(exit::WAITING)
    .for_approval(id)
}

/// The error a request that did not end in an allow becomes.
fn ended_error(approval: &Value) -> CliError {
    let id = text(approval, "id");
    let summary = text(approval, "summary");
    match state_of(approval) {
        "denied" => {
            let mut message = format!("The owner denied this: {summary}");
            if let Some(note) = approval.get("decisionNote").and_then(Value::as_str) {
                message.push_str(&format!(" Their note: {}", plain(note)));
            }
            CliError::new(message)
                .with_exit(exit::DENIED)
                .for_approval(id)
        }
        state => {
            let why = approval
                .get("reason")
                .and_then(Value::as_str)
                .map_or_else(|| format!("it was {state}"), plain);
            CliError::new(format!(
                "The request ended without an answer ({why}): {summary}"
            ))
            .with_exit(exit::ENDED)
            .for_approval(id)
        }
    }
}

/// Follows a request until the owner answers, or `options.wait` passes. An allow returns what
/// the call returns: the job it started (follow it as usual) or its result. A deny, an end and
/// a wait that ran out are errors with their own exit codes.
pub async fn follow(
    engine: &Engine,
    id: &str,
    summary: &str,
    options: &WaitOptions,
) -> Result<Value, CliError> {
    let summary = silo_api::plain_text(summary);
    if let Some(reason) = options.reason.as_deref().filter(|r| !r.trim().is_empty()) {
        let noted = engine
            .call_unattended(requests::approvals_note(ApprovalsNoteParams {
                id: id.to_owned(),
                note: reason.to_owned(),
            }))
            .await;
        if let Err(error) = noted {
            eprintln!("silo: could not add your reason to the request: {error}");
        }
    }
    if options.announce {
        eprintln!("This needs the owner's answer in Silo: {summary} (request {id}).");
    }
    if options.wait.is_zero() {
        return Err(waiting_error(id, &summary));
    }
    if options.announce {
        eprintln!(
            "Waiting up to {}s for the owner. Check later with `silo approvals status {id}`.",
            options.wait.as_secs()
        );
    }
    let deadline = Instant::now() + options.wait;
    loop {
        let approval = engine
            .call_unattended(requests::approvals_get(ApprovalIdParams {
                id: id.to_owned(),
            }))
            .await?;
        match state_of(&approval) {
            "pending" => {}
            "allowed" => {
                if let Some(result) = allowed_result(&approval)? {
                    return Ok(result);
                }
            }
            _ => return Err(ended_error(&approval)),
        }
        if Instant::now() >= deadline {
            return Err(waiting_error(id, &summary));
        }
        tokio::time::sleep(options.poll).await;
    }
}

/// `silo approvals wait`: the request as it stands once the owner answered, or after `wait`.
pub async fn wait(
    engine: &Engine,
    id: &str,
    wait: Duration,
    poll: Duration,
) -> Result<Value, CliError> {
    let deadline = Instant::now() + wait;
    loop {
        let approval = show(engine, id).await?;
        let state = state_of(&approval);
        let answered =
            state != "pending" && (state != "allowed" || approval["outcome"].is_object());
        if answered || Instant::now() >= deadline {
            return Ok(approval);
        }
        tokio::time::sleep(poll).await;
    }
}

/// `silo approvals note`: the caller's own words on why it asked.
pub async fn note(engine: &Engine, id: &str, note: &str) -> Result<Value, CliError> {
    engine
        .call(requests::approvals_note(ApprovalsNoteParams {
            id: id.to_owned(),
            note: note.to_owned(),
        }))
        .await
}

/// The pending approvals, newest first.
pub async fn list(engine: &Engine) -> Result<Value, CliError> {
    list_state(engine, Some(ApprovalState::Pending)).await
}

/// The approvals in `state` (any, when `None`), newest first. A computer's agent sees its own
/// and those of the computers it made.
pub async fn list_state(engine: &Engine, state: Option<ApprovalState>) -> Result<Value, CliError> {
    engine
        .call(requests::approvals_list(ApprovalsListParams {
            state,
            computer_id: None,
        }))
        .await
}

pub async fn show(engine: &Engine, id: &str) -> Result<Value, CliError> {
    engine
        .call(requests::approvals_get(ApprovalIdParams {
            id: id.to_owned(),
        }))
        .await
}

pub async fn decide(
    engine: &Engine,
    id: &str,
    decision: ApprovalDecision,
    note: Option<String>,
) -> Result<Value, CliError> {
    engine
        .call(requests::approvals_decide(ApprovalsDecideParams {
            id: id.to_owned(),
            decision,
            note,
        }))
        .await
}

/// A text field, made plain: whatever a computer's agent influenced is printed without its
/// control characters, terminal escapes and direction overrides.
fn text(value: &Value, key: &str) -> String {
    plain(value.get(key).and_then(Value::as_str).unwrap_or(""))
}

fn plain(text: &str) -> String {
    silo_api::plain_text(text)
}

pub fn render_list(value: &Value) -> String {
    let approvals = value["approvals"].as_array().cloned().unwrap_or_default();
    if approvals.is_empty() {
        return "No requests are waiting for your answer.".to_owned();
    }
    let mut out = String::new();
    for approval in &approvals {
        out.push_str(&format!(
            "{}  {}  {}\n",
            text(approval, "id"),
            text(approval, "computerName"),
            text(approval, "summary"),
        ));
    }
    out.push_str("Show one with `silo approvals show <id>`; answer with `silo approvals allow <id>` or `silo approvals deny <id>`.");
    out
}

pub fn render_show(approval: &Value) -> String {
    let mut out = format!(
        "{}  {}\nAsked by: {}\nAsks for: {}\n",
        text(approval, "id"),
        text(approval, "state"),
        text(approval, "computerName"),
        text(approval, "summary"),
    );
    // The agent's own words, never mixed with what Silo wrote.
    if let Some(note) = approval.get("note").and_then(Value::as_str) {
        out.push_str(&format!("The agent's note: {}\n", plain(note)));
    }
    out.push_str(&format!(
        "Call: {} {}\n",
        text(approval, "method"),
        plain(&serde_json::to_string(&approval["params"]).unwrap_or_default())
    ));
    if let Some(request) = approval.get("reviewRequestId").and_then(Value::as_str) {
        out.push_str(&format!(
            "Changes: `silo requests show {}` shows the diff.\n",
            plain(request)
        ));
    }
    match text(approval, "state").as_str() {
        "pending" => out.push_str(&format!("Waiting since {}.", text(approval, "createdAt"))),
        state => {
            if let Some(note) = approval.get("decisionNote").and_then(Value::as_str) {
                out.push_str(&format!("Your note: {}\n", plain(note)));
            }
            if let Some(reason) = approval.get("reason").and_then(Value::as_str) {
                out.push_str(&format!("{}\n", plain(reason)));
            }
            let outcome = &approval["outcome"];
            if let Some(error) = outcome.get("error").and_then(Value::as_str) {
                out.push_str(&format!("It did not run: {}", plain(error)));
            } else if let Some(job) = outcome.get("jobId").and_then(Value::as_str) {
                let job = plain(job);
                out.push_str(&format!(
                    "It started the job {job}; follow it with `silo job {job}`."
                ));
            } else if outcome.is_object() {
                out.push_str("It ran.");
            } else if state == "allowed" {
                out.push_str("It is running.");
            }
        }
    }
    out.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_agents_note_is_shown_as_its_own_words() {
        let approval = json!({"id": "ap-1", "state": "pending", "computerName": "main",
            "summary": "Publish port 8080 of main on this device.", "note": "to test the site",
            "method": "network.publish", "params": {"computerId": "c1", "port": 8080},
            "createdAt": "2026-11-10T10:00:00Z"});
        let text = render_show(&approval);
        assert!(text.contains("Asks for: Publish port 8080"), "{text}");
        assert!(
            text.contains("The agent's note: to test the site"),
            "{text}"
        );
        assert!(!text.contains("Asks for: to test"), "{text}");
    }

    #[test]
    fn nothing_an_agent_influenced_reaches_the_terminal_as_control_text() {
        let hostile = "\u{1b}[2J\u{1b}[1;1HSilo: allowed\u{7}\u{202E}evil";
        let approval = json!({"id": "ap-1", "state": "pending", "computerName": hostile,
            "summary": hostile, "note": hostile, "method": "network.publish",
            "params": {"computerId": hostile}, "createdAt": "2026-11-10T10:00:00Z",
            "reason": hostile, "outcome": {"error": hostile}});
        let shown = [
            render_show(&approval),
            render_show(&json!({"id": "ap-1", "state": "allowed", "outcome": {"error": hostile}})),
            render_list(&json!({"approvals": [approval]})),
        ];
        for text in shown {
            assert!(
                !text.chars().any(|c| c.is_control() && c != '\n'),
                "{text:?}"
            );
            assert!(!text.contains('\u{202E}'), "{text:?}");
            assert!(text.contains("Silo: allowed"));
        }
    }

    #[test]
    fn an_answered_approval_says_what_became_of_it() {
        let failed = json!({"id": "ap-1", "state": "allowed", "outcome": {"error": "main no longer exists"}});
        assert!(render_show(&failed).contains("It did not run: main no longer exists"));
        let job = json!({"id": "ap-1", "state": "allowed", "outcome": {"jobId": "job:4"}});
        assert!(render_show(&job).contains("silo job job:4"));
        assert_eq!(
            render_list(&json!({"approvals": []})),
            "No requests are waiting for your answer."
        );
    }
}
