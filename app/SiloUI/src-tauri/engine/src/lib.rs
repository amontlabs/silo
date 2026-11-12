//! Engine library for Silo.
// The macOS computer code compiles everywhere and runs only on macOS, as in silod.
#![cfg_attr(not(target_os = "macos"), allow(dead_code, private_interfaces))]

extern crate self as silo_engine;

pub mod agent_tools;
pub mod approvals;
pub mod audit;
pub mod agents;
pub mod backup;
pub mod backup_controller;
pub mod bridge_error;
pub mod bundled_tools;
pub mod changes;
pub mod checkpoints;
pub(crate) mod child_process;
pub mod clipboard;
pub mod computer_gateway;
pub mod containers;
pub mod computer_names;
pub mod computer_tree;
pub mod export_scrub;
pub mod computer_use;
pub mod context;
pub mod creation_inputs;
pub mod credential_access;
#[cfg(target_os = "linux")]
pub mod credential_file_store;
pub mod credential_store;
pub mod credentials_usage;
pub mod dependencies;
pub mod desktop;
pub mod desktop_bridge;
pub mod desktop_proxy;
pub mod desktop_viewer;
pub mod desktop_viewer_api;
pub mod desktop_viewer_media;
pub mod device_facts;
pub mod resources;
pub mod device_identity;
pub(crate) mod download;
pub mod editor;
pub mod engine_update;
pub mod events;
pub mod files;
pub mod fold_requests;
pub mod gateway_control;
pub mod generation;
pub mod github;
pub mod github_http;
pub mod github_tokens;
pub mod guest;
pub mod guest_client;
pub mod guest_components;
pub mod health_watch;
pub mod host_push;
pub mod host_push_cache;
pub mod host_push_operations;
pub mod host_push_transport;
pub mod jobs;
pub mod legacy_mounts;
pub mod log_export;
pub mod log_retention;
#[path = "macos_computers.rs"]
pub mod macos_computers_engine;
#[cfg(target_os = "linux")]
pub mod safe_directories;
pub use macos_computers_engine as macos_computers;
pub mod macos_display_gateway;
#[path = "macos_remote.rs"]
pub mod macos_remote_engine;
pub mod network;
pub mod network_policy;
pub mod notices;
pub mod owned_tunnel;
pub mod policy;
pub mod pre_upgrade_backup;
pub mod preparation;
pub mod recipes;
pub mod rebuild;
pub mod remote;
pub mod remote_access;
pub mod remote_events;
pub mod remote_network;
pub mod remote_ssh_access;
pub mod remote_watch;
pub mod runtime;
pub mod runtime_migration;
pub mod secrets;
pub mod settings;
pub mod ssh_access;
pub mod ssh_connection;
pub mod startup;
pub mod steps;
pub mod sync;
pub mod terminal;
pub mod transfer;
pub mod transfer_area;
pub mod updates;
pub mod url_proxy;
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
pub use silo_host::{applications, channel, notifications, system_integrations, system_shutdown};
