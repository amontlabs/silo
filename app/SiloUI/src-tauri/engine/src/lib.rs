//! Engine library for Silo.

extern crate self as silo_engine;

pub mod applications;
pub mod backup;
pub mod backup_controller;
pub mod bridge_error;
pub mod bundled_tools;
pub mod channel;
pub mod chatgpt_app;
pub mod checkpoints;
pub mod clipboard;
pub mod computer_names;
pub mod computer_use;
pub mod context;
pub mod creation_inputs;
pub mod dependencies;
pub mod desktop;
pub mod desktop_bridge;
pub mod desktop_proxy;
pub mod desktop_viewer;
pub mod desktop_viewer_media;
pub mod device_facts;
pub mod device_identity;
pub mod editor;
pub mod events;
pub mod files;
pub mod github;
pub mod github_http;
pub mod github_tokens;
pub mod health_watch;
pub mod host_push;
pub mod host_push_cache;
pub mod host_push_operations;
pub mod host_push_transport;
pub mod jobs;
pub mod log_export;
pub mod log_retention;
#[path = "macos_computers.rs"]
pub mod macos_computers_engine;
pub use macos_computers_engine as macos_computers;
pub mod macos_display_gateway;
#[path = "macos_remote.rs"]
pub mod macos_remote_engine;
pub mod network;
pub mod notices;
pub mod notifications;
pub mod owned_tunnel;
pub mod pre_upgrade_backup;
pub mod preparation;
pub mod remote;
pub mod remote_access;
pub mod remote_network;
pub mod remote_ssh_access;
pub mod runtime;
pub mod runtime_migration;
pub mod secrets;
pub mod settings;
pub mod ssh_access;
pub mod ssh_connection;
pub mod startup;
pub mod sync;
pub mod system_integrations;
pub mod system_shutdown;
pub mod terminal;
pub mod transfer;
pub mod updates;
pub mod viewer_clipboard;
pub mod working_account;

#[cfg(test)]
#[path = "github_live_tests.rs"]
mod github_live_tests;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use channel::{Channel, Keychain};
pub use context::{
    Audience, ClientId, EngineContext, EngineIdentity, EnginePaths, EngineRuntimeState, Event,
    MainThread, PackageKind, ProcessControl,
};
