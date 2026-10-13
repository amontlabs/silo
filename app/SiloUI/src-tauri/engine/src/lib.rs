//! Tauri-free backend library for Silo.

pub mod channel;
pub mod context;

pub use channel::{Channel, Keychain};
pub use context::{
    Audience, ClientId, EngineContext, EngineIdentity, EnginePaths, Event, MainThread, PackageKind,
    ProcessControl,
};
