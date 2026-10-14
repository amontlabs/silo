//! Method contracts split by API domain.

macro_rules! wire_struct {
    ($(#[$item_attribute:meta])* $name:ident { $($(#[$attribute:meta])* $field:ident: $field_type:ty),* $(,)? }) => {
        $(#[$item_attribute])*
        #[derive(Clone, Debug, serde::Deserialize, PartialEq, serde::Serialize, schemars::JsonSchema)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            $($(#[$attribute])* pub $field: $field_type,)*
        }
    };
}

pub mod checkpoints;
pub mod clipboard;
pub mod common;
pub mod computer_use;
pub mod computers;
pub mod connections;
pub mod desktop;
pub mod device;
pub mod display;
pub mod engine;
pub mod exports;
pub mod files;
pub mod github;
pub mod guest;
pub mod logs;
pub mod macos;
pub mod migration;
pub mod network;
pub mod notifications;
pub mod operations;
pub mod preparation;
pub mod push;
pub mod registry;
pub mod secrets;
pub mod settings;
pub mod ssh_access;
pub mod storage;
pub mod terminal;
pub mod updates;

pub use common::EmptyParams;
pub use registry::{
    method_registry, MethodDescriptor, MethodKind, MethodName, MethodParams, MethodResults,
};
