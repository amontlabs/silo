//! Short human text for the values the operations return.

use serde_json::Value;

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

pub fn list(value: &Value) -> String {
    let computers = value["computers"].as_array().cloned().unwrap_or_default();
    if computers.is_empty() {
        return "No computers yet. Create one with `silo create <name>`.".to_owned();
    }
    let width = computers
        .iter()
        .map(|computer| text(computer, "name").len())
        .max()
        .unwrap_or(4)
        .max(4);
    let mut out = format!(
        "{:<width$}  {:<9}  {:<6}  {}\n",
        "NAME", "STATE", "KIND", "SIZE"
    );
    for computer in &computers {
        out.push_str(&format!(
            "{:<width$}  {:<9}  {:<6}  {} cpu, {} GiB\n",
            text(computer, "name"),
            text(computer, "state"),
            text(computer, "kind"),
            number(computer, "cpus"),
            number(computer, "memoryGiB"),
        ));
    }
    out.trim_end().to_owned()
}

pub fn status(value: &Value) -> String {
    let mut out = format!(
        "{} ({}): {}\n{} cpu, {} GiB memory",
        text(value, "name"),
        text(value, "kind"),
        text(value, "state"),
        number(value, "cpus"),
        number(value, "memoryGiB"),
    );
    if let Some(disk) = value.get("diskGiB").and_then(Value::as_u64) {
        out.push_str(&format!(", {disk} GiB disk"));
    }
    out.push_str(&format!("\n{} checkpoints", number(value, "checkpoints")));
    if let Some(detail) = value.get("detail").and_then(Value::as_str) {
        out.push_str(&format!("\n{detail}"));
    }
    if let Some(quiet) = value.get("quiet").filter(|quiet| quiet.is_object()) {
        out.push_str(&format!("\n{}", quiet_line(quiet)));
    }
    if let Some(secrets) = value.get("secrets").and_then(Value::as_array) {
        out.push_str(&format!("\nsecrets assigned: {}", names_or_none(secrets)));
    }
    if let Some(github) = value.get("github").filter(|github| github.is_object()) {
        out.push_str(&format!("\n{}", github_line(github)));
    }
    out
}

fn names_or_none(items: &[Value]) -> String {
    let names: Vec<&str> = items.iter().filter_map(Value::as_str).collect();
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(", ")
    }
}

fn list_of(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// `quiet: applied; stopped: a; scrubbed: none; secrets: none`, then the macOS extras.
fn quiet_line(quiet: &Value) -> String {
    let applied = quiet.get("applied").and_then(Value::as_bool) == Some(true);
    let mut line = format!(
        "quiet: {}; stopped: {}; scrubbed: {}; secrets: {}",
        if applied { "applied" } else { "pending" },
        names_or_none(&list_of(quiet, "stop")),
        names_or_none(&list_of(quiet, "scrub")),
        names_or_none(&list_of(quiet, "secrets")),
    );
    let parked = list_of(quiet, "parked");
    if !parked.is_empty() {
        line.push_str(&format!("; parked: {}", parked.len()));
    }
    let still = list_of(quiet, "stillScheduled");
    if !still.is_empty() {
        line.push_str(&format!("; still scheduled: {}", names_or_none(&still)));
    }
    line
}

fn github_line(github: &Value) -> String {
    let repositories = list_of(github, "repositories");
    let pushes = repositories
        .iter()
        .any(|repository| repository["allowPushes"] == true)
        || github["allRepositoriesAllowChanges"] == true;
    let which = if text(github, "repositoryMode") == "all" {
        "all repositories".to_owned()
    } else {
        names_or_none(
            &repositories
                .iter()
                .map(|repository| repository["repository"].clone())
                .collect::<Vec<_>>(),
        )
    };
    format!(
        "github: {} ({which})",
        if pushes { "can push" } else { "read-only" }
    )
}

pub fn secrets(value: &Value) -> String {
    let items = value["secrets"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return "No secrets yet.".to_owned();
    }
    items
        .iter()
        .map(|item| {
            format!(
                "{}  {}",
                text(item, "name"),
                names_or_none(&list_of(item, "computers"))
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn computer_line(verb: &str, value: &Value) -> String {
    format!("{verb} {} ({})", text(value, "name"), text(value, "state"))
}

pub fn checkpoints(value: &Value) -> String {
    let items = value["checkpoints"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return format!("{} has no checkpoints.", text(value, "computer"));
    }
    items
        .iter()
        .map(|item| {
            format!(
                "{}  {}  {}  {}",
                text(item, "id"),
                text(item, "name"),
                text(item, "kind"),
                text(item, "createdAt")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn list_shows_a_table() {
        let value = json!({"computers": [
            {"name": "main", "state": "running", "kind": "linux", "cpus": 2, "memoryGiB": 4}
        ]});
        let out = list(&value);
        assert!(out.starts_with("NAME"));
        assert!(out.contains("main  running"));
        assert!(out.contains("2 cpu, 4 GiB"));
        assert!(list(&json!({"computers": []})).contains("silo create"));
    }
}

fn lines(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// `silo fold`: what was kept, what a dry run would keep, or why nothing was changed.
pub fn promote(value: &Value) -> String {
    let report = &value["report"];
    let (fork, target) = (text(report, "fork"), text(report, "target"));
    let archive = text(report, "archive");
    let mut out = match text(value, "status") {
        "refused" => return text(&value["refusal"], "message").to_owned(),
        "dry-run" => format!(
            "Would make {fork} the new {target}; {target} would be archived as {archive}."
        ),
        _ => format!("{fork} is now {target}. The old {target} is {archive} (stopped)."),
    };
    for line in lines(report, "notes") {
        out.push_str(&format!("\nnote: {line}"));
    }
    out
}

pub fn fold(value: &Value) -> String {
    let report = &value["report"];
    let (fork, target) = (text(report, "fork"), text(report, "target"));
    let mut out = String::new();
    match text(value, "status") {
        "refused" => {
            let refusal = &value["refusal"];
            out.push_str(text(refusal, "message"));
            for conflict in refusal["conflicts"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "\n  {} {}: {}",
                    text(conflict, "kind"),
                    text(conflict, "item"),
                    text(conflict, "detail")
                ));
            }
            out.push_str(&format!("\n{}", text(refusal, "hint")));
            return out;
        }
        "dry-run" => {
            out.push_str(&format!("Would keep {fork} into {target}:"));
            for line in lines(report, "summary") {
                out.push_str(&format!("\n  {line}"));
            }
        }
        _ => {
            out.push_str(&format!("Kept {fork} into {target}."));
            for line in lines(report, "steps") {
                out.push_str(&format!("\n  {line}"));
            }
        }
    }
    for note in lines(report, "notes") {
        out.push_str(&format!("\nnote: {note}"));
    }
    if text(value, "status") == "kept" {
        out.push_str(&format!("\nUndo with `silo undo {target}`."));
    }
    out
}

pub fn undo(value: &Value) -> String {
    let computer = text(value, "computer");
    match text(value, "status") {
        "undone" => {
            let name = text(&value["checkpoint"], "name");
            let how = if value["recovered"] == true {
                " (an interrupted keep)"
            } else {
                ""
            };
            let mut out = format!("Put {computer} back to {name}{how}.");
            let changed = lines(value, "changedSince");
            if !changed.is_empty() {
                out.push_str("\nThese changes made on the computer after the keep are gone:");
                for line in changed {
                    out.push_str(&format!("\n  {line}"));
                }
            }
            out
        }
        _ => format!("Nothing to undo on {computer}."),
    }
}
