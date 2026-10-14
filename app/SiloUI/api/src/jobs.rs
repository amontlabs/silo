//! Shared job ids, progress, terminal results, and error codes.

use std::{borrow::Cow, fmt, str::FromStr};

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

use crate::methods::{
    computers::ComputerConfigurationProgress, exports::BackupOperation, files::TransferProgress,
    operations::OperationKind, push::PushOperation, updates::UpdateProgressData,
};

/// Stable snake_case wire error code. Existing values are sourced from
/// `app/SiloUI/src-tauri/src/bridge_error.rs`; pending values are listed in 95 A-10.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    UpdateInProgress,
    UnsupportedRemoteOperation,
    Cancelled,
    AlreadyQueued,
    Busy,
    NotFound,
    IncompatibleVersion,
    Internal,
    NotRunning,
    OpenSshMissing,
    IncludeNotWritable,
    AccountNotMigrated,
    TimedOut,
    OutputTooLarge,
    StdinClosed,
    PermissionDenied,
    NotAFile,
    Exists,
    Changed,
    RunningLimit,
    NeedsPersonalizing,
    SetupIncomplete,
    ScreenSharingUnavailable,
}

/// A string JobId with one of the six canonical id-space prefixes.
///
/// The opaque domain id is preserved after the first colon. `job:` ids use a
/// decimal counter; its durable allocation remains an engine concern.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct JobId(String);

impl JobId {
    pub fn new(value: impl Into<String>) -> Result<Self, JobIdError> {
        let value = value.into();
        validate_job_id(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn space(&self) -> JobSpace {
        match self.0.split_once(':').map(|(space, _)| space) {
            Some("setup") => JobSpace::Setup,
            Some("transfer") => JobSpace::Transfer,
            Some("push") => JobSpace::Push,
            Some("exports") => JobSpace::Exports,
            Some("inspect") => JobSpace::Inspect,
            Some("job") => JobSpace::Job,
            _ => unreachable!("JobId validates its space on construction"),
        }
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for JobId {
    type Err = JobIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for JobId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for JobId {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("JobId")
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^((setup|transfer|push|exports|inspect):.+|job:[0-9]+)$",
            "description": "Opaque id in setup:, transfer:, push:, exports:, inspect:, or decimal job: space"
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobSpace {
    Setup,
    Transfer,
    Push,
    Exports,
    Inspect,
    Job,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobIdError(&'static str);

impl fmt::Display for JobIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for JobIdError {}

fn validate_job_id(value: &str) -> Result<(), JobIdError> {
    let (space, id) = value
        .split_once(':')
        .ok_or(JobIdError("job id must contain a space prefix and colon"))?;
    if id.is_empty() {
        return Err(JobIdError("job id payload must not be empty"));
    }
    match space {
        "setup" | "transfer" | "push" | "exports" | "inspect" => Ok(()),
        "job" if id.bytes().all(|byte| byte.is_ascii_digit()) => Ok(()),
        "job" => Err(JobIdError("job: ids must use a decimal counter")),
        _ => Err(JobIdError("unknown job id space")),
    }
}

/// The values emitted as the `data` field of `operations.job.progress`.
///
/// Payload member names mirror their source serde structs. The macOS contract
/// uses `computerId` as stated in 05 §4.2 E; the current MacosComputer row uses
/// `id`, a source/spec discrepancy recorded in the implementation report.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(
    tag = "source",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum JobProgress {
    Setup {
        job_id: JobId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<OperationKind>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
        data: ComputerConfigurationProgress,
    },
    Lifecycle {
        job_id: JobId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<OperationKind>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
        data: LifecycleProgress,
    },
    Transfer {
        job_id: JobId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<OperationKind>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
        data: TransferProgress,
    },
    Backup {
        job_id: JobId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<OperationKind>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
        data: BackupOperation,
    },
    Push {
        job_id: JobId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<OperationKind>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
        data: PushOperation,
    },
    Macos {
        job_id: JobId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<OperationKind>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
        data: MacosJobProgress,
    },
    Update {
        job_id: JobId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<OperationKind>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_id: Option<String>,
        data: UpdateProgressData,
    },
}

/// Canonical lifecycle payload at `runtime.rs:3343`; the step values mirror
/// `app/SiloUI/src-tauri/src/runtime.rs::LifecycleStep` (line 1275).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleProgress {
    pub computer_id: String,
    pub action: String,
    pub step: LifecycleStep,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleStep {
    Boot,
    Network,
    Account,
}

/// The macOS progress projection from 05 §4.2 E (`MacosComputer` row fields).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MacosJobProgress {
    pub computer_id: String,
    pub state: crate::methods::computers::MacosComputerState,
    pub progress: Option<f64>,
    pub detail: Option<String>,
    pub checkpoint_operation: Option<crate::methods::checkpoints::Operation>,
}

/// One terminal job event. `result` remains the untagged final JSON value
/// returned by the method that started the job. The schema's
/// `x-silo-job-results` catalogue maps methods to their terminal result schemas.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobFinished {
    pub job_id: JobId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::transport::ErrorObject>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobOutcome {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::transport::ErrorObject>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Waiting,
    Running,
    Finished,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobLookupResult {
    pub job_id: JobId,
    pub state: JobState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<JobOutcome>,
}

/// Kept in this module as a re-export for callers that model the shared job
/// acknowledgement contract directly.
pub use crate::methods::operations::AcknowledgeJobParams;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_id_accepts_each_canonical_space_and_round_trips() {
        for value in [
            "setup:request-1",
            "transfer:transfer-1",
            "push:operation-1",
            "exports:archive-1",
            "inspect:request-1",
            "job:0",
            "job:42",
        ] {
            let id: JobId = serde_json::from_str(&serde_json::to_string(value).unwrap()).unwrap();
            assert_eq!(id.as_str(), value);
            assert_eq!(
                serde_json::from_str::<JobId>(&serde_json::to_string(&id).unwrap()).unwrap(),
                id
            );
        }
    }

    #[test]
    fn job_id_rejects_unknown_or_malformed_spaces() {
        for value in ["x:1", "setup:", "job:abc", "job:-1", "no-colon"] {
            assert!(JobId::new(value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn job_progress_uses_camel_case_for_envelope_and_source_fields() {
        let progress = JobProgress::Lifecycle {
            job_id: JobId::new("job:12").unwrap(),
            kind: Some(OperationKind::Lifecycle),
            computer_id: Some("computer-1".into()),
            data: LifecycleProgress {
                computer_id: "computer-1".into(),
                action: "start".into(),
                step: LifecycleStep::Boot,
            },
        };
        let value = serde_json::to_value(progress).unwrap();
        assert_eq!(value["source"], "lifecycle");
        assert_eq!(value["jobId"], "job:12");
        assert_eq!(value["computerId"], "computer-1");
        assert_eq!(value["data"]["computerId"], "computer-1");
        assert!(value.get("job_id").is_none());
        assert!(value["data"].get("computer_id").is_none());
    }
}
