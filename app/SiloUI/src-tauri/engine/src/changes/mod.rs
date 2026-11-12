//! Change capture for forks: collect guest manifests, parse them and compare them into a
//! bucketed, reviewable [`diff::Diff`]. Format and contract: `docs/v1/PHASE2_CONTRACT.md`;
//! output shape: `docs/SiloUI-CHANGES.md`.

pub mod collect;
pub mod credentials;
pub mod diff;
pub mod fork_base;
pub mod ignore;
pub mod items;
pub mod keep;
pub mod manifest;
pub mod promote;
pub mod service;
pub mod store;

#[cfg(test)]
mod tests;

pub use diff::{diff, Diff};
pub use ignore::IgnoreList;
pub use manifest::Manifest;
