//! Retained, client-rendered notices.

use super::common::DeviceParams;
use serde::{Deserialize, Serialize};

pub type NotificationsListParams = DeviceParams;
pub type NotificationsListResult = Vec<Notice>;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NoticeCategory {
    Failures,
    Changes,
    Completions,
}

wire_struct! {
    NoticeComputer {
        id: String,
        name: String,
    }
}

wire_struct! {
    Notice {
        category: NoticeCategory,
        key: String,
        title: String,
        body: String,
        computer: Option<NoticeComputer>,
    }
}

wire_struct! {
    NotificationsClearedPayload {
        computer_id: String,
    }
}
