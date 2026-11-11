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
        "{:<width$}  {:<9}  {:<9}  {}\n",
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
            "{:<width$}  {:<9}  {:<9}  {} cpu, {} GiB\n",
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
    if value["desktop"] == false {
        out.push_str(&format!(
            "\n{}",
            crate::gui::no_desktop_message(text(value, "name"))
        ));
    }
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
    if let Some(computer_use) = value
        .get("computerUse")
        .filter(|c| c.is_object() && value["desktop"] != false)
    {
        out.push_str(&format!("\n{}", computer_use_line(computer_use)));
    }
    out
}

fn or_unknown(text: &str) -> &str {
    if text.is_empty() {
        "unknown"
    } else {
        text
    }
}

/// `computer use: ready, approval auto, driver 0.38.0`.
pub fn computer_use_line(value: &Value) -> String {
    let mut line = format!(
        "computer use: {}, approval {}",
        or_unknown(text(value, "state")),
        or_unknown(text(value, "approval"))
    );
    if let Some(version) = value.get("driverVersion").and_then(Value::as_str) {
        line.push_str(&format!(", driver {version}"));
    }
    if let Some(reason) = value.get("reason").and_then(Value::as_str) {
        line.push_str(&format!(" ({reason})"));
    }
    line
}

/// `Forked try-1 (running), desktop and computer use ready` and the computer-use line.
pub fn forked_gui(value: &Value) -> String {
    format!(
        "{}\n{}",
        forked(value),
        computer_use_line(&value["computerUse"])
    )
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
    let mut line = format!("{verb} {} ({})", text(value, "name"), text(value, "state"));
    push_warning(&mut line, value);
    line
}

/// The device's warning sentence, when a command left it short of memory or disk.
fn push_warning(out: &mut String, value: &Value) {
    if let Some(warning) = value["warning"].as_str().filter(|w| !w.is_empty()) {
        out.push_str(&format!("\nwarning: {warning}"));
    }
}

/// `Exported main to /path/main-2026-11-07.silo-backup (3.1 GiB)`.
pub fn exported(value: &Value) -> String {
    let mut line = format!(
        "Exported {} to {} ({})",
        text(value, "computer"),
        text(value, "archive"),
        text(value, "size")
    );
    if let Some(left_out) = value["leftOut"]
        .as_array()
        .filter(|left_out| !left_out.is_empty())
    {
        let names: Vec<&str> = left_out.iter().filter_map(Value::as_str).collect();
        line.push_str(&format!(
            "\nRemoved from the exported computer: {}. Traces of them may remain in the file's disk layers, so keep the file private.",
            names.join(", ")
        ));
    }
    if let Some(logins) = value["logins"]
        .as_array()
        .filter(|logins| !logins.is_empty())
    {
        line.push_str(&format!(
            "\nIt contains {} logins: anyone with this file can use them.",
            logins.len()
        ));
    }
    if let Some(why) = value["unchecked"].as_str() {
        let other = if value["logins"].as_array().is_some_and(|l| !l.is_empty()) {
            "other "
        } else {
            ""
        };
        line.push_str(&format!(
            "\nSilo could not check it for {other}logins ({}).",
            why.trim().trim_end_matches('.')
        ));
    }
    line
}

/// `Imported main-imported (stopped). Start it with \`silo start main-imported\`.`
pub fn imported(value: &Value) -> String {
    let name = text(value, "name");
    format!(
        "{}\nStart it with `silo start {name}`. If it was a fork, bring its work into main with `silo merge {name} --into main`.",
        computer_line("Imported", value)
    )
}

/// `Forked try (stopped): main as of 14:02:31 (new checkpoint)`.
pub fn forked(value: &Value) -> String {
    let mut line = format!("Forked {} ({})", text(value, "name"), text(value, "state"));
    let checkpoint = &value["checkpoint"];
    if checkpoint.is_object() && !text(value, "forkedAsOf").is_empty() {
        let what = if checkpoint["reused"] == true {
            match text(checkpoint, "kind") {
                "" => "checkpoint".to_owned(),
                kind => format!("{kind} checkpoint"),
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
        line = format!(
            "{line}: {} as of {} ({what}{age})",
            text(value, "source"),
            text(value, "forkedAsOf")
        );
    }
    push_warning(&mut line, value);
    line
}

/// `silo network policy`.
pub fn network_policy(value: &Value) -> String {
    let mode = text(&value["policy"], "mode");
    let timing = match text(value, "timing") {
        "applied" => "in force",
        "nextStart" => "applies when it starts",
        "nextRestart" => "applies when it restarts; it keeps its current rules until then",
        other => other,
    };
    let described = match mode {
        "open" => "open: any public address".to_owned(),
        "closed" => "closed: no outbound traffic".to_owned(),
        _ => "a list of allowed hosts".to_owned(),
    };
    let mut lines = vec![format!(
        "{}: {described} ({timing})",
        text(value, "computer")
    )];
    let hosts = value["allowedHosts"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if mode == "allowlist" {
        if hosts.is_empty() {
            lines.push("No hosts are allowed; names still resolve.".into());
        }
        for host in &hosts {
            let ports: Vec<String> = host["ports"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .map(|port| port.to_string())
                .collect();
            let from = match text(host, "from") {
                "list" => "on the list".to_owned(),
                "github" => "GitHub access".to_owned(),
                other => other.replacen(':', " ", 1),
            };
            lines.push(format!(
                "  {}  port {}  ({from})",
                text(host, "host"),
                ports.join(", ")
            ));
        }
    } else {
        let listed = value["policy"]["hosts"].as_array().map_or(0, Vec::len)
            + value["policy"]["presets"].as_array().map_or(0, Vec::len);
        if listed > 0 {
            lines.push("Its hosts and presets are kept and apply in allowlist mode.".into());
        }
    }
    let presets: Vec<&str> = value["presets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|preset| preset["name"].as_str())
        .collect();
    lines.push(format!("Presets: {}", presets.join(", ")));
    for update in value["presetUpdates"].as_array().into_iter().flatten() {
        let added: Vec<&str> = update["addedHosts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        lines.push(format!(
            "The {} list has new hosts ({}). The computer keeps the list it was approved with until you set its policy again with this preset.",
            text(update, "name"),
            added.join(", ")
        ));
    }
    for notice in value["notices"].as_array().into_iter().flatten() {
        if let Some(notice) = notice.as_str() {
            lines.push(format!("Note: {notice}"));
        }
    }
    lines.join("\n")
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
    fn a_network_policy_shows_its_notices_and_preset_updates() {
        let text = network_policy(&json!({
            "computer": "main",
            "policy": {"mode": "open", "hosts": [], "presets": ["npm"]},
            "allowedHosts": [], "timing": "applied",
            "presets": [{"name": "npm"}],
            "presetUpdates": [{"name": "npm", "title": "npm", "addedHosts": ["registry.example"]}],
            "notices": ["A secret for *.example.com is not on the list."]
        }));
        assert!(
            text.contains("The npm list has new hosts (registry.example)"),
            "{text}"
        );
        assert!(
            text.contains("Note: A secret for *.example.com is not on the list."),
            "{text}"
        );
    }

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
    fn an_export_that_could_not_be_fully_checked_says_other_logins_only_when_some_were_listed() {
        let with = json!({"computer": "main", "archive": "/a", "size": "1 GiB",
            "logins": ["a GitHub CLI login (/root/.config/gh/hosts.yml)"], "unchecked": "the computer is stopped."});
        let text = exported(&with);
        assert!(
            text.contains("could not check it for other logins (the computer is stopped)"),
            "{text}"
        );
        let without = json!({"computer": "main", "archive": "/a", "size": "1 GiB",
            "logins": [], "unchecked": "the computer is stopped."});
        assert!(
            exported(&without).contains("could not check it for logins (the computer is stopped)")
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
    let mut out = format!("main is {}{made}.", text(main, "state"));
    if let Some(host) = value["ssh"]["host"].as_str() {
        out.push_str(&format!(" ssh {host} reaches it."));
    } else if let Some(error) = value["ssh"]["error"].as_str() {
        out.push_str(&format!(" SSH name not set up: {error}"));
    }
    if let Some(error) = value["portsError"].as_str() {
        out.push_str(&format!(" Ports not published: {error}"));
    }
    push_warning(&mut out, value);
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
    let mut out = format!(
        "{:<width$}  {:<9}  {:<6}  BUILT\n",
        "NAME", "FROM", "SYSTEM"
    );
    for row in rows {
        let built = if row["built"] == true {
            format!("{} ({})", text(&row, "builtAt"), text(&row, "imageVersion"))
        } else {
            "not built".to_owned()
        };
        out.push_str(&format!(
            "{:<width$}  {:<9}  {:<6}  {built}\n",
            text(&row, "name"),
            text(&row, "origin"),
            text(&row, "os"),
        ));
    }
    out.trim_end().to_owned()
}

/// `silo recipe add`.
pub fn recipe_added(value: &Value) -> String {
    let mut out = format!(
        "Stored the recipe {} ({}). Computers already built from an earlier {} keep the text they were built from.",
        text(value, "name"),
        &text(value, "hash")[..12.min(text(value, "hash").len())],
        text(value, "name"),
    );
    for warning in lines(value, "warnings") {
        out.push_str(&format!("\nwarning: {warning}"));
    }
    out
}

/// `silo recipe show`: the recipe as a file, then what the engine knows about it.
pub fn recipe_shown(value: &Value) -> String {
    let recipe = serde_json::from_value::<silo_api::methods::recipes::RecipeDefinition>(
        value["recipe"].clone(),
    )
    .map(|definition| crate::recipe_file::RecipeFile::of(&definition).to_yaml())
    .unwrap_or_default();
    let mut out = recipe.trim_end().to_owned();
    if out.is_empty() {
        out.push_str("(an empty recipe)");
    }
    out.push_str(&format!(
        "\n---\n{}{}, hash {}",
        text(value, "name"),
        if value["builtIn"] == true {
            " (ships with Silo)"
        } else {
            ""
        },
        &text(value, "hash")[..12.min(text(value, "hash").len())],
    ));
    out.push_str(if value["built"].is_object() {
        ", built"
    } else {
        ", not built"
    });
    if value["recipe"]["commandsHidden"] == true {
        out.push_str(
            "\nThe setup commands, service commands and keep commands are hidden from a computer; the engine runs them. Put tokens in secrets, never in them.",
        );
    }
    for warning in lines(value, "warnings") {
        out.push_str(&format!("\nwarning: {warning}"));
    }
    out
}

fn gib(bytes: u64) -> String {
    let tenths = (bytes as f64 / (1u64 << 30) as f64 * 10.0).round() as u64;
    if tenths % 10 == 0 {
        format!("{} GiB", tenths / 10)
    } else {
        format!("{}.{} GiB", tenths / 10, tenths % 10)
    }
}

/// `silo resources`: what the device has left, and the computers running.
pub fn resources(value: &Value) -> String {
    let bytes = |key: &str| value[key].as_u64();
    let mut out = String::new();
    let memory = match (bytes("memoryAvailableBytes"), bytes("memoryTotalBytes")) {
        (Some(free), Some(total)) => format!("{} free of {}", gib(free), gib(total)),
        (None, Some(total)) => format!("{} (free memory unknown)", gib(total)),
        _ => "unknown".to_owned(),
    };
    out.push_str(&format!("Memory: {memory}"));
    if let Some(pressure) = value["memoryPressure"].as_str() {
        out.push_str(&format!(", pressure {pressure}"));
    }
    out.push_str(&format!(
        "\nDisk: {} free of {}\nCPUs: {}\nRunning: {} computer{} using {} GiB of memory and {} CPU{}",
        gib(bytes("diskFreeBytes").unwrap_or(0)),
        gib(bytes("diskTotalBytes").unwrap_or(0)),
        number(value, "logicalCpus"),
        number(value, "runningComputers"),
        if number(value, "runningComputers") == 1 { "" } else { "s" },
        number(value, "runningMemoryGib"),
        number(value, "runningCpus"),
        if number(value, "runningCpus") == 1 { "" } else { "s" },
    ));
    for computer in value["computers"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "\n  {}  {}  {} GiB, {} CPU{}",
            text(computer, "name"),
            if computer["running"] == true {
                "running"
            } else {
                "stopped"
            },
            number(computer, "memoryGib"),
            number(computer, "cpus"),
            if number(computer, "cpus") == 1 {
                ""
            } else {
                "s"
            },
        ));
    }
    out.push_str(&format!(
        "\nState: {}. {}",
        text(value, "state"),
        text(value, "message")
    ));
    out
}

/// `silo access`: what each computer holds.
pub fn held(value: &Value) -> String {
    let mut blocks = Vec::new();
    for computer in value["computers"].as_array().into_iter().flatten() {
        let mut out = format!("{}", text(computer, "name"));
        let secrets: Vec<String> = computer["secrets"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|secret| {
                let hosts: Vec<&str> = secret["hosts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                format!("{} (to {})", text(secret, "name"), hosts.join(", "))
            })
            .collect();
        out.push_str(&format!(
            "\n  secrets: {}",
            if secrets.is_empty() {
                "none".to_owned()
            } else {
                secrets.join("; ")
            }
        ));
        out.push_str(&format!(
            "\n  GitHub: {}",
            github_access(&computer["github"])
        ));
        let ports: Vec<String> = computer["ports"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|port| port["configured"] == true)
            .map(|port| format!("{} ({})", number(port, "port"), text(port, "state")))
            .collect();
        out.push_str(&format!(
            "\n  published ports: {}",
            if ports.is_empty() {
                "none".to_owned()
            } else {
                ports.join(", ")
            }
        ));
        out.push_str(&format!("\n  SSH: {}", ssh_access(&computer["ssh"])));
        out.push_str(&format!(
            "\n  network: {}",
            network_access(&computer["networkPolicy"])
        ));
        blocks.push(out);
    }
    if blocks.is_empty() {
        return "No computers.".to_owned();
    }
    blocks.join("\n")
}

fn github_access(github: &Value) -> String {
    if !github.is_object() {
        return "none".to_owned();
    }
    let method = text(github, "method");
    if github["allRepositories"] == true {
        let write = if github["allRepositoriesWrite"] == true {
            "with changes allowed"
        } else {
            "read-only"
        };
        return format!("every repository, {write} ({method})");
    }
    let repositories: Vec<String> = github["repositories"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|repository| {
            let write = if repository["write"] == true {
                " (can push)"
            } else {
                ""
            };
            format!("{}{write}", text(repository, "repository"))
        })
        .collect();
    if repositories.is_empty() {
        format!("signed in, no repository ({method})")
    } else {
        format!("{} ({method})", repositories.join(", "))
    }
}

fn ssh_access(ssh: &Value) -> String {
    if !ssh.is_object() || ssh["enabled"] != true {
        return "off".to_owned();
    }
    format!("on, port {} ({})", number(ssh, "port"), text(ssh, "state"))
}

fn network_access(policy: &Value) -> String {
    if !policy.is_object() {
        return "open".to_owned();
    }
    let mode = text(policy, "mode");
    let hosts: Vec<&str> = policy["hosts"]
        .as_array()
        .into_iter()
        .chain(policy["presets"].as_array())
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if hosts.is_empty() {
        mode.to_owned()
    } else {
        format!("{mode}: {}", hosts.join(", "))
    }
}

/// `silo agents`: the coding agents a computer has.
pub fn agents(value: &Value) -> String {
    let rows = value["agents"].as_array().cloned().unwrap_or_default();
    let mut out = format!("Agents in {}:", text(value, "computer"));
    for row in rows {
        let version = match row["version"].as_str() {
            Some(version) => format!(" {version}"),
            None => String::new(),
        };
        out.push_str(&format!(
            "\n  {}  {}{version}",
            text(&row, "name"),
            text(&row, "state")
        ));
    }
    out
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

/// `silo replace-main`: what happened, what a dry run would do, or why nothing changed.
pub fn replace_main(value: &Value) -> String {
    let report = &value["report"];
    let (fork, target) = (text(report, "fork"), text(report, "target"));
    let archive = text(report, "archive");
    let mut out = match text(value, "status") {
        "refused" => return text(&value["refusal"], "message").to_owned(),
        "dry-run" => {
            format!("Would replace {target} with {fork}; the old {target} would be kept aside as {archive}.")
        }
        _ => format!("{fork} replaced {target}. The old {target} is {archive} (stopped)."),
    };
    for line in lines(report, "notes") {
        out.push_str(&format!("\nnote: {line}"));
    }
    if text(value, "status") == "promoted" {
        out.push_str(&format!("\nUndo with `silo undo {target}`."));
    }
    out
}

/// `silo rebuild`: report what happened.
pub fn rebuild(value: &Value) -> String {
    let name = text(value, "name");
    let undo = text(value, "undoName");
    let image = text(value, "image");
    if undo.is_empty() {
        return format!("Abandoned the rebuild of {name}. {name} is as it was before the rebuild.");
    }
    format!("Rebuilt {name} on {image}. The previous computer is {undo} (stopped).")
}

/// The conflicts of a merge that needs answers, one per item with the id to answer by and the
/// answers that settle it.
fn conflict_lines(refusal: &Value) -> String {
    let mut out = String::new();
    for conflict in refusal["conflicts"].as_array().into_iter().flatten() {
        let id = text(conflict, "id");
        out.push_str(&format!(
            "\n  {} {}: {}",
            text(conflict, "kind"),
            text(conflict, "item"),
            text(conflict, "detail")
        ));
        if !id.is_empty() {
            let allowed: Vec<&str> = conflict["allowed"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let (fork, main) = (text(conflict, "forkChange"), text(conflict, "mainChange"));
            if !fork.is_empty() || !main.is_empty() {
                out.push_str(&format!("\n    fork: {fork}; main: {main}"));
            }
            out.push_str(&format!(
                "\n    id: {id}; answers: {}",
                if allowed.is_empty() {
                    "none".to_owned()
                } else {
                    allowed.join(", ")
                }
            ));
        }
    }
    out
}

/// `silo merge`: what was merged, what a dry run would merge, or why nothing was changed.
pub fn merge(value: &Value) -> String {
    let report = &value["report"];
    let (fork, target) = (text(report, "fork"), text(report, "target"));
    let mut out = String::new();
    match text(value, "status") {
        "refused" => {
            let refusal = &value["refusal"];
            out.push_str(text(refusal, "message"));
            out.push_str(&conflict_lines(refusal));
            if text(refusal, "reason") == "choices-needed" {
                out.push_str(&format!(
                    "\nRun `silo merge {fork}` again with --choose <id>=<answer> for each item above (or --skip <id> to leave an item as {target} has it).",
                ));
            } else {
                out.push_str(&format!("\n{}", text(refusal, "hint")));
            }
            return out;
        }
        "unchanged" => {
            return lines(report, "steps").join("\n");
        }
        "dry-run" => {
            out.push_str(&format!("Would merge {fork} into {target}:"));
            for line in lines(report, "summary") {
                out.push_str(&format!("\n  {line}"));
            }
        }
        _ => {
            out.push_str(&format!("Merged {fork} into {target}."));
            for line in lines(report, "steps") {
                out.push_str(&format!("\n  {line}"));
            }
        }
    }
    for note in lines(report, "notes") {
        out.push_str(&format!("\nnote: {note}"));
    }
    if value["report"]["recipe"].is_object() {
        out.push_str(&recipe_update(&value["report"]["recipe"]));
    }
    if text(value, "status") == "kept" {
        out.push_str(&format!("\nUndo with `silo undo {target}`."));
    }
    out
}

/// What a merge appended to the recipe of the computer it merged into.
fn recipe_update(update: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (manager, names) in update["packages"].as_object().into_iter().flatten() {
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
    let files: Vec<&str> = update["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !files.is_empty() {
        parts.push(format!("files: {}", files.join(", ")));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!(
        "\nAdded to the recipe {}: {}.",
        text(update, "name"),
        parts.join("; ")
    )
}

pub fn undo(value: &Value) -> String {
    let computer = text(value, "computer");
    match text(value, "status") {
        "undone" if text(value, "kind") == "promote" => {
            let how = if value["recovered"] == true {
                " (an interrupted replace of main)"
            } else {
                ""
            };
            format!(
                "Undid the replace{how}: {computer} is main again, and {} has its own name back.",
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
                " (an interrupted merge)"
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
                out.push_str("\nThese changes made on the computer after the merge are gone:");
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
        "{} fork{} of {}; review with `silo diff <fork>`, merge with `silo merge <fork>`.",
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
        "\nBring the commits back with `silo merge {}`.",
        text(value, "name")
    ));
    out
}

/// `silo merge --request`.
pub fn requested(value: &Value) -> String {
    let request = &value["request"];
    format!(
        "Requested a review of {} (request {}).",
        text(value, "fork"),
        text(request, "id")
    )
}

/// `silo merge --pr`.
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

/// A request's status, with why it was closed when its fork or target was deleted.
fn status_label(request: &Value) -> String {
    let status = text(request, "status");
    match request.get("closedReason").and_then(Value::as_str) {
        Some("fork-deleted") => format!("closed: fork deleted ({status})"),
        Some("target-deleted") => format!("closed: target deleted ({status})"),
        _ => status.to_owned(),
    }
}

/// `silo requests`.
pub fn requests_list(value: &Value) -> String {
    let entries = list_of(value, "requests");
    if entries.is_empty() {
        let mut out =
            "No review requests. Ask for one with `silo merge <fork> --request`.".to_owned();
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
            status_label(request),
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
            "\nThis fork changed since the request was made, so a merge refuses. Ask again with `silo merge {} --request`.",
            text(value, "forkName")
        ));
    }
    out
}

/// `silo requests decline`.
pub fn request_declined(value: &Value) -> String {
    match text(value, "dropped") {
        "" => format!(
            "The fork was already gone. Closed {}.",
            text(&value["request"], "id")
        ),
        name => format!(
            "Declined {name} and closed {}.",
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
