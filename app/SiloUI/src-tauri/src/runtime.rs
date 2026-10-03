pub(crate) mod checkpoints;
pub(crate) mod configuration_recovery;
#[cfg(test)]
pub(crate) mod contract_tests;
mod crash_acknowledgement;
#[path = "guest_image.rs"]
pub(crate) mod guest_image;
pub(crate) mod image_cache;
pub(crate) mod lifecycle_recovery;
pub(crate) mod operation_gate;
pub(crate) mod remote_ops;
#[path = "runtime_activity.rs"]
mod runtime_activity;
#[path = "runtime_logs.rs"]
pub(crate) mod runtime_logs;
#[path = "secrets_runtime.rs"]
mod secrets_runtime;
pub(crate) mod shutdown;
pub(crate) mod storage;
pub(crate) mod update_recovery;
use crate::bridge_error::{BridgeError, ErrorCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::collections::VecDeque;
use std::{
    collections::{HashMap, HashSet},
    ffi::CString,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};

const READ_TIMEOUT: Duration = Duration::from_secs(10);
const MUTATION_TIMEOUT: Duration = Duration::from_secs(180);
const STOP_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_OUTPUT_BYTES: u64 = 1024 * 1024;
pub(crate) const WORKSPACE_MOUNT: &str = "/workspace";
const MAX_COMPUTER_COUNT: usize = 64;
const MANAGED_LABEL: &str = "silo.managed=true";

/// Ordered admission for computer-changing operations on this device. See `operation_gate`.
pub(crate) static OPERATIONS: operation_gate::OperationGate = operation_gate::OperationGate::new();

#[tauri::command]
pub fn read_operation_queue() -> operation_gate::OperationQueue {
    OPERATIONS.snapshot()
}

/// Ask to cancel a queued or running operation by its queue id. A waiting operation
/// leaves the queue; a running operation is stopped only when it opted in as cancellable.
#[tauri::command]
pub fn cancel_operation(id: u64) -> Result<(), BridgeError> {
    OPERATIONS.cancel(id).map_err(BridgeError::from)
}
const DISABLED_GITHUB_PROFILE: &str = r#"{"version":1,"owners":[]}"#;
static GITHUB_PROFILES: OnceLock<Mutex<HashMap<(PathBuf, String), String>>> = OnceLock::new();
const GITHUB_UPDATE_REPLACED: &str = "A newer GitHub access choice has replaced this update.";

/// Per-computer coordination of the GitHub profile and secret material a computer boots with.
#[derive(Default)]
struct ComputerAccessState {
    /// Highest accepted GitHub access revision. Held only to compare or record it
    /// (together with the cached profile), never while a runtime command runs.
    revision: Mutex<u64>,
    /// Excludes a boot (start, restart or exec's temporary boot) from a secret or
    /// GitHub update of the same computer, so a computer never boots with material an update is
    /// replacing. Held across one runtime step and always acquired with a bound
    /// (`lock_computer_runtime`); the operation gate already orders these steps, so it is
    /// normally uncontended.
    runtime: Mutex<()>,
}
type ComputerAccessStates = HashMap<(PathBuf, String), Arc<ComputerAccessState>>;
static COMPUTER_ACCESS_STATES: OnceLock<Mutex<ComputerAccessStates>> = OnceLock::new();

fn computer_access_state(home: &Path, computer: &str) -> Result<Arc<ComputerAccessState>, String> {
    Ok(COMPUTER_ACCESS_STATES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "GitHub runtime state is unavailable.")?
        .entry((home.to_owned(), computer.into()))
        .or_default()
        .clone())
}

/// Wait at most `timeout` for exclusive runtime access to one computer's secret and GitHub
/// material. A cancel of the current operation ends the wait.
fn lock_computer_runtime<'a>(
    state: &'a ComputerAccessState,
    timeout: Duration,
    operation: &str,
) -> Result<std::sync::MutexGuard<'a, ()>, RuntimeError> {
    let deadline = Instant::now() + timeout;
    loop {
        match state.runtime.try_lock() {
            Ok(guard) => return Ok(guard),
            // The lock guards no data: a panic of a previous holder leaves nothing to repair.
            Err(std::sync::TryLockError::Poisoned(poisoned)) => return Ok(poisoned.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => {}
        }
        if runtime_cancel_requested() {
            return Err(RuntimeError::Cancelled {
                operation: operation.into(),
            });
        }
        if Instant::now() >= deadline {
            return Err(RuntimeError::Busy);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Forget cached GitHub state for a removed computer so a new computer reusing its
/// name never starts with the old one's access profile.
fn forget_github_state(home: &Path, computer: &str) {
    let key = (home.to_owned(), computer.to_owned());
    if let Ok(mut profiles) = GITHUB_PROFILES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        profiles.remove(&key);
    }
    if let Ok(mut states) = COMPUTER_ACCESS_STATES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        states.remove(&key);
    }
}

fn accept_github_revision(current: &mut u64, revision: u64) -> Result<(), String> {
    if revision < *current {
        return Err(GITHUB_UPDATE_REPLACED.into());
    }
    *current = revision;
    Ok(())
}

fn github_command_computer(args: &[String]) -> Option<&str> {
    match args.first().map(String::as_str) {
        Some("start" | "modify" | "restart") => args
            .get(1)
            .map(String::as_str)
            .filter(|name| validate_name(name).is_ok()),
        Some("restore") => args
            .windows(2)
            .find(|pair| pair[0] == "--name")
            .map(|pair| pair[1].as_str())
            .filter(|name| validate_name(name).is_ok()),
        _ => None,
    }
}

pub(crate) fn github_environment(paths: &RuntimePaths, args: &[String]) -> String {
    // Restore uses the profile keyed to the new target computer. Historical
    // checkpoint state never supplies host-side GitHub authority.
    let Some(computer) = github_command_computer(args) else {
        return DISABLED_GITHUB_PROFILE.into();
    };
    let cache = GITHUB_PROFILES.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(profiles) = cache.lock() else {
        return DISABLED_GITHUB_PROFILE.into();
    };
    profiles
        .get(&(paths.home.clone(), computer.into()))
        .cloned()
        .unwrap_or_else(|| DISABLED_GITHUB_PROFILE.into())
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimePaths {
    pub(crate) guest_image: PathBuf,
    pub(crate) executable: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) storage_home: Option<PathBuf>,
    pub(crate) library: PathBuf,
    pub(crate) metadata: PathBuf,
    pub(crate) volumes: PathBuf,
}

#[derive(Debug)]
pub(crate) struct CommandOutput {
    pub(crate) stdout: String,
    #[allow(dead_code)]
    pub(crate) stderr: String,
}

#[derive(Debug)]
pub(crate) enum RuntimeError {
    Busy,
    Admission(operation_gate::GateError),
    Invalid(String),
    Unavailable(String),
    /// The bundled runtime process could not be started at this moment. Retrying may
    /// succeed, unlike a missing or invalid installation (`Unavailable`).
    Launch(String),
    TimedOut {
        operation: String,
    },
    Cancelled {
        operation: String,
    },
    /// The runtime ran and reported a failure. `exit_code` is its exit status when it
    /// exited; `detail` is its own explanation with Silo's storage path hidden. The
    /// detail is diagnostic only: `Display` and `failure_report` summaries never show
    /// it, so raw runtime output is never the user-facing error.
    Failed {
        operation: String,
        exit_code: Option<i32>,
        detail: String,
    },
    Malformed(String),
    /// A configuration batch failed with this error after some of its changes were
    /// applied; the completed changes were kept.
    Partial(Box<RuntimeError>),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Admission(error) => write!(formatter, "{error}"),
            Self::Busy => formatter.write_str("Another computer operation is still running."),
            Self::Invalid(message)
            | Self::Unavailable(message)
            | Self::Malformed(message) => formatter.write_str(message),
            Self::Launch(_) => formatter.write_str("Silo could not start the computer runtime. Retry; if it keeps happening, quit and reopen Silo."),
            Self::TimedOut { operation } => {
                write!(
                    formatter,
                    "{operation} timed out. Check the computer state, then retry."
                )
            }
            Self::Cancelled { operation } => {
                write!(formatter, "{operation} was cancelled.")
            }
            Self::Failed { operation, detail, .. } => {
                write!(formatter, "{operation}: {}", failure_reason(failure_category(detail)))
            }
            Self::Partial(error) => write!(formatter, "{error} {PARTIAL_CHANGES_KEPT}"),
        }
    }
}

const PARTIAL_CHANGES_KEPT: &str =
    "Completed changes were kept; reload the computer list before retrying.";

impl From<operation_gate::GateError> for RuntimeError {
    fn from(error: operation_gate::GateError) -> Self {
        match error {
            operation_gate::GateError::Busy => RuntimeError::Busy,
            operation_gate::GateError::Cancelled => RuntimeError::Cancelled {
                operation: "The operation".into(),
            },
            other => RuntimeError::Admission(other),
        }
    }
}

pub(crate) trait RuntimeRunner {
    fn prepare_guest_image(&self, paths: &RuntimePaths) -> Result<String, RuntimeError> {
        guest_image::prepare(self, paths)
    }

    fn run(
        &self,
        paths: &RuntimePaths,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError>;
}

pub(crate) struct ProcessRunner;

impl RuntimeRunner for ProcessRunner {
    fn run(
        &self,
        paths: &RuntimePaths,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        run_msb(paths, args, timeout)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComputerConfigurationRequest {
    pub(crate) schema_version: u8,
    pub(crate) computers: Vec<ComputerConfiguration>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerConfiguration {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) cpus: u8,
    #[serde(rename = "maxCPUs")]
    pub(crate) max_cpus: u8,
    #[serde(rename = "memoryGiB")]
    pub(crate) memory_gib: u32,
    #[serde(rename = "maxMemoryGiB")]
    pub(crate) max_memory_gib: u32,
    #[serde(rename = "workspaceStorageGiB")]
    pub(crate) workspace_storage_gib: u32,
    #[serde(rename = "runtimeStorageGiB")]
    pub(crate) runtime_storage_gib: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) desktop: Option<crate::desktop::DesktopConfiguration>,
}

impl ComputerConfiguration {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

/// Why a targeted configuration change could not be applied to the current inventory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChangeRejection {
    /// The targeted computer's current configuration differs from the one the caller expected.
    Stale,
    /// The computer the caller wanted to remove is no longer present.
    Missing,
    /// A replacement's identity does not match the targeted computer.
    WrongTarget,
}

/// Apply one targeted change to `computers` using optimistic concurrency: the computer
/// identified by `id` must currently equal `expected` (both absent for a create), then
/// it is replaced in place by `replacement`, or removed when `replacement` is `None`.
/// Every other computer and the overall order are preserved. Shared by the local and
/// remote change paths so both apply against fresh state instead of a stale snapshot.
fn without_built_in(configuration: &ComputerConfiguration) -> ComputerConfiguration {
    let mut configuration = configuration.clone();
    if let Some(desktop) = &mut configuration.desktop {
        desktop.built_in = false;
    }
    configuration
}

pub(crate) fn change_computer(
    computers: &mut Vec<ComputerConfiguration>,
    id: &str,
    expected: Option<&ComputerConfiguration>,
    replacement: Option<&ComputerConfiguration>,
) -> Result<(), ChangeRejection> {
    let position = computers.iter().position(|m| m.id() == id);
    let current = position.map(|index| &computers[index]);
    // `desktop.builtIn` is the owner's decision: a controller from before it existed
    // never sees it, so it takes no part in the comparison.
    if current.map(without_built_in).as_ref() != expected.map(without_built_in).as_ref() {
        return Err(ChangeRejection::Stale);
    }
    if replacement.is_some_and(|m| m.id() != id) {
        return Err(ChangeRejection::WrongTarget);
    }
    if replacement.is_none() && current.is_none() {
        return Err(ChangeRejection::Missing);
    }
    match (position, replacement) {
        (Some(index), Some(configuration)) => {
            // ...and the owner's value survives the replacement.
            let mut configuration = configuration.clone();
            if let (
                ComputerConfiguration {
                    desktop: Some(next),
                    ..
                },
                Some(was),
            ) = (
                &mut configuration,
                crate::desktop::configuration(&computers[index]),
            ) {
                next.built_in = was.built_in;
                // A built-in desktop always starts with its computer: computer use needs the
                // session. A controller from before `builtIn` existed (the compatibility
                // path) may still send `startWithComputer: false`; only legacy computers keep a
                // manual choice.
                if was.built_in {
                    next.start_with_computer = true;
                }
            }
            computers[index] = configuration;
        }
        (Some(index), None) => {
            computers.remove(index);
        }
        (None, Some(configuration)) => computers.push(configuration.clone()),
        (None, None) => {}
    }
    Ok(())
}

/// Reorder `computers` to match `order` (a permutation of the current identities), but
/// only when the current order still equals `expected_order`. Optimistic concurrency
/// for the local reorder flow.
pub(crate) fn reorder_computers(
    computers: &mut [ComputerConfiguration],
    order: &[String],
    expected_order: &[String],
) -> Result<(), ChangeRejection> {
    let current: Vec<&str> = computers.iter().map(ComputerConfiguration::id).collect();
    if current
        != expected_order
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    {
        return Err(ChangeRejection::Stale);
    }
    let mut sorted_new: Vec<&str> = order.iter().map(String::as_str).collect();
    sorted_new.sort_unstable();
    let mut sorted_current = current.clone();
    sorted_current.sort_unstable();
    if sorted_new != sorted_current {
        return Err(ChangeRejection::Missing);
    }
    let position: HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(index, id)| (id.as_str(), index))
        .collect();
    computers.sort_by_key(|configuration| position[configuration.id()]);
    Ok(())
}

/// One targeted change to the local computer inventory, sent by the UI with the configuration
/// it started editing from so a queued edit applies to fresh state or is rejected.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ComputerConfigurationChange {
    /// Create or edit a single computer. `expected` is `None` for a create.
    Upsert {
        configuration: ComputerConfiguration,
        #[serde(default)]
        expected: Option<ComputerConfiguration>,
    },
    /// Remove a single computer that currently equals `expected`.
    Delete {
        #[serde(rename = "computerId")]
        computer_id: String,
        expected: ComputerConfiguration,
    },
    /// Reorder the whole inventory. `order`/`expectedOrder` are computer identities.
    Reorder {
        order: Vec<String>,
        #[serde(rename = "expectedOrder")]
        expected_order: Vec<String>,
    },
    /// Apply several changes atomically, in order, against fresh state. Each carries its
    /// own optimistic `expected` check; any rejection rejects the whole batch with no
    /// partial save. Nesting is rejected. Used for onboarding's initial set of creates
    /// and any multi-change submission, replacing the old whole-list save.
    Batch {
        changes: Vec<ComputerConfigurationChange>,
    },
}

impl ComputerConfigurationChange {
    fn label(&self) -> String {
        match self {
            Self::Upsert { .. } => "Saving computer settings".into(),
            Self::Delete { expected, .. } => format!("Deleting {}", expected.name()),
            Self::Reorder { .. } => "Reordering computers".into(),
            Self::Batch { .. } => "Saving computer settings".into(),
        }
    }

    /// Ids of the computers this change removes.
    fn deleted_ids(&self) -> Vec<String> {
        match self {
            Self::Delete { computer_id, .. } => vec![computer_id.clone()],
            Self::Batch { changes } => changes.iter().flat_map(Self::deleted_ids).collect(),
            _ => Vec::new(),
        }
    }

    /// Notification title naming the computer this change is about, when there is one.
    fn failure_title(&self) -> String {
        match self {
            Self::Upsert { configuration, .. } => {
                format!("Couldn\u{2019}t save changes to {}", configuration.name())
            }
            Self::Delete { expected, .. } => format!("Couldn\u{2019}t delete {}", expected.name()),
            Self::Batch { changes } if changes.len() == 1 => changes[0].failure_title(),
            _ => "Couldn\u{2019}t save computer settings".into(),
        }
    }

    fn apply(&self, computers: &mut Vec<ComputerConfiguration>) -> Result<(), String> {
        if let Self::Batch { changes } = self {
            // Apply to a draft so a later rejection leaves the inventory untouched.
            let mut draft = computers.clone();
            for change in changes {
                if matches!(change, Self::Batch { .. }) {
                    return Err("Nested computer configuration batches are not supported.".into());
                }
                change.apply(&mut draft)?;
            }
            *computers = draft;
            return Ok(());
        }
        let outcome = match self {
            Self::Upsert {
                configuration,
                expected,
            } => change_computer(
                computers,
                configuration.id(),
                expected.as_ref(),
                Some(configuration),
            ),
            Self::Delete {
                computer_id,
                expected,
            } => change_computer(computers, computer_id, Some(expected), None),
            Self::Reorder {
                order,
                expected_order,
            } => reorder_computers(computers, order, expected_order),
            Self::Batch { .. } => unreachable!("batch handled above"),
        };
        outcome.map_err(|rejection| match (self, rejection) {
            (Self::Reorder { .. }, _) => {
                "These computers changed while your edit was waiting. Review them and try again."
                    .to_string()
            }
            (_, ChangeRejection::Missing) => "This computer no longer exists.".to_string(),
            (_, ChangeRejection::Stale | ChangeRejection::WrongTarget) => {
                "This computer changed while your edit was waiting. Review it and try again."
                    .to_string()
            }
        })
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSource {
    runtime_repair: Option<Value>,
    computers: Vec<ApplicationComputer>,
    activities: Vec<Value>,
    computer_configuration_operation: Option<Value>,
    repository_push_operations: Vec<Value>,
    github: Value,
    secrets: Vec<Value>,
    /// This device's limits for computer resource ceilings. Absent when the device could not
    /// be measured (every computer change is then rejected by `validate_device_ceiling`).
    #[serde(skip_serializing_if = "Option::is_none")]
    device_capacity: Option<DeviceCapacity>,
}

/// The device limits `validate_device_ceiling` enforces, so editors can clamp defaults and
/// presets instead of offering ceilings Silo will reject.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceCapacity {
    /// Logical CPUs; a computer's CPU ceiling may not exceed this.
    logical_cpus: usize,
    physical_memory_bytes: u64,
    /// The largest whole-GiB memory ceiling Silo accepts on this host.
    max_memory_gib: u64,
}

impl DeviceCapacity {
    fn of(device: &DeviceResources) -> Option<Self> {
        let physical_memory_bytes = device.physical_memory_bytes.filter(|bytes| *bytes > 0)?;
        (device.logical_cpus > 0).then_some(Self {
            logical_cpus: device.logical_cpus,
            physical_memory_bytes,
            max_memory_gib: physical_memory_bytes / (1024 * 1024 * 1024),
        })
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplicationComputer {
    configuration: ComputerConfiguration,
    purpose: String,
    state: ComputerState,
    state_detail: String,
    can_dismiss_error: bool,
    /// Serialized as `lifecycleFailure` (one line) and `lifecycleFailureDiagnostic`
    /// (the runtime's explanation, for a Details disclosure); both omitted when none.
    #[serde(flatten)]
    lifecycle_failure: Option<runtime_activity::LifecycleFailureView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attention: Option<ComputerAttention>,
    freshness: Freshness,
    /// True when an operation on this computer overlapped the read. The runtime fields
    /// (`state`, `stateDetail`, `attention`, `canDismissError`, `repositories`) are then
    /// the last value read while the computer was idle (or this read's, when there is none);
    /// Silo's own records (checkpoints, failures, secrets) stay current. Omitted when false.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    settling: bool,
    repositories: Vec<Value>,
    files: Vec<Value>,
    ports: Vec<Value>,
    logs: Vec<Value>,
    github_repositories: Vec<String>,
    secret_names: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pending_secret_revocations: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    checkpoints: Vec<checkpoints::Checkpoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_checkpoint_restore: Option<checkpoints::PendingRestore>,
    #[serde(skip_serializing_if = "Option::is_none")]
    checkpoint_operation: Option<checkpoints::Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unfinished_restore: Option<checkpoints::UnfinishedRestore>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum ComputerState {
    Running,
    Starting,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
struct ComputerAttention {
    level: AttentionLevel,
    message: String,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum AttentionLevel {
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Freshness {
    Fresh,
    /// This computer's own reading failed while nothing was changing it; the runtime fields
    /// are its last known values and `attention` says why.
    Stale,
}

#[derive(Debug, Deserialize)]
struct ListedSandbox {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(from = "RawInspectedSandbox")]
pub(crate) struct InspectedSandbox {
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) config: Value,
    pub(crate) active_config: Option<Value>,
    pub(crate) updated_at: Option<String>,
    pub(crate) runtime_instance_id: Option<String>,
    /// Whether the runtime's output has a `runtime_instance_id` entry at all (a null value
    /// counts). Silo's runtime patch always writes it; a runtime without the patch never does.
    pub(crate) runtime_instance_reported: bool,
}

#[derive(Deserialize)]
struct RawInspectedSandbox {
    name: String,
    status: String,
    config: Value,
    #[serde(default)]
    active_config: Option<Value>,
    #[serde(default)]
    updated_at: Option<String>,
    /// `None`: no entry. `Some(None)`: an entry that is null.
    #[serde(default, deserialize_with = "present_or_null")]
    runtime_instance_id: Option<Option<String>>,
}

fn present_or_null<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
}

impl From<RawInspectedSandbox> for InspectedSandbox {
    fn from(raw: RawInspectedSandbox) -> Self {
        Self {
            name: raw.name,
            status: raw.status,
            config: raw.config,
            active_config: raw.active_config,
            updated_at: raw.updated_at,
            runtime_instance_reported: raw.runtime_instance_id.is_some(),
            runtime_instance_id: raw.runtime_instance_id.flatten(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DeviceResources {
    logical_cpus: usize,
    physical_memory_bytes: Option<u64>,
}

/// The runtime Silo is using now. Every runtime command, database and metadata file
/// is reached through these paths, so this is where the storage migration gate sits:
/// it refuses until the migration is `complete` or `not-required`. The previous
/// generation is a pre-upgrade backup, so nothing may run `msb` against it while the
/// migration is pending, running or failed, or after the user continued into a fresh
/// runtime.
pub(crate) fn runtime_paths(app: &AppHandle) -> Result<RuntimePaths, String> {
    paths_for_storage(app, &crate::runtime_migration::runtime_storage(app)?)
}

/// `None` while the storage migration holds the runtime back. For callers that only
/// look for running computers to report or stop (Quit, updates, health checks): no
/// runtime is in use then, so nothing runs, and the migration must not make Quit or an
/// update fail. Anything that acts on a computer uses `runtime_paths` and is refused.
pub(crate) fn runtime_paths_if_in_use(app: &AppHandle) -> Result<Option<RuntimePaths>, String> {
    if crate::runtime_migration::blocks_operations(app) {
        return Ok(None);
    }
    runtime_paths(app).map(Some)
}

/// `paths` with no way to start `msb`: every runtime command fails as unavailable
/// before any process or database is touched, while saved files stay readable.
pub(crate) fn without_runtime(mut paths: RuntimePaths) -> RuntimePaths {
    paths.executable = PathBuf::new();
    paths.library = PathBuf::new();
    paths
}

/// The files of the selected storage with no runtime to start, for settling saved
/// state that must finish before the storage migration can start (an interrupted
/// export or import, E-50). Nothing started through these paths can reach `msb`.
pub(crate) fn inert_runtime_paths(app: &AppHandle) -> Result<RuntimePaths, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Silo could not locate its application storage: {error}"))?;
    let storage = crate::runtime_migration::selected_runtime_storage(&app_data)?;
    paths_for_storage(app, &storage).map(without_runtime)
}

/// The runtime stored in `storage`, whatever the migration state. Only the
/// migration's staged conversion may call this, to reach the staged runtime while
/// every other caller is refused. Everything else uses `runtime_paths`.
pub(crate) fn migration_runtime_paths(
    app: &AppHandle,
    storage: &Path,
) -> Result<RuntimePaths, String> {
    paths_for_storage(app, storage)
}

fn paths_for_storage(app: &AppHandle, storage: &Path) -> Result<RuntimePaths, String> {
    let executable =
        crate::bundled_tools::directory(app)?.join(if cfg!(windows) { "msb.exe" } else { "msb" });
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|error| format!("Silo could not locate its bundled resources: {error}"))?;
    let user_home = app.path().home_dir().map_err(|error| error.to_string())?;
    let guest_image = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Silo could not locate its application storage: {error}"))?
        .join("guest-image");
    Ok(paths_in_storage(
        executable,
        &resource_dir,
        &user_home,
        storage,
        guest_image,
    ))
}

fn paths_in_storage(
    executable: PathBuf,
    resource_dir: &Path,
    user_home: &Path,
    storage: &Path,
    guest_image: PathBuf,
) -> RuntimePaths {
    let library = bundled_runtime_library(
        &executable,
        resource_dir,
        tauri::utils::platform::bundle_type(),
    );
    let storage_home = storage.join("microsandbox");
    let home = runtime_home_alias(user_home, &storage_home);
    RuntimePaths {
        guest_image,
        executable,
        home,
        storage_home: Some(storage_home),
        library,
        metadata: storage.join("computers.json"),
        volumes: storage.join("volumes"),
    }
}

pub(crate) fn bundled_runtime_library(
    executable: &Path,
    resource_dir: &Path,
    _bundle: Option<tauri::utils::config::BundleType>,
) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let _ = resource_dir;
        executable
            .parent()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new(""))
            .join("Frameworks/libkrunfw.5.dylib")
    }
    #[cfg(not(target_os = "macos"))]
    {
        if crate::bundled_tools::is_packaged_linux(_bundle) {
            return executable
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join("libkrunfw.so.5.6.1");
        }
        let target = match std::env::consts::ARCH {
            "aarch64" => "aarch64-unknown-linux-gnu",
            "x86_64" => "x86_64-unknown-linux-gnu",
            _ => "unsupported-target",
        };
        resource_dir
            .join("microsandbox")
            .join(target)
            .join("lib/libkrunfw.so.5.6.1")
    }
}

pub(crate) fn runtime_home_alias(user_home: &Path, storage_home: &Path) -> PathBuf {
    let digest = Sha256::digest(storage_home.as_os_str().as_encoded_bytes());
    crate::channel::current()
        .state_dir(user_home)
        .join(format!("{:x}", digest)[..12].to_string())
}

/// Create or secure an account-owned directory without following a symlink.
#[cfg(unix)]
pub(crate) fn prepare_private_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    match builder.create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = directory.metadata()?;
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "Silo's directory {} is owned by UID {}, not this account.",
                path.display(),
                metadata.uid()
            ),
        ));
    }
    if metadata.mode() & 0o077 != 0 {
        directory.set_permissions(fs::Permissions::from_mode(metadata.mode() & 0o700))?;
    }
    Ok(())
}

pub(crate) fn prepare_runtime_home(
    home: &Path,
    storage_home: Option<&Path>,
) -> Result<(), RuntimeError> {
    let prepare = || -> std::io::Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        let Some(storage_home) = storage_home else {
            builder.create(home)?;
            return prepare_private_directory(home);
        };
        let maximum = if cfg!(target_os = "macos") { 103 } else { 107 };
        let longest_socket = home.join("run/sandboxes/000000000000000000000000/control.sock");
        if longest_socket.as_os_str().as_encoded_bytes().len() > maximum {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "This account's home path is too long for MicroSandbox Unix sockets.",
            ));
        }
        let parent = home.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "The Silo runtime alias path is invalid.",
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            prepare_private_directory(parent)?;
            match fs::symlink_metadata(home) {
                Ok(metadata) => {
                    let conflict = if !metadata.file_type().is_symlink() {
                        Some("is an existing file or directory, not a symbolic link".to_string())
                    } else if metadata.uid() != unsafe { libc::geteuid() } {
                        Some(format!(
                            "is owned by UID {}, not this account",
                            metadata.uid()
                        ))
                    } else {
                        let target = fs::read_link(home)?;
                        (target != storage_home).then(|| format!("points to {}", target.display()))
                    };
                    if let Some(conflict) = conflict {
                        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!(
                            "Silo's runtime alias {} {conflict}. Expected a symbolic link to {}. No existing data was changed.",
                            home.display(), storage_home.display()
                        )));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::os::unix::fs::symlink(storage_home, home)?
                }
                Err(error) => return Err(error),
            }
        }
        builder.create(storage_home)?;
        prepare_private_directory(storage_home)
    };
    prepare().map_err(|error| {
        RuntimeError::Unavailable(format!(
            "Silo could not prepare its managed runtime path: {error}"
        ))
    })
}

struct SetupRunner<'a> {
    request_id: &'a str,
    publish: &'a dyn Fn(ComputerConfigurationProgress),
}

impl RuntimeRunner for SetupRunner<'_> {
    fn run(
        &self,
        paths: &RuntimePaths,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        let computer = args
            .windows(2)
            .find(|pair| pair[0] == "--name")
            .map(|pair| pair[1].as_str())
            .unwrap_or("");
        // The first computer on a device imports the VM image, one blocking runtime
        // call with no progress output of its own.
        if matches!(args, [first, second, ..] if first == "image" && second == "load") {
            (self.publish)(computer_progress(
                self.request_id,
                "computer-image-import",
                "",
                0,
            ));
        }
        let layers = Mutex::new(HashMap::<u64, u64>::new());
        let total = Mutex::new(None::<u64>);
        run_msb_with_progress(paths, args, timeout, &|value| {
            let Some(phase) = value.get("phase").and_then(Value::as_str) else {
                return;
            };
            let message = match phase {
                "image-resolving" => "Resolving the VM image…",
                "image-resolved" => "VM image resolved; preparing the download…",
                "image-download" => "Downloading the VM image…",
                "image-downloaded" => "VM image layer downloaded.",
                "image-verifying" => "Checking the downloaded image…",
                "image-preparing" => "Preparing the VM image on disk…",
                "image-ready" => {
                    "VM image ready; preparing the system disk and runtime configuration…"
                }
                "runtime-waiting" => "Waiting for the runtime to finish preparing the computer…",
                _ => return,
            };
            let mut event = computer_progress(self.request_id, phase, computer, 0);
            event.fraction = None;
            event.message = if computer.is_empty() {
                message.into()
            } else {
                format!("{computer}: {message}")
            };
            if phase == "image-resolved" {
                *total.lock().unwrap() = value.get("totalBytes").and_then(Value::as_u64);
            }
            if matches!(phase, "image-download" | "image-downloaded") {
                if let (Some(index), Some(bytes)) = (
                    value.get("layerIndex").and_then(Value::as_u64),
                    value.get("downloadedBytes").and_then(Value::as_u64),
                ) {
                    let mut layers = layers.lock().unwrap();
                    if layers.len() < 1024 || layers.contains_key(&index) {
                        layers.insert(index, bytes);
                        event.downloaded_bytes =
                            Some(layers.values().copied().fold(0u64, u64::saturating_add));
                        event.total_bytes = *total.lock().unwrap();
                    }
                }
            }
            (self.publish)(event);
        })
    }
}

pub(crate) fn run_msb(
    paths: &RuntimePaths,
    args: &[String],
    timeout: Duration,
) -> Result<CommandOutput, RuntimeError> {
    if matches!(
        args.first().map(String::as_str),
        Some("start" | "restart" | "exec")
    ) {
        if let Some(name) = args.get(1) {
            if let Some(configuration) = read_metadata(&paths.metadata)?
                .computers
                .into_iter()
                .find(|configuration| configuration.name() == name)
            {
                if checkpoints::needs_explicit_start(paths, configuration.id())? {
                    return Err(RuntimeError::Invalid(checkpoints::explicit_start_message(
                        paths,
                        configuration.id(),
                        configuration.name(),
                    )));
                }
            }
        }
    }
    run_msb_with_progress(paths, args, timeout, &|_| {})
}

fn run_msb_with_progress(
    paths: &RuntimePaths,
    args: &[String],
    timeout: Duration,
    report: &dyn Fn(Value),
) -> Result<CommandOutput, RuntimeError> {
    if let Some(computer) = args.get(1).filter(|computer| {
        validate_name(computer).is_ok()
            && matches!(
                args.first().map(String::as_str),
                Some("start" | "restart" | "exec")
            )
    }) {
        // `--no-start` never boots the computer, so it must not wait behind a live access
        // change (read paths such as repository discovery).
        if args[0] == "exec"
            && args
                .iter()
                .take_while(|arg| arg.as_str() != "--")
                .any(|arg| arg == "--no-start")
        {
            return run_msb_process(paths, args, timeout, report);
        }
        let access =
            computer_access_state(&paths.home, computer).map_err(RuntimeError::Unavailable)?;
        let boot_wait = if args[0] == "exec" {
            MUTATION_TIMEOUT
        } else {
            timeout
        };
        let guard = lock_computer_runtime(&access, boot_wait, &operation_name(args))?;
        if args[0] != "exec" {
            lifecycle_step(LifecycleStep::Boot);
            let result = run_msb_process(paths, args, timeout, report);
            drop(guard);
            if result.is_ok() {
                lifecycle_step(LifecycleStep::Network);
                crate::network::reconcile_started(paths, computer);
                crate::ssh_access::reconcile(paths);
                lifecycle_step(LifecycleStep::Account);
            }
            return result.and_then(|output| prepare_booted(paths, computer).map(|()| output));
        }
        // Finish a possible boot under the same lock as live access changes,
        // then release it before running arbitrary, possibly long guest commands.
        let state = inspect_computer(&ProcessRunner, paths, computer)?;
        let temporary_boot = matches!(state.status.as_str(), "Created" | "Stopped" | "Crashed");
        if temporary_boot {
            if let Err(error) = run_msb_process(
                paths,
                &["start".into(), computer.clone()],
                MUTATION_TIMEOUT,
                report,
            ) {
                // A cancelled or failed start may already have booted the computer. The
                // cleanup stop is not cancellable, so the computer is not left running.
                let _ = without_cancellation(|| {
                    run_msb_process(
                        paths,
                        &["stop".into(), computer.clone()],
                        STOP_TIMEOUT,
                        &|_| {},
                    )
                });
                drop(guard);
                crate::ssh_access::reconcile(paths);
                return Err(error);
            }
        }
        drop(guard);
        if temporary_boot {
            crate::network::reconcile_started(paths, computer);
            crate::ssh_access::reconcile(paths);
        }
        let result = if temporary_boot {
            crate::working_account::prepare(&Booted, paths, computer)
        } else {
            Ok(())
        }
        .and_then(|()| run_msb_process(paths, args, timeout, report));
        if temporary_boot {
            // Preserve msb exec's temporary-boot behavior even on guest failure. The
            // stop runs even if a concurrent access update keeps the lock busy.
            let stopped = without_cancellation(|| {
                let _guard =
                    lock_computer_runtime(&access, STOP_TIMEOUT, "Stopping the computer").ok();
                run_msb_process(
                    paths,
                    &["stop".into(), computer.clone()],
                    STOP_TIMEOUT,
                    &|_| {},
                )
            });
            crate::ssh_access::reconcile(paths);
            return match (result, stopped) {
                (Ok(output), Ok(_)) => Ok(output),
                (Err(error), Ok(_)) => Err(error),
                (Ok(_), Err(error)) => Err(error),
                (Err(error), Err(cleanup)) => Err(RuntimeError::Unavailable(format!(
                    "{error} Stopping the temporary computer also failed: {cleanup}"
                ))),
            };
        }
        return result;
    }
    let result = run_msb_process(paths, args, timeout, report);
    if args
        .first()
        .is_some_and(|command| matches!(command.as_str(), "stop" | "remove"))
    {
        crate::ssh_access::reconcile(paths);
    }
    // A restore boots the restored computer under its new name.
    let restored = args
        .windows(2)
        .find(|pair| args[0] == "restore" && pair[0] == "--name")
        .map(|pair| pair[1].as_str());
    match restored {
        Some(name) => result.and_then(|output| prepare_booted(paths, name).map(|()| output)),
        None => result,
    }
}

/// Runs the account setup of a computer being booted. Unlike `ProcessRunner`, it does not
/// wait for a pending checkpoint's explicit Start: the boot is that Start.
struct Booted;

impl RuntimeRunner for Booted {
    fn run(
        &self,
        paths: &RuntimePaths,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        run_msb_with_progress(paths, args, timeout, &|_| {})
    }
}

/// Make a computer Silo just booted ready for the silo account, or stop it again, so a running
/// Computer always has its account. The stop is not cancellable.
fn prepare_booted(paths: &RuntimePaths, computer: &str) -> Result<(), RuntimeError> {
    let prepared = crate::working_account::prepare(&Booted, paths, computer);
    if prepared.is_err() {
        let _ = without_cancellation(|| {
            let access = computer_access_state(&paths.home, computer).ok();
            let _guard = access.as_deref().and_then(|access| {
                lock_computer_runtime(access, STOP_TIMEOUT, "Stopping the computer").ok()
            });
            run_msb_process(
                paths,
                &["stop".into(), computer.into()],
                STOP_TIMEOUT,
                &|_| {},
            )
        });
        crate::ssh_access::reconcile(paths);
    } else {
        // Built-in computer use installs itself in the background; it never fails a boot.
        let _ = crate::computer_use::after_boot(std::sync::Arc::new(Booted), paths, computer);
    }
    prepared
}

/// Where a start is, reported while the operation gate is held so the UI can follow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LifecycleStep {
    /// The computer is booting.
    Boot,
    /// The computer is up; published ports and SSH access are being connected.
    Network,
    /// The computer's working account is being checked.
    Account,
}

impl LifecycleStep {
    fn id(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Network => "network",
            Self::Account => "account",
        }
    }
}

type LifecycleSink = std::rc::Rc<dyn Fn(LifecycleStep)>;

thread_local! {
    /// Receiver of start progress for the lifecycle command running on this thread.
    static LIFECYCLE_SINK: std::cell::RefCell<Option<LifecycleSink>> = const { std::cell::RefCell::new(None) };
}

/// Report start progress to this thread's lifecycle command, if one is listening.
fn lifecycle_step(step: LifecycleStep) {
    let sink = LIFECYCLE_SINK.with(|sink| sink.borrow().clone());
    if let Some(sink) = sink {
        sink(step);
    }
}

/// Run `work` with `sink` receiving the start progress it reports on this thread.
fn with_lifecycle_sink<T>(sink: LifecycleSink, work: impl FnOnce() -> T) -> T {
    let previous = LIFECYCLE_SINK.with(|slot| slot.replace(Some(sink)));
    let result = work();
    LIFECYCLE_SINK.with(|slot| *slot.borrow_mut() = previous);
    result
}

thread_local! {
    /// Set while cleanup that must finish (such as the stop after exec's temporary
    /// boot) runs, so a cancel of the surrounding operation does not kill it.
    static CANCELLATION_MASKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run `work` with this thread's operation cancellation ignored by runtime commands.
fn without_cancellation<T>(work: impl FnOnce() -> T) -> T {
    let previous = CANCELLATION_MASKED.with(|masked| masked.replace(true));
    let result = work();
    CANCELLATION_MASKED.with(|masked| masked.set(previous));
    result
}

fn runtime_cancel_requested() -> bool {
    !CANCELLATION_MASKED.with(std::cell::Cell::get) && operation_gate::cancel_requested()
}

fn run_msb_process(
    paths: &RuntimePaths,
    args: &[String],
    timeout: Duration,
    report: &dyn Fn(Value),
) -> Result<CommandOutput, RuntimeError> {
    // Tear down client sessions before any runtime command can end or replace
    // their computer. This also covers exec's temporary boot and stop cleanup path.
    if args
        .first()
        .is_some_and(|command| matches!(command.as_str(), "stop" | "remove" | "restart"))
    {
        if let Some(computer) = args
            .iter()
            .skip(1)
            .find(|argument| !argument.starts_with('-'))
        {
            crate::ssh_access::close_computer(computer);
        }
    }
    ensure_runtime_files(paths)?;
    let mut secret_revision = None;
    let general_secrets = if let Some(computer) = github_command_computer(args) {
        secret_revision =
            Some(crate::secrets::computer_revision(computer).map_err(RuntimeError::Unavailable)?);
        let material =
            crate::secrets::runtime_material(computer).map_err(RuntimeError::Unavailable)?;
        secrets_runtime::validate_material(&material).map_err(RuntimeError::Invalid)?;
        if matches!(args[0].as_str(), "start" | "restart") {
            secrets_runtime::apply(paths, computer, &material, true).map_err(|attempt| {
                // A cancel during the boot-time secret application must surface as a
                // cancellation, not an unavailable-runtime failure, so callers and the
                // retry boundary treat it as the user's stop rather than a fault.
                match attempt {
                    secrets_runtime::Attempt::Cancelled(_) => RuntimeError::Cancelled {
                        operation: operation_name(args),
                    },
                    other => RuntimeError::Unavailable(String::from(other)),
                }
            })?;
        }
        material
    } else {
        Vec::new()
    };
    let output = spawn_runtime(
        paths,
        RuntimeLaunch {
            args,
            timeout,
            material: &general_secrets,
            github_profile: &github_environment(paths, args),
            capture: true,
            report,
        },
    )?;
    if let Some(computer) =
        github_command_computer(args).filter(|_| matches!(args[0].as_str(), "start" | "restart"))
    {
        record_start_refresh(paths, computer, || {
            let inspected = inspect_computer(&ProcessRunner, paths, computer)?;
            if inspected.status == "Running" {
                crate::secrets::computer_started(
                    computer,
                    secret_revision.as_deref().unwrap_or_default(),
                )
                .map_err(|_| {
                    RuntimeError::Unavailable(
                        "Silo could not record the computer's applied secret revision.".into(),
                    )
                })?;
            }
            Ok(())
        });
    }
    Ok(output)
}

// Post-boot reads and host bookkeeping cannot undo a successful runtime start.
// Keep a visible hint until a later start verifies both state and secret revision.
type StartRefreshWarnings = HashMap<(PathBuf, String), String>;
static START_REFRESH_WARNINGS: OnceLock<Mutex<StartRefreshWarnings>> = OnceLock::new();

fn record_start_refresh(
    paths: &RuntimePaths,
    computer: &str,
    refresh: impl FnOnce() -> Result<(), RuntimeError>,
) {
    let result = refresh();
    let mut warnings = START_REFRESH_WARNINGS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = (paths.home.clone(), computer.to_owned());
    match result {
        Ok(()) => {
            warnings.remove(&key);
        }
        Err(error) => {
            warnings.insert(key, format!("The computer started, but Silo could not refresh its state or secret status. {}", safe_activity_error(&error)));
        }
    }
}

fn start_refresh_attention(paths: &RuntimePaths, computer: &str) -> Option<ComputerAttention> {
    START_REFRESH_WARNINGS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&(paths.home.clone(), computer.to_owned()))
        .cloned()
        .map(|message| ComputerAttention {
            level: AttentionLevel::Warning,
            message,
        })
}

fn ensure_runtime_files(paths: &RuntimePaths) -> Result<(), RuntimeError> {
    for (description, path) in [
        ("bundled MicroSandbox executable", &paths.executable),
        ("bundled MicroSandbox library", &paths.library),
    ] {
        if !path.is_file() {
            return Err(RuntimeError::Unavailable(format!(
                "The {description} is unavailable in this Silo installation."
            )));
        }
    }
    prepare_runtime_home(&paths.home, paths.storage_home.as_deref())
}

/// One runtime child. Every `msb` launch goes through `spawn_runtime`, so the
/// executable checks, runtime-home preparation, the worker lock for mutating
/// commands, secret and GitHub transport, cancellation, timeouts and output limits are
/// the same for lifecycle commands, secret updates and GitHub access updates.
struct RuntimeLaunch<'a> {
    args: &'a [String],
    timeout: Duration,
    /// Secret values the runtime resolves by source name. Only names reach argv.
    material: &'a secrets_runtime::Material,
    /// The GitHub access profile the runtime resolves for `SILO_GITHUB`.
    github_profile: &'a str,
    /// False discards stdout and stderr: updates that carry secret material never
    /// keep runtime output, not even in temporary files.
    capture: bool,
    report: &'a dyn Fn(Value),
}

fn ignore_progress(_: Value) {}

/// Asks the bundled runtime (Silo's secret-values patch) to read secret source values
/// from standard input instead of its environment.
const SECRET_VALUES_STDIN_FLAG: &str = "MSB_SECRET_VALUES_STDIN";

/// The secret source values one runtime child may resolve, as the JSON object it reads
/// on standard input: the GitHub access profile under `SILO_GITHUB` and each general
/// secret under a generated source name (guest names never become host variables).
fn secret_values_document(
    material: &secrets_runtime::Material,
    github_profile: &str,
) -> Result<Vec<u8>, RuntimeError> {
    let mut values = serde_json::Map::new();
    values.insert("SILO_GITHUB".into(), github_profile.into());
    for (name, value, _) in material {
        if values
            .insert(secrets_runtime::source_name(name), value.as_str().into())
            .is_some()
        {
            return Err(RuntimeError::Invalid(
                "Computer secret source names conflict. Rename one secret and retry.".into(),
            ));
        }
    }
    serde_json::to_vec(&Value::Object(values))
        .map_err(|_| RuntimeError::Invalid("Silo could not prepare the computer's secrets.".into()))
}

/// Commands that change runtime state hold the worker lock for the child's lifetime,
/// so they never overlap another Silo process's runtime mutation.
fn takes_worker_lock(args: &[String]) -> bool {
    args.first().is_some_and(|command| {
        matches!(
            command.as_str(),
            "create" | "modify" | "remove" | "stop" | "restore" | "adopt-disk"
        )
    })
}

/// Arguments handed to the bundled `msb`. A noninteractive `exec` (`--no-tty`) never
/// forwards host input: Silo's only standard input is the secret document the runtime
/// consumes at startup. `--no-stdin` (MicroSandbox 0.7.5 and later) gives the guest EOF
/// without starting a stdin forwarder, so an open pipe can never hold the command up.
/// Interactive terminals (`--tty`) and commands that already choose are left as written.
fn runtime_arguments(args: &[String]) -> Vec<String> {
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    if args.first().map(String::as_str) != Some("exec")
        || !has("--no-tty")
        || has("--tty")
        || has("--no-stdin")
    {
        return args.to_vec();
    }
    let mut adjusted = args.to_vec();
    let at = adjusted
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(adjusted.len());
    adjusted.insert(at, "--no-stdin".into());
    adjusted
}

thread_local! {
    // Quit already owns the device gate and a parent worker flock. Its scoped
    // workers may share that flock only for independent stop commands.
    static SHUTDOWN_WORKER_LOCK: std::cell::RefCell<Option<configuration_recovery::CommandLock>> = const { std::cell::RefCell::new(None) };
}

fn with_shutdown_worker_lock<T>(
    lock: configuration_recovery::CommandLock,
    work: impl FnOnce() -> T,
) -> T {
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            SHUTDOWN_WORKER_LOCK.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }
    SHUTDOWN_WORKER_LOCK.with(|slot| {
        assert!(
            slot.borrow().is_none(),
            "shutdown worker lock cannot be nested"
        );
        *slot.borrow_mut() = Some(lock);
    });
    let _clear = Clear;
    work()
}

fn runtime_worker_lock(
    paths: &RuntimePaths,
    args: &[String],
    timeout: Duration,
) -> Result<configuration_recovery::CommandLock, RuntimeError> {
    if args.first().is_some_and(|command| command == "stop") {
        if let Some(lock) = SHUTDOWN_WORKER_LOCK.with(|slot| {
            slot.borrow()
                .as_ref()
                .map(configuration_recovery::CommandLock::duplicate_for_shutdown)
        }) {
            return lock;
        }
    }
    configuration_recovery::command_lock(paths, timeout)
}

fn spawn_runtime(
    paths: &RuntimePaths,
    launch: RuntimeLaunch<'_>,
) -> Result<CommandOutput, RuntimeError> {
    let RuntimeLaunch {
        args,
        timeout,
        material,
        github_profile,
        capture,
        report,
    } = launch;
    ensure_runtime_files(paths)?;
    let captures = if capture {
        let stdout = tempfile::NamedTempFile::new().map_err(|error| {
            RuntimeError::Unavailable(format!("Silo could not capture runtime output: {error}"))
        })?;
        let stderr = tempfile::NamedTempFile::new().map_err(|error| {
            RuntimeError::Unavailable(format!("Silo could not capture runtime errors: {error}"))
        })?;
        Some((stdout, stderr))
    } else {
        None
    };
    let mut worker_lock = if takes_worker_lock(args) {
        Some(runtime_worker_lock(paths, args, timeout)?)
    } else {
        None
    };
    let mut command = Command::new(&paths.executable);
    if let Some(lock) = &worker_lock {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        let fd = lock.as_raw_fd();
        // SAFETY: only async-signal-safe fcntl runs between fork and exec.
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    // Secret values never enter the runtime's environment (readable by other processes
    // of this user, and where a secret named like a host variable could change the
    // runtime's behaviour). The runtime reads them from standard input instead, and
    // then resolves secret sources only from them (D-45, B-28).
    let secret_values = secret_values_document(material, github_profile)?;
    command
        .args(runtime_arguments(args))
        .env("MSB_HOME", &paths.home)
        .env("MSB_PATH", &paths.executable)
        .env("MSB_LIBKRUNFW_PATH", &paths.library)
        .env(SECRET_VALUES_STDIN_FLAG, "1")
        .stdin(Stdio::piped());
    if let Some((stdout, stderr)) = &captures {
        command
            .stdout(Stdio::from(stdout.as_file().try_clone().map_err(
                |error| {
                    RuntimeError::Unavailable(format!(
                        "Silo could not capture runtime output: {error}"
                    ))
                },
            )?))
            .stderr(Stdio::from(stderr.as_file().try_clone().map_err(
                |error| {
                    RuntimeError::Unavailable(format!(
                        "Silo could not capture runtime errors: {error}"
                    ))
                },
            )?));
    } else {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let mut child = command.spawn().map_err(|error| {
        RuntimeError::Launch(format!("Silo could not start its bundled runtime: {error}"))
    })?;
    // The runtime reads the whole document before anything else; a separate writer
    // never blocks this thread, even if the child exits without reading it.
    if let Some(mut stdin) = child.stdin.take() {
        thread::spawn(move || {
            let _ = stdin.write_all(&secret_values);
        });
    }
    // Spawn succeeded with pre_exec clearing close-on-exec for this lock only.
    // A surviving child must keep the flock if Silo exits before it does.
    if let Some(lock) = worker_lock.as_mut() {
        lock.mark_inherited_by_child();
    }
    let mut stop_child = |child: &mut std::process::Child| {
        let _ = child.kill();
        if child.wait().is_ok() {
            if let Some(lock) = worker_lock.as_mut() {
                lock.mark_child_exited();
            }
        }
    };
    let deadline = Instant::now() + timeout;
    let mut progress_offset = 0;
    let mut progress_pending = String::new();
    let mut next_progress = Instant::now();
    let mut last_progress = Instant::now();
    let mut last_phase = serde_json::json!({"phase": "runtime-waiting"});
    let mut exited = None;
    let status = loop {
        if let Some((stdout_file, stderr_file)) = &captures {
            if args.iter().any(|arg| arg == "--progress-json") && Instant::now() >= next_progress {
                next_progress = Instant::now() + Duration::from_secs(1);
                if let Ok(mut capture) = stderr_file.reopen() {
                    let _ = capture.seek(SeekFrom::Start(progress_offset));
                    let mut bytes = Vec::new();
                    if capture
                        .take(MAX_OUTPUT_BYTES)
                        .read_to_end(&mut bytes)
                        .is_ok()
                    {
                        progress_offset += bytes.len() as u64;
                        progress_pending.push_str(&String::from_utf8_lossy(&bytes));
                        let mut latest = None;
                        while let Some(end) = progress_pending.find('\n') {
                            let line: String = progress_pending.drain(..=end).collect();
                            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                                if value.get("type").and_then(Value::as_str)
                                    == Some("silo-progress")
                                {
                                    // Keep phase boundaries; collapse repeated chunk updates within this poll.
                                    if latest.as_ref().is_some_and(|old: &Value| {
                                        old.get("phase") != value.get("phase")
                                            || old.get("layerIndex") != value.get("layerIndex")
                                    }) {
                                        report(latest.take().unwrap());
                                    }
                                    latest = Some(value);
                                }
                            }
                        }
                        if let Some(value) = latest {
                            last_phase = value.clone();
                            report(value);
                            last_progress = Instant::now();
                        }
                    }
                }
                if last_progress.elapsed() >= Duration::from_secs(5) {
                    report(last_phase.clone());
                    last_progress = Instant::now();
                }
            }
            let too_large = |file: &tempfile::NamedTempFile| {
                file.as_file()
                    .metadata()
                    .map(|value| value.len())
                    .unwrap_or(0)
                    > MAX_OUTPUT_BYTES
            };
            if too_large(stdout_file) || too_large(stderr_file) {
                stop_child(&mut child);
                return Err(RuntimeError::Failed {
                    operation: operation_name(args),
                    exit_code: None,
                    detail: "the runtime returned too much output".into(),
                });
            }
        }
        if let Some(status) = exited.take() {
            break status;
        }
        // A cancellable operation asked to stop: kill the child like the timeout path.
        if runtime_cancel_requested() {
            stop_child(&mut child);
            return Err(RuntimeError::Cancelled {
                operation: operation_name(args),
            });
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                exited = Some(status);
                next_progress = Instant::now();
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                stop_child(&mut child);
                return Err(RuntimeError::TimedOut {
                    operation: operation_name(args),
                });
            }
            Err(error) => {
                stop_child(&mut child);
                return Err(RuntimeError::Failed {
                    operation: operation_name(args),
                    exit_code: None,
                    detail: format!("the process could not be observed: {error}"),
                });
            }
        }
    };
    if let Some(lock) = worker_lock.as_mut() {
        lock.mark_child_exited();
    }
    let (stdout, stderr) = match captures {
        Some((stdout_file, stderr_file)) => (
            read_capture(stdout_file.into_file())?,
            read_capture(stderr_file.into_file())?,
        ),
        None => (String::new(), String::new()),
    };
    if !status.success() {
        let stderr_detail = runtime_error_text(&stderr);
        let stdout_detail = runtime_error_text(&stdout);
        let raw_detail = if stderr_detail.trim().is_empty() {
            &stdout_detail
        } else {
            &stderr_detail
        };
        return Err(RuntimeError::Failed {
            operation: operation_name(args),
            // A child ended by a signal has no exit status; report it as -1.
            exit_code: Some(status.code().unwrap_or(-1)),
            detail: clean_detail(raw_detail, &paths.home),
        });
    }
    Ok(CommandOutput { stdout, stderr })
}

fn runtime_error_text(capture: &str) -> String {
    capture
        .lines()
        .filter(|line| {
            !serde_json::from_str::<Value>(line)
                .ok()
                .is_some_and(|event| {
                    event.get("type").and_then(Value::as_str) == Some("silo-progress")
                })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn mentions_http_status(detail: &str, status: &str) -> bool {
    let words: Vec<_> = detail
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    words
        .windows(2)
        .any(|pair| matches!(pair[0], "http" | "status") && pair[1] == status)
        || words
            .windows(3)
            .any(|parts| parts[0] == "status" && parts[1] == "code" && parts[2] == status)
}

fn read_capture(mut file: File) -> Result<String, RuntimeError> {
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        RuntimeError::Unavailable(format!("Silo could not read runtime output: {error}"))
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            RuntimeError::Unavailable(format!("Silo could not read runtime output: {error}"))
        })?;
    if bytes.len() as u64 > MAX_OUTPUT_BYTES {
        return Err(RuntimeError::Malformed(
            "The bundled runtime returned too much output.".into(),
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| RuntimeError::Malformed("The bundled runtime returned invalid text.".into()))
}

fn operation_name(args: &[String]) -> String {
    match args.first().map(String::as_str) {
        Some("create") => "Creating the computer".into(),
        Some("start") => "Starting the computer".into(),
        Some("stop") => "Stopping the computer".into(),
        Some("restart") => "Restarting the computer".into(),
        Some("modify") => "Updating the computer".into(),
        Some("remove") => "Removing the computer".into(),
        Some("inspect") | Some("list") => "Reading computer state".into(),
        _ => "The computer operation".into(),
    }
}

fn clean_detail(detail: &str, home: &Path) -> String {
    let detail = detail.trim();
    if detail.is_empty() {
        return "the bundled runtime did not provide an error message".into();
    }
    detail.replace(home.to_string_lossy().as_ref(), "Silo managed storage")
}

fn device_resources() -> Result<DeviceResources, RuntimeError> {
    let logical_cpus = thread::available_parallelism()
        .map_err(|error| {
            RuntimeError::Unavailable(format!("Silo could not inspect host CPUs: {error}"))
        })?
        .get();
    Ok(DeviceResources {
        logical_cpus,
        physical_memory_bytes: physical_memory_bytes()?,
    })
}

#[cfg(target_os = "macos")]
fn physical_memory_bytes() -> Result<Option<u64>, RuntimeError> {
    let name = CString::new("hw.memsize").expect("static sysctl name");
    let mut value = 0_u64;
    let mut length = std::mem::size_of::<u64>();
    // SAFETY: `name` is NUL-terminated and both output pointers refer to
    // writable values of the declared length for the duration of the call.
    let result = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut u64).cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if result == 0 && length == std::mem::size_of::<u64>() && value > 0 {
        Ok(Some(value))
    } else {
        Err(RuntimeError::Unavailable(
            "Silo could not measure physical memory for this computer operation.".into(),
        ))
    }
}

#[cfg(target_os = "linux")]
fn physical_memory_bytes() -> Result<Option<u64>, RuntimeError> {
    // SAFETY: zero is a valid initial bit pattern for `libc::sysinfo`, and the
    // kernel writes the complete structure through the exclusive pointer.
    let mut info: libc::sysinfo = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::sysinfo(&mut info) };
    let bytes = (info.totalram as u64).checked_mul(u64::from(info.mem_unit));
    if result == 0 && bytes.is_some_and(|value| value > 0) {
        Ok(bytes)
    } else {
        Err(RuntimeError::Unavailable(
            "Silo could not measure physical memory for this computer operation.".into(),
        ))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn physical_memory_bytes() -> Result<Option<u64>, RuntimeError> {
    Err(RuntimeError::Unavailable(
        "Local MicroSandbox computers are unavailable on this operating system.".into(),
    ))
}

fn validate_requested_resources(
    request: &ComputerConfigurationRequest,
    device: &DeviceResources,
) -> Result<(), RuntimeError> {
    for configuration in &request.computers {
        let ComputerConfiguration {
            name,
            max_cpus,
            max_memory_gib,
            ..
        } = configuration;
        validate_device_ceiling(name, *max_cpus, *max_memory_gib, device)?;
    }
    Ok(())
}

fn validate_inspected_resources(
    name: &str,
    config: &Value,
    device: &DeviceResources,
) -> Result<(), RuntimeError> {
    let max_cpus = config
        .pointer("/resources/max_cpus")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| {
            RuntimeError::Malformed(format!(
                "Computer '{name}' does not report a valid CPU ceiling. It was not started."
            ))
        })?;
    let max_memory_mib = config
        .pointer("/resources/max_memory_mib")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            RuntimeError::Malformed(format!(
                "Computer '{name}' does not report a valid memory ceiling. It was not started."
            ))
        })?;
    let max_memory_gib = u32::try_from(max_memory_mib.div_ceil(1024)).map_err(|_| {
        RuntimeError::Malformed(format!(
            "Computer '{name}' reports an invalid memory ceiling. It was not started."
        ))
    })?;
    validate_device_ceiling(name, max_cpus, max_memory_gib, device)
}

fn validate_device_ceiling(
    name: &str,
    max_cpus: u8,
    max_memory_gib: u32,
    device: &DeviceResources,
) -> Result<(), RuntimeError> {
    if max_cpus == 0 || max_memory_gib == 0 || device.logical_cpus == 0 {
        return Err(RuntimeError::Unavailable(
            "Silo could not verify valid device and computer resource limits.".into(),
        ));
    }
    let physical = device
        .physical_memory_bytes
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            RuntimeError::Unavailable(
                "Silo could not verify physical memory for this computer operation.".into(),
            )
        })?;
    if usize::from(max_cpus) > device.logical_cpus {
        return Err(RuntimeError::Invalid(format!(
            "Computer '{name}' has a {max_cpus} CPU ceiling, but this device reports {} logical CPUs. No computer was started or changed.",
            device.logical_cpus
        )));
    }
    let requested_memory = u64::from(max_memory_gib)
        .checked_mul(1024 * 1024 * 1024)
        .ok_or_else(|| {
            RuntimeError::Invalid(format!("Computer '{name}' requests too much memory."))
        })?;
    if requested_memory > physical {
        return Err(RuntimeError::Invalid(format!(
            "Computer '{name}' has a {max_memory_gib} GiB memory ceiling, but this device reports {} GiB of physical memory. No computer was started or changed.",
            physical / (1024 * 1024 * 1024)
        )));
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerIdentity {
    computer: String,
    name: String,
    email: String,
    apply: bool,
}

#[tauri::command]
pub async fn verify_computer_identities(
    app: AppHandle,
    identities: Vec<ComputerIdentity>,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let paths = runtime_paths(&app)?;
        shutdown::ensure_accepting_operations()?;
        // Each computer is checked in its own lane, one at a time, so other computers keep working.
        verify_computer_identities_in(&ProcessRunner, &paths, &identities, &identity_lane)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| internal_failure("checking Git identities"))?
}

/// Holds one computer's lane while its Git identity is checked or written. Returns `None` when
/// the caller already holds the gate (and in tests).
type IdentityLane<'a> =
    &'a dyn Fn(
        &ComputerConfiguration,
        &str,
    ) -> Result<Option<operation_gate::OperationGuard<'static>>, RuntimeError>;

/// Identity work on one computer: its own, cancellable queue entry naming the computer.
fn identity_lane(
    configuration: &ComputerConfiguration,
    label: &str,
) -> Result<Option<operation_gate::OperationGuard<'static>>, RuntimeError> {
    let guard = OPERATIONS.computer(configuration.id(), configuration.name(), label)?;
    guard.allow_cancel();
    guard.expect_within(Duration::from_secs(600));
    Ok(Some(guard))
}

fn no_identity_lane(
    _computer: &ComputerConfiguration,
    _label: &str,
) -> Result<Option<operation_gate::OperationGuard<'static>>, RuntimeError> {
    Ok(None)
}

#[cfg(test)]
fn verify_computer_identities_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    identities: &[ComputerIdentity],
) -> Result<bool, RuntimeError> {
    verify_computer_identities_in(runner, paths, identities, &no_identity_lane)
}

/// Whether every requested identity is already applied. Verification only decides a
/// status, so it never boots a computer: a stopped computer's identity is unknown (`false`), and a
/// running computer is checked in place with `exec --no-start`.
fn verify_computer_identities_in(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    identities: &[ComputerIdentity],
    lane: IdentityLane<'_>,
) -> Result<bool, RuntimeError> {
    if identities.len() > MAX_COMPUTER_COUNT {
        return Ok(false);
    }
    let metadata = read_metadata(&paths.metadata)?;
    if identities.is_empty() {
        return Ok(metadata.computers.is_empty());
    }
    let mut names = HashSet::new();
    for identity in identities {
        validate_name(&identity.computer)?;
        if !names.insert(&identity.computer)
            || !metadata
                .computers
                .iter()
                .any(|configuration| configuration.name() == identity.computer)
        {
            return Ok(false);
        }
        let Some(configuration) = metadata
            .computers
            .iter()
            .find(|configuration| configuration.name() == identity.computer)
        else {
            return Ok(false);
        };
        let _lane = lane(
            configuration,
            &format!("Checking Git identity for {}", configuration.name()),
        )?;
        ensure_current_computer(paths, configuration)?;
        let inspected = inspect_computer(runner, paths, &identity.computer)?;
        ensure_computer_identity(configuration, &inspected)?;
        if !identity.apply {
            continue;
        }
        if [&identity.name, &identity.email].iter().any(|value| {
            value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control)
        }) {
            return Ok(false);
        }
        if inspected.status != "Running" {
            return Ok(false);
        }
        if !verify_guest_identity(runner, paths, identity, GuestBoot::Never)? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[tauri::command]
pub async fn configure_computer_identities(
    app: AppHandle,
    identities: Vec<ComputerIdentity>,
) -> Result<(), String> {
    let notify_app = app.clone();
    let names: Vec<String> = identities
        .iter()
        .map(|identity| identity.computer.clone())
        .collect();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let paths = runtime_paths(&app)?;
        shutdown::ensure_accepting_operations()?;
        // Every request is validated first; then each computer is written in its own lane, one
        // at a time, cancellable, and named in the queue.
        let result =
            configure_computer_identities_in(&ProcessRunner, &paths, &identities, &identity_lane);
        result.map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| internal_failure("saving Git identities"))
    .and_then(|result| result);
    if let Err(message) = &result {
        crate::notifications::notify_native(&notify_app, git_identity_notice(&names, message));
    }
    result
}

fn git_identity_notice(names: &[String], message: &str) -> crate::notifications::Notice {
    let title = match names {
        [only] => format!("Couldn\u{2019}t save the Git identity for {only}"),
        _ => "Couldn\u{2019}t save Git identities".to_string(),
    };
    crate::notifications::failure("git-identity", &title, message, None)
}

fn configure_computer_identities_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    identities: &[ComputerIdentity],
) -> Result<(), RuntimeError> {
    configure_computer_identities_in(runner, paths, identities, &no_identity_lane)
}

fn configure_computer_identities_in(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    identities: &[ComputerIdentity],
    lane: IdentityLane<'_>,
) -> Result<(), RuntimeError> {
    if identities.len() > MAX_COMPUTER_COUNT {
        return Err(RuntimeError::Invalid(
            "Too many computer identities.".into(),
        ));
    }
    let metadata = read_metadata(&paths.metadata)?;
    let mut names = HashSet::new();
    let mut changed = Vec::new();
    for identity in identities.iter().filter(|identity| identity.apply) {
        validate_name(&identity.computer)?;
        if !names.insert(&identity.computer)
            || [&identity.name, &identity.email].iter().any(|value| {
                value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control)
            })
        {
            return Err(RuntimeError::Invalid(
                "Each computer needs one valid Git name and email address.".into(),
            ));
        }
        let Some(configuration) = metadata
            .computers
            .iter()
            .find(|configuration| configuration.name() == identity.computer)
        else {
            return Err(RuntimeError::Invalid(format!(
                "Computer '{}' is not a configured local computer. Its Git identity was not changed.",
                identity.computer
            )));
        };
        let inspected = inspect_computer(runner, paths, &identity.computer)?;
        ensure_computer_identity(configuration, &inspected)?;
        changed.push((identity, configuration));
    }
    for (identity, configuration) in changed {
        let _lane = lane(
            configuration,
            &format!("Saving Git identity for {}", configuration.name()),
        )?;
        ensure_current_computer(paths, configuration)?;
        ensure_computer_identity(
            configuration,
            &inspect_computer(runner, paths, &identity.computer)?,
        )?;
        // Remove old boot overrides: normal Git/jj configuration must own defaults.
        let mut args = vec!["modify".into(), identity.computer.clone()];
        for key in checkpoints::IDENTITY_ENVIRONMENT {
            args.extend(["--env-rm".into(), key.into()]);
        }
        args.extend(["--format".into(), "json".into()]);
        runner.run(paths, &args, MUTATION_TIMEOUT)?;
        checkpoints::forget_identity_environment(paths, configuration.id())?;
        let script = r#"set -eu
 git config --global -- user.name "$1"
 git config --global -- user.email "$2"
 if command -v jj >/dev/null 2>&1; then
   jj config set --user -- user.name "$3"
   jj config set --user -- user.email "$4"
 fi"#;
        run_identity_script(runner, paths, identity, script, GuestBoot::Temporary)?;
        if !verify_guest_identity(runner, paths, identity, GuestBoot::Temporary)? {
            return Err(RuntimeError::Malformed(format!(
                "Silo could not verify the saved Git identity for '{}'. Setup is not complete.",
                identity.computer
            )));
        }
    }
    Ok(())
}

/// Whether a guest identity command may boot a stopped computer for its duration.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GuestBoot {
    /// `exec` starts a stopped computer temporarily and stops it again.
    Temporary,
    /// Only a running computer is used (`exec --no-start`).
    Never,
}

fn run_identity_script(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    identity: &ComputerIdentity,
    script: &str,
    boot: GuestBoot,
) -> Result<CommandOutput, RuntimeError> {
    // exec starts stopped computers temporarily (unless `--no-start`) and preserves
    // already-running computers. Values are positional arguments, never interpolated shell source.
    let user = crate::working_account::USER;
    let mut args: Vec<String> = vec![
        "exec".into(),
        identity.computer.clone(),
        "--user".into(),
        user.into(),
        "--env".into(),
        format!("USER={user}"),
        "--env".into(),
        format!("LOGNAME={user}"),
        "--no-tty".into(),
        "--workdir".into(),
        "/".into(),
        "--quiet".into(),
        "--timeout".into(),
        "30s".into(),
    ];
    if boot == GuestBoot::Never {
        args.push("--no-start".into());
    }
    args.extend([
        "--".into(),
        "sh".into(),
        "-c".into(),
        script.into(),
        "silo-git-identity".into(),
        identity.name.clone(),
        identity.email.clone(),
        serde_json::to_string(&identity.name)
            .map_err(|_| RuntimeError::Invalid("Invalid Git name.".into()))?,
        serde_json::to_string(&identity.email)
            .map_err(|_| RuntimeError::Invalid("Invalid Git email.".into()))?,
    ]);
    runner.run(paths, &args, MUTATION_TIMEOUT)
}

fn verify_guest_identity(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    identity: &ComputerIdentity,
    boot: GuestBoot,
) -> Result<bool, RuntimeError> {
    let script = r#"set -eu
 if [ "$(git config --global --get user.name)" != "$1" ] ||
    [ "$(git config --global --get user.email)" != "$2" ]; then exit 0; fi
 if command -v jj >/dev/null 2>&1; then
   [ "$(jj config get user.name)" = "$1" ] || exit 0
   [ "$(jj config get user.email)" = "$2" ] || exit 0
 fi
 printf '%s' silo-identity-verified"#;
    Ok(run_identity_script(runner, paths, identity, script, boot)?
        .stdout
        .trim()
        == "silo-identity-verified")
}

/// GitHub tokens a computer's cached access profile still uses. Host-only retirement
/// material: the type is deliberately not `Serialize`, and `Debug` never prints a
/// token, so it cannot reach the frontend, a log or an error message by accident.
#[derive(Default)]
pub(crate) struct ScopedTokens(Vec<String>);

impl ScopedTokens {
    pub(crate) fn contains(&self, token: &str) -> bool {
        self.0.iter().any(|existing| existing == token)
    }
}

impl Extend<String> for ScopedTokens {
    fn extend<I: IntoIterator<Item = String>>(&mut self, tokens: I) {
        for token in tokens {
            if !self.contains(&token) {
                self.0.push(token);
            }
        }
    }
}

impl std::fmt::Debug for ScopedTokens {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "ScopedTokens([{} redacted])", self.0.len())
    }
}

/// Compile-time guard: this fails to build if `ScopedTokens` ever implements
/// `Serialize`, because the call below then matches two impls.
const _: fn() = || {
    trait AmbiguousIfSerialize<Marker> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfSerialize<()> for T {}
    impl<T: ?Sized + Serialize> AmbiguousIfSerialize<u8> for T {}
    <ScopedTokens as AmbiguousIfSerialize<_>>::check();
};

fn scoped_tokens_of(raw: &str) -> Result<ScopedTokens, String> {
    let profile: Value = serde_json::from_str(raw).map_err(|_| "Invalid cached GitHub state.")?;
    let mut tokens = ScopedTokens::default();
    for owner in profile["owners"]
        .as_array()
        .ok_or("Invalid cached GitHub grants.")?
    {
        tokens.extend(
            ["readToken", "writeToken"]
                .into_iter()
                .filter_map(|key| owner[key].as_str().map(str::to_owned)),
        );
    }
    Ok(tokens)
}

/// Host-only retirement material; see `ScopedTokens`.
pub(crate) fn scoped_cached_tokens(
    app: &AppHandle,
    computer: &str,
) -> Result<ScopedTokens, String> {
    let paths = runtime_paths(app)?;
    let cache = GITHUB_PROFILES.get_or_init(|| Mutex::new(HashMap::new()));
    let profiles = cache
        .lock()
        .map_err(|_| "GitHub runtime state is unavailable.")?;
    match profiles.get(&(paths.home, computer.into())) {
        Some(raw) => scoped_tokens_of(raw),
        None => Ok(ScopedTokens::default()),
    }
}

/// Check the attachment cache as well as the grant cache. Failed updates clear this cache.
pub(crate) fn github_policy_is_cached(
    app: &AppHandle,
    computer: &str,
    profile: &Value,
) -> Result<bool, String> {
    let paths = runtime_paths(app)?;
    let serialized = serde_json::to_string(profile).map_err(|_| "Invalid GitHub profile.")?;
    Ok(GITHUB_PROFILES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "GitHub runtime state is unavailable.")?
        .get(&(paths.home, computer.into()))
        == Some(&serialized))
}

/// A managed computer receives credentials through a host-only secret source reference.
/// The JSON profile reaches the runtime on standard input only: never as a command
/// argument, an environment variable, a config value or captured log.
pub(crate) fn apply_github_policy(
    app: &AppHandle,
    computer: &str,
    revision: u64,
    profiles: &Value,
) -> Result<(), String> {
    let paths = runtime_paths(app)?;
    apply_github_policy_with(&paths, computer, revision, profiles, MUTATION_TIMEOUT)
}

/// Apply one computer's GitHub access profile with `msb modify`.
///
/// The revision is accepted (or rejected as older) and the cached boot profile is
/// discarded at once, without waiting for any runtime command, so a failed or
/// superseded update never leaves a stale credential for the next start. The update
/// then waits its turn on the computer's operation gate, like every other change to that computer,
/// and runs through the shared runtime launcher (worker lock, runtime-home checks,
/// bounded wait). A newer accepted revision abandons an older update that is still
/// waiting, and the applied profile is cached only if no newer revision arrived.
fn apply_github_policy_with(
    paths: &RuntimePaths,
    computer: &str,
    revision: u64,
    profiles: &Value,
    timeout: Duration,
) -> Result<(), String> {
    validate_name(computer).map_err(|error| error.to_string())?;
    if !matches!(profiles["version"].as_u64(), Some(1 | 2)) || !profiles["owners"].is_array() {
        return Err("Invalid GitHub access profile.".into());
    }
    let token_protocol = profiles["version"] == 2;
    let profile =
        serde_json::to_string(profiles).map_err(|_| "Cannot prepare GitHub access.".to_string())?;
    if profile.len() > 128 * 1024 {
        return Err("GitHub access profile is too large.".into());
    }
    let computer_id = resolve_computer_id(paths, computer).map_err(|error| error.to_string())?;
    let access = computer_access_state(&paths.home, computer)?;
    let profile_key = (paths.home.clone(), computer.to_owned());
    let cache = GITHUB_PROFILES.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let mut current = access
            .revision
            .lock()
            .map_err(|_| "GitHub runtime state is unavailable.")?;
        accept_github_revision(&mut current, revision)?;
        cache
            .lock()
            .map_err(|_| "GitHub runtime state is unavailable.")?
            .remove(&profile_key);
    }
    let superseded = || {
        access
            .revision
            .lock()
            .map_or(true, |current| *current > revision)
    };
    let deadline = Instant::now() + timeout;
    let _guard = match OPERATIONS
        .kind(operation_gate::OperationKind::GithubApply)
        .acquire_while(
            operation_gate::Scope::Computer { id: computer_id.clone() },
            Some(computer.to_owned()),
            &format!("Applying GitHub access to {computer}"),
            &|| !superseded() && Instant::now() < deadline,
        ) {
        Ok(guard) => {
            guard.expect_within(timeout);
            Some(guard)
        }
        // A caller that already runs an operation keeps its own ordering.
        Err(operation_gate::GateError::Nested) => None,
        Err(operation_gate::GateError::Abandoned) if superseded() => {
            return Err(GITHUB_UPDATE_REPLACED.into())
        }
        Err(operation_gate::GateError::Abandoned) => {
            return Err(format!(
                "Another operation on {computer} is still running. Silo applies its GitHub access again shortly."
            ))
        }
        Err(operation_gate::GateError::Cancelled) => {
            return Err("Applying GitHub access was cancelled.".into())
        }
        Err(error) => return Err(error.to_string()),
    };
    if superseded() {
        return Err(GITHUB_UPDATE_REPLACED.into());
    }
    let configuration = read_metadata(&paths.metadata)
        .map_err(|error| error.to_string())?
        .computers
        .into_iter()
        .find(|configuration| configuration.id() == computer_id && configuration.name() == computer)
        .ok_or("The computer identity changed. No GitHub access was applied.")?;
    let capability = run_msb(
        paths,
        &[if token_protocol {
            "--silo-github-token-protocol".into()
        } else {
            "--silo-github-protocol".into()
        }],
        READ_TIMEOUT,
    )
    .map_err(|_| {
        "This Silo runtime must be updated before GitHub access can be enabled.".to_string()
    })?;
    if capability.stdout.trim() != "1" {
        return Err("This runtime does not support Silo GitHub permissions.".into());
    }
    let inspected =
        inspect_computer(&ProcessRunner, paths, computer).map_err(|error| error.to_string())?;
    ensure_computer_identity(&configuration, &inspected).map_err(|error| error.to_string())?;
    if inspected
        .config
        .pointer("/labels/silo.github-protocol")
        .and_then(Value::as_str)
        != Some("1")
    {
        return Err(
            "Recreate this development computer to enable the new GitHub integration.".into(),
        );
    }
    let general_secrets = crate::secrets::runtime_material(computer)?;
    secrets_runtime::validate_material(&general_secrets)?;
    let args: Vec<String> = [
        "modify",
        computer,
        "--secret",
        secrets_runtime::SILO_GITHUB_SECRET_SPEC,
        "--format",
        "json",
    ]
    .map(String::from)
    .into();
    let applied = {
        let _runtime = lock_computer_runtime(&access, timeout, "Applying GitHub access")
            .map_err(|error| github_update_error(error, token_protocol))?;
        if superseded() {
            return Err(GITHUB_UPDATE_REPLACED.into());
        }
        spawn_runtime(
            paths,
            RuntimeLaunch {
                args: &args,
                timeout,
                material: &general_secrets,
                github_profile: &profile,
                capture: false,
                report: &ignore_progress,
            },
        )
    };
    applied.map_err(|error| github_update_error(error, token_protocol))?;
    let current = access
        .revision
        .lock()
        .map_err(|_| "GitHub runtime state is unavailable.".to_string())?;
    if *current > revision {
        return Err(GITHUB_UPDATE_REPLACED.into());
    }
    cache
        .lock()
        .map_err(|_| "GitHub runtime state is unavailable.".to_string())?
        .insert(profile_key, profile);
    Ok(())
}

/// Fixed, actionable text for a failed GitHub access update. Runtime output is
/// discarded for these updates, so no detail can carry credential material.
fn github_update_error(error: RuntimeError, token_protocol: bool) -> String {
    match error {
        RuntimeError::Cancelled { .. } => "Applying GitHub access was cancelled.".into(),
        RuntimeError::TimedOut { .. } => {
            "Applying GitHub access timed out; it was not marked complete.".into()
        }
        RuntimeError::Busy => {
            "Another change to this computer is still running. Silo applies its GitHub access again shortly.".into()
        }
        RuntimeError::Failed { .. } if token_protocol => "The computer rejected the token update. If Silo was updated while this computer was running, restart the computer and retry.".into(),
        RuntimeError::Failed { .. } => {
            "The computer rejected the GitHub access update. Retry after checking its state.".into()
        }
        _ => "Could not apply GitHub access to the computer.".into(),
    }
}

pub(crate) fn apply_github_identity(
    app: &AppHandle,
    computer: &str,
    identity: &Value,
) -> Result<(), String> {
    // Applies GitHub identity inside one computer's guest only.
    let paths = runtime_paths(app)?;
    let computer_id = resolve_computer_id(&paths, computer).map_err(|error| error.to_string())?;
    let parsed: ComputerIdentity = serde_json::from_value(serde_json::json!({
        "computer": computer, "name": identity["name"], "email": identity["email"], "apply": identity["apply"]
    })).map_err(|_| "Invalid Git author configuration.".to_string())?;
    let base_label = format!("Applying GitHub access to {computer}");
    let acquire = |label: &str| -> Result<operation_gate::OperationGuard<'static>, RuntimeError> {
        OPERATIONS
            .kind(operation_gate::OperationKind::GithubApply)
            .computer(&computer_id, computer, label)
            .map_err(RuntimeError::from)
    };
    // Applying identity to a running guest can be cancelled; its child polling loops
    // observe the request through the current-operation token.
    let prepare = |guard: &operation_gate::OperationGuard<'static>| {
        guard.allow_cancel();
        guard.expect_within(Duration::from_secs(600));
    };
    let work = || -> Result<(), RuntimeError> {
        shutdown::ensure_accepting_operations().map_err(RuntimeError::Unavailable)?;
        configure_computer_identities_with(&ProcessRunner, &paths, std::slice::from_ref(&parsed))
    };
    // Writing Git identity re-runs the same guest command, so a timed-out or momentarily
    // unavailable runtime is retried; validation and verification failures are final.
    gated_auto_retry(&base_label, acquire, prepare, work).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_computer_configuration(
    app: AppHandle,
) -> Result<ComputerConfigurationRequest, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let paths = runtime_paths(&app)?;
        // Saved names and resources can render before live computer inspection finishes.
        // This command does not infer or return a running/stopped state.
        read_metadata(&paths.metadata).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn read_application_state(
    app: AppHandle,
    refresh_repositories: Option<bool>,
) -> Result<ApplicationSource, BridgeError> {
    crate::runtime_migration::ensure_ready_async(&app).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let paths = runtime_paths(&app)?;
        crate::secrets::schedule_revocations(&app);
        // Only visible device-wide work (computer configuration) can add or remove
        // computers; work on one computer settles only that computer's row, and hidden housekeeping
        // never affects the read.
        let mut source = read_application_snapshot(&ProcessRunner, &paths, &OPERATIONS)?;
        // Expired-log cleanup runs in the background, at most hourly per stopped computer, so a
        // state read never waits for it or re-inspects every stopped computer.
        let due = plan_log_cleanup(&paths, &mut source.computers, Instant::now());
        if !due.is_empty() {
            let cleanup_paths = paths.clone();
            thread::spawn(move || {
                clean_expired_logs(&ProcessRunner, &cleanup_paths, &OPERATIONS, &due)
            });
        }
        enrich_application_state(
            &app,
            &paths,
            &mut source,
            Repositories::Discover {
                refresh: refresh_repositories.unwrap_or(false),
            },
        );
        remember_settled(&paths, &source.computers);
        Ok(source)
    })
    .await
    .map_err(|_| BridgeError::from(internal_failure("reading computer state")))?
}

/// Expired logs of a stopped computer are cleaned at most this often, off the state-read path.
const LOG_CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Default)]
struct LogCleanup {
    /// When each logs directory was last cleaned.
    cleaned: HashMap<PathBuf, Instant>,
    /// Logs directories whose last cleanup failed; their rows carry a warning.
    failed: HashSet<PathBuf>,
    /// A background cleanup pass is in progress.
    running: bool,
}

static LOG_CLEANUP: OnceLock<Mutex<LogCleanup>> = OnceLock::new();

fn log_cleanup() -> std::sync::MutexGuard<'static, LogCleanup> {
    // Bookkeeping only: a panic while holding it at worst repeats a cleanup.
    LOG_CLEANUP
        .get_or_init(|| Mutex::new(LogCleanup::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn computer_logs(paths: &RuntimePaths, name: &str) -> PathBuf {
    paths.home.join("sandboxes").join(name).join("logs")
}

/// Flag rows whose last log cleanup failed, and return the fresh, stopped computers whose
/// logs are due for cleanup (at most hourly each). Returning names starts a pass, so
/// only one runs at a time; `clean_expired_logs` ends it.
fn plan_log_cleanup(
    paths: &RuntimePaths,
    computers: &mut [ApplicationComputer],
    now: Instant,
) -> Vec<String> {
    let mut state = log_cleanup();
    for computer in computers.iter_mut() {
        if computer.attention.is_none()
            && state
                .failed
                .contains(&computer_logs(paths, computer.configuration.name()))
        {
            computer.attention = Some(ComputerAttention {
                level: AttentionLevel::Warning,
                message: "Expired logs could not be cleaned up.".into(),
            });
        }
    }
    if state.running {
        return Vec::new();
    }
    let due: Vec<String> = computers
        .iter()
        .filter(|computer| {
            matches!(computer.state, ComputerState::Stopped)
                && !computer.settling
                && computer.freshness == Freshness::Fresh
                && state
                    .cleaned
                    .get(&computer_logs(paths, computer.configuration.name()))
                    .is_none_or(|at| now.saturating_duration_since(*at) >= LOG_CLEANUP_INTERVAL)
        })
        .map(|computer| computer.configuration.name().to_owned())
        .collect();
    state.running = !due.is_empty();
    due
}

/// One background cleanup pass. It runs only while nothing else holds the gate (hidden,
/// so it never shows in the queue), re-checks that each computer is still stopped, and records
/// the outcome; a busy gate postpones the pass to a later read.
fn clean_expired_logs(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    gate: &operation_gate::OperationGate,
    names: &[String],
) {
    if let Ok(_guard) = gate.try_device_hidden("Cleaning up expired logs") {
        for name in names {
            if !inspect_computer(runner, paths, name)
                .is_ok_and(|computer| runtime_logs::is_stopped(&computer.status))
            {
                continue;
            }
            let logs = computer_logs(paths, name);
            let result = crate::log_retention::enforce(&logs);
            let mut state = log_cleanup();
            state.cleaned.insert(logs.clone(), Instant::now());
            if result.is_err() {
                state.failed.insert(logs);
            } else {
                state.failed.remove(&logs);
            }
        }
    }
    log_cleanup().running = false;
}

/// Run `work` for `key` one caller at a time, so concurrent readers (for example two
/// windows refreshing together) share host_push's short-lived repository cache instead
/// of each scanning the same guest.
fn single_flight<T>(key: String, work: impl FnOnce() -> T) -> T {
    static TURNS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let turn = TURNS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entry(key)
        .or_default()
        .clone();
    let _turn = turn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    work()
}

/// How enrichment fills running computers' repositories.
#[derive(Clone, Copy)]
enum Repositories {
    /// Scan each fresh running guest (cached briefly unless `refresh`).
    Discover { refresh: bool },
    /// Keep each computer's last settled list; used by change responses.
    LastKnown,
}

/// Add what the runtime read does not carry: push operations, each fresh running computer's
/// repositories, and GitHub state. A settling or stale row keeps the repositories of its
/// last known reading instead of scanning a guest that is changing or unreadable.
fn enrich_application_state(
    app: &AppHandle,
    paths: &RuntimePaths,
    source: &mut ApplicationSource,
    repositories: Repositories,
) {
    source.repository_push_operations =
        crate::host_push_operations::merge(app, crate::host_push::operations())
            .unwrap_or_else(|_| crate::host_push::operations());
    match repositories {
        Repositories::LastKnown => keep_last_known_repositories(paths, &mut source.computers),
        Repositories::Discover { refresh } => {
            for computer in &mut source.computers {
                if matches!(computer.state, ComputerState::Running)
                    && !computer.settling
                    && computer.freshness == Freshness::Fresh
                {
                    let name = computer.configuration.name();
                    let key = format!("{}:{}", paths.home.display(), computer.configuration.id());
                    match single_flight(key, || {
                        crate::host_push::discover(
                            paths,
                            name,
                            computer.configuration.id(),
                            refresh,
                        )
                    }) {
                        Ok(repositories) => computer.repositories = repositories,
                        Err(message) => {
                            if computer.attention.is_none() {
                                computer.attention = Some(ComputerAttention {
                                    level: AttentionLevel::Warning,
                                    message,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    source.github = crate::github::snapshot(app).unwrap_or_else(|message| serde_json::json!({
        "state": "disconnected", "accessEnabled": false, "repositoryCatalog": [],
        "repositoryCatalogStatus": {"status": "unavailable", "message": message, "canRetry": true},
        "computerOperations": [], "deviceIdentity": crate::device_identity::cached(|| {}),
    }));
}

/// The controller UI can remain usable when local computer inspection fails. This is
/// never used for a remote owner's snapshot and never invents local computer states.
#[tauri::command]
pub async fn read_application_shell(
    app: AppHandle,
    error: String,
) -> Result<ApplicationSource, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let paths = runtime_paths(&app)?;
        let mut source = application_shell(&paths, &error)?;
        source.github = crate::github::snapshot(&app).unwrap_or_else(|message| serde_json::json!({
            "state": "disconnected", "accessEnabled": false, "repositoryCatalog": [],
            "repositoryCatalogStatus": {"status": "unavailable", "message": message, "canRetry": true},
            "computerOperations": [], "deviceIdentity": crate::device_identity::cached(|| {}),
        }));
        Ok(source)
    }).await.map_err(|error| error.to_string())?
}

fn application_shell(paths: &RuntimePaths, error: &str) -> Result<ApplicationSource, String> {
    let mut source =
        application_source_for_computers(paths, Vec::new()).map_err(|error| error.to_string())?;
    source.runtime_repair = Some(serde_json::json!({
        "status": "unavailable", "reason": error.chars().take(1024).collect::<String>(),
        "recovery": "Check this device’s runtime and retry. Connected devices remain available."
    }));
    Ok(source)
}

/// The user-facing state read. Visible device-wide work (it can add or remove
/// computers) defers the whole read with `update_in_progress`; work on one computer only
/// settles that computer's row (`settle_rows`), and hidden housekeeping never affects it.
fn read_application_snapshot(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    gate: &operation_gate::OperationGate,
) -> Result<ApplicationSource, BridgeError> {
    const MAX_ATTEMPTS: usize = 2;
    const RETRY_DELAY: Duration = Duration::from_millis(100);
    for attempt in 0..MAX_ATTEMPTS {
        // Configuration recovery is durable work in progress. Keep its immediate
        // code distinct from transient lock contention below.
        if configuration_recovery::pending(paths)? {
            return Err(BridgeError::updating());
        }
        match read_application_snapshot_once(runner, paths, gate) {
            Ok(source) => return Ok(source),
            Err(error) if error.code == ErrorCode::UpdateInProgress => {
                if configuration_recovery::pending(paths)? {
                    return Err(BridgeError::updating());
                }
                if attempt + 1 < MAX_ATTEMPTS {
                    thread::sleep(RETRY_DELAY);
                } else {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("at least one application snapshot attempt is required")
}

fn read_application_snapshot_once(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    gate: &operation_gate::OperationGate,
) -> Result<ApplicationSource, BridgeError> {
    // Runtime creation and metadata publication are separate steps. Discard
    // observations overlapping a mutation, without blocking progress updates.
    if configuration_recovery::pending(paths)? {
        return Err(BridgeError::updating());
    }
    let check_idle = || -> Result<(), BridgeError> {
        if gate.is_device_idle() {
            Ok(())
        } else {
            Err(BridgeError::updating())
        }
    };
    check_idle()?;
    let started = gate.generations();
    let before = fs::read(&paths.metadata).ok();
    let rows = read_rows(runner, paths);
    check_idle()?;
    if before != fs::read(&paths.metadata).ok() {
        return Err(BridgeError::updating());
    }
    // Decided after the read: a computer is settled only if nothing touched it meanwhile.
    let settled = |id: &str| gate.is_computer_quiet(id) && gate.generation(id) == started.of(id);
    rows.and_then(|rows| settle_rows(paths, rows, &settled))
        .and_then(|computers| application_source_for_computers(paths, computers))
        .map_err(BridgeError::from)
}

/// Longest a single health-check runtime call may take.
const HEALTH_CALL_BUDGET: Duration = Duration::from_secs(5);

/// Runs health-check runtime calls with a budget per call rather than one budget shared
/// by the list and every inspection: with many computers a shared budget ran out before the
/// last computers were inspected. A slow computer now fails only its own (stale, skipped) row.
struct HealthRunner<'a> {
    inner: &'a dyn RuntimeRunner,
    budget: Duration,
}

impl RuntimeRunner for HealthRunner<'_> {
    fn run(
        &self,
        paths: &RuntimePaths,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        if !paths.home.is_dir()
            || paths
                .storage_home
                .as_ref()
                .is_some_and(|home| !home.is_dir())
        {
            return Err(RuntimeError::Unavailable(
                "The managed runtime is unavailable.".into(),
            ));
        }
        self.inner.run(paths, args, timeout.min(self.budget))
    }
}

/// A background health observation uses the same real inspection as the UI, without
/// host identity discovery. Never hold the mutation lock while inspecting: user
/// actions take priority. Each computer reports whether it stayed idle and untouched by any
/// operation during the read, so the caller skips busy computers without discarding the
/// rest; a metadata change during the read discards the whole reading. No computer is
/// created, started, or changed here.
pub(crate) fn health_observations(app: &AppHandle) -> crate::health_watch::Reading {
    use crate::health_watch::{ComputerReading, Reading};
    // An unfinished storage migration holds the runtime back: there is nothing to
    // observe, and that is not a health problem to report.
    if crate::runtime_migration::blocks_operations(app) {
        return Reading::Discarded;
    }
    let paths = runtime_paths(app);
    let before = paths
        .as_ref()
        .ok()
        .and_then(|paths| fs::read(&paths.metadata).ok());
    // Generations at the start: an unchanged generation and an idle computer afterwards mean
    // no operation touched the computer during the read.
    let started = OPERATIONS.generations();
    let source = paths.as_ref().map_err(Clone::clone).and_then(|paths| {
        read_application_state_with(
            &HealthRunner {
                inner: &ProcessRunner,
                budget: HEALTH_CALL_BUDGET,
            },
            paths,
        )
        .map_err(|error| error.to_string())
    });
    let after = paths
        .as_ref()
        .ok()
        .and_then(|paths| fs::read(&paths.metadata).ok());
    if before != after {
        return Reading::Discarded;
    }
    let Ok(source) = source else {
        return Reading::Unavailable;
    };
    Reading::Computers(
        source
            .computers
            .into_iter()
            .map(|computer| {
                let id = computer.configuration.id().to_owned();
                let state = if computer.attention.is_some() {
                    "Health or configuration check failed"
                } else {
                    match computer.state {
                        ComputerState::Running => "Running",
                        ComputerState::Stopped => "Stopped",
                        ComputerState::Starting => "Starting",
                        ComputerState::Failed => "Failed",
                    }
                };
                let generation = started.of(&id);
                ComputerReading {
                    // A computer whose own inspection failed says nothing about its health.
                    settled: computer.freshness == Freshness::Fresh
                        && OPERATIONS.is_computer_idle(&id)
                        && OPERATIONS.generation(&id) == generation,
                    generation,
                    name: computer.configuration.name().into(),
                    state,
                    id,
                }
            })
            .collect(),
    )
}

/// User-facing label for a lifecycle action on one computer.
fn lifecycle_label(action: &str, name: &str) -> String {
    match action {
        "start" => format!("Starting {name}"),
        "stop" => format!("Stopping {name}"),
        "restart" => format!("Restarting {name}"),
        "dismiss-error" => format!("Dismissing error for {name}"),
        _ => format!("Updating {name}"),
    }
}

/// Total attempts (one initial try plus these many retries) and the wait before each retry.
const AUTO_RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(2), Duration::from_secs(5)];

/// Runtime failures that are worth retrying automatically: a timed-out child, or a
/// runtime that could not be spawned at that moment. Deliberately excludes `Busy`,
/// `Cancelled`, and validation/configuration errors, which retrying cannot fix.
pub(crate) fn transient_runtime_error(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::TimedOut { .. } | RuntimeError::Launch(_)
    )
}

/// Run a gated operation with automatic retries for transient failures.
///
/// The gate is re-acquired for every attempt and released between attempts (the guard is
/// dropped before sleeping), so other queued work can run while this one backs off. Only
/// idempotent callers should use this. `label` receives the zero-based attempt index and
/// returns the queue label; retries append "(attempt N of M)". `on_progress` is called
/// before each retry so the caller can surface progress.
pub(crate) fn gated_auto_retry<T>(
    base_label: &str,
    acquire: impl Fn(&str) -> Result<operation_gate::OperationGuard<'static>, RuntimeError>,
    prepare: impl Fn(&operation_gate::OperationGuard<'static>),
    work: impl Fn() -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    gated_auto_retry_with(&AUTO_RETRY_DELAYS, base_label, acquire, prepare, work)
}

fn gated_auto_retry_with<T>(
    delays: &[Duration],
    base_label: &str,
    acquire: impl Fn(&str) -> Result<operation_gate::OperationGuard<'static>, RuntimeError>,
    prepare: impl Fn(&operation_gate::OperationGuard<'static>),
    work: impl Fn() -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    gated_auto_retry_classified(
        delays,
        base_label,
        acquire,
        prepare,
        work,
        transient_runtime_error,
        || RuntimeError::Cancelled {
            operation: base_label.to_owned(),
        },
    )
}

/// The retry engine behind `gated_auto_retry`, generic over the error type so callers
/// whose inner work does not speak `RuntimeError` (for example the secrets path, which
/// keeps a typed `secrets_runtime::Attempt` up to this boundary) can classify their own
/// transient failures. The gate is re-acquired per attempt and the guard is dropped
/// before any backoff sleep, so other queued work runs between attempts. Only genuinely
/// transient failures (`is_transient` returns true) are retried, up to `delays.len()`
/// extra attempts; every other outcome, including gate errors from `acquire`, is final.
///
/// A single cancel token spans the whole sequence: it is adopted into every attempt's
/// guard (`adopt_cancel_token`), so a cancel issued against one attempt's queue id carries
/// over to the remaining attempts and is honored before the next attempt runs. The token
/// is checked before re-acquiring and during the backoff (which sleeps in short slices);
/// when set, the sequence stops and returns `cancelled()`.
///
/// Limitation: between attempts the guard is dropped, so the retry has no queue entry while
/// it backs off. The gate can only flip the shared token while a guard adopting it is
/// running, so a cancel requested purely during the backoff window is observed at the next
/// attempt's acquisition rather than mid-sleep; a cancel during an attempt's own run carries
/// over immediately. Surfacing a non-blocking "retrying soon" queue entry would remove this
/// gap but requires a new gate scope excluded from conflict checks; not done here.
fn gated_auto_retry_classified<T, E>(
    delays: &[Duration],
    base_label: &str,
    acquire: impl Fn(&str) -> Result<operation_gate::OperationGuard<'static>, E>,
    prepare: impl Fn(&operation_gate::OperationGuard<'static>),
    work: impl Fn() -> Result<T, E>,
    is_transient: impl Fn(&E) -> bool,
    cancelled: impl Fn() -> E,
) -> Result<T, E> {
    let total = delays.len() + 1;
    // One shared cancel token for the whole retry sequence, created before the first attempt.
    let token = Arc::new(AtomicBool::new(false));
    // The first attempt's start time, so every attempt reports one continuous
    // operation in the queue and slow-operation flagging can fire (D-27).
    let mut first_since = None;
    // A Quit or update that began after this request stopped the computers; even if it failed
    // and admission reopened, a later attempt must not undo that (D-30).
    let quit = shutdown::generation();
    let mut attempt = 0usize;
    loop {
        // A cancel from a previous attempt (or during its backoff) stops the sequence before
        // re-acquiring the gate.
        if token.load(Ordering::SeqCst) || shutdown::generation() != quit {
            return Err(cancelled());
        }
        let label = if attempt == 0 {
            base_label.to_owned()
        } else {
            format!("{base_label} (attempt {} of {total})", attempt + 1)
        };
        let outcome = {
            let mut guard = acquire(&label)?;
            if shutdown::generation() != quit {
                return Err(cancelled());
            }
            match first_since {
                Some(since) => guard.continue_since(since),
                None => first_since = Some(guard.since()),
            }
            // Share the sequence-wide token so a cancel against this attempt is observed by
            // the work (through the current-operation token) and carries to later attempts.
            guard.adopt_cancel_token(token.clone());
            prepare(&guard);
            work()
            // guard dropped here, releasing the gate before any backoff sleep.
        };
        match outcome {
            Ok(value) => return Ok(value),
            Err(error) => {
                // A cancel requested during the attempt takes priority over the work's own
                // error classification: never retry a cancelled operation.
                if token.load(Ordering::SeqCst) {
                    return Err(cancelled());
                }
                if attempt < delays.len() && is_transient(&error) {
                    // Sleep in short slices so a cancel is observed promptly and the sequence
                    // stops instead of running a further attempt.
                    let mut remaining = delays[attempt];
                    let slice = Duration::from_millis(100);
                    while !remaining.is_zero() {
                        if token.load(Ordering::SeqCst) || shutdown::generation() != quit {
                            return Err(cancelled());
                        }
                        let step = remaining.min(slice);
                        thread::sleep(step);
                        remaining -= step;
                    }
                    attempt += 1;
                    continue;
                }
                return Err(error);
            }
        }
    }
}

#[tauri::command]
pub async fn computer_action(
    app: AppHandle,
    action: String,
    name: String,
    path: Option<String>,
) -> Result<ApplicationSource, BridgeError> {
    crate::runtime_migration::ensure_ready_async(&app).await?;
    if matches!(action.as_str(), "open-editor" | "open-terminal") {
        shutdown::ensure_accepting_operations()?;
        return tauri::async_runtime::spawn_blocking(move || {
            if action == "open-terminal" {
                crate::terminal::open(&app, &name)?;
            } else {
                crate::editor::open(&app, &name, path.as_deref())?;
            }
            let paths = runtime_paths(&app)?;
            application_state_response(&app, &paths).map_err(BridgeError::from)
        })
        .await
        .map_err(|_| BridgeError::from("The application launcher failed."))?;
    }
    let worker_app = app.clone();
    // Elapsed time counts from the command, including any wait for the operation gate: it
    // is what the user experienced, and decides whether a success is worth a notice.
    let started = std::time::Instant::now();
    let notice_action = action.clone();
    let notice_name = name.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<(String, ApplicationSource, bool), (Option<String>, LifecycleFailure, BridgeError)> {
        let app = worker_app;
        // Setup failures before the operation runs are genuine faults worth notifying about.
        let paths = runtime_paths(&app).map_err(|error| (None, LifecycleFailure::Failed, error.into()))?;
        // Start/stop/restart change only this computer's runtime; resource admission is
        // against device totals, not other computers, so per-computer ordering is sufficient. The
        // key collapses double-clicked lifecycle requests into one queued action.
        // Resolve the stable id before acquiring so ordering survives a rename.
        let computer_id = resolve_computer_id(&paths, &name).map_err(|error| (None, LifecycleFailure::Failed, error.into()))?;
        let base_label = lifecycle_label(&action, &name);
        let key = format!("computer:{computer_id}:{action}");
        // Start/restart may be cancelled while running; stop may not. Expected durations
        // flag slow operations in the UI (no auto-kill).
        let expected = match action.as_str() {
            "start" | "restart" => Duration::from_secs(180),
            "stop" => Duration::from_secs(120),
            _ => Duration::from_secs(600),
        };
        let allow_cancel = matches!(action.as_str(), "start" | "restart");
        let last_request = std::cell::Cell::new(None);
        let acquire = |label: &str| -> Result<operation_gate::OperationGuard<'static>, RuntimeError> {
            let guard = OPERATIONS
                .kind(operation_gate::OperationKind::Lifecycle)
                .retry_after(last_request.get())
                .acquire(
                    operation_gate::Scope::Computer { id: computer_id.clone() },
                    Some(name.clone()),
                    label,
                    Some(key.clone()),
                )
                .map_err(RuntimeError::from)?;
            last_request.set(Some(guard.request_id()));
            Ok(guard)
        };
        let prepare = |guard: &operation_gate::OperationGuard<'static>| {
            if allow_cancel {
                guard.allow_cancel();
            }
            guard.expect_within(expected);
        };
        let work = || -> Result<(), RuntimeError> {
            shutdown::ensure_accepting_operations().map_err(RuntimeError::Unavailable)?;
            // Re-read metadata once this attempt's turn arrives and re-derive the display
            // name by its stable id: a rename or removal may have landed while waiting.
            let metadata = read_metadata(&paths.metadata)?;
            let configuration = metadata
                .computers
                .iter()
                .find(|configuration| configuration.id() == computer_id)
                .ok_or_else(|| RuntimeError::Invalid("This computer no longer exists.".into()))?;
            // No state event while the gate is held: the queue event already shows the
            // action, and this computer's row keeps its last state until the post-release event
            // below (D-18), so an in-gate refresh in every window would be wasted.
            let resources = device_resources()?;
            // The gate is held, so no state event follows until it is released. Progress goes
            // out as its own event, and the first step after the boot refreshes the state so
            // the row leaves "stopped" as soon as the computer is up.
            let sink: LifecycleSink = {
                let (app, computer_id, action) = (app.clone(), computer_id.clone(), action.clone());
                std::rc::Rc::new(move |step| {
                    let _ = app.emit(
                        "silo://lifecycle-progress",
                        json!({"computerId": computer_id, "action": action, "step": step.id()}),
                    );
                    if step == LifecycleStep::Network {
                        let _ = app.emit("silo://application-state-changed", ());
                    }
                })
            };
            with_lifecycle_sink(sink, || {
                explicit_computer_action_with(&ProcessRunner, &paths, &resources, &action, configuration.name())
            })
        };
        // Start/stop/restart are idempotent, so transient failures retry automatically.
        // Other lifecycle actions run once.
        let result = if matches!(action.as_str(), "start" | "stop" | "restart") {
            gated_auto_retry(&base_label, acquire, prepare, work)
        } else {
            match acquire(&base_label) {
                Ok(guard) => {
                    prepare(&guard);
                    let outcome = work();
                    drop(guard);
                    outcome
                }
                Err(error) => Err(error),
            }
        };
        let _ = app.emit("silo://application-state-changed", ());
        let (result, handed_off) = hand_off_duplicate(result);
        let result = result.and_then(|_| application_state_response(&app, &paths));
        // Classify before the typed error is flattened to a message: a cancellation or a
        // deduplicated request is an expected outcome, not a failure to notify about.
        match result {
            Ok(state) => Ok((computer_id, state, handed_off)),
            Err(error) => Err((
                Some(computer_id),
                lifecycle_failure(&error),
                BridgeError::from(error),
            )),
        }
    }).await.map_err(|_| BridgeError::from(internal_failure("running the computer action")))?;
    let elapsed = started.elapsed();
    let notify = |computer_id: Option<String>, outcome: crate::notifications::Outcome<'_>| {
        let computer = computer_id.map(|id| crate::notifications::NoticeComputer {
            id,
            name: notice_name.clone(),
        });
        if let Some(notice) = crate::notifications::lifecycle_notice(
            &notice_action,
            &notice_name,
            computer,
            elapsed,
            outcome,
        ) {
            crate::notifications::notify_native(&app, notice);
        }
    };
    match result {
        Ok((computer_id, state, handed_off)) => {
            notify(
                Some(computer_id),
                if handed_off {
                    crate::notifications::Outcome::AlreadyQueued
                } else {
                    crate::notifications::Outcome::Succeeded
                },
            );
            Ok(state)
        }
        Err((computer_id, failure, message)) => {
            notify(computer_id, failure.outcome(&message.message));
            Err(message)
        }
    }
}

/// How a failed lifecycle action reads to the notification router.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LifecycleFailure {
    Cancelled,
    AlreadyQueued,
    Failed,
}

impl LifecycleFailure {
    fn outcome(self, message: &str) -> crate::notifications::Outcome<'_> {
        match self {
            Self::Cancelled => crate::notifications::Outcome::Cancelled,
            Self::AlreadyQueued => crate::notifications::Outcome::AlreadyQueued,
            Self::Failed => crate::notifications::Outcome::Failed(message),
        }
    }
}

/// A request deduplicated into an identical queued action (a double-click, or an
/// auto-retry finding the same request already waiting) was handed off, not failed:
/// the caller returns current state without an error row or notification (D-13).
fn hand_off_duplicate(result: Result<(), RuntimeError>) -> (Result<(), RuntimeError>, bool) {
    match result {
        Err(error) if lifecycle_failure(&error) == LifecycleFailure::AlreadyQueued => {
            (Ok(()), true)
        }
        other => (other, false),
    }
}

/// A user cancellation is an expected outcome and a duplicate request handed to the one
/// already queued is not a failure (D-13); neither warrants an alert.
fn lifecycle_failure(error: &RuntimeError) -> LifecycleFailure {
    match error {
        RuntimeError::Cancelled { .. } => LifecycleFailure::Cancelled,
        RuntimeError::Admission(operation_gate::GateError::AlreadyQueued) => {
            LifecycleFailure::AlreadyQueued
        }
        _ => LifecycleFailure::Failed,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerConfigurationProgress {
    schema_version: u8,
    #[serde(rename = "type")]
    event_type: String,
    request_id: String,
    phase: String,
    step: String,
    computer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fraction: Option<u8>,
    message: String,
    safe_for_display: bool,
    timestamp: u64,
    level: String,
    elapsed_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    downloaded_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    /// For `setup-failed` only: the runtime's own explanation, filtered like Logs and
    /// bounded, for a Details disclosure. Never part of `message`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diagnostic: Option<String>,
    /// Some changes in this setup completed before the failure.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    partial: bool,
}

fn computer_progress(
    request_id: &str,
    step: &str,
    computer: &str,
    fraction: u8,
) -> ComputerConfigurationProgress {
    let message = match (step, fraction) {
        ("computer-configuration", 0) => format!("Configuring {computer}…"),
        ("desktop-installation", _) => "Installing the Linux desktop.".into(),
        ("computer-use-setup", _) => "Setting up the desktop and computer use.".into(),
        ("computer-use-pending", _) => "Computer use will finish setting up at first start.".into(),
        ("computer-image-wait", _) => "Waiting for the VM image…".into(),
        ("chatgpt-app-wait", _) => "Waiting for ChatGPT for Linux…".into(),
        ("chatgpt-app-download", _) => "Downloading ChatGPT for Linux…".into(),
        ("computer-configuration", _) => format!("{computer} configured."),
        ("computer-verification", 0) => format!("Verifying {computer}…"),
        ("computer-verification", _) => format!("{computer} verified."),
        ("computer-image-preparation", _) => "Preparing the VM image…".into(),
        ("computer-image-import", _) => {
            "Importing the VM image (first time only, about a minute)…".into()
        }
        ("computer-disk-preparation", _) => format!("Preparing {computer}'s workspace disk…"),
        ("computer-runtime-preparation", _) => {
            format!("Preparing {computer}'s VM image and system disk…")
        }
        ("computer-settings", _) => format!("Saving {computer}'s configuration…"),
        ("setup-started", _) => "Computer setup started.".into(),
        ("setup-completed", _) => "Computer setup completed.".into(),
        ("setup-failed", _) => "Computer setup failed.".into(),
        ("setup-interrupted", _) => {
            "Computer setup was interrupted when Silo closed. Check the computer state, then retry."
                .into()
        }
        ("computer-removal", 0) => format!("Removing {computer}…"),
        _ => format!("{computer} removed."),
    };
    ComputerConfigurationProgress {
        schema_version: 1,
        event_type: "progress".into(),
        request_id: request_id.into(),
        phase: "computers".into(),
        step: step.into(),
        computer: computer.into(),
        fraction: (!step.starts_with("setup-")
            && matches!(
                step,
                "desktop-installation"
                    | "computer-configuration"
                    | "computer-verification"
                    | "computer-removal"
            ))
        .then_some(fraction),
        message,
        safe_for_display: true,
        timestamp: activity_timestamp(),
        level: "info".into(),
        elapsed_seconds: 0,
        downloaded_bytes: None,
        total_bytes: None,
        failure_code: None,
        exit_code: None,
        diagnostic: None,
        partial: false,
    }
}

/// Shows what a creation is waiting for before it takes the operation gate. These events
/// are for the creation toast only; they are not part of the setup journal.
pub(crate) fn publish_creation_wait(
    app: &AppHandle,
    request_id: &str,
    step: &crate::creation_inputs::Step,
) {
    use crate::creation_inputs::{ChatGpt, Step};
    let event = match step {
        Step::Image => computer_progress(request_id, "computer-image-wait", "", 0),
        Step::ChatGpt(ChatGpt::Downloading { received, total }) => {
            let mut event = computer_progress(request_id, "chatgpt-app-download", "", 0);
            event.downloaded_bytes = Some(*received);
            event.total_bytes = Some(*total).filter(|total| *total > 0);
            event
        }
        Step::ChatGpt(ChatGpt::Failed { reason }) => {
            let mut event = computer_progress(request_id, "chatgpt-app-failed", "", 0);
            event.level = "warning".into();
            event.message = reason.chars().take(300).collect();
            event
        }
        Step::ChatGpt(_) => computer_progress(request_id, "chatgpt-app-wait", "", 0),
    };
    let _ = app.emit_to("main", "silo://computer-configuration-progress", &event);
}

/// The computers a change would create, for what creation has to wait for before the gate.
fn creation_needs(
    paths: &RuntimePaths,
    mut request: ComputerConfigurationRequest,
) -> crate::creation_inputs::Needs {
    let Ok(previous) = read_metadata(&paths.metadata) else {
        return Default::default();
    };
    apply_desktop_defaults(&previous, &mut request);
    let mut needs = crate::creation_inputs::Needs::default();
    for configuration in request.computers.iter() {
        if previous
            .computers
            .iter()
            .any(|old| old.id() == configuration.id())
        {
            continue;
        }
        needs.computers.push(configuration.id().to_owned());
        if crate::computer_use::is_built_in(configuration) {
            needs.computer_use.push(configuration.id().to_owned());
        }
    }
    needs
}

/// "Finish without computer use" from the creation toast while it waits for ChatGPT for Linux.
#[tauri::command]
pub fn skip_computer_use_wait(request_id: String) {
    crate::creation_inputs::skip(&request_id);
}

fn activity_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn activity_path(paths: &RuntimePaths) -> PathBuf {
    paths.metadata.with_file_name("setup-activity.json")
}

/// The one classification of runtime failures (D-17). Its category decides both the
/// setup activity code and the reason shown to the user, so the two cannot disagree.
fn failure_category(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    if lower.contains("unauthorized")
        || lower.contains("authentication")
        || mentions_http_status(&lower, "401")
    {
        "auth"
    } else if lower.contains("forbidden")
        || lower.contains("denied access")
        || mentions_http_status(&lower, "403")
    {
        "access"
    } else if lower.contains("no space left") || lower.contains("free disk space") {
        "disk"
    } else if lower.contains("permission denied") || lower.contains("permission was denied") {
        "permission"
    } else if lower.contains("connection")
        || lower.contains("dns")
        || lower.contains("error sending request")
        || lower.contains("could not be reached")
    {
        "network"
    } else if lower.contains("timeout") || lower.contains("timed out") {
        "timeout"
    } else if lower.contains("digest")
        || lower.contains("checksum")
        || lower.contains("integrity check")
    {
        "integrity"
    } else if lower.contains("cpu")
        || lower.contains("memory")
        || lower.contains("storage allocation")
        || lower.contains("resource")
    {
        "resources"
    } else {
        "runtime"
    }
}

/// What happened and what to do next, for one failure category.
fn failure_reason(code: &str) -> &'static str {
    match code {
        "auth" => "The image registry rejected authentication. Check registry access and retry.",
        "access" => "The image registry denied access. Check registry access and retry.",
        "disk" => "Not enough free disk space. Free some space and retry.",
        "permission" => "Permission was denied. Check access to Silo's storage and retry.",
        "network" => "The image registry could not be reached. Check your internet connection and retry.",
        "timeout" => "The operation timed out. Check the computer state and retry.",
        "integrity" => "The downloaded image failed its integrity check. Retry the download.",
        "resources" => "Computer CPU, memory, or storage limits could not be validated. Review the computer resources against this device's limits and retry.",
        "configuration" => "The computer configuration could not be applied or verified. Review its settings and current state before retrying.",
        "unavailable" => "A required runtime or host resource is unavailable. Quit and reopen Silo to check it again, then retry.",
        _ => "The runtime did not complete the operation. Check the computer state and retry.",
    }
}

/// The persisted setup-failure message for a category. Unknown categories are rejected
/// so a modified history cannot inject text.
fn setup_failure_message(code: &str) -> Option<String> {
    matches!(
        code,
        "auth"
            | "access"
            | "disk"
            | "permission"
            | "network"
            | "timeout"
            | "integrity"
            | "resources"
            | "configuration"
            | "unavailable"
            | "runtime"
    )
    .then(|| format!("Computer setup failed: {}", failure_reason(code)))
}

/// Longest diagnostic kept for one failure, in characters.
const MAX_DIAGNOSTIC_CHARS: usize = 8_192;

/// Runtime output prepared for a Details disclosure: sensitive lines hidden like Logs,
/// control sequences removed, and bounded.
fn diagnostic_text(text: &str) -> Option<String> {
    let filtered = runtime_activity::log_text(text);
    let filtered = filtered.trim();
    if filtered.is_empty() {
        return None;
    }
    let bounded: String = filtered.chars().take(MAX_DIAGNOSTIC_CHARS).collect();
    Some(if bounded.len() < filtered.len() {
        format!("{bounded}\n[Diagnostic truncated]")
    } else {
        bounded
    })
}

/// A diagnostic read back from Silo's own history: filtered again and bounded, since
/// the file could have been modified.
fn stored_diagnostic(text: &str) -> Option<String> {
    let filtered = runtime_activity::log_text(text);
    let filtered = filtered.trim();
    (!filtered.is_empty()).then(|| filtered.chars().take(MAX_DIAGNOSTIC_CHARS + 64).collect())
}

/// A failure as the user sees it (D-39): a stable category, one line saying what
/// happened and what to do next (never raw runtime output or an exit code), and the
/// runtime's own explanation separately, for a Details disclosure.
pub(crate) struct FailureReport {
    pub(crate) code: &'static str,
    pub(crate) summary: String,
    pub(crate) exit_code: Option<i32>,
    pub(crate) diagnostic: Option<String>,
    pub(crate) partial: bool,
}

pub(crate) fn failure_report(error: &RuntimeError) -> FailureReport {
    match error {
        RuntimeError::Failed {
            exit_code, detail, ..
        } => {
            let mut diagnostic = exit_code
                .map(|code| format!("Exit code {code}"))
                .unwrap_or_default();
            if let Some(text) = diagnostic_text(detail) {
                if !diagnostic.is_empty() {
                    diagnostic.push('\n');
                }
                diagnostic.push_str(&text);
            }
            FailureReport {
                code: failure_category(detail),
                summary: error.to_string(),
                exit_code: *exit_code,
                diagnostic: (!diagnostic.is_empty()).then_some(diagnostic),
                partial: false,
            }
        }
        RuntimeError::Partial(inner) => {
            let inner = failure_report(inner);
            FailureReport {
                summary: format!("{} {PARTIAL_CHANGES_KEPT}", inner.summary),
                partial: true,
                ..inner
            }
        }
        RuntimeError::Admission(_)
        | RuntimeError::Busy
        | RuntimeError::TimedOut { .. }
        | RuntimeError::Cancelled { .. } => {
            let summary = error.to_string();
            let code = match (failure_category(&summary), error) {
                (_, RuntimeError::Busy) => "configuration",
                (code, _) => code,
            };
            FailureReport {
                code,
                summary,
                exit_code: None,
                diagnostic: None,
                partial: false,
            }
        }
        RuntimeError::Launch(message) => FailureReport {
            code: "unavailable",
            summary: error.to_string(),
            exit_code: None,
            diagnostic: diagnostic_text(message),
            partial: false,
        },
        RuntimeError::Invalid(message)
        | RuntimeError::Malformed(message)
        | RuntimeError::Unavailable(message) => {
            let code = match (failure_category(message), error) {
                ("runtime", RuntimeError::Invalid(_) | RuntimeError::Malformed(_)) => {
                    "configuration"
                }
                ("runtime", _) => "unavailable",
                (code, _) => code,
            };
            // Silo writes these messages, but they may name OS paths or process details.
            let summary = message
                .split_whitespace()
                .map(|word| {
                    if word.contains('/')
                        || word.contains('@')
                        || word.to_lowercase().contains("token")
                        || word.contains('=')
                    {
                        "[redacted]"
                    } else {
                        word
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(800)
                .collect();
            FailureReport {
                code,
                summary,
                exit_code: None,
                diagnostic: None,
                partial: false,
            }
        }
    }
}

/// Setup activity journals being written in this process. While one is live, the saved
/// history is current: an unfinished last attempt is running, not interrupted.
static LIVE_SETUP_JOURNALS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Serializes writes of the setup activity file with the interruption check, so a
/// journal starting meanwhile is never overwritten by a stale "interrupted" marker.
static SETUP_ACTIVITY_FILE: Mutex<()> = Mutex::new(());

fn setup_activity_file() -> std::sync::MutexGuard<'static, ()> {
    SETUP_ACTIVITY_FILE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Held by a live journal; see `LIVE_SETUP_JOURNALS`.
struct LiveSetupJournal(());

impl LiveSetupJournal {
    fn new() -> Self {
        let _file = setup_activity_file();
        LIVE_SETUP_JOURNALS.fetch_add(1, Ordering::SeqCst);
        Self(())
    }
}

impl Drop for LiveSetupJournal {
    fn drop(&mut self) {
        LIVE_SETUP_JOURNALS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Repeated progress within one stage (download bytes, heartbeats) is saved at most this
/// often; stage boundaries and outcomes are saved immediately.
const ACTIVITY_PERSIST_INTERVAL: Duration = Duration::from_secs(2);
const MAX_ACTIVITY_EVENTS: usize = 512;

fn persist_activity(path: &Path, events: &impl Serialize) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Silo's activity storage path is invalid.")?;
    fs::create_dir_all(parent).map_err(|_| "Silo could not prepare setup activity storage.")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "Silo could not save setup activity.")?;
    serde_json::to_writer(&mut file, events)
        .map_err(|_| "Silo could not encode setup activity.")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "Silo could not save setup activity.")?;
    file.persist(path)
        .map_err(|_| "Silo could not save setup activity.")?;
    Ok(())
}

/// The one-line, user-facing text for a runtime error. See `failure_report`.
pub(crate) fn safe_activity_error(error: &RuntimeError) -> String {
    failure_report(error).summary
}

/// A background task of a command ended unexpectedly (a panic). The message says what
/// Silo was doing and what to try, without naming internals.
fn internal_failure(activity: &str) -> String {
    format!("Silo ran into an internal error while {activity}. Retry; if it keeps happening, quit and reopen Silo.")
}

struct ActivityJournal {
    path: PathBuf,
    events: std::collections::VecDeque<ComputerConfigurationProgress>,
    started: Instant,
    /// When the history was last written, and whether events arrived since.
    persisted_at: Option<Instant>,
    unsaved: bool,
    _live: LiveSetupJournal,
}

impl ActivityJournal {
    fn start(paths: &RuntimePaths, _request_id: &str) -> Result<Self, String> {
        let journal = Self {
            path: activity_path(paths),
            events: std::collections::VecDeque::new(),
            started: Instant::now(),
            persisted_at: None,
            unsaved: false,
            _live: LiveSetupJournal::new(),
        };
        // Failure to retain diagnostics must not prevent the requested setup.
        // The first append publishes a visible warning if storage is unavailable.
        Ok(journal)
    }

    fn persist(&mut self) -> Result<(), String> {
        let _file = setup_activity_file();
        self.persisted_at = Some(Instant::now());
        self.unsaved = false;
        persist_activity(&self.path, &self.events)
    }

    fn push(&mut self, event: ComputerConfigurationProgress) {
        // Keep the first event (the attempt's start) and drop the oldest progress after it.
        if self.events.len() >= MAX_ACTIVITY_EVENTS {
            self.events.remove(1);
        }
        self.events.push_back(event);
    }

    fn append(
        &mut self,
        mut event: ComputerConfigurationProgress,
    ) -> ComputerConfigurationProgress {
        event.elapsed_seconds = self.started.elapsed().as_secs();
        // Progress updates replace the previous update for the same stage, retaining boundaries.
        let repeated = self.events.back().is_some_and(|last| {
            last.step == event.step
                && last.computer == event.computer
                && last.fraction.is_none()
                && !event.step.starts_with("setup-")
        });
        if repeated {
            self.events.pop_back();
        }
        self.push(event.clone());
        self.unsaved = true;
        // A repeated update within one stage is saved at most every few seconds (and when
        // the journal ends); a new stage, a boundary or an outcome is saved immediately.
        if repeated
            && self
                .persisted_at
                .is_some_and(|at| at.elapsed() < ACTIVITY_PERSIST_INTERVAL)
        {
            return event;
        }
        if self.persist().is_err() {
            let mut warning = event.clone();
            warning.level = "warning".into();
            warning.message = "Setup continues, but Silo could not retain its activity history. Copy the activity before closing Silo.".into();
            warning.step = "activity-storage-warning".into();
            warning.fraction = None;
            self.push(warning);
        }
        event
    }
}

impl Drop for ActivityJournal {
    fn drop(&mut self) {
        // Save a throttled progress update the attempt ended on.
        if self.unsaved {
            let _ = self.persist();
        }
    }
}

fn read_activity(
    paths: &RuntimePaths,
    recover_interrupted: bool,
) -> Result<Vec<ComputerConfigurationProgress>, String> {
    let path = activity_path(paths);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut bytes = Vec::new();
    File::open(&path)
        .map_err(|_| "Silo could not read its setup activity history.")?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Silo could not read its setup activity history.")?;
    if bytes.len() > 1024 * 1024 {
        return Err("Silo's setup activity history is too large to read.".into());
    }
    let mut events: Vec<ComputerConfigurationProgress> =
        serde_json::from_slice(&bytes).map_err(|_| "Silo's setup activity history is damaged.")?;
    if events.len() > 512
        || events.iter().any(|event| {
            event.schema_version != 1
                || event.event_type != "progress"
                || event.phase != "computers"
                || !event.safe_for_display
                || event.request_id.is_empty()
                || event.request_id.len() > 256
                || event.computer.len() > 128
                || !event
                    .computer
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || "-_".contains(ch))
                || !matches!(event.level.as_str(), "info" | "warning" | "error")
                || event.fraction.is_some_and(|value| value > 1)
        })
    {
        return Err("Silo's setup activity history is invalid.".into());
    }
    for event in &mut events {
        event.message = match event.step.as_str() {
            "desktop-installation" | "computer-use-setup" | "computer-use-pending" | "computer-configuration" | "computer-verification" | "computer-removal" | "computer-disk-preparation" | "computer-image-preparation" | "computer-image-import" | "computer-runtime-preparation" | "computer-settings" | "setup-started" | "setup-completed" | "setup-interrupted" => computer_progress(&event.request_id, &event.step, &event.computer, event.fraction.unwrap_or(0)).message,
            "image-resolving" => format!("{}: Resolving the VM image…", event.computer),
            "image-resolved" => format!("{}: VM image resolved; preparing the download…", event.computer),
            "image-download" => format!("{}: Downloading the VM image…", event.computer),
            "image-downloaded" => format!("{}: VM image layer downloaded.", event.computer),
            "image-verifying" => format!("{}: Checking the downloaded image…", event.computer),
            "image-preparing" => format!("{}: Preparing the VM image on disk…", event.computer),
            "image-ready" => format!("{}: VM image ready; preparing the system disk and runtime configuration…", event.computer),
            "runtime-waiting" => format!("{}: Waiting for the runtime to finish preparing the computer…", event.computer),
            "host-memory-warning" => "Silo could not measure host memory. Setup can continue, but available memory could not be checked.".into(),
            "activity-storage-warning" => "Silo could not retain its activity history. Copy the activity before closing Silo.".into(),
            "setup-failed" => setup_failure_message(event.failure_code.as_deref().unwrap_or("runtime")).ok_or("Silo's setup activity history contains an unknown failure.")?,
            _ => return Err("Silo's setup activity history contains an unknown operation.".into()),
        };
        event.partial &= event.step == "setup-failed";
        if event.partial {
            event.message.push(' ');
            event.message.push_str(PARTIAL_CHANGES_KEPT);
        }
        // A stored diagnostic is filtered again, like the message is re-derived above.
        event.diagnostic = event
            .diagnostic
            .take()
            .filter(|_| event.step == "setup-failed")
            .and_then(|text| stored_diagnostic(&text));
    }
    if recover_interrupted
        && events
            .iter()
            .rev()
            .find(|event| event.step != "activity-storage-warning")
            .is_some_and(|event| {
                !matches!(
                    event.step.as_str(),
                    "setup-completed" | "setup-failed" | "setup-interrupted"
                )
            })
    {
        let last = events.last().unwrap();
        let mut interrupted =
            computer_progress(&last.request_id, "setup-interrupted", &last.computer, 0);
        interrupted.level = "warning".into();
        interrupted.elapsed_seconds = last.elapsed_seconds;
        if events.len() >= 512 {
            events.remove(1);
        }
        events.push(interrupted);
        persist_activity(&path, &events)?;
    }
    Ok(events)
}

fn pending_verification_computer(events: &[ComputerConfigurationProgress]) -> Option<String> {
    let last = events
        .iter()
        .rev()
        .find(|event| event.step != "activity-storage-warning")?;
    if last.step == "setup-completed" {
        return None;
    }
    events
        .iter()
        .rev()
        .find(|event| event.step == "computer-verification" && event.request_id == last.request_id)
        .filter(|event| event.fraction == Some(0))
        .map(|event| event.computer.clone())
}

/// Reading may rewrite (and fsync) the journal, so it runs off the main thread.
#[tauri::command]
pub async fn read_setup_activity(
    app: AppHandle,
) -> Result<Vec<ComputerConfigurationProgress>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let paths = runtime_paths(&app)?;
        read_setup_activity_at(&paths)
    })
    .await
    .map_err(|_| internal_failure("reading setup activity"))?
}

/// An unfinished last attempt was interrupted only when no setup journal is being
/// written in this process. Unrelated work (launch auto-start, hidden housekeeping, other
/// computers) does not make an interrupted setup look in progress.
fn read_setup_activity_at(
    paths: &RuntimePaths,
) -> Result<Vec<ComputerConfigurationProgress>, String> {
    let _file = setup_activity_file();
    read_activity(paths, LIVE_SETUP_JOURNALS.load(Ordering::SeqCst) == 0)
}

fn normalize_request_id(request_id: Option<String>) -> Result<String, String> {
    let request_id = request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if request_id.trim().is_empty() || request_id.len() > 256 {
        return Err("Invalid computer configuration request ID.".into());
    }
    Ok(request_id)
}

/// Validate, save and verify a finalized configuration with the shared journal,
/// progress events and completion outcome. The caller holds the operation gate and
/// supplies the exact `request` to persist (a whole-list save or a targeted change
/// already applied to fresh metadata). Preserves `request_id`, progress events,
/// `retry_computer` semantics and pending-verification resumption.
fn apply_configuration_with_progress(
    app: &AppHandle,
    paths: &RuntimePaths,
    request: ComputerConfigurationRequest,
    request_id: &str,
    retry_computer: Option<String>,
) -> Result<ApplicationSource, String> {
    debug_assert!(
        operation_gate::held(),
        "configuration changes require the operation gate"
    );
    let resources = device_resources().map_err(|e| e.to_string())?;
    let mut request = request;
    validate_request(&request).map_err(|e| e.to_string())?;
    validate_requested_resources(&request, &resources).map_err(|e| e.to_string())?;
    let previous = read_metadata(&paths.metadata).map_err(|e| e.to_string())?;
    apply_desktop_defaults(&previous, &mut request);
    configuration_recovery::prepare_retry(&ProcessRunner, paths, Some(&request))
        .map_err(|e| e.to_string())?;
    let retry_computer = retry_computer.or_else(|| {
        // A no-change retry after relaunch resumes the failed verification only.
        // A fresh add, edit or removal must not replay another computer's work.
        if read_metadata(&paths.metadata).ok().as_ref() != Some(&request) {
            return None;
        }
        pending_verification_computer(&read_activity(paths, false).unwrap_or_default()).filter(
            |name| {
                request
                    .computers
                    .iter()
                    .any(|configuration| configuration.name() == name)
            },
        )
    });
    let journal = Mutex::new(ActivityJournal::start(paths, request_id)?);
    let publish = |event: ComputerConfigurationProgress| {
        let mut journal = journal.lock().unwrap_or_else(|error| error.into_inner());
        let event = journal.append(event);
        if let Some(warning) = journal
            .events
            .back()
            .filter(|entry| entry.step == "activity-storage-warning")
        {
            let _ = app.emit_to("main", "silo://computer-configuration-progress", warning);
        }
        let _ = app.emit_to("main", "silo://computer-configuration-progress", &event);
    };
    publish(computer_progress(request_id, "setup-started", "", 0));
    let progress = |step: &str, computer: &str, fraction: u8| {
        publish(computer_progress(request_id, step, computer, fraction));
    };
    let result = Ok(resources)
        .and_then(|resources| {
            if resources.physical_memory_bytes.is_none() {
                let mut warning = computer_progress(request_id, "host-memory-warning", "", 0);
                warning.level = "warning".into();
                warning.message = "Silo could not measure host memory. Setup can continue, but available memory could not be checked.".into();
                publish(warning);
            }
            validate_request(&request)?;
            validate_requested_resources(&request, &resources)?;
            apply_whole_configuration_with_progress(
                &SetupRunner {
                    request_id,
                    publish: &publish,
                },
                paths,
                &resources,
                request,
                retry_computer.as_deref(),
                &progress,
            )
        });
    let mut outcome = computer_progress(
        request_id,
        if result.is_ok() {
            "setup-completed"
        } else {
            "setup-failed"
        },
        "",
        0,
    );
    if let Err(error) = &result {
        outcome.level = "error".into();
        let report = failure_report(error);
        outcome.failure_code = Some(report.code.into());
        outcome.exit_code = report.exit_code;
        outcome.diagnostic = report.diagnostic;
        outcome.partial = report.partial;
        let last = journal
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .events
            .back()
            .cloned();
        if let Some(last) = last {
            outcome.computer = last.computer;
        }
        outcome.message = format!("Computer setup failed: {}", report.summary);
    }
    publish(outcome);
    result.map_err(|error| safe_activity_error(&error))?;
    // The setup's outcome is decided above; refreshing the list is separate (D-11).
    application_state_response(app, paths).map_err(|error| safe_activity_error(&error))
}

/// Resume a failed computer setup or verification without the UI resending a whole list.
/// The interrupted attempt's target configuration is recorded in the configuration
/// recovery journal; this re-applies it against current state (idempotent for computers
/// already completed) and resumes the pending verification. When no attempt is pending,
/// it re-verifies the committed inventory. Concurrent changes made since the attempt are
/// rejected by the recovery reconciliation ("Computer settings changed since the
/// interruption."), so a resume never overwrites work done meanwhile.
#[tauri::command]
pub async fn retry_computer_configuration(
    app: AppHandle,
    request_id: Option<String>,
    retry_computer: Option<String>,
) -> Result<ApplicationSource, String> {
    crate::runtime_migration::ensure_ready_async(&app).await?;
    let request_id = normalize_request_id(request_id)?;
    let notify_app = app.clone();
    let failure_title = retry_computer.as_deref().map_or_else(
        || "Couldn\u{2019}t finish computer setup".to_string(),
        |name| format!("Couldn\u{2019}t finish setting up {name}"),
    );
    let result = tauri::async_runtime::spawn_blocking(move || {
        let paths = runtime_paths(&app)?;
        // Waited for before the gate, as in `change_computer_configuration`.
        let needs = configuration_recovery::pending_request(&paths)
            .ok()
            .flatten()
            .or_else(|| read_metadata(&paths.metadata).ok())
            .map(|request| creation_needs(&paths, request))
            .unwrap_or_default();
        crate::creation_inputs::wait_before_gate(&app, &paths, &request_id, &needs)?;
        // Changes the shared computer inventory/metadata; device-wide.
        let _guard = OPERATIONS
            .kind(operation_gate::OperationKind::ComputerConfiguration)
            .device("Retrying computer settings")
            .map_err(|e| e.to_string())?;
        shutdown::ensure_accepting_operations()?;
        // Resume the configuration the interrupted attempt recorded. With no pending
        // attempt, re-verify the committed inventory (an idempotent no-op on success).
        let request =
            match configuration_recovery::pending_request(&paths).map_err(|e| e.to_string())? {
                Some(request) => request,
                None => read_metadata(&paths.metadata).map_err(|e| e.to_string())?,
            };
        apply_configuration_with_progress(&app, &paths, request, &request_id, retry_computer)
    })
    .await
    .map_err(|_| internal_failure("changing computer settings"))
    .and_then(|result| result);
    if let Err(message) = &result {
        notify_configuration_failure(&notify_app, &failure_title, message);
    }
    result
}

/// The setup UI shows the failure in place; the system notice matters when Silo is in the
/// background. A rejected stale edit is shown inline by the editor and is not a failure.
fn notify_configuration_failure(app: &AppHandle, title: &str, message: &str) {
    if message.contains("changed while your edit was waiting") {
        return;
    }
    crate::notifications::notify_native(
        app,
        crate::notifications::failure("computer-setup", title, message, None),
    );
}

/// Apply one targeted change (create, edit, delete or reorder) against the current
/// inventory. Because the request may wait its turn on the operation gate, the change
/// is applied to fresh metadata read under the gate and rejected if the targeted computer
/// changed while the edit was waiting, rather than overwriting with a stale list.
#[tauri::command]
pub async fn change_computer_configuration(
    app: AppHandle,
    change: ComputerConfigurationChange,
    request_id: Option<String>,
    retry_computer: Option<String>,
) -> Result<ApplicationSource, String> {
    crate::runtime_migration::ensure_ready_async(&app).await?;
    let request_id = normalize_request_id(request_id)?;
    let notify_app = app.clone();
    let failure_title = change.failure_title();
    let deleted = change.deleted_ids();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let paths = runtime_paths(&app)?;
        // The image import and the ChatGPT download are waited for before the gate: holding
        // the device-wide gate for minutes would stall every lifecycle operation and Quit.
        let needs = read_metadata(&paths.metadata)
            .ok()
            .and_then(|mut request| {
                change.apply(&mut request.computers).ok()?;
                Some(creation_needs(&paths, request))
            })
            .unwrap_or_default();
        crate::creation_inputs::wait_before_gate(&app, &paths, &request_id, &needs)?;
        // Changes the shared computer inventory/metadata; device-wide.
        let _guard = OPERATIONS
            .removing(&change.deleted_ids(), &change.label())
            .map_err(|e| e.to_string())?;
        shutdown::ensure_accepting_operations()?;
        // Read fresh, then apply the specific change so a queued edit lands on the
        // latest inventory instead of overwriting concurrent work.
        let mut request = read_metadata(&paths.metadata).map_err(|e| e.to_string())?;
        change.apply(&mut request.computers)?;
        apply_configuration_with_progress(&app, &paths, request, &request_id, retry_computer)
    })
    .await
    .map_err(|_| internal_failure("changing computer settings"))
    .and_then(|result| result);
    match &result {
        Err(message) => notify_configuration_failure(&notify_app, &failure_title, message),
        // A deleted computer has nothing left to open: withdraw its delivered notices.
        Ok(_) => deleted
            .iter()
            .for_each(|id| crate::notifications::clear_computer(&notify_app, id)),
    }
    result
}

/// Read every computer with no operation-overlap policy: each computer's reading is taken as
/// settled. A computer whose own inspection fails is returned stale (its last known state with
/// the reason) instead of failing every computer. Health checks and tests use this.
fn read_application_state_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
) -> Result<ApplicationSource, RuntimeError> {
    let computers = settle_rows(paths, read_rows(runner, paths)?, &|_| true)?;
    application_source_for_computers(paths, computers)
}

/// One configured computer as read from the runtime, before settling.
struct Row {
    /// The fresh reading, or a placeholder when `unread` is set.
    computer: ApplicationComputer,
    unread: Option<Unread>,
}

/// Why a configured computer has no fresh reading.
enum Unread {
    /// The runtime list disagrees with Silo's records for this computer (absent although
    /// expected, or present while a pending restore expects none).
    Mismatch,
    /// Its own inspection failed.
    Failed(RuntimeError),
}

const INVENTORY_MISMATCH: &str = "Silo's saved computer configuration does not match its managed runtime state. No computer operation was performed.";

/// List and inspect every configured computer. Only a failure that no single computer
/// explains (metadata, the runtime list, or a managed runtime computer Silo does not know)
/// fails the whole read; a computer's own failure is carried in its row for `settle_rows`.
fn read_rows(runner: &dyn RuntimeRunner, paths: &RuntimePaths) -> Result<Vec<Row>, RuntimeError> {
    let metadata = read_metadata(&paths.metadata)?;
    let listed = if metadata.computers.is_empty() {
        Vec::new()
    } else {
        list_managed(runner, paths)?
    };
    let listed_names: HashSet<&str> = listed.iter().map(|entry| entry.name.as_str()).collect();
    let configured: HashSet<&str> = metadata
        .computers
        .iter()
        .map(ComputerConfiguration::name)
        .collect();
    if listed_names.iter().any(|name| !configured.contains(name)) {
        return Err(RuntimeError::Malformed(INVENTORY_MISMATCH.into()));
    }
    // Whether each computer is read from Silo's pending-restore record rather than the runtime.
    // An unreadable record (damaged, or written by a newer Silo) degrades only its own
    // computer: the runtime decides, and its row is flagged when checkpoints are loaded.
    let from_record = |configuration: &ComputerConfiguration| {
        let listed = listed_names.contains(configuration.name());
        checkpoints::pending_view(paths, configuration.id(), listed).unwrap_or(!listed)
    };
    let mut rows = Vec::with_capacity(metadata.computers.len());
    for configuration in metadata.computers.iter().cloned() {
        let listed = listed_names.contains(configuration.name());
        let row = match (from_record(&configuration), listed) {
            (true, false) => Row {
                computer: checkpoints::pending_computer(configuration)?,
                unread: None,
            },
            (true, true) | (false, false) => Row {
                computer: unread_computer(configuration),
                unread: Some(Unread::Mismatch),
            },
            (false, true) => {
                match inspect_computer(runner, paths, configuration.name()).and_then(|inspected| {
                    ensure_managed(&inspected)?;
                    Ok(inspected)
                }) {
                    Ok(inspected) => Row {
                        computer: application_computer(paths, configuration, &inspected),
                        unread: None,
                    },
                    Err(error) => Row {
                        computer: unread_computer(configuration),
                        unread: Some(Unread::Failed(error)),
                    },
                }
            }
        };
        rows.push(row);
    }
    Ok(rows)
}

/// Publish rows. `settled(id)` is true when no operation touched the computer during the read.
///
/// - A touched computer keeps its last settled reading, marked `settling` (per the D-02 owner
///   answer, the row keeps its last known state next to its operation label); with no
///   earlier reading it shows this read's value, or a neutral placeholder.
/// - An untouched computer whose own inspection failed keeps its last known state, marked
///   `stale` with the reason, instead of failing every computer.
/// - An inventory mismatch on an untouched computer fails the whole read, as before.
fn settle_rows(
    paths: &RuntimePaths,
    rows: Vec<Row>,
    settled: &dyn Fn(&str) -> bool,
) -> Result<Vec<ApplicationComputer>, RuntimeError> {
    let last = last_settled(paths);
    rows.into_iter()
        .map(
            |Row {
                 mut computer,
                 unread,
             }| {
                let previous = last.get(computer.configuration.id());
                if !settled(computer.configuration.id()) {
                    match (previous, &unread) {
                        (Some(previous), _) => keep_runtime_fields(&mut computer, previous),
                        (None, Some(_)) => {
                            computer.state = ComputerState::Starting;
                            computer.state_detail = "Updating".into();
                        }
                        (None, None) => {}
                    }
                    computer.settling = true;
                    return Ok(computer);
                }
                match unread {
                    None => Ok(computer),
                    Some(Unread::Mismatch) => {
                        Err(RuntimeError::Malformed(INVENTORY_MISMATCH.into()))
                    }
                    Some(Unread::Failed(error)) => {
                        if let Some(previous) = previous {
                            keep_runtime_fields(&mut computer, previous);
                        }
                        computer.freshness = Freshness::Stale;
                        computer.attention = Some(ComputerAttention {
                            level: AttentionLevel::Warning,
                            message: format!(
                                "Silo could not refresh this computer's state. {}",
                                safe_activity_error(&error)
                            ),
                        });
                        Ok(computer)
                    }
                }
            },
        )
        .collect()
}

/// Copy the runtime-derived fields of an earlier reading; Silo's records stay current.
fn keep_runtime_fields(computer: &mut ApplicationComputer, previous: &ApplicationComputer) {
    computer.state = previous.state;
    computer.state_detail = previous.state_detail.clone();
    computer.attention = previous.attention.clone();
    // The cache keeps runtime state while a computer settles. Revocation warnings are
    // derived from today's journal, so do not copy yesterday's warning into it.
    if !previous.pending_secret_revocations.is_empty() {
        let warning = secret_revocation_warning(&previous.pending_secret_revocations);
        if let Some(attention) = &mut computer.attention {
            if attention.message == warning {
                computer.attention = None;
            } else if let Some(message) = attention.message.strip_suffix(&format!(" {warning}")) {
                attention.message = message.into();
            }
        }
    }
    computer.can_dismiss_error = previous.can_dismiss_error;
    computer.repositories = previous.repositories.clone();
}

/// Each computer's last settled, fresh reading (after enrichment), keyed by the inventory it
/// belongs to and the computer's stable id. Returned in place of readings that overlap work.
type SettledReadings = HashMap<(PathBuf, String), ApplicationComputer>;
static LAST_SETTLED: OnceLock<Mutex<SettledReadings>> = OnceLock::new();

fn settled_readings() -> std::sync::MutexGuard<'static, SettledReadings> {
    // A cache of readings: a panic while holding it cannot leave a reading half-written.
    LAST_SETTLED
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn last_settled(paths: &RuntimePaths) -> HashMap<String, ApplicationComputer> {
    settled_readings()
        .iter()
        .filter(|((metadata, _), _)| *metadata == paths.metadata)
        .map(|((_, id), computer)| (id.clone(), computer.clone()))
        .collect()
}

/// Remember the fresh, settled computer rows of a published source, and forget removed computers.
fn remember_settled(paths: &RuntimePaths, computers: &[ApplicationComputer]) {
    let present: HashSet<&str> = computers
        .iter()
        .map(|computer| computer.configuration.id())
        .collect();
    let mut readings = settled_readings();
    readings
        .retain(|(metadata, id), _| *metadata != paths.metadata || present.contains(id.as_str()));
    for computer in computers {
        if !computer.settling && computer.freshness == Freshness::Fresh {
            readings.insert(
                (
                    paths.metadata.clone(),
                    computer.configuration.id().to_owned(),
                ),
                computer.clone(),
            );
        }
    }
}

/// The state a change returns once it has succeeded. It is enriched like a normal read
/// (GitHub state, push operations, repositories) so publishing it does not blank those
/// panels (D-08), and a failed refresh never turns the finished change into an error
/// (D-11): see `state_after_change`.
pub(crate) fn application_state_response(
    app: &AppHandle,
    paths: &RuntimePaths,
) -> Result<ApplicationSource, RuntimeError> {
    let mut source = state_after_change(&ProcessRunner, paths, &OPERATIONS)?;
    enrich_application_state(app, paths, &mut source, Repositories::LastKnown);
    remember_settled(paths, &source.computers);
    Ok(source)
}

/// State returned by a change that already succeeded. Its outcome and the refresh are
/// separate: when the follow-up read fails, every computer keeps its last known state, marked
/// stale with the reason, instead of the finished change being reported as failed.
/// Work on other computers settles their rows as in a normal read; the caller's own operation
/// (if it still holds the gate) does not.
fn state_after_change(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    gate: &operation_gate::OperationGate,
) -> Result<ApplicationSource, RuntimeError> {
    let started = gate.generations();
    let settled = |id: &str| gate.is_computer_quiet(id) && gate.generation(id) == started.of(id);
    let computers =
        match read_rows(runner, paths).and_then(|rows| settle_rows(paths, rows, &settled)) {
            Ok(computers) => computers,
            Err(error) => last_known_computers(paths, &error)?,
        };
    application_source_for_computers(paths, computers)
}

fn last_known_computers(
    paths: &RuntimePaths,
    error: &RuntimeError,
) -> Result<Vec<ApplicationComputer>, RuntimeError> {
    let last = last_settled(paths);
    let message = format!(
        "The change finished, but Silo could not refresh computer states. {}",
        safe_activity_error(error)
    );
    Ok(read_metadata(&paths.metadata)?
        .computers
        .into_iter()
        .map(|configuration| {
            let mut computer = unread_computer(configuration);
            if let Some(previous) = last.get(computer.configuration.id()) {
                keep_runtime_fields(&mut computer, previous);
            }
            computer.freshness = Freshness::Stale;
            computer.attention = Some(ComputerAttention {
                level: AttentionLevel::Warning,
                message: message.clone(),
            });
            computer
        })
        .collect())
}

/// A change's response does not scan guests: each fresh running computer shows the
/// repositories of its last settled reading until the next full read refreshes them.
fn keep_last_known_repositories(paths: &RuntimePaths, computers: &mut [ApplicationComputer]) {
    let last = last_settled(paths);
    for computer in computers {
        if matches!(computer.state, ComputerState::Running) && computer.repositories.is_empty() {
            if let Some(previous) = last.get(computer.configuration.id()) {
                computer.repositories = previous.repositories.clone();
            }
        }
    }
}

/// A computer row with no reading yet; its runtime fields are replaced when settled.
fn unread_computer(configuration: ComputerConfiguration) -> ApplicationComputer {
    ApplicationComputer {
        configuration,
        purpose: "Local MicroSandbox".into(),
        state: ComputerState::Failed,
        state_detail: "State unavailable".into(),
        can_dismiss_error: false,
        lifecycle_failure: None,
        attention: None,
        freshness: Freshness::Fresh,
        settling: false,
        repositories: Vec::new(),
        files: Vec::new(),
        ports: Vec::new(),
        logs: Vec::new(),
        github_repositories: Vec::new(),
        secret_names: Vec::new(),
        pending_secret_revocations: Vec::new(),
        checkpoints: Vec::new(),
        pending_checkpoint_restore: None,
        checkpoint_operation: None,
        unfinished_restore: None,
    }
}

/// A persisted running checkpoint operation is interrupted only when no operation
/// currently holds its computer; otherwise it is live and keeps its running stage.
fn checkpoint_operation_view(
    mut operation: checkpoints::Operation,
    live: bool,
) -> checkpoints::Operation {
    if operation.status == "running" && !live {
        operation.status = "failed".into();
        operation.stage = "Interrupted operation".into();
        operation.error = Some(
            "Silo closed during this operation. Retry to reconcile its saved checkpoint.".into(),
        );
    }
    operation
}

fn secret_revocation_warning(names: &[String]) -> String {
    format!(
        "May still have access to {} until it restarts.",
        names.join(", ")
    )
}

fn application_source_for_computers(
    paths: &RuntimePaths,
    mut computers: Vec<ApplicationComputer>,
) -> Result<ApplicationSource, RuntimeError> {
    let secrets = crate::secrets::snapshot().map_err(RuntimeError::Unavailable)?;
    // Journal read failures are reported as an Activity warning by read() below.
    let mut failures = runtime_activity::failures(paths).unwrap_or_default();
    for computer in &mut computers {
        computer.lifecycle_failure = failures.remove(computer.configuration.id());
        match checkpoints::load(paths, computer.configuration.id()) {
            Ok(checkpoint) => {
                computer.pending_checkpoint_restore =
                    checkpoints::view_pending(&checkpoint, computer.configuration.name());
                computer.unfinished_restore = checkpoints::view_unfinished_restore(&checkpoint);
                computer.checkpoints = checkpoint.checkpoints;
                let live = !OPERATIONS.is_computer_idle(computer.configuration.id());
                computer.checkpoint_operation = checkpoint
                    .checkpoint_operation
                    .map(|operation| checkpoint_operation_view(operation, live));
            }
            // One unreadable record must not fail every computer: flag only this one.
            Err(error) => {
                computer.checkpoints = Vec::new();
                computer.pending_checkpoint_restore = None;
                computer.checkpoint_operation = None;
                computer.unfinished_restore = None;
                computer.attention = Some(ComputerAttention {
                        level: AttentionLevel::Error,
                        message: format!("{error} Checkpoints and actions that need them are unavailable for this computer."),
                    });
            }
        }
        computer.pending_secret_revocations =
            crate::secrets::pending_names(computer.configuration.name())
                .map_err(RuntimeError::Unavailable)?;
        if !computer.pending_secret_revocations.is_empty() {
            let warning = secret_revocation_warning(&computer.pending_secret_revocations);
            if let Some(attention) = &mut computer.attention {
                attention.message.push(' ');
                attention.message.push_str(&warning);
            } else {
                computer.attention = Some(ComputerAttention {
                    level: AttentionLevel::Warning,
                    message: warning,
                });
            }
        }
        computer.secret_names = secrets
            .iter()
            .filter(|secret| {
                secret["removing"] != true
                    && secret["computers"].as_array().is_some_and(|names| {
                        names
                            .iter()
                            .any(|name| name.as_str() == Some(computer.configuration.name()))
                    })
            })
            .filter_map(|secret| secret["name"].as_str().map(str::to_owned))
            .collect();
    }
    let mut activities = runtime_activity::read(paths)?;
    activities.extend(crate::secrets::activities().map_err(RuntimeError::Unavailable)?);
    activities.sort_by(|a, b| b["occurredAt"].as_str().cmp(&a["occurredAt"].as_str()));
    activities.truncate(200);
    Ok(ApplicationSource {
        runtime_repair: None,
        computers,
        activities,
        computer_configuration_operation: None,
        repository_push_operations: Vec::new(),
        github: serde_json::json!({"state": "disconnected"}),
        secrets,
        device_capacity: device_resources()
            .ok()
            .as_ref()
            .and_then(DeviceCapacity::of),
    })
}

fn list_managed(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
) -> Result<Vec<ListedSandbox>, RuntimeError> {
    let output = runner.run(
        paths,
        &[
            "list".into(),
            "--label".into(),
            MANAGED_LABEL.into(),
            "--format".into(),
            "json".into(),
        ],
        READ_TIMEOUT,
    )?;
    serde_json::from_str(&output.stdout).map_err(|_| {
        RuntimeError::Malformed("The bundled runtime returned an invalid computer list.".into())
    })
}

pub(crate) fn inspect_computer(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
) -> Result<InspectedSandbox, RuntimeError> {
    validate_name(name)?;
    let output = runner.run(
        paths,
        &[
            "inspect".into(),
            name.into(),
            "--format".into(),
            "json".into(),
        ],
        READ_TIMEOUT,
    )?;
    serde_json::from_str(&output.stdout).map_err(|_| {
        RuntimeError::Malformed(format!(
            "The bundled runtime returned invalid state for computer '{name}'."
        ))
    })
}

/// Silo's runtime patch makes `inspect` name the instance of a running computer. Computer-use
/// setup and storage reclaim trust only that identity, so a runtime without it would silently
/// disable both. The patch always writes the entry (null while no active run matches), so a
/// running computer whose output has no entry at all comes from a runtime that lacks the
/// capability: that is an explicit error here, and one diagnostic line per runtime.
static INSTANCE_ID_WARNED: OnceLock<Mutex<std::collections::HashSet<PathBuf>>> = OnceLock::new();

/// The running instance a runtime reports for `inspected`: `None` when the computer is not
/// running or its instance is not established yet.
pub(crate) fn running_instance_id(
    paths: &RuntimePaths,
    inspected: &InspectedSandbox,
) -> Result<Option<String>, RuntimeError> {
    if let Some(instance) = inspected
        .runtime_instance_id
        .as_ref()
        .filter(|instance| !instance.is_empty())
    {
        return Ok(Some(instance.clone()));
    }
    if inspected.runtime_instance_reported || !inspected.status.eq_ignore_ascii_case("running") {
        return Ok(None);
    }
    let message = "The bundled runtime does not report running instances (its inspect output has no runtime_instance_id), so computer-use setup and storage reclaim are disabled. Reinstall Silo.";
    let first = INSTANCE_ID_WARNED
        .get_or_init(|| Mutex::new(std::collections::HashSet::new()))
        .lock()
        .map(|mut warned| warned.insert(paths.executable.clone()))
        .unwrap_or(true);
    if first {
        eprintln!("{message}");
    }
    Err(RuntimeError::Unavailable(message.into()))
}

/// Live runtime state of a Silo computer that may not exist in the runtime yet.
pub(crate) enum ComputerRuntime {
    /// No runtime computer exists (a checkpoint restore is pending). It is stopped.
    Absent,
    Present(InspectedSandbox),
}

fn is_missing_computer(error: &RuntimeError) -> bool {
    match error {
        RuntimeError::Failed { detail, .. } => {
            let detail = detail.to_ascii_lowercase();
            detail.contains("not found")
                || detail.contains("no such sandbox")
                || detail.contains("does not exist")
        }
        _ => false,
    }
}

/// A Silo computer that is pending checkpoint restore has no runtime computer to configure yet.
pub(crate) fn is_pending_restore(paths: &RuntimePaths, name: &str) -> bool {
    resolve_computer_id(paths, name)
        .and_then(|id| checkpoints::is_pending(paths, &id))
        .unwrap_or(false)
}

/// Inspect a Silo computer without treating "not created yet" as a failure. An unattempted
/// restore is absent; after an attempt, inspect the computer it may have created. A runtime
/// that reports the computer as missing is likewise `Absent`.
pub(crate) fn observe_computer(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
) -> Result<ComputerRuntime, RuntimeError> {
    validate_name(name)?;
    if let Ok(id) = resolve_computer_id(paths, name) {
        if checkpoints::pending_view(paths, &id, true)? {
            return Ok(ComputerRuntime::Absent);
        }
    }
    match inspect_computer(runner, paths, name) {
        Ok(inspected) => Ok(ComputerRuntime::Present(inspected)),
        Err(error) if is_missing_computer(&error) => Ok(ComputerRuntime::Absent),
        Err(error) => Err(error),
    }
}

pub(crate) fn ensure_managed(inspected: &InspectedSandbox) -> Result<(), RuntimeError> {
    if inspected.name.is_empty()
        || inspected
            .config
            .pointer("/labels/silo.managed")
            .and_then(Value::as_str)
            != Some("true")
    {
        return Err(RuntimeError::Invalid(format!(
            "Computer '{}' is not owned by Silo. No computer operation was performed.",
            inspected.name
        )));
    }
    Ok(())
}

fn ensure_current_computer(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    if !read_metadata(&paths.metadata)?
        .computers
        .iter()
        .any(|current| current.id() == configuration.id() && current.name() == configuration.name())
    {
        return Err(RuntimeError::Invalid(
            "The computer identity changed. Its replacement was preserved.".into(),
        ));
    }
    Ok(())
}

fn ensure_computer_identity(
    configuration: &ComputerConfiguration,
    inspected: &InspectedSandbox,
) -> Result<(), RuntimeError> {
    ensure_managed(inspected)?;
    if inspected.name != configuration.name()
        || inspected
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            != Some(configuration.id())
    {
        return Err(RuntimeError::Invalid(
            "The computer identity changed. No settings were changed on the replacement.".into(),
        ));
    }
    Ok(())
}

fn application_computer(
    paths: &RuntimePaths,
    configuration: ComputerConfiguration,
    inspected: &InspectedSandbox,
) -> ApplicationComputer {
    let (state, state_detail, attention) = match inspected.status.as_str() {
        "Running" => (
            ComputerState::Running,
            "Running".into(),
            configuration_attention(paths, &configuration, inspected)
                .or_else(|| start_refresh_attention(paths, configuration.name())),
        ),
        "Starting" => (ComputerState::Starting, "Starting".into(), None),
        "Draining" => (ComputerState::Starting, "Stopping".into(), None),
        "Created" | "Stopped" => (
            ComputerState::Stopped,
            "Stopped".into(),
            configuration_attention(paths, &configuration, inspected),
        ),
        "Paused" => (ComputerState::Stopped, "Paused".into(), None),
        "Crashed" if crash_acknowledgement::is_acknowledged(paths, &configuration, inspected) => (
            ComputerState::Stopped,
            "Stopped".into(),
            configuration_attention(paths, &configuration, inspected),
        ),
        "Crashed" => (
            ComputerState::Failed,
            "The MicroSandbox runtime reported a crash.".into(),
            Some(ComputerAttention {
                level: AttentionLevel::Error,
                message: "The computer runtime crashed. Restart it to retry.".into(),
            }),
        ),
        other => (
            ComputerState::Failed,
            format!("The MicroSandbox runtime reported unknown state '{other}'."),
            Some(ComputerAttention {
                level: AttentionLevel::Error,
                message: format!("The computer runtime returned unknown state '{other}'."),
            }),
        ),
    };
    ApplicationComputer {
        configuration,
        purpose: "Local MicroSandbox".into(),
        state,
        state_detail,
        can_dismiss_error: inspected.status == "Crashed"
            && inspected
                .updated_at
                .as_ref()
                .is_some_and(|value| !value.is_empty())
            && matches!(state, ComputerState::Failed),
        lifecycle_failure: None,
        attention,
        freshness: Freshness::Fresh,
        settling: false,
        repositories: Vec::new(),
        files: Vec::new(),
        ports: Vec::new(),
        logs: Vec::new(),
        github_repositories: Vec::new(),
        secret_names: Vec::new(),
        pending_secret_revocations: Vec::new(),
        checkpoints: Vec::new(),
        pending_checkpoint_restore: None,
        checkpoint_operation: None,
        unfinished_restore: None,
    }
}

fn configuration_attention(
    _paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
    inspected: &InspectedSandbox,
) -> Option<ComputerAttention> {
    let ComputerConfiguration {
        cpus,
        max_cpus,
        memory_gib,
        max_memory_gib,
        runtime_storage_gib,
        ..
    } = configuration;
    let expected = (
        u64::from(*cpus),
        u64::from(*max_cpus),
        u64::from(*memory_gib) * 1024,
        u64::from(*max_memory_gib) * 1024,
        u64::from(*runtime_storage_gib) * 1024,
    );
    let actual = (
        inspected
            .config
            .pointer("/resources/cpus")
            .and_then(Value::as_u64),
        inspected
            .config
            .pointer("/resources/max_cpus")
            .and_then(Value::as_u64),
        inspected
            .config
            .pointer("/resources/memory_mib")
            .and_then(Value::as_u64),
        inspected
            .config
            .pointer("/resources/max_memory_mib")
            .and_then(Value::as_u64),
        inspected
            .config
            .pointer("/image/Oci/root_disk/size_mib")
            .and_then(Value::as_u64),
    );
    let workspace_storage_gib = configuration.workspace_storage_gib;
    let mounts_match = inspected
        .config
        .get("mounts")
        .and_then(Value::as_array)
        .is_some_and(|mounts| {
            mounts
                .iter()
                .filter(|mount| mount["guest"] == WORKSPACE_MOUNT)
                .count()
                == 1
                && mounts.iter().any(|mount| {
                    mount["guest"] == WORKSPACE_MOUNT
                        && mount["type"] == "Owned"
                        && mount.pointer("/storage/kind").and_then(Value::as_str) == Some("disk")
                        && mount
                            .pointer("/storage/capacity_mib")
                            .and_then(Value::as_u64)
                            == Some(u64::from(workspace_storage_gib) * 1024)
                })
        });
    if mounts_match
        && actual
            == (
                Some(expected.0),
                Some(expected.1),
                Some(expected.2),
                Some(expected.3),
                Some(expected.4),
            )
    {
        None
    } else {
        Some(ComputerAttention {
            level: AttentionLevel::Warning,
            message: "Saved Silo resource settings differ from the runtime configuration.".into(),
        })
    }
}

pub(crate) fn start_at_launch(app: &AppHandle, id: &str) -> Result<(), String> {
    let paths = runtime_paths(app)?;
    // A launch start changes only its computer, so it takes that computer's lane like a user Start:
    // other computers' actions and state are not held behind each boot.
    let guard = match launch_start_guard(&OPERATIONS, &paths, id)
        .map_err(|error| safe_activity_error(&error))?
    {
        LaunchAdmission::Admitted(guard) => Some(guard),
        LaunchAdmission::Unguarded => None,
        LaunchAdmission::Skipped => return Ok(()),
    };
    shutdown::ensure_accepting_operations()?;
    if crate::startup::is_cancelled(app) {
        return Ok(());
    }
    let outcome = device_resources()
        .and_then(|device| start_at_launch_with(&ProcessRunner, &paths, &device, id));
    drop(guard);
    match outcome {
        Ok(LaunchStart::Done) => Ok(()),
        Ok(LaunchStart::NeedsExplicitStart(name)) => Err(format!(
            "{name} starts from a checkpoint and needs an explicit Start from its computer view."
        )),
        // The user cancelled this start from the queue; that is not a launch failure.
        Err(RuntimeError::Cancelled { .. }) => Ok(()),
        Err(error) => Err(safe_activity_error(&error)),
    }
}

enum LaunchAdmission<'a> {
    /// Run the start holding this computer's lane.
    Admitted(operation_gate::OperationGuard<'a>),
    /// Not a configured local computer: run unguarded so `start_at_launch_with` reports why.
    Unguarded,
    /// The user already queued the same Start, or cancelled this one while it waited.
    Skipped,
}

/// Admit a launch-time start on the selected computer's own lane, with the same label, kind,
/// dedupe key, cancellability and expected duration as a user Start.
fn launch_start_guard<'a>(
    gate: &'a operation_gate::OperationGate,
    paths: &RuntimePaths,
    id: &str,
) -> Result<LaunchAdmission<'a>, RuntimeError> {
    let metadata = read_metadata(&paths.metadata)?;
    let Some(configuration) = metadata
        .computers
        .iter()
        .find(|configuration| configuration.id() == id)
    else {
        return Ok(LaunchAdmission::Unguarded);
    };
    let guard = gate.kind(operation_gate::OperationKind::Lifecycle).acquire(
        operation_gate::Scope::Computer { id: id.to_owned() },
        Some(configuration.name().to_owned()),
        &lifecycle_label("start", configuration.name()),
        Some(format!("computer:{id}:start")),
    );
    match guard {
        Ok(guard) => {
            guard.allow_cancel();
            guard.expect_within(Duration::from_secs(180));
            Ok(LaunchAdmission::Admitted(guard))
        }
        Err(operation_gate::GateError::AlreadyQueued | operation_gate::GateError::Cancelled) => {
            Ok(LaunchAdmission::Skipped)
        }
        Err(error) => Err(RuntimeError::from(error)),
    }
}

/// What a launch-time start did for one selected computer.
#[derive(Debug, PartialEq, Eq)]
enum LaunchStart {
    /// Started, or already running.
    Done,
    /// A fork or restore waits for its first explicit Start; it was not started.
    NeedsExplicitStart(String),
}

fn start_at_launch_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    device: &DeviceResources,
    id: &str,
) -> Result<LaunchStart, RuntimeError> {
    let metadata = read_metadata(&paths.metadata)?;
    let configuration = metadata.computers.iter().find(|configuration| configuration.id() == id)
        .ok_or_else(|| RuntimeError::Invalid("A computer selected for launch no longer exists. Update the startup selection in Settings.".into()))?;
    let name = configuration.name();
    if checkpoints::needs_explicit_start(paths, configuration.id())? {
        return Ok(LaunchStart::NeedsExplicitStart(name.to_owned()));
    }
    let result = (|| {
        let inspected = inspect_computer(runner, paths, name)?;
        ensure_computer_identity(configuration, &inspected)?;
        match inspected.status.to_ascii_lowercase().as_str() {
            "running" => Ok(()),
            "created" | "stopped" => computer_action_with(runner, paths, device, "start", name),
            _ => Err(RuntimeError::Invalid(format!(
                "{name} is not stopped or running. Check its status before starting it."
            ))),
        }
    })();
    match result {
        Ok(()) => Ok(LaunchStart::Done),
        Err(error @ RuntimeError::Cancelled { .. }) => Err(error),
        Err(error) => Err(RuntimeError::Invalid(format!(
            "{name}: {}",
            safe_activity_error(&error)
        ))),
    }
}

// Both local and remote user actions must activate the same selected checkpoint.
// Background startup and other implicit callers retain the explicit-Start guard.
fn explicit_computer_action_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    device: &DeviceResources,
    action: &str,
    name: &str,
) -> Result<(), RuntimeError> {
    if action == "start" {
        if let Some(configuration) = read_metadata(&paths.metadata)?
            .computers
            .into_iter()
            .find(|configuration| configuration.name() == name)
        {
            if checkpoints::needs_explicit_start(paths, configuration.id())? {
                checkpoints::start_pending(runner, paths, &configuration)?;
                crate::github::computer_restored(name);
                return Ok(());
            }
        }
    }
    computer_action_with(runner, paths, device, action, name)
}

fn computer_action_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    device: &DeviceResources,
    action: &str,
    name: &str,
) -> Result<(), RuntimeError> {
    if matches!(action, "start" | "stop" | "restart") {
        if let Some(configuration) = read_metadata(&paths.metadata)?
            .computers
            .into_iter()
            .find(|configuration| configuration.name() == name)
        {
            if checkpoints::needs_explicit_start(paths, configuration.id())? {
                return Err(RuntimeError::Invalid(checkpoints::explicit_start_message(
                    paths,
                    configuration.id(),
                    configuration.name(),
                )));
            }
        }
    }
    if action == "dismiss-error" {
        return crash_acknowledgement::dismiss(runner, paths, name);
    }
    if matches!(action, "start" | "stop" | "restart") {
        crash_acknowledgement::clear(paths, name)?;
    }
    lifecycle_recovery::perform(runner, paths, device, action, name)
}

#[cfg(test)]
fn apply_whole_configuration(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    device: &DeviceResources,
    request: ComputerConfigurationRequest,
) -> Result<(), RuntimeError> {
    apply_whole_configuration_with_progress(runner, paths, device, request, None, &|_, _, _| {})
}

/// Default the desktop of new v4 computers. This must run before a request is compared
/// with or recorded in the configuration journal, so a resubmitted failed creation
/// matches the journaled (already defaulted) request.
fn apply_desktop_defaults(
    previous: &ComputerConfigurationRequest,
    request: &mut ComputerConfigurationRequest,
) {
    // Tests never depend on which image the lock pins: they run as a v3 image unless
    // they pin another version (`guest_image::pin_test_version`).
    #[cfg(test)]
    let version = guest_image::test_version();
    #[cfg(not(test))]
    let version = guest_image::pinned_version();
    apply_desktop_defaults_for(version.as_deref(), previous, request);
}

fn apply_desktop_defaults_for(
    version: Option<&str>,
    previous: &ComputerConfigurationRequest,
    request: &mut ComputerConfigurationRequest,
) {
    if let Some(version) = version {
        crate::desktop::default_new_computer_desktops(
            &mut request.computers,
            &previous.computers,
            version,
        );
    }
}

fn apply_whole_configuration_with_progress(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    device: &DeviceResources,
    mut request: ComputerConfigurationRequest,
    retry_computer: Option<&str>,
    progress: &dyn Fn(&str, &str, u8),
) -> Result<(), RuntimeError> {
    let _attempt = configuration_recovery::attempt();
    validate_request(&request)?;
    let previous = read_metadata(&paths.metadata)?;
    apply_desktop_defaults(&previous, &mut request);
    if retry_computer.is_some_and(|name| {
        !request
            .computers
            .iter()
            .chain(&previous.computers)
            .any(|configuration| configuration.name() == name)
    }) {
        return Err(RuntimeError::Invalid(
            "The computer selected for retry is not in this configuration.".into(),
        ));
    }
    validate_requested_resources(&request, device)?;
    let previous_by_id: HashMap<&str, &ComputerConfiguration> = previous
        .computers
        .iter()
        .map(|configuration| (configuration.id(), configuration))
        .collect();
    let requested_ids: HashSet<&str> = request
        .computers
        .iter()
        .map(ComputerConfiguration::id)
        .collect();

    for configuration in &request.computers {
        if let Some(old) = previous_by_id.get(configuration.id()) {
            if *old != configuration {
                validate_computer_update(old, configuration)?;
                ensure_computer_identity(
                    configuration,
                    &inspect_computer(runner, paths, configuration.name())?,
                )?;
            }
        }
    }
    for configuration in previous
        .computers
        .iter()
        .filter(|configuration| !requested_ids.contains(configuration.id()))
    {
        preflight_removal(runner, paths, configuration)?;
    }
    configuration_recovery::begin(paths, &request)?;
    let mut applied = previous.clone();
    let mut changed = false;
    let result = (|| {
        // Removals run first, as the frontend sends them: deleting a computer and adding
        // a new one with the same name in one batch must free its name and storage.
        for configuration in previous
            .computers
            .iter()
            .filter(|configuration| !requested_ids.contains(configuration.id()))
        {
            progress("computer-removal", configuration.name(), 0);
            remove_computer_runtime(runner, paths, configuration)?;
            changed = true;
            crate::network::computer_removed(paths, configuration.name())
                .map_err(RuntimeError::Unavailable)?;
            applied
                .computers
                .retain(|existing| existing.id() != configuration.id());
            write_metadata(&paths.metadata, &applied)?;
            lifecycle_recovery::forget_removed(paths, configuration)?;
            crate::secrets::computer_removed(configuration.name())
                .map_err(RuntimeError::Unavailable)?;
            forget_github_state(&paths.home, configuration.name());
            crate::github::computer_removed(configuration.name())
                .map_err(RuntimeError::Unavailable)?;
            remove_computer_volumes(paths, configuration)?;
            checkpoints::remove_deleted_snapshots(
                runner,
                paths,
                configuration.id(),
                configuration.name(),
            )?;
            checkpoints::forget_removed(paths, configuration.id())?;
            progress("computer-removal", configuration.name(), 1);
        }
        for configuration in &request.computers {
            match previous_by_id.get(configuration.id()) {
                None => {
                    progress("computer-configuration", configuration.name(), 0);
                    create_computer_with_progress(runner, paths, configuration, progress)?;
                }
                Some(old) if *old == configuration => {
                    if retry_computer == Some(configuration.name()) {
                        progress("computer-verification", configuration.name(), 0);
                        verify_computer_configuration(runner, paths, configuration)?;
                        progress("computer-verification", configuration.name(), 1);
                    }
                    continue;
                }
                Some(old) => {
                    progress("computer-configuration", configuration.name(), 0);
                    update_computer(runner, paths, old, configuration)?;
                }
            }
            changed = true;
            applied
                .computers
                .retain(|existing| existing.id() != configuration.id());
            applied.computers.push(configuration.clone());
            progress("computer-settings", configuration.name(), 0);
            write_metadata(&paths.metadata, &applied)?;
            progress("computer-configuration", configuration.name(), 1);
            progress("computer-verification", configuration.name(), 0);
            verify_computer_configuration(runner, paths, configuration)?;
            progress("computer-verification", configuration.name(), 1);
        }
        write_metadata(&paths.metadata, &request)?;
        configuration_recovery::finish(paths)
    })();
    result.map_err(|error| {
        if changed {
            // Keep the typed error so its precise text and category survive (D-16).
            RuntimeError::Partial(Box::new(error))
        } else {
            error
        }
    })
}

fn verify_computer_configuration(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    let inspected = inspect_computer(runner, paths, configuration.name())?;
    ensure_managed(&inspected)?;
    if configuration_attention(paths, configuration, &inspected).is_some() {
        return Err(RuntimeError::Malformed(format!("Saved runtime resources for '{}' do not match the requested configuration. Setup is not complete.", configuration.name())));
    }
    Ok(())
}

fn validate_computer_update(
    previous: &ComputerConfiguration,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    if crate::desktop::configuration(previous).is_some()
        && crate::desktop::configuration(configuration).is_none()
    {
        return Err(RuntimeError::Invalid(
            "Desktop removal is not supported. Turn off automatic startup instead.".into(),
        ));
    }
    if previous.name() != configuration.name() {
        return Err(RuntimeError::Invalid(format!("Bundled MicroSandbox cannot rename computer '{}'. Keep its name or create a new computer.", previous.name())));
    }
    if previous.workspace_storage_gib == configuration.workspace_storage_gib
        && previous.runtime_storage_gib == configuration.runtime_storage_gib
    {
        Ok(())
    } else {
        Err(RuntimeError::Invalid("Storage disks cannot be resized in place. Keep both saved sizes or create a new computer.".into()))
    }
}

fn verify_guest_tools(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
) -> Result<(), RuntimeError> {
    runner.run(
        paths,
        &[
            "exec".into(),
            name.into(),
            "--no-tty".into(),
            "--quiet".into(),
            "--timeout".into(),
            "30s".into(),
            "--user".into(),
            "root".into(),
            "--workdir".into(),
            "/".into(),
            "--".into(),
            "sh".into(),
            "-c".into(),
            include_str!("../guest/verify-tools.sh").into(),
        ],
        Duration::from_secs(45),
    )?;
    let inspected = inspect_computer(runner, paths, name)?;
    ensure_managed(&inspected)?;
    if !matches!(inspected.status.as_str(), "Created" | "Stopped") {
        return Err(RuntimeError::Malformed(
            "The computer tools were prepared, but the runtime did not restore its stopped state."
                .into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
fn create_computer(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    create_computer_with_progress(runner, paths, configuration, &|_, _, _| {})
}

fn create_computer_with_progress(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
    progress: &dyn Fn(&str, &str, u8),
) -> Result<(), RuntimeError> {
    let ComputerConfiguration {
        id,
        name,
        cpus,
        max_cpus,
        memory_gib,
        max_memory_gib,
        workspace_storage_gib,
        runtime_storage_gib,
        desktop,
    } = configuration;
    // Needed before anything is claimed or created: a computer without its mount never gets it.
    let mounts = crate::computer_use::mount_args(configuration)?;
    configuration_recovery::claim(paths, configuration)?;
    progress("computer-disk-preparation", name, 0);
    let preflight = (|| {
        let listed = runner.run(
            paths,
            &["list".into(), "--format".into(), "json".into()],
            READ_TIMEOUT,
        )?;
        let listed: Vec<ListedSandbox> = serde_json::from_str(&listed.stdout).map_err(|_| {
            RuntimeError::Malformed(
                "The runtime returned an invalid computer list before creation.".into(),
            )
        })?;
        if listed.iter().any(|computer| computer.name == *name) {
            return Err(RuntimeError::Invalid(format!(
                "Computer '{name}' already exists in the runtime. No existing computer was changed."
            )));
        }
        let protocol = runner.run(paths, &["--silo-github-protocol".into()], READ_TIMEOUT)?;
        if protocol.stdout.trim() != "1" {
            return Err(RuntimeError::Unavailable(
                "The bundled runtime does not support secure GitHub access. Repair Silo before creating computers.".into(),
            ));
        }
        let protocol = runner.run(
            paths,
            &["--silo-working-account-protocol".into()],
            READ_TIMEOUT,
        )?;
        if protocol.stdout.trim() != "1" {
            return Err(RuntimeError::Unavailable(
                "The bundled runtime does not support normal working accounts. Repair or update Silo before creating computers.".into(),
            ));
        }
        Ok(())
    })();
    preflight?;
    progress("computer-image-preparation", name, 0);
    let image = runner.prepare_guest_image(paths)?;
    let mut args: Vec<String> = vec![
        "create".into(),
        image,
        "--pull".into(),
        "never".into(),
        "--name".into(),
        name.clone(),
        "--cpus".into(),
        cpus.to_string(),
        "--max-cpus".into(),
        max_cpus.to_string(),
        "--memory".into(),
        format!("{memory_gib}G"),
        "--max-memory".into(),
        format!("{max_memory_gib}G"),
        "--root-disk".into(),
        format!("{runtime_storage_gib}G"),
        "--mount-owned".into(),
        format!("{WORKSPACE_MOUNT}:kind=disk,size={workspace_storage_gib}G"),
        "--label".into(),
        MANAGED_LABEL.into(),
        "--label".into(),
        format!("silo.machine-id={id}"),
        "--label".into(),
        format!("silo.workspace-storage-gib={workspace_storage_gib}"),
        "--label".into(),
        format!("silo.runtime-storage-gib={runtime_storage_gib}"),
        "--secret".into(),
        secrets_runtime::SILO_GITHUB_SECRET_SPEC.into(),
        // Explicit, so a computer does not depend on the runtime's default (off in
        // 0.7.2, on since 0.7.4). It matches `guest/github-network-default.json`,
        // which exports compare against; no hostname rule makes it observable.
        "--net-strict=true".into(),
        "--env".into(),
        "GH_TOKEN=$MSB_SILO_GITHUB".into(),
        "--label".into(),
        "silo.github-protocol=1".into(),
        "--no-start".into(),
        "--quiet".into(),
        "--progress-json".into(),
    ];
    // Built-in computer use: the device's ChatGPT app folder, read-only. A snapshot
    // never carries host mounts, so every restore passes this again (see checkpoints).
    let before_start = args
        .iter()
        .position(|arg| arg == "--no-start")
        .unwrap_or(args.len());
    args.splice(before_start..before_start, mounts);
    progress("computer-runtime-preparation", name, 0);
    if let Err(error) = runner.run(paths, &args, MUTATION_TIMEOUT) {
        return Err(with_cleanup_error(
            error,
            cleanup_failed_create(runner, paths, name, id),
        ));
    }
    let inspected = inspect_computer(runner, paths, name)?;
    ensure_managed(&inspected)?;
    if !matches!(inspected.status.as_str(), "Created" | "Stopped") {
        return Err(with_cleanup_error(
            RuntimeError::Malformed(format!(
                "Computer '{name}' did not remain stopped after creation."
            )),
            cleanup_failed_create(runner, paths, name, id),
        ));
    }
    if let Err(error) = verify_guest_tools(runner, paths, name) {
        return Err(with_cleanup_error(
            error,
            cleanup_failed_create(runner, paths, name, id),
        ));
    }
    if let Some(desktop) = desktop {
        progress("desktop-installation", name, 0);
        crate::desktop::configure_with(runner, paths, name, None, desktop)?;
        let inspected = inspect_computer(runner, paths, name)?;
        ensure_managed(&inspected)?;
        if !matches!(inspected.status.as_str(), "Created" | "Stopped") {
            return Err(RuntimeError::Malformed("Desktop installation completed, but the computer did not return to its stopped state.".into()));
        }
        progress("desktop-installation", name, 1);
    }
    crate::computer_use::start_with(paths, id, crate::computer_use::initial_approval());
    // A skipped setup is consumed even for a computer without computer use.
    let without_computer_use = crate::creation_inputs::take_without_computer_use(id);
    if crate::computer_use::is_built_in(configuration) {
        progress("computer-use-setup", name, 0);
        let finished = !without_computer_use
            && crate::computer_use::finish_in_creation(runner, paths, id, name)
                .map_err(|reason| {
                    eprintln!("Computer use was not set up while creating {name}: {reason}")
                })
                .is_ok();
        // The first start applies it, as it does for any apply that did not finish.
        if !finished {
            progress("computer-use-pending", name, 0);
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn apply_disposable_test_identity(
    paths: &RuntimePaths,
    name: &str,
) -> Result<(), RuntimeError> {
    configure_computer_identities_with(
        &ProcessRunner,
        paths,
        &[ComputerIdentity {
            computer: name.into(),
            name: "Silo Test".into(),
            email: "silo-test@example.invalid".into(),
            apply: true,
        }],
    )
}

#[cfg(test)]
pub(crate) fn create_disposable_test_computer(
    paths: &RuntimePaths,
    name: &str,
) -> Result<ComputerConfiguration, RuntimeError> {
    let configuration = ComputerConfiguration {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.into(),
        cpus: 1,
        max_cpus: 1,
        memory_gib: 1,
        max_memory_gib: 1,
        workspace_storage_gib: 1,
        runtime_storage_gib: 2,
        desktop: None,
    };
    let mut request = read_metadata(&paths.metadata)?;
    request.computers.push(configuration.clone());
    apply_whole_configuration(&ProcessRunner, paths, &device_resources()?, request)?;
    Ok(configuration)
}

/// Like `create_disposable_test_computer` with sizes a desktop needs. The configuration
/// is applied as the app does: a v4 image makes the new computer's desktop built in.
#[cfg(test)]
pub(crate) fn create_disposable_desktop_computer(
    paths: &RuntimePaths,
    name: &str,
) -> Result<ComputerConfiguration, RuntimeError> {
    let configuration = ComputerConfiguration {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.into(),
        cpus: 2,
        max_cpus: 2,
        memory_gib: 4,
        max_memory_gib: 4,
        workspace_storage_gib: 2,
        runtime_storage_gib: 6,
        desktop: None,
    };
    let mut request = read_metadata(&paths.metadata)?;
    request.computers.push(configuration);
    // Unit tests default to a v3 image; a live run uses the image it was given, so a
    // v4 image makes the new computer built in exactly as the app does.
    let _pin = guest_image::pinned_version().map(|version| guest_image::pin_test_version(&version));
    apply_whole_configuration(&ProcessRunner, paths, &device_resources()?, request)?;
    read_metadata(&paths.metadata)?
        .computers
        .into_iter()
        .find(|configuration| configuration.name() == name)
        .ok_or_else(|| RuntimeError::Invalid("The disposable computer was not saved.".into()))
}

/// The app's explicit Start for a disposable computer (boot hooks included).
#[cfg(test)]
pub(crate) fn start_disposable_test_computer(
    paths: &RuntimePaths,
    name: &str,
) -> Result<(), RuntimeError> {
    computer_action_with(&ProcessRunner, paths, &device_resources()?, "start", name)
}

/// The app's `stop` or `restart` for a disposable computer (boot hooks included).
#[cfg(test)]
pub(crate) fn disposable_test_action(
    paths: &RuntimePaths,
    name: &str,
    action: &str,
) -> Result<(), RuntimeError> {
    computer_action_with(&ProcessRunner, paths, &device_resources()?, action, name)
}

/// Exercise the same explicit Start path as the app for an imported computer.
/// Live regressions supply disposable runtime paths; no app data is resolved here.
#[cfg(test)]
pub(crate) fn start_disposable_test_import(
    paths: &RuntimePaths,
    name: &str,
) -> Result<(), RuntimeError> {
    let _guard = OPERATIONS
        .device("Starting disposable imported computer")
        .map_err(|error| RuntimeError::Invalid(error.to_string()))?;
    let configuration = read_metadata(&paths.metadata)?
        .computers
        .into_iter()
        .find(|configuration| configuration.name() == name)
        .ok_or_else(|| RuntimeError::Invalid("Imported test computer is missing.".into()))?;
    checkpoints::start_pending(&ProcessRunner, paths, &configuration)
}

/// Remove the runtime's computer `name` when it is Silo's own with `computer_id`, then the
/// managed disk of the computer. Used after a failed create, and by recovery
/// for a computer an import created and never saved.
pub(crate) fn cleanup_failed_create(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
    computer_id: &str,
) -> Result<(), RuntimeError> {
    let listed = runner.run(
        paths,
        &["list".into(), "--format".into(), "json".into()],
        READ_TIMEOUT,
    )?;
    let listed: Vec<ListedSandbox> = serde_json::from_str(&listed.stdout).map_err(|_| {
        RuntimeError::Malformed(
            "The bundled runtime returned an invalid computer list during cleanup.".into(),
        )
    })?;
    if listed.iter().any(|computer| computer.name == name) {
        let inspected = inspect_computer(runner, paths, name)?;
        ensure_managed(&inspected)?;
        if inspected
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            != Some(computer_id)
        {
            return Err(RuntimeError::Invalid(
                "The runtime computer identity changed during creation. Its data was preserved."
                    .into(),
            ));
        }
        runner.run(
            paths,
            &[
                "remove".into(),
                "--force".into(),
                "--quiet".into(),
                name.into(),
            ],
            STOP_TIMEOUT,
        )?;
    }
    remove_disk_path(&disk_path(paths, name, "workspace"))?;
    Ok(())
}

pub(crate) fn disk_path(paths: &RuntimePaths, computer_name: &str, role: &str) -> PathBuf {
    paths
        .volumes
        .join(computer_name)
        .join(format!("{role}.raw"))
}

fn remove_disk_path(path: &Path) -> Result<(), RuntimeError> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(RuntimeError::Unavailable(format!(
                "Silo could not remove an incomplete managed disk: {error}"
            )))
        }
    }
    Ok(())
}

fn remove_computer_volumes(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    let ComputerConfiguration { name, .. } = configuration;
    let mut failure = None;
    for role in ["workspace"] {
        if let Err(error) = remove_disk_path(&disk_path(paths, name, role)) {
            failure = Some(match failure {
                None => error,
                Some(previous) => with_cleanup_error(previous, Err(error)),
            });
        }
    }
    failure.map_or(Ok(()), Err)?;
    match fs::remove_dir(paths.volumes.join(name)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(RuntimeError::Unavailable(format!(
            "Silo could not remove the computer's managed disk folder: {error}"
        ))),
    }
}

fn with_cleanup_error(original: RuntimeError, cleanup: Result<(), RuntimeError>) -> RuntimeError {
    match cleanup {
        Ok(()) => original,
        Err(cleanup) => RuntimeError::Failed {
            operation: "Applying the computer configuration".into(),
            exit_code: None,
            detail: format!("{original} Cleanup also failed: {cleanup}"),
        },
    }
}

fn update_computer(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    previous: &ComputerConfiguration,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    // Renames, storage resizes and desktop removal are rejected here.
    validate_computer_update(previous, configuration)?;
    if crate::desktop::only_desktop_changed(previous, configuration) {
        ensure_computer_identity(
            previous,
            &inspect_computer(runner, paths, configuration.name())?,
        )?;
        return crate::desktop::configure_with(
            runner,
            paths,
            configuration.name(),
            crate::desktop::configuration(previous),
            crate::desktop::configuration(configuration)
                .ok_or_else(|| RuntimeError::Invalid("Desktop removal is not supported.".into()))?,
        );
    }
    {
        {
            let ComputerConfiguration { name, .. } = previous;
            let ComputerConfiguration {
                id,
                cpus,
                max_cpus,
                memory_gib,
                max_memory_gib,
                workspace_storage_gib,
                runtime_storage_gib,
                ..
            } = configuration;
            let inspected = inspect_computer(runner, paths, name)?;
            ensure_computer_identity(previous, &inspected)?;
            if inspected.status == "Running" {
                runner.run(
                    paths,
                    &["stop".into(), name.clone(), "--quiet".into()],
                    STOP_TIMEOUT,
                )?;
                let stopped = inspect_computer(runner, paths, name)?;
                ensure_computer_identity(previous, &stopped)?;
                if stopped.status != "Stopped" {
                    return Err(RuntimeError::Invalid(format!(
                        "{name} did not stop. Its settings were not changed. Retry after checking its state."
                    )));
                }
            } else if !matches!(inspected.status.as_str(), "Stopped" | "Created") {
                return Err(RuntimeError::Invalid(format!(
                    "{name} is not ready for editing. Stop it before saving changes."
                )));
            }
            runner.run(
                paths,
                &[
                    "modify".into(),
                    name.clone(),
                    "--cpus".into(),
                    cpus.to_string(),
                    "--max-cpus".into(),
                    max_cpus.to_string(),
                    "--memory".into(),
                    format!("{memory_gib}G"),
                    "--max-memory".into(),
                    format!("{max_memory_gib}G"),
                    "--label".into(),
                    format!("silo.machine-id={id}"),
                    "--label".into(),
                    format!("silo.workspace-storage-gib={workspace_storage_gib}"),
                    "--label".into(),
                    format!("silo.runtime-storage-gib={runtime_storage_gib}"),
                    "--next-start".into(),
                    "--format".into(),
                    "json".into(),
                ],
                MUTATION_TIMEOUT,
            )?;
            if let Some(desktop) = crate::desktop::configuration(configuration) {
                if crate::desktop::configuration(previous) != Some(desktop) {
                    crate::desktop::configure_with(
                        runner,
                        paths,
                        name,
                        crate::desktop::configuration(previous),
                        desktop,
                    )?;
                }
            }
            Ok(())
        }
    }
}

/// What deleting a computer removes from the runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RemovalTarget {
    /// Nothing: a pending checkpoint restore with no runtime computer yet.
    Nothing,
    /// An ordinary stopped computer.
    Stopped,
    /// The runtime computer a pending restore's attempt created (then failed verification or
    /// timed out). Its computer cannot be started or stopped normally, so deletion stops
    /// it first when it is still running.
    RestoreAttempt { running: bool },
}

fn preflight_removal(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<RemovalTarget, RuntimeError> {
    let ComputerConfiguration { name, .. } = configuration;
    // An unreadable checkpoint record leaves the pending state unknown; the runtime and
    // its labels then decide, so a damaged record never blocks deleting its computer.
    let pending = checkpoints::is_pending(paths, configuration.id()).ok();
    let inspected = match inspect_computer(runner, paths, name) {
        Ok(inspected) => inspected,
        Err(error) if pending != Some(false) && is_missing_computer(&error) => {
            return Ok(RemovalTarget::Nothing)
        }
        Err(error) => return Err(error),
    };
    ensure_managed(&inspected)?;
    if inspected.name != *name
        || inspected
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            != Some(configuration.id())
    {
        return Err(RuntimeError::Invalid(
            "The computer selected for deletion changed identity. It was preserved.".into(),
        ));
    }
    let stopped = matches!(inspected.status.as_str(), "Stopped" | "Created" | "Crashed");
    let attempt = inspected
        .config
        .pointer("/labels/silo.restore-attempt")
        .and_then(Value::as_str)
        .is_some();
    if pending != Some(false) && attempt {
        return Ok(RemovalTarget::RestoreAttempt { running: !stopped });
    }
    if pending == Some(true) {
        return Err(RuntimeError::Invalid(format!(
            "A runtime computer named '{name}' was not created by this computer's checkpoint restore. It was preserved."
        )));
    }
    if !stopped {
        return Err(RuntimeError::Invalid(format!(
            "Stop computer '{name}' before removing it from Silo. This computer was not removed."
        )));
    }
    Ok(RemovalTarget::Stopped)
}

fn remove_computer_runtime(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    let name = configuration.name();
    match preflight_removal(runner, paths, configuration)? {
        RemovalTarget::Nothing => return Ok(()),
        RemovalTarget::Stopped | RemovalTarget::RestoreAttempt { running: false } => {}
        RemovalTarget::RestoreAttempt { running: true } => {
            runner.run(
                paths,
                &["stop".into(), name.into(), "--quiet".into()],
                STOP_TIMEOUT,
            )?;
        }
    }
    runner.run(
        paths,
        &["remove".into(), "--quiet".into(), name.into()],
        STOP_TIMEOUT,
    )?;
    Ok(())
}

#[cfg(test)]
fn remove_computer(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    remove_computer_runtime(runner, paths, configuration)?;
    remove_computer_volumes(paths, configuration)
}

fn validate_request(request: &ComputerConfigurationRequest) -> Result<(), RuntimeError> {
    if request.schema_version != 1 {
        return Err(RuntimeError::Invalid(
            "The computer configuration version is not supported.".into(),
        ));
    }
    if request.computers.len() > MAX_COMPUTER_COUNT {
        return Err(RuntimeError::Invalid(format!(
            "Configure at most {MAX_COMPUTER_COUNT} computers."
        )));
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for configuration in &request.computers {
        validate_name(configuration.name())?;
        if uuid::Uuid::try_parse(configuration.id()).is_err() || !ids.insert(configuration.id()) {
            return Err(RuntimeError::Invalid(
                "Every computer must have a unique valid identifier.".into(),
            ));
        }
        if !names.insert(configuration.name().to_ascii_lowercase()) {
            return Err(RuntimeError::Invalid(
                "Computer names must be unique.".into(),
            ));
        }
        let ComputerConfiguration {
            cpus,
            max_cpus,
            memory_gib,
            max_memory_gib,
            workspace_storage_gib,
            runtime_storage_gib,
            ..
        } = configuration;
        if *cpus == 0 || cpus > max_cpus {
            return Err(RuntimeError::Invalid(format!(
                "Computer '{}' has an invalid CPU limit or ceiling.",
                configuration.name()
            )));
        }
        if *memory_gib == 0 || memory_gib > max_memory_gib {
            return Err(RuntimeError::Invalid(format!(
                "Computer '{}' has an invalid memory limit or ceiling.",
                configuration.name()
            )));
        }
        let total = workspace_storage_gib
            .checked_add(*runtime_storage_gib)
            .and_then(|gib| gib.checked_mul(1024));
        if *workspace_storage_gib == 0 || *runtime_storage_gib == 0 || total.is_none() {
            return Err(RuntimeError::Invalid(format!(
                "Computer '{}' has an invalid storage allocation.",
                configuration.name()
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_name(name: &str) -> Result<(), RuntimeError> {
    let mut characters = name.chars();
    let valid = name.len() <= 32
        && characters
            .next()
            .is_some_and(|character| character.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        });
    if valid {
        Ok(())
    } else {
        Err(RuntimeError::Invalid(format!(
            "Computer name '{name}' must start with a lowercase letter and contain at most 32 lowercase letters, numbers, or hyphens."
        )))
    }
}

pub(crate) fn read_metadata(path: &Path) -> Result<ComputerConfigurationRequest, RuntimeError> {
    Ok(
        read_saved_metadata(path)?.unwrap_or(ComputerConfigurationRequest {
            schema_version: 1,
            computers: Vec::new(),
        }),
    )
}
/// The saved configuration held by `inventory`, checked as `read_metadata` checks a file.
pub(crate) fn metadata_from_value(
    inventory: serde_json::Value,
) -> Result<ComputerConfigurationRequest, RuntimeError> {
    let request: ComputerConfigurationRequest =
        serde_json::from_value(inventory).map_err(|_| {
            RuntimeError::Malformed("Silo's saved computer configuration is invalid.".into())
        })?;
    validate_request(&request)?;
    Ok(request)
}

fn read_saved_metadata(path: &Path) -> Result<Option<ComputerConfigurationRequest>, RuntimeError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(RuntimeError::Unavailable(format!(
                "Silo could not read its computer configuration: {error}"
            )))
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            RuntimeError::Unavailable(format!(
                "Silo could not read its computer configuration: {error}"
            ))
        })?;
    if bytes.len() as u64 > MAX_OUTPUT_BYTES {
        return Err(RuntimeError::Malformed(
            "Silo's computer configuration is too large.".into(),
        ));
    }
    let request: ComputerConfigurationRequest = serde_json::from_slice(&bytes).map_err(|_| {
        RuntimeError::Malformed("Silo's saved computer configuration is invalid.".into())
    })?;
    validate_request(&request)?;
    Ok(Some(request))
}

/// Resolve a local computer's stable id from its current display name in fresh metadata.
/// Per-computer operation ordering keys on the stable id, so callers that only have a name
/// turn it into an id before acquiring the operation gate. Returns a clear error when
/// no local computer by that name currently exists.
pub(crate) fn resolve_computer_id(
    paths: &RuntimePaths,
    name: &str,
) -> Result<String, RuntimeError> {
    read_metadata(&paths.metadata)?
        .computers
        .into_iter()
        .find(|configuration| configuration.name() == name)
        .map(|configuration| configuration.id().to_owned())
        .ok_or_else(|| RuntimeError::Invalid("This computer no longer exists.".into()))
}

pub(crate) fn write_metadata(
    path: &Path,
    request: &ComputerConfigurationRequest,
) -> Result<(), RuntimeError> {
    let parent = path.parent().ok_or_else(|| {
        RuntimeError::Unavailable("Silo's computer storage path is invalid.".into())
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        RuntimeError::Unavailable(format!(
            "Silo could not prepare its computer settings: {error}"
        ))
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        RuntimeError::Unavailable(format!(
            "Silo could not stage its computer settings: {error}"
        ))
    })?;
    let mut bytes = serde_json::to_vec_pretty(request).map_err(|_| {
        RuntimeError::Malformed("Silo could not encode its computer settings.".into())
    })?;
    bytes.push(b'\n');
    temporary.write_all(&bytes).map_err(|error| {
        RuntimeError::Unavailable(format!(
            "Silo could not save its computer settings: {error}"
        ))
    })?;
    temporary.as_file().sync_all().map_err(|error| {
        RuntimeError::Unavailable(format!(
            "Silo could not save its computer settings: {error}"
        ))
    })?;
    temporary.persist(path).map_err(|error| {
        RuntimeError::Unavailable(format!(
            "Silo could not finalize its computer settings: {}",
            error.error
        ))
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            RuntimeError::Unavailable(format!(
                "Silo could not finalize its computer settings: {error}"
            ))
        })
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_migrated_inventory_and_setup_history_load() {
        let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
        let paths = migrated.runtime_paths();
        let request = read_metadata(&paths.metadata).unwrap();
        let names: Vec<_> = request.computers.iter().map(|c| c.name()).collect();
        assert_eq!(names, ["dev", "fork"]);
        assert_eq!(
            request.computers[0].id(),
            crate::runtime_migration::vocabulary_tests::ID
        );
        let desktop = request.computers[0].desktop.as_ref().unwrap();
        assert!(!desktop.start_with_computer && desktop.built_in);
        let events = read_activity(&paths, false).unwrap();
        assert_eq!(events.len(), 3);
        assert!(events
            .iter()
            .all(|event| event.phase == "computers" && event.computer == "dev"));
        assert_eq!(events[0].step, "computer-image-import");
    }
    use super::*;

    #[test]
    fn metadata_large_input_memory_is_bounded() {
        const PROBE: &str = "SILO_TEST_METADATA_MEMORY_PROBE";
        if std::env::var_os(PROBE).is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "runtime::tests::metadata_large_input_memory_is_bounded",
                    "--nocapture",
                ])
                .env(PROBE, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed;"));
            return;
        }

        fn peak_bytes() -> u64 {
            // SAFETY: getrusage initializes the supplied rusage structure.
            let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
            assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) }, 0);
            #[cfg(target_os = "macos")]
            return usage.ru_maxrss as u64;
            #[cfg(not(target_os = "macos"))]
            return usage.ru_maxrss as u64 * 1024;
        }

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("computers.json");
        fs::File::create(&path)
            .unwrap()
            .set_len(128 * 1024 * 1024)
            .unwrap();
        let before = peak_bytes();
        assert!(
            matches!(read_metadata(&path), Err(RuntimeError::Malformed(message))
            if message == "Silo's computer configuration is too large.")
        );
        let extra = peak_bytes().saturating_sub(before);
        eprintln!("large metadata peak RSS increase: {extra} bytes");
        assert!(
            extra < 32 * 1024 * 1024,
            "oversized metadata allocated {extra} bytes"
        );
        assert_eq!(fs::metadata(&path).unwrap().len(), 128 * 1024 * 1024);
    }

    #[test]
    fn auto_retry_retries_transient_failures_and_releases_the_gate_between_attempts() {
        let _test_state = crate::test_support::global_state();
        use std::sync::atomic::{AtomicUsize, Ordering};
        let attempts = AtomicUsize::new(0);
        let delays = [Duration::from_millis(5), Duration::from_millis(5)];
        let result: Result<(), RuntimeError> = gated_auto_retry_with(
            &delays,
            "Starting retry-transient",
            |label| {
                // Each attempt must find the scope free: the previous guard was released
                // before the backoff, so the gate is not held across retries.
                assert!(OPERATIONS.is_computer_idle("retry-transient-id"));
                OPERATIONS
                    .computer("retry-transient-id", "retry-transient", label)
                    .map_err(RuntimeError::from)
            },
            |_guard| {},
            || {
                if attempts.fetch_add(1, Ordering::SeqCst) < 2 {
                    Err(RuntimeError::TimedOut {
                        operation: "Starting retry-transient".into(),
                    })
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_ok());
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert!(OPERATIONS.is_computer_idle("retry-transient-id"));
    }

    #[test]
    fn cancelled_and_deduplicated_actions_are_not_notified_but_other_failures_are() {
        let _test_state = crate::test_support::global_state();
        assert_eq!(
            lifecycle_failure(&RuntimeError::Cancelled {
                operation: "Starting dev".into(),
            }),
            LifecycleFailure::Cancelled
        );
        assert_eq!(
            lifecycle_failure(&RuntimeError::from(
                operation_gate::GateError::AlreadyQueued
            )),
            LifecycleFailure::AlreadyQueued
        );
        assert_eq!(
            lifecycle_failure(&RuntimeError::Failed {
                operation: "Starting dev".into(),
                exit_code: Some(1),
                detail: "boot failed".into(),
            }),
            LifecycleFailure::Failed
        );
        assert_eq!(
            lifecycle_failure(&RuntimeError::TimedOut {
                operation: "Starting dev".into(),
            }),
            LifecycleFailure::Failed
        );
        assert_eq!(
            lifecycle_failure(&RuntimeError::Busy),
            LifecycleFailure::Failed
        );
        assert_eq!(
            lifecycle_failure(&RuntimeError::from(operation_gate::GateError::Nested)),
            LifecycleFailure::Failed
        );
    }

    #[test]
    fn auto_retry_attempts_keep_the_first_attempts_start_time() {
        let _test_state = crate::test_support::global_state();
        let gate: &'static operation_gate::OperationGate =
            Box::leak(Box::new(operation_gate::OperationGate::new()));
        let seen = Mutex::new(Vec::new());
        let delays = [Duration::from_millis(20), Duration::from_millis(20)];
        let result: Result<(), RuntimeError> = gated_auto_retry_classified(
            &delays,
            "Stopping since-dev",
            |label| {
                gate.computer("since-dev-id", "since-dev", label)
                    .map_err(RuntimeError::from)
            },
            |_guard| {},
            || {
                let mut seen = seen.lock().unwrap();
                seen.push(gate.snapshot().running[0].since_ms);
                if seen.len() < 3 {
                    Err(RuntimeError::TimedOut {
                        operation: "Stopping since-dev".into(),
                    })
                } else {
                    Ok(())
                }
            },
            transient_runtime_error,
            || RuntimeError::Cancelled {
                operation: "Stopping since-dev".into(),
            },
        );
        assert!(result.is_ok());
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.len(), 3);
        assert!(seen.iter().all(|since| *since == seen[0]), "{seen:?}");
    }

    #[test]
    fn auto_retry_does_not_resume_after_a_quit_began_even_if_it_failed() {
        let _test_state = crate::test_support::global_state();
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Reopen;
        impl Drop for Reopen {
            fn drop(&mut self) {
                shutdown::cancel();
            }
        }
        let gate: &'static operation_gate::OperationGate =
            Box::leak(Box::new(operation_gate::OperationGate::new()));
        let attempts = AtomicUsize::new(0);
        let delays = [Duration::from_millis(300), Duration::from_millis(300)];
        let result: Result<(), RuntimeError> = gated_auto_retry_classified(
            &delays,
            "Starting quit-dev",
            |label| {
                gate.computer("quit-dev-id", "quit-dev", label)
                    .map_err(RuntimeError::from)
            },
            |_guard| {},
            || {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    // Complete a failed Quit before returning the first failure. The
                    // retry must remember its generation even though admission reopened.
                    // Keep the global mutation on this test's thread under its guard.
                    let _reopen = Reopen;
                    shutdown::begin();
                }
                Err(RuntimeError::TimedOut {
                    operation: "Starting quit-dev".into(),
                })
            },
            transient_runtime_error,
            || RuntimeError::Cancelled {
                operation: "Starting quit-dev".into(),
            },
        );
        assert!(
            matches!(result, Err(RuntimeError::Cancelled { .. })),
            "{result:?}"
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(shutdown::ensure_accepting_operations().is_ok());
    }

    #[test]
    fn auto_retry_does_not_retry_non_transient_failures() {
        let _test_state = crate::test_support::global_state();
        use std::sync::atomic::{AtomicUsize, Ordering};
        let attempts = AtomicUsize::new(0);
        let delays = [Duration::from_millis(5), Duration::from_millis(5)];
        let result: Result<(), RuntimeError> = gated_auto_retry_with(
            &delays,
            "Starting retry-nontransient",
            |label| {
                OPERATIONS
                    .computer("retry-nontransient-id", "retry-nontransient", label)
                    .map_err(RuntimeError::from)
            },
            |_guard| {},
            || {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(RuntimeError::Failed {
                    operation: "Starting retry-nontransient".into(),
                    exit_code: Some(1),
                    detail: "the configuration is invalid".into(),
                })
            },
        );
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    // Remote lifecycle (`remote_ops::remote_action`) and `apply_github_identity` both drive
    // `gated_auto_retry`, whose transient classification is exercised by the two tests above.
    // The secrets path instead keeps a typed `secrets_runtime::Attempt` up to this boundary
    // and classifies with `Attempt::is_transient`; these tests cover that wiring.
    #[test]
    fn secret_updates_retry_transient_attempts_and_release_the_gate_between_attempts() {
        let _test_state = crate::test_support::global_state();
        use super::secrets_runtime::Attempt;
        use std::sync::atomic::{AtomicUsize, Ordering};
        // An isolated gate keeps the release-between-attempts assertion deterministic
        // regardless of other tests sharing the global `OPERATIONS`.
        let gate: &'static operation_gate::OperationGate =
            Box::leak(Box::new(operation_gate::OperationGate::new()));
        let attempts = AtomicUsize::new(0);
        let delays = [Duration::from_millis(5), Duration::from_millis(5)];
        let result: Result<(), Attempt> = gated_auto_retry_classified(
            &delays,
            "Saving secrets for retry-secret",
            |label| {
                // The previous guard is released before the backoff, so each attempt finds
                // the computer lane free.
                assert!(gate.is_computer_idle("retry-secret-id"));
                gate.computer("retry-secret-id", "retry-secret", label)
                    .map_err(|error| Attempt::Final(error.to_string()))
            },
            |_guard| {},
            || {
                if attempts.fetch_add(1, Ordering::SeqCst) < 2 {
                    Err(Attempt::Transient(
                        "Updating computer secrets timed out.".into(),
                    ))
                } else {
                    Ok(())
                }
            },
            Attempt::is_transient,
            || Attempt::Cancelled("Saving secrets was cancelled.".into()),
        );
        assert!(result.is_ok());
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert!(gate.is_computer_idle("retry-secret-id"));
    }

    #[test]
    fn secret_updates_do_not_retry_final_attempts() {
        let _test_state = crate::test_support::global_state();
        use super::secrets_runtime::Attempt;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let gate: &'static operation_gate::OperationGate =
            Box::leak(Box::new(operation_gate::OperationGate::new()));
        let attempts = AtomicUsize::new(0);
        let delays = [Duration::from_millis(5), Duration::from_millis(5)];
        let result: Result<(), Attempt> = gated_auto_retry_classified(
            &delays,
            "Saving secrets for final-secret",
            |label| {
                gate.computer("final-secret-id", "final-secret", label)
                    .map_err(|error| Attempt::Final(error.to_string()))
            },
            |_guard| {},
            || {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(Attempt::Final(
                    "The computer rejected the secret update.".into(),
                ))
            },
            Attempt::is_transient,
            || Attempt::Cancelled("Saving secrets was cancelled.".into()),
        );
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(gate.is_computer_idle("final-secret-id"));
    }

    #[test]
    fn retry_sequence_stops_and_carries_cancellation_across_attempts() {
        let _test_state = crate::test_support::global_state();
        use super::secrets_runtime::Attempt;
        use std::sync::atomic::{AtomicUsize, Ordering};
        // A shared gate lets a second thread cancel the running attempt by its queue id; the
        // sequence-wide token must then stop the retries and report the cancellation.
        let gate: &'static operation_gate::OperationGate =
            Box::leak(Box::new(operation_gate::OperationGate::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let delays = [Duration::from_millis(50), Duration::from_millis(50)];
        let counted = attempts.clone();
        let handle = std::thread::spawn(move || -> Result<(), Attempt> {
            gated_auto_retry_classified(
                &delays,
                "Saving secrets for cancel-secret",
                |label| {
                    gate.computer("cancel-secret-id", "cancel-secret", label)
                        .map_err(|error| Attempt::Final(error.to_string()))
                },
                |guard| guard.allow_cancel(),
                || {
                    counted.fetch_add(1, Ordering::SeqCst);
                    // Model cancellable in-flight work: stay running until cancelled (or a
                    // safety deadline) so the canceller reliably observes a running entry.
                    // Without a cancel every attempt would return this transient error.
                    let deadline = Instant::now() + Duration::from_secs(2);
                    while Instant::now() < deadline && !operation_gate::cancel_requested() {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(Attempt::Transient(
                        "Updating computer secrets timed out.".into(),
                    ))
                },
                Attempt::is_transient,
                || Attempt::Cancelled("Saving secrets was cancelled.".into()),
            )
        });
        // Cancel the first attempt while it is the running entry.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(entry) = gate.snapshot().running.first() {
                gate.cancel(entry.id).unwrap();
                break;
            }
            assert!(Instant::now() < deadline, "attempt never started running");
            std::thread::sleep(Duration::from_millis(2));
        }
        let result = handle.join().unwrap();
        assert!(matches!(result, Err(Attempt::Cancelled(_))));
        // The cancel carried over instead of running all three attempts.
        assert!(attempts.load(Ordering::SeqCst) < 3);
        assert!(gate.is_computer_idle("cancel-secret-id"));
    }

    struct StubRunner {
        outputs: Mutex<VecDeque<Result<CommandOutput, RuntimeError>>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl StubRunner {
        fn new(outputs: Vec<Result<CommandOutput, RuntimeError>>) -> Self {
            Self {
                outputs: Mutex::new(outputs.into()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn successful_json(values: Vec<Value>) -> Self {
            Self::new(
                values
                    .into_iter()
                    .map(|value| {
                        Ok(CommandOutput {
                            stdout: value.to_string(),
                            stderr: String::new(),
                        })
                    })
                    .collect(),
            )
        }
    }

    impl RuntimeRunner for StubRunner {
        fn prepare_guest_image(&self, _paths: &RuntimePaths) -> Result<String, RuntimeError> {
            Ok("ghcr.io/0xpolarzero/silo-guest:test".into())
        }

        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            self.outputs
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing stub output")
        }
    }

    #[test]
    fn noninteractive_exec_gets_no_stdin_before_the_command_separator() {
        let to_args =
            |args: &[&str]| -> Vec<String> { args.iter().map(|a| (*a).to_owned()).collect() };
        assert_eq!(
            runtime_arguments(&to_args(&[
                "exec", "dev", "--no-tty", "--quiet", "--", "sh", "-c", "true"
            ])),
            to_args(&[
                "exec",
                "dev",
                "--no-tty",
                "--quiet",
                "--no-stdin",
                "--",
                "sh",
                "-c",
                "true"
            ])
        );
        // The first separator ends the options; later ones belong to the command.
        assert_eq!(
            runtime_arguments(&to_args(&[
                "exec", "dev", "--no-tty", "--", "sh", "--", "x"
            ])),
            to_args(&[
                "exec",
                "dev",
                "--no-tty",
                "--no-stdin",
                "--",
                "sh",
                "--",
                "x"
            ])
        );
        assert_eq!(
            runtime_arguments(&to_args(&["exec", "dev", "--no-tty"])),
            to_args(&["exec", "dev", "--no-tty", "--no-stdin"])
        );
        // Interactive, already-decided, non-exec and option-free commands are untouched.
        for unchanged in [
            vec!["exec", "dev", "--tty", "--no-start"],
            vec!["exec", "dev", "--no-tty", "--no-stdin", "--", "true"],
            vec!["exec", "dev", "--", "true"],
            vec!["start", "dev", "--no-tty"],
            vec!["create", "dev", "--", "--no-tty"],
        ] {
            assert_eq!(runtime_arguments(&to_args(&unchanged)), to_args(&unchanged));
        }
    }

    #[test]
    fn the_spawned_runtime_receives_no_stdin_for_noninteractive_exec() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fs::create_dir_all(&paths.home).unwrap();
        fs::write(&paths.library, b"test").unwrap();
        crate::test_support::write_shell_script(
            &paths.executable,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$MSB_HOME/args\"\n".to_string(),
        );
        let run = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
            run_msb_with_progress(&paths, &args, Duration::from_secs(20), &|_| {})
        };
        run(&["exec", "dev", "--no-start", "--no-tty", "--", "true"]).unwrap();
        run(&["inspect", "dev"]).unwrap();
        assert_eq!(
            fs::read_to_string(paths.home.join("args")).unwrap(),
            "exec dev --no-start --no-tty --no-stdin -- true\ninspect dev\n"
        );
    }

    fn fake_lifecycle_msb(paths: &RuntimePaths, block_on: &str) {
        fs::create_dir_all(&paths.home).unwrap();
        fs::write(&paths.library, b"test").unwrap();
        fs::write(paths.home.join("state"), "Stopped").unwrap();
        crate::test_support::write_shell_script(
            &paths.executable,
            format!(
                r#"#!/bin/sh
printf '%s\n' "$1" >> "$MSB_HOME/calls"
case "$1" in
  inspect) state=$(cat "$MSB_HOME/state"); printf '{{"name":"cleanup","status":"%s","config":{{"labels":{{"silo.managed":"true"}}}},"active_config":{{}}}}\n' "$state" ;;
  start) printf Running > "$MSB_HOME/state" ;;
  restore) printf Running > "$MSB_HOME/state" ;;
  stop) printf Stopped > "$MSB_HOME/state" ;;
  exec) case "$*" in
    *'echo missing'*) cat "$MSB_HOME/account" 2>/dev/null || echo ready ;;
    *'apt-get update'*) echo 'set up' >> "$MSB_HOME/calls"
      if [ -f "$MSB_HOME/setup-fails" ]; then echo 'RuntimeError: The reserved account ID 1001 belongs to another account.' >&2; exit 1; fi
      echo ready > "$MSB_HOME/account" ;;
  esac ;;
esac
if [ "$1" = "{block_on}" ]; then touch "$MSB_HOME/blocked"; exec sleep 5; fi
"#
            ),
        );
    }

    #[test]
    fn every_boot_sets_up_the_silo_account_and_a_failed_setup_stops_the_computer() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fake_lifecycle_msb(&paths, "never");
        let run = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
            run_msb_with_progress(&paths, &args, Duration::from_secs(20), &|_| {})
        };
        let set_ups = || {
            fs::read_to_string(paths.home.join("calls"))
                .unwrap()
                .matches("set up")
                .count()
        };
        let state = || fs::read_to_string(paths.home.join("state")).unwrap();
        // An older computer is set up at its first boot; later boots only check.
        fs::write(paths.home.join("account"), "missing").unwrap();
        run(&["start", "cleanup"]).unwrap();
        run(&["stop", "cleanup"]).unwrap();
        run(&["start", "cleanup"]).unwrap();
        assert_eq!((set_ups(), state().as_str()), (1, "Running"));
        run(&["stop", "cleanup"]).unwrap();
        // A failed setup reports the guest's reason and leaves the computer stopped.
        fs::write(paths.home.join("account"), "missing").unwrap();
        fs::write(paths.home.join("setup-fails"), "").unwrap();
        let error = run(&["start", "cleanup"]).unwrap_err().to_string();
        assert!(error.contains("reserved account ID 1001"), "{error}");
        assert_eq!(state(), "Stopped");
        let error = run(&["exec", "cleanup", "--", "true"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("reserved account ID 1001"), "{error}");
        assert_eq!(state(), "Stopped");
        fs::remove_file(paths.home.join("setup-fails")).unwrap();
        // A temporary boot is set up before the guest command; a restore before it is used.
        run(&["exec", "cleanup", "--", "true"]).unwrap();
        assert_eq!((set_ups(), state().as_str()), (4, "Stopped"));
        fs::write(paths.home.join("account"), "missing").unwrap();
        run(&["restore", "source:snapshot", "--name", "cleanup"]).unwrap();
        assert_eq!((set_ups(), state().as_str()), (5, "Running"));
    }

    /// Opt-in: a new computer gets the account at creation, an older layout is moved to it by
    /// the next Start, and a setup that cannot finish leaves the computer stopped.
    #[test]
    #[ignore = "requires the packaged runtime and hardware virtualization"]
    fn live_start_sets_up_the_silo_account_of_new_and_older_computers() {
        crate::test_support::live::require_confirmation();
        let _test_state = crate::test_support::global_state();
        // The live runtime control socket requires a short root (104 bytes on macOS).
        let directory = tempfile::Builder::new()
            .prefix("silo-account-")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let paths = RuntimePaths {
            guest_image: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/guest-image"),
            executable: PathBuf::from(std::env::var("SILO_TEST_MSB").expect("packaged msb path")),
            library: PathBuf::from(
                std::env::var("SILO_TEST_LIBKRUNFW").expect("packaged library path"),
            ),
            home: directory.path().join("runtime"),
            storage_home: None,
            metadata: directory.path().join("computers.json"),
            volumes: directory.path().join("volumes"),
        };
        let name = "silo-account-proof";
        let restored = "silo-account-restored";
        struct Stop<'a>(&'a RuntimePaths);
        impl Drop for Stop<'_> {
            fn drop(&mut self) {
                for name in ["silo-account-proof", "silo-account-restored"] {
                    let args = ["stop".into(), name.into()];
                    let _ = run_msb(self.0, &args, Duration::from_secs(60));
                }
            }
        }
        let _stop = Stop(&paths);
        let device = device_resources().unwrap();
        let action =
            |action: &str| computer_action_with(&ProcessRunner, &paths, &device, action, name);
        let exec_in = |name: &str, user: &str, script: &str| {
            let args = [
                "exec",
                name,
                "--no-start",
                "--user",
                user,
                "--",
                "sh",
                "-ec",
                script,
            ];
            run_msb(&paths, &args.map(String::from), Duration::from_secs(180))
                .map(|output| output.stdout)
        };
        let exec = |user: &str, script: &str| exec_in(name, user, script);
        let moved = r#"test "$(cat /home/silo/.codex/auth.json)" = secret
test "$(head -1 /home/silo/.local/bin/tool)" = '#!/home/silo/.local/bin/python'
grep -q 'PATH=/home/silo/.local/bin' /home/silo/.bashrc
test "$(stat -c %U /workspace/project /home/silo/.codex/auth.json | sort -u)" = silo
test "$(cat /root/.codex/auth.json)" = secret
test -f /var/lib/silo/working-account.json"#;
        create_disposable_test_computer(&paths, name).unwrap();
        action("start").unwrap();
        assert_eq!(
            exec("silo", "id -un; cat /var/lib/silo/working-account.json").unwrap(),
            "silo\n{\"schemaVersion\":1,\"user\":\"silo\",\"home\":\"/home/silo\"}\n"
        );
        // Recreate the layout of an older Silo: agent state under root, no silo account.
        exec(
            "root",
            r#"mkdir -p /root/.local/bin /root/.codex
printf secret > /root/.codex/auth.json
printf '#!/root/.local/bin/python\n' > /root/.local/bin/tool
printf 'export PATH=/root/.local/bin:$PATH\n' >> /root/.bashrc
printf work > /workspace/project
userdel -rf silo
rm /etc/sudoers.d/silo /var/lib/silo/working-account.json
chown -R root:root /workspace"#,
        )
        .unwrap();
        action("stop").unwrap();
        let snapshot = ["snapshot", "create", "--sandbox", name, "older", "--quiet"];
        run_msb(
            &paths,
            &snapshot.map(String::from),
            Duration::from_secs(300),
        )
        .unwrap();
        action("start").unwrap();
        exec("root", moved).unwrap();
        assert_eq!(exec("silo", "id -un; sudo -n id -u").unwrap(), "silo\n0\n");
        // A checkpoint from before the move is set up as it is restored.
        let restore = ["restore", "silo-account-proof:older", "--name", restored];
        run_msb(&paths, &restore.map(String::from), Duration::from_secs(300)).unwrap();
        exec_in(restored, "root", moved).unwrap();
        assert_eq!(exec_in(restored, "silo", "id -un").unwrap(), "silo\n");
        // A setup that cannot finish names its reason and leaves the computer stopped.
        exec(
            "root",
            "userdel -rf silo; rm /var/lib/silo/working-account.json; useradd -u 1001 conflict",
        )
        .unwrap();
        action("stop").unwrap();
        let error = action("start").unwrap_err().to_string();
        assert!(error.contains("reserved account ID 1001"), "{error}");
        assert_eq!(
            inspect_computer(&ProcessRunner, &paths, name)
                .unwrap()
                .status,
            "Stopped"
        );
    }

    #[test]
    fn cancelling_exec_still_stops_its_temporary_boot() {
        let _test_state = crate::test_support::global_state();
        for block_on in ["start", "exec"] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            fake_lifecycle_msb(&paths, block_on);
            let guard = OPERATIONS
                .computer("cleanup-id", "cleanup", "Running a guest command")
                .unwrap();
            guard.allow_cancel();
            let token = guard.cancel_token();
            let blocked = paths.home.join("blocked");
            let canceller = thread::spawn(move || {
                while !blocked.exists() {
                    thread::sleep(Duration::from_millis(10));
                }
                token.store(true, Ordering::SeqCst);
            });
            let result = run_msb_with_progress(
                &paths,
                &["exec".into(), "cleanup".into(), "--".into(), "true".into()],
                Duration::from_secs(20),
                &|_| {},
            );
            canceller.join().unwrap();
            drop(guard);
            assert!(
                matches!(result, Err(RuntimeError::Cancelled { .. })),
                "{block_on}: {result:?}"
            );
            let calls = fs::read_to_string(paths.home.join("calls")).unwrap();
            assert_eq!(calls.lines().last(), Some("stop"), "{block_on}: {calls}");
            assert_eq!(
                fs::read_to_string(paths.home.join("state")).unwrap(),
                "Stopped",
                "{block_on}"
            );
        }
    }

    #[test]
    fn a_successful_start_with_failed_verification_or_bookkeeping_keeps_running_with_a_warning() {
        let _test_state = crate::test_support::global_state();
        use std::os::unix::fs::PermissionsExt;
        for failure in ["inspect", "record"] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            fake_lifecycle_msb(&paths, "never");
            let secrets_directory = directory.path().join("secrets");
            fs::create_dir_all(&secrets_directory).unwrap();
            // After booting, the runtime either stops answering inspect, or the secret
            // settings become unwritable so the booted secret revision cannot be recorded.
            let script = fs::read_to_string(&paths.executable).unwrap().replace(
                "  start) printf Running > \"$MSB_HOME/state\" ;;",
                &format!(
                    "  start) printf Running > \"$MSB_HOME/state\"; {} ;;",
                    if failure == "inspect" { "touch \"$MSB_HOME/inspect-fails\"".to_string() } else { format!("chmod 500 '{}'", secrets_directory.display()) }
                ),
            ).replace(
                "  inspect) state=",
                "  inspect) if [ -f \"$MSB_HOME/inspect-fails\" ] && [ \"$(cat \"$MSB_HOME/state\")\" = Running ]; then echo unavailable >&2; exit 1; fi; state=",
            );
            fs::write(&paths.executable, script).unwrap();
            crate::secrets::use_test_store(Some(secrets_directory.join("secrets.json")));
            let result = run_msb_with_progress(
                &paths,
                &["start".into(), "cleanup".into()],
                Duration::from_secs(20),
                &|_| {},
            );
            crate::secrets::use_test_store(None);
            fs::set_permissions(&secrets_directory, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(
                result.is_ok(),
                "{failure}: the successful start was reported as a failure: {result:?}"
            );
            let calls = fs::read_to_string(paths.home.join("calls")).unwrap();
            assert!(
                !calls.lines().any(|command| command == "stop"),
                "{failure}: {calls}"
            );
            assert_eq!(
                fs::read_to_string(paths.home.join("state")).unwrap(),
                "Running",
                "{failure}"
            );
            assert!(
                start_refresh_attention(&paths, "cleanup").is_some(),
                "{failure}: missing post-boot warning"
            );
        }
    }

    #[test]
    fn a_start_reports_its_steps_in_order() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fake_lifecycle_msb(&paths, "never");
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorder = seen.clone();
        let sink: LifecycleSink = std::rc::Rc::new(move |step| recorder.borrow_mut().push(step));
        let result = with_lifecycle_sink(sink, || {
            run_msb_with_progress(
                &paths,
                &["start".into(), "cleanup".into()],
                Duration::from_secs(20),
                &|_| {},
            )
        });
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            *seen.borrow(),
            vec![
                LifecycleStep::Boot,
                LifecycleStep::Network,
                LifecycleStep::Account
            ]
        );
        // Without a listener (every other caller) reporting is a no-op.
        lifecycle_step(LifecycleStep::Boot);
    }

    #[test]
    fn exec_without_start_does_not_wait_for_the_github_revision_lock() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fake_lifecycle_msb(&paths, "never");
        let access = computer_access_state(&paths.home, "cleanup").unwrap();
        let _held = access.runtime.lock().unwrap();
        let worker_paths = paths.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let result = run_msb_with_progress(
                &worker_paths,
                &[
                    "exec".into(),
                    "cleanup".into(),
                    "--no-start".into(),
                    "--".into(),
                    "true".into(),
                ],
                Duration::from_secs(5),
                &|_| {},
            );
            let _ = sender.send(result.is_ok());
        });
        assert_eq!(receiver.recv_timeout(Duration::from_secs(5)), Ok(true));
    }

    #[test]
    fn desktop_guest_configuration_preserves_computer_lifecycle_even_when_guest_fails() {
        let _test_state = crate::test_support::global_state();
        use std::os::unix::fs::PermissionsExt;
        for installed in [false, true] {
            for running in [false, true] {
                for guest_fails in [false, true] {
                    let directory = tempfile::tempdir().unwrap();
                    let paths = paths(&directory);
                    fs::create_dir_all(&paths.home).unwrap();
                    fs::write(&paths.library, b"test").unwrap();
                    fs::write(
                        paths.home.join("state"),
                        if running { "Running" } else { "Stopped" },
                    )
                    .unwrap();
                    if guest_fails {
                        fs::write(paths.home.join("fail"), b"1").unwrap();
                    }
                    fs::write(&paths.executable, r#"#!/bin/sh
set -eu
printf '%s\n' "$1" >> "$MSB_HOME/calls"
case "$1" in
  --silo-desktop-protocol) printf '1\n' ;;
  inspect) state=$(cat "$MSB_HOME/state"); printf '{"name":"desktop-preserve-test","status":"%s","config":{"labels":{"silo.managed":"true"}},"active_config":{}}\n' "$state" ;;
  start) printf Running > "$MSB_HOME/state" ;;
  stop) printf Stopped > "$MSB_HOME/state" ;;
  exec) case "$*" in *'echo missing'*) echo ready; exit ;; esac
    if [ -f "$MSB_HOME/fail" ]; then echo 'Synthetic desktop setup failure' >&2; exit 1; fi ;;
  *) echo 'Unexpected runtime operation' >&2; exit 1 ;;
esac
"#).unwrap();
                    fs::set_permissions(&paths.executable, fs::Permissions::from_mode(0o700))
                        .unwrap();
                    let old = crate::desktop::DesktopConfiguration {
                        start_with_computer: true,
                        built_in: false,
                    };
                    let desired = crate::desktop::DesktopConfiguration {
                        start_with_computer: false,
                        built_in: false,
                    };
                    let result = crate::desktop::configure_with(
                        &ProcessRunner,
                        &paths,
                        "desktop-preserve-test",
                        installed.then_some(&old),
                        &desired,
                    );
                    assert_eq!(result.is_err(), guest_fails, "{result:?}");
                    assert_eq!(
                        fs::read_to_string(paths.home.join("state")).unwrap(),
                        if running { "Running" } else { "Stopped" }
                    );
                    let calls = fs::read_to_string(paths.home.join("calls")).unwrap();
                    assert_eq!(
                        calls.lines().filter(|v| *v == "start").count(),
                        usize::from(!running)
                    );
                    assert_eq!(
                        calls.lines().filter(|v| *v == "stop").count(),
                        usize::from(!running)
                    );
                    if !installed {
                        assert_eq!(calls.lines().nth(1), Some("--silo-desktop-protocol"));
                    }
                }
            }
        }
    }

    #[test]
    fn desktop_only_configuration_does_not_stop_or_modify_running_computer() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let previous = computer();
        let mut desired = previous.clone();
        {
            let ComputerConfiguration { desktop, .. } = &mut desired;
            *desktop = Some(crate::desktop::DesktopConfiguration {
                start_with_computer: true,
                built_in: false,
            });
        }
        let runner = StubRunner::successful_json(vec![
            inspect(&paths(&dir), "Running"),
            inspect(&paths(&dir), "Running"),
            json!(1),
            json!({}),
        ]);
        update_computer(&runner, &paths(&dir), &previous, &desired).unwrap();
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[3][0], "exec");
        assert!(calls[3]
            .last()
            .unwrap()
            .contains("silo-desktop autostart true"));
    }

    #[test]
    fn desktop_configuration_removal_is_rejected_before_guest_mutation() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let desired = computer();
        let mut previous = desired.clone();
        {
            let ComputerConfiguration { desktop, .. } = &mut previous;
            *desktop = Some(crate::desktop::DesktopConfiguration {
                start_with_computer: true,
                built_in: false,
            });
        }
        let runner = StubRunner::successful_json(vec![]);
        assert!(update_computer(&runner, &paths(&dir), &previous, &desired)
            .unwrap_err()
            .to_string()
            .contains("removal"));
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn legacy_computer_configuration_does_not_enable_desktop() {
        let _test_state = crate::test_support::global_state();
        let value = serde_json::to_value(computer()).unwrap();
        assert!(value.get("desktop").is_none());
        let decoded: ComputerConfiguration = serde_json::from_value(value).unwrap();
        assert!(crate::desktop::configuration(&decoded).is_none());
    }

    #[test]
    fn dismiss_crash_persists_only_that_crash_without_mutating_the_computer() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let configuration = computer();
        write_metadata(&paths.metadata, &request(vec![configuration.clone()])).unwrap();
        let crash = json!({"name":"dev", "status":"Crashed", "updated_at":"2026-09-16T00:00:00.123Z",
            "config":{"labels":{"silo.managed":"true","silo.machine-id":configuration.id()}}});
        let runner = StubRunner::successful_json(vec![crash.clone()]);
        let inspected: InspectedSandbox = serde_json::from_value(crash.clone()).unwrap();
        assert!(matches!(
            application_computer(&paths, configuration.clone(), &inspected).state,
            ComputerState::Failed
        ));
        computer_action_with(&runner, &paths, &generous_device(), "dismiss-error", "dev").unwrap();
        assert_eq!(runner.calls.lock().unwrap().len(), 1);
        assert_eq!(runner.calls.lock().unwrap()[0][0], "inspect");
        let view = application_computer(&paths, configuration.clone(), &inspected);
        assert!(matches!(view.state, ComputerState::Stopped));
        assert!(!view.can_dismiss_error);
        let mut new_crash = crash.clone();
        new_crash["updated_at"] = json!("2026-09-16T00:00:00.456Z");
        let inspected = serde_json::from_value(new_crash).unwrap();
        assert!(matches!(
            application_computer(&paths, configuration.clone(), &inspected).state,
            ComputerState::Failed
        ));
        // Clearing before a new lifecycle attempt also prevents masking same-timestamp failures.
        crash_acknowledgement::clear(&paths, "dev").unwrap();
        let inspected = serde_json::from_value(crash).unwrap();
        assert!(matches!(
            application_computer(&paths, configuration, &inspected).state,
            ComputerState::Failed
        ));
    }

    #[test]
    fn dismiss_crash_rejects_running_unknown_and_replaced_computers() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let configuration = computer();
        write_metadata(&paths.metadata, &request(vec![configuration.clone()])).unwrap();
        for (status, id, timestamp) in [
            ("Running", configuration.id(), Some("now")),
            ("Unknown", configuration.id(), Some("now")),
            ("Crashed", "replacement", Some("now")),
            ("Crashed", configuration.id(), None),
        ] {
            let runner = StubRunner::successful_json(vec![
                json!({"name":"dev", "status":status, "updated_at":timestamp,
                "config":{"labels":{"silo.managed":"true","silo.machine-id":id}}}),
            ]);
            assert!(computer_action_with(
                &runner,
                &paths,
                &generous_device(),
                "dismiss-error",
                "dev"
            )
            .is_err());
            assert_eq!(runner.calls.lock().unwrap().len(), 1);
        }
    }

    #[test]
    fn bundled_image_preparation_reports_its_actual_stage() {
        let _test_state = crate::test_support::global_state();
        let event = computer_progress("attempt", "computer-image-preparation", "dev", 0);
        assert_eq!(event.message, "Preparing the VM image…");
    }

    #[test]
    fn first_time_image_import_is_reported_without_a_fraction() {
        let _test_state = crate::test_support::global_state();
        let event = computer_progress("attempt", "computer-image-import", "", 0);
        assert_eq!(
            event.message,
            "Importing the VM image (first time only, about a minute)…"
        );
        assert_eq!(event.fraction, None);
    }

    #[test]
    fn scoped_tokens_are_deduplicated_and_never_printed() {
        let _test_state = crate::test_support::global_state();
        let mut tokens = scoped_tokens_of(
            r#"{"version":1,"owners":[{"readToken":"ghs_read","writeToken":"ghs_write"},{"readToken":"ghs_read"}]}"#,
        )
        .unwrap();
        tokens.extend(["ghs_write".to_string(), "ghs_retained".to_string()]);
        assert!(
            tokens.contains("ghs_read")
                && tokens.contains("ghs_write")
                && tokens.contains("ghs_retained")
        );
        assert_eq!(tokens.0.len(), 3);
        let printed = format!("{tokens:?}");
        assert!(!printed.contains("ghs_"), "{printed}");
        assert!(scoped_tokens_of("not json").is_err());
    }

    #[test]
    fn github_revisions_reject_delayed_updates_but_allow_same_revision_completion() {
        let _test_state = crate::test_support::global_state();
        let mut revision = 4;
        assert!(accept_github_revision(&mut revision, 5).is_ok());
        assert!(accept_github_revision(&mut revision, 4).is_err());
        assert_eq!(revision, 5);
        assert!(accept_github_revision(&mut revision, 5).is_ok());
    }

    #[test]
    fn computer_access_is_per_computer_and_revisions_never_wait_for_runtime_work() {
        let _test_state = crate::test_support::global_state();
        let home = tempfile::tempdir().unwrap();
        let a = computer_access_state(home.path(), "a").unwrap();
        let same = computer_access_state(home.path(), "a").unwrap();
        let b = computer_access_state(home.path(), "b").unwrap();
        // A boot or access update of `a` is running.
        let _running = a.runtime.lock().unwrap();
        // Recording a newer revision does not wait for it.
        accept_github_revision(&mut same.revision.try_lock().unwrap(), 7).unwrap();
        assert!(same.runtime.try_lock().is_err());
        assert!(b.runtime.try_lock().is_ok());
        // Waiting for runtime access is bounded instead of blocking a thread forever.
        let started = Instant::now();
        assert!(matches!(
            lock_computer_runtime(&same, Duration::from_millis(60), "Test"),
            Err(RuntimeError::Busy)
        ));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// A fake runtime for GitHub access updates. `modify` records its arguments, the
    /// secret values it read on standard input and its environment, then blocks while
    /// `modify-block` exists and fails when `modify-fail` exists.
    fn fake_github_msb(paths: &RuntimePaths) {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(&paths.home).unwrap();
        fs::write(&paths.library, b"test").unwrap();
        fs::write(&paths.executable, r#"#!/bin/sh
case "$1" in
  --silo-github-protocol|--silo-github-token-protocol) echo 1 ;;
  inspect) printf '{"name":"%s","status":"Running","config":{"labels":{"silo.managed":"true","silo.machine-id":"SILO_TEST_COMPUTER_ID","silo.github-protocol":"1"}},"active_config":{}}\n' "$2" ;;
  modify)
    printf '%s\n' "$*" >> "$MSB_HOME/modify-args"
    cat > "$MSB_HOME/modify-values"
    env > "$MSB_HOME/modify-env"
    echo $$ > "$MSB_HOME/modify.pid"
    while [ -f "$MSB_HOME/modify-block" ]; do sleep 0.05; done
    if [ -f "$MSB_HOME/modify-fail" ]; then echo "rejected $(cat "$MSB_HOME/modify-values")" >&2; exit 3; fi ;;
esac
"#.replace("SILO_TEST_COMPUTER_ID", computer().id())).unwrap();
        fs::set_permissions(&paths.executable, fs::Permissions::from_mode(0o700)).unwrap();
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
    }

    fn github_profile(token: &str) -> Value {
        json!({"version":1,"owners":[{"login":"octo","repositoryIds":[1],"readToken":token,"writeToken":null,"expiresAt":1}]})
    }

    fn cached_github_profile(paths: &RuntimePaths) -> Option<String> {
        GITHUB_PROFILES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .get(&(paths.home.clone(), "dev".into()))
            .cloned()
    }

    fn wait_for(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "{} never appeared",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn process_alive(pid_file: &Path) -> bool {
        let pid: i32 = fs::read_to_string(pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[test]
    fn secret_apply_accepts_the_matching_saved_runtime_identity() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fake_github_msb(&paths);
        let store = directory.path().join("secrets.json");
        fs::write(&store, r#"{"secrets":[]}"#).unwrap();
        crate::secrets::use_test_store(Some(store));
        crate::secrets::use_test_vault(Some(Default::default()));

        let result = apply_secrets_at_paths(paths.clone(), "dev");

        crate::secrets::use_test_store(None);
        crate::secrets::use_test_vault(None);
        assert!(result.unwrap().is_empty());
        forget_github_state(&paths.home, "dev");
    }

    #[test]
    fn secret_apply_rejects_a_replaced_runtime_identity() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fake_github_msb(&paths);
        let script = fs::read_to_string(&paths.executable)
            .unwrap()
            .replace(computer().id(), "22222222-2222-4222-8222-222222222222");
        fs::write(&paths.executable, script).unwrap();
        let store = directory.path().join("secrets.json");
        fs::write(&store, r#"{"secrets":[]}"#).unwrap();
        crate::secrets::use_test_store(Some(store));
        crate::secrets::use_test_vault(Some(Default::default()));

        let result = apply_secrets_at_paths(paths.clone(), "dev");

        crate::secrets::use_test_store(None);
        crate::secrets::use_test_vault(None);
        let error = result.unwrap_err();
        assert!(error.contains("identity"), "{error}");
        assert!(!paths.home.join("modify-args").exists());
        assert!(!paths.home.join("modify-values").exists());
        forget_github_state(&paths.home, "dev");
    }

    #[test]
    fn secret_apply_rejects_a_replacement_saved_while_waiting() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fake_github_msb(&paths);
        let store = directory.path().join("secrets.json");
        fs::write(&store, r#"{"secrets":[]}"#).unwrap();
        let held = OPERATIONS.device("Replacing test computer").unwrap();
        let worker_paths = paths.clone();
        let update = thread::spawn(move || {
            crate::secrets::use_test_store(Some(store));
            crate::secrets::use_test_vault(Some(Default::default()));
            let result = apply_secrets_at_paths(worker_paths, "dev");
            crate::secrets::use_test_store(None);
            crate::secrets::use_test_vault(None);
            result
        });
        wait_for_queue(&OPERATIONS, |queue| {
            queue
                .waiting
                .iter()
                .any(|entry| entry.computer_name.as_deref() == Some("dev"))
        });
        let mut replacement = computer();
        {
            let ComputerConfiguration { id, .. } = &mut replacement;
            *id = "22222222-2222-4222-8222-222222222222".into();
        }
        write_metadata(&paths.metadata, &request(vec![replacement])).unwrap();
        drop(held);

        let error = update.join().unwrap().unwrap_err();

        assert!(error.contains("identity"), "{error}");
        assert!(!paths.home.join("modify-args").exists());
        assert!(!paths.home.join("modify-values").exists());
        forget_github_state(&paths.home, "dev");
    }

    #[test]
    fn github_update_rejects_a_replaced_runtime_identity() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fake_github_msb(&paths);
        let script = fs::read_to_string(&paths.executable)
            .unwrap()
            .replace(computer().id(), "22222222-2222-4222-8222-222222222222");
        fs::write(&paths.executable, script).unwrap();

        let error = apply_github_policy_with(
            &paths,
            "dev",
            1,
            &github_profile("synthetic-scoped-token"),
            Duration::from_secs(10),
        )
        .unwrap_err();

        assert!(error.contains("identity"), "{error}");
        assert!(!paths.home.join("modify-args").exists());
        assert!(!paths.home.join("modify-values").exists());
        assert!(cached_github_profile(&paths).is_none());
        forget_github_state(&paths.home, "dev");
    }

    #[test]
    fn github_update_rejects_a_replacement_saved_while_waiting() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fake_github_msb(&paths);
        let held = OPERATIONS.device("Replacing test computer").unwrap();
        let worker_paths = paths.clone();
        let update = thread::spawn(move || {
            apply_github_policy_with(
                &worker_paths,
                "dev",
                1,
                &github_profile("synthetic-scoped-token"),
                Duration::from_secs(10),
            )
        });
        wait_for_queue(&OPERATIONS, |queue| {
            queue.waiting.iter().any(|entry| {
                entry.kind == operation_gate::OperationKind::GithubApply
                    && entry.computer_name.as_deref() == Some("dev")
            })
        });
        let mut replacement = computer();
        {
            let ComputerConfiguration { id, .. } = &mut replacement;
            *id = "22222222-2222-4222-8222-222222222222".into();
        }
        write_metadata(&paths.metadata, &request(vec![replacement])).unwrap();
        drop(held);

        let error = update.join().unwrap().unwrap_err();

        assert!(error.contains("identity"), "{error}");
        assert!(!paths.home.join("modify-args").exists());
        assert!(!paths.home.join("modify-values").exists());
        assert!(cached_github_profile(&paths).is_none());
        forget_github_state(&paths.home, "dev");
    }

    #[test]
    fn github_update_passes_profile_and_secrets_only_on_standard_input() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fake_github_msb(&paths);
        let store = directory.path().join("secrets.json");
        fs::write(&store, r#"{"secrets":[{"id":"s1","name":"API_TOKEN","valueId":"v1","computers":["dev"],"allowedDomains":["api.example.com"]}]}"#).unwrap();
        crate::secrets::use_test_store(Some(store));
        crate::secrets::use_test_vault(Some(
            [("v1".to_string(), "secret-value".to_string())].into(),
        ));
        let profile = github_profile("ghs_scoped");
        let result = apply_github_policy_with(&paths, "dev", 1, &profile, Duration::from_secs(10));
        crate::secrets::use_test_store(None);
        crate::secrets::use_test_vault(None);
        result.unwrap();
        let args = fs::read_to_string(paths.home.join("modify-args")).unwrap();
        assert_eq!(
            args.trim(),
            format!(
                "modify dev --secret {} --format json",
                secrets_runtime::SILO_GITHUB_SECRET_SPEC
            )
        );
        assert!(!args.contains("ghs_scoped") && !args.contains("secret-value"));
        let values: Value =
            serde_json::from_str(&fs::read_to_string(paths.home.join("modify-values")).unwrap())
                .unwrap();
        assert_eq!(
            values,
            json!({"SILO_GITHUB": profile.to_string(), secrets_runtime::source_name("API_TOKEN"): "secret-value"})
        );
        // Neither the profile nor a secret, nor any secret-named variable, is in the environment.
        let environment = fs::read_to_string(paths.home.join("modify-env")).unwrap();
        assert!(environment
            .lines()
            .any(|line| line == "MSB_SECRET_VALUES_STDIN=1"));
        assert!(!environment.contains("ghs_scoped") && !environment.contains("secret-value"));
        assert!(!environment
            .lines()
            .any(|line| line.starts_with("SILO_GITHUB=") || line.starts_with("API_TOKEN=")));
        assert_eq!(cached_github_profile(&paths), Some(profile.to_string()));
        forget_github_state(&paths.home, "dev");
    }

    #[test]
    fn secret_values_document_carries_the_profile_and_each_secret_by_source_name() {
        let _test_state = crate::test_support::global_state();
        let material = vec![(
            "API_KEY".to_string(),
            "value \"quoted\"".to_string(),
            vec!["api.example.com".to_string()],
        )];
        let document: Value = serde_json::from_slice(
            &secret_values_document(&material, DISABLED_GITHUB_PROFILE).unwrap(),
        )
        .unwrap();
        assert_eq!(
            document,
            json!({"SILO_GITHUB": DISABLED_GITHUB_PROFILE, secrets_runtime::source_name("API_KEY"): "value \"quoted\""})
        );
    }

    #[test]
    fn github_update_clears_the_boot_profile_first_and_rejects_older_revisions() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fake_github_msb(&paths);
        GITHUB_PROFILES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert(
                (paths.home.clone(), "dev".into()),
                github_profile("ghs_old").to_string(),
            );
        fs::write(paths.home.join("modify-block"), "").unwrap();
        let (older, newer) = (github_profile("ghs_four"), github_profile("ghs_five"));
        let worker_paths = paths.clone();
        let first = thread::spawn(move || {
            apply_github_policy_with(&worker_paths, "dev", 4, &older, Duration::from_secs(20))
        });
        wait_for(&paths.home.join("modify.pid"));
        // While the update runs, a boot of this computer can only get the disabled profile.
        assert_eq!(
            github_environment(&paths, &["start".into(), "dev".into()]),
            DISABLED_GITHUB_PROFILE
        );
        // An older revision is rejected at once, without waiting for the running update.
        let started = Instant::now();
        assert_eq!(
            apply_github_policy_with(
                &paths,
                "dev",
                3,
                &github_profile("ghs_three"),
                Duration::from_secs(20)
            )
            .unwrap_err(),
            GITHUB_UPDATE_REPLACED
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        // A newer revision is accepted at once and applies after the running update.
        let worker_paths = paths.clone();
        let expected = newer.clone();
        let second = thread::spawn(move || {
            apply_github_policy_with(&worker_paths, "dev", 5, &newer, Duration::from_secs(20))
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while !OPERATIONS
            .snapshot()
            .waiting
            .iter()
            .any(|entry| entry.kind == operation_gate::OperationKind::GithubApply)
        {
            assert!(Instant::now() < deadline, "the newer update never queued");
            thread::sleep(Duration::from_millis(10));
        }
        fs::remove_file(paths.home.join("modify-block")).unwrap();
        // The superseded update finished, but its profile is not recorded as applied.
        assert_eq!(first.join().unwrap().unwrap_err(), GITHUB_UPDATE_REPLACED);
        second.join().unwrap().unwrap();
        assert_eq!(cached_github_profile(&paths), Some(expected.to_string()));
        assert_eq!(
            fs::read_to_string(paths.home.join("modify-args"))
                .unwrap()
                .lines()
                .count(),
            2
        );
        forget_github_state(&paths.home, "dev");
    }

    #[test]
    fn failed_github_update_keeps_no_profile_and_reports_fixed_text() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fake_github_msb(&paths);
        GITHUB_PROFILES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert(
                (paths.home.clone(), "dev".into()),
                github_profile("ghs_old").to_string(),
            );
        fs::write(paths.home.join("modify-fail"), "").unwrap();
        let error = apply_github_policy_with(
            &paths,
            "dev",
            1,
            &github_profile("ghs_new"),
            Duration::from_secs(10),
        )
        .unwrap_err();
        assert!(
            error.contains("rejected the GitHub access update"),
            "{error}"
        );
        assert!(!error.contains("ghs_"));
        assert_eq!(cached_github_profile(&paths), None);
        forget_github_state(&paths.home, "dev");
    }

    #[test]
    fn github_update_timeout_and_cancel_kill_the_runtime_child() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fake_github_msb(&paths);
        fs::write(paths.home.join("modify-block"), "").unwrap();
        let started = Instant::now();
        let error = apply_github_policy_with(
            &paths,
            "dev",
            1,
            &github_profile("ghs_slow"),
            Duration::from_secs(2),
        )
        .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(15));
        assert!(!process_alive(&paths.home.join("modify.pid")));
        assert_eq!(cached_github_profile(&paths), None);
        fs::remove_file(paths.home.join("modify.pid")).unwrap();
        // Inside a cancellable operation (for example a caller already holding the gate),
        // a cancel kills the runtime child as well.
        let outer = OPERATIONS
            .computer("github-cancel-outer", "outer", "Outer operation")
            .unwrap();
        outer.allow_cancel();
        let token = outer.cancel_token();
        let pid_file = paths.home.join("modify.pid");
        let canceller = thread::spawn(move || {
            wait_for(&pid_file);
            token.store(true, Ordering::SeqCst);
        });
        let error = apply_github_policy_with(
            &paths,
            "dev",
            2,
            &github_profile("ghs_cancel"),
            Duration::from_secs(20),
        )
        .unwrap_err();
        canceller.join().unwrap();
        drop(outer);
        assert_eq!(error, "Applying GitHub access was cancelled.");
        assert!(!process_alive(&paths.home.join("modify.pid")));
        forget_github_state(&paths.home, "dev");
    }

    #[test]
    fn github_update_waits_for_the_computers_operations_and_the_worker_lock() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fake_github_msb(&paths);
        let (release, released) = std::sync::mpsc::channel::<()>();
        let (held, holding) = std::sync::mpsc::channel();
        let holder = thread::spawn(move || {
            let _guard = OPERATIONS
                .computer(
                    "00000000-0000-4000-8000-000000000001",
                    "dev",
                    "Creating checkpoint",
                )
                .unwrap();
            held.send(()).unwrap();
            released.recv().unwrap();
        });
        holding.recv().unwrap();
        let worker_paths = paths.clone();
        let update = thread::spawn(move || {
            apply_github_policy_with(
                &worker_paths,
                "dev",
                1,
                &github_profile("ghs_wait"),
                Duration::from_secs(20),
            )
        });
        thread::sleep(Duration::from_millis(300));
        assert!(
            !paths.home.join("modify-args").exists(),
            "the update ran during another operation on its computer"
        );
        assert!(OPERATIONS.snapshot().waiting.iter().any(|entry| entry.kind
            == operation_gate::OperationKind::GithubApply
            && entry.computer_name.as_deref() == Some("dev")));
        release.send(()).unwrap();
        holder.join().unwrap();
        update.join().unwrap().unwrap();
        // Another runtime mutation holds the worker lock: the update never overlaps it.
        let lock = configuration_recovery::command_lock(&paths, Duration::from_secs(1)).unwrap();
        fs::remove_file(paths.home.join("modify-args")).unwrap();
        assert!(apply_github_policy_with(
            &paths,
            "dev",
            2,
            &github_profile("ghs_locked"),
            Duration::from_secs(1)
        )
        .is_err());
        assert!(!paths.home.join("modify-args").exists());
        drop(lock);
        forget_github_state(&paths.home, "dev");
    }

    #[test]
    fn github_environment_is_bound_to_runtime_home_and_explicit_command_target() {
        let _test_state = crate::test_support::global_state();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let paths = paths(&first);
        let other = super::tests::paths(&second);
        let cache = GITHUB_PROFILES.get_or_init(|| Mutex::new(HashMap::new()));
        cache
            .lock()
            .unwrap()
            .insert((paths.home.clone(), "exec".into()), "profile-a".into());
        cache
            .lock()
            .unwrap()
            .insert((paths.home.clone(), "dev".into()), "profile-b".into());
        let args = vec!["start".into(), "dev".into()];
        assert_eq!(github_environment(&paths, &args), "profile-b");
        assert_eq!(github_environment(&other, &args), DISABLED_GITHUB_PROFILE);
        // An exec that races a stop must never implicitly boot with a captured
        // old token. Explicit boot preparation above owns credential injection.
        assert_eq!(
            github_environment(&paths, &["exec".into(), "dev".into()]),
            DISABLED_GITHUB_PROFILE
        );
        assert_eq!(
            github_environment(&paths, &["list".into(), "dev".into()]),
            DISABLED_GITHUB_PROFILE
        );
        assert_eq!(
            github_environment(&paths, &["create".into(), "dev".into()]),
            DISABLED_GITHUB_PROFILE
        );
        cache
            .lock()
            .unwrap()
            .retain(|(home, _), _| home != &paths.home);
    }

    #[test]
    fn production_lifecycle_actions_use_the_target_computers_github_profile() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let other_directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let other = super::tests::paths(&other_directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let cache = GITHUB_PROFILES.get_or_init(|| Mutex::new(HashMap::new()));
        cache.lock().unwrap().insert(
            (paths.home.clone(), "dev".into()),
            "source-write-profile".into(),
        );
        cache
            .lock()
            .unwrap()
            .insert((paths.home.clone(), "other".into()), "other-profile".into());
        cache.lock().unwrap().insert(
            (paths.home.clone(), "fork".into()),
            "fork-read-only-profile".into(),
        );
        // Restore selects the fork's current host-side policy, never the
        // source computer's cached profile or a token captured in checkpoint RAM.
        assert_eq!(
            github_environment(
                &paths,
                &[
                    "restore".into(),
                    "dev:c000000000000000000000000000000".into(),
                    "--name".into(),
                    "fork".into()
                ]
            ),
            "fork-read-only-profile"
        );
        assert_eq!(
            github_environment(
                &paths,
                &[
                    "restore".into(),
                    "dev:c000000000000000000000000000000".into(),
                    "--name".into(),
                    "unassigned-fork".into()
                ]
            ),
            DISABLED_GITHUB_PROFILE
        );
        for action in ["start", "restart"] {
            let runner = StubRunner::successful_json(vec![
                inspect(&paths, "Stopped"),
                json!(null),
                inspect(&paths, "Running"),
            ]);
            computer_action_with(&runner, &paths, &generous_device(), action, "dev").unwrap();
            let calls = runner.calls.lock().unwrap();
            let command = &calls[1];
            assert_eq!(github_command_computer(command), Some("dev"));
            assert_eq!(github_environment(&paths, command), "source-write-profile");
            assert_eq!(github_environment(&other, command), DISABLED_GITHUB_PROFILE);
        }
        cache
            .lock()
            .unwrap()
            .retain(|(home, _), _| home != &paths.home);
    }

    #[test]
    fn activity_history_survives_restart_and_marks_only_unfinished_attempts_interrupted() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut journal = ActivityJournal::start(&paths, "attempt-1").unwrap();
        journal.append(computer_progress("attempt-1", "setup-started", "", 0));
        journal.append(computer_progress(
            "attempt-1",
            "computer-verification",
            "dev",
            0,
        ));
        assert_eq!(read_activity(&paths, false).unwrap().len(), 2);
        let recovered = read_activity(&paths, true).unwrap();
        assert_eq!(recovered.last().unwrap().step, "setup-interrupted");
        assert_eq!(recovered.last().unwrap().level, "warning");
        assert_eq!(recovered.last().unwrap().computer, "dev");
        assert!(recovered.last().unwrap().fraction.is_none());
        assert_eq!(read_activity(&paths, true).unwrap().len(), 3);
        let mut journal = ActivityJournal::start(&paths, "attempt-2").unwrap();
        journal.append(computer_progress("attempt-2", "setup-started", "", 0));
        journal.append(computer_progress("attempt-2", "setup-completed", "", 0));
        let completed = read_activity(&paths, true).unwrap();
        assert_eq!(completed.len(), 2);
        assert_eq!(completed.last().unwrap().step, "setup-completed");
    }

    #[test]
    fn setup_activity_is_interrupted_only_when_no_setup_journal_is_live() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut journal = ActivityJournal::start(&paths, "attempt").unwrap();
        journal.append(computer_progress("attempt", "setup-started", "", 0));
        journal.append(computer_progress(
            "attempt",
            "computer-configuration",
            "dev",
            0,
        ));
        // While the setup runs, its unfinished history is current, not interrupted.
        let running = read_setup_activity_at(&paths).unwrap();
        assert_eq!(running.last().unwrap().step, "computer-configuration");
        drop(journal);
        // Unrelated work, such as a launch auto-start on another computer, does not make an
        // interrupted setup look in progress.
        let _launch = OPERATIONS
            .computer("setup-activity-other-id", "other", "Starting other")
            .unwrap();
        let recovered = read_setup_activity_at(&paths).unwrap();
        assert_eq!(recovered.last().unwrap().step, "setup-interrupted");
        assert_eq!(read_activity(&paths, false).unwrap().len(), 3);
    }

    fn download_progress(bytes: u64) -> ComputerConfigurationProgress {
        let mut event = computer_progress("attempt", "image-download", "dev", 0);
        event.step = "image-download".into();
        event.fraction = None;
        event.downloaded_bytes = Some(bytes);
        event
    }

    #[test]
    fn repeated_progress_is_saved_at_stage_boundaries_and_when_the_journal_ends() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let saved_bytes = || {
            read_activity(&paths, false)
                .unwrap()
                .iter()
                .rev()
                .find(|event| event.step == "image-download")
                .and_then(|event| event.downloaded_bytes)
        };
        let mut journal = ActivityJournal::start(&paths, "attempt").unwrap();
        journal.append(computer_progress("attempt", "setup-started", "", 0));
        journal.append(download_progress(1));
        assert_eq!(saved_bytes(), Some(1), "a new stage is saved immediately");
        journal.append(download_progress(2));
        journal.append(download_progress(3));
        assert_eq!(
            saved_bytes(),
            Some(1),
            "repeated progress within a stage is throttled"
        );
        assert_eq!(
            journal.events.len(),
            2,
            "repeated progress replaces the previous update"
        );
        journal.append(computer_progress(
            "attempt",
            "computer-configuration",
            "dev",
            1,
        ));
        assert_eq!(
            saved_bytes(),
            Some(3),
            "a boundary saves the latest progress with it"
        );
        journal.append(download_progress(4));
        journal.append(download_progress(5));
        drop(journal);
        assert_eq!(
            saved_bytes(),
            Some(5),
            "the last update is saved when the journal ends"
        );
    }

    #[test]
    fn a_full_journal_keeps_its_first_event_and_drops_the_oldest_progress() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let mut journal = ActivityJournal::start(&paths(&directory), "attempt").unwrap();
        journal.append(computer_progress("attempt", "setup-started", "", 0));
        for index in 0..MAX_ACTIVITY_EVENTS {
            let name = format!("computer{index}");
            journal.append(computer_progress(
                "attempt",
                "computer-configuration",
                &name,
                0,
            ));
        }
        assert_eq!(journal.events.len(), MAX_ACTIVITY_EVENTS);
        assert_eq!(journal.events[0].step, "setup-started");
        assert_eq!(journal.events[1].computer, "computer1");
        assert_eq!(
            journal.events.back().unwrap().computer,
            format!("computer{}", MAX_ACTIVITY_EVENTS - 1)
        );
    }

    #[test]
    fn activity_does_not_publish_private_runtime_error_details() {
        let _test_state = crate::test_support::global_state();
        let error = RuntimeError::Failed { operation: "Creating the computer".into(), exit_code: None, detail: "error sending request https://user:SECRET@registry.test/image?token=SECRET /Users/alice/private".into() };
        let safe = safe_activity_error(&error);
        assert!(safe.contains("registry could not be reached"));
        for private in ["SECRET", "alice", "registry.test", "token"] {
            assert!(!safe.contains(private));
        }
    }

    #[test]
    fn activity_reports_history_write_failure_without_losing_the_operation() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let mut journal = ActivityJournal::start(&paths(&directory), "attempt").unwrap();
        journal.path = directory.path().join("missing-parent/file/activity.json");
        fs::write(directory.path().join("missing-parent"), "blocked").unwrap();
        let event = journal.append(computer_progress(
            "attempt",
            "computer-verification",
            "dev",
            1,
        ));
        assert_eq!(event.fraction, Some(1));
        assert_eq!(
            journal.events.back().unwrap().step,
            "activity-storage-warning"
        );
        assert!(journal.events.iter().any(|event| event.fraction == Some(1)));
    }

    #[test]
    fn structured_progress_is_drained_on_exit_and_ignores_untrusted_text() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fs::write(&paths.library, "test").unwrap();
        crate::test_support::write_shell_script(&paths.executable, "#!/bin/sh\nprintf '%s\\n' 'private token=SECRET' '{\"type\":\"silo-progress\",\"phase\":\"image-download\",\"layerIndex\":0,\"downloadedBytes\":7,\"totalBytes\":9}' '{\"type\":\"silo-progress\",\"phase\":\"image-ready\"}' >&2\n");
        let events = Mutex::new(Vec::new());
        let publish = |event| events.lock().unwrap().push(event);
        SetupRunner {
            request_id: "attempt",
            publish: &publish,
        }
        .run(
            &paths,
            &[
                "create".into(),
                "--name".into(),
                "dev".into(),
                "--progress-json".into(),
            ],
            Duration::from_secs(2),
        )
        .unwrap();
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].downloaded_bytes, Some(7));
        assert_eq!(events[1].step, "image-ready");
        assert!(events.iter().all(|event| !event.message.contains("SECRET")));
    }

    #[test]
    fn activity_preserves_safe_failure_categories_and_rejects_modified_history() {
        let _test_state = crate::test_support::global_state();
        for (detail, expected) in [
            ("401 Unauthorized SECRET", "authentication"),
            ("403 forbidden SECRET", "denied access"),
            ("digest mismatch SECRET", "integrity check"),
            ("no space left SECRET", "free disk space"),
        ] {
            let safe = safe_activity_error(&RuntimeError::Failed {
                operation: "Creating the computer".into(),
                exit_code: Some(1),
                detail: detail.into(),
            });
            assert!(safe.contains(expected));
            assert!(!safe.contains("SECRET"));
        }
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut journal = ActivityJournal::start(&paths, "attempt").unwrap();
        let mut event = computer_progress("attempt", "setup-completed", "", 0);
        event.message = "SECRET arbitrary persisted text".into();
        journal.append(event);
        assert_eq!(
            read_activity(&paths, true).unwrap()[0].message,
            "Computer setup completed."
        );
        journal.events[0].computer = "https://SECRET".into();
        journal.persist().unwrap();
        assert!(read_activity(&paths, true).is_err());
    }

    #[test]
    fn failed_activity_keeps_typed_reason_and_exit_code_across_restart() {
        let _test_state = crate::test_support::global_state();
        let cases = [
            (
                RuntimeError::Failed {
                    operation: "Creating the computer".into(),
                    exit_code: Some(17),
                    detail: "401 unauthorized SECRET".into(),
                },
                "auth",
                "authentication",
                Some(17),
            ),
            (
                RuntimeError::Failed {
                    operation: "Creating the computer".into(),
                    exit_code: Some(13),
                    detail: "Permission denied /private/SECRET".into(),
                },
                "permission",
                "Permission was denied",
                Some(13),
            ),
            (
                RuntimeError::Invalid("Computer dev has an invalid CPU limit or ceiling.".into()),
                "resources",
                "CPU, memory, or storage",
                None,
            ),
            (
                RuntimeError::Failed {
                    operation: "Creating the computer".into(),
                    exit_code: Some(29),
                    detail: "unexpected SECRET".into(),
                },
                "runtime",
                "did not complete",
                Some(29),
            ),
        ];
        for (error, code, message, exit_code) in cases {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            let mut journal = ActivityJournal::start(&paths, "attempt").unwrap();
            let report = failure_report(&error);
            let mut event = computer_progress("attempt", "setup-failed", "dev", 0);
            event.failure_code = Some(report.code.into());
            event.exit_code = report.exit_code;
            event.diagnostic = report.diagnostic;
            event.level = "error".into();
            event.message = "untrusted SECRET must never be shown".into();
            journal.append(event);
            let recovered = read_activity(&paths, true).unwrap();
            let event = recovered.last().unwrap();
            assert_eq!(event.failure_code.as_deref(), Some(code));
            assert_eq!(event.exit_code, exit_code);
            assert!(event.message.contains(message));
            assert!(!event.message.contains("SECRET"));
            // The one-line message never carries an exit code; the diagnostic does.
            assert!(!event.message.contains("exit code"), "{}", event.message);
            if let Some(code) = exit_code {
                assert!(event
                    .diagnostic
                    .as_deref()
                    .unwrap()
                    .starts_with(&format!("Exit code {code}")));
            } else {
                assert!(event.diagnostic.is_none());
            }
        }
        // A modified history cannot place a diagnostic on another kind of event, and
        // a stored diagnostic is filtered again when read.
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut journal = ActivityJournal::start(&paths, "attempt").unwrap();
        let mut started = computer_progress("attempt", "setup-started", "", 0);
        started.diagnostic = Some("planted".into());
        journal.append(started);
        let mut failed = computer_progress("attempt", "setup-failed", "dev", 0);
        failed.failure_code = Some("runtime".into());
        failed.diagnostic = Some("Exit code 1\nTOKEN=planted-secret\nkept line".into());
        journal.append(failed);
        let recovered = read_activity(&paths, false).unwrap();
        assert!(recovered[0].diagnostic.is_none());
        let diagnostic = recovered[1].diagnostic.as_deref().unwrap();
        assert!(diagnostic.contains("kept line") && !diagnostic.contains("planted-secret"));
    }

    #[test]
    fn setup_activity_keeps_diagnostics_and_partial_guidance_in_application_state() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut event = computer_progress("attempt", "setup-failed", "dev", 0);
        event.failure_code = Some("permission".into());
        event.diagnostic = Some("Exit code 13\nPermission denied".into());
        event.level = "error".into();
        let mut encoded = serde_json::to_value(&event).unwrap();
        encoded["partial"] = json!(true);
        let event = serde_json::from_value(encoded).unwrap();
        let mut journal = ActivityJournal::start(&paths, "attempt").unwrap();
        journal.append(event);
        let history = read_activity(&paths, false).unwrap();
        assert!(history[0].message.ends_with(PARTIAL_CHANGES_KEPT));
        let activities = runtime_activity::read(&paths).unwrap();
        assert_eq!(
            activities[0]["diagnostic"],
            "Exit code 13\nPermission denied"
        );
        assert_eq!(activities[0]["partial"], true);
        assert!(!activities[0]["title"]
            .as_str()
            .unwrap()
            .contains("Exit code"));
    }

    #[test]
    fn partial_configuration_failure_keeps_the_precise_error_and_guidance() {
        let _test_state = crate::test_support::global_state();
        let precise = RuntimeError::Invalid(
            "Computer 'second' is not owned by Silo. No computer operation was performed.".into(),
        );
        let report = failure_report(&RuntimeError::Partial(Box::new(precise)));
        assert_eq!(report.code, "configuration");
        assert_eq!(report.summary, format!("Computer 'second' is not owned by Silo. No computer operation was performed. {PARTIAL_CHANGES_KEPT}"));
        let runtime = RuntimeError::Partial(Box::new(RuntimeError::Failed {
            operation: "Creating the computer".into(),
            exit_code: Some(7),
            detail: "no space left on device".into(),
        }));
        let report = failure_report(&runtime);
        assert_eq!(report.code, "disk");
        assert!(
            report.summary.contains("Not enough free disk space")
                && report.summary.ends_with(PARTIAL_CHANGES_KEPT)
        );
        assert_eq!(report.exit_code, Some(7));
        assert_eq!(runtime.to_string(), report.summary);
        // One classifier: a spawn failure is transient and is reported as unavailable.
        let launch = RuntimeError::Launch("Silo could not start its bundled runtime: busy".into());
        assert!(transient_runtime_error(&launch));
        assert_eq!(failure_report(&launch).code, "unavailable");
        assert!(!transient_runtime_error(&RuntimeError::Unavailable(
            "Silo could not start its bundled runtime".into()
        )));
    }

    #[test]
    fn user_facing_failures_never_name_exit_codes_or_workers() {
        let _test_state = crate::test_support::global_state();
        let error = RuntimeError::Failed {
            operation: "Starting the computer".into(),
            exit_code: Some(3),
            detail: "boom".into(),
        };
        for text in [
            error.to_string(),
            safe_activity_error(&error),
            setup_failure_message("runtime").unwrap(),
            internal_failure("reading computer state"),
            safe_activity_error(&RuntimeError::Launch(
                "could not launch: worker failure (exit code 2)".into(),
            )),
        ] {
            let lower = text.to_lowercase();
            assert!(
                !lower.contains("exit code") && !lower.contains("worker") && !text.contains('\n'),
                "{text}"
            );
        }
    }

    #[test]
    fn interruption_recovery_keeps_a_full_journal_within_its_bound() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut journal = ActivityJournal::start(&paths, "attempt").unwrap();
        journal.events = (0..512)
            .map(|_| computer_progress("attempt", "computer-verification", "dev", 0))
            .collect();
        journal.persist().unwrap();
        assert_eq!(read_activity(&paths, true).unwrap().len(), 512);
        assert_eq!(read_activity(&paths, true).unwrap().len(), 512);
    }

    #[test]
    fn structured_byte_counts_cannot_change_runtime_error_classification() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fs::write(&paths.library, "test").unwrap();
        crate::test_support::write_shell_script(&paths.executable, "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"silo-progress\",\"phase\":\"image-download\",\"layerIndex\":0,\"downloadedBytes\":40123,\"totalBytes\":40399}' 'DNS lookup failed' >&2\nexit 1\n");
        let error = run_msb(
            &paths,
            &["create".into(), "--progress-json".into()],
            Duration::from_secs(2),
        )
        .unwrap_err();
        assert_eq!(failure_report(&error).code, "network");
        assert_eq!(failure_report(&error).exit_code, Some(1));
        assert!(safe_activity_error(&error).contains("could not be reached"));
        assert!(!error.to_string().contains("40123"));
        assert!(!error.to_string().contains("DNS lookup failed"));
        for detail in [
            "DNS failure for item40123",
            "downloaded 40399 bytes then DNS failure",
        ] {
            assert_eq!(
                failure_report(&RuntimeError::Failed {
                    operation: "Creating".into(),
                    exit_code: None,
                    detail: detail.into()
                })
                .code,
                "network"
            );
        }
        assert!(mentions_http_status("http status: 401", "401"));
        assert!(mentions_http_status("status code 403", "403"));
        assert!(!mentions_http_status("http 40123", "401"));
    }

    #[test]
    fn activity_contract_matches_frontend_fixture() {
        let _test_state = crate::test_support::global_state();
        let mut started = computer_progress("attempt-1", "setup-started", "", 0);
        started.timestamp = 1_700_000_000_000;
        let mut completed = computer_progress("attempt-1", "setup-completed", "", 0);
        completed.timestamp = 1_700_000_001_000;
        completed.elapsed_seconds = 1;
        let mut failed = computer_progress("attempt-2", "setup-failed", "dev", 0);
        failed.timestamp = 1_700_000_002_000;
        failed.elapsed_seconds = 2;
        failed.level = "error".into();
        failed.failure_code = Some("permission".into());
        failed.exit_code = Some(13);
        failed.message = setup_failure_message("permission").unwrap();
        failed.diagnostic = Some("Exit code 13\nPermission denied".into());
        let events = vec![started, completed, failed];
        let fixture: Value =
            serde_json::from_str(include_str!("../../src/test/contracts/setup-activity.json"))
                .unwrap();
        assert_eq!(serde_json::to_value(events).unwrap(), fixture);
    }

    pub(super) fn paths(directory: &tempfile::TempDir) -> RuntimePaths {
        crate::test_support::paths(directory.path())
    }

    pub(super) fn computer() -> ComputerConfiguration {
        ComputerConfiguration {
            id: "00000000-0000-4000-8000-000000000001".into(),
            name: "dev".into(),
            cpus: 4,
            max_cpus: 6,
            memory_gib: 16,
            max_memory_gib: 32,
            workspace_storage_gib: 60,
            runtime_storage_gib: 80,
            desktop: None,
        }
    }

    pub(super) fn request(computers: Vec<ComputerConfiguration>) -> ComputerConfigurationRequest {
        ComputerConfigurationRequest {
            schema_version: 1,
            computers,
        }
    }

    fn generous_device() -> DeviceResources {
        DeviceResources {
            logical_cpus: 64,
            physical_memory_bytes: Some(256 * 1024 * 1024 * 1024),
        }
    }

    fn inspect(_paths: &RuntimePaths, status: &str) -> Value {
        json!({
            "name": "dev",
            "status": status,
            "config": {
                "name": "dev",
                "image": {"Oci": {"reference": "ubuntu", "root_disk": {"kind": "managed", "size_mib": 81920}}},
                "resources": {"cpus": 4, "max_cpus": 6, "memory_mib": 16384, "max_memory_mib": 32768},
                "labels": {"silo.managed": "true", "silo.machine-id": computer().id()},
                "mounts": [{
                    "type":"Owned", "guest":WORKSPACE_MOUNT,
                    "storage":{"kind":"disk","capacity_mib":61440}
                }]
            },
            "active_config": null,
            "pending_changes": []
        })
    }

    fn inspect_owned_workspace(paths: &RuntimePaths, status: &str) -> Value {
        inspect(paths, status)
    }

    #[test]
    fn configuration_recovery_adopts_only_the_created_computer_with_the_saved_id() {
        let _test_state = crate::test_support::global_state();
        let gate = operation_gate::OperationGate::new();
        let _guard = gate.device("Recovering test configuration").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let candidate = request(vec![computer()]);
        configuration_recovery::begin(&paths, &candidate).unwrap();
        configuration_recovery::claim(&paths, &computer()).unwrap();
        let mut actual = inspect(&paths, "Created");
        actual["config"]["labels"]["silo.machine-id"] = json!(computer().id());
        let runner = StubRunner::successful_json(vec![
            json!([{"name":"dev","status":"Created","image":"ubuntu"}]),
            actual.clone(),
            actual.clone(),
            json!(null),
            actual,
        ]);
        configuration_recovery::recover_at_paths(
            &runner,
            &paths,
            &generous_device(),
            &|_, _, _| {},
        )
        .unwrap();
        assert_eq!(read_metadata(&paths.metadata).unwrap(), candidate);
        assert!(!paths
            .metadata
            .with_file_name("configuration-operation.json")
            .exists());
        assert!(!paths.volumes.join("dev/.silo-configuration-owner").exists());
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args[0] == "create" || args[0] == "remove"));
    }

    #[test]
    fn recovery_normalizes_a_journaled_creation_from_before_the_built_in_desktop() {
        let _test_state = crate::test_support::global_state();
        let _v4 = guest_image::pin_test_version("ubuntu-24.04-v4");
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        // A 0.7.4-era journal: an unfinished creation with no desktop settings.
        let old = request(vec![computer()]);
        assert!(configuration_recovery::begin(&paths, &old).is_ok());
        let journal = configuration_recovery::load(&paths).unwrap().unwrap();
        // No computer exists in the runtime yet: the creation is still to be done.
        let runner = StubRunner::successful_json(vec![json!([])]);
        let journal =
            configuration_recovery::normalize_desktop_intent(&runner, &paths, journal).unwrap();
        assert!(crate::computer_use::is_built_in(
            &journal.request.computers[0]
        ));
        // Replay defaults the same way and must now match the saved intent.
        let mut replayed = old.clone();
        apply_desktop_defaults(&journal.previous, &mut replayed);
        configuration_recovery::begin(&paths, &replayed).unwrap();
        let saved = configuration_recovery::pending_request(&paths)
            .unwrap()
            .unwrap();
        assert_eq!(saved, replayed);
    }

    #[test]
    fn recovery_keeps_the_settings_of_a_partially_created_computer_that_already_exists() {
        let _test_state = crate::test_support::global_state();
        let _v4 = guest_image::pin_test_version("ubuntu-24.04-v4");
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        // An older Silo started creating `dev` from the v3 image; its computer exists in
        // the runtime, the metadata commit never happened, and the journal has no desktop.
        let old = request(vec![computer()]);
        configuration_recovery::begin(&paths, &old).unwrap();
        let journal = configuration_recovery::load(&paths).unwrap().unwrap();
        let runner = StubRunner::successful_json(vec![json!([
            {"name":"dev","status":"Created","image":"ubuntu"}
        ])]);
        let journal =
            configuration_recovery::normalize_desktop_intent(&runner, &paths, journal).unwrap();
        // The v4 defaults are for creations still to be done: this one is adopted as it is.
        assert_eq!(journal.request, old);
        assert!(!crate::computer_use::is_built_in(
            &journal.request.computers[0]
        ));
        assert_eq!(
            configuration_recovery::pending_request(&paths).unwrap(),
            Some(old.clone())
        );
        // With no computer in the runtime, creation is still necessary and the defaults apply.
        let runner = StubRunner::successful_json(vec![json!([])]);
        let journal = configuration_recovery::load(&paths).unwrap().unwrap();
        let journal =
            configuration_recovery::normalize_desktop_intent(&runner, &paths, journal).unwrap();
        assert!(crate::computer_use::is_built_in(
            &journal.request.computers[0]
        ));
    }

    #[test]
    fn recovery_adopts_a_partially_created_v3_computer_without_making_it_built_in() {
        let _test_state = crate::test_support::global_state();
        let _v4 = guest_image::pin_test_version("ubuntu-24.04-v4");
        let gate = operation_gate::OperationGate::new();
        let _guard = gate.device("Recovering test configuration").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let candidate = request(vec![computer()]);
        configuration_recovery::begin(&paths, &candidate).unwrap();
        configuration_recovery::claim(&paths, &computer()).unwrap();
        let mut actual = inspect(&paths, "Created");
        actual["config"]["labels"]["silo.machine-id"] = json!(computer().id());
        let listed = json!([{"name":"dev","status":"Created","image":"ubuntu"}]);
        let runner = StubRunner::successful_json(vec![
            // The normalization asks which computers exist, then reconciliation adopts.
            listed.clone(),
            listed,
            actual.clone(),
            actual.clone(),
            json!(null),
            actual,
        ]);
        configuration_recovery::recover_at_paths(
            &runner,
            &paths,
            &generous_device(),
            &|_, _, _| {},
        )
        .unwrap();
        let committed = read_metadata(&paths.metadata).unwrap();
        assert_eq!(committed, candidate);
        assert!(!crate::computer_use::is_built_in(&committed.computers[0]));
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args[0] == "create" || args[0] == "remove"));
    }

    #[test]
    fn an_older_controller_cannot_stop_a_built_in_desktop_from_starting_with_its_computer() {
        let built_in = |start| {
            let mut configuration = computer();
            {
                let ComputerConfiguration { desktop, .. } = &mut configuration;
                *desktop = Some(crate::desktop::DesktopConfiguration {
                    start_with_computer: start,
                    built_in: true,
                });
            }
            configuration
        };
        let legacy = |start| {
            let mut configuration = computer();
            {
                let ComputerConfiguration { desktop, .. } = &mut configuration;
                *desktop = Some(crate::desktop::DesktopConfiguration {
                    start_with_computer: start,
                    built_in: false,
                });
            }
            configuration
        };
        let started = |computers: &[ComputerConfiguration]| {
            crate::desktop::configuration(&computers[0])
                .map(|d| (d.start_with_computer, d.built_in))
        };
        // The older controller does not know `builtIn`: its expected and replacement carry
        // `false` and `startWithComputer: false`.
        let mut computers = vec![built_in(true)];
        change_computer(
            &mut computers,
            computer().id(),
            Some(&legacy(true)),
            Some(&legacy(false)),
        )
        .unwrap();
        assert_eq!(started(&computers), Some((true, true)));
        // A controller that knows the field cannot turn it off either.
        change_computer(
            &mut computers,
            computer().id(),
            Some(&built_in(true)),
            Some(&built_in(false)),
        )
        .unwrap();
        assert_eq!(started(&computers), Some((true, true)));
        // A legacy computer keeps its manual choice.
        let mut computers = vec![legacy(true)];
        change_computer(
            &mut computers,
            computer().id(),
            Some(&legacy(true)),
            Some(&legacy(false)),
        )
        .unwrap();
        assert_eq!(started(&computers), Some((false, false)));
    }

    #[test]
    fn tests_run_as_a_v3_image_unless_they_pin_another_version() {
        let defaulted = || {
            let mut request = request(vec![computer()]);
            apply_desktop_defaults(&request_without_computers(), &mut request);
            request.computers[0].clone()
        };
        // Whatever image the lock pins, the default is a plain v3 computer.
        let plain = defaulted();
        assert!(!crate::computer_use::is_built_in(&plain), "{plain:?}");
        {
            let _v4 = guest_image::pin_test_version("ubuntu-24.04-v4");
            let built_in = defaulted();
            assert!(crate::computer_use::is_built_in(&built_in), "{built_in:?}");
        }
        let plain = defaulted();
        assert!(!crate::computer_use::is_built_in(&plain), "{plain:?}");
    }

    fn request_without_computers() -> ComputerConfigurationRequest {
        request(vec![])
    }

    #[test]
    fn resubmitted_failed_creation_without_desktop_matches_the_journaled_request() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let previous = request(vec![]);
        // The first attempt journaled the defaulted request, then failed.
        let mut first = request(vec![computer()]);
        apply_desktop_defaults_for(Some("ubuntu-24.04-v4"), &previous, &mut first);
        configuration_recovery::begin(&paths, &first).unwrap();
        configuration_recovery::claim(&paths, &first.computers[0]).unwrap();
        // The same creation is resubmitted without a desktop choice.
        let mut again = request(vec![computer()]);
        apply_desktop_defaults_for(Some("ubuntu-24.04-v4"), &previous, &mut again);
        assert_eq!(again, first);
        // Without the defaulting the journal would reject the resubmission.
        assert!(configuration_recovery::begin(&paths, &request(vec![computer()])).is_err());
        let runner = StubRunner::successful_json(vec![json!([]), json!([])]);
        configuration_recovery::prepare_retry(&runner, &paths, Some(&again)).unwrap();
        configuration_recovery::begin(&paths, &again).unwrap();
        // A second recorded retry keeps working.
        configuration_recovery::prepare_retry(&runner, &paths, Some(&again)).unwrap();
        configuration_recovery::begin(&paths, &again).unwrap();
    }

    #[test]
    fn interrupted_desktop_creation_is_retried_before_metadata_adoption() {
        let _test_state = crate::test_support::global_state();
        let gate = operation_gate::OperationGate::new();
        let _guard = gate.device("Recovering test configuration").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut configuration = computer();
        {
            let ComputerConfiguration { desktop, .. } = &mut configuration;
            *desktop = Some(crate::desktop::DesktopConfiguration {
                start_with_computer: false,
                built_in: false,
            });
        }
        let candidate = request(vec![configuration.clone()]);
        configuration_recovery::begin(&paths, &candidate).unwrap();
        configuration_recovery::claim(&paths, &configuration).unwrap();
        let mut actual = inspect(&paths, "Stopped");
        actual["config"]["labels"]["silo.machine-id"] = json!(configuration.id());
        let outputs = || {
            vec![
                json!([{"name":"dev","status":"Stopped","image":"ubuntu"}]),
                actual.clone(),
                actual.clone(),
                json!(null),
                actual.clone(),
                actual.clone(),
                json!(1),
            ]
        };
        let mut failure_outputs: Vec<_> = outputs()
            .into_iter()
            .map(|v| {
                Ok(CommandOutput {
                    stdout: v.to_string(),
                    stderr: String::new(),
                })
            })
            .collect();
        failure_outputs.push(Err(RuntimeError::Unavailable(
            "Desktop download interrupted".into(),
        )));
        let interrupted = StubRunner::new(failure_outputs);
        assert!(configuration_recovery::recover_at_paths(
            &interrupted,
            &paths,
            &generous_device(),
            &|_, _, _| {}
        )
        .is_err());
        assert!(read_metadata(&paths.metadata).unwrap().computers.is_empty());
        assert!(paths
            .metadata
            .with_file_name("configuration-operation.json")
            .exists());
        let mut success = outputs();
        success.push(json!(null));
        success.push(actual);
        let retry = StubRunner::successful_json(success);
        configuration_recovery::recover_at_paths(&retry, &paths, &generous_device(), &|_, _, _| {})
            .unwrap();
        assert_eq!(read_metadata(&paths.metadata).unwrap(), candidate);
        assert!(!paths
            .metadata
            .with_file_name("configuration-operation.json")
            .exists());
        assert!(!retry
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| matches!(args[0].as_str(), "create" | "remove")));
    }

    #[test]
    fn configuration_recovery_verifies_an_edit_committed_before_interruption() {
        let _test_state = crate::test_support::global_state();
        let gate = operation_gate::OperationGate::new();
        let _guard = gate.device("Recovering test configuration").unwrap();
        for (valid, retry) in [(false, false), (true, false), (false, true), (true, true)] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
            let mut edited = computer();
            {
                let ComputerConfiguration { cpus, .. } = &mut edited;
                *cpus = 5;
            }
            let candidate = request(vec![edited]);
            configuration_recovery::begin(&paths, &candidate).unwrap();
            // Simulate termination between writing metadata and verifying runtime.
            write_metadata(&paths.metadata, &candidate).unwrap();
            let mut actual = inspect(&paths, "Stopped");
            actual["config"]["labels"]["silo.machine-id"] = json!(computer().id());
            if valid {
                actual["config"]["resources"]["cpus"] = json!(5);
            }
            let runner = StubRunner::successful_json(vec![
                json!([{"name":"dev","status":"Stopped","image":"ubuntu"}]),
                actual.clone(),
                actual,
            ]);
            let result = if retry {
                configuration_recovery::prepare_retry(&runner, &paths, Some(&candidate))
            } else {
                configuration_recovery::recover_at_paths(
                    &runner,
                    &paths,
                    &generous_device(),
                    &|_, _, _| {},
                )
            };
            assert_eq!(result.is_ok(), valid);
            assert_eq!(
                paths
                    .metadata
                    .with_file_name("configuration-operation.json")
                    .exists(),
                retry || !valid
            );
            assert_eq!(read_metadata(&paths.metadata).unwrap(), candidate);
            assert!(!runner
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|args| matches!(args[0].as_str(), "create" | "modify" | "remove")));
        }
    }

    #[test]
    fn configuration_retry_can_correct_failed_creation_and_preserve_completed_work() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        configuration_recovery::begin(&paths, &request(vec![computer()])).unwrap();
        // The computer failed while preparing its disk before its metadata committed.
        write_metadata(&paths.metadata, &request(Vec::new())).unwrap();
        configuration_recovery::claim(&paths, &computer()).unwrap();
        fs::write(disk_path(&paths, "dev", "workspace"), b"incomplete").unwrap();
        let mut corrected = computer();
        corrected.memory_gib = 8;
        let revised = request(vec![corrected]);
        let runner = StubRunner::successful_json(vec![json!([])]);
        configuration_recovery::prepare_retry(&runner, &paths, Some(&revised)).unwrap();
        assert_eq!(read_metadata(&paths.metadata).unwrap(), request(Vec::new()));
        assert!(!paths.volumes.join("dev").exists());
        let journal: Value = serde_json::from_slice(
            &fs::read(
                paths
                    .metadata
                    .with_file_name("configuration-operation.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(journal["request"], serde_json::to_value(&revised).unwrap());
        assert_eq!(
            journal["previous"],
            serde_json::to_value(read_metadata(&paths.metadata).unwrap()).unwrap()
        );
        // No relaunch is needed and the replaced durable request is accepted.
        configuration_recovery::begin(&paths, &revised).unwrap();
    }

    #[test]
    fn configuration_adoption_releases_worker_lock_before_guest_verification() {
        let _test_state = crate::test_support::global_state();
        let gate = operation_gate::OperationGate::new();
        let _guard = gate.device("Recovering test configuration").unwrap();
        struct LockAwareRunner(StubRunner);
        impl RuntimeRunner for LockAwareRunner {
            fn run(
                &self,
                paths: &RuntimePaths,
                args: &[String],
                timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                if args[0] == "exec" {
                    // Real guest verification may perform a temporary start/stop.
                    // It must be able to acquire the runtime child lock itself.
                    drop(configuration_recovery::command_lock(paths, Duration::ZERO)?);
                }
                self.0.run(paths, args, timeout)
            }
        }
        for retry in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            let candidate = request(vec![computer()]);
            configuration_recovery::begin(&paths, &candidate).unwrap();
            configuration_recovery::claim(&paths, &computer()).unwrap();
            let mut actual = inspect(&paths, "Created");
            actual["config"]["labels"]["silo.machine-id"] = json!(computer().id());
            let runner = LockAwareRunner(StubRunner::successful_json(vec![
                json!([{"name":"dev","status":"Created","image":"ubuntu"}]),
                actual.clone(),
                actual.clone(),
                json!(null),
                actual,
            ]));
            if retry {
                configuration_recovery::prepare_retry(&runner, &paths, Some(&candidate)).unwrap();
            } else {
                configuration_recovery::recover_at_paths(
                    &runner,
                    &paths,
                    &generous_device(),
                    &|_, _, _| {},
                )
                .unwrap();
            }
            assert_eq!(read_metadata(&paths.metadata).unwrap(), candidate);
            assert!(runner
                .0
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|args| args[0] == "exec"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn successful_start_survives_failed_post_boot_inspection() {
        let _test_state = crate::test_support::global_state();
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fs::create_dir_all(&paths.home).unwrap();
        fs::write(&paths.library, b"test library").unwrap();
        fs::write(
            &paths.executable,
            r#"#!/bin/sh
printf '%s\n' "$1" >> "$MSB_HOME/commands"
case "$1" in
  inspect)
    if [ -f "$MSB_HOME/started" ]; then exit 9; fi
    printf '{"name":"dev","status":"Stopped","config":{"labels":{"silo.managed":"true"}}}\n' ;;
  start) cat >/dev/null; touch "$MSB_HOME/started" ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&paths.executable, fs::Permissions::from_mode(0o700)).unwrap();
        let output = run_msb_process(
            &paths,
            &["start".into(), "dev".into()],
            Duration::from_secs(5),
            &|_| {},
        );
        assert!(output.is_ok(), "{output:?}");
        let commands = fs::read_to_string(paths.home.join("commands")).unwrap();
        assert!(commands.ends_with("start\ninspect\n"), "{commands}");
        assert!(!commands.lines().any(|command| command == "stop"));
        let row = application_computer(
            &paths,
            computer(),
            &serde_json::from_value(inspect(&paths, "Running")).unwrap(),
        );
        assert!(matches!(row.state, ComputerState::Running));
        assert!(row.attention.unwrap().message.contains("computer started"));
    }

    #[test]
    fn failed_start_bookkeeping_keeps_a_warning_until_verified() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        record_start_refresh(&paths, "dev", || {
            Err(RuntimeError::Unavailable(
                "Applied secret revision could not be saved.".into(),
            ))
        });
        assert!(start_refresh_attention(&paths, "dev")
            .unwrap()
            .message
            .contains("secret status"));
        record_start_refresh(&paths, "dev", || Ok(()));
        assert!(start_refresh_attention(&paths, "dev").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_stops_share_the_worker_flock_without_serializing_children() {
        let _test_state = crate::test_support::global_state();
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fs::create_dir_all(&paths.home).unwrap();
        fs::write(&paths.library, b"test library").unwrap();
        fs::write(
            &paths.executable,
            r#"#!/bin/sh
cat >/dev/null
touch "$MSB_HOME/$2-entered"
for attempt in 1 2 3 4 5 6 7 8 9 10; do
  if [ -f "$MSB_HOME/first-entered" ] && [ -f "$MSB_HOME/second-entered" ]; then exit 0; fi
  sleep 0.1
done
exit 9
"#,
        )
        .unwrap();
        fs::set_permissions(&paths.executable, fs::Permissions::from_mode(0o700)).unwrap();
        let parent = configuration_recovery::command_lock(&paths, Duration::ZERO).unwrap();
        thread::scope(|scope| {
            let workers: Vec<_> = ["first", "second"]
                .into_iter()
                .map(|computer| {
                    let shared = parent.duplicate_for_shutdown().unwrap();
                    let paths = &paths;
                    scope.spawn(move || {
                        with_shutdown_worker_lock(shared, || {
                            run_msb_process(
                                paths,
                                &["stop".into(), computer.into()],
                                Duration::from_secs(5),
                                &|_| {},
                            )
                        })
                    })
                })
                .collect();
            for worker in workers {
                worker.join().unwrap().unwrap();
            }
        });
        // A child that finished first must not unlock the parent's shared flock.
        assert!(configuration_recovery::command_lock(&paths, Duration::ZERO).is_err());
        drop(parent);
        drop(configuration_recovery::command_lock(&paths, Duration::ZERO).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn command_lock_is_released_after_failed_spawn_and_completed_child() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let mut paths = paths(&directory);
        fs::write(&paths.library, b"test library").unwrap();
        let command = ["create".to_string()];
        fs::write(&paths.executable, b"not executable").unwrap();
        assert!(run_msb_process(&paths, &command, Duration::from_secs(5), &|_| {}).is_err());
        drop(configuration_recovery::command_lock(&paths, Duration::ZERO).unwrap());

        paths.executable = "/usr/bin/true".into();
        run_msb_process(&paths, &command, Duration::from_secs(5), &|_| {}).unwrap();
        drop(configuration_recovery::command_lock(&paths, Duration::ZERO).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn command_lock_remains_held_while_runtime_child_runs() {
        let _test_state = crate::test_support::global_state();
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fs::write(&paths.library, b"test library").unwrap();
        fs::create_dir_all(&paths.home).unwrap();
        for name in ["ready", "release"] {
            let fifo = std::ffi::CString::new(paths.home.join(name).to_str().unwrap()).unwrap();
            // SAFETY: fifo is a valid NUL-terminated path in this test directory.
            assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        }
        fs::write(
            &paths.executable,
            b"#!/bin/sh\nprintf x > \"$MSB_HOME/ready\"\nread line < \"$MSB_HOME/release\"\n",
        )
        .unwrap();
        fs::set_permissions(&paths.executable, fs::Permissions::from_mode(0o700)).unwrap();
        let worker_paths = paths.clone();
        let worker = thread::spawn(move || {
            run_msb_process(
                &worker_paths,
                &["create".into()],
                Duration::from_secs(5),
                &|_| {},
            )
        });
        let _ready = File::open(paths.home.join("ready")).unwrap();
        assert!(configuration_recovery::command_lock(&paths, Duration::ZERO).is_err());
        fs::write(paths.home.join("release"), b"done\n").unwrap();
        worker.join().unwrap().unwrap();
        drop(configuration_recovery::command_lock(&paths, Duration::ZERO).unwrap());
    }

    #[test]
    fn configuration_recovery_preserves_a_replacement_computer() {
        let _test_state = crate::test_support::global_state();
        let gate = operation_gate::OperationGate::new();
        let _guard = gate.device("Recovering test configuration").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        configuration_recovery::begin(&paths, &request(vec![computer()])).unwrap();
        let mut actual = inspect(&paths, "Created");
        actual["config"]["labels"]["silo.machine-id"] = json!("someone-else");
        let runner = StubRunner::successful_json(vec![
            json!([{"name":"dev","status":"Created","image":"ubuntu"}]),
            actual,
        ]);
        let error = configuration_recovery::recover_at_paths(
            &runner,
            &paths,
            &generous_device(),
            &|_, _, _| {},
        )
        .unwrap_err();
        assert!(error.to_string().contains("different computer"));
        assert!(paths
            .metadata
            .with_file_name("configuration-operation.json")
            .exists());
        assert!(read_metadata(&paths.metadata).unwrap().computers.is_empty());
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args[0] == "remove"));
    }

    #[test]
    fn configuration_recovery_retries_owned_incomplete_storage_without_relaunch() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let candidate = request(vec![computer()]);
        configuration_recovery::begin(&paths, &candidate).unwrap();
        configuration_recovery::claim(&paths, &computer()).unwrap();
        fs::write(disk_path(&paths, "dev", "workspace"), b"incomplete").unwrap();
        let runner = StubRunner::successful_json(vec![json!([])]);
        configuration_recovery::prepare_retry(&runner, &paths, None).unwrap();
        assert!(!paths.volumes.join("dev").exists());
        configuration_recovery::claim(&paths, &computer()).unwrap();
        assert!(paths.volumes.join("dev/.silo-configuration-owner").exists());
        assert!(paths
            .metadata
            .with_file_name("configuration-operation.json")
            .exists());
    }

    #[test]
    fn configuration_recovery_preserves_committed_storage_when_runtime_is_missing() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let candidate = request(vec![computer()]);
        configuration_recovery::begin(&paths, &candidate).unwrap();
        configuration_recovery::claim(&paths, &computer()).unwrap();
        fs::write(disk_path(&paths, "dev", "workspace"), b"saved-data").unwrap();
        write_metadata(&paths.metadata, &candidate).unwrap();
        let runner = StubRunner::successful_json(vec![json!([])]);
        assert!(configuration_recovery::prepare_retry(&runner, &paths, None)
            .unwrap_err()
            .to_string()
            .contains("missing from the runtime"));
        assert_eq!(
            fs::read(disk_path(&paths, "dev", "workspace")).unwrap(),
            b"saved-data"
        );
    }

    #[test]
    fn configuration_recovery_finishes_interrupted_deletion() {
        let _test_state = crate::test_support::global_state();
        let gate = operation_gate::OperationGate::new();
        let _guard = gate.device("Recovering test configuration").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        fs::create_dir_all(paths.volumes.join("dev")).unwrap();
        fs::write(
            disk_path(&paths, "dev", "workspace"),
            b"deleted-computer-disk",
        )
        .unwrap();
        configuration_recovery::begin(&paths, &request(Vec::new())).unwrap();
        let runner = StubRunner::successful_json(vec![json!([])]);
        configuration_recovery::recover_at_paths(
            &runner,
            &paths,
            &generous_device(),
            &|_, _, _| {},
        )
        .unwrap();
        assert_eq!(read_metadata(&paths.metadata).unwrap(), request(Vec::new()));
        assert!(!paths.volumes.join("dev").exists());
    }

    #[test]
    fn deleting_managed_volumes_removes_empty_folder_and_preserves_unknown_files() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let folder = paths.volumes.join("dev");
        fs::create_dir_all(&folder).unwrap();
        fs::write(disk_path(&paths, "dev", "workspace"), b"disk").unwrap();
        remove_computer_volumes(&paths, &computer()).unwrap();
        assert!(!folder.exists());
        remove_computer_volumes(&paths, &computer()).unwrap();
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("unknown.raw"), b"keep").unwrap();
        assert!(remove_computer_volumes(&paths, &computer()).is_err());
        assert_eq!(fs::read(folder.join("unknown.raw")).unwrap(), b"keep");
    }

    #[test]
    fn removed_computer_github_profile_is_not_inherited_by_a_new_one() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        GITHUB_PROFILES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert(
                (paths.home.clone(), "dev".into()),
                r#"{"version":1,"owners":["old"]}"#.into(),
            );
        let args = ["start".to_string(), "dev".to_string()];
        assert_ne!(github_environment(&paths, &args), DISABLED_GITHUB_PROFILE);
        forget_github_state(&paths.home, "dev");
        assert_eq!(github_environment(&paths, &args), DISABLED_GITHUB_PROFILE);
    }

    #[test]
    fn deleting_a_computer_removes_its_github_assignment() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let document = directory.path().join("github.json");
        std::fs::write(&document, serde_json::to_vec(&json!({"revision":3,"accessEnabled":true,"account":null,
            "computers":[{"computer":"dev","repositoryMode":"all","allRepositoriesAllowChanges":true,"repositories":[],
                "identity":{"name":"","email":"","apply":false}}],"accessPending":["dev"]})).unwrap()).unwrap();
        crate::github::use_test_document(Some(document.clone()));
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let inspected = inspect(&paths, "Stopped");
        let runner = StubRunner::successful_json(vec![inspected.clone(), inspected, json!(null)]);
        let result =
            apply_whole_configuration(&runner, &paths, &generous_device(), request(vec![]));
        crate::github::use_test_document(None);
        result.unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&document).unwrap()).unwrap();
        assert_eq!(
            saved["computers"],
            json!([]),
            "a new computer named dev would inherit write access"
        );
        assert_eq!(saved["accessPending"], json!([]));
    }

    #[test]
    fn duplicate_lifecycle_request_is_handed_off_not_failed() {
        let _test_state = crate::test_support::global_state();
        let (result, handed_off) = hand_off_duplicate(Err(RuntimeError::from(
            operation_gate::GateError::AlreadyQueued,
        )));
        assert!(result.is_ok() && handed_off);
        let (result, handed_off) = hand_off_duplicate(Err(RuntimeError::Busy));
        assert!(result.is_err() && !handed_off);
        let (result, handed_off) = hand_off_duplicate(Ok(()));
        assert!(result.is_ok() && !handed_off);
    }

    #[test]
    fn running_checkpoint_operation_is_interrupted_only_when_its_computer_is_idle() {
        let _test_state = crate::test_support::global_state();
        let running = || checkpoints::Operation {
            kind: "create".into(),
            status: "running".into(),
            stage: "Saving disk".into(),
            error: None,
        };
        let live = checkpoint_operation_view(running(), true);
        assert_eq!(
            (live.status.as_str(), live.stage.as_str(), live.error),
            ("running", "Saving disk", None)
        );
        let interrupted = checkpoint_operation_view(running(), false);
        assert_eq!(interrupted.status, "failed");
        assert!(interrupted.error.unwrap().contains("Silo closed"));
    }

    #[test]
    fn application_snapshot_ignores_hidden_housekeeping_and_single_computer_work() {
        let _test_state = crate::test_support::global_state();
        let gate = operation_gate::OperationGate::new();
        assert!(gate.is_device_idle());
        let housekeeping = gate.try_device_hidden("Cleaning up expired logs").unwrap();
        assert!(!gate.is_idle());
        assert!(
            gate.is_device_idle(),
            "hidden housekeeping must not freeze state reads"
        );
        drop(housekeeping);
        let checkpoint = gate.computer("a-id", "a", "Creating checkpoint").unwrap();
        assert!(
            gate.is_device_idle(),
            "work on one computer must not freeze every computer"
        );
        drop(checkpoint);
        let change = gate.device("Applying computer changes").unwrap();
        assert!(!gate.is_device_idle());
        drop(change);
    }

    #[test]
    fn application_snapshot_defers_during_mutation_but_reports_real_mismatches() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let runner = StubRunner::successful_json(vec![json!([])]);
        let gate = operation_gate::OperationGate::new();
        let change = gate.device("Applying computer changes").unwrap();
        assert_eq!(
            read_application_snapshot(&runner, &paths, &gate)
                .unwrap_err()
                .code,
            ErrorCode::UpdateInProgress
        );
        drop(change);
        assert!(read_application_snapshot(&runner, &paths, &gate)
            .unwrap_err()
            .message
            .contains("does not match"));
    }

    fn leaked_gate() -> &'static operation_gate::OperationGate {
        Box::leak(Box::new(operation_gate::OperationGate::new()))
    }

    /// Hold the guard `acquire` takes on another thread until the sender is dropped.
    fn hold_elsewhere(
        gate: &'static operation_gate::OperationGate,
        acquire: impl FnOnce(
                &'static operation_gate::OperationGate,
            ) -> operation_gate::OperationGuard<'static>
            + Send
            + 'static,
    ) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
        let (release, released) = std::sync::mpsc::channel::<()>();
        let (held, holding) = std::sync::mpsc::channel::<()>();
        let holder = std::thread::spawn(move || {
            let _guard = acquire(gate);
            held.send(()).unwrap();
            let _ = released.recv();
        });
        holding.recv().unwrap();
        (release, holder)
    }

    fn two_computer_reading(paths: &RuntimePaths, dev: &str, work: &str) -> StubRunner {
        StubRunner::successful_json(vec![
            json!([{"name": "dev"}, {"name": "work"}]),
            inspect_named(paths, &computer(), dev),
            inspect_named(paths, &second_computer(), work),
        ])
    }

    fn row<'a>(source: &'a Value, name: &str) -> &'a Value {
        source["computers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["configuration"]["name"] == name)
            .unwrap()
    }

    fn published(source: &ApplicationSource) -> Value {
        serde_json::to_value(source).unwrap()
    }

    #[test]
    fn a_busy_computer_keeps_its_last_settled_reading_while_other_rows_refresh() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &request(vec![computer(), second_computer()]),
        )
        .unwrap();
        let gate = leaked_gate();
        let first = read_application_snapshot(
            &two_computer_reading(&paths, "Running", "Stopped"),
            &paths,
            gate,
        )
        .unwrap();
        remember_settled(&paths, &first.computers);
        // A checkpoint pauses dev while work is started outside it.
        let (release, holder) = hold_elsewhere(gate, |gate| {
            gate.computer(computer().id(), "dev", "Creating checkpoint")
                .unwrap()
        });
        let busy = read_application_snapshot(
            &two_computer_reading(&paths, "Paused", "Running"),
            &paths,
            gate,
        )
        .unwrap();
        let encoded = published(&busy);
        assert_eq!(
            (
                row(&encoded, "dev")["state"].as_str(),
                row(&encoded, "dev")["settling"].as_bool()
            ),
            (Some("running"), Some(true))
        );
        assert_eq!(row(&encoded, "dev")["freshness"], "fresh");
        assert_eq!(row(&encoded, "work")["state"], "running");
        assert!(row(&encoded, "work").get("settling").is_none());
        // A settling row never replaces the remembered reading.
        remember_settled(&paths, &busy.computers);
        assert!(matches!(
            last_settled(&paths)[computer().id()].state,
            ComputerState::Running
        ));
        drop(release);
        holder.join().unwrap();
        // Once nothing touches dev during a read, its row is fresh again.
        let settled = published(
            &read_application_snapshot(
                &two_computer_reading(&paths, "Stopped", "Running"),
                &paths,
                gate,
            )
            .unwrap(),
        );
        assert_eq!(row(&settled, "dev")["state"], "stopped");
        assert!(row(&settled, "dev").get("settling").is_none());
    }

    #[test]
    fn a_busy_computer_missing_from_the_runtime_or_unreadable_does_not_fail_the_read() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &request(vec![computer(), second_computer()]),
        )
        .unwrap();
        let gate = leaked_gate();
        remember_settled(
            &paths,
            &read_application_snapshot(
                &two_computer_reading(&paths, "Stopped", "Running"),
                &paths,
                gate,
            )
            .unwrap()
            .computers,
        );
        let (release, holder) = hold_elsewhere(gate, |gate| {
            gate.computer(computer().id(), "dev", "Restoring checkpoint")
                .unwrap()
        });
        // A restore has removed dev's runtime computer and not yet recreated it.
        let removed = StubRunner::successful_json(vec![
            json!([{"name": "work"}]),
            inspect_named(&paths, &second_computer(), "Running"),
        ]);
        let encoded = published(&read_application_snapshot(&removed, &paths, gate).unwrap());
        assert_eq!(
            (
                row(&encoded, "dev")["state"].as_str(),
                row(&encoded, "dev")["settling"].as_bool()
            ),
            (Some("stopped"), Some(true))
        );
        assert_eq!(row(&encoded, "work")["state"], "running");
        // Its inspection fails while the restore finishes.
        let unreadable = StubRunner::new(vec![
            identity_output(&json!([{"name": "dev"}, {"name": "work"}]).to_string()),
            Err(RuntimeError::TimedOut {
                operation: "inspect dev".into(),
            }),
            identity_output(&inspect_named(&paths, &second_computer(), "Running").to_string()),
        ]);
        let encoded = published(&read_application_snapshot(&unreadable, &paths, gate).unwrap());
        assert_eq!(
            (
                row(&encoded, "dev")["state"].as_str(),
                row(&encoded, "dev")["settling"].as_bool()
            ),
            (Some("stopped"), Some(true))
        );
        assert!(row(&encoded, "dev").get("attention").is_none());
        // With no earlier reading, a busy unreadable computer shows a neutral updating row.
        let fresh_directory = tempfile::tempdir().unwrap();
        let fresh_paths = super::tests::paths(&fresh_directory);
        write_metadata(
            &fresh_paths.metadata,
            &request(vec![computer(), second_computer()]),
        )
        .unwrap();
        let removed = StubRunner::successful_json(vec![
            json!([{"name": "work"}]),
            inspect_named(&fresh_paths, &second_computer(), "Running"),
        ]);
        let encoded = published(&read_application_snapshot(&removed, &fresh_paths, gate).unwrap());
        assert_eq!(
            (
                row(&encoded, "dev")["state"].as_str(),
                row(&encoded, "dev")["stateDetail"].as_str(),
                row(&encoded, "dev")["settling"].as_bool()
            ),
            (Some("starting"), Some("Updating"), Some(true)),
        );
        drop(release);
        holder.join().unwrap();
    }

    #[test]
    fn hidden_housekeeping_and_launch_starts_never_defer_or_settle_other_rows() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &request(vec![computer(), second_computer()]),
        )
        .unwrap();
        let gate = leaked_gate();
        let (release, holder) = hold_elsewhere(gate, |gate| {
            gate.try_device_hidden("Cleaning up expired logs").unwrap()
        });
        let encoded = published(
            &read_application_snapshot(
                &two_computer_reading(&paths, "Stopped", "Running"),
                &paths,
                gate,
            )
            .unwrap(),
        );
        assert!(encoded["computers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("settling").is_none()));
        drop(release);
        holder.join().unwrap();
        // The first read at launch succeeds while a selected computer boots.
        let launch_paths = paths.clone();
        let (release, holder) = hold_elsewhere(gate, move |gate| {
            match launch_start_guard(gate, &launch_paths, computer().id()).unwrap() {
                LaunchAdmission::Admitted(guard) => guard,
                _ => panic!("the launch start was not admitted"),
            }
        });
        let encoded = published(
            &read_application_snapshot(
                &two_computer_reading(&paths, "Starting", "Running"),
                &paths,
                gate,
            )
            .unwrap(),
        );
        assert_eq!(row(&encoded, "dev")["settling"], true);
        assert!(row(&encoded, "work").get("settling").is_none());
        drop(release);
        holder.join().unwrap();
    }

    #[test]
    fn an_idle_computer_whose_inspection_fails_is_stale_with_its_last_known_state() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &request(vec![computer(), second_computer()]),
        )
        .unwrap();
        let gate = leaked_gate();
        remember_settled(
            &paths,
            &read_application_snapshot(
                &two_computer_reading(&paths, "Running", "Running"),
                &paths,
                gate,
            )
            .unwrap()
            .computers,
        );
        let unreadable = || {
            StubRunner::new(vec![
                identity_output(&json!([{"name": "dev"}, {"name": "work"}]).to_string()),
                Err(RuntimeError::TimedOut {
                    operation: "inspect dev".into(),
                }),
                identity_output(&inspect_named(&paths, &second_computer(), "Stopped").to_string()),
            ])
        };
        for source in [
            read_application_snapshot(&unreadable(), &paths, gate).unwrap(),
            read_application_state_with(&unreadable(), &paths).unwrap(),
        ] {
            let encoded = published(&source);
            let dev = row(&encoded, "dev");
            assert_eq!(
                (dev["state"].as_str(), dev["freshness"].as_str()),
                (Some("running"), Some("stale"))
            );
            assert_eq!(dev["attention"]["level"], "warning");
            assert!(dev["attention"]["message"]
                .as_str()
                .unwrap()
                .contains("could not refresh"));
            assert!(dev.get("settling").is_none());
            assert_eq!(
                (
                    row(&encoded, "work")["state"].as_str(),
                    row(&encoded, "work")["freshness"].as_str()
                ),
                (Some("stopped"), Some("fresh"))
            );
            // A stale row is never remembered as a settled reading.
            remember_settled(&paths, &source.computers);
            assert!(matches!(
                last_settled(&paths)[computer().id()].freshness,
                Freshness::Fresh
            ));
        }
    }
    #[test]
    fn a_finished_change_is_not_reported_failed_when_the_refresh_fails() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &request(vec![computer(), second_computer()]),
        )
        .unwrap();
        let gate = leaked_gate();
        remember_settled(
            &paths,
            &read_application_snapshot(
                &two_computer_reading(&paths, "Running", "Stopped"),
                &paths,
                gate,
            )
            .unwrap()
            .computers,
        );
        // One computer's inspection times out after the change: only its row is stale.
        let unreadable = StubRunner::new(vec![
            identity_output(&json!([{"name": "dev"}, {"name": "work"}]).to_string()),
            Err(RuntimeError::TimedOut {
                operation: "inspect dev".into(),
            }),
            identity_output(&inspect_named(&paths, &second_computer(), "Running").to_string()),
        ]);
        let encoded = published(&state_after_change(&unreadable, &paths, gate).unwrap());
        assert_eq!(
            (
                row(&encoded, "dev")["state"].as_str(),
                row(&encoded, "dev")["freshness"].as_str()
            ),
            (Some("running"), Some("stale"))
        );
        assert_eq!(
            (
                row(&encoded, "work")["state"].as_str(),
                row(&encoded, "work")["freshness"].as_str()
            ),
            (Some("running"), Some("fresh"))
        );
        // The whole read fails: every computer keeps its last known state with the reason.
        let failed = StubRunner::new(vec![Err(RuntimeError::Unavailable(
            "synthetic runtime read failure".into(),
        ))]);
        let encoded = published(&state_after_change(&failed, &paths, gate).unwrap());
        for (name, state) in [("dev", "running"), ("work", "stopped")] {
            let stale = row(&encoded, name);
            assert_eq!(
                (stale["state"].as_str(), stale["freshness"].as_str()),
                (Some(state), Some("stale")),
                "{name}"
            );
            assert!(stale["attention"]["message"]
                .as_str()
                .unwrap()
                .contains("The change finished"));
        }
    }

    #[test]
    fn a_change_response_settles_other_busy_computers_but_not_the_callers_own_work() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &request(vec![computer(), second_computer()]),
        )
        .unwrap();
        let gate = leaked_gate();
        remember_settled(
            &paths,
            &read_application_snapshot(
                &two_computer_reading(&paths, "Running", "Running"),
                &paths,
                gate,
            )
            .unwrap()
            .computers,
        );
        let (release, holder) = hold_elsewhere(gate, |gate| {
            gate.computer(second_computer().id(), "work", "Creating checkpoint")
                .unwrap()
        });
        // The caller still holds dev's lane while it reads its own result.
        let own = gate
            .computer(computer().id(), "dev", "Stopping dev")
            .unwrap();
        let encoded = published(
            &state_after_change(
                &two_computer_reading(&paths, "Stopped", "Paused"),
                &paths,
                gate,
            )
            .unwrap(),
        );
        drop(own);
        assert_eq!(row(&encoded, "dev")["state"], "stopped");
        assert!(row(&encoded, "dev").get("settling").is_none());
        assert_eq!(
            (
                row(&encoded, "work")["state"].as_str(),
                row(&encoded, "work")["settling"].as_bool()
            ),
            (Some("running"), Some(true))
        );
        drop(release);
        holder.join().unwrap();
    }

    #[test]
    fn a_change_response_keeps_running_computers_repositories_until_the_next_read() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &request(vec![computer(), second_computer()]),
        )
        .unwrap();
        let gate = leaked_gate();
        let mut first = read_application_snapshot(
            &two_computer_reading(&paths, "Running", "Running"),
            &paths,
            gate,
        )
        .unwrap();
        let repositories = vec![
            json!({"path": "/workspace/app", "branch": "main", "ahead": 0, "behind": 0, "dirty": false}),
        ];
        first.computers[0].repositories = repositories.clone();
        remember_settled(&paths, &first.computers);
        let mut response = state_after_change(
            &two_computer_reading(&paths, "Running", "Stopped"),
            &paths,
            gate,
        )
        .unwrap();
        keep_last_known_repositories(&paths, &mut response.computers);
        assert_eq!(response.computers[0].repositories, repositories);
        assert!(
            response.computers[1].repositories.is_empty(),
            "a stopped computer lists no repositories"
        );
    }

    #[test]
    fn health_checks_budget_each_runtime_call_instead_of_the_whole_reading() {
        let _test_state = crate::test_support::global_state();
        struct SlowRunner {
            timeouts: Mutex<Vec<Duration>>,
            inner: StubRunner,
        }
        impl RuntimeRunner for SlowRunner {
            fn run(
                &self,
                paths: &RuntimePaths,
                args: &[String],
                timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.timeouts.lock().unwrap().push(timeout);
                std::thread::sleep(Duration::from_millis(30));
                self.inner.run(paths, args, timeout)
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fs::create_dir_all(&paths.home).unwrap();
        write_metadata(
            &paths.metadata,
            &request(vec![computer(), second_computer()]),
        )
        .unwrap();
        let slow = SlowRunner {
            timeouts: Mutex::new(Vec::new()),
            inner: two_computer_reading(&paths, "Running", "Stopped"),
        };
        // Three calls take about 90 ms together; each still gets its own 50 ms budget.
        let health = HealthRunner {
            inner: &slow,
            budget: Duration::from_millis(50),
        };
        let encoded = published(&read_application_state_with(&health, &paths).unwrap());
        assert!(encoded["computers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["freshness"] == "fresh"));
        let timeouts = slow.timeouts.lock().unwrap();
        assert_eq!(timeouts.len(), 3);
        assert!(timeouts
            .iter()
            .all(|timeout| *timeout == Duration::from_millis(50)));
    }

    #[test]
    fn health_checks_report_an_unavailable_runtime_home_without_running_it() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let inner = StubRunner::successful_json(vec![]);
        let health = HealthRunner {
            inner: &inner,
            budget: HEALTH_CALL_BUDGET,
        };
        assert!(matches!(
            health.run(&paths, &["list".into()], READ_TIMEOUT),
            Err(RuntimeError::Unavailable(_))
        ));
        assert!(inner.calls.lock().unwrap().is_empty());
    }

    fn stopped_dev(paths: &RuntimePaths) -> Vec<ApplicationComputer> {
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let runner =
            StubRunner::successful_json(vec![json!([{"name": "dev"}]), inspect(paths, "Stopped")]);
        read_application_state_with(&runner, paths)
            .unwrap()
            .computers
    }

    #[test]
    fn expired_log_cleanup_runs_off_the_read_path_at_most_hourly_per_computer() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut computers = stopped_dev(&paths);
        fs::create_dir_all(computer_logs(&paths, "dev")).unwrap();
        let now = Instant::now();
        let due = plan_log_cleanup(&paths, &mut computers, now);
        assert_eq!(due, vec!["dev".to_string()]);
        assert!(
            plan_log_cleanup(&paths, &mut computers, now).is_empty(),
            "one pass at a time"
        );
        let gate = operation_gate::OperationGate::new();
        let runner = StubRunner::successful_json(vec![inspect(&paths, "Stopped")]);
        clean_expired_logs(&runner, &paths, &gate, &due);
        assert_eq!(
            runner.calls.lock().unwrap().len(),
            1,
            "the pass re-checks the computer is stopped"
        );
        assert!(
            plan_log_cleanup(&paths, &mut computers, now).is_empty(),
            "cleaned within the hour"
        );
        assert_eq!(
            plan_log_cleanup(
                &paths,
                &mut computers,
                Instant::now() + LOG_CLEANUP_INTERVAL
            ),
            vec!["dev".to_string()]
        );
        clean_expired_logs(&StubRunner::successful_json(vec![]), &paths, &gate, &[]);
        assert!(computers[0].attention.is_none());
        // Running, settling or stale rows are never cleaned from a read.
        computers[0].settling = true;
        assert!(plan_log_cleanup(
            &paths,
            &mut computers,
            Instant::now() + 2 * LOG_CLEANUP_INTERVAL
        )
        .is_empty());
    }

    #[test]
    fn a_busy_gate_postpones_log_cleanup_and_a_failure_flags_only_that_computer() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut computers = stopped_dev(&paths);
        // Logs that cannot be read as a directory make retention fail.
        fs::create_dir_all(computer_logs(&paths, "dev").parent().unwrap()).unwrap();
        fs::write(computer_logs(&paths, "dev"), b"not a directory").unwrap();
        let now = Instant::now();
        let gate = leaked_gate();
        let (release, holder) = hold_elsewhere(gate, |gate| {
            gate.computer("other-id", "other", "Starting other")
                .unwrap()
        });
        let due = plan_log_cleanup(&paths, &mut computers, now);
        let idle = StubRunner::successful_json(vec![]);
        clean_expired_logs(&idle, &paths, gate, &due);
        assert!(
            idle.calls.lock().unwrap().is_empty(),
            "busy: nothing was inspected"
        );
        assert_eq!(
            plan_log_cleanup(&paths, &mut computers, now),
            due,
            "still due after a postponed pass"
        );
        drop(release);
        holder.join().unwrap();
        clean_expired_logs(
            &StubRunner::successful_json(vec![inspect(&paths, "Stopped")]),
            &paths,
            gate,
            &due,
        );
        assert!(plan_log_cleanup(&paths, &mut computers, now).is_empty());
        assert_eq!(
            computers[0].attention.as_ref().unwrap().message,
            "Expired logs could not be cleaned up."
        );
    }

    #[test]
    fn repository_discovery_for_one_computer_runs_one_caller_at_a_time() {
        let _test_state = crate::test_support::global_state();
        use std::sync::atomic::{AtomicUsize, Ordering};
        let active = Arc::new(AtomicUsize::new(0));
        let overlapped = Arc::new(AtomicUsize::new(0));
        let callers: Vec<_> = (0..4)
            .map(|_| {
                let (active, overlapped) = (active.clone(), overlapped.clone());
                std::thread::spawn(move || {
                    single_flight("single-flight-test:dev".into(), || {
                        if active.fetch_add(1, Ordering::SeqCst) > 0 {
                            overlapped.fetch_add(1, Ordering::SeqCst);
                        }
                        std::thread::sleep(Duration::from_millis(10));
                        active.fetch_sub(1, Ordering::SeqCst);
                    })
                })
            })
            .collect();
        callers
            .into_iter()
            .for_each(|caller| caller.join().unwrap());
        assert_eq!(overlapped.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn application_snapshot_discards_read_when_metadata_changes_and_retries_fresh() {
        let _test_state = crate::test_support::global_state();
        struct ChangeMetadataOnce {
            calls: Mutex<Vec<Vec<String>>>,
            changed: Mutex<bool>,
        }
        impl RuntimeRunner for ChangeMetadataOnce {
            fn run(
                &self,
                paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.calls.lock().unwrap().push(args.to_vec());
                let stdout = match args[0].as_str() {
                    "list" => {
                        json!([{"name":"dev","status":"Running","image":"ubuntu"}]).to_string()
                    }
                    "inspect" => {
                        let mut changed = self.changed.lock().unwrap();
                        if !*changed {
                            *changed = true;
                            let mut edited = computer();
                            edited.cpus += 1;
                            write_metadata(&paths.metadata, &request(vec![edited])).unwrap();
                        }
                        inspect(paths, "Running").to_string()
                    }
                    _ => panic!("unexpected runtime command: {args:?}"),
                };
                Ok(CommandOutput {
                    stdout,
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let runner = ChangeMetadataOnce {
            calls: Mutex::new(Vec::new()),
            changed: Mutex::new(false),
        };
        let source =
            read_application_snapshot(&runner, &paths, &operation_gate::OperationGate::new())
                .unwrap();
        assert_eq!(source.computers.len(), 1);
        assert_eq!(source.computers[0].configuration.cpus, computer().cpus + 1);
        assert_eq!(runner.calls.lock().unwrap().len(), 4);
    }

    #[test]
    fn application_snapshot_does_not_retry_real_runtime_read_errors() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let runner = crate::test_support::runner::ScriptedRunner::new([
            crate::test_support::runner::ExpectedCommand::error(
                ["list", "--label", "silo.managed=true", "--format", "json"],
                RuntimeError::Unavailable("synthetic runtime read failure".into()),
            )
            .with_timeout(READ_TIMEOUT),
        ]);
        assert_eq!(
            read_application_snapshot(&runner, &paths, &operation_gate::OperationGate::new())
                .unwrap_err(),
            BridgeError::from("synthetic runtime read failure"),
        );
        runner.assert_finished();
    }

    #[test]
    fn application_snapshot_defers_immediately_for_durable_configuration_recovery() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let configuration = request(vec![computer()]);
        write_metadata(&paths.metadata, &configuration).unwrap();
        configuration_recovery::begin(&paths, &configuration).unwrap();
        let runner = StubRunner::successful_json(Vec::new());
        assert_eq!(
            read_application_snapshot(&runner, &paths, &operation_gate::OperationGate::new())
                .unwrap_err()
                .code,
            ErrorCode::UpdateInProgress,
        );
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn read_uses_only_managed_runtime_state_and_exact_saved_resources() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let runner = StubRunner::successful_json(vec![
            json!([{"name":"dev","status":"Running","image":"ubuntu"}]),
            inspect(&paths, "Running"),
        ]);

        let state = read_application_state_with(&runner, &paths).unwrap();
        let encoded = serde_json::to_value(state).unwrap();
        assert_eq!(encoded["computers"][0]["state"], "running");
        assert_eq!(encoded["computers"][0]["configuration"]["memoryGiB"], 16);
        assert_eq!(
            encoded["computers"][0]["configuration"]["workspaceStorageGiB"],
            60
        );
        assert!(encoded["computers"][0].get("attention").is_none());
        assert_eq!(
            runner.calls.lock().unwrap()[0],
            vec!["list", "--label", MANAGED_LABEL, "--format", "json"]
        );
    }

    #[test]
    fn application_state_reports_the_host_capacity_that_ceilings_are_checked_against() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let encoded =
            serde_json::to_value(application_source_for_computers(&paths, Vec::new()).unwrap())
                .unwrap();
        let device = device_resources().unwrap();
        let capacity = &encoded["deviceCapacity"];
        assert_eq!(capacity["logicalCpus"], json!(device.logical_cpus));
        assert_eq!(
            capacity["physicalMemoryBytes"],
            json!(device.physical_memory_bytes.unwrap())
        );
        // The published limits are exactly the largest ceilings the backend accepts.
        let cpus = u8::try_from(device.logical_cpus).unwrap_or(u8::MAX);
        let memory = u32::try_from(capacity["maxMemoryGib"].as_u64().unwrap()).unwrap();
        assert!(validate_device_ceiling("dev", cpus, memory, &device).is_ok());
        assert!(validate_device_ceiling("dev", cpus, memory + 1, &device).is_err());
        if device.logical_cpus < usize::from(u8::MAX) {
            assert!(validate_device_ceiling("dev", cpus + 1, memory, &device).is_err());
        }
    }

    #[test]
    fn host_capacity_rounds_memory_down_and_is_absent_when_unmeasured() {
        let _test_state = crate::test_support::global_state();
        let gib = 1024 * 1024 * 1024;
        let capacity = DeviceCapacity::of(&DeviceResources {
            logical_cpus: 8,
            physical_memory_bytes: Some(16 * gib - 1),
        })
        .unwrap();
        assert_eq!((capacity.logical_cpus, capacity.max_memory_gib), (8, 15));
        assert!(DeviceCapacity::of(&DeviceResources {
            logical_cpus: 8,
            physical_memory_bytes: None
        })
        .is_none());
        assert!(DeviceCapacity::of(&DeviceResources {
            logical_cpus: 0,
            physical_memory_bytes: Some(gib)
        })
        .is_none());
    }

    fn second_computer() -> ComputerConfiguration {
        let mut other = computer();
        {
            let ComputerConfiguration { id, name, .. } = &mut other;
            *id = "00000000-0000-4000-8000-000000000002".into();
            *name = "work".into();
        }
        other
    }

    fn inspect_named(
        paths: &RuntimePaths,
        configuration: &ComputerConfiguration,
        status: &str,
    ) -> Value {
        let mut inspected = inspect(paths, status);
        inspected["name"] = json!(configuration.name());
        inspected["config"]["labels"]["silo.machine-id"] = json!(configuration.id());
        inspected
    }

    fn damaged_checkpoint_record(paths: &RuntimePaths, configuration: &ComputerConfiguration) {
        let directory = paths.metadata.with_file_name("checkpoints");
        fs::create_dir_all(&directory).unwrap();
        // For example, written by a newer Silo before a downgrade.
        fs::write(
            directory.join(format!("{}.json", configuration.id())),
            json!({"version": 2, "checkpoints": []}).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn a_damaged_checkpoint_record_degrades_only_its_own_computer() {
        let _test_state = crate::test_support::global_state();
        for listed in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            write_metadata(
                &paths.metadata,
                &request(vec![computer(), second_computer()]),
            )
            .unwrap();
            damaged_checkpoint_record(&paths, &computer());
            let mut outputs = Vec::new();
            if listed {
                outputs.push(json!([{"name": "dev"}, {"name": "work"}]));
                outputs.push(inspect_named(&paths, &computer(), "Running"));
            } else {
                // A pending fork with no runtime computer yet: its record decides, and is unreadable.
                outputs.push(json!([{"name": "work"}]));
            }
            outputs.push(inspect_named(&paths, &second_computer(), "Running"));
            let runner = StubRunner::successful_json(outputs);
            let source = read_application_state_with(&runner, &paths).unwrap();
            let encoded = serde_json::to_value(&source).unwrap();
            let damaged = &encoded["computers"][0];
            assert_eq!(damaged["attention"]["level"], "error", "{listed}");
            assert!(damaged["attention"]["message"]
                .as_str()
                .unwrap()
                .contains("Checkpoint history is invalid"));
            assert!(
                damaged.get("checkpoints").is_none()
                    && damaged.get("pendingCheckpointRestore").is_none()
            );
            let healthy = &encoded["computers"][1];
            assert_eq!(healthy["state"], "running");
            assert!(healthy.get("attention").is_none());
        }
    }

    fn pending_secret_fixture(directory: &tempfile::TempDir) -> crate::secrets::PendingRevocation {
        let store = directory.path().join("secrets.json");
        fs::write(&store, r#"{"pendingRevocations":[{"secretId":"old-secret","generation":"old-value","name":"API_KEY","computer":"dev"}]}"#).unwrap();
        crate::secrets::use_test_store(Some(store));
        crate::secrets::use_test_vault(Some(Default::default()));
        crate::secrets::pending_revocations().unwrap().remove(0)
    }

    fn inspected_secret(paths: &RuntimePaths, status: &str, names: &[&str]) -> Value {
        let mut value = inspect(paths, status);
        value["config"]["network"]["secrets"]["secrets"] = json!(names
            .iter()
            .map(|name| json!({"env_var":name}))
            .collect::<Vec<_>>());
        value["active_config"] = value["config"].clone();
        value
    }

    #[test]
    fn pending_secret_revocation_requires_verified_removal_or_a_guest_that_cannot_hold_values() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let record = pending_secret_fixture(&directory);
        let removed = std::cell::RefCell::new(Vec::new());
        let mut remove = |name: &str| {
            removed.borrow_mut().push(name.to_string());
            Ok(())
        };
        for status in ["Starting", "Draining", "Paused", "Unknown"] {
            assert!(!revoke_secret_with(
                &StubRunner::successful_json(vec![inspected_secret(&paths, status, &["API_KEY"])]),
                &paths,
                &record,
                &mut remove
            )
            .unwrap());
        }
        assert!(revoke_secret_with(
            &StubRunner::new(vec![Err(RuntimeError::Unavailable("unreadable".into()))]),
            &paths,
            &record,
            &mut remove
        )
        .is_err());
        let no_active = inspect(&paths, "Running");
        assert!(!revoke_secret_with(
            &StubRunner::successful_json(vec![no_active]),
            &paths,
            &record,
            &mut remove
        )
        .unwrap());
        for status in ["Stopped", "Created", "Crashed"] {
            assert!(revoke_secret_with(
                &StubRunner::successful_json(vec![inspect(&paths, status)]),
                &paths,
                &record,
                &mut remove
            )
            .unwrap());
        }
        let missing = StubRunner::new(vec![Err(RuntimeError::Failed {
            operation: "inspect".into(),
            exit_code: Some(1),
            detail: "computer 'dev' not found".into(),
        })]);
        assert!(revoke_secret_with(&missing, &paths, &record, &mut remove).unwrap());
        assert!(
            removed.borrow().is_empty(),
            "unreadable/transitional and stopped guests need no runtime mutation"
        );
        // The runtime claims success but still exposes the name: do not clear the warning.
        assert!(!revoke_secret_with(
            &StubRunner::successful_json(vec![
                inspected_secret(&paths, "Running", &["API_KEY", "KEEP"]),
                inspected_secret(&paths, "Running", &["API_KEY", "KEEP"])
            ]),
            &paths,
            &record,
            &mut remove
        )
        .unwrap());
        assert!(revoke_secret_with(
            &StubRunner::successful_json(vec![
                inspected_secret(&paths, "Running", &["API_KEY", "KEEP"]),
                inspected_secret(&paths, "Running", &["KEEP"])
            ]),
            &paths,
            &record,
            &mut remove
        )
        .unwrap());
        assert_eq!(*removed.borrow(), ["API_KEY", "API_KEY"]);
        assert!(revoke_secret_with(
            &StubRunner::successful_json(vec![inspected_secret(&paths, "Running", &["API_KEY"])]),
            &paths,
            &record,
            &mut |_| Err("runtime failed".into())
        )
        .is_err());
        write_metadata(&paths.metadata, &request(Vec::new())).unwrap();
        assert!(revoke_secret_with(
            &StubRunner::new(Vec::new()),
            &paths,
            &record,
            &mut |_| panic!("deleted guest")
        )
        .unwrap());
        crate::secrets::use_test_store(None);
        crate::secrets::use_test_vault(None);
    }

    #[test]
    fn pending_secret_revocation_does_not_retire_a_running_failed_restore() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let pending: checkpoints::Record = serde_json::from_value(json!({
            "version":1,
            "checkpoints":[],
            "restoreAttempted":true,
            "pendingCheckpointRestore":{
                "checkpointId":"c000000000000000000000000000000",
                "sourceComputer":"source",
                "state":"disk"
            }
        }))
        .unwrap();
        checkpoints::save(&paths, computer().id(), &pending).unwrap();
        let record = pending_secret_fixture(&directory);
        let mut removed = Vec::new();
        let result = revoke_secret_with(
            &StubRunner::successful_json(vec![
                inspected_secret(&paths, "Running", &["API_KEY"]),
                inspected_secret(&paths, "Running", &["API_KEY"]),
            ]),
            &paths,
            &record,
            &mut |name| {
                removed.push(name.to_owned());
                Ok(())
            },
        )
        .unwrap();
        crate::secrets::use_test_store(None);
        crate::secrets::use_test_vault(None);
        assert!(
            !result,
            "a running restore still exposes the revoked secret"
        );
        assert_eq!(removed, ["API_KEY"]);
    }

    #[test]
    fn pending_secret_retry_skips_busy_guests_and_rechecks_replacement_after_the_computer_settles()
    {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let record = pending_secret_fixture(&directory);
        let (release, released) = std::sync::mpsc::channel::<()>();
        let (held, holding) = std::sync::mpsc::channel();
        let holder = thread::spawn(move || {
            let _guard = OPERATIONS
                .computer(
                    "00000000-0000-4000-8000-000000000001",
                    "dev",
                    "Updating dev",
                )
                .unwrap();
            held.send(()).unwrap();
            released.recv().unwrap();
        });
        holding.recv().unwrap();
        assert!(!revoke_secret_with(
            &StubRunner::new(Vec::new()),
            &paths,
            &record,
            &mut |_| panic!("busy guest")
        )
        .unwrap());
        // Replacement was applied by the preceding operation, before this retry's
        // Computer gate. The old journal entry must never remove the replacement by name.
        let store = directory.path().join("secrets.json");
        let mut document: Value = serde_json::from_slice(&fs::read(&store).unwrap()).unwrap();
        document["secrets"] = json!([{"id":"new-secret","valueId":"new-value","name":"API_KEY","computers":["dev"],"allowedDomains":["api.example.com"]}]);
        fs::write(&store, document.to_string()).unwrap();
        release.send(()).unwrap();
        holder.join().unwrap();
        assert!(!revoke_secret_with(
            &StubRunner::successful_json(vec![inspected_secret(&paths, "Running", &["API_KEY"])]),
            &paths,
            &record,
            &mut |_| panic!("must preserve new value")
        )
        .unwrap());
        crate::secrets::use_test_store(None);
        crate::secrets::use_test_vault(None);
    }

    #[test]
    fn pending_secret_warning_is_current_while_runtime_fields_are_cached() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let _record = pending_secret_fixture(&directory);
        let source = application_source_for_computers(
            &paths,
            vec![application_computer(
                &paths,
                computer(),
                &serde_json::from_value(inspect(&paths, "Running")).unwrap(),
            )],
        )
        .unwrap();
        let previous = &source.computers[0];
        assert_eq!(previous.pending_secret_revocations, ["API_KEY"]);
        assert_eq!(
            previous.attention.as_ref().unwrap().message,
            "May still have access to API_KEY until it restarts."
        );
        crate::secrets::computer_started("dev", &crate::secrets::computer_revision("dev").unwrap())
            .unwrap();
        let mut current = unread_computer(computer());
        keep_runtime_fields(&mut current, previous);
        let cleared = application_source_for_computers(&paths, vec![current]).unwrap();
        assert!(cleared.computers[0].pending_secret_revocations.is_empty());
        assert!(cleared.computers[0].attention.is_none());
        crate::secrets::use_test_store(None);
        crate::secrets::use_test_vault(None);
    }

    #[test]
    fn native_state_omits_legacy_placeholders() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let runner = StubRunner::successful_json(vec![
            json!([{"name":"dev","status":"Running","image":"ubuntu"}]),
            inspect(&paths, "Running"),
        ]);
        let encoded =
            serde_json::to_value(read_application_state_with(&runner, &paths).unwrap()).unwrap();
        assert!(encoded.get("preferences").is_none());
        assert!(encoded.get("backup").is_none());
    }

    #[test]
    fn read_refuses_missing_runtime_rows_instead_of_publishing_false_success() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let runner = StubRunner::successful_json(vec![json!([])]);

        let error = read_application_state_with(&runner, &paths).unwrap_err();
        assert!(error.to_string().contains("does not match"));
    }

    #[test]
    fn create_keeps_computer_and_runtime_on_independent_app_owned_disks() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let runner = StubRunner::successful_json(vec![
            json!([]),
            json!(1),
            json!(1),
            json!(null),
            inspect_owned_workspace(&paths, "Created"),
            json!(null),
            inspect_owned_workspace(&paths, "Stopped"),
        ]);

        create_computer(&runner, &paths, &computer()).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            &calls[3][..2],
            ["create", "ghcr.io/0xpolarzero/silo-guest:test"]
        );
        assert!(calls[3]
            .windows(2)
            .any(|pair| pair == ["--root-disk", "80G"]));
        assert!(!disk_path(&paths, "dev", "runtime").exists());
        assert!(calls[3]
            .windows(2)
            .any(|pair| pair == ["--mount-owned", "/workspace:kind=disk,size=60G"]));
        assert!(calls[3]
            .windows(2)
            .any(|pair| pair == ["--secret", secrets_runtime::SILO_GITHUB_SECRET_SPEC]));
        // The network option is set explicitly, to the value an export compares against.
        let profile: Value =
            serde_json::from_str(include_str!("../guest/github-network-default.json")).unwrap();
        assert_eq!(profile["strict"], true);
        assert_eq!(
            calls[3]
                .iter()
                .filter(|arg| arg.starts_with("--net-strict"))
                .collect::<Vec<_>>(),
            ["--net-strict=true"]
        );
        assert!(!calls[3].iter().any(|arg| arg == "--mount-disk"));
        assert!(calls[3]
            .windows(2)
            .any(|pair| pair == ["--label", MANAGED_LABEL]));
        // The account is the guest's own, set up after each boot, never a label.
        assert!(!calls[3].iter().any(|arg| arg.contains("working-account")));
        let tools = &calls[5];
        assert!(tools.windows(2).any(|pair| pair == ["--user", "root"]));
    }

    #[test]
    fn desktop_creation_installs_after_base_tools_and_verifies_stopped_state() {
        let _test_state = crate::test_support::global_state();
        for final_state in ["Stopped", "Running"] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            let mut configuration = computer();
            {
                let ComputerConfiguration { desktop, .. } = &mut configuration;
                *desktop = Some(crate::desktop::DesktopConfiguration {
                    start_with_computer: true,
                    built_in: false,
                });
            }
            let runner = StubRunner::successful_json(vec![
                json!([]),
                json!(1),
                json!(1),
                json!(null),
                inspect(&paths, "Created"),
                json!(null),
                inspect(&paths, "Stopped"),
                inspect(&paths, "Stopped"),
                json!(1),
                json!(null),
                inspect(&paths, final_state),
            ]);
            let result = create_computer(&runner, &paths, &configuration);
            assert_eq!(result.is_ok(), final_state == "Stopped");
            let calls = runner.calls.lock().unwrap();
            assert_eq!(calls[9][0], "exec");
            assert!(calls[9]
                .last()
                .unwrap()
                .contains("silo-desktop autostart true"));
            assert!(calls[3].contains(&"--no-start".into()));
        }
    }

    /// What the guest helper answers when it applied computer use.
    fn applied_report() -> Value {
        json!({"state": "ready", "apply": {"approval": "ask", "outcome": "applied", "reason": null}})
    }

    fn built_in_computer() -> ComputerConfiguration {
        let mut configuration = computer();
        {
            let ComputerConfiguration { desktop, .. } = &mut configuration;
            *desktop = Some(crate::desktop::DesktopConfiguration {
                start_with_computer: true,
                built_in: true,
            });
        }
        configuration
    }

    #[test]
    fn creating_a_built_in_computer_mounts_the_published_app_folder_read_only() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let published = directory.path().join("published");
        fs::create_dir(&published).unwrap();
        // Empty on purpose: the ChatGPT app may still be downloading.
        assert_eq!(fs::read_dir(&published).unwrap().count(), 0);
        crate::computer_use::set_test_published_dir(Some(published.clone()));
        let runner = StubRunner::successful_json(vec![
            json!([]),
            json!(1),
            json!(1),
            json!(null),
            inspect(&paths, "Created"),
            json!(null),
            inspect(&paths, "Stopped"),
            inspect(&paths, "Stopped"),
            json!(1),
            json!(null),
            inspect(&paths, "Stopped"),
            applied_report(),
        ]);
        create_computer(&runner, &paths, &built_in_computer()).unwrap();
        crate::computer_use::set_test_published_dir(None);
        let calls = runner.calls.lock().unwrap();
        let mount = format!("{}:/opt/silo/chatgpt:ro,uid=0,gid=0", published.display());
        let create = &calls[3];
        let position = create.iter().position(|arg| arg == "-v").unwrap();
        assert_eq!(create[position + 1], mount);
        assert_eq!(create.iter().filter(|arg| *arg == "-v").count(), 1);
        assert!(position < create.iter().position(|arg| arg == "--no-start").unwrap());
        // The workspace disk is still the only owned mount.
        assert!(create
            .windows(2)
            .any(|pair| pair == ["--mount-owned", "/workspace:kind=disk,size=60G"]));
    }

    #[test]
    fn a_created_computer_takes_its_initial_approval_mode_from_the_setting() {
        use crate::computer_use::Approval;
        let _test_state = crate::test_support::global_state();
        for mode in [Approval::Ask, Approval::Auto] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            let published = directory.path().join("published");
            fs::create_dir(&published).unwrap();
            crate::computer_use::set_test_published_dir(Some(published));
            let runner = StubRunner::successful_json(vec![
                json!([]),
                json!(1),
                json!(1),
                json!(null),
                inspect(&paths, "Created"),
                json!(null),
                inspect(&paths, "Stopped"),
                inspect(&paths, "Stopped"),
                json!(1),
                json!(null),
                inspect(&paths, "Stopped"),
                applied_report(),
            ]);
            let configuration = built_in_computer();
            crate::computer_use::with_initial_approval(mode, || {
                create_computer(&runner, &paths, &configuration).unwrap()
            });
            crate::computer_use::set_test_published_dir(None);
            let settings = crate::computer_use::settings(&paths, configuration.id());
            assert_eq!(settings.approval, mode);
            // Creation applies the mode itself, in the one boot that has the desktop up.
            let apply = runner.calls.lock().unwrap().last().cloned().unwrap();
            assert!(apply
                .last()
                .unwrap()
                .contains(&format!("apply --approval {} --boot", mode.as_str())));
            assert_eq!(
                settings.last.map(|attempt| attempt.outcome),
                Some(crate::computer_use::Outcome::Applied)
            );
        }
    }

    #[test]
    fn creation_without_the_computer_use_apply_finishes_with_a_warning_and_no_boot() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let published = directory.path().join("published");
        fs::create_dir(&published).unwrap();
        crate::computer_use::set_test_published_dir(Some(published));
        let runner = StubRunner::successful_json(vec![
            json!([]),
            json!(1),
            json!(1),
            json!(null),
            inspect(&paths, "Created"),
            json!(null),
            inspect(&paths, "Stopped"),
            inspect(&paths, "Stopped"),
            json!(1),
            json!(null),
            inspect(&paths, "Stopped"),
        ]);
        let configuration = built_in_computer();
        crate::creation_inputs::exclude_computer_use(&[configuration.id().to_owned()]);
        let events = Mutex::new(Vec::new());
        create_computer_with_progress(&runner, &paths, &configuration, &|step, _, _| {
            events.lock().unwrap().push(step.to_owned())
        })
        .unwrap();
        crate::computer_use::set_test_published_dir(None);
        let calls = runner.calls.lock().unwrap();
        assert!(!calls
            .last()
            .unwrap()
            .iter()
            .any(|arg| arg.contains("silo-computer-use")));
        assert_eq!(
            events.lock().unwrap().last().map(String::as_str),
            Some("computer-use-pending")
        );
        assert!(!crate::creation_inputs::take_without_computer_use(
            configuration.id()
        ));
    }

    #[test]
    fn creating_a_computer_needs_the_image_and_the_app_only_when_it_is_new_and_built_in() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut plain = computer();
        {
            let ComputerConfiguration { desktop, .. } = &mut plain;
            *desktop = None;
        }
        let request = |computers: Vec<ComputerConfiguration>| ComputerConfigurationRequest {
            schema_version: 1,
            computers,
        };
        write_metadata(&paths.metadata, &request(Vec::new())).unwrap();
        // A v3 image has no built-in desktop: the image is needed, the app is not.
        let needs = creation_needs(&paths, request(vec![plain.clone()]));
        assert_eq!(needs.computers, [plain.id().to_owned()]);
        assert!(needs.computer_use.is_empty());
        {
            let _v4 = guest_image::pin_test_version("ubuntu-24.04-v4");
            let needs = creation_needs(&paths, request(vec![plain.clone()]));
            assert_eq!(needs.computer_use, [plain.id().to_owned()]);
        }
        // An existing computer is never waited for.
        write_metadata(&paths.metadata, &request(vec![plain.clone()])).unwrap();
        assert!(creation_needs(&paths, request(vec![plain])).is_empty());
    }

    #[test]
    fn creating_a_computer_without_built_in_computer_use_adds_no_mount() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        crate::computer_use::set_test_published_dir(Some(directory.path().to_path_buf()));
        let runner = StubRunner::successful_json(vec![
            json!([]),
            json!(1),
            json!(1),
            json!(null),
            inspect_owned_workspace(&paths, "Created"),
            json!(null),
            inspect_owned_workspace(&paths, "Stopped"),
        ]);
        create_computer(&runner, &paths, &computer()).unwrap();
        crate::computer_use::set_test_published_dir(None);
        let calls = runner.calls.lock().unwrap();
        assert!(!calls[3]
            .iter()
            .any(|arg| arg == "-v" || arg.contains("/opt/silo")));
    }

    #[test]
    fn a_built_in_computer_is_not_created_without_the_shared_folder() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        crate::computer_use::set_test_published_dir(None);
        let runner = StubRunner::successful_json(vec![]);
        let error = create_computer(&runner, &paths, &built_in_computer()).unwrap_err();
        assert!(error.to_string().contains("shared ChatGPT folder"));
        // Refused before the runtime was asked anything or anything was claimed.
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn guest_tool_setup_does_not_hide_failure_to_restore_stopped_state() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let runner = StubRunner::successful_json(vec![json!(null), inspect(&paths, "Running")]);
        assert!(verify_guest_tools(&runner, &paths, "dev")
            .unwrap_err()
            .to_string()
            .contains("stopped state"));
    }

    #[test]
    fn create_rejects_runtime_without_secure_github_protocol_before_provisioning() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let runner = StubRunner::successful_json(vec![json!([]), json!(0)]);
        let error = create_computer(&runner, &paths, &computer()).unwrap_err();
        assert!(error.to_string().contains("secure GitHub access"));
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args[0] == "create"));
        assert!(!disk_path(&paths, "dev", "workspace").exists());
    }

    #[test]
    fn create_rejects_unexpected_running_state() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let runner = StubRunner::successful_json(vec![
            json!([]),
            json!(1),
            json!(1),
            json!(null),
            inspect(&paths, "Running"),
            json!([]),
        ]);
        let error = create_computer(&runner, &paths, &computer()).unwrap_err();
        assert!(error.to_string().contains("did not remain stopped"));
        assert!(runner.calls.lock().unwrap()[3]
            .iter()
            .any(|arg| arg == "--no-start"));
    }

    #[test]
    fn create_preserves_a_runtime_name_collision() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let runner = StubRunner::successful_json(vec![json!([{"name":"dev"}])]);
        assert!(create_computer(&runner, &paths, &computer()).is_err());
        assert_eq!(runner.calls.lock().unwrap().len(), 1);
        assert!(!disk_path(&paths, "dev", "workspace").exists());
    }

    #[test]
    fn update_rejects_storage_resize_before_any_runtime_mutation() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let previous = computer();
        let mut changed = previous.clone();
        {
            let ComputerConfiguration {
                workspace_storage_gib,
                runtime_storage_gib,
                ..
            } = &mut changed;
            *workspace_storage_gib += 1;
            *runtime_storage_gib += 2;
        }
        let runner = StubRunner::new(Vec::new());

        let error = update_computer(&runner, &paths, &previous, &changed).unwrap_err();

        assert!(error.to_string().contains("cannot be resized in place"));
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn edit_rejects_a_replacement_before_any_configuration_side_effect() {
        let _test_state = crate::test_support::global_state();
        for status in ["Running", "Stopped"] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            let previous = request(vec![computer()]);
            write_metadata(&paths.metadata, &previous).unwrap();
            let mut changed = computer();
            {
                let ComputerConfiguration { cpus, .. } = &mut changed;
                *cpus = 2;
            }
            let mut replacement = inspect(&paths, status);
            replacement["config"]["labels"]["silo.machine-id"] =
                json!("22222222-2222-4222-8222-222222222222");
            let mut modified = inspect(&paths, "Stopped");
            modified["config"]["resources"]["cpus"] = json!(2);
            let mut outputs = vec![replacement.clone(), replacement];
            if status == "Running" {
                outputs.push(json!(null));
                outputs.push(inspect(&paths, "Stopped"));
            }
            outputs.extend([json!(null), modified]);
            let runner = StubRunner::successful_json(outputs);

            let error = apply_whole_configuration(
                &runner,
                &paths,
                &generous_device(),
                request(vec![changed]),
            )
            .unwrap_err();

            assert!(error.to_string().contains("identity"), "{error}");
            assert_eq!(read_metadata(&paths.metadata).unwrap(), previous);
            assert!(configuration_recovery::load(&paths).unwrap().is_none());
            assert_eq!(runner.calls.lock().unwrap().len(), 1);
        }
    }

    #[test]
    fn edit_rejects_replacements_for_resource_and_desktop_changes() {
        let _test_state = crate::test_support::global_state();
        for desktop_only in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            let previous = computer();
            let mut changed = previous.clone();
            {
                let ComputerConfiguration { cpus, desktop, .. } = &mut changed;
                if desktop_only {
                    *desktop = Some(crate::desktop::DesktopConfiguration {
                        start_with_computer: true,
                        built_in: false,
                    });
                } else {
                    *cpus = 2;
                }
            }
            let mut replacement = inspect(&paths, "Stopped");
            replacement["config"]["labels"]["silo.machine-id"] =
                json!("22222222-2222-4222-8222-222222222222");
            let runner = StubRunner::successful_json(vec![replacement, json!(1), json!(null)]);

            let error = update_computer(&runner, &paths, &previous, &changed).unwrap_err();

            assert!(error.to_string().contains("identity"), "{error}");
            assert_eq!(runner.calls.lock().unwrap().len(), 1);
        }
    }

    #[test]
    fn edit_rechecks_identity_after_stopping() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let previous = computer();
        let mut changed = previous.clone();
        {
            let ComputerConfiguration { cpus, .. } = &mut changed;
            *cpus = 2;
        }
        let mut replacement = inspect(&paths, "Stopped");
        replacement["config"]["labels"]["silo.machine-id"] =
            json!("22222222-2222-4222-8222-222222222222");
        let runner = StubRunner::successful_json(vec![
            inspect(&paths, "Running"),
            json!(null),
            replacement,
            json!(null),
        ]);

        let error = update_computer(&runner, &paths, &previous, &changed).unwrap_err();

        assert!(error.to_string().contains("identity"), "{error}");
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args[0] == "modify"));
    }

    #[test]
    fn edit_stops_running_computer_before_modifying_and_does_not_restart() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let previous = computer();
        let mut changed = previous.clone();
        {
            let ComputerConfiguration { cpus, .. } = &mut changed;
            *cpus = 2;
        }
        let runner = StubRunner::successful_json(vec![
            inspect(&paths, "Running"),
            json!(null),
            inspect(&paths, "Stopped"),
            json!(null),
        ]);
        update_computer(&runner, &paths, &previous, &changed).unwrap();
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[1], vec!["stop", "dev", "--quiet"]);
        assert_eq!(calls[3][0], "modify");
        assert!(!calls
            .iter()
            .any(|call| matches!(call[0].as_str(), "start" | "restart")));
    }

    #[test]
    fn edit_does_not_modify_when_stop_is_unverified() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let previous = computer();
        let runner = StubRunner::successful_json(vec![
            inspect(&paths, "Running"),
            json!(null),
            inspect(&paths, "Running"),
        ]);
        let error = update_computer(&runner, &paths, &previous, &previous).unwrap_err();
        assert!(error.to_string().contains("did not stop"));
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call[0] == "modify"));
    }

    fn wait_for_queue(
        gate: &operation_gate::OperationGate,
        ready: impl Fn(&operation_gate::OperationQueue) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready(&gate.snapshot()) {
            assert!(
                Instant::now() < deadline,
                "operation queue did not reach the expected state"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn launch_start_takes_only_its_computers_lane_like_a_user_start() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let gate: &'static operation_gate::OperationGate =
            Box::leak(Box::new(operation_gate::OperationGate::new()));
        let LaunchAdmission::Admitted(guard) =
            launch_start_guard(gate, &paths, computer().id()).unwrap()
        else {
            panic!("the launch start was not admitted");
        };
        let queue = gate.snapshot();
        let entry = &queue.running[0];
        assert_eq!(
            (
                entry.label.as_str(),
                entry.computer_id.as_deref(),
                entry.kind,
                entry.cancellable
            ),
            (
                "Starting dev",
                Some(computer().id()),
                operation_gate::OperationKind::Lifecycle,
                true
            ),
        );
        assert_eq!(entry.expected_ms, Some(180_000));
        // Other computers' actions and state reads are not held behind this boot.
        assert!(gate.is_device_idle());
        std::thread::spawn(move || {
            drop(
                gate.try_computer("other-id", "other", "Starting other")
                    .unwrap(),
            )
        })
        .join()
        .unwrap();
        drop(guard);
        assert!(matches!(
            launch_start_guard(gate, &paths, "deleted").unwrap(),
            LaunchAdmission::Unguarded
        ));
    }

    #[test]
    fn launch_start_hands_off_to_a_user_start_already_waiting_for_the_computer() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let gate: &'static operation_gate::OperationGate =
            Box::leak(Box::new(operation_gate::OperationGate::new()));
        let (release, released) = std::sync::mpsc::channel::<()>();
        let holder = std::thread::spawn(move || {
            let _checkpoint = gate
                .computer(computer().id(), "dev", "Creating checkpoint")
                .unwrap();
            released.recv().unwrap();
        });
        wait_for_queue(gate, |queue| queue.running.len() == 1);
        let user = std::thread::spawn(move || {
            gate.kind(operation_gate::OperationKind::Lifecycle)
                .acquire(
                    operation_gate::Scope::Computer {
                        id: computer().id().into(),
                    },
                    Some("dev".into()),
                    "Starting dev",
                    Some(format!("computer:{}:start", computer().id())),
                )
                .map(drop)
        });
        wait_for_queue(gate, |queue| queue.waiting.len() == 1);
        let launch_paths = paths.clone();
        let skipped = std::thread::spawn(move || {
            matches!(
                launch_start_guard(gate, &launch_paths, computer().id()),
                Ok(LaunchAdmission::Skipped)
            )
        })
        .join()
        .unwrap();
        assert!(skipped);
        release.send(()).unwrap();
        holder.join().unwrap();
        assert!(user.join().unwrap().is_ok());
    }

    #[test]
    fn launch_start_reports_a_fork_that_needs_its_first_explicit_start() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        pending_restore_record(&paths, false);
        let runner = StubRunner::successful_json(vec![]);
        assert_eq!(
            start_at_launch_with(&runner, &paths, &generous_device(), computer().id()).unwrap(),
            LaunchStart::NeedsExplicitStart("dev".into()),
        );
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn launch_starts_selected_existing_computer_and_verifies_running() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        for initial_status in ["Created", "Stopped"] {
            let runner = StubRunner::successful_json(vec![
                inspect(&paths, initial_status),
                inspect(&paths, initial_status),
                json!(null),
                inspect(&paths, "Running"),
            ]);
            start_at_launch_with(&runner, &paths, &generous_device(), computer().id()).unwrap();
            let calls = runner.calls.lock().unwrap();
            assert_eq!(calls[2], vec!["start", "dev", "--quiet"]);
            assert_eq!(calls[3], vec!["inspect", "dev", "--format", "json"]);
            assert!(!calls.iter().any(|call| call[0] == "create"));
        }
    }

    #[test]
    fn launch_rejects_a_running_replacement_instead_of_reporting_the_selected_computer_ready() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        for wrong_name in [false, true] {
            let mut observed = inspect(&paths, "Running");
            if wrong_name {
                observed["name"] = json!("replacement");
            } else {
                observed["config"]["labels"]["silo.machine-id"] =
                    json!(uuid::Uuid::new_v4().to_string());
            }
            let runner = StubRunner::successful_json(vec![observed]);
            let error = start_at_launch_with(&runner, &paths, &generous_device(), computer().id())
                .unwrap_err();
            assert!(error.to_string().contains("identity changed"), "{error}");
            assert!(
                runner
                    .calls
                    .lock()
                    .unwrap()
                    .iter()
                    .all(|call| call[0] == "inspect"),
                "no replacement may be started"
            );
        }
    }

    #[test]
    fn launch_skips_running_and_rejects_missing_and_unready() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let runner = StubRunner::successful_json(vec![inspect(&paths, "Running")]);
        start_at_launch_with(&runner, &paths, &generous_device(), computer().id()).unwrap();
        assert_eq!(runner.calls.lock().unwrap().len(), 1);
        assert!(start_at_launch_with(&runner, &paths, &generous_device(), "deleted").is_err());
        assert_eq!(runner.calls.lock().unwrap().len(), 1);
        let runner = StubRunner::successful_json(vec![
            inspect(&paths, "Stopped"),
            inspect(&paths, "Stopped"),
            json!(null),
            inspect(&paths, "Stopped"),
        ]);
        assert!(
            start_at_launch_with(&runner, &paths, &generous_device(), computer().id())
                .unwrap_err()
                .to_string()
                .contains("did not reach")
        );
    }

    #[test]
    fn launch_does_not_recover_crashed_or_transitioning_computers() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        for status in ["Crashed", "Starting", "Draining", "Paused"] {
            let runner = StubRunner::successful_json(vec![inspect(&paths, status)]);
            assert!(
                start_at_launch_with(&runner, &paths, &generous_device(), computer().id()).is_err()
            );
            assert_eq!(runner.calls.lock().unwrap().len(), 1);
        }
    }

    #[test]
    fn launch_respects_resources_and_ownership_without_starting() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let mut unowned = inspect(&paths, "Stopped");
        unowned["config"]["labels"] = json!({});
        let runner = StubRunner::successful_json(vec![unowned]);
        assert!(
            start_at_launch_with(&runner, &paths, &generous_device(), computer().id()).is_err()
        );
        let runner = StubRunner::successful_json(vec![
            inspect(&paths, "Stopped"),
            inspect(&paths, "Stopped"),
        ]);
        let device = DeviceResources {
            logical_cpus: 1,
            physical_memory_bytes: Some(1024),
        };
        assert!(start_at_launch_with(&runner, &paths, &device, computer().id()).is_err());
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call[0] == "start"));
    }

    #[test]
    fn lifecycle_checks_metadata_and_runtime_ownership_before_mutation() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let runner = StubRunner::successful_json(vec![
            inspect(&paths, "Stopped"),
            json!(null),
            inspect(&paths, "Running"),
        ]);

        computer_action_with(&runner, &paths, &generous_device(), "start", "dev").unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0], vec!["inspect", "dev", "--format", "json"]);
        assert_eq!(calls[1], vec!["start", "dev", "--quiet"]);
    }

    #[test]
    fn lifecycle_does_not_report_success_when_runtime_stays_stopped() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let runner = StubRunner::successful_json(vec![
            inspect(&paths, "Running"),
            json!(null),
            inspect(&paths, "Stopped"),
            json!(null),
            inspect(&paths, "Stopped"),
        ]);
        let error = computer_action_with(&runner, &paths, &generous_device(), "restart", "dev")
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("did not reach the Running state"));
    }

    #[test]
    fn lifecycle_refuses_a_runtime_row_without_silo_ownership() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let mut unowned = inspect(&paths, "Stopped");
        unowned["config"]["labels"] = json!({});
        let runner = StubRunner::successful_json(vec![unowned]);

        let error =
            computer_action_with(&runner, &paths, &generous_device(), "start", "dev").unwrap_err();
        assert!(error.to_string().contains("not owned by Silo"));
        assert_eq!(runner.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn validation_rejects_duplicates_and_invalid_resource_order() {
        let _test_state = crate::test_support::global_state();
        let mut duplicate = computer();
        {
            let ComputerConfiguration { name, .. } = &mut duplicate;
            *name = "dev".into();
        }
        assert!(validate_request(&request(vec![computer(), duplicate])).is_err());

        let mut invalid = computer();
        {
            let ComputerConfiguration { cpus, max_cpus, .. } = &mut invalid;
            *cpus = 8;
            *max_cpus = 4;
        }
        assert!(validate_request(&request(vec![invalid])).is_err());
    }

    #[test]
    fn saved_configuration_is_available_without_runtime_files() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let expected = request(vec![computer()]);
        write_metadata(&paths.metadata, &expected).unwrap();

        assert_eq!(read_metadata(&paths.metadata).unwrap(), expected);
        assert!(!paths.executable.exists());
        assert!(!paths.home.exists());
        assert!(!paths.library.exists());
    }

    #[test]
    fn saved_configuration_distinguishes_first_launch_from_invalid_data() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("computers.json");
        assert!(read_metadata(&path).unwrap().computers.is_empty());

        fs::write(&path, b"not json").unwrap();
        assert!(read_metadata(&path).is_err());
        fs::write(&path, br#"{"schemaVersion":1,"computers":[]}"#).unwrap();
        assert!(read_metadata(&path).unwrap().computers.is_empty());
    }

    #[test]
    fn pre_desktop_metadata_keeps_its_persisted_field_names_on_round_trip() {
        let saved = json!({
            "schemaVersion": 1,
            "computers": [
                {
                    "id": "00000000-0000-4000-8000-000000000001",
                    "name": "dev",
                    "cpus": 2,
                    "maxCPUs": 4,
                    "memoryGiB": 2,
                    "maxMemoryGiB": 4,
                    "workspaceStorageGiB": 10,
                    "runtimeStorageGiB": 5
                }
            ]
        });
        let request: ComputerConfigurationRequest = serde_json::from_value(saved.clone()).unwrap();
        validate_request(&request).unwrap();
        assert!(matches!(
            request.computers[0],
            ComputerConfiguration { desktop: None, .. }
        ));
        assert_eq!(serde_json::to_value(&request).unwrap(), saved);
    }

    #[test]
    fn metadata_round_trip_is_atomic_and_preserves_split_storage_settings() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let expected = request(vec![computer()]);

        write_metadata(&paths.metadata, &expected).unwrap();

        assert_eq!(read_metadata(&paths.metadata).unwrap(), expected);
    }

    #[test]
    fn measured_resources_reject_impossible_cpu_and_memory_requests() {
        let _test_state = crate::test_support::global_state();
        let request = request(vec![computer()]);
        let constrained_cpu = DeviceResources {
            logical_cpus: 4,
            physical_memory_bytes: Some(256 * 1024 * 1024 * 1024),
        };
        assert!(validate_requested_resources(&request, &constrained_cpu)
            .unwrap_err()
            .to_string()
            .contains("reports 4 logical CPUs"));

        let constrained_memory = DeviceResources {
            logical_cpus: 64,
            physical_memory_bytes: Some(31 * 1024 * 1024 * 1024),
        };
        assert!(validate_requested_resources(&request, &constrained_memory)
            .unwrap_err()
            .to_string()
            .contains("reports 31 GiB"));
    }

    #[test]
    fn removal_cleans_computer_only_after_runtime_removal_succeeds() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let disk = disk_path(&paths, "dev", "workspace");
        fs::create_dir_all(disk.parent().unwrap()).unwrap();
        fs::write(&disk, b"workspace-data").unwrap();
        let failed = StubRunner::new(vec![
            Ok(CommandOutput {
                stdout: inspect(&paths, "Stopped").to_string(),
                stderr: String::new(),
            }),
            Err(RuntimeError::Unavailable("runtime removal failed".into())),
        ]);
        assert!(remove_computer(&failed, &paths, &computer()).is_err());
        assert_eq!(fs::read(&disk).unwrap(), b"workspace-data");
        let successful = StubRunner::successful_json(vec![inspect(&paths, "Stopped"), json!(null)]);
        remove_computer(&successful, &paths, &computer()).unwrap();
        assert!(!disk.exists());
    }

    #[test]
    fn empty_setup_saves_and_verifies_without_initializing_runtime() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let runner = StubRunner::successful_json(vec![]);
        let request = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![],
        };
        apply_whole_configuration_with_progress(
            &runner,
            &paths,
            &generous_device(),
            request.clone(),
            None,
            &|_, _, _| {},
        )
        .unwrap();
        configure_computer_identities_with(&runner, &paths, &[]).unwrap();
        assert!(verify_computer_identities_with(&runner, &paths, &[]).unwrap());
        read_application_state_with(&runner, &paths).unwrap();
        assert_eq!(read_metadata(&paths.metadata).unwrap(), request);
        assert!(!paths.home.exists());
        // Empty identity input must not verify a configured computer's identity.
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![computer()],
            },
        )
        .unwrap();
        assert!(!verify_computer_identities_with(&runner, &paths, &[]).unwrap());
    }

    fn test_identity() -> ComputerIdentity {
        ComputerIdentity {
            computer: "dev".into(),
            name: "Test User".into(),
            email: "test@example.com".into(),
            apply: true,
        }
    }

    fn identity_output(value: &str) -> Result<CommandOutput, RuntimeError> {
        Ok(CommandOutput {
            stdout: value.into(),
            stderr: String::new(),
        })
    }

    #[test]
    fn git_identity_rejects_a_replacement_before_writing_or_verifying() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let mut replacement = inspect(&paths, "Running");
        replacement["config"]["labels"]["silo.machine-id"] =
            json!("22222222-2222-4222-8222-222222222222");
        let writer = StubRunner::new(vec![
            identity_output(&replacement.to_string()),
            identity_output("{}"),
            identity_output(""),
            identity_output("silo-identity-verified"),
        ]);
        let verifier = StubRunner::new(vec![
            identity_output(&replacement.to_string()),
            identity_output("silo-identity-verified"),
        ]);

        assert!(configure_computer_identities_with(&writer, &paths, &[test_identity()]).is_err());
        assert!(verify_computer_identities_with(&verifier, &paths, &[test_identity()]).is_err());
        for runner in [writer, verifier] {
            assert!(runner
                .calls
                .lock()
                .unwrap()
                .iter()
                .all(|args| args[0] == "inspect"));
        }
    }

    #[test]
    fn git_identity_rechecks_runtime_identity_after_preflight() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let mut replacement = inspect(&paths, "Running");
        replacement["config"]["labels"]["silo.machine-id"] =
            json!("22222222-2222-4222-8222-222222222222");
        let runner = StubRunner::new(vec![
            identity_output(&inspect(&paths, "Running").to_string()),
            identity_output(&replacement.to_string()),
            identity_output(""),
            identity_output("silo-identity-verified"),
        ]);

        assert!(configure_computer_identities_with(&runner, &paths, &[test_identity()]).is_err());
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|args| args[0] == "inspect"));
    }

    #[test]
    fn git_identity_rechecks_the_computer_after_admission() {
        let _test_state = crate::test_support::global_state();
        for verify_only in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
            let lane = |_: &ComputerConfiguration, _: &str| {
                let mut replacement = computer();
                {
                    let ComputerConfiguration { id, .. } = &mut replacement;
                    *id = "22222222-2222-4222-8222-222222222222".into();
                }
                write_metadata(&paths.metadata, &request(vec![replacement]))?;
                Ok(None)
            };
            let runner = StubRunner::new(if verify_only {
                vec![
                    identity_output(&inspect(&paths, "Running").to_string()),
                    identity_output("silo-identity-verified"),
                ]
            } else {
                vec![
                    identity_output(&inspect(&paths, "Running").to_string()),
                    identity_output("{}"),
                    identity_output(""),
                    identity_output("silo-identity-verified"),
                ]
            });

            let rejected = if verify_only {
                verify_computer_identities_in(&runner, &paths, &[test_identity()], &lane).is_err()
            } else {
                configure_computer_identities_in(&runner, &paths, &[test_identity()], &lane)
                    .is_err()
            };

            assert!(rejected);
            assert!(runner
                .calls
                .lock()
                .unwrap()
                .iter()
                .all(|args| args[0] == "inspect"));
        }
    }

    #[test]
    fn identity_resume_verifies_guest_files_not_boot_environment() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let runner = StubRunner::new(vec![
            identity_output(&inspect(&paths, "Running").to_string()),
            identity_output("silo-identity-verified"),
        ]);
        assert!(verify_computer_identities_with(&runner, &paths, &[test_identity()]).unwrap());
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[1][0], "exec");
        assert!(!calls.iter().any(|args| args[0] == "modify"));
    }

    #[test]
    fn working_account_git_identity_uses_the_same_home_for_write_and_verification() {
        let _test_state = crate::test_support::global_state();
        let user = "silo";
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let state = inspect(&paths, "Running");
        let runner = StubRunner::new(vec![
            identity_output(&state.to_string()),
            identity_output(&state.to_string()),
            identity_output("{}"),
            identity_output(""),
            identity_output("silo-identity-verified"),
        ]);
        configure_computer_identities_with(&runner, &paths, &[test_identity()]).unwrap();
        let calls = runner.calls.lock().unwrap();
        for command in calls.iter().filter(|args| args[0] == "exec") {
            assert!(command.windows(2).any(|pair| pair == ["--user", user]));
        }
    }

    #[test]
    fn running_identity_change_uses_normal_config_without_restart() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let mut identity = test_identity();
        identity.name = "O'Neil $(touch /tmp/unsafe)".into();
        let runner = StubRunner::new(vec![
            identity_output(&inspect(&paths, "Running").to_string()),
            identity_output(&inspect(&paths, "Running").to_string()),
            identity_output("{}"),
            identity_output(""),
            identity_output("silo-identity-verified"),
        ]);
        configure_computer_identities_with(&runner, &paths, &[identity]).unwrap();
        let calls = runner.calls.lock().unwrap();
        assert!(calls[2].iter().any(|arg| arg == "--env-rm"));
        assert_eq!(calls[3][0], "exec");
        assert!(calls[3]
            .iter()
            .any(|arg| arg == "O'Neil $(touch /tmp/unsafe)"));
        assert!(!calls
            .iter()
            .any(|args| ["restart", "stop", "start"].contains(&args[0].as_str())));
    }

    #[test]
    fn saving_an_identity_clears_its_boot_overrides_from_the_checkpoint_record() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let mut record = checkpoints::load(&paths, computer().id()).unwrap();
        record.set_environment_for_test(&[
            ("PROJECT_MODE", "kept"),
            ("GIT_AUTHOR_NAME", "Old Author"),
            ("JJ_USER", "Old Author"),
        ]);
        checkpoints::save(&paths, computer().id(), &record).unwrap();
        let state = inspect(&paths, "Running").to_string();
        let runner = StubRunner::new(vec![
            identity_output(&state),
            identity_output(&state),
            identity_output("{}"),
            identity_output(""),
            identity_output("silo-identity-verified"),
        ]);
        configure_computer_identities_with(&runner, &paths, &[test_identity()]).unwrap();
        assert_eq!(
            checkpoints::load(&paths, computer().id())
                .unwrap()
                .environment_keys_for_test(),
            ["PROJECT_MODE"]
        );
    }

    #[test]
    fn identity_verification_never_boots_a_stopped_computer() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        // A stopped computer's identity is unknown without booting it: not verified, no exec.
        let stopped = StubRunner::new(vec![identity_output(
            &inspect(&paths, "Stopped").to_string(),
        )]);
        assert!(!verify_computer_identities_with(&stopped, &paths, &[test_identity()]).unwrap());
        assert!(stopped
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| args[0] == "inspect"));
        // A running computer is checked in place, never through exec's temporary boot.
        let running = StubRunner::new(vec![
            identity_output(&inspect(&paths, "Running").to_string()),
            identity_output("silo-identity-verified"),
        ]);
        assert!(verify_computer_identities_with(&running, &paths, &[test_identity()]).unwrap());
        let calls = running.calls.lock().unwrap();
        let options: Vec<&str> = calls[1]
            .iter()
            .take_while(|arg| *arg != "--")
            .map(String::as_str)
            .collect();
        assert!(options.contains(&"--no-start"), "{options:?}");
    }

    #[test]
    fn identity_work_holds_one_named_cancellable_computer_lane_at_a_time() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut other = computer();
        {
            let ComputerConfiguration { id, name, .. } = &mut other;
            *id = "00000000-0000-4000-8000-000000000002".into();
            *name = "work".into();
        }
        write_metadata(&paths.metadata, &request(vec![computer(), other.clone()])).unwrap();
        let lanes = Mutex::new(Vec::new());
        let lane = |configuration: &ComputerConfiguration, label: &str| {
            let guard = identity_lane(configuration, label)?;
            let queue = OPERATIONS.snapshot();
            assert!(
                OPERATIONS.is_device_idle(),
                "identity work must not take the whole device"
            );
            let entry = queue
                .running
                .iter()
                .find(|entry| entry.computer_id.as_deref() == Some(configuration.id()))
                .unwrap();
            assert!(entry.cancellable);
            lanes.lock().unwrap().push(entry.label.clone());
            Ok(guard)
        };
        let mut work_inspect = inspect(&paths, "Running");
        work_inspect["name"] = json!("work");
        work_inspect["config"]["labels"]["silo.machine-id"] = json!(other.id());
        let runner = StubRunner::new(vec![
            identity_output(&inspect(&paths, "Running").to_string()),
            identity_output("silo-identity-verified"),
            identity_output(&work_inspect.to_string()),
            identity_output("silo-identity-verified"),
        ]);
        let mut work = test_identity();
        work.computer = "work".into();
        assert!(
            verify_computer_identities_in(&runner, &paths, &[test_identity(), work], &lane)
                .unwrap()
        );
        assert_eq!(
            *lanes.lock().unwrap(),
            vec![
                "Checking Git identity for dev",
                "Checking Git identity for work"
            ]
        );
        assert!(
            OPERATIONS.is_computer_idle(computer().id()) && OPERATIONS.is_computer_idle(other.id())
        );
        lanes.lock().unwrap().clear();
        let writer = StubRunner::new(vec![
            identity_output(&inspect(&paths, "Running").to_string()),
            identity_output(&inspect(&paths, "Running").to_string()),
            identity_output("{}"),
            identity_output(""),
            identity_output("silo-identity-verified"),
        ]);
        configure_computer_identities_in(&writer, &paths, &[test_identity()], &lane).unwrap();
        assert_eq!(*lanes.lock().unwrap(), vec!["Saving Git identity for dev"]);
    }

    #[test]
    fn identity_missing_or_failed_guest_verification_never_succeeds() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let runner = StubRunner::new(vec![
            identity_output(&inspect(&paths, "Running").to_string()),
            identity_output(""),
        ]);
        assert!(!verify_computer_identities_with(&runner, &paths, &[test_identity()]).unwrap());
        let failed = StubRunner::new(vec![
            identity_output(&inspect(&paths, "Running").to_string()),
            Err(RuntimeError::Unavailable("guest unavailable".into())),
        ]);
        assert!(verify_computer_identities_with(&failed, &paths, &[test_identity()]).is_err());
    }

    #[test]
    fn unapplied_identity_does_not_modify_computer() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let runner = StubRunner::new(vec![]);
        configure_computer_identities_with(
            &runner,
            &paths,
            &[ComputerIdentity {
                computer: "dev".into(),
                name: String::new(),
                email: String::new(),
                apply: false,
            }],
        )
        .unwrap();
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn unknown_and_overflowing_resource_checks_never_pass() {
        let _test_state = crate::test_support::global_state();
        let unknown = DeviceResources {
            logical_cpus: 8,
            physical_memory_bytes: None,
        };
        assert!(validate_device_ceiling("dev", 1, 1, &unknown).is_err());
        assert!(validate_device_ceiling("dev", 0, 1, &generous_device()).is_err());
        assert!(validate_inspected_resources(
            "dev",
            &json!({"resources": {"max_cpus": 1, "max_memory_mib": u64::MAX}}),
            &generous_device()
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn runtime_alias_is_short_and_preserves_existing_storage() {
        let _test_state = crate::test_support::global_state();
        // A short root keeps the modeled control.sock under macOS's 104-byte limit.
        let directory = tempfile::Builder::new()
            .prefix("silo")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let storage = directory
            .path()
            .join("long-application-support-path/runtime/microsandbox");
        fs::create_dir_all(&storage).unwrap();
        fs::write(storage.join("existing-computer-data"), b"preserved").unwrap();
        let alias = runtime_home_alias(directory.path(), &storage);
        prepare_runtime_home(&alias, Some(&storage)).unwrap();
        assert_eq!(fs::read_link(&alias).unwrap(), storage);
        let socket = alias.join("run/sandboxes/000000000000000000000000/control.sock");
        fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert_eq!(
            fs::read(alias.join("existing-computer-data")).unwrap(),
            b"preserved"
        );
        assert!(
            alias
                .join("run/sandboxes/000000000000000000000000/control.sock")
                .as_os_str()
                .as_encoded_bytes()
                .len()
                <= 103
        );
        assert_eq!(
            fs::read(storage.join("existing-computer-data")).unwrap(),
            b"preserved"
        );
        prepare_runtime_home(&alias, Some(&storage)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn runtime_alias_secures_existing_parent_without_changing_its_contents() {
        let _test_state = crate::test_support::global_state();
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // A short root keeps the modeled control.sock under macOS's 104-byte limit.
        let directory = tempfile::Builder::new()
            .prefix("silo")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let storage = directory.path().join("storage");
        let alias = runtime_home_alias(directory.path(), &storage);
        let parent = alias.parent().unwrap();
        fs::create_dir(parent).unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o775)).unwrap();
        fs::write(parent.join("existing"), b"preserved").unwrap();
        prepare_runtime_home(&alias, Some(&storage)).unwrap();
        assert_eq!(fs::metadata(parent).unwrap().mode() & 0o777, 0o700);
        assert_eq!(fs::read(parent.join("existing")).unwrap(), b"preserved");
    }

    #[cfg(unix)]
    #[test]
    fn runtime_home_is_private_with_permissive_umask() {
        const CHILD: &str = "SILO_PRIVATE_RUNTIME_HOME_TEST";
        if std::env::var_os(CHILD).is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "runtime::tests::runtime_home_is_private_with_permissive_umask",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // This process runs only this test, so its umask cannot affect parallel tests.
        unsafe { libc::umask(0) };
        let directory = tempfile::Builder::new()
            .prefix("silo")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let storage = directory.path().join("generation/microsandbox");
        let alias = runtime_home_alias(directory.path(), &storage);
        prepare_runtime_home(&alias, Some(&storage)).unwrap();
        assert_eq!(fs::metadata(&storage).unwrap().mode() & 0o777, 0o700);
        assert_eq!(
            fs::metadata(storage.parent().unwrap()).unwrap().mode() & 0o777,
            0o700
        );
        fs::write(storage.join("private-config"), b"fixture").unwrap();
        fs::set_permissions(&storage, fs::Permissions::from_mode(0o755)).unwrap();
        prepare_runtime_home(&alias, Some(&storage)).unwrap();
        assert_eq!(fs::metadata(&storage).unwrap().mode() & 0o777, 0o700);
        assert_eq!(
            fs::read(storage.join("private-config")).unwrap(),
            b"fixture"
        );
        let standalone = directory.path().join("standalone");
        prepare_runtime_home(&standalone, None).unwrap();
        assert_eq!(fs::metadata(standalone).unwrap().mode() & 0o777, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn runtime_alias_rejects_symlink_parent_without_changing_target_permissions() {
        let _test_state = crate::test_support::global_state();
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // A short root keeps the modeled control.sock under macOS's 104-byte limit.
        let directory = tempfile::Builder::new()
            .prefix("silo")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let other = directory.path().join("other");
        fs::create_dir(&other).unwrap();
        fs::set_permissions(&other, fs::Permissions::from_mode(0o775)).unwrap();
        let storage = directory.path().join("storage");
        let alias = runtime_home_alias(directory.path(), &storage);
        std::os::unix::fs::symlink(&other, alias.parent().unwrap()).unwrap();
        assert!(prepare_runtime_home(&alias, Some(&storage)).is_err());
        assert_eq!(fs::metadata(other).unwrap().mode() & 0o777, 0o775);
        assert!(!storage.exists());
    }

    #[cfg(unix)]
    #[test]
    fn runtime_alias_reports_existing_directory_and_preserves_data() {
        let _test_state = crate::test_support::global_state();
        // A short root keeps the modeled control.sock under macOS's 104-byte limit.
        let directory = tempfile::Builder::new()
            .prefix("silo")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let storage = directory.path().join("storage");
        let alias = runtime_home_alias(directory.path(), &storage);
        fs::create_dir_all(&alias).unwrap();
        fs::write(alias.join("data"), b"preserved").unwrap();
        let error = prepare_runtime_home(&alias, Some(&storage))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a symbolic link"));
        assert!(error.contains(&alias.display().to_string()));
        assert!(error.contains(&storage.display().to_string()));
        assert_eq!(fs::read(alias.join("data")).unwrap(), b"preserved");
        assert!(!storage.exists());
    }

    #[cfg(unix)]
    #[test]
    fn runtime_alias_is_prepared_before_configuration_lock_on_first_run() {
        let _test_state = crate::test_support::global_state();
        // A short root keeps the modeled control.sock under macOS's 104-byte limit.
        let directory = tempfile::Builder::new()
            .prefix("silo")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let mut paths = paths(&directory);
        let storage = directory.path().join("storage");
        paths.home = runtime_home_alias(directory.path(), &storage);
        paths.storage_home = Some(storage.clone());
        let lock = configuration_recovery::command_lock(&paths, Duration::ZERO).unwrap();
        assert_eq!(fs::read_link(&paths.home).unwrap(), storage);
        prepare_runtime_home(&paths.home, paths.storage_home.as_deref()).unwrap();
        assert!(storage.join(".silo-configuration-worker.lock").is_file());
        drop(lock);
    }

    #[cfg(unix)]
    #[test]
    fn runtime_alias_never_replaces_an_existing_wrong_target() {
        let _test_state = crate::test_support::global_state();
        // A short root keeps the modeled control.sock under macOS's 104-byte limit.
        let directory = tempfile::Builder::new()
            .prefix("silo")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let storage = directory.path().join("intended");
        let other = directory.path().join("existing");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("data"), b"preserved").unwrap();
        let alias = runtime_home_alias(directory.path(), &storage);
        fs::create_dir(alias.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&other, &alias).unwrap();
        assert!(prepare_runtime_home(&alias, Some(&storage)).is_err());
        assert_eq!(fs::read_link(&alias).unwrap(), other);
        assert_eq!(fs::read(other.join("data")).unwrap(), b"preserved");
        assert!(!storage.exists());
    }

    #[test]
    fn resolving_runtime_paths_does_not_require_runtime_files_or_manifest() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("Silo.app/Contents/MacOS/msb");
        let resource_dir = directory.path().join("Silo.app/Contents/Resources");
        let library = bundled_runtime_library(
            &executable,
            &resource_dir,
            Some(tauri::utils::config::BundleType::App),
        );
        assert!(!library.exists());
        #[cfg(target_os = "macos")]
        assert_eq!(
            library,
            directory
                .path()
                .join("Silo.app/Contents/Frameworks/libkrunfw.5.dylib")
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            bundled_runtime_library(
                &crate::bundled_tools::resolve(
                    Path::new("/usr/bin/silo-ui"),
                    &resource_dir,
                    Some(tauri::utils::config::BundleType::Deb),
                    None,
                )
                .unwrap()
                .join("msb"),
                &resource_dir,
                Some(tauri::utils::config::BundleType::Deb),
            ),
            Path::new("/usr/libexec/silo/tools/libkrunfw.so.5.6.1")
        );
        let paths = RuntimePaths {
            guest_image: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("runtime/guest-image"),
            storage_home: None,
            executable,
            library,
            home: directory.path().join("home"),
            metadata: directory.path().join("computers.json"),
            volumes: directory.path().join("volumes"),
        };
        assert!(matches!(
            run_msb(&paths, &["list".into()], READ_TIMEOUT),
            Err(RuntimeError::Unavailable(_))
        ));
    }

    #[test]
    fn host_resource_probe_returns_measured_cpu_and_memory() {
        let _test_state = crate::test_support::global_state();
        let measured = device_resources().unwrap();
        assert!(measured.logical_cpus > 0);
        assert!(measured
            .physical_memory_bytes
            .is_some_and(|bytes| bytes > 0));
    }

    #[test]
    fn configuration_progress_reports_real_boundaries_and_never_false_verification() {
        let _test_state = crate::test_support::global_state();
        for fail_verification in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            let mut final_state = inspect(&paths, "Created");
            if fail_verification {
                final_state["config"]["resources"]["cpus"] = json!(1);
            }
            let runner = StubRunner::successful_json(vec![
                json!([]),
                json!(1),
                json!(1),
                json!(null),
                inspect(&paths, "Created"),
                json!(null),
                inspect(&paths, "Stopped"),
                final_state,
            ]);
            let events = Mutex::new(Vec::new());
            let report = |step: &str, computer: &str, fraction: u8| {
                events.lock().unwrap().push(computer_progress(
                    "request-1",
                    step,
                    computer,
                    fraction,
                ))
            };
            let result = apply_whole_configuration_with_progress(
                &runner,
                &paths,
                &generous_device(),
                request(vec![computer()]),
                None,
                &report,
            );
            assert_eq!(result.is_err(), fail_verification);
            let events = events.lock().unwrap();
            let boundaries: Vec<_> = events
                .iter()
                .filter_map(|event| {
                    event
                        .fraction
                        .map(|fraction| (event.step.as_str(), fraction))
                })
                .collect();
            let mut expected = vec![
                ("computer-configuration", 0),
                ("computer-configuration", 1),
                ("computer-verification", 0),
            ];
            if !fail_verification {
                expected.push(("computer-verification", 1));
            }
            assert_eq!(boundaries, expected);
            for event in events.iter() {
                let encoded = serde_json::to_value(event).unwrap();
                assert_eq!(encoded["type"], "progress");
                assert_eq!(encoded["requestId"], "request-1");
                assert_eq!(encoded["computer"], "dev");
                assert_eq!(encoded["safeForDisplay"], true);
            }
        }
    }

    #[test]
    fn unavailable_local_runtime_shell_preserves_error_without_inventing_computer_state() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let source = application_shell(&paths, "Local runtime could not be reached.").unwrap();
        assert!(source.computers.is_empty());
        assert_eq!(
            source.runtime_repair.unwrap()["reason"],
            "Local runtime could not be reached."
        );
        assert!(!paths.executable.exists());
        assert_eq!(
            read_metadata(&paths.metadata).unwrap(),
            request(vec![computer()])
        );
    }

    #[test]
    fn deleting_last_computer_persists_empty_inventory_without_requiring_runtime_for_snapshot() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let inspected = inspect(&paths, "Stopped");
        let runner = StubRunner::successful_json(vec![inspected.clone(), inspected, json!(null)]);
        apply_whole_configuration(&runner, &paths, &generous_device(), request(vec![])).unwrap();
        assert!(read_metadata(&paths.metadata).unwrap().computers.is_empty());
        let unavailable_runtime = StubRunner::successful_json(vec![]);
        let source = read_application_state_with(&unavailable_runtime, &paths).unwrap();
        assert!(source.computers.is_empty());
        assert!(unavailable_runtime.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn network_mappings_are_removed_before_a_deleted_name_can_be_reused() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        let network = paths.metadata.with_file_name("network.json");
        fs::write(
            &network,
            json!({"mappings":[
                {"computer":"dev","port":3000,"hostPort":43000,"scheme":"http","enabled":true},
                {"computer":"other","port":8080,"hostPort":null,"scheme":null,"enabled":true}
            ]})
            .to_string(),
        )
        .unwrap();
        let inspected = inspect(&paths, "Stopped");
        let runner = StubRunner::successful_json(vec![inspected.clone(), inspected, json!(null)]);
        apply_whole_configuration(&runner, &paths, &generous_device(), request(vec![])).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(&network).unwrap()).unwrap();
        assert_eq!(saved["mappings"].as_array().unwrap().len(), 1);
        assert_eq!(saved["mappings"][0]["computer"], "other");
        assert!(read_metadata(&paths.metadata).unwrap().computers.is_empty());
    }

    #[test]
    fn network_cleanup_failure_keeps_the_deleted_name_reserved_for_recovery() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let previous = request(vec![computer()]);
        write_metadata(&paths.metadata, &previous).unwrap();
        fs::write(paths.metadata.with_file_name("network.json"), "invalid").unwrap();
        let inspected = inspect(&paths, "Stopped");
        let runner = StubRunner::successful_json(vec![inspected.clone(), inspected, json!(null)]);
        let error = apply_whole_configuration(&runner, &paths, &generous_device(), request(vec![]))
            .unwrap_err();
        assert!(error.to_string().contains("Saved ports are invalid"));
        assert_eq!(read_metadata(&paths.metadata).unwrap(), previous);
    }

    /// The runtime's computer `dev` as the bundled `msb` treats it: `inspect` reports its
    /// status, and `remove` accepts one that is `Created` (never started), `Stopped` or
    /// `Crashed` and refuses any other with the runtime's own error. Every call is recorded.
    struct RemovalRuntime {
        status: &'static str,
        paths: RuntimePaths,
        removed: Mutex<bool>,
        calls: Mutex<Vec<String>>,
    }

    impl RuntimeRunner for RemovalRuntime {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.join(" "));
            let stdout = match args
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice()
            {
                ["inspect", "dev", "--format", "json"] => {
                    inspect(&self.paths, self.status).to_string()
                }
                ["remove", "--quiet", "dev"] => {
                    if !matches!(self.status, "Created" | "Stopped" | "Crashed") {
                        return Err(RuntimeError::Failed {
                            operation: "Removing the computer".into(),
                            exit_code: Some(1),
                            detail: format!(
                                "computer still running: cannot remove computer \"dev\": status is {}",
                                self.status
                            ),
                        });
                    }
                    *self.removed.lock().unwrap() = true;
                    String::new()
                }
                ["snapshot", "list", "--format", "json"] | ["list", "--format", "json"] => {
                    "[]".into()
                }
                other => panic!("unexpected runtime command: {other:?}"),
            };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn deleting_a_computer_that_never_started_removes_it_from_the_runtime_and_its_disk() {
        let _test_state = crate::test_support::global_state();
        // A computer whose first start never happened is `Created`: a setup that failed before
        // its first boot, a fork or restore not yet started, an interrupted import's orphan.
        // Silo treats it as stopped, and the runtime now removes it the same way.
        for status in ["Created", "Stopped", "Crashed"] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
            let disk = disk_path(&paths, "dev", "workspace");
            fs::create_dir_all(disk.parent().unwrap()).unwrap();
            fs::write(&disk, b"workspace-data").unwrap();
            let runner = RemovalRuntime {
                status,
                paths: paths.clone(),
                removed: Mutex::new(false),
                calls: Mutex::new(Vec::new()),
            };

            apply_whole_configuration(&runner, &paths, &generous_device(), request(vec![]))
                .unwrap_or_else(|error| panic!("{status}: {error}"));

            let calls = runner.calls.lock().unwrap().clone();
            assert!(
                calls.iter().any(|call| call == "remove --quiet dev"),
                "{status}: {calls:?}"
            );
            assert!(*runner.removed.lock().unwrap(), "{status}");
            assert!(read_metadata(&paths.metadata).unwrap().computers.is_empty());
            assert!(!disk.exists(), "{status}: the disk goes with the computer");
        }
    }

    #[test]
    fn deleting_a_computer_the_runtime_would_refuse_to_remove_keeps_everything() {
        let _test_state = crate::test_support::global_state();
        for status in ["Running", "Starting", "Draining", "Paused"] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
            let disk = disk_path(&paths, "dev", "workspace");
            fs::create_dir_all(disk.parent().unwrap()).unwrap();
            fs::write(&disk, b"workspace-data").unwrap();
            let runner = RemovalRuntime {
                status,
                paths: paths.clone(),
                removed: Mutex::new(false),
                calls: Mutex::new(Vec::new()),
            };

            let error = remove_computer(&runner, &paths, &computer()).unwrap_err();

            // Silo refuses first, so `remove` is never issued for a computer that may run.
            assert!(
                error.to_string().contains("Stop computer 'dev'"),
                "{status}"
            );
            assert_eq!(
                *runner.calls.lock().unwrap(),
                ["inspect dev --format json"],
                "{status}"
            );
            assert!(!*runner.removed.lock().unwrap(), "{status}");
            assert_eq!(fs::read(&disk).unwrap(), b"workspace-data", "{status}");
        }
    }

    #[test]
    fn adding_or_removing_a_computer_does_not_recheck_or_report_unchanged_computers() {
        let _test_state = crate::test_support::global_state();
        for (removing, retry_computer) in [(false, None), (true, None), (true, Some("work"))] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            let mut other = computer();
            {
                let ComputerConfiguration { id, name, .. } = &mut other;
                *id = "00000000-0000-4000-8000-000000000002".into();
                *name = "work".into();
            }
            let mut other_inspect = inspect(&paths, "Stopped");
            other_inspect["name"] = json!("work");
            other_inspect["config"]["name"] = json!("work");
            other_inspect["config"]["labels"]["silo.machine-id"] = json!(other.id());
            let previous = if removing {
                request(vec![computer(), other.clone()])
            } else {
                request(vec![computer()])
            };
            let requested = if removing {
                request(vec![computer()])
            } else {
                request(vec![computer(), other])
            };
            write_metadata(&paths.metadata, &previous).unwrap();
            let runner = StubRunner::successful_json(if removing {
                vec![other_inspect.clone(), other_inspect, json!(null)]
            } else {
                vec![
                    json!([]),
                    json!(1),
                    json!(1),
                    json!(null),
                    other_inspect.clone(),
                    json!(null),
                    other_inspect.clone(),
                    other_inspect,
                ]
            });
            let events = Mutex::new(Vec::new());
            apply_whole_configuration_with_progress(
                &runner,
                &paths,
                &generous_device(),
                requested.clone(),
                retry_computer,
                &|step, name, fraction| {
                    events
                        .lock()
                        .unwrap()
                        .push((step.to_string(), name.to_string(), fraction));
                },
            )
            .unwrap();
            assert_eq!(read_metadata(&paths.metadata).unwrap(), requested);
            assert!(!events.lock().unwrap().is_empty());
            assert!(events
                .lock()
                .unwrap()
                .iter()
                .all(|(_, name, _)| name == "work"));
            assert!(runner
                .calls
                .lock()
                .unwrap()
                .iter()
                .all(|args| !args.iter().any(|arg| arg == "dev")));
        }
    }

    #[test]
    fn interrupted_verification_resumes_only_until_it_completes() {
        let _test_state = crate::test_support::global_state();
        let started = computer_progress("request-1", "computer-verification", "dev", 0);
        let failed = computer_progress("request-1", "setup-failed", "dev", 0);
        assert_eq!(
            pending_verification_computer(&[started.clone(), failed]),
            Some("dev".into())
        );
        let done = computer_progress("request-1", "computer-verification", "dev", 1);
        assert_eq!(
            pending_verification_computer(&[started.clone(), done]),
            None
        );
        let completed = computer_progress("request-1", "setup-completed", "", 0);
        assert_eq!(pending_verification_computer(&[started, completed]), None);
    }

    #[test]
    fn retry_checks_unchanged_saved_settings_for_runtime_resource_mismatch() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let configured = request(vec![computer()]);
        write_metadata(&paths.metadata, &configured).unwrap();
        let mut mismatch = inspect(&paths, "Stopped");
        mismatch["config"]["resources"]["cpus"] = json!(1);
        let runner = StubRunner::successful_json(vec![mismatch]);
        let error = apply_whole_configuration_with_progress(
            &runner,
            &paths,
            &generous_device(),
            configured,
            Some("dev"),
            &|_, _, _| {},
        )
        .unwrap_err();
        assert!(error.to_string().contains("do not match"));
        assert!(runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| args[0] == "inspect"));
    }

    #[test]
    fn removal_preflight_checks_every_computer_before_deleting_any() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let first = computer();
        let mut second = computer();
        {
            let ComputerConfiguration { id, name, .. } = &mut second;
            *id = "00000000-0000-4000-8000-000000000002".into();
            *name = "work".into();
        }
        let remote = ComputerConfiguration {
            id: "00000000-0000-4000-8000-000000000003".into(),
            name: "keep".into(),
            ..computer()
        };
        let previous = request(vec![first, second, remote.clone()]);
        write_metadata(&paths.metadata, &previous).unwrap();
        let runner = StubRunner::successful_json(vec![
            inspect(&paths, "Stopped"),
            inspect(&paths, "Running"),
        ]);
        assert!(apply_whole_configuration(
            &runner,
            &paths,
            &generous_device(),
            request(vec![remote])
        )
        .is_err());
        assert!(runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| args[0] == "inspect"));
        assert_eq!(read_metadata(&paths.metadata).unwrap(), previous);
    }

    #[test]
    fn failed_later_removal_keeps_metadata_for_surviving_computers() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut second = computer();
        {
            let ComputerConfiguration { id, name, .. } = &mut second;
            *id = "00000000-0000-4000-8000-000000000002".into();
            *name = "work".into();
        }
        let remote = ComputerConfiguration {
            id: "00000000-0000-4000-8000-000000000003".into(),
            name: "keep".into(),
            ..computer()
        };
        write_metadata(
            &paths.metadata,
            &request(vec![computer(), second.clone(), remote.clone()]),
        )
        .unwrap();
        let stopped = |configuration: &ComputerConfiguration| {
            let mut inspected = inspect(&paths, "Stopped");
            inspected["name"] = json!(configuration.name());
            inspected["config"]["labels"]["silo.machine-id"] = json!(configuration.id());
            Ok(CommandOutput {
                stdout: inspected.to_string(),
                stderr: String::new(),
            })
        };
        let runner = StubRunner::new(vec![
            stopped(&computer()),
            stopped(&second),
            stopped(&computer()),
            Ok(CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
            }),
            stopped(&second),
            Err(RuntimeError::Unavailable("remove failed".into())),
        ]);
        let error = apply_whole_configuration(
            &runner,
            &paths,
            &generous_device(),
            request(vec![remote.clone()]),
        )
        .unwrap_err();
        assert!(error.to_string().contains("Completed changes were kept"));
        assert_eq!(
            read_metadata(&paths.metadata).unwrap().computers,
            vec![second, remote]
        );
    }

    #[test]
    fn failed_multi_create_keeps_metadata_for_completed_creation() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let first = computer();
        let mut second = computer();
        {
            let ComputerConfiguration { id, name, .. } = &mut second;
            *id = "00000000-0000-4000-8000-000000000002".into();
            *name = "work".into();
        }
        let runner = StubRunner::new(vec![
            Ok(CommandOutput {
                stdout: "[]".into(),
                stderr: String::new(),
            }),
            Ok(CommandOutput {
                stdout: "1".into(),
                stderr: String::new(),
            }),
            Ok(CommandOutput {
                stdout: "1".into(),
                stderr: String::new(),
            }),
            Ok(CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
            }),
            Ok(CommandOutput {
                stdout: inspect(&paths, "Created").to_string(),
                stderr: String::new(),
            }),
            Ok(CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
            }),
            Ok(CommandOutput {
                stdout: inspect(&paths, "Stopped").to_string(),
                stderr: String::new(),
            }),
            Ok(CommandOutput {
                stdout: inspect(&paths, "Created").to_string(),
                stderr: String::new(),
            }),
            Ok(CommandOutput {
                stdout: "[]".into(),
                stderr: String::new(),
            }),
            Ok(CommandOutput {
                stdout: "1".into(),
                stderr: String::new(),
            }),
            Ok(CommandOutput {
                stdout: "1".into(),
                stderr: String::new(),
            }),
            Err(RuntimeError::Failed {
                operation: "Creating the computer".into(),
                exit_code: Some(1),
                detail: "image pull failed".into(),
            }),
            Ok(CommandOutput {
                stdout: "[]".into(),
                stderr: String::new(),
            }),
            Ok(CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
            }),
        ]);

        let error = apply_whole_configuration(
            &runner,
            &paths,
            &generous_device(),
            request(vec![first, second]),
        )
        .unwrap_err();

        assert!(matches!(error, RuntimeError::Partial(_)));
        assert!(!error.to_string().contains("image pull failed"));
        assert!(failure_report(&error)
            .diagnostic
            .is_some_and(|text| text.contains("image pull failed")));
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args[0] == "remove"));
        assert_eq!(
            read_metadata(&paths.metadata).unwrap().computers,
            vec![computer()]
        );
        assert!(error.to_string().contains("Completed changes were kept"));
    }

    const ATTEMPT_ID: &str = "00000000-0000-4000-8000-0000000000aa";

    fn pending_restore_record(paths: &RuntimePaths, attempted: bool) {
        let directory = paths.metadata.with_file_name("checkpoints");
        fs::create_dir_all(&directory).unwrap();
        let mut record = json!({
            "version": 1,
            "checkpoints": [],
            "snapshotGroup": "dev",
            "pendingCheckpointRestore": {"checkpointId": "c000000000000000000000000000000", "sourceComputer": "dev", "state": "full"},
            "checkpointOperation": null,
        });
        if attempted {
            record["restoreAttempted"] = json!(true);
            record["restoreAttemptId"] = json!(ATTEMPT_ID);
        }
        fs::write(
            directory.join(format!("{}.json", computer().id())),
            record.to_string(),
        )
        .unwrap();
    }

    fn restore_attempt(paths: &RuntimePaths, status: &str) -> Value {
        let mut attempt = inspect(paths, status);
        attempt["config"]["labels"]["silo.restore-attempt"] = json!(ATTEMPT_ID);
        attempt
    }

    fn missing_computer() -> Result<CommandOutput, RuntimeError> {
        Err(RuntimeError::Failed {
            operation: "inspect dev".into(),
            exit_code: Some(1),
            detail: "computer not found".into(),
        })
    }

    #[test]
    fn deleting_a_pending_restore_removes_the_runtime_its_attempt_created() {
        let _test_state = crate::test_support::global_state();
        for (status, stops) in [("Stopped", false), ("Running", true)] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
            pending_restore_record(&paths, true);
            let attempt = restore_attempt(&paths, status);
            use crate::test_support::runner::{ExpectedCommand, ScriptedRunner};
            let mut commands = vec![
                ExpectedCommand::ok(["inspect", "dev", "--format", "json"], attempt.to_string()),
                ExpectedCommand::ok(["inspect", "dev", "--format", "json"], attempt.to_string()),
            ];
            if stops {
                commands.push(ExpectedCommand::ok(["stop", "dev", "--quiet"], ""));
            }
            let inventory = json!([{"snapshot_id": "restore-snapshot", "group": "dev", "name": "c000000000000000000000000000000"}]).to_string();
            commands.extend([
                ExpectedCommand::ok(["remove", "--quiet", "dev"], ""),
                ExpectedCommand::ok(["snapshot", "list", "--format", "json"], inventory.clone()),
                ExpectedCommand::ok(["snapshot", "list", "--format", "json"], inventory),
                ExpectedCommand::ok(["list", "--format", "json"], "[]"),
                ExpectedCommand::ok(
                    ["snapshot", "head", "dev", "--format", "json"],
                    json!({"head": "restore-snapshot"}).to_string(),
                ),
                ExpectedCommand::ok(
                    [
                        "snapshot",
                        "remove",
                        "dev:c000000000000000000000000000000",
                        "--quiet",
                    ],
                    "",
                ),
            ]);
            let runner = ScriptedRunner::new(commands);
            apply_whole_configuration(&runner, &paths, &generous_device(), request(vec![]))
                .unwrap();
            runner.assert_finished();
            // The runtime computer is removed before Silo forgets the computer and its record.
            assert!(read_metadata(&paths.metadata).unwrap().computers.is_empty());
            assert!(!paths
                .metadata
                .with_file_name("checkpoints")
                .join(format!("{}.json", computer().id()))
                .exists());
        }
    }

    #[test]
    fn deleting_a_pending_restore_without_runtime_state_removes_only_silo_records() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        pending_restore_record(&paths, false);
        use crate::test_support::runner::{ExpectedCommand, ScriptedRunner};
        let runner = ScriptedRunner::new([
            ExpectedCommand::error(
                ["inspect", "dev", "--format", "json"],
                missing_computer().unwrap_err(),
            ),
            ExpectedCommand::error(
                ["inspect", "dev", "--format", "json"],
                missing_computer().unwrap_err(),
            ),
            ExpectedCommand::ok(["snapshot", "list", "--format", "json"], "[]"),
        ]);
        apply_whole_configuration(&runner, &paths, &generous_device(), request(vec![])).unwrap();
        runner.assert_finished();
        assert!(read_metadata(&paths.metadata).unwrap().computers.is_empty());
    }

    #[test]
    fn deleting_a_pending_restore_preserves_runtime_state_it_did_not_create() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(&paths.metadata, &request(vec![computer()])).unwrap();
        pending_restore_record(&paths, true);
        let runner = StubRunner::successful_json(vec![inspect(&paths, "Stopped")]);
        let error = apply_whole_configuration(&runner, &paths, &generous_device(), request(vec![]))
            .unwrap_err();
        assert!(error.to_string().contains("preserved"), "{error}");
        assert!(runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| args[0] == "inspect"));
        assert_eq!(
            read_metadata(&paths.metadata).unwrap().computers,
            vec![computer()]
        );
    }

    #[test]
    fn remove_refuses_a_running_computer_without_stopping_it_implicitly() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let runner = StubRunner::successful_json(vec![inspect(&paths, "Running")]);

        let error = remove_computer(&runner, &paths, &computer()).unwrap_err();
        assert!(error.to_string().contains("Stop computer 'dev'"));
        assert_eq!(runner.calls.lock().unwrap().len(), 1);
    }
}

#[cfg(test)]
#[path = "runtime_github_tests.rs"]
mod github_integration_tests;

/// Apply secret policy under the same per-computer lock as GitHub updates and boot.
pub(crate) fn apply_secrets(app: &AppHandle, computer: &str) -> Result<Vec<String>, String> {
    validate_name(computer).map_err(|error| error.to_string())?;
    apply_secrets_at_paths(runtime_paths(app)?, computer)
}

fn apply_secrets_at_paths(paths: RuntimePaths, computer: &str) -> Result<Vec<String>, String> {
    let computer_id = resolve_computer_id(&paths, computer).map_err(|error| error.to_string())?;
    let base_label = format!("Saving secrets for {computer}");
    let acquire =
        |label: &str| -> Result<operation_gate::OperationGuard<'static>, secrets_runtime::Attempt> {
            OPERATIONS
                .computer(&computer_id, computer, label)
                // Gate rejections (busy, already queued, cancelled) are never transient.
                .map_err(|error| secrets_runtime::Attempt::Final(error.to_string()))
        };
    // Applying secrets to a running guest is cancellable through the current-operation token.
    let prepare = |guard: &operation_gate::OperationGuard<'static>| {
        guard.allow_cancel();
        guard.expect_within(Duration::from_secs(600));
    };
    let work = || -> Result<Vec<String>, secrets_runtime::Attempt> {
        shutdown::ensure_accepting_operations().map_err(secrets_runtime::Attempt::Final)?;
        let access = computer_access_state(&paths.home, computer)
            .map_err(secrets_runtime::Attempt::Final)?;
        let _guard = lock_computer_runtime(&access, MUTATION_TIMEOUT, "Saving secrets").map_err(
            |error| {
                match error {
                    RuntimeError::Cancelled { .. } => {
                        secrets_runtime::Attempt::Cancelled("Saving secrets was cancelled.".into())
                    }
                    // Another change to this computer is still finishing; a later attempt may proceed.
                    _ => secrets_runtime::Attempt::Transient(
                        "Another change to this computer is still running. Retry after it finishes."
                            .into(),
                    ),
                }
            },
        )?;
        let configuration = read_metadata(&paths.metadata)
            .map_err(|error| secrets_runtime::Attempt::Final(error.to_string()))?
            .computers
            .into_iter()
            .find(|configuration| {
                configuration.id() == computer_id && configuration.name() == computer
            })
            .ok_or_else(|| {
                secrets_runtime::Attempt::Final(
                    "The computer identity changed. No secrets were applied.".into(),
                )
            })?;
        let inspected = inspect_computer(&ProcessRunner, &paths, computer).map_err(|_| {
            secrets_runtime::Attempt::Final("Could not inspect computer secrets.".into())
        })?;
        ensure_computer_identity(&configuration, &inspected)
            .map_err(|error| secrets_runtime::Attempt::Final(error.to_string()))?;
        // An edit/remove may have committed while this operation waited for the
        // Computer gate. Never send the caller's stale values back into the guest.
        let revision = crate::secrets::computer_revision(computer)?;
        let desired = crate::secrets::runtime_material(computer)?;
        let records = crate::secrets::pending_revocations()?;
        let pending = secrets_runtime::apply(&paths, computer, &desired, false)?;
        if crate::secrets::computer_revision(computer)? == revision {
            crate::secrets::revocations_replaced(&records, computer, &pending)?;
        }
        Ok(pending)
    };
    // Re-applying the same desired secrets is idempotent, so a timed-out runtime command
    // is retried; validation, rejection, and verification failures are final. The typed
    // `Attempt` is converted to a plain message only here, at the command edge.
    gated_auto_retry_classified(
        &AUTO_RETRY_DELAYS,
        &base_label,
        acquire,
        prepare,
        work,
        secrets_runtime::Attempt::is_transient,
        || secrets_runtime::Attempt::Cancelled("Saving secrets was cancelled.".into()),
    )
    .map_err(String::from)
}

/// Background name-only revocation. Use the owner device's existing computer gate;
/// skip busy/transitional/unreadable guests and retry on a later state refresh.
pub(crate) fn revoke_secret(
    app: &AppHandle,
    record: &crate::secrets::PendingRevocation,
) -> Result<bool, String> {
    let paths = runtime_paths(app)?;
    revoke_secret_with(&ProcessRunner, &paths, record, &mut |name| {
        secrets_runtime::remove_name(&paths, &record.computer, name)
    })
}

fn revoke_secret_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    record: &crate::secrets::PendingRevocation,
    remove: &mut dyn FnMut(&str) -> Result<(), String>,
) -> Result<bool, String> {
    let metadata = read_metadata(&paths.metadata).map_err(|error| error.to_string())?;
    let Some(configuration) = metadata
        .computers
        .iter()
        .find(|configuration| configuration.name() == record.computer)
    else {
        return Ok(true);
    };
    let Ok(_turn) = OPERATIONS.try_computer_hidden(
        configuration.id(),
        &record.computer,
        "Revoking removed secret",
    ) else {
        return Ok(false);
    };
    let access = computer_access_state(&paths.home, &record.computer)?;
    let Ok(_runtime) = access.runtime.try_lock() else {
        return Ok(false);
    };
    // Read after acquiring the computer gate: a queued old retry cannot remove a
    // replacement that has since been applied under that same gate.
    let observed =
        observe_computer(runner, paths, &record.computer).map_err(|error| error.to_string())?;
    match observed {
        ComputerRuntime::Absent => Ok(true),
        ComputerRuntime::Present(inspected) => {
            ensure_managed(&inspected).map_err(|error| error.to_string())?;
            if inspected.name != record.computer
                || inspected
                    .config
                    .pointer("/labels/silo.machine-id")
                    .and_then(Value::as_str)
                    != Some(configuration.id())
            {
                return Err(
                    "The computer identity changed. Its secret revocation remains pending.".into(),
                );
            }
            if matches!(inspected.status.as_str(), "Stopped" | "Created" | "Crashed") {
                return Ok(true);
            }
            if !crate::secrets::revocation_needed(record)? {
                return Ok(false);
            }
            secrets_runtime::revoke_observed_with(
                runner,
                paths,
                &record.computer,
                &record.name,
                &inspected,
                remove,
            )
        }
    }
}

#[cfg(test)]
pub(crate) fn validate_secret_material_for_tests(
    material: &[(String, String, Vec<String>)],
) -> Result<(), String> {
    secrets_runtime::validate_material(&material.to_vec())
}

pub(crate) fn validate_secret_computers(
    app: &AppHandle,
    computers: &[String],
) -> Result<(), String> {
    let paths = runtime_paths(app)?;
    let metadata = read_metadata(&paths.metadata)
        .map_err(|_| "Computer settings could not be read.".to_string())?;
    for computer in computers {
        validate_name(computer).map_err(|_| "Invalid computer selection.".to_string())?;
        if !metadata
            .computers
            .iter()
            .any(|configuration| configuration.name() == computer)
        {
            return Err("Secrets can only be assigned to local Silo computers.".into());
        }
        let inspected = inspect_computer(&ProcessRunner, &paths, computer)
            .map_err(|_| "Could not verify the selected computer.".to_string())?;
        ensure_managed(&inspected)
            .map_err(|_| "Secrets can only be assigned to managed Silo computers.".to_string())?;
    }
    Ok(())
}

/// Caller holds the operation gate for this computer and has verified the stable computer identity.
pub(crate) fn start_for_desktop(paths: &RuntimePaths, computer: &str) -> Result<(), RuntimeError> {
    debug_assert!(
        operation_gate::held(),
        "a desktop start requires the computer's operation gate"
    );
    computer_action_with(
        &ProcessRunner,
        paths,
        &device_resources()?,
        "start",
        computer,
    )
}

#[cfg(test)]
mod change_configuration_tests {
    use super::*;

    fn computer(id: &str, name: &str, cpus: u8) -> ComputerConfiguration {
        ComputerConfiguration {
            id: id.into(),
            name: name.into(),
            cpus,
            max_cpus: 4,
            memory_gib: 2,
            max_memory_gib: 4,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            desktop: None,
        }
    }

    fn upsert(
        configuration: &ComputerConfiguration,
        expected: Option<&ComputerConfiguration>,
    ) -> ComputerConfigurationChange {
        ComputerConfigurationChange::Upsert {
            configuration: configuration.clone(),
            expected: expected.cloned(),
        }
    }

    #[test]
    fn upsert_rejected_when_expected_does_not_match_current() {
        let _test_state = crate::test_support::global_state();
        let current = computer("a", "dev", 2);
        let mut computers = vec![current.clone(), computer("b", "web", 2)];
        // The user edited from a two-CPU baseline, but the computer now has four CPUs.
        let stale_expected = computer("a", "dev", 4);
        let edited = computer("a", "dev", 3);
        let error = upsert(&edited, Some(&stale_expected))
            .apply(&mut computers)
            .unwrap_err();
        assert!(
            error.contains("changed while your edit was waiting"),
            "{error}"
        );
        // Nothing is mutated on rejection.
        assert_eq!(computers, vec![current, computer("b", "web", 2)]);
    }

    #[test]
    fn delete_rejected_when_computer_no_longer_exists() {
        let _test_state = crate::test_support::global_state();
        let mut computers = vec![computer("b", "web", 2)];
        let change = ComputerConfigurationChange::Delete {
            computer_id: "a".into(),
            expected: computer("a", "dev", 2),
        };
        // Current config for "a" is absent, so it never equals the expected snapshot.
        let error = change.apply(&mut computers).unwrap_err();
        assert!(
            error.contains("changed while your edit was waiting"),
            "{error}"
        );
        assert_eq!(computers, vec![computer("b", "web", 2)]);
    }

    #[test]
    fn delete_of_present_computer_removes_only_that_computer() {
        let _test_state = crate::test_support::global_state();
        let mut computers = vec![computer("a", "dev", 2), computer("b", "web", 2)];
        let change = ComputerConfigurationChange::Delete {
            computer_id: "a".into(),
            expected: computer("a", "dev", 2),
        };
        change.apply(&mut computers).unwrap();
        assert_eq!(computers, vec![computer("b", "web", 2)]);
        // A second identical delete now fails: the computer is gone.
        assert!(change.apply(&mut computers).is_err());
    }

    #[test]
    fn two_sequential_changes_both_land_second_on_top_of_first() {
        let _test_state = crate::test_support::global_state();
        // Each targeted change reads fresh state and applies on top of the previous
        // one, so both survive instead of the second overwriting the first.
        let mut computers = vec![computer("a", "dev", 2), computer("b", "web", 2)];

        // First edit: change "a" to three CPUs, expecting the two-CPU baseline.
        let first = upsert(&computer("a", "dev", 3), Some(&computer("a", "dev", 2)));
        first.apply(&mut computers).unwrap();

        // Second edit targets "b" and expects the current "b" (unchanged by the first).
        let second = upsert(&computer("b", "web", 3), Some(&computer("b", "web", 2)));
        second.apply(&mut computers).unwrap();

        // Both edits are preserved, and order is stable (edit in place, not append).
        assert_eq!(
            computers,
            vec![computer("a", "dev", 3), computer("b", "web", 3)]
        );
    }

    #[test]
    fn edit_preserves_position_instead_of_appending() {
        let _test_state = crate::test_support::global_state();
        let mut computers = vec![
            computer("a", "dev", 2),
            computer("b", "web", 2),
            computer("c", "db", 2),
        ];
        upsert(&computer("a", "dev", 3), Some(&computer("a", "dev", 2)))
            .apply(&mut computers)
            .unwrap();
        assert_eq!(
            computers,
            vec![
                computer("a", "dev", 3),
                computer("b", "web", 2),
                computer("c", "db", 2)
            ]
        );
    }

    #[test]
    fn reorder_requires_matching_expected_order() {
        let _test_state = crate::test_support::global_state();
        let mut computers = vec![
            computer("a", "dev", 2),
            computer("b", "web", 2),
            computer("c", "db", 2),
        ];
        let change = ComputerConfigurationChange::Reorder {
            order: vec!["c".into(), "a".into(), "b".into()],
            expected_order: vec!["a".into(), "b".into(), "c".into()],
        };
        change.apply(&mut computers).unwrap();
        assert_eq!(
            computers
                .iter()
                .map(ComputerConfiguration::id)
                .collect::<Vec<_>>(),
            vec!["c", "a", "b"]
        );

        // A stale expected order (the inventory changed meanwhile) is rejected.
        let stale = ComputerConfigurationChange::Reorder {
            order: vec!["a".into(), "b".into(), "c".into()],
            expected_order: vec!["a".into(), "b".into(), "c".into()],
        };
        let error = stale.apply(&mut computers).unwrap_err();
        assert!(
            error.contains("changed while your edit was waiting"),
            "{error}"
        );
    }

    #[test]
    fn create_with_existing_id_is_rejected() {
        let _test_state = crate::test_support::global_state();
        // A create carries `expected: null` meaning "must not already exist". An id that
        // is already present makes the current value differ from the expected absence.
        let mut computers = vec![computer("a", "dev", 2)];
        let error = upsert(&computer("a", "dev", 3), None)
            .apply(&mut computers)
            .unwrap_err();
        assert!(
            error.contains("changed while your edit was waiting"),
            "{error}"
        );
        assert_eq!(computers, vec![computer("a", "dev", 2)]);
    }

    #[test]
    fn batch_applies_every_change_in_order() {
        let _test_state = crate::test_support::global_state();
        let mut computers = vec![computer("a", "dev", 2)];
        let batch = ComputerConfigurationChange::Batch {
            changes: vec![
                upsert(&computer("a", "dev", 3), Some(&computer("a", "dev", 2))),
                upsert(&computer("b", "web", 2), None),
                upsert(&computer("c", "db", 2), None),
            ],
        };
        batch.apply(&mut computers).unwrap();
        assert_eq!(
            computers,
            vec![
                computer("a", "dev", 3),
                computer("b", "web", 2),
                computer("c", "db", 2)
            ]
        );
    }

    #[test]
    fn batch_rejects_all_or_nothing_when_one_change_is_stale() {
        let _test_state = crate::test_support::global_state();
        let mut computers = vec![computer("a", "dev", 2)];
        // The second change expects a two-CPU baseline for "a" that the first already
        // moved to three, so it is stale and the whole batch is rejected untouched.
        let batch = ComputerConfigurationChange::Batch {
            changes: vec![
                upsert(&computer("b", "web", 2), None),
                upsert(&computer("a", "dev", 4), Some(&computer("a", "dev", 3))),
            ],
        };
        let error = batch.apply(&mut computers).unwrap_err();
        assert!(
            error.contains("changed while your edit was waiting"),
            "{error}"
        );
        // No change survives: neither the new "b" nor the edit to "a".
        assert_eq!(computers, vec![computer("a", "dev", 2)]);
    }

    #[test]
    fn nested_batches_are_rejected() {
        let _test_state = crate::test_support::global_state();
        let mut computers = vec![computer("a", "dev", 2)];
        let batch = ComputerConfigurationChange::Batch {
            changes: vec![ComputerConfigurationChange::Batch { changes: vec![] }],
        };
        let error = batch.apply(&mut computers).unwrap_err();
        assert!(error.contains("Nested"), "{error}");
        assert_eq!(computers, vec![computer("a", "dev", 2)]);
    }
}
