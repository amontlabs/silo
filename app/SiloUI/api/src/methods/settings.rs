//! Shared settings snapshots and validated patches.

use serde::{Deserialize, Deserializer, Serialize};

pub type SettingsReadParams = super::common::EmptyParams;
pub type SettingsFlushParams = super::common::EmptyParams;
pub type SettingsReadResult = SettingsSnapshot;
pub type SettingsUpdateResult = SettingsSnapshot;
pub type SettingsFlushResult = ();

wire_struct! {
    SettingsUpdateParams {
        patch: SettingsValues,
    }
}

/// Known settings exposed by 60 §2.3. Unknown keys are rejected by the API;
/// disk migration and legacy preservation remain engine concerns.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsValues {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "Theme")]
    pub theme: Option<Theme>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "bool")]
    pub reduce_motion: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "bool")]
    pub launch_at_login: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "bool")]
    pub start_computers_at_launch: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "Vec<String>")]
    pub startup_computer_ids: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "bool")]
    pub onboarding_complete: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "bool")]
    pub computer_use_auto_approval: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "bool")]
    pub notifications_enabled: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "bool")]
    pub notify_failures: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "bool")]
    pub notify_changes: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "bool")]
    pub notify_completions: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "String")]
    pub terminal: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "String")]
    pub editor: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "String")]
    pub browser: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present_nullable"
    )]
    #[schemars(with = "Option<String>")]
    pub terminal_path: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present_nullable"
    )]
    #[schemars(with = "Option<String>")]
    pub editor_path: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present_nullable"
    )]
    #[schemars(with = "Option<String>")]
    pub browser_path: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "bool")]
    pub terminal_use_system_default: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "bool")]
    pub editor_use_system_default: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "bool")]
    pub browser_use_system_default: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "Vec<String>")]
    pub computer_order: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "bool")]
    pub alpha_notice_dismissed: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present_nullable"
    )]
    #[schemars(with = "Option<String>")]
    pub editor_include_notice_dismissed: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "u64")]
    pub automatic_checkpoint_retention_count: Option<u64>,
}

fn deserialize_present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

fn deserialize_present_nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

/// Mirrors `app/SiloUI/src-tauri/engine/src/settings.rs::Snapshot`, with the
/// engine-owned API draft fixed to JSON null as 60 §2.1 and 95 A-5 require.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub revision: u64,
    pub settings: SettingsValues,
    pub onboarding_draft: (),
    pub save_error: Option<String>,
    pub write_protected: bool,
}

pub type SettingsChangedPayload = SettingsSnapshot;
