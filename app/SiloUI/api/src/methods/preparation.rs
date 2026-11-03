//! Guest image preparation contracts.

use serde::{Deserialize, Serialize};

use super::common::DeviceParams;

pub type PreparationStatusParams = DeviceParams;
pub type PreparationRetryParams = DeviceParams;
pub type PreparationRetryResult = PreparationStatus;

wire_struct! {
    #[doc = "Mirrors `app/SiloUI/src-tauri/engine/src/preparation.rs::PreparationStatus`."]
    PreparationStatus {
        image: PreparationTask,
    }
}

wire_struct! {
    PreparationTask {
        state: PreparationTaskState,
        fraction: Option<u8>,
        message: Option<String>,
        retryable: bool,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PreparationTaskState {
    Pending,
    Running,
    Ready,
    Failed,
}
