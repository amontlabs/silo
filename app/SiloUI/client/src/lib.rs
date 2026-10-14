//! A thin client for `engine.sock`: framing, the handshake, request correlation and the
//! session event feed. It knows nothing about engine domains and does not depend on
//! `silo-engine`; the wire types come from `silo-api`.

pub mod channel;
mod codec;
mod session;

/// Selectors for the events an application UI follows, for one subscription that carries them
/// all. The `guest.*` streams are excluded: a client asks for those for itself.
pub const APP_EVENTS: &[&str] = &[
    "computers.*",
    "checkpoints.*",
    "exports.*",
    "migration.*",
    "github.*",
    "secrets.*",
    "push.*",
    "operations.*",
    "network.*",
    "editor.*",
    "engine.*",
    "settings.*",
    "updates.*",
    "preparation.*",
    "notifications.*",
    "connections.*",
    "display.*",
];

pub use codec::{read_frame, write_frame, MAX_FRAME_BYTES};
pub use session::{
    establish, Client, ClientError, ConnectOptions, EventFeed, Handshake, RequestOptions,
};
