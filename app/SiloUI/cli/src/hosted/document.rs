//! What is sent to the hosted reviewer: the diff document and nothing else. The document is
//! rebuilt from an allowlist of its parts; crontab and launch agent texts, credential file
//! changes, the home directory name and the engine's capture details stay behind.

use serde_json::{json, Map, Value};

use crate::pr::credential_like;

/// The parts of the document that are sent as they are.
const KEPT: &[&str] = &["format", "summary", "code", "database"];

fn path_of(entry: &Value) -> Option<&str> {
    entry.as_str().or_else(|| entry["path"].as_str())
}

/// The entries of a list that do not name a credential file; the number dropped.
fn without_credentials(list: &Value) -> (Value, usize) {
    let Some(entries) = list.as_array() else {
        return (list.clone(), 0);
    };
    let kept: Vec<Value> = entries
        .iter()
        .filter(|entry| !path_of(entry).is_some_and(credential_like))
        .cloned()
        .collect();
    let dropped = entries.len() - kept.len();
    (Value::Array(kept), dropped)
}

fn strip_text(entries: &Value) -> Value {
    let mut entries = entries.clone();
    for entry in entries.as_array_mut().into_iter().flatten() {
        if let Some(object) = entry.as_object_mut() {
            for key in ["before", "after"] {
                if object.get(key).is_some_and(Value::is_string) {
                    object.remove(key);
                }
            }
        }
    }
    entries
}

fn packages(list: &Value) -> Value {
    let mut list = list.clone();
    for package in list.as_array_mut().into_iter().flatten() {
        if let Some(object) = package.as_object_mut() {
            object.remove("owned_changes");
        }
    }
    list
}

/// Replaces the home directory with `~` in every string.
fn scrub(value: &mut Value, home: &str) {
    match value {
        Value::String(text) if !home.is_empty() => {
            *text = text.replace(&format!("{home}/"), "~/");
        }
        Value::Array(items) => items.iter_mut().for_each(|item| scrub(item, home)),
        Value::Object(object) => object.values_mut().for_each(|item| scrub(item, home)),
        _ => {}
    }
}

/// The document `silo diff --json` (or a request's `diff`) holds, cut to what a reviewer
/// reads. Returns it with the number of credential file entries left out.
pub fn review_document(diff: &Value) -> (Value, usize) {
    let mut out = Map::new();
    let mut hidden = 0;
    for key in KEPT {
        if diff.get(*key).is_some() {
            out.insert((*key).to_owned(), diff[*key].clone());
        }
    }
    out.insert("packages".into(), packages(&diff["packages"]));
    let (other, a) = without_credentials(&diff["other"]);
    let (repo_files, b) = without_credentials(&diff["repo_files"]);
    let (files, c) = without_credentials(&diff["settings"]["files"]);
    hidden += a + b + c;
    out.insert("other".into(), other);
    out.insert("repo_files".into(), repo_files);
    out.insert(
        "settings".into(),
        json!({
            "files": files,
            "cron": strip_text(&diff["settings"]["cron"]),
            "services": diff["settings"]["services"],
            "launch_agents": strip_text(&diff["settings"]["launch_agents"]),
        }),
    );
    let not_kept = &diff["not_kept"];
    let credentials = not_kept["credentials"]
        .as_object()
        .map_or(0, |groups| groups.len());
    out.insert(
        "not_kept".into(),
        json!({
            "listening": not_kept["listening"],
            "processes": not_kept["processes"],
            "services": not_kept["services"],
            "credential_groups_changed": credentials,
        }),
    );
    out.insert(
        "ignored_count".into(),
        diff["ignored"]["paths"]
            .as_array()
            .map_or(json!(0), |p| json!(p.len())),
    );
    let mut document = Value::Object(out);
    scrub(&mut document, diff["home"].as_str().unwrap_or_default());
    (document, hidden)
}

/// A diff the engine could not give (a fork made from a folder): the readable summary only.
pub fn text_document(text: &str, home: &str) -> Value {
    let mut document = json!({"summary_text": text});
    scrub(&mut document, home);
    document
}
