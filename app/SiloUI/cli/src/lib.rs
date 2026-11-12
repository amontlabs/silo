//! The `silo` command line and its MCP server, built on `silo-client` and the typed
//! requests in `silo-api`.

pub mod access;
pub mod approvals;
pub mod cli;
pub mod clock;
pub mod driver;
pub mod endpoint;
pub mod engine;
pub mod error;
pub mod fanout;
pub mod flows;
pub mod folder;
pub mod gui;
pub mod hosted;
pub mod hosts;
pub mod mcp;
pub mod ops;
pub mod pr;
pub mod recipe_file;
pub mod render;
pub mod review;
pub mod share;
pub mod steps;
pub mod summary;
pub mod tools;
pub mod usetools;
