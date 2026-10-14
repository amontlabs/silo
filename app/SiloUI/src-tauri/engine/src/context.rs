use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use crate::channel::Channel;

/// Everything engine logic receives from its host process instead of a UI framework handle.
pub trait EngineContext: Send + Sync + 'static {
    fn paths(&self) -> &EnginePaths;
    fn identity(&self) -> &EngineIdentity;
    fn publish(&self, event: Event, audience: Audience);
    fn main_thread(&self) -> Option<&dyn MainThread>;
    fn process(&self) -> &dyn ProcessControl;

    /// Ends client-owned remote display sessions. `None` closes all sessions.
    fn close_remote_displays(&self, _device: Option<&str>) {}
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnginePaths {
    pub app_data: PathBuf,
    pub app_config: PathBuf,
    pub app_cache: PathBuf,
    pub app_log: PathBuf,
    pub resources: PathBuf,
    pub home: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineIdentity {
    pub channel: Channel,
    pub version: String,
    pub package_kind: PackageKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageKind {
    MacApp,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub name: String,
    pub payload: serde_json::Value,
}

impl Event {
    pub fn new(name: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            payload,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ClientId(String);

impl ClientId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Audience {
    All,
    Client(ClientId),
}

pub trait MainThread: Send + Sync {
    fn run(&self, work: Box<dyn FnOnce() + Send>) -> Result<(), String>;
}

pub trait ProcessControl: Send + Sync {
    fn exit(&self, code: i32);
    fn restart(&self) -> !;
}

static CONTEXT: OnceLock<Arc<dyn EngineContext>> = OnceLock::new();

/// Installs the host's context once, at startup. Engine code reads it with [`current`].
pub fn install(context: Arc<dyn EngineContext>) -> Result<(), String> {
    CONTEXT
        .set(context)
        .map_err(|_| "The engine context is already installed".to_owned())
}

/// The installed host context, or `None` before [`install`] (for example in unit tests).
pub fn current() -> Option<Arc<dyn EngineContext>> {
    CONTEXT.get().cloned()
}
