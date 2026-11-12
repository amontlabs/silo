//! Host facts and dependency-report methods.

use super::common::DeviceParams;
use serde::{Deserialize, Serialize};

wire_struct! {
    DependencyReportParams {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
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
        remediation: Option<String>,
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
pub type DeviceResourcesParams = DeviceParams;

/// How short of memory or disk the device is, judged against the floors Silo keeps free.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ResourceState {
    /// Enough is free.
    Ok,
    /// Less than twice the floor is free: starting another computer works but may slow the device.
    Low,
    /// Less than the floor is free: an agent inside a computer cannot start or create another.
    Critical,
}

/// The system's own reading of memory pressure.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryPressure {
    Normal,
    Warning,
    Critical,
}

wire_struct! {
    #[doc = "`device.resources`: what this device has left, so an agent can judge whether another computer fits."]
    DeviceResources {
        logical_cpus: u64,
        memory_total_bytes: u64,
        #[doc = "Memory that can be given to a computer now; absent when the system cannot tell."]
        memory_available_bytes: Option<u64>,
        #[doc = "Absent where the system gives no level."]
        memory_pressure: Option<MemoryPressure>,
        #[doc = "Free space on the disk that holds computers."]
        disk_free_bytes: u64,
        disk_total_bytes: u64,
        running_computers: u64,
        #[doc = "The memory the running computers were configured with, in GiB."]
        running_memory_gib: u64,
        running_cpus: u64,
        #[doc = "An agent inside a computer cannot start or create a computer below this much free memory."]
        memory_floor_bytes: u64,
        #[doc = "An agent inside a computer cannot start or create a computer below this much free disk."]
        disk_floor_bytes: u64,
        state: ResourceState,
        #[doc = "One plain sentence about the state."]
        message: String,
        #[doc = "The computers the caller may see: all of them for the owner, the caller and the computers it made otherwise."]
        computers: Vec<ComputerResources>,
    }
}

wire_struct! {
    ComputerResources {
        id: String,
        name: String,
        #[doc = "Whether the computer is running or starting."]
        running: bool,
        cpus: u64,
        #[doc = "The memory the computer is configured with, in GiB."]
        memory_gib: u64,
        #[doc = "The disk the computer is configured with, in GiB: workspace plus runtime storage for a Linux computer, the disk for a macOS computer."]
        disk_gib: u64,
        #[doc = "A Linux computer's workspace storage, in GiB; absent for a macOS computer."]
        workspace_storage_gib: Option<u64>,
        #[doc = "A Linux computer's runtime storage, in GiB; absent for a macOS computer."]
        runtime_storage_gib: Option<u64>,
    }
}
