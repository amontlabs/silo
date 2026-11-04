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
    out
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
