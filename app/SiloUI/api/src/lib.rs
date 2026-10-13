//! Canonical Silo engine wire types.
//!
//! This crate intentionally has no dependency on the Tauri application or the
//! engine implementation. The engine workspace can add it as a path member.

pub mod jobs;
pub mod transport;

pub use jobs::{ErrorCode, JobId};
pub use transport::{ErrorObject, RequestEnvelope, ResponseEnvelope};
