//! Shared job ids, progress, terminal results, and error codes.

use std::{borrow::Cow, fmt, str::FromStr};

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

/// Stable wire error code. The first eight names are in today's bridge enum;
/// the remaining variants are the pending A-10 codes in the API spec.
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
            "pattern": "^(setup|transfer|push|exports|inspect):.+|job:[0-9]+$",
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

// The remaining job contract types are completed in the domain modules.

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
}
