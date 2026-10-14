//! Host facts and dependency-report methods.

use super::common::DeviceParams;
use serde::{Deserialize, Serialize};

wire_struct! {
    DependencyReportParams {
        request_id: String,
    }
}

wire_struct! {
    DependencyReport {
        schema_version: u8,
        request_id: String,
        checked_at_ms: u64,
        checks: Vec<DependencyCheck>,
    }
}

wire_struct! {
    DependencyCheck {
        id: String,
        title: String,
        status: DependencyStatus,
        detail: String,
        remediation: String,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DependencyStatus {
    Pass,
    Failed,
    Unavailable,
    Timeout,
}

wire_struct! {
    DeviceFacts {
        device_capacity: Option<crate::methods::computers::DeviceCapacity>,
        macos_host_limits: Option<MacosHostLimits>,
        remote_login: RemoteLoginStatus,
    }
}

wire_struct! {
    MacosHostLimits {
        cpus: u64,
        #[serde(rename = "memory_gib")]
        memory_gib: u64,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RemoteLoginStatus {
    Enabled,
    Disabled,
    Unknown,
}

pub type DeviceFactsParams = DeviceParams;
