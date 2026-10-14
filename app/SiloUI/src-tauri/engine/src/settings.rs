use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};

const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024;
const MAX_DRAFT_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    revision: u64,
    settings: Map<String, Value>,
    onboarding_draft: Value,
    save_error: Option<String>,
    /// The settings file is protected from writes (newer, invalid or unreadable).
    /// Changes apply for this session only; Quit and updates still proceed.
    write_protected: bool,
}

impl Snapshot {
    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    pub fn theme(&self) -> Option<Theme> {
        match self.settings.get("theme").and_then(Value::as_str) {
            Some("light") => Some(Theme::Light),
            Some("dark") => Some(Theme::Dark),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

struct SettingsStore {
    path: Option<PathBuf>,
    document: Map<String, Value>,
    snapshot: Snapshot,
    protected_error: Option<String>,
    dirty: bool,
}

impl SettingsStore {
    fn load(path: Option<PathBuf>) -> Self {
        let mut store = Self {
            path,
            document: json!({"schemaVersion": 1, "settings": {}, "onboardingDraft": null})
                .as_object()
                .unwrap()
                .clone(),
            snapshot: Snapshot {
                revision: 0,
                settings: Map::new(),
                onboarding_draft: Value::Null,
                save_error: None,
                write_protected: false,
            },
            protected_error: None,
            dirty: false,
        };
        if let Some(path) = &store.path {
            match read_document(path) {
                Ok(Some(document)) => {
                    if document.get("schemaVersion").and_then(Value::as_u64) != Some(1) {
                        store.protect("Settings use an unsupported file version; the file was left unchanged.");
                    }
                    if let Some(settings) = document.get("settings").and_then(Value::as_object) {
                        for (key, value) in settings {
                            match valid_setting(key, value) {
                                Some(true) => { store.snapshot.settings.insert(key.clone(), value.clone()); }
                                Some(false) => store.protect("Saved settings contain an invalid value; the file was left unchanged."),
                                None => {}, // Preserve future fields on disk without exposing them to views.
                            }
                        }
                    } else if document.contains_key("settings") {
                        store.protect("Saved settings have an invalid structure; the file was left unchanged.");
                    }
                    if let Some(draft) = document.get("onboardingDraft") {
                        if valid_draft(draft) {
                            store.snapshot.onboarding_draft = draft.clone();
                        } else {
                            store.protect(
                                "Saved onboarding data is invalid; the file was left unchanged.",
                            );
                        }
                    }
                    store.document = document;
                }
                Ok(None) => {}
                Err(error) => store.protect(&format!(
                    "Settings could not be read; the file was left unchanged: {error}"
                )),
            }
        }
        store
    }

    fn protect(&mut self, error: &str) {
        self.protected_error = Some(error.to_owned());
        self.snapshot.save_error = self.protected_error.clone();
        self.snapshot.write_protected = true;
    }

    fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }

    fn update(&mut self, patch: Map<String, Value>) -> Result<Snapshot, String> {
        if patch
            .iter()
            .any(|(key, value)| valid_setting(key, value) != Some(true))
        {
            return Err("Invalid settings change".into());
        }
        if !patch.is_empty() {
            self.snapshot.settings.extend(patch);
            self.changed();
        }
        Ok(self.snapshot())
    }

    fn update_draft(&mut self, draft: Value) -> Result<Snapshot, String> {
        if !valid_draft(&draft) {
            return Err("Invalid onboarding draft".into());
        }
        self.snapshot.onboarding_draft = draft;
        self.changed();
        Ok(self.snapshot())
    }

    fn import_theme(&mut self, theme: String) -> Result<Snapshot, String> {
        if valid_setting("theme", &Value::String(theme.clone())) != Some(true) {
            return Err("Invalid legacy theme".into());
        }
        if !self.snapshot.settings.contains_key("theme") {
            self.snapshot
                .settings
                .insert("theme".into(), Value::String(theme));
            self.changed();
        }
        Ok(self.snapshot())
    }

    fn changed(&mut self) {
        self.snapshot.revision += 1;
        self.dirty = true;
        let _ = self.save();
    }

    fn save(&mut self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(error) = &self.protected_error {
            return Err(error.clone());
        }
        let mut document = self.document.clone();
        let settings = document.entry("settings").or_insert_with(|| json!({}));
        settings
            .as_object_mut()
            .expect("invalid document is write-protected")
            .extend(self.snapshot.settings.clone());
        document.insert(
            "onboardingDraft".into(),
            self.snapshot.onboarding_draft.clone(),
        );
        let result = match &self.path {
            Some(path) => write_document(path, &document)
                .map_err(|error| format!("Settings could not be saved: {error}")),
            None => Ok(()),
        };
        self.snapshot.save_error = result.as_ref().err().cloned();
        if result.is_ok() {
            self.document = document;
            self.dirty = false;
        }
        result
    }

    /// Persist pending changes before Quit or an update. Write protection blocks
    /// only the disk write: the session keeps its in-memory settings and continues.
    fn flush(&mut self) -> Result<(), String> {
        match self.save() {
            Err(error) if self.protected_error.as_ref() == Some(&error) => {
                eprintln!("Silo settings: changes were not saved: {error}");
                Ok(())
            }
            result => result,
        }
    }
}

fn read_document(path: &Path) -> io::Result<Option<Map<String, Value>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "settings file is too large",
        ));
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    value
        .as_object()
        .cloned()
        .map(Some)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "expected a settings object"))
}

fn write_document(path: &Path, document: &Map<String, Value>) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing settings directory"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut bytes = serde_json::to_vec_pretty(document)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "settings file is too large",
        ));
    }
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    // Rename is atomic; syncing the directory makes its new entry durable as well.
    File::open(parent)?.sync_all()
}

fn bounded_string(value: &Value, max: usize, allow_empty: bool) -> bool {
    value
        .as_str()
        .is_some_and(|text| text.encode_utf16().count() <= max && (allow_empty || !text.is_empty()))
}

fn valid_setting(key: &str, value: &Value) -> Option<bool> {
    Some(match key {
        "theme" => matches!(value.as_str(), Some("system" | "light" | "dark")),
        "launchAtLogin"
        | "onboardingComplete"
        | "alphaNoticeDismissed"
        | "startComputersAtLaunch"
        | "reduceMotion"
        | "computerUseAutoApproval"
        | "notificationsEnabled"
        | "notifyFailures"
        | "notifyChanges"
        | "notifyCompletions"
        | "terminalUseSystemDefault"
        | "editorUseSystemDefault"
        | "browserUseSystemDefault" => value.is_boolean(),
        "terminal" | "editor" | "browser" => bounded_string(value, 256, false),
        // The SSH `Include` line whose notice the user dismissed; null until then.
        "editorIncludeNoticeDismissed" => value.is_null() || bounded_string(value, 8192, false),
        "terminalPath" | "editorPath" | "browserPath" => {
            value.is_null()
                || (bounded_string(value, 4096, false)
                    && value
                        .as_str()
                        .is_some_and(|path| Path::new(path).is_absolute()))
        }
        "startupComputerIds" => value.as_array().is_some_and(|ids| {
            ids.len() <= 256 && ids.iter().all(|id| bounded_string(id, 256, false))
        }),
        // This device's computer list order, local and remote; the UI owns the keys.
        "computerOrder" => value.as_array().is_some_and(|keys| {
            keys.len() <= 1024 && keys.iter().all(|key| bounded_string(key, 512, false))
        }),
        "automaticCheckpointRetentionCount" => value
            .as_u64()
            .is_some_and(|count| count <= u64::from(u32::MAX)),
        _ => return None,
    })
}

fn only_fields(object: &Map<String, Value>, required: &[&str], optional: &[&str]) -> bool {
    required.iter().all(|key| object.contains_key(*key))
        && object
            .keys()
            .all(|key| required.contains(&key.as_str()) || optional.contains(&key.as_str()))
}

fn valid_uuid(value: &Value) -> bool {
    let Some(text) = value.as_str() else {
        return false;
    };
    let Ok(id) = uuid::Uuid::try_parse(text) else {
        return false;
    };
    id.hyphenated().to_string().eq_ignore_ascii_case(text)
        && (id.is_nil()
            || text == "ffffffff-ffff-ffff-ffff-ffffffffffff"
            || (id.get_variant() == uuid::Variant::RFC4122
                && (1..=8).contains(&id.get_version_num())))
}

fn valid_name(value: &Value) -> bool {
    value.as_str().is_some_and(|name| {
        (1..=32).contains(&name.len())
            && name.as_bytes()[0].is_ascii_lowercase()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

fn valid_computer(value: &Value, unfinished: bool) -> bool {
    let Some(configuration) = value.as_object() else {
        return false;
    };
    let fields = &[
        "id",
        "name",
        "cpus",
        "maxCPUs",
        "memoryGiB",
        "maxMemoryGiB",
        "workspaceStorageGiB",
        "runtimeStorageGiB",
    ][..];
    if !only_fields(configuration, fields, &["desktop"])
        || configuration.get("desktop").is_some_and(|desktop| {
            desktop.get("startWithComputer").is_none()
                || serde_json::from_value::<crate::desktop::DesktopConfiguration>(desktop.clone())
                    .is_err()
        })
        || !valid_uuid(&configuration["id"])
        || !(if unfinished {
            configuration["name"].is_string()
        } else {
            valid_name(&configuration["name"])
        })
    {
        return false;
    }
    [
        "cpus",
        "maxCPUs",
        "memoryGiB",
        "maxMemoryGiB",
        "workspaceStorageGiB",
        "runtimeStorageGiB",
    ]
    .iter()
    .all(|key| {
        if unfinished {
            configuration[*key].as_f64().is_some_and(f64::is_finite)
        } else {
            configuration[*key]
                .as_u64()
                .is_some_and(|value| (1..=u64::from(u32::MAX)).contains(&value))
        }
    }) && (unfinished
        || (configuration["cpus"].as_u64() <= configuration["maxCPUs"].as_u64()
            && configuration["memoryGiB"].as_u64() <= configuration["maxMemoryGiB"].as_u64()
            && configuration["workspaceStorageGiB"]
                .as_u64()
                .unwrap_or(u64::MAX)
                .checked_add(
                    configuration["runtimeStorageGiB"]
                        .as_u64()
                        .unwrap_or(u64::MAX),
                )
                .is_some_and(|total| total <= u64::from(u32::MAX) / 1024)))
}

// This boundary accepts unfinished text, but never accepts credentials, runtime state, or arbitrary fields.
// TypeScript applies the existing domain validation before a draft is used as configuration.
fn valid_draft(value: &Value) -> bool {
    if value.is_null() {
        return true;
    }
    let Some(draft) = value.as_object() else {
        return false;
    };
    if serde_json::to_vec(value).map_or(true, |bytes| bytes.len() > MAX_DRAFT_BYTES)
        || !only_fields(
            draft,
            &[
                "currentStep",
                "computers",
                "unfinishedComputerEditor",
                "computerSelections",
                "computerIdentities",
            ],
            &["computerRepositoryAccess"],
        )
    {
        return false;
    }
    if !matches!(
        draft["currentStep"].as_str(),
        Some("dependencies" | "computers" | "github" | "review")
    ) || !draft["computers"].as_array().is_some_and(|computers| {
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        computers.len() <= 64
            && computers.iter().all(|configuration| {
                valid_computer(configuration, false)
                    && ids.insert(configuration["id"].as_str().unwrap())
                    && names.insert(configuration["name"].as_str().unwrap().to_lowercase())
            })
    }) {
        return false;
    }
    if !draft["unfinishedComputerEditor"].is_null() {
        let Some(editor) = draft["unfinishedComputerEditor"].as_object() else {
            return false;
        };
        if !only_fields(
            editor,
            &["draft", "insertAt"],
            &["originalID", "displayAfterID"],
        ) || !valid_computer(&editor["draft"], true)
            || !editor["insertAt"].as_f64().is_some_and(|position| {
                position.fract() == 0. && (0. ..=9007199254740991.).contains(&position)
            })
            || ["originalID", "displayAfterID"]
                .iter()
                .any(|key| editor.get(*key).is_some_and(|value| !valid_uuid(value)))
        {
            return false;
        }
    }
    if draft.get("computerRepositoryAccess").is_some_and(|access| {
        !access.as_object().is_some_and(|computers| {
            computers.values().all(|value| {
                value.as_object().is_some_and(|policy| {
                    only_fields(
                        policy,
                        &["repositoryMode", "allRepositoriesAllowChanges"],
                        &["authenticationMethod"],
                    ) && matches!(policy["repositoryMode"].as_str(), Some("selected" | "all"))
                        && policy["allRepositoriesAllowChanges"].is_boolean()
                        && policy
                            .get("authenticationMethod")
                            .is_none_or(|method| matches!(method.as_str(), Some("oauth" | "token")))
                })
            })
        })
    }) {
        return false;
    }
    let Some(selections) = draft["computerSelections"].as_object() else {
        return false;
    };
    let Some(identities) = draft["computerIdentities"].as_object() else {
        return false;
    };
    selections.values().all(|value| {
        value.as_array().is_some_and(|repositories| {
            repositories.iter().all(|value| {
                value.as_object().is_some_and(|repository| {
                    only_fields(repository, &["repository", "allowPushes"], &[])
                        && repository["repository"].is_string()
                        && repository["allowPushes"].is_boolean()
                })
            })
        })
    }) && identities.values().all(|value| {
        value.as_object().is_some_and(|identity| {
            only_fields(identity, &["name", "email", "apply"], &[])
                && identity["name"].is_string()
                && identity["email"].is_string()
                && identity["apply"].is_boolean()
        })
    })
}

struct InitializedSettings {
    store: SettingsStore,
}

#[derive(Default)]
pub struct SettingsState {
    store: Mutex<Option<InitializedSettings>>,
    ready: Condvar,
}

impl SettingsState {
    fn initialize(
        &self,
        path: impl FnOnce() -> Result<Option<PathBuf>, String>,
    ) -> Result<Snapshot, String> {
        let mut initialized = self.store.lock().map_err(|_| "Settings are unavailable")?;
        if let Some(current) = initialized.as_ref() {
            return Ok(current.store.snapshot());
        }
        let store = match path() {
            Ok(path) => SettingsStore::load(path),
            Err(error) => {
                let mut store = SettingsStore::load(None);
                store.protect(&format!("The settings directory is unavailable: {error}"));
                store
            }
        };
        let snapshot = store.snapshot();
        crate::computer_use::sync_initial_approval(&snapshot.settings);
        *initialized = Some(InitializedSettings { store });
        self.ready.notify_all();
        Ok(snapshot)
    }

    fn initialized(&self) -> Result<MutexGuard<'_, Option<InitializedSettings>>, String> {
        let guard = self.store.lock().map_err(|_| "Settings are unavailable")?;
        // This is called only from blocking workers. Waiting releases the mutex so
        // the main window can initialize persistent settings first.
        let (guard, _) = self
            .ready
            .wait_timeout_while(guard, Duration::from_secs(10), |value| value.is_none())
            .map_err(|_| "Settings are unavailable")?;
        if guard.is_none() {
            return Err("Settings have not been initialized by the main window".into());
        }
        Ok(guard)
    }
}

/// How long Quit waits for the webview to acknowledge its settings flush.
pub const FRONTEND_FLUSH_FALLBACK: Duration = Duration::from_secs(2);

pub fn session_flush_wait(deadline: Instant, now: Instant) -> Duration {
    // Keep at least half the remaining session budget for native shutdown.
    FRONTEND_FLUSH_FALLBACK.min(deadline.saturating_duration_since(now) / 2)
}

#[derive(Default)]
pub struct ShutdownState(Mutex<ShutdownProgress>);
#[derive(Default)]
struct ShutdownProgress {
    phase: u8,
    generation: u64,
    restarting: bool,
    /// Logout, shutdown or SIGTERM: stop computers by this time and never cancel the exit.
    session_deadline: Option<Instant>,
}

impl ShutdownState {
    const REQUESTED: u8 = 1;
    const APPROVED: u8 = 2;
    const FINISHING: u8 = 3;
    const FLUSHING: u8 = 4;

    #[cfg(test)]
    fn request(&self) -> bool {
        self.request_generation().is_some()
    }
    pub fn request_generation(&self) -> Option<u64> {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.phase != 0 {
            return None;
        }
        state.phase = Self::REQUESTED;
        state.generation += 1;
        Some(state.generation)
    }
    #[cfg(test)]
    fn generation(&self) -> u64 {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .generation
    }
    pub fn begin_flush(&self) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.phase != Self::REQUESTED {
            return false;
        }
        state.phase = Self::FLUSHING;
        true
    }
    pub fn claim_exit(&self, frontend_completed: bool) -> bool {
        self.claim_exit_for(frontend_completed, None)
    }
    pub fn claim_exit_for(&self, frontend_completed: bool, generation: Option<u64>) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        // When the session ends, the fallback also cuts off a frontend flush that
        // has not finished in time.
        let allowed = if frontend_completed {
            state.phase == Self::FLUSHING
        } else {
            state.phase == Self::REQUESTED
                || (state.session_deadline.is_some() && state.phase == Self::FLUSHING)
        };
        if !allowed || generation.is_some_and(|generation| generation != state.generation) {
            return false;
        }
        state.phase = Self::FINISHING;
        true
    }
    pub fn active(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .phase
            != 0
    }
    pub fn approved(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .phase
            == Self::APPROVED
    }
    #[cfg(test)]
    fn cancel(&self) -> bool {
        self.cancel_with(|| {})
    }
    pub fn cancel_with(&self, reopen: impl FnOnce()) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.session_deadline.is_some() || state.phase == Self::APPROVED {
            return false;
        }
        reopen();
        state.phase = 0;
        true
    }
    /// Keep the earliest deadline when the session end is reported twice.
    pub fn begin_session_end(&self, deadline: Instant) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.session_deadline = Some(
            state
                .session_deadline
                .map_or(deadline, |current| current.min(deadline)),
        );
    }
    pub fn session_deadline(&self) -> Option<Instant> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .session_deadline
    }
    pub fn expire_session(&self, now: Instant) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.phase == Self::APPROVED
            || !state
                .session_deadline
                .is_some_and(|deadline| now >= deadline)
        {
            return false;
        }
        state.phase = Self::APPROVED;
        true
    }
    pub fn allow_exit(&self) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.phase == Self::APPROVED {
            return false;
        }
        state.phase = Self::APPROVED;
        true
    }
    pub fn mark_restart(&self) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .restarting = true;
    }
    pub fn restarting(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .restarting
    }
}

/// How an `ExitRequested` event is handled.
#[derive(Debug, PartialEq, Eq)]
pub enum ExitRequest {
    /// The host restart path. It ignores exit prevention
    /// here, and the installer already stopped computers and saved settings, so
    /// the Quit flow (overlay, flush request, computer shutdown) must not start.
    Restart,
    /// The graceful Quit path finished; let Tauri exit.
    Approved,
    /// Hold the exit until settings are saved and local computers are stopped.
    Gated,
}

pub fn exit_request(restarting: bool, approved: bool) -> ExitRequest {
    if restarting {
        ExitRequest::Restart
    } else if approved {
        ExitRequest::Approved
    } else {
        ExitRequest::Gated
    }
}

fn runtime_state() -> Result<std::sync::Arc<silo_engine::EngineRuntimeState>, String> {
    silo_engine::context::current()
        .map(|context| context.runtime_state())
        .ok_or_else(|| "The engine context is unavailable".to_owned())
}

pub fn shutdown_state() -> std::sync::Arc<ShutdownState> {
    runtime_state()
        .expect("shutdown state is used only after the engine context is installed")
        .shutdown()
        .clone()
}

pub fn shutdown_state_if_installed() -> Option<std::sync::Arc<ShutdownState>> {
    runtime_state().ok().map(|state| state.shutdown().clone())
}

pub fn allow_exit_if_installed() {
    if let Some(state) = shutdown_state_if_installed() {
        state.allow_exit();
    }
}

pub fn quit_confirmation() -> std::sync::Arc<QuitConfirmation> {
    runtime_state()
        .expect("Quit confirmation is used only after the engine context is installed")
        .quit_confirmation()
        .clone()
}

fn configured_path(context: &dyn silo_engine::EngineContext) -> Result<Option<PathBuf>, String> {
    Ok(Some(context.paths().app_config.join("settings.json")))
}

/// Load validated settings from the host-provided settings path.
pub fn initialize(path: Option<PathBuf>) -> Result<Snapshot, String> {
    let state = runtime_state()?;
    state.settings().initialize(|| Ok(path))
}

/// Read validated, persisted preferences. This also supports native callers that
/// run before the settings window has requested its initial snapshot.
pub fn current_settings() -> Result<Map<String, Value>, String> {
    let context = silo_engine::context::current()
        .ok_or_else(|| "The engine context is unavailable".to_owned())?;
    settings_of(context.as_ref())
}

/// Validated, persisted preferences of one engine context.
pub fn settings_of(context: &dyn silo_engine::EngineContext) -> Result<Map<String, Value>, String> {
    let state = context.runtime_state();
    let snapshot = state.settings().initialize(|| configured_path(context))?;
    if let Some(error) = &snapshot.save_error {
        eprintln!("Silo settings: using unsaved settings: {error}");
    }
    Ok(snapshot.settings)
}

pub fn read(main: bool) -> Result<Snapshot, String> {
    let state = runtime_state()?;
    let mut snapshot = state
        .settings()
        .initialized()?
        .as_ref()
        .unwrap()
        .store
        .snapshot();
    if !main {
        snapshot.onboarding_draft = Value::Null;
    }
    Ok(snapshot)
}

fn change(
    state: &SettingsState,
    operation: impl FnOnce(&mut SettingsStore) -> Result<Snapshot, String>,
) -> Result<Snapshot, String> {
    let mut initialized = state.initialized()?;
    let snapshot = operation(&mut initialized.as_mut().unwrap().store)?;
    crate::computer_use::sync_initial_approval(&snapshot.settings);
    Ok(snapshot)
}

pub fn update(patch: Map<String, Value>) -> Result<Snapshot, String> {
    let state = runtime_state()?;
    change(state.settings(), |store| store.update(patch))
}

pub fn update_draft(draft: Value) -> Result<Snapshot, String> {
    let state = runtime_state()?;
    change(state.settings(), |store| store.update_draft(draft))
}

pub fn import_legacy_theme(theme: String) -> Result<Snapshot, String> {
    let state = runtime_state()?;
    change(state.settings(), |store| {
        // Fixture modes never inherit a theme from the production webview origin.
        if store.path.is_none() {
            Ok(store.snapshot())
        } else {
            store.import_theme(theme)
        }
    })
}

pub fn flush() -> Result<(Snapshot, Result<(), String>), String> {
    let state = runtime_state()?;
    let mut saved = Ok(());
    let snapshot = change(state.settings(), |store| {
        store.snapshot.revision += 1;
        saved = store.flush();
        Ok(store.snapshot())
    })?;
    Ok((snapshot, saved))
}

pub fn flush_for_update() -> Result<(), String> {
    let state = runtime_state()?;
    let mut initialized = state
        .settings()
        .store
        .lock()
        .map_err(|_| "Settings could not be saved before updating.")?;
    if let Some(current) = initialized.as_mut() {
        current.store.flush()?;
    }
    Ok(())
}

pub fn flush_pending() -> Result<(), String> {
    let state = runtime_state()?;
    let mut initialized = state
        .settings()
        .store
        .lock()
        .map_err(|_| "Settings storage is unavailable.")?;
    match initialized.as_mut() {
        Some(current) => current.store.flush(),
        None => Ok(()),
    }
}

pub fn public_snapshot(snapshot: &Snapshot) -> Snapshot {
    // Catch-all listeners receive targeted events too; drafts stay in authorized reads.
    let mut public = snapshot.clone();
    public.onboarding_draft = Value::Null;
    public
}

/// Publish the public settings event through the host's event adapter.
pub fn publish_snapshot(
    snapshot: &Snapshot,
    publish: impl FnOnce(silo_engine::Event, silo_engine::Audience),
) {
    publish(
        silo_engine::Event::new(
            "settings:changed",
            serde_json::to_value(public_snapshot(snapshot)).unwrap_or(Value::Null),
        ),
        silo_engine::Audience::All,
    );
}

/// One confirm-capable Quit path (decision 7). Until the main UI opts in with
/// `enable_quit_confirmation`, requests exit directly as before.
#[derive(Default)]
pub struct QuitConfirmation(Mutex<QuitRequests>);
#[derive(Default)]
struct QuitRequests {
    enabled: bool,
    session_ending: bool,
    pending: Option<u64>,
    next: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuitRequest {
    pub request_id: u64,
    /// Running local computer names. Empty means their status could not be read.
    pub computers: Vec<String>,
}

impl QuitConfirmation {
    pub fn enabled(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .enabled
    }
    /// `None` exits now: nothing runs, or no UI can answer.
    pub fn enable(&self) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .enabled = true;
    }
    pub fn ask(&self, running: Result<Vec<String>, String>) -> Option<QuitRequest> {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if !state.enabled || state.session_ending {
            return None;
        }
        let computers = match running {
            Ok(names) if names.is_empty() => return None,
            Ok(names) => names,
            Err(error) => {
                eprintln!("Silo quit: computer status is unavailable: {error}");
                Vec::new()
            }
        };
        let request_id = match state.pending {
            Some(id) => id,
            None => {
                state.next += 1;
                state.pending = Some(state.next);
                state.next
            }
        };
        Some(QuitRequest {
            request_id,
            computers,
        })
    }
    /// The session is ending: the open prompt no longer applies.
    pub fn close(&self) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.session_ending = true;
        state.pending = None;
    }
    /// Returns whether Silo should exit.
    pub fn answer(&self, request_id: u64, confirmed: bool) -> Result<bool, String> {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.pending != Some(request_id) {
            return Err("This Quit request is no longer current.".into());
        }
        state.pending = None;
        Ok(confirmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use silo_engine::EngineRuntimeState;

    #[test]
    fn settings_shutdown_and_startup_state_belong_to_their_engine_instance() {
        let first_directory = tempfile::tempdir().unwrap();
        let second_directory = tempfile::tempdir().unwrap();
        let first_path = first_directory.path().join("settings.json");
        let second_path = second_directory.path().join("settings.json");
        std::fs::write(
            &first_path,
            br#"{"schemaVersion":1,"settings":{"theme":"dark"}}"#,
        )
        .unwrap();
        std::fs::write(
            &second_path,
            br#"{"schemaVersion":1,"settings":{"theme":"light"}}"#,
        )
        .unwrap();

        let first = EngineRuntimeState::default();
        let second = EngineRuntimeState::default();
        let first_snapshot = first
            .settings()
            .initialize(|| Ok(Some(first_path.clone())))
            .unwrap();
        let second_snapshot = second
            .settings()
            .initialize(|| Ok(Some(second_path.clone())))
            .unwrap();

        assert_eq!(first_snapshot.theme(), Some(Theme::Dark));
        assert_eq!(second_snapshot.theme(), Some(Theme::Light));
        assert_eq!(
            first
                .settings()
                .initialize(|| Ok(Some(second_path)))
                .unwrap()
                .theme(),
            Some(Theme::Dark)
        );
        assert_eq!(first.shutdown().request_generation(), Some(1));
        assert_eq!(second.shutdown().request_generation(), Some(1));
        first.quit_confirmation().enable();
        assert!(first.quit_confirmation().enabled());
        assert!(!second.quit_confirmation().enabled());
        first.startup().cancel();
        assert!(first.startup().is_cancelled());
        assert!(!second.startup().is_cancelled());
    }

    #[test]
    fn migrated_settings_and_onboarding_draft_are_accepted() {
        let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
        let store = SettingsStore::load(Some(migrated.settings.clone()));
        assert_eq!(store.protected_error, None);
        let settings = &store.snapshot.settings;
        assert_eq!(settings["startComputersAtLaunch"], true);
        assert_eq!(
            settings["startupComputerIds"],
            json!([crate::runtime_migration::vocabulary_tests::ID])
        );
        assert_eq!(settings["computerOrder"].as_array().unwrap().len(), 2);
        let draft = &store.snapshot.onboarding_draft;
        assert_eq!(draft["currentStep"], "computers");
        assert_eq!(draft["computers"].as_array().unwrap().len(), 1);
        assert!(draft["unfinishedComputerEditor"]["draft"]
            .get("kind")
            .is_none());
    }

    #[test]
    fn native_theme_follows_saved_preferences_and_releases_system_override() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::load(Some(path.clone()));
        assert_eq!(store.snapshot().theme(), None);

        for (preference, native) in [
            ("dark", Some(Theme::Dark)),
            ("light", Some(Theme::Light)),
            ("system", None),
        ] {
            let snapshot = store
                .update(json!({"theme": preference}).as_object().unwrap().clone())
                .unwrap();
            assert_eq!(snapshot.theme(), native);

            // Reopening the app must use the saved appearance. Updating an
            // unrelated preference must not reset that appearance.
            store = SettingsStore::load(Some(path.clone()));
            assert_eq!(store.snapshot().theme(), native);
            let snapshot = store
                .update(json!({"reduceMotion": true}).as_object().unwrap().clone())
                .unwrap();
            assert_eq!(snapshot.theme(), native);
        }
    }

    #[test]
    fn native_theme_uses_legacy_import_without_overwriting_explicit_preference() {
        let mut store = SettingsStore::load(None);
        assert_eq!(
            store.import_theme("light".into()).unwrap().theme(),
            Some(Theme::Light),
        );
        store
            .update(json!({"theme": "system"}).as_object().unwrap().clone())
            .unwrap();
        assert_eq!(store.import_theme("dark".into()).unwrap().theme(), None);
    }

    #[test]
    fn system_default_modes_preserve_explicit_applications_across_restarts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let explicit = json!({
            "terminal": "Custom Terminal", "terminalPath": "/Applications/Custom Terminal.app",
            "editor": "Custom Editor", "editorPath": "/Applications/Custom Editor.app",
            "browser": "Custom Browser", "browserPath": "/Applications/Custom Browser.app"
        })
        .as_object()
        .unwrap()
        .clone();
        let mut store = SettingsStore::load(Some(path.clone()));
        store.update(explicit.clone()).unwrap();
        store = SettingsStore::load(Some(path.clone()));
        assert_eq!(store.snapshot().settings, explicit);

        for enabled in [true, false] {
            let modes = json!({
                "terminalUseSystemDefault": enabled,
                "editorUseSystemDefault": enabled,
                "browserUseSystemDefault": enabled
            })
            .as_object()
            .unwrap()
            .clone();
            assert!(store.update(modes.clone()).unwrap().save_error.is_none());
            store = SettingsStore::load(Some(path.clone()));
            let mut expected = explicit.clone();
            expected.extend(modes);
            assert_eq!(store.snapshot().settings, expected);
            assert!(store.snapshot().save_error.is_none());
        }

        let saved = fs::read(&path).unwrap();
        for key in [
            "terminalUseSystemDefault",
            "editorUseSystemDefault",
            "browserUseSystemDefault",
        ] {
            for invalid in [json!("true"), Value::Null, json!(0)] {
                let mut patch = json!({"theme": "light"}).as_object().unwrap().clone();
                patch.insert(key.into(), invalid);
                assert!(store.update(patch).is_err());
                assert_eq!(fs::read(&path).unwrap(), saved);
            }
        }
        let document: Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(document["schemaVersion"], 1);
    }

    #[test]
    fn chosen_application_label_and_location_survive_restart_together() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::load(Some(path.clone()));
        store.update(json!({ "editor": "Custom Editor", "editorPath": "/Applications/Custom Editor.app" }).as_object().unwrap().clone()).unwrap();
        let restored = SettingsStore::load(Some(path)).snapshot();
        assert_eq!(
            restored.settings.get("editor"),
            Some(&json!("Custom Editor"))
        );
        assert_eq!(
            restored.settings.get("editorPath"),
            Some(&json!("/Applications/Custom Editor.app"))
        );
        assert_eq!(
            valid_setting("editorPath", &json!("relative.app")),
            Some(false)
        );
        assert_eq!(valid_setting("editorPath", &Value::Null), Some(true));
    }
    #[test]
    fn settings_survive_restart_including_false_and_empty_selections() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let patch = json!({
            "theme": "light", "launchAtLogin": false,
            "startComputersAtLaunch": false, "startupComputerIds": [],
            "terminal": "iTerm", "editor": "Cursor", "browser": "Firefox",
            "reduceMotion": true, "notificationsEnabled": false,
            "notifyFailures": true, "notifyChanges": false, "notifyCompletions": true,
            "alphaNoticeDismissed": true,
            "editorIncludeNoticeDismissed": "Include \"/home/user/.silo/bbbbbbbbbbbb/ssh/*.conf\""
        });
        let mut store = SettingsStore::load(Some(path.clone()));
        assert!(store
            .update(patch.as_object().unwrap().clone())
            .unwrap()
            .save_error
            .is_none());
        assert_eq!(
            SettingsStore::load(Some(path)).snapshot().settings,
            patch.as_object().unwrap().clone()
        );
    }

    #[test]
    fn a_dismissed_ssh_include_notice_is_saved_replaced_and_cleared() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let key = "editorIncludeNoticeDismissed";
        let line = |home: &str| format!("Include \"/home/user/.silo/{home}/ssh/*.conf\"");
        let save = |value: Value| {
            let mut patch = Map::new();
            patch.insert(key.into(), value);
            patch
        };
        let mut store = SettingsStore::load(Some(path.clone()));
        assert!(!store.snapshot().settings.contains_key(key));
        for value in [
            json!(line("aaaaaaaaaaaa")),
            json!(line("bbbbbbbbbbbb")),
            Value::Null,
        ] {
            assert!(store
                .update(save(value.clone()))
                .unwrap()
                .save_error
                .is_none());
            assert_eq!(
                SettingsStore::load(Some(path.clone())).snapshot().settings[key],
                value
            );
        }
        let saved = std::fs::read(&path).unwrap();
        for invalid in [json!(""), json!(true), json!("x".repeat(8193))] {
            assert!(store.update(save(invalid)).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), saved);
        }
    }

    #[test]
    fn settings_saved_without_the_dismissed_line_still_load_and_keep_every_field() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "schemaVersion": 1, "futureDocumentField": 1,
                "settings": {"theme": "dark", "alphaNoticeDismissed": true, "futurePreference": 42},
                "onboardingDraft": null
            }))
            .unwrap(),
        )
        .unwrap();
        let mut store = SettingsStore::load(Some(path.clone()));
        let before = store.snapshot();
        assert!(!before.write_protected && before.save_error.is_none());
        assert_eq!(before.settings["theme"], "dark");
        assert!(!before.settings.contains_key("editorIncludeNoticeDismissed"));
        let mut patch = Map::new();
        patch.insert("editorIncludeNoticeDismissed".into(), json!("Include x"));
        store.update(patch).unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["schemaVersion"], 1);
        assert_eq!(saved["futureDocumentField"], 1);
        assert_eq!(saved["settings"]["alphaNoticeDismissed"], true);
        assert_eq!(saved["settings"]["futurePreference"], 42);
        assert_eq!(
            saved["settings"]["editorIncludeNoticeDismissed"],
            "Include x"
        );
    }

    #[test]
    fn independent_changes_preserve_unknown_saved_fields() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "schemaVersion": 1, "futureDocumentField": {"keep": true},
                "settings": {"theme": "dark", "launchAtLogin": false, "futurePreference": 42},
                "onboardingDraft": null
            }))
            .unwrap(),
        )
        .unwrap();
        let mut store = SettingsStore::load(Some(path.clone()));
        store
            .update(json!({"editor": "Cursor"}).as_object().unwrap().clone())
            .unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["settings"]["theme"], "dark");
        assert_eq!(saved["settings"]["launchAtLogin"], false);
        assert_eq!(saved["settings"]["futurePreference"], 42);
        assert_eq!(saved["futureDocumentField"]["keep"], true);
    }

    #[test]
    fn quit_asks_only_when_computers_run_and_a_ui_can_answer() {
        let quit = QuitConfirmation::default();
        assert_eq!(quit.ask(Ok(vec!["dev".into()])), None, "no UI has opted in");
        quit.0.lock().unwrap().enabled = true;
        assert_eq!(quit.ask(Ok(vec![])), None, "nothing is running");
        let request = quit.ask(Ok(vec!["dev".into(), "api".into()])).unwrap();
        assert_eq!(request.computers, vec!["dev", "api"]);
        let again = quit.ask(Ok(vec!["dev".into()])).unwrap();
        assert_eq!(
            again.request_id, request.request_id,
            "a repeated Quit reuses the open prompt"
        );
        assert_eq!(
            quit.answer(request.request_id + 1, true),
            Err("This Quit request is no longer current.".into())
        );
        assert_eq!(quit.answer(request.request_id, false), Ok(false));
        assert!(
            quit.answer(request.request_id, true).is_err(),
            "an answered request is closed"
        );
        let unknown = quit.ask(Err("inspect failed".into())).unwrap();
        assert!(unknown.computers.is_empty());
        assert_ne!(unknown.request_id, request.request_id);
        assert_eq!(quit.answer(unknown.request_id, true), Ok(true));
    }

    #[test]
    fn write_protected_settings_never_block_quit_or_updates() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let bytes = br#"{"schemaVersion":2,"settings":{"theme":"dark"}}"#;
        std::fs::write(&path, bytes).unwrap();
        let mut store = SettingsStore::load(Some(path.clone()));
        assert!(store.snapshot().write_protected);
        assert_eq!(store.save(), Ok(()));
        assert_eq!(store.flush(), Ok(()));
        store
            .update(json!({"editor":"Cursor"}).as_object().unwrap().clone())
            .unwrap();
        assert!(store.save().is_err());
        assert_eq!(store.flush(), Ok(()));
        assert_eq!(store.snapshot().settings["editor"], "Cursor");
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn storage_failures_still_fail_a_flush() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let mut store = SettingsStore::load(Some(directory.path().join("settings.json")));
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
        store
            .update(json!({"editor":"Cursor"}).as_object().unwrap().clone())
            .unwrap();
        let flushed = store.flush();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!store.snapshot().write_protected);
        assert!(flushed.is_err());
    }

    #[test]
    fn corrupt_and_newer_documents_are_never_overwritten() {
        for bytes in [
            b"{broken".as_slice(),
            br#"{"schemaVersion":2,"settings":{"theme":"dark"}}"#,
            br#"{"schemaVersion":1,"settings":{"theme":"dark","reduceMotion":"yes"}}"#,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("settings.json");
            std::fs::write(&path, bytes).unwrap();
            let mut store = SettingsStore::load(Some(path.clone()));
            assert!(store.snapshot().save_error.is_some());
            let snapshot = store
                .update(json!({"editor":"Cursor"}).as_object().unwrap().clone())
                .unwrap();
            assert_eq!(snapshot.settings["editor"], "Cursor");
            assert!(snapshot.save_error.is_some());
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn legacy_theme_fills_only_a_missing_value_and_does_not_seed_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::load(Some(path.clone()));
        assert!(store.snapshot().settings.is_empty());
        assert!(!path.exists());
        assert!(store.import_theme("sepia".into()).is_err());
        assert!(!path.exists());
        store.import_theme("light".into()).unwrap();
        store.import_theme("dark".into()).unwrap();
        assert_eq!(
            SettingsStore::load(Some(path)).snapshot().settings,
            json!({"theme":"light"}).as_object().unwrap().clone()
        );
    }

    #[test]
    fn rejected_patch_does_not_partially_change_other_settings() {
        let mut store = SettingsStore::load(None);
        for patch in [
            json!({"theme":"light", "reduceMotion":"yes"}),
            json!({"theme":"light", "credentials":"secret"}),
            json!({"terminal":""}),
            json!({"startupComputerIds":[""]}),
        ] {
            assert!(store.update(patch.as_object().unwrap().clone()).is_err());
            assert_eq!(store.snapshot().revision, 0);
            assert!(store.snapshot().settings.is_empty());
        }
    }

    #[test]
    fn settings_event_publishes_revision_and_settings_without_onboarding_draft() {
        let mut store = SettingsStore::load(None);
        store.update_draft(unfinished_draft()).unwrap();
        let snapshot = store.snapshot();

        let mut published = None;
        publish_snapshot(&snapshot, |event, audience| {
            published = Some((event, audience))
        });

        let (event, audience) = published.unwrap();
        assert_eq!(event.name, "settings:changed");
        assert_eq!(audience, silo_engine::Audience::All);
        assert!(event.payload["onboardingDraft"].is_null());
        assert_eq!(event.payload["revision"], snapshot.revision);
        assert_eq!(event.payload["settings"], json!(snapshot.settings));
        assert_eq!(snapshot.onboarding_draft, unfinished_draft());
    }

    fn unfinished_draft() -> Value {
        json!({
            "currentStep":"computers", "computers": [{
                "id":"95168b7e-aa9f-4dc1-a5de-2865c1b0bb64", "name":"dev",
                "cpus":2,"maxCPUs":4,"memoryGiB":4,"maxMemoryGiB":8,
                "workspaceStorageGiB":10,"runtimeStorageGiB":10
            }], "unfinishedComputerEditor": {
                "draft": {"id":"025da8eb-56bf-4519-85cb-3316b2feb549", "name":"",
                    "cpus":0,"maxCPUs":0,"memoryGiB":0,"maxMemoryGiB":0,
                    "workspaceStorageGiB":0,"runtimeStorageGiB":0}, "insertAt":1
            }, "computerSelections":{"dev":[{"repository":"owner/repo", "allowPushes":false}]},
            "computerIdentities":{"dev":{"name":"", "email":"unfinished@", "apply":false}}
        })
    }

    #[test]
    fn authentication_method_survives_draft_restart_and_rejects_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::load(Some(path.clone()));
        let mut draft = unfinished_draft();
        for method in ["token", "oauth"] {
            draft["computerRepositoryAccess"] = json!({"dev":{
                "repositoryMode":"selected","allRepositoriesAllowChanges":false,
                "authenticationMethod":method
            }});
            store.update_draft(draft.clone()).unwrap();
            assert_eq!(
                SettingsStore::load(Some(path.clone()))
                    .snapshot()
                    .onboarding_draft,
                draft
            );
        }
        for invalid in [json!("unknown"), json!(null), json!(true), json!(1)] {
            draft["computerRepositoryAccess"]["dev"]["authenticationMethod"] = invalid;
            assert!(!valid_draft(&draft));
        }
        draft["computerRepositoryAccess"]["dev"]["authenticationMethod"] = json!("token");
        draft["computerRepositoryAccess"]["dev"]["token"] = json!("secret");
        assert!(!valid_draft(&draft));
    }

    #[test]
    fn all_repository_intent_survives_restart_and_rejects_malformed_access() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::load(Some(path.clone()));
        let mut draft = unfinished_draft();
        draft["computerRepositoryAccess"] =
            json!({"dev":{"repositoryMode":"all","allRepositoriesAllowChanges":false}});
        store.update_draft(draft.clone()).unwrap();
        assert_eq!(
            SettingsStore::load(Some(path)).snapshot().onboarding_draft,
            draft.clone()
        );
        for invalid in [
            json!(null),
            json!({"dev":{"repositoryMode":"unknown","allRepositoriesAllowChanges":false}}),
            json!({"dev":{"repositoryMode":"all","allRepositoriesAllowChanges":"yes"}}),
            json!({"dev":{"repositoryMode":"all","allRepositoriesAllowChanges":false,"token":"secret"}}),
        ] {
            draft["computerRepositoryAccess"] = invalid;
            assert!(!valid_draft(&draft));
        }
    }

    #[test]
    fn unfinished_onboarding_survives_restart_and_clearing_preserves_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::load(Some(path.clone()));
        store.import_theme("dark".into()).unwrap();
        store.update_draft(unfinished_draft()).unwrap();
        let mut restarted = SettingsStore::load(Some(path.clone()));
        assert_eq!(restarted.snapshot().onboarding_draft, unfinished_draft());
        assert!(restarted.snapshot().save_error.is_none());
        restarted.update_draft(Value::Null).unwrap();
        let snapshot = SettingsStore::load(Some(path)).snapshot();
        assert!(snapshot.onboarding_draft.is_null());
        assert_eq!(snapshot.settings["theme"], "dark");
    }

    #[test]
    fn deleting_the_last_onboarding_computer_survives_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::load(Some(path.clone()));
        let mut draft = unfinished_draft();
        store.update_draft(draft.clone()).unwrap();
        draft["computers"] = json!([]);
        for editor in [Value::Null, draft["unfinishedComputerEditor"].clone()] {
            draft["unfinishedComputerEditor"] = editor;
            store.update_draft(draft.clone()).unwrap();
            let snapshot = SettingsStore::load(Some(path.clone())).snapshot();
            assert_eq!(snapshot.onboarding_draft, draft);
            assert!(snapshot.save_error.is_none());
        }
    }

    #[test]
    fn drafts_reject_credentials_and_runtime_results() {
        let mut store = SettingsStore::load(None);
        for field in ["credentials", "connection", "progress", "completed"] {
            let mut draft = unfinished_draft();
            draft[field] = json!("do not persist");
            assert!(store.update_draft(draft).is_err());
        }
        let mut draft = unfinished_draft();
        draft["unfinishedComputerEditor"]["draft"]["password"] = json!("secret");
        assert!(store.update_draft(draft).is_err());
        assert!(store.snapshot().onboarding_draft.is_null());
    }

    #[test]
    fn independent_concurrent_patches_do_not_lose_other_fields() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let store = std::sync::Arc::new(Mutex::new(SettingsStore::load(Some(path.clone()))));
        let threads: Vec<_> = [
            json!({"theme":"dark"}),
            json!({"editor":"Cursor"}),
            json!({"startupComputerIds":[]}),
        ]
        .into_iter()
        .map(|patch| {
            let store = store.clone();
            std::thread::spawn(move || {
                store
                    .lock()
                    .unwrap()
                    .update(patch.as_object().unwrap().clone())
                    .unwrap()
            })
        })
        .collect();
        let mut revisions: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap().revision)
            .collect();
        revisions.sort();
        assert_eq!(revisions, [1, 2, 3]);
        assert_eq!(
            SettingsStore::load(Some(path)).snapshot().settings,
            json!({"theme":"dark","editor":"Cursor","startupComputerIds":[]})
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn failed_write_keeps_old_file_and_retries_session_choices() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::load(Some(path.clone()));
        store.import_theme("dark".into()).unwrap();
        let saved = fs::read(&path).unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let result = store
            .update(json!({"editor":"Cursor"}).as_object().unwrap().clone())
            .unwrap();
        // Restore permissions before asserting, so a failed assertion cannot strand the fixture.
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.save_error.is_some());
        assert_eq!(result.settings["editor"], "Cursor");
        assert_eq!(fs::read(&path).unwrap(), saved);
        let snapshot = store
            .update(json!({"browser":"Firefox"}).as_object().unwrap().clone())
            .unwrap();
        assert!(snapshot.save_error.is_none());
        assert_eq!(
            SettingsStore::load(Some(path)).snapshot().settings["editor"],
            "Cursor"
        );
    }

    #[test]
    fn incomplete_temporary_file_does_not_replace_the_last_commit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::load(Some(path.clone()));
        store.import_theme("light".into()).unwrap();
        let mut interrupted = tempfile::NamedTempFile::new_in(directory.path()).unwrap();
        interrupted
            .write_all(br#"{"schemaVersion":1,"settings":{"theme":"da"#)
            .unwrap();
        interrupted.as_file().sync_all().unwrap();
        assert_eq!(
            SettingsStore::load(Some(path)).snapshot().settings["theme"],
            "light"
        );
    }

    #[test]
    fn memory_fixture_has_no_saved_state_to_reload() {
        let mut fixture = SettingsStore::load(None);
        fixture.import_theme("light".into()).unwrap();
        fixture.update_draft(unfinished_draft()).unwrap();
        assert_eq!(fixture.snapshot().settings["theme"], "light");
        assert!(SettingsStore::load(None).snapshot().settings.is_empty());
        assert!(SettingsStore::load(None)
            .snapshot()
            .onboarding_draft
            .is_null());
    }

    #[test]
    fn invalid_saved_computer_semantics_protect_the_entire_original_file() {
        let mut candidates = Vec::new();
        let mut malformed = unfinished_draft();
        malformed["computers"] = json!({});
        candidates.push(malformed);
        for (field, invalid) in [
            ("id", json!("not-a-uuid")),
            ("name", json!("Invalid name")),
            ("host", json!("")),
            ("user", json!("root user")),
            ("port", json!(0)),
        ] {
            let mut draft = unfinished_draft();
            draft["computers"][0][field] = invalid;
            candidates.push(draft);
        }
        let mut duplicate = unfinished_draft();
        let configuration = duplicate["computers"][0].clone();
        duplicate["computers"]
            .as_array_mut()
            .unwrap()
            .push(configuration);
        candidates.push(duplicate);
        for draft in candidates {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("settings.json");
            let original = serde_json::to_vec(
                &json!({"schemaVersion":1,"settings":{"theme":"dark"},"onboardingDraft":draft}),
            )
            .unwrap();
            fs::write(&path, &original).unwrap();
            let mut store = SettingsStore::load(Some(path.clone()));
            assert!(store.snapshot().save_error.is_some());
            assert_eq!(store.snapshot().settings["theme"], "dark");
            store.update_draft(unfinished_draft()).unwrap();
            store
                .update(json!({"editor":"Cursor"}).as_object().unwrap().clone())
                .unwrap();
            assert_eq!(fs::read(path).unwrap(), original);
        }
    }

    #[test]
    fn unfinished_computer_resources_allow_custom_input_but_saved_limits_are_checked() {
        let mut draft = unfinished_draft();
        let computer = json!({
            "id":"025da8eb-56bf-4519-85cb-3316b2feb549", "name":"unfinished name",
            "cpus":12,"maxCPUs":4,"memoryGiB":48,"maxMemoryGiB":16,
            "workspaceStorageGiB":60,"runtimeStorageGiB":80
        });
        draft["unfinishedComputerEditor"]["draft"] = computer.clone();
        assert!(valid_draft(&draft));
        draft["unfinishedComputerEditor"]["draft"]["cpus"] = json!("invalid");
        assert!(!valid_draft(&draft));
        let mut saved = computer;
        saved["name"] = json!("dev");
        assert!(!valid_computer(&saved, false));
        saved["maxCPUs"] = json!(12);
        saved["maxMemoryGiB"] = json!(48);
        assert!(valid_computer(&saved, false));
    }

    #[test]
    fn malformed_saved_desktop_policy_protects_the_original_draft() {
        let mut draft = unfinished_draft();
        draft["computers"][0] = json!({
            "id":"95168b7e-aa9f-4dc1-a5de-2865c1b0bb64", "name":"dev",
            "cpus":2,"maxCPUs":4,"memoryGiB":4,"maxMemoryGiB":8,
            "workspaceStorageGiB":60,"runtimeStorageGiB":80
        });
        for desktop in [json!({}), json!({"builtIn":true})] {
            draft["computers"][0]["desktop"] = desktop;
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("settings.json");
            let original = serde_json::to_vec(&json!({
                "schemaVersion":1,"settings":{"theme":"dark"},"onboardingDraft":draft
            }))
            .unwrap();
            fs::write(&path, &original).unwrap();
            let mut store = SettingsStore::load(Some(path.clone()));
            assert!(store.snapshot().write_protected);
            assert!(store.snapshot().save_error.is_some());
            store.update_draft(unfinished_draft()).unwrap();
            assert_eq!(fs::read(path).unwrap(), original);
        }
        for desktop in [
            json!({"startWithComputer":false}),
            json!({"startWithComputer":true,"builtIn":true}),
        ] {
            draft["computers"][0]["desktop"] = desktop;
            assert!(valid_draft(&draft));
        }
    }

    #[test]
    fn custom_resources_survive_settings_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::load(Some(path.clone()));
        let mut draft = unfinished_draft();
        draft["computers"][0] = json!({
            "id":"95168b7e-aa9f-4dc1-a5de-2865c1b0bb64", "name":"dev",
            "cpus":3,"maxCPUs":5,"memoryGiB":10,"maxMemoryGiB":12,
            "workspaceStorageGiB":35,"runtimeStorageGiB":25,"desktop":{"startWithComputer":false}
        });
        store.update_draft(draft.clone()).unwrap();
        assert_eq!(
            SettingsStore::load(Some(path)).snapshot().onboarding_draft,
            draft
        );
        draft["unfinishedComputerEditor"]["draft"] = draft["computers"][0].clone();
        draft["unfinishedComputerEditor"]["draft"]["memoryGiB"] = json!(0);
        assert!(valid_draft(&draft));
        for invalid in [json!(0), json!(-1), json!(1.5), json!(4294967296_u64)] {
            draft["computers"][0]["memoryGiB"] = invalid;
            assert!(!valid_draft(&draft));
        }
    }

    #[test]
    fn startup_id_limit_matches_the_typescript_boundary() {
        let ids = vec![json!("temporarily-unavailable-id"); 256];
        assert_eq!(valid_setting("startupComputerIds", &json!(ids)), Some(true));
        assert_eq!(
            valid_setting("startupComputerIds", &json!(vec!["id"; 257])),
            Some(false)
        );
    }

    #[test]
    fn computer_order_limit_matches_the_typescript_boundary() {
        let key = "remote:".to_owned() + &"a".repeat(505);
        assert_eq!(
            valid_setting("computerOrder", &json!(vec![key.as_str(); 1024])),
            Some(true)
        );
        assert_eq!(
            valid_setting("computerOrder", &json!(vec!["local:id"; 1025])),
            Some(false)
        );
        assert_eq!(
            valid_setting("computerOrder", &json!([key.clone() + "a"])),
            Some(false)
        );
        assert_eq!(valid_setting("computerOrder", &json!([""])), Some(false));
    }

    #[test]
    fn initialization_resolves_storage_once_and_reuses_loaded_state() {
        let state = SettingsState::default();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let snapshot = state.initialize(|| Ok(Some(path))).unwrap();
        assert!(snapshot.settings.is_empty());
        state
            .initialized()
            .unwrap()
            .as_mut()
            .unwrap()
            .store
            .import_theme("light".into())
            .unwrap();
        assert_eq!(
            state
                .initialize(|| panic!("reinitialization must not reread storage"))
                .unwrap()
                .settings["theme"],
            "light"
        );
    }

    #[test]
    fn status_reads_wait_for_main_initialization_without_holding_its_lock() {
        let state = std::sync::Arc::new(SettingsState::default());
        let reader_state = state.clone();
        let (started, ready) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            started.send(()).unwrap();
            reader_state
                .initialized()
                .unwrap()
                .as_ref()
                .unwrap()
                .store
                .snapshot()
        });
        ready.recv().unwrap();
        state.initialize(|| Ok(None)).unwrap();
        assert!(reader.join().unwrap().settings.is_empty());
    }

    #[test]
    fn timeout_from_cancelled_quit_cannot_finish_a_new_quit_attempt() {
        let state = ShutdownState::default();
        assert!(state.request());
        let old = state.generation();
        assert!(state.begin_flush());
        state.cancel();
        assert!(state.request());
        assert!(!state.claim_exit_for(false, Some(old)));
        assert!(state.begin_flush());
        assert!(state.claim_exit(true));
    }

    #[test]
    fn failed_shutdown_keeps_app_open_and_allows_another_quit_attempt() {
        let state = ShutdownState::default();
        assert!(state.request());
        assert!(state.claim_exit(false));
        assert!(state.active());
        state.cancel();
        assert!(!state.active());
        assert!(!state.approved());
        assert!(state.request());
        assert!(state.begin_flush());
        assert!(state.claim_exit(true));
        state.allow_exit();
        assert!(state.approved());
    }

    #[test]
    fn quit_timeout_cannot_cut_off_an_acknowledged_flush() {
        let state = ShutdownState::default();
        assert!(state.request());
        assert!(state.begin_flush());
        assert!(!state.claim_exit(false));
        assert!(!state.request());
        assert!(!state.approved());
        assert!(state.claim_exit(true));
        assert!(!state.approved());
        state.allow_exit();
        assert!(state.approved());
        assert!(!state.claim_exit(true));
    }

    #[test]
    fn session_end_fallback_cuts_off_a_slow_frontend_flush() {
        let state = ShutdownState::default();
        assert!(state.request());
        let generation = state.generation();
        assert!(state.begin_flush());
        assert!(
            !state.claim_exit_for(false, Some(generation)),
            "a user Quit waits for the frontend"
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        state.begin_session_end(deadline);
        state.begin_session_end(deadline + Duration::from_secs(5));
        assert_eq!(
            state.session_deadline(),
            Some(deadline),
            "the earliest deadline wins"
        );
        assert!(state.claim_exit_for(false, Some(generation)));
        assert!(
            !state.claim_exit(true),
            "a late frontend completion cannot finish twice"
        );
        assert!(!state.claim_exit_for(false, None));
    }

    #[test]
    fn session_end_can_start_before_any_quit_and_cannot_be_cancelled() {
        let state = ShutdownState::default();
        state.begin_session_end(Instant::now());
        assert!(
            state.request(),
            "a session end starts the ordinary exit phases"
        );
        assert!(state.session_deadline().is_some());
        assert!(!state.cancel());
        assert!(state.session_deadline().is_some());
        assert!(state.active());
    }

    #[test]
    fn session_deadline_ends_a_quit_whose_stop_worker_is_blocked() {
        let state = std::sync::Arc::new(ShutdownState::default());
        assert!(state.request());
        assert!(state.begin_flush());
        let (stopping, started) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || {
            assert!(worker_state.claim_exit(true));
            stopping.send(()).unwrap();
            blocked.recv().unwrap();
        });
        started.recv().unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        state.begin_session_end(deadline);
        assert!(!state.expire_session(deadline - Duration::from_millis(1)));
        let expired = state.expire_session(deadline);
        release.send(()).unwrap();
        worker.join().unwrap();
        assert!(
            expired,
            "session termination must not wait for the stop worker"
        );
        assert!(state.approved());
        assert!(
            !state.expire_session(deadline),
            "exit is approved only once"
        );
    }

    #[test]
    fn session_deadline_never_expires_an_ordinary_quit() {
        let state = ShutdownState::default();
        assert!(state.request());
        assert!(state.begin_flush());
        assert!(state.claim_exit(true));
        assert!(!state.expire_session(Instant::now()));
        assert!(!state.approved());
    }

    #[test]
    fn session_deadline_cannot_be_cancelled_by_a_failed_quit() {
        let state = ShutdownState::default();
        assert!(state.request());
        assert!(state.claim_exit(false));
        let deadline = Instant::now();
        state.begin_session_end(deadline);
        let mut reopened = false;
        assert!(!state.cancel_with(|| reopened = true));
        assert!(
            !reopened,
            "session termination must not reopen computer admission"
        );
        assert_eq!(state.session_deadline(), Some(deadline));
        assert!(state.expire_session(deadline));
    }

    #[test]
    fn session_end_closes_an_open_quit_prompt() {
        let quit = QuitConfirmation::default();
        quit.0.lock().unwrap().enabled = true;
        let request = quit.ask(Ok(vec!["dev".into()])).unwrap();
        quit.close();
        assert!(
            quit.answer(request.request_id, false).is_err(),
            "a late Cancel cannot keep Silo open"
        );
        assert_eq!(quit.ask(Ok(vec!["dev".into()])), None);
    }

    #[test]
    fn a_late_quit_status_read_cannot_reopen_the_session_end_prompt() {
        let quit = QuitConfirmation::default();
        quit.0.lock().unwrap().enabled = true;
        quit.close();
        assert_eq!(quit.ask(Ok(vec!["dev".into()])), None);
        assert_eq!(quit.ask(Err("status unavailable".into())), None);
    }

    #[test]
    fn session_fallback_starts_native_shutdown_before_a_short_deadline() {
        let now = Instant::now();
        for budget in [Duration::from_secs(1), Duration::from_millis(4250)] {
            let deadline = now + budget;
            let wait = session_flush_wait(deadline, now);
            assert!(
                now + wait < deadline,
                "native shutdown needs time before expiry"
            );
        }
        assert_eq!(
            session_flush_wait(now + Duration::from_secs(20), now),
            FRONTEND_FLUSH_FALLBACK
        );
    }

    #[test]
    fn an_elapsed_session_deadline_does_not_wait_for_the_frontend() {
        let now = Instant::now();
        assert_eq!(session_flush_wait(now, now), Duration::ZERO);
        assert_eq!(
            session_flush_wait(now, now + Duration::from_secs(1)),
            Duration::ZERO
        );
    }

    #[test]
    fn update_restart_does_not_start_the_quit_flow() {
        assert_eq!(exit_request(true, false), ExitRequest::Restart);
        assert_eq!(exit_request(true, true), ExitRequest::Restart);
        assert_eq!(exit_request(false, true), ExitRequest::Approved);
        assert_eq!(exit_request(false, false), ExitRequest::Gated);
        let state = ShutdownState::default();
        state.mark_restart();
        assert!(state.restarting());
        assert!(!state.active(), "a restart never enters the Quit phases");
    }

    #[test]
    fn quit_timeout_can_finish_when_the_frontend_never_acknowledges() {
        let state = ShutdownState::default();
        assert!(state.request());
        assert!(state.claim_exit(false));
        assert!(!state.begin_flush());
        assert!(!state.claim_exit(true));
        state.allow_exit();
        assert!(state.approved());
    }
}
