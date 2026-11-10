//! Canonical Silo engine wire types.
//!
//! This crate intentionally has no dependency on the Tauri application or the
//! engine implementation. The engine workspace can add it as a path member.

pub mod events;
pub mod jobs;
pub mod methods;
pub mod plain_text;
pub mod schema;
pub mod transport;

pub use jobs::{ErrorCode, JobId};
pub use plain_text::{plain_text, plain_text_within};
pub use schema::{schema_json, write_schema, SiloApiSchema};
pub use transport::{ErrorObject, RequestEnvelope, ResponseEnvelope};
