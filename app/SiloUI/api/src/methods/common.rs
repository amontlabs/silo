//! Shared parameter and result shapes used by multiple domains.

wire_struct! {
    EmptyParams {}
}

wire_struct! {
    DeviceParams {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    ComputerTargetParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    }
}

wire_struct! {
    JobStartResult {
        job_id: crate::jobs::JobId,
    }
}
