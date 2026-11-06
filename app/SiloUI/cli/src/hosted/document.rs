//! What is sent to the hosted reviewer besides code changes: the diff document, rebuilt from
//! an allowlist. Names of credential files, file hashes and sizes, crontab and launch agent
//! texts, the engine's readable summary, the home directory name and capture details stay
//! behind. Code changes are added separately by [`super::codediff`].

use serde_json::{json, Map, Value};

use crate::pr::credential_like;

/// Identity and digest fields: what a file hashes to, not what changed.
const IDENTITY_KEYS: &[&str] = &[
    "hash",
    "size",
    "target",
    "digest",
    "digest_before",
    "digest_after",
    "schema",
    "before",
    "after",
    "mtime",
    "dirty_identity",
];

/// Whether a path names a credential, a key or an environment file, by its own name or by
/// the absolute path it has under `repo`.
pub fn hidden(repo: &str, path: &str) -> bool {
    let full = if path.starts_with('/') || path.starts_with('~') || repo.is_empty() {
        path.to_owned()
    } else {
        format!("{}/{path}", repo.trim_end_matches('/'))
    };
    let base = full.rsplit('/').next().unwrap_or(&full);
    credential_like(&full)
        || credential_like(path)
        || base == ".env"
        || base.starts_with(".env.")
        || base.ends_with(".env")
        || base.ends_with(".tfvars")
        || base.contains(".tfstate")
        || base.ends_with(".kdbx")
        || base.ends_with(".ovpn")
        || base.ends_with("-sa.json")
        || base.starts_with("firebase-adminsdk")
        || ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"]
            .iter()
            .any(|name| base.starts_with(name))
        || [
            ".pgpass",
            ".htpasswd",
            ".vault-token",
            ".dockercfg",
            "kubeconfig",
        ]
        .contains(&base)
        || base.starts_with("credentials")
        || base == "secrets.yaml"
        || base == "secrets.json"
}

fn path_of(entry: &Value) -> Option<&str> {
    entry.as_str().or_else(|| entry["path"].as_str())
}

/// The entries of a list that name no hidden file, and how many were dropped.
fn visible(repo: &str, list: &Value) -> (Value, usize) {
    let Some(entries) = list.as_array() else {
        return (json!([]), 0);
    };
    let kept: Vec<Value> = entries
        .iter()
        .filter(|entry| !path_of(entry).is_some_and(|p| hidden(repo, p)))
        .cloned()
        .collect();
    let dropped = entries.len() - kept.len();
    (Value::Array(kept), dropped)
}

fn strip_identities(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(strip_identities),
        Value::Object(object) => {
            object.retain(|key, _| !IDENTITY_KEYS.contains(&key.as_str()));
            object.values_mut().for_each(strip_identities);
        }
        _ => {}
    }
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

/// A map keyed by repo-relative path, without the hidden ones.
fn visible_map(repo: &str, map: &Value, hidden_count: &mut usize) -> Value {
    let mut out = Map::new();
    for (key, value) in map.as_object().into_iter().flatten() {
        let old_hidden = value.as_str().is_some_and(|old| hidden(repo, old));
        if hidden(repo, key) || old_hidden {
            *hidden_count += 1;
        } else {
            out.insert(key.clone(), value.clone());
        }
    }
    Value::Object(out)
}

fn code(list: &Value, hidden_count: &mut usize) -> Value {
    let mut out = Vec::new();
    for repo in list.as_array().into_iter().flatten() {
        let name = repo["repo"].as_str().unwrap_or_default();
        if hidden("", &format!("{}/", name.trim_end_matches('/'))) {
            *hidden_count += 1;
            continue;
        }
        let (gone, a) = visible(name, &repo["dirty_gone"]);
        *hidden_count += a;
        let mut entry = Map::new();
        for key in [
            "repo",
            "new",
            "removed",
            "removed_from",
            "commits",
            "branch",
            "dirty_delta",
        ] {
            if repo.get(key).is_some() {
                entry.insert(key.to_owned(), repo[key].clone());
            }
        }
        entry.insert(
            "dirty_new".into(),
            visible_map(name, &repo["dirty_new"], hidden_count),
        );
        entry.insert("dirty_gone".into(), gone);
        entry.insert(
            "renamed_from".into(),
            visible_map(name, &repo["renamed_from"], hidden_count),
        );
        let ignored = repo["gitignored"].as_array().map_or(0, Vec::len);
        entry.insert("gitignored_changed".into(), json!(ignored));
        out.push(Value::Object(entry));
    }
    Value::Array(out)
}

/// Per package: how many files it changed and the first five (not the hidden ones).
fn packages(list: &Value, hidden_count: &mut usize) -> Value {
    let mut list = list.clone();
    for package in list.as_array_mut().into_iter().flatten() {
        let Some(object) = package.as_object_mut() else {
            continue;
        };
        object.remove("owned_changes");
        let mut owned = Map::new();
        for (name, files) in object
            .get("owned_files")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            let (files, dropped) = visible("", files);
            *hidden_count += dropped;
            let all = files.as_array().cloned().unwrap_or_default();
            let first: Vec<Value> = all.iter().take(5).cloned().collect();
            owned.insert(name.clone(), json!({"count": all.len(), "first": first}));
        }
        object.insert("owned_files".into(), Value::Object(owned));
    }
    list
}

fn strip_text(entries: &Value) -> Value {
    let mut entries = entries.clone();
    for entry in entries.as_array_mut().into_iter().flatten() {
        if let Some(object) = entry.as_object_mut() {
            object.remove("before");
            object.remove("after");
        }
    }
    entries
}

/// The document `silo diff --json` (or a request's `diff`) holds, cut to what a reviewer
/// reads. Returns it with the number of entries about credential and environment files that
/// were left out.
pub fn review_document(diff: &Value) -> (Value, usize) {
    let mut hidden_count = 0;
    let mut out = Map::new();
    out.insert("format".into(), diff["format"].clone());
    out.insert("code".into(), code(&diff["code"], &mut hidden_count));
    out.insert(
        "packages".into(),
        packages(&diff["packages"], &mut hidden_count),
    );
    let (other, a) = visible("", &diff["other"]);
    let (repo_files, b) = visible("", &diff["repo_files"]);
    let (files, c) = visible("", &diff["settings"]["files"]);
    hidden_count += a + b + c;
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
    let (database, d) = visible(
        "",
        &diff["database"].as_array().map_or(json!([]), |dbs| {
            Value::Array(
                dbs.iter()
                    .map(|db| json!({"path": db["name"], "entry": db}))
                    .collect(),
            )
        }),
    );
    hidden_count += d;
    let databases: Vec<Value> = database
        .as_array()
        .into_iter()
        .flatten()
        .map(|wrapped| wrapped["entry"].clone())
        .collect();
    out.insert("database".into(), Value::Array(databases));
    let not_kept = &diff["not_kept"];
    out.insert(
        "not_kept".into(),
        json!({
            "listening": not_kept["listening"],
            "processes": not_kept["processes"],
            "services": not_kept["services"],
            "credential_groups_changed": not_kept["credentials"].as_object().map_or(0, Map::len),
        }),
    );
    out.insert(
        "ignored_count".into(),
        json!(diff["ignored"]["paths"].as_array().map_or(0, Vec::len)),
    );
    let mut document = Value::Object(out);
    strip_identities_except_packages(&mut document);
    scrub(&mut document, diff["home"].as_str().unwrap_or_default());
    (document, hidden_count)
}

fn strip_identities_except_packages(document: &mut Value) {
    if let Some(object) = document.as_object_mut() {
        for (key, value) in object.iter_mut() {
            if key != "packages" {
                strip_identities(value);
            }
        }
    }
}

/// A diff the engine could not give (a fork made from a folder): its readable summary, one
/// line at a time, without lines that name hidden files or credentials.
pub fn text_document(text: &str, home: &str) -> (Value, usize) {
    let mut dropped = 0;
    let mut lines = Vec::new();
    for line in text.lines() {
        let lower = line.to_lowercase();
        let names_path = line
            .split_whitespace()
            .any(|word| hidden("", word.trim_matches(|c| c == '"' || c == '\'' || c == ',')));
        if names_path || lower.contains("credential") || lower.contains("gitignored") {
            dropped += 1;
        } else {
            lines.push(line);
        }
    }
    let mut document = json!({"summary_text": lines.join("\n")});
    scrub(&mut document, home);
    (document, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_summary_loses_lines_that_name_credentials_or_env_files() {
        let text = "Commits since abc:\nabc1 add run script\nUncommitted:\n M src/app.rs\n?? .env\n?? deploy.key\n M ~/.ssh/config\nNot kept: credentials: ~/.aws/credentials";
        let (document, dropped) = text_document(text, "/home/dev");
        let kept = document["summary_text"].as_str().unwrap();
        assert!(kept.contains("add run script") && kept.contains("src/app.rs"));
        for gone in [".env", "deploy.key", ".ssh", "credentials"] {
            assert!(!kept.contains(gone), "{gone} in {kept}");
        }
        assert_eq!(dropped, 4);
    }

    #[test]
    fn code_names_are_filtered_by_their_path_under_the_repo() {
        let diff = json!({"home": "/home/dev", "code": [{
            "repo": "/home/dev/.ssh/proj",
            "dirty_new": {"a.txt": "M"},
            "gitignored": [{"path": ".env"}]
        }, {
            "repo": "/home/dev/app",
            "dirty_new": {"ok.rs": "M", "certs/site.key": "??", ".env.local": "??"},
            "dirty_gone": ["old.pem", "gone.rs"],
            "renamed_from": {"b.rs": "id_rsa"},
            "gitignored": [{"path": ".env"}]
        }]});
        let (document, hidden) = review_document(&diff);
        let sent = document.to_string();
        for gone in ["a.txt", "site.key", ".env", "old.pem", "id_rsa", "b.rs"] {
            assert!(!sent.contains(gone), "{gone} in {sent}");
        }
        assert!(sent.contains("ok.rs") && sent.contains("gone.rs"));
        assert!(hidden >= 5);
    }
}
