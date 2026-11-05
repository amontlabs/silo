//! Change capture for forks: parse guest manifests and compare them into a bucketed,
//! reviewable [`diff::Diff`]. Format and contract: `docs/v1/PHASE2_CONTRACT.md`;
//! output shape: `docs/SiloUI-CHANGES.md`.

pub mod diff;
pub mod ignore;
pub mod keep;
pub mod manifest;

#[cfg(test)]
mod tests;

pub use diff::{diff, Diff};
pub use ignore::IgnoreList;
pub use manifest::Manifest;
