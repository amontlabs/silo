//! `silo approvals`: the requests computers' agents made that wait for your answer. Listing,
//! showing, allowing and denying them; allowing runs the request's call in the engine.

use serde_json::Value;
use silo_api::methods::{
    approvals::{
        ApprovalDecision, ApprovalIdParams, ApprovalState, ApprovalsDecideParams,
        ApprovalsListParams,
    },
    requests,
};

use crate::{engine::Engine, error::CliError};

/// The pending approvals, newest first.
pub async fn list(engine: &Engine) -> Result<Value, CliError> {
    engine
        .call(requests::approvals_list(ApprovalsListParams {
            state: Some(ApprovalState::Pending),
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
            assert!(!text.chars().any(|c| c.is_control() && c != '\n'), "{text:?}");
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
