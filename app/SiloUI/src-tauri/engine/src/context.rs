use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use crate::channel::Channel;

/// Everything engine logic receives from its host process instead of a UI framework handle.
pub trait EngineContext: Send + Sync + 'static {
    fn paths(&self) -> &EnginePaths;
    fn identity(&self) -> &EngineIdentity;
    fn runtime_state(&self) -> Arc<EngineRuntimeState>;
    fn publish(&self, event: Event, audience: Audience);
    fn main_thread(&self) -> Option<&dyn MainThread>;
    fn process(&self) -> &dyn ProcessControl;

    /// Ends client-owned remote display sessions. `None` closes all sessions.
    fn close_remote_displays(&self, _device: Option<&str>) {}
}

/// Publishes progress that belongs only in Silo's main window.
pub fn publish_to_main_window(context: &dyn EngineContext, event: Event) {
    context.publish(event, Audience::Client(ClientId::new("main")));
}

/// Mutable state whose lifetime belongs to one installed engine instance.
///
/// Keeping these stores here makes their ownership explicit: a second engine
/// context receives fresh settings, shutdown, quit-confirmation, and startup
/// state instead of inheriting process-wide state from an earlier instance.
#[derive(Default)]
pub struct EngineRuntimeState {
    settings: Arc<crate::settings::SettingsState>,
    shutdown: Arc<crate::settings::ShutdownState>,
    quit_confirmation: Arc<crate::settings::QuitConfirmation>,
    startup: Arc<crate::startup::StartupState>,
}

impl EngineRuntimeState {
    pub fn settings(&self) -> &Arc<crate::settings::SettingsState> {
        &self.settings
    }

    pub fn shutdown(&self) -> &Arc<crate::settings::ShutdownState> {
        &self.shutdown
    }

    pub fn quit_confirmation(&self) -> &Arc<crate::settings::QuitConfirmation> {
        &self.quit_confirmation
    }

    pub fn startup(&self) -> &Arc<crate::startup::StartupState> {
        &self.startup
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct RecordingContext {
        paths: EnginePaths,
        identity: EngineIdentity,
        runtime_state: Arc<EngineRuntimeState>,
        published: Mutex<Vec<(Event, Audience)>>,
    }

    impl EngineContext for RecordingContext {
        fn paths(&self) -> &EnginePaths {
            &self.paths
        }

        fn identity(&self) -> &EngineIdentity {
            &self.identity
        }

        fn runtime_state(&self) -> Arc<EngineRuntimeState> {
            self.runtime_state.clone()
        }

        fn publish(&self, event: Event, audience: Audience) {
            self.published.lock().unwrap().push((event, audience));
        }

        fn main_thread(&self) -> Option<&dyn MainThread> {
            None
        }

        fn process(&self) -> &dyn ProcessControl {
            self
        }
    }

    impl ProcessControl for RecordingContext {
        fn exit(&self, _code: i32) {}

        fn restart(&self) -> ! {
            panic!("test context cannot restart")
        }
    }

    #[test]
    fn main_window_progress_uses_the_main_client_audience() {
        let context = RecordingContext {
            paths: EnginePaths::default(),
            identity: EngineIdentity {
                channel: crate::channel::Channel::Production,
                version: "0.11.1".into(),
                package_kind: PackageKind::MacApp,
            },
            runtime_state: Arc::new(EngineRuntimeState::default()),
            published: Mutex::new(Vec::new()),
        };

        publish_to_main_window(&context, Event::new("progress", serde_json::Value::Null));

        let published = context.published.lock().unwrap();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0.name, "progress");
        assert_eq!(
            published[0].1,
            Audience::Client(ClientId::new("main"))
        );
    }
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
