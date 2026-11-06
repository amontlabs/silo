//! The readable summary of a fork's changes that review requests and pull request bodies
//! share: the buckets of the diff, then what a keep leaves out that needs a decision.

use serde_json::Value;

/// The diff's readable text (buckets, one section each), then a line naming the credential
/// paths a keep would not bring when the diff found any.
pub fn review(diff: &Value) -> String {
    let mut text = diff["text"]
        .as_str()
        .unwrap_or_default()
        .trim_end()
        .to_owned();
    if let Some(line) = credentials_line(diff) {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&line);
    }
    text
}

/// `Credentials not kept: ~/.ssh/ (2 files), ~/.aws/credentials`, or nothing.
pub fn credentials_line(diff: &Value) -> Option<String> {
    let credentials = diff["not_kept"]["credentials"].as_object()?;
    if credentials.is_empty() {
        return None;
    }
    let groups: Vec<String> = credentials
        .iter()
        .map(
            |(group, paths)| match paths.as_array().map_or(0, Vec::len) {
                0 | 1 => group.clone(),
                count => format!("{group} ({count} files)"),
            },
        )
        .collect();
    Some(format!("Credentials not kept: {}", groups.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_summary_is_the_bucket_text_and_a_credentials_line() {
        let diff = json!({
            "text": "Code\n  ~/app: 2 commits\n",
            "not_kept": {"credentials": {"/root/.ssh/": ["a", "b"], "/root/.aws/credentials": ["c"]}}
        });
        assert_eq!(
            review(&diff),
            "Code\n  ~/app: 2 commits\n\nCredentials not kept: /root/.aws/credentials, /root/.ssh/ (2 files)"
        );
        assert_eq!(review(&json!({"text": "Code\n"})), "Code");
        assert_eq!(review(&json!({"not_kept": {"credentials": {}}})), "");
    }
}
