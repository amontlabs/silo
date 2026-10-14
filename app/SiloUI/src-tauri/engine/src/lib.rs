//! Engine library for Silo.

extern crate self as silo_engine;

#[path = "../../src/applications.rs"]
pub mod applications;
pub mod backup;
pub mod backup_controller;
#[path = "../../src/bridge_error.rs"]
pub mod bridge_error;
#[path = "../../src/bundled_tools.rs"]
pub mod bundled_tools;
pub mod channel;
#[path = "../../src/chatgpt_app.rs"]
pub mod chatgpt_app;
#[path = "../../src/clipboard.rs"]
pub mod clipboard;
pub mod computer_names;
#[path = "../../src/computer_use.rs"]
pub mod computer_use;
pub mod creation_inputs;
#[path = "../../src/dependencies.rs"]
pub mod dependencies;
#[path = "../../src/desktop.rs"]
pub mod desktop;
#[path = "../../src/desktop_bridge.rs"]
pub mod desktop_bridge;
#[path = "../../src/desktop_proxy.rs"]
pub mod desktop_proxy;
#[path = "../../src/desktop_viewer.rs"]
pub mod desktop_viewer;
#[path = "../../src/desktop_viewer_media.rs"]
pub mod desktop_viewer_media;
pub mod device_identity;
pub mod editor;
pub mod context;
pub mod files;
pub mod github;
pub mod github_http;
pub mod github_tokens;
pub mod host_push;
pub mod host_push_cache;
pub mod host_push_operations;
pub mod host_push_transport;
pub mod health_watch;
pub mod log_export;
#[path = "../../src/log_retention.rs"]
pub mod log_retention;
#[path = "macos_computers.rs"]
pub mod macos_computers_engine;
pub use macos_computers_engine as macos_computers;
pub mod macos_display_gateway;
#[path = "macos_remote.rs"]
pub mod macos_remote_engine;
pub mod network;
#[path = "../../src/notifications.rs"]
pub mod notifications;
pub mod owned_tunnel;
pub mod pre_upgrade_backup;
#[path = "../../src/preparation.rs"]
pub mod preparation;
pub mod remote;
pub mod remote_access;
pub mod remote_network;
pub mod remote_ssh_access;
pub mod runtime;
pub mod runtime_migration;
pub mod secrets;
#[path = "../../src/settings.rs"]
pub mod settings;
pub mod ssh_access;
pub mod ssh_connection;
pub mod startup;
#[path = "../../src/system_shutdown.rs"]
pub mod system_shutdown;
#[path = "../../src/sync.rs"]
pub mod sync;
pub mod terminal;
pub mod transfer;
#[path = "../../src/updates.rs"]
pub mod updates;
#[path = "../../src/viewer_clipboard.rs"]
pub mod viewer_clipboard;
pub mod working_account;

#[cfg(test)]
#[path = "github_live_tests.rs"]
mod github_live_tests;
#[cfg(test)]
#[path = "../../src/test_support.rs"]
pub(crate) mod test_support;

pub use channel::{Channel, Keychain};
pub use context::{
    Audience, ClientId, EngineContext, EngineIdentity, EnginePaths, Event, MainThread, PackageKind,
    ProcessControl,
};
