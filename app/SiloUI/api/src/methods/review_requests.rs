//! Review requests: `changes.requests.*` list, show, keep, throw away and open the requests
//! `changes.request` records.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::keep::{FoldRequest, KeepHooks, KeepRefusal, KeepReport};

/// Which requests `changes.requests.list` returns.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum RequestFilter {
    #[default]
    Open,
    Kept,
    ThrownAway,
    All,
}

wire_struct! {
    ChangesRequestsListParams {
        /// Defaults to `open`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<RequestFilter>,
    }
}

wire_struct! {
    /// One request with the fork it is about.
    RequestEntry {
        request: FoldRequest,
        /// The fork's name; `None` when the fork no longer exists.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fork_name: Option<String>,
        /// `running`, `stopped`, and so on; `None` when the fork no longer exists.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fork_state: Option<String>,
        /// The target computer's name; `None` for a folder or a computer that is gone.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_name: Option<String>,
    }
}

wire_struct! {
    ChangesRequestsListResult {
        requests: Vec<RequestEntry>,
    }
}

wire_struct! {
    /// A request named by its id.
    RequestIdParams {
        request_id: String,
    }
}

wire_struct! {
    /// A secret a fork's agent used: which one, towards which host, and how many times.
    CredentialUse {
        secret: String,
        host: String,
        method: String,
        count: u64,
    }
}

wire_struct! {
    /// A request the proxy refused because a secret was not allowed for it.
    CredentialBlock {
        host: String,
        method: String,
        count: u64,
        reason: String,
    }
}

wire_struct! {
    /// The credentials a fork's agent used and the ones it was refused.
    CredentialUsage {
        #[serde(default)]
        used: Vec<CredentialUse>,
        #[serde(default)]
        blocked: Vec<CredentialBlock>,
    }
}

impl Default for CredentialUsage {
    fn default() -> Self {
        Self {
            used: Vec::new(),
            blocked: Vec::new(),
        }
    }
}

wire_struct! {
    /// A request, a fresh diff of its fork and the credentials the fork used.
    ChangesRequestsShowResult {
        request: FoldRequest,
        fork_name: String,
        fork_state: String,
        /// `true` when the fork's changes differ from the ones the request was made from; a
        /// keep of the request refuses. `None` when this cannot be told (a folder fork).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stale: Option<bool>,
        /// The digest of the fork's changes now.
        review_digest: String,
        /// Whether the fork was looked at now (`true`) or the last look was used (the fork
        /// is stopped).
        fresh: bool,
        /// The `silo diff --json` document; `null` for a folder fork.
        diff: Value,
        /// The readable form of `diff`, or the request's own summary for a folder fork.
        text: String,
        /// The number of changes in each bucket of `diff`.
        counts: BTreeMap<String, u64>,
        credentials: CredentialUsage,
    }
}

wire_struct! {
    ChangesRequestsKeepParams {
        request_id: String,
        /// Carry database changes even though no migration step covers them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data_ok: Option<bool>,
        /// The recipe's `keep:` steps to run around the keep.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep: Option<KeepHooks>,
    }
}

wire_struct! {
    /// The result of `changes.requests.keep`: the keep's own result and the request after it.
    /// A refused keep leaves the request open.
    ChangesRequestsKeepResult {
        /// `kept` or `refused`.
        status: String,
        report: KeepReport,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refusal: Option<KeepRefusal>,
        request: FoldRequest,
    }
}

wire_struct! {
    ChangesRequestsThrowAwayResult {
        request: FoldRequest,
        /// The name of the fork that was dropped.
        dropped: String,
    }
}

wire_struct! {
    ChangesRequestsOpenResult {
        request: FoldRequest,
        computer_id: String,
        name: String,
        /// The state after the call: `running` unless the fork could not be started.
        state: String,
        /// Whether this call started the fork.
        started: bool,
        /// The SSH alias that reaches the fork, when one is set up.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ssh_alias: Option<String>,
        /// The URLs the fork's published ports answer on.
        #[serde(default)]
        urls: Vec<String>,
    }
}

/// The number of changes in each bucket of a `silo diff --json` document.
pub fn bucket_counts(diff: &Value) -> BTreeMap<String, u64> {
    fn count(value: &Value) -> u64 {
        match value {
            Value::Array(items) => items.len() as u64,
            Value::Object(fields) => fields.values().map(count).sum(),
            _ => 0,
        }
    }
    ["code", "packages", "settings", "database", "other"]
        .into_iter()
        .map(|bucket| (bucket.to_owned(), diff.get(bucket).map_or(0, count)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_count_arrays_and_nested_settings() {
        let diff = serde_json::json!({
            "code": [1, 2], "packages": [], "database": [1],
            "settings": {"shell": [1], "cron": [1, 2]}, "other": [1]
        });
        let counts = bucket_counts(&diff);
        assert_eq!(counts["code"], 2);
        assert_eq!(counts["settings"], 3);
        assert_eq!(counts["packages"], 0);
        assert_eq!(counts["other"], 1);
        assert_eq!(bucket_counts(&serde_json::json!({}))["code"], 0);
    }

    #[test]
    fn the_filter_defaults_to_open_and_names_kebab_case() {
        let params: ChangesRequestsListParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params.status.unwrap_or_default(), RequestFilter::Open);
        let all: ChangesRequestsListParams =
            serde_json::from_str(r#"{"status":"thrown-away"}"#).unwrap();
        assert_eq!(all.status, Some(RequestFilter::ThrownAway));
    }

    #[test]
    fn credentials_default_to_empty() {
        let none: CredentialUsage = serde_json::from_str("{}").unwrap();
        assert!(none.used.is_empty() && none.blocked.is_empty());
    }
}
