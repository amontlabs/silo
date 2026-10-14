//! Filtered log query and export contracts.

use super::common::JobStartResult;
use serde::{Deserialize, Serialize};

wire_struct! {
    LogsQueryParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<LogSource>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        since: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        until: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        around_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        follow: Option<String>,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LogSource {
    All,
    Stdout,
    Stderr,
    Output,
    System,
    Runtime,
    Kernel,
}

wire_struct! {
    LogPage {
        entries: Vec<LogEntry>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_cursor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        oldest_available_timestamp: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        newest_available_timestamp: Option<String>,
        total_matches: u64,
        timestamp_estimated: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unsupported: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unreadable_records: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot: Option<String>,
    }
}

wire_struct! {
    LogEntry {
        id: String,
        line: String,
        occurred_at: String,
        computer_id: String,
        computer_name: String,
        device_id: String,
        device_name: String,
        source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guest_timestamp: Option<String>,
    }
}

wire_struct! {
    LogsExportParams {
        requests: Vec<LogsQueryParams>,
        destination: String,
    }
}

pub type LogsExportResult = JobStartResult;
pub type LogsExportFinishedResult = bool;
