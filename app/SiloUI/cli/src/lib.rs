//! The `silo` command line and its MCP server, built on `silo-client` and the typed
//! requests in `silo-api`.

pub mod access;
pub mod cli;
pub mod clock;
pub mod config;
pub mod engine;
pub mod error;
pub mod fanout;
pub mod flows;
pub mod folder;
pub mod mcp;
pub mod ops;
pub mod render;
pub mod review;
pub mod steps;
