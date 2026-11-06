//! The `silo` command line and its MCP server, built on `silo-client` and the typed
//! requests in `silo-api`.

pub mod access;
pub mod autokeep;
pub mod cli;
pub mod clock;
pub mod config;
pub mod engine;
pub mod error;
pub mod fanout;
pub mod flows;
pub mod folder;
pub mod guest_client;
pub mod guest_install;
pub mod mcp;
pub mod ops;
pub mod pr;
pub mod render;
pub mod review;
pub mod steps;
pub mod summary;
pub mod writeback;
