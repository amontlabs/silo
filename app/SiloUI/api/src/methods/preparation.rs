//! Image, LCU, and ChatGPT application preparation contracts.

use serde::{Deserialize, Serialize};

use super::common::{DeviceParams, EmptyParams};

pub type PreparationStatusParams = EmptyParams;
pub type PreparationRetryParams = EmptyParams;
pub type ChatGptPreparationStatusParams = DeviceParams;
pub type ChatGptPreparationRetryParams = DeviceParams;
pub type PreparationRetryResult = PreparationStatus;
pub type ChatGptPreparationResult = ChatGptPreparationStatus;

wire_struct! {
    #[doc = "Mirrors `app/SiloUI/src-tauri/src/preparation.rs::PreparationStatus`."]
    PreparationStatus {
        image: PreparationTask,
        lcu: PreparationTask,
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(
    tag = "state",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum ChatGptPreparationStatus {
    Idle,
    Downloading {
        received_bytes: u64,
        total_bytes: u64,
    },
    Verifying,
    Extracting,
    Ready {
        path: String,
        version: String,
    },
    Failed {
        reason: String,
        retryable: bool,
    },
}
