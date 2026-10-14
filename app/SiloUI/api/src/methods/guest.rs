//! Guest account, process, file, and SSH contracts.

use serde::{Deserialize, Serialize};

use super::common::{ComputerTargetParams, JobStartResult};

wire_struct! { GuestAccountParams { computer_id: String, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String> } }
wire_struct! { GuestAccount { user: String, uid: u32, home: String } }

wire_struct! {
    GuestEnvVar { name: String, value: String }
}

wire_struct! {
    GuestExecParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        argv: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        env: Option<Vec<GuestEnvVar>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(rename = "as")]
        as_user: Option<GuestExecUser>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stdin: Option<Vec<u8>>,
        timeout_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_output_bytes: Option<u64>,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GuestExecUser {
    Account,
    Root,
}

wire_struct! {
    GuestExecResult {
        exit_code: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signal: Option<String>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    }
}

wire_struct! {
    GuestExecStartParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        argv: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        env: Option<Vec<GuestEnvVar>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(rename = "as")]
        as_user: Option<GuestExecUser>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stdin: Option<Vec<u8>>,
        stdin_open: bool,
        retain_output_bytes: u64,
    }
}

pub type GuestExecStartResult = JobStartResult;

wire_struct! {
    GuestExecInputParams {
        job_id: crate::jobs::JobId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<Vec<u8>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        close: Option<bool>,
    }
}

wire_struct! {
    GuestExecGetParams {
        job_id: crate::jobs::JobId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        since_seq: Option<u64>,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GuestExecState {
    Running,
    Exited,
    Cancelled,
    Lost,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GuestExecStream {
    Stdout,
    Stderr,
}

wire_struct! { GuestExecOutput { seq: u64, stream: GuestExecStream, data: Vec<u8> } }
wire_struct! { GuestExecGetResult { state: GuestExecState, #[serde(default, skip_serializing_if = "Option::is_none")] exit_code: Option<i32>, output: Vec<GuestExecOutput>, first_retained_seq: u64 } }
wire_struct! { GuestExecFinishedResult { exit_code: Option<i32>, #[serde(default, skip_serializing_if = "Option::is_none")] signal: Option<String> } }

wire_struct! { GuestFilesListParams { computer_id: String, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String>, path: String } }
wire_struct! { GuestFileEntry { name: String, #[serde(rename = "type")] kind: GuestFileKind, size: u64, mtime_ms: u64 } }

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GuestFileKind {
    File,
    Dir,
    Link,
    Other,
}

wire_struct! { GuestFilesListResult { entries: Vec<GuestFileEntry> } }
wire_struct! { GuestFilesReadParams { computer_id: String, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String>, path: String, #[serde(default, skip_serializing_if = "Option::is_none")] offset: Option<u64>, #[serde(default, skip_serializing_if = "Option::is_none")] length: Option<u64> } }
wire_struct! { GuestFilesReadResult { data: Vec<u8>, size: u64, mtime_ms: u64, inode: u64, mode: u32 } }

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum GuestFileReplace {
    Always,
    Never,
    IfSha256,
}

wire_struct! { GuestFilesWriteParams { computer_id: String, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String>, path: String, data: Vec<u8>, mode: u32, #[serde(default, skip_serializing_if = "Option::is_none")] create_parents: Option<bool>, replace: GuestFileReplace, #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_present_nullable")] #[schemars(with = "Option<String>")] expected_sha256: Option<Option<String>> } }

fn deserialize_present_nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}
wire_struct! { GuestFilesWriteResult { size: u64, mtime_ms: u64, inode: u64 } }
wire_struct! { GuestFilesRemoveParams { computer_id: String, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String>, paths: Vec<String> } }
wire_struct! { GuestFilesRemoveResult { removed: Vec<String> } }
wire_struct! { GuestFilesWatchParams { computer_id: String, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String>, paths: Vec<String> } }
wire_struct! { GuestFilesWatchResult { subscription_id: String } }
wire_struct! { GuestFilesChangedPayload { subscription_id: String, path: String } }

wire_struct! {
    GuestPrepareParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        public_key: String,
        path: String,
        account_protocol: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        forwarding: Option<bool>,
    }
}

wire_struct! { GuestPrepareResult { host_public_key: String, user: String, guest_address: Option<String> } }

wire_struct! {
    GuestSshParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        purpose: Option<GuestSshPurpose>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<u16>,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GuestSshPurpose {
    Port,
}

wire_struct! { GuestSshAliasEnsureParams { computer_id: String, #[serde(default, skip_serializing_if = "Option::is_none")] device_id: Option<String>, #[serde(default, skip_serializing_if = "Option::is_none")] install_include: Option<bool>, #[serde(default, skip_serializing_if = "Option::is_none")] path: Option<String> } }
wire_struct! { GuestSshAliasEnsureResult { alias: String, user: String, home: String, config_path: String, include_line: String, include_installed: bool } }
pub type GuestSshAliasRemoveParams = ComputerTargetParams;

#[cfg(test)]
mod tests {
    use super::GuestExecGetParams;
    use crate::jobs::JobId;

    #[test]
    fn exec_get_accepts_an_optional_owner_device() {
        let local: GuestExecGetParams = serde_json::from_value(serde_json::json!({
            "jobId": "job:1",
            "sinceSeq": 3
        }))
        .unwrap();
        assert_eq!(local.device_id, None);

        let remote: GuestExecGetParams = serde_json::from_value(serde_json::json!({
            "jobId": "job:1",
            "deviceId": "owner-device",
            "sinceSeq": 3
        }))
        .unwrap();
        assert_eq!(remote.device_id.as_deref(), Some("owner-device"));
        assert_eq!(remote.job_id, JobId::new("job:1").unwrap());
    }
}
