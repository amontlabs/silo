//! Whole-computer change capture and review: `changes.*`.

use serde::{Deserialize, Serialize};

/// Which stored manifest a capture replaces.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChangesRole {
    /// The computer as it was when it was forked, before any agent work.
    Base,
    /// The most recent look at the computer.
    Latest,
}

wire_struct! {
    ChangesCaptureParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        /// `base` records the fork-time state; `latest` (the default) records the current one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<ChangesRole>,
        /// For a `base` capture: the checkpoint (or computer) the fork was made from. The
        /// no-op baseline is cached under it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
    }
}

wire_struct! {
    ChangesCaptureResult {
        role: ChangesRole,
        captured_at: String,
        records: u64,
        bytes: u64,
    }
}

wire_struct! {
    ChangesDiffParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        /// Cut the readable text to this many lines.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_lines: Option<u32>,
        /// Subtract the cached no-op baseline when there is one (default true).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        baseline: Option<bool>,
        /// Capture the computer now (default true); false compares the last capture.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh: Option<bool>,
    }
}

wire_struct! {
    ChangesDiffResult {
        /// The `silo diff --json` document.
        diff: serde_json::Value,
        /// The readable form of `diff`.
        text: String,
        base_captured_at: String,
        captured_at: String,
        baseline_applied: bool,
        baseline_available: bool,
        /// The checkpoint (or computer) the base was forked from, when recorded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_source: Option<String>,
        /// Seconds between the base capture and this capture.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        elapsed_seconds: Option<u64>,
        /// A digest of the changes a keep acts on; `silo merge --reviewed` pins a merge to it.
        #[serde(default)]
        review_digest: String,
    }
}

wire_struct! {
    ChangesBaselineParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        /// The checkpoint the idle fork came from; defaults to the source recorded with its base.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        /// How long after the base capture the fork is left idle before it is captured again.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        idle_seconds: Option<u64>,
    }
}

wire_struct! {
    ChangesBaselineResult {
        source: String,
        idle_seconds: u64,
        captured_at: String,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_params_default_to_a_latest_capture() {
        let params: ChangesCaptureParams = serde_json::from_str(r#"{"computerId":"c"}"#).unwrap();
        assert_eq!(params.role, None);
        let text = serde_json::to_string(&params).unwrap();
        assert_eq!(text, r#"{"computerId":"c"}"#);
        let base: ChangesCaptureParams =
            serde_json::from_str(r#"{"computerId":"c","role":"base","source":"cp1"}"#).unwrap();
        assert_eq!(base.role, Some(ChangesRole::Base));
        assert_eq!(base.source.as_deref(), Some("cp1"));
    }

    #[test]
    fn diff_result_carries_the_document_and_the_text() {
        let result = ChangesDiffResult {
            diff: serde_json::json!({"code": []}),
            text: "No changes.".into(),
            base_captured_at: "a".into(),
            captured_at: "b".into(),
            baseline_applied: false,
            baseline_available: true,
            base_source: Some("cp1".into()),
            elapsed_seconds: Some(30),
            review_digest: "d".into(),
        };
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["baselineAvailable"], true);
        assert_eq!(
            serde_json::from_value::<ChangesDiffResult>(value).unwrap(),
            result
        );
    }
}
