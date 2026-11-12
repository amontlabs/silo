//! The words of a request's summary. Whatever a computer's agent chose (a name, a host, a
//! path, a command) is made plain, cut and quoted here, so it reads as what the agent wrote
//! and never as Silo's own sentence.

use serde_json::Value;
use silo_api::plain_text_within;

/// The longest agent-chosen text a summary shows in one piece.
const QUOTED_MAX_CHARS: usize = 80;
/// The most items of a list a summary names; the rest are counted.
const LISTED_MAX: usize = 5;

/// `text` as one quoted, plain, bounded piece.
pub(super) fn quoted(text: &str) -> String {
    let plain = plain_text_within(text, QUOTED_MAX_CHARS);
    format!("“{}”", plain.replace(['“', '”', '"'], "'"))
}

/// The items quoted and joined, the first few only: "“a”, “b” and 3 more".
pub(super) fn listed(items: &[String]) -> String {
    let mut text = items
        .iter()
        .take(LISTED_MAX)
        .map(|item| quoted(item))
        .collect::<Vec<_>>()
        .join(", ");
    if items.len() > LISTED_MAX {
        text.push_str(&format!(" and {} more", items.len() - LISTED_MAX));
    }
    text
}

/// What a computer's GitHub policy gives it, in plain words; `None` is no policy.
pub(super) fn github_access(policy: Option<&Value>) -> String {
    let Some(policy) = policy else {
        return "no access".to_owned();
    };
    let all = policy.get("repositoryMode").and_then(Value::as_str) == Some("all");
    if all {
        return if policy.get("allRepositoriesAllowChanges") == Some(&Value::Bool(true)) {
            "all repositories, with writing".to_owned()
        } else {
            "all repositories, read only".to_owned()
        };
    }
    let repositories: Vec<(String, bool)> = policy
        .get("repositories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|repository| {
            Some((
                repository.get("repository")?.as_str()?.to_owned(),
                repository.get("allowPushes") == Some(&Value::Bool(true)),
            ))
        })
        .collect();
    if repositories.is_empty() {
        return "no access".to_owned();
    }
    let mut parts: Vec<String> = repositories
        .iter()
        .take(LISTED_MAX)
        .map(|(name, writes)| {
            format!(
                "{} ({})",
                quoted(name),
                if *writes { "can write" } else { "read only" }
            )
        })
        .collect();
    if repositories.len() > LISTED_MAX {
        let rest = &repositories[LISTED_MAX..];
        let writing = rest.iter().filter(|(_, writes)| *writes).count();
        parts.push(format!(
            "and {} more, {writing} of them with writing",
            rest.len()
        ));
    }
    format!("repositories {}", parts.join(", "))
}

/// Whether a GitHub policy lets its computer write to any repository.
pub(super) fn github_writes(policy: Option<&Value>) -> bool {
    let Some(policy) = policy else {
        return false;
    };
    if policy.get("repositoryMode").and_then(Value::as_str) == Some("all") {
        return policy.get("allRepositoriesAllowChanges") == Some(&Value::Bool(true));
    }
    policy
        .get("repositories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|repository| repository.get("allowPushes") == Some(&Value::Bool(true)))
}

/// The identity a policy has commits made under, in plain words.
pub(super) fn github_identity(policy: Option<&Value>) -> String {
    let identity = policy.and_then(|policy| policy.get("identity"));
    let text = |key: &str| {
        identity
            .and_then(|identity| identity.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    if identity.and_then(|identity| identity.get("apply")) != Some(&Value::Bool(true)) {
        return "no commit identity set by Silo".to_owned();
    }
    format!(
        "commits as {} {}",
        quoted(text("name")),
        quoted(text("email"))
    )
}
