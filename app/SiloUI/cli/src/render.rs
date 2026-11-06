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
    // A list across devices says where each computer is.
    let across = computers
        .iter()
        .any(|computer| computer.get("deviceName").is_some());
    let device_width = computers
        .iter()
        .map(|computer| text(computer, "deviceName").len())
        .max()
        .unwrap_or(6)
        .max(6);
    let mut out = String::new();
    if across {
        out.push_str(&format!("{:<device_width$}  ", "DEVICE"));
    }
    out.push_str(&format!(
        "{:<width$}  {:<9}  {:<6}  {}\n",
        "NAME", "STATE", "KIND", "SIZE"
    ));
    for computer in &computers {
        if across {
            out.push_str(&format!(
                "{:<device_width$}  ",
                text(computer, "deviceName")
            ));
        }
        out.push_str(&format!(
            "{:<width$}  {:<9}  {:<6}  {} cpu, {} GiB\n",
            text(computer, "name"),
            text(computer, "state"),
            text(computer, "kind"),
            number(computer, "cpus"),
            number(computer, "memoryGiB"),
        ));
    }
    out.push_str(&unreachable_lines(value));
    out.trim_end().to_owned()
}

/// A line for each device a list across devices could not read.
fn unreachable_lines(value: &Value) -> String {
    list_of(value, "unreachable")
        .iter()
        .map(|device| {
            format!(
                "Could not reach {}: {}\n",
                text(device, "deviceName"),
                text(device, "reason")
            )
        })
        .collect()
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
    if let Some(used) = value.get("credentialsUsed").filter(|used| used.is_object()) {
        out.push_str(&format!("\n{}", credentials_line(used)));
    }
    out
}

/// `credentials used: KEY → host (POST ×3); blocked: 1 (TOKEN → evil.example GET)`, or
/// `credentials used: none`.
pub fn credentials_line(usage: &Value) -> String {
    let line = credentials_text(usage);
    line.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

fn credentials_text(usage: &Value) -> String {
    let used: Vec<String> = list_of(usage, "used")
        .iter()
        .map(|entry| {
            format!(
                "{} → {} ({} ×{})",
                text(entry, "secret"),
                text(entry, "host"),
                text(entry, "method"),
                number(entry, "count"),
            )
        })
        .collect();
    let mut line = format!(
        "credentials used: {}",
        if used.is_empty() {
            "none".to_owned()
        } else {
            used.join(", ")
        }
    );
    let blocked = list_of(usage, "blocked");
    if !blocked.is_empty() {
        let total: u64 = blocked
            .iter()
            .map(|entry| entry.get("count").and_then(Value::as_u64).unwrap_or(1))
            .sum();
        let details: Vec<String> = blocked
            .iter()
            .map(|entry| match entry.get("secret").and_then(Value::as_str) {
                Some(secret) => format!(
                    "{secret} → {} {}",
                    text(entry, "host"),
                    text(entry, "method")
                ),
                None => format!("{} {}", text(entry, "host"), text(entry, "method")),
            })
            .collect();
        line.push_str(&format!("; blocked: {} ({})", total, details.join(", ")));
    }
    line
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

/// `Forked try (stopped): main as of 14:02:31 (warm checkpoint, unchanged since, 40s old)`.
pub fn forked(value: &Value) -> String {
    let line = computer_line("Forked", value);
    let checkpoint = &value["checkpoint"];
    if !checkpoint.is_object() || text(value, "forkedAsOf").is_empty() {
        return line;
    }
    let what = if checkpoint["reused"] == true {
        let named = match text(checkpoint, "kind") {
            "" => "checkpoint".to_owned(),
            kind => format!("{kind} checkpoint"),
        };
        if checkpoint["unchanged"] == true {
            format!("{named}, unchanged since")
        } else {
            named
        }
    } else {
        "new checkpoint".to_owned()
    };
    let age = if checkpoint["reused"] == true {
        format!(
            ", {} old",
            crate::clock::age_text(checkpoint["ageSeconds"].as_i64().unwrap_or(0))
        )
    } else {
        String::new()
    };
    format!(
        "{line}: {} as of {} ({what}{age})",
        text(value, "source"),
        text(value, "forkedAsOf")
    )
}

pub fn ports(value: &Value) -> String {
    let rows = value["ports"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        return format!("No ports are published from {}.", text(value, "computer"));
    }
    rows.iter()
        .map(|row| {
            let target = row["url"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| row["hostPort"].as_u64().map(|p| format!("127.0.0.1:{p}")))
                .unwrap_or_else(|| "not reachable yet".to_owned());
            format!(
                "{}  {}  {}  {}",
                number(row, "port"),
                text(row, "scheme"),
                text(row, "state"),
                target
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn ssh(value: &Value) -> String {
    format!(
        "ssh {} reaches {}.",
        text(value, "host"),
        text(value, "computer")
    )
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

/// The steps of a computer as a table, oldest first.
pub fn steps(value: &Value) -> String {
    let items = value["steps"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return format!("{} has no steps yet.", text(value, "computer"));
    }
    let checkpoint_width = items
        .iter()
        .map(|step| text(step, "checkpointId").len())
        .max()
        .unwrap_or(0)
        .max(10);
    let mut out = format!(
        "{:<5}  {:<4}  {:<checkpoint_width$}  {:<20}  {}\n",
        "STEP", "EXIT", "CHECKPOINT", "FINISHED", "COMMAND"
    );
    for step in &items {
        let exit = step
            .get("exitCode")
            .and_then(Value::as_i64)
            .map_or_else(|| "-".to_owned(), |code| code.to_string());
        let checkpoint = match text(step, "checkpointId") {
            "" => "-",
            id => id,
        };
        out.push_str(&format!(
            "{:<5}  {:<4}  {:<checkpoint_width$}  {:<20}  {}\n",
            number(step, "number"),
            exit,
            checkpoint,
            text(step, "finishedAt"),
            text(step, "command"),
        ));
    }
    out.trim_end().to_owned()
}

fn from_text(node: &Value, names: &std::collections::HashMap<&str, &str>) -> String {
    let parent = node["parent"]
        .as_str()
        .and_then(|id| names.get(id).copied())
        .unwrap_or("its parent");
    let from = &node["from"];
    match text(from, "kind") {
        "main" => format!("from {parent}"),
        "checkpoint" => format!("from {parent} checkpoint {}", text(from, "checkpointId")),
        "step" => format!("from {parent} step {}", number(from, "number")),
        "recipe" => format!(
            "from recipe {}",
            text(from, "hash").chars().take(8).collect::<String>()
        ),
        "folder" => format!("from folder {}", text(from, "path")),
        _ => String::new(),
    }
}

fn pending_text(node: &Value) -> String {
    let pending = &node["pendingChanges"];
    let mut parts = Vec::new();
    if let Some(request) = pending["requestId"].as_str() {
        parts.push(format!("review {request}"));
    }
    if let Some(buckets) = pending["buckets"].as_object() {
        let counts: Vec<String> = buckets
            .iter()
            .filter(|(_, count)| count.as_u64().unwrap_or(0) > 0)
            .map(|(bucket, count)| format!("{bucket} {count}"))
            .collect();
        if !counts.is_empty() {
            parts.push(format!("changed: {}", counts.join(", ")));
        }
    }
    parts.join("; ")
}

/// The fork tree: each computer under the one it was forked from, with its state, what it
/// came from and its pending changes.
pub fn tree(value: &Value) -> String {
    let nodes = value["nodes"].as_array().cloned().unwrap_or_default();
    if nodes.is_empty() {
        return "No computers yet. Create one with `silo create <name>`.".to_owned();
    }
    let names: std::collections::HashMap<&str, &str> = nodes
        .iter()
        .map(|node| (text(node, "id"), text(node, "name")))
        .collect();
    let known = |node: &Value| {
        node["parent"]
            .as_str()
            .is_some_and(|parent| names.contains_key(parent))
    };
    let mut out = Vec::new();
    fn walk(
        nodes: &[Value],
        names: &std::collections::HashMap<&str, &str>,
        id: &str,
        prefix: &str,
        last: bool,
        top: bool,
        out: &mut Vec<String>,
        depth: usize,
    ) {
        let Some(node) = nodes.iter().find(|node| text(node, "id") == id) else {
            return;
        };
        let mut line = format!("{}  [{}]", text(node, "name"), text(node, "state"));
        for extra in [from_text(node, names), pending_text(node)] {
            if !extra.is_empty() {
                line.push_str(&format!("  {extra}"));
            }
        }
        if top {
            out.push(line);
        } else {
            out.push(format!(
                "{prefix}{}{line}",
                if last { "└─ " } else { "├─ " }
            ));
        }
        if depth > 32 {
            return;
        }
        let children: Vec<&Value> = nodes
            .iter()
            .filter(|child| child["parent"].as_str() == Some(id))
            .collect();
        let below = if top {
            String::new()
        } else {
            format!("{prefix}{}", if last { "   " } else { "│  " })
        };
        for (index, child) in children.iter().enumerate() {
            walk(
                nodes,
                names,
                text(child, "id"),
                &below,
                index + 1 == children.len(),
                false,
                out,
                depth + 1,
            );
        }
    }
    for node in nodes.iter().filter(|node| !known(node)) {
        walk(
            &nodes,
            &names,
            text(node, "id"),
            "",
            true,
            true,
            &mut out,
            0,
        );
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn credentials_line_names_used_and_blocked() {
        let usage = json!({"used": [{"secret": "OPENAI_API_KEY", "host": "api.openai.com", "method": "POST", "count": 3}],
            "blocked": [{"secret": "GITHUB_TOKEN", "host": "evil.example", "method": "GET", "count": 1, "reason": "host_not_allowed"}]});
        assert_eq!(
            credentials_line(&usage),
            "credentials used: OPENAI_API_KEY → api.openai.com (POST ×3); blocked: 1 (GITHUB_TOKEN → evil.example GET)"
        );
        assert_eq!(
            credentials_line(&json!({"used": [], "blocked": []})),
            "credentials used: none"
        );
    }

    #[test]
    fn credentials_line_escapes_control_characters() {
        let usage = json!({"used": [{"secret": "K\u{1b}[2J", "host": "h\nx", "method": "GET", "count": 1}], "blocked": []});
        let line = credentials_line(&usage);
        assert!(!line.chars().any(char::is_control));
        assert!(line.contains("K?[2J"));
    }

    #[test]
    fn status_prints_the_credentials_line_when_known() {
        let value = json!({"name": "a", "kind": "linux", "state": "running", "credentialsUsed": {"used": [], "blocked": []}});
        assert!(status(&value).ends_with("credentials used: none"));
        assert!(!status(&json!({"name": "a"})).contains("credentials"));
    }

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

    #[test]
    fn steps_are_a_table_with_the_checkpoint_each_left() {
        let out = steps(&json!({"computer": "main", "steps": [
            {"number": 1, "exitCode": 0, "checkpointId": "cp-aaa", "finishedAt": "2026-01-01T00:00:01Z", "command": "make test"},
            {"number": 2, "exitCode": 2, "finishedAt": "2026-01-01T00:00:09Z", "command": "ls 'a b'"},
        ]}));
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].starts_with("STEP") && lines[0].ends_with("COMMAND"));
        assert!(
            lines[1].starts_with("1 ")
                && lines[1].contains("cp-aaa")
                && lines[1].ends_with("make test")
        );
        assert!(lines[2].contains("  2  ") && lines[2].contains("  -  "));
        assert!(steps(&json!({"computer": "main", "steps": []})).contains("no steps"));
    }

    #[test]
    fn the_tree_indents_forks_under_their_parent() {
        let out = tree(&json!({"nodes": [
            {"id": "m", "name": "main", "parent": null, "from": {"kind": "root"}, "state": "running",
             "pendingChanges": {"requestId": null, "buckets": null}},
            {"id": "a", "name": "try-a", "parent": "m", "from": {"kind": "step", "number": 3, "checkpointId": "cp"},
             "state": "stopped", "pendingChanges": {"requestId": "fr-1", "buckets": {"code": 2, "config": 0}}},
            {"id": "c", "name": "deeper", "parent": "a", "from": {"kind": "main"}, "state": "running",
             "pendingChanges": {"requestId": null, "buckets": null}},
            {"id": "b", "name": "try-b", "parent": "m", "from": {"kind": "folder", "path": "/r"}, "state": "running",
             "pendingChanges": {"requestId": null, "buckets": null}},
        ]}));
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "main  [running]");
        assert_eq!(
            lines[1],
            "├─ try-a  [stopped]  from main step 3  review fr-1; changed: code 2"
        );
        assert_eq!(lines[2], "│  └─ deeper  [running]  from try-a");
        assert_eq!(lines[3], "└─ try-b  [running]  from folder /r");
        assert!(tree(&json!({"nodes": []})).contains("silo create"));
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

pub fn up(value: &Value) -> String {
    let main = &value["main"];
    let made = if value["created"] == true {
        format!(" (made from {})", text(value, "recipe"))
    } else {
        String::new()
    };
    let mut out = format!(
        "main is {}{made}. Warm checkpoint {} saved.",
        text(main, "state"),
        text(value, "warmCheckpoint")
    );
    if let Some(host) = value["ssh"]["host"].as_str() {
        out.push_str(&format!(" ssh {host} reaches it."));
    } else if let Some(error) = value["ssh"]["error"].as_str() {
        out.push_str(&format!(" SSH name not set up: {error}"));
    }
    if let Some(error) = value["portsError"].as_str() {
        out.push_str(&format!(" Ports not published: {error}"));
    }
    out
}

pub fn recipes(value: &Value) -> String {
    let rows = value["recipes"].as_array().cloned().unwrap_or_default();
    let width = rows
        .iter()
        .map(|r| text(r, "name").len())
        .max()
        .unwrap_or(4)
        .max(4);
    let mut out = format!("{:<width$}  {:<9}  BUILT\n", "NAME", "FROM");
    for row in rows {
        let built = if row["built"] == true {
            format!("{} ({})", text(&row, "builtAt"), text(&row, "imageVersion"))
        } else {
            "not built".to_owned()
        };
        out.push_str(&format!(
            "{:<width$}  {:<9}  {built}\n",
            text(&row, "name"),
            text(&row, "origin")
        ));
    }
    out.trim_end().to_owned()
}

pub fn recipe_built(value: &Value) -> String {
    let recipe = &value["recipe"];
    let verb = if value["built"] == true {
        "Built"
    } else {
        "Already built:"
    };
    format!(
        "{verb} {} ({}), checkpoint {}.",
        text(recipe, "name"),
        &text(recipe, "hash")[..12.min(text(recipe, "hash").len())],
        text(recipe, "checkpointId")
    )
}

pub fn promote(value: &Value) -> String {
    let report = &value["report"];
    let (fork, target) = (text(report, "fork"), text(report, "target"));
    let archive = text(report, "archive");
    let mut out = match text(value, "status") {
        "refused" => return text(&value["refusal"], "message").to_owned(),
        "dry-run" => {
            format!("Would make {fork} the new {target}; {target} would be archived as {archive}.")
        }
        _ => format!("{fork} is now {target}. The old {target} is {archive} (stopped)."),
    };
    for line in lines(report, "notes") {
        out.push_str(&format!("\nnote: {line}"));
    }
    out
}

/// `silo fold`: what was kept, what a dry run would keep, or why nothing was changed.
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
        "unchanged" => {
            return lines(report, "steps").join("\n");
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
    if let Some(rule) = value["autoKeep"]["rule"].as_str() {
        out.push_str(&format!(
            "\nKept without asking: the auto_keep rule {rule} matched."
        ));
    }
    if value["writeBack"].is_object() {
        out.push_str(&write_back(&value["writeBack"]));
    }
    if text(value, "status") == "kept" {
        out.push_str(&format!("\nUndo with `silo undo {target}`."));
    }
    out
}

pub fn undo(value: &Value) -> String {
    let computer = text(value, "computer");
    match text(value, "status") {
        "undone" if text(value, "kind") == "promote" => {
            let how = if value["recovered"] == true {
                " (an interrupted promote)"
            } else {
                ""
            };
            format!(
                "Undid the promote{how}: {computer} is main again, and {} has its own name back.",
                text(value, "fork")
            )
        }
        "undone" => {
            if value.get("branch").is_some() {
                return format!(
                    "Put {} in {computer} back to {}.",
                    text(value, "branch"),
                    &text(value, "head")[..8.min(text(value, "head").len())]
                );
            }
            let name = text(&value["checkpoint"], "name");
            let how = if value["recovered"] == true {
                " (an interrupted keep)"
            } else {
                ""
            };
            let mut out = format!("Put {computer} back to {name}{how}.");
            if text(value, "resumedFrom") == "memory" {
                out.push_str(&format!(" {computer} is running again, as it was."));
            }
            for note in lines(value, "notes") {
                out.push_str(&format!("\nnote: {note}"));
            }
            let changed = lines(value, "changedSince");
            if !changed.is_empty() {
                out.push_str("\nThese changes made on the computer after the keep are gone:");
                for line in changed {
                    out.push_str(&format!("\n  {line}"));
                }
            }
            out
        }
        "refused" => text(value, "message").to_owned(),
        _ => format!("Nothing to undo on {computer}."),
    }
}

/// `silo fork -n N` and `silo fork ... -- cmd`: one line per fork, with its URLs and, when a
/// command ran, how it ended.
pub fn forks(value: &Value) -> String {
    let forks = value["forks"].as_array().cloned().unwrap_or_default();
    let width = forks
        .iter()
        .map(|fork| text(fork, "name").len())
        .max()
        .unwrap_or(4);
    let mut out = String::new();
    for fork in &forks {
        let name = text(fork, "name");
        let status = if let Some(error) = fork.get("error").and_then(Value::as_str) {
            format!("failed: {error}")
        } else if let Some(code) = fork.get("exitCode").and_then(Value::as_i64) {
            format!("exit {code}")
        } else {
            text(fork, "state").to_owned()
        };
        let urls: Vec<&str> = fork["urls"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        out.push_str(&format!("{name:<width$}  {status}"));
        if !urls.is_empty() {
            out.push_str(&format!("  {}", urls.join(" ")));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "{} fork{} of {}; review with `silo diff <fork>`, keep with `silo fold <fork>`.",
        forks.len(),
        if forks.len() == 1 { "" } else { "s" },
        text(value, "source"),
    ));
    out
}

/// `silo fork ./dir`.
pub fn folder_fork(value: &Value) -> String {
    let mut out = format!(
        "Forked {} into {}; the repository is at {} in the fork.",
        text(value, "folder"),
        text(value, "name"),
        text(value, "guestPath"),
    );
    for url in value["urls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        out.push_str(&format!("\n{url}"));
    }
    out.push_str(&format!(
        "\nBring the commits back with `silo fold {}`.",
        text(value, "name")
    ));
    out
}

/// `silo fold --request`.
pub fn requested(value: &Value) -> String {
    let request = &value["request"];
    let mut out = format!(
        "Requested a review of {} (request {}).",
        text(value, "fork"),
        text(request, "id")
    );
    if let Some(reason) = value["autoKeep"]["declined"].as_str() {
        out.push_str(&format!("\nNot kept without asking: {reason}."));
    }
    out
}

/// What a keep appended to a recipe file (or why it could not).
fn write_back(report: &Value) -> String {
    let file = text(report, "file");
    let mut parts: Vec<String> = Vec::new();
    for (manager, names) in report["packages"].as_object().into_iter().flatten() {
        let names: Vec<&str> = names
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !names.is_empty() {
            parts.push(format!("{manager}: {}", names.join(", ")));
        }
    }
    let files: Vec<&str> = report["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !files.is_empty() {
        parts.push(format!("files: {}", files.join(", ")));
    }
    let mut out = String::new();
    if let Some(error) = report["error"].as_str() {
        let what = if file.is_empty() {
            "No recipe file".to_owned()
        } else {
            file.to_owned()
        };
        out.push_str(&format!("\nnote: {what} was not changed ({error})."));
        if !parts.is_empty() {
            out.push_str(&format!(" Add by hand: {}.", parts.join("; ")));
        }
    } else if report["written"] == true {
        out.push_str(&format!("\nAppended to {file}: {}.", parts.join("; ")));
    }
    for skipped in report["skipped"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        out.push_str(&format!(
            "\nnote: {skipped} has no list in a recipe; not written."
        ));
    }
    out
}

/// One line for what the `auto_keep` rules did to a fork at the end of its command.
pub fn auto_keep(value: &Value) -> String {
    let fork = text(value, "fork");
    if let Some(error) = value["error"].as_str() {
        return format!("{fork}: could not check the auto_keep rules: {error}");
    }
    match value["autoKeep"]["rule"].as_str() {
        Some(rule) => format!(
            "Kept {fork} without asking: the auto_keep rule {rule} matched. Undo with `silo undo {}`.",
            text(&value["report"], "target")
        ),
        None => format!(
            "{fork} was not kept without asking ({}); left a review request ({}).",
            text(&value["autoKeep"], "declined"),
            text(&value["request"], "id")
        ),
    }
}

/// `silo fold --pr`.
pub fn pull_request(value: &Value) -> String {
    let mut out = format!(
        "Pushed {} to {} and opened a pull request into {} of {}:\n{}",
        text(value, "branch"),
        text(value, "remote"),
        text(value, "base"),
        text(value, "repository"),
        text(&value["pullRequest"], "url"),
    );
    for note in value["notes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        out.push_str(&format!("\nnote: {note}"));
    }
    out
}

/// `code 2, settings 1`: the buckets of a request that hold changes.
fn counts_line(counts: &Value) -> String {
    let parts: Vec<String> = counts
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(bucket, count)| {
            count
                .as_u64()
                .filter(|n| *n > 0)
                .map(|n| format!("{bucket} {n}"))
        })
        .collect();
    if parts.is_empty() {
        "no changes counted".to_owned()
    } else {
        parts.join(", ")
    }
}

/// `silo requests`.
pub fn requests_list(value: &Value) -> String {
    let entries = list_of(value, "requests");
    if entries.is_empty() {
        let mut out =
            "No review requests. Ask for one with `silo fold <fork> --request`.".to_owned();
        let missed = unreachable_lines(value);
        if !missed.is_empty() {
            out.push('\n');
            out.push_str(missed.trim_end());
        }
        return out;
    }
    let mut out = String::new();
    for entry in &entries {
        if let Some(device) = entry.get("deviceName").and_then(Value::as_str) {
            out.push_str(&format!("[{device}] "));
        }
        let request = &entry["request"];
        let fork = entry.get("forkName").and_then(Value::as_str).map_or_else(
            || "(fork gone)".to_owned(),
            |name| format!("{name} ({})", text(entry, "forkState")),
        );
        let target = match text(request, "target") {
            folder if folder.starts_with("folder:") => folder.to_owned(),
            _ => entry
                .get("targetName")
                .and_then(Value::as_str)
                .unwrap_or("(computer gone)")
                .to_owned(),
        };
        out.push_str(&format!(
            "{}  {}  into {}  {}  [{}]\n",
            text(request, "id"),
            fork,
            target,
            counts_line(&request["counts"]),
            text(request, "status"),
        ));
    }
    out.push_str(&unreachable_lines(value));
    out.trim_end().to_owned()
}

/// The `Credentials used: ...` line, then a line for what was refused.
pub fn credentials_lines(credentials: &Value) -> String {
    let used: Vec<String> = list_of(credentials, "used")
        .iter()
        .map(|item| {
            format!(
                "{} to {} ({} x{})",
                text(item, "secret"),
                text(item, "host"),
                text(item, "method"),
                number(item, "count")
            )
        })
        .collect();
    let mut out = if used.is_empty() {
        "Credentials used: none".to_owned()
    } else {
        format!("Credentials used: {}", used.join(", "))
    };
    let blocked: Vec<String> = list_of(credentials, "blocked")
        .iter()
        .map(|item| {
            format!(
                "{} {} x{} ({})",
                text(item, "method"),
                text(item, "host"),
                number(item, "count"),
                text(item, "reason")
            )
        })
        .collect();
    if !blocked.is_empty() {
        out.push_str(&format!("\nBlocked: {}", blocked.join(", ")));
    }
    out
}

/// `silo requests show`: the same buckets as `silo diff`, then the credentials line.
pub fn request_show(value: &Value) -> String {
    let request = &value["request"];
    let mut out = format!(
        "Request {}: {} ({}) into {} [{}]\n\n{}\n\n{}",
        text(request, "id"),
        text(value, "forkName"),
        text(value, "forkState"),
        text(request, "target"),
        text(request, "status"),
        text(value, "text"),
        credentials_lines(&value["credentials"]),
    );
    if value["fresh"] == false {
        out.push_str("\nThe fork is stopped, so this is its last look, not its state now.");
    }
    if value["stale"] == true {
        out.push_str(&format!(
            "\nThis fork changed since the request was made, so a keep refuses. Ask again with `silo fold {} --request`.",
            text(value, "forkName")
        ));
    }
    out
}

/// `silo requests throw-away`.
pub fn request_thrown_away(value: &Value) -> String {
    match text(value, "dropped") {
        "" => format!(
            "The fork was already gone. Closed {}.",
            text(&value["request"], "id")
        ),
        name => format!(
            "Threw away {name} and closed {}.",
            text(&value["request"], "id")
        ),
    }
}

/// `silo requests open`.
pub fn request_open(value: &Value) -> String {
    let mut out = format!(
        "{} is {}{}.",
        text(value, "name"),
        text(value, "state"),
        if value["started"] == true {
            " (started now)"
        } else {
            ""
        }
    );
    if let Some(alias) = value.get("sshAlias").and_then(Value::as_str) {
        out.push_str(&format!(
            "\nssh {alias} (after `silo ssh-setup {}`)",
            text(value, "name")
        ));
    }
    for url in list_of(value, "urls").iter().filter_map(Value::as_str) {
        out.push_str(&format!("\n{url}"));
    }
    for url in list_of(value, "deviceUrls")
        .iter()
        .filter_map(Value::as_str)
    {
        out.push_str(&format!("\n{url} (as the other device sees it)"));
    }
    out
}
