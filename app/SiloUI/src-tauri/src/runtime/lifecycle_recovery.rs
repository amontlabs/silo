//! Durable desired state for explicit local computer actions. Detached runtime children
//! own MicroSandbox's transition/lifecycle guards, not Silo's command-worker lock.
use super::*;
use silo_engine::context::{Audience, Event};

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Phase {
    StopPending,
    StartPending,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u8,
    computer_id: String,
    name: String,
    action: String,
    phase: Phase,
    event: runtime_activity::Event,
}
fn error(message: impl Into<String>) -> RuntimeError {
    RuntimeError::Unavailable(message.into())
}
fn directory(paths: &RuntimePaths) -> PathBuf {
    paths.metadata.with_file_name("lifecycle-operations")
}
fn path(paths: &RuntimePaths, id: &str) -> PathBuf {
    directory(paths).join(format!("{:x}.json", Sha256::digest(id.as_bytes())))
}
fn store(paths: &RuntimePaths, intent: &Intent) -> Result<(), RuntimeError> {
    let directory = directory(paths);
    fs::create_dir_all(&directory)
        .map_err(|_| error("Computer action progress could not be saved."))?;
    let mut file = tempfile::NamedTempFile::new_in(&directory)
        .map_err(|_| error("Computer action progress could not be saved."))?;
    serde_json::to_writer(&mut file, intent)
        .map_err(|_| error("Computer action progress could not be encoded."))?;
    file.as_file()
        .sync_all()
        .map_err(|_| error("Computer action progress could not be synced."))?;
    file.persist(path(paths, &intent.computer_id))
        .map_err(|_| error("Computer action progress could not be saved."))?;
    File::open(&directory)
        .and_then(|f| f.sync_all())
        .map_err(|_| error("Computer action progress could not be synced."))?;
    File::open(directory.parent().unwrap())
        .and_then(|f| f.sync_all())
        .map_err(|_| error("Computer action progress could not be synced."))
}
fn load(file: &Path) -> Result<Option<Intent>, RuntimeError> {
    let file = match File::open(file) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(error("Saved computer action could not be read.")),
    };
    let mut bytes = Vec::new();
    file.take(MAX_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error("Saved computer action could not be read."))?;
    if bytes.len() as u64 > MAX_OUTPUT_BYTES {
        return Err(error("Saved computer action is too large."));
    }
    let intent: Intent = serde_json::from_slice(&bytes)
        .map_err(|_| error("Saved computer action is invalid; it was preserved."))?;
    if intent.version != 1
        || uuid::Uuid::parse_str(&intent.computer_id).is_err()
        || validate_name(&intent.name).is_err()
        || !matches!(intent.action.as_str(), "start" | "stop" | "restart")
        || (intent.action == "start" && intent.phase != Phase::StartPending)
        || (intent.action == "stop" && intent.phase != Phase::StopPending)
        || !runtime_activity::matches(&intent.event, &intent.action, &intent.name)
    {
        return Err(error("Saved computer action is invalid; it was preserved."));
    }
    Ok(Some(intent))
}
fn computer_configuration(
    paths: &RuntimePaths,
    name: &str,
) -> Result<ComputerConfiguration, RuntimeError> {
    validate_name(name)?;
    let metadata = read_metadata(&paths.metadata)?;
    metadata
        .computers
        .into_iter()
        .find(|m| m.name() == name)
        .ok_or_else(|| {
            error(format!(
                "Computer '{name}' is not a configured local computer. No action was performed."
            ))
        })
}
fn inspect(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    intent: &Intent,
) -> Result<InspectedSandbox, RuntimeError> {
    if computer_configuration(paths, &intent.name)?.id() != intent.computer_id {
        return Err(error("The computer was replaced. Its data was preserved."));
    }
    let value = inspect_computer(runner, paths, &intent.name)?;
    ensure_managed(&value)?;
    if value.name != intent.name
        || value
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            != Some(&intent.computer_id)
    {
        return Err(error(
            "The computer identity changed. No action was performed on the replacement.",
        ));
    }
    Ok(value)
}
fn stable(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    intent: &Intent,
    mut value: InspectedSandbox,
) -> Result<InspectedSandbox, RuntimeError> {
    let deadline = Instant::now() + MUTATION_TIMEOUT;
    while matches!(
        value.status.to_ascii_lowercase().as_str(),
        "starting" | "stopping" | "draining"
    ) {
        if Instant::now() >= deadline {
            return Err(error("The previous computer action is still finishing. Its saved progress was preserved; retry shortly."));
        }
        thread::sleep(Duration::from_millis(100));
        value = inspect(runner, paths, intent)?;
    }
    Ok(value)
}
fn stopped(value: &InspectedSandbox) -> bool {
    matches!(
        value.status.to_ascii_lowercase().as_str(),
        "stopped" | "created" | "crashed"
    )
}
fn advance(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    device: &DeviceResources,
    intent: &mut Intent,
    initial: InspectedSandbox,
) -> Result<(), RuntimeError> {
    let mut observed = stable(runner, paths, intent, initial)?;
    if observed.status == "Paused"
        && checkpoints::recover_paused_capture(runner, paths, &intent.computer_id, &intent.name)?
    {
        observed = stable(runner, paths, intent, inspect(runner, paths, intent)?)?;
    }
    if stopped(&observed) {
        let _ = crate::secrets::computer_stopped(&intent.name);
    }
    loop {
        // Honour a cancel between the discrete stop/start steps of a restart.
        if crate::runtime::operation_gate::check_cancelled().is_err() {
            return Err(RuntimeError::Cancelled {
                operation: format!("{} {}", intent.action, intent.name),
            });
        }
        let reached = match intent.phase {
            Phase::StopPending => stopped(&observed),
            Phase::StartPending => observed.status.eq_ignore_ascii_case("running"),
        };
        if reached {
            if intent.action == "restart" && intent.phase == Phase::StopPending {
                intent.phase = Phase::StartPending;
                store(paths, intent)?;
                continue;
            }
            return Ok(());
        }
        let command = match intent.phase {
            Phase::StopPending if observed.status.eq_ignore_ascii_case("running") => "stop",
            Phase::StartPending if stopped(&observed) => {
                validate_inspected_resources(&intent.name, &observed.config, device)?;
                "start"
            }
            _ => {
                return Err(error(format!(
                    "{} is not ready for this action. Check its status and retry.",
                    intent.name
                )))
            }
        };
        // A surviving detached start can win the runtime's own transition guard.
        // Verify the desired state even when its duplicate command reports failure.
        if command == "stop" {
            storage::before_stop(runner, paths, &observed);
        }
        let args = [command.into(), intent.name.clone(), "--quiet".into()];
        // Stop is not cancellable: a cancel during a restart's stop step is
        // honoured before the start step instead of killing `msb stop`.
        let result = if command == "stop" {
            crate::runtime::operation_gate::uncancellable(|| runner.run(paths, &args, STOP_TIMEOUT))
        } else {
            runner.run(paths, &args, MUTATION_TIMEOUT)
        };
        let observe = || stable(runner, paths, intent, inspect(runner, paths, intent)?);
        observed = if command == "stop" {
            crate::runtime::operation_gate::uncancellable(observe)
        } else {
            observe()
        }?;
        let reached = if command == "stop" {
            stopped(&observed)
        } else {
            observed.status.eq_ignore_ascii_case("running")
        };
        let started_here = command == "start" && result.is_ok();
        if reached && command == "stop" {
            let _ = crate::secrets::computer_stopped(&intent.name);
        }
        if !reached {
            // The timed-out stop was already followed by the full state wait;
            // report it as final rather than a transient error that is retried
            // with another full stop timeout and wait.
            if command == "stop" && matches!(result, Err(RuntimeError::TimedOut { .. })) {
                return Err(error(format!(
                    "{} did not stop in time. Check its status and retry.",
                    intent.name
                )));
            }
            result?;
            return Err(error(format!(
                "{} did not reach the {} state. Retry to continue the saved action.",
                intent.name,
                if command == "stop" {
                    "Stopped"
                } else {
                    "Running"
                }
            )));
        }
        if started_here {
            storage::after_start(runner, paths, &observed);
        }
    }
}
fn settle(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    device: &DeviceResources,
    intent: &mut Intent,
    initial: InspectedSandbox,
) -> Result<(), RuntimeError> {
    runtime_activity::resume(paths, &mut intent.event, &intent.computer_id);
    let result = advance(runner, paths, device, intent, initial);
    runtime_activity::finish(paths, &mut intent.event, &result);
    // A user cancel retires the intent: the next launch must not resume an
    // action the user explicitly abandoned.
    let cancelled = matches!(result, Err(RuntimeError::Cancelled { .. }));
    if result.is_ok() || cancelled {
        fs::remove_file(path(paths, &intent.computer_id)).map_err(|_| error(if cancelled {
            "The computer action was cancelled, but its saved progress could not be cleared."
        } else {
            "The computer action completed, but its saved progress could not be cleared. Retry to verify it."
        }))?;
        File::open(directory(paths))
            .and_then(|f| f.sync_all())
            .map_err(|_| error("Completed computer action progress could not be synced."))?;
    }
    result
}
/// True when an unfinished action is saved for this computer.
pub(super) fn has_intent(paths: &RuntimePaths, computer_id: &str) -> bool {
    path(paths, computer_id).exists()
}
pub(super) fn perform(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    device: &DeviceResources,
    action: &str,
    name: &str,
) -> Result<(), RuntimeError> {
    if !matches!(action, "start" | "stop" | "restart") {
        return Err(RuntimeError::Invalid(format!(
            "Unknown computer action '{action}'. No computer operation was performed."
        )));
    }
    let configuration = computer_configuration(paths, name)?;
    // An explicit action replaces a saved one it cannot continue: another action,
    // one saved for a different identity or name, or an unreadable file (for example
    // from a newer Silo). Only an intact intent for this exact action is resumed.
    let (existing, superseded) = match load(&path(paths, configuration.id())) {
        Ok(Some(saved))
            if saved.computer_id == configuration.id()
                && saved.name == name
                && saved.action == action =>
        {
            (Some(saved), None)
        }
        Ok(Some(saved)) if saved.computer_id == configuration.id() => (None, Some(saved)),
        Ok(_) | Err(_) => (None, None),
    };
    let mut intent = if let Some(saved) = existing {
        saved
    } else {
        Intent {
            version: 1,
            computer_id: configuration.id().into(),
            name: name.into(),
            action: action.into(),
            phase: if action == "start" {
                Phase::StartPending
            } else {
                Phase::StopPending
            },
            event: runtime_activity::begin(paths, action, name, configuration.id())
                .map_err(error)?,
        }
    };
    let initial = match inspect(runner, paths, &intent).and_then(|initial| {
        if matches!(action, "start" | "restart") {
            validate_inspected_resources(name, &initial.config, device)?;
        }
        store(paths, &intent)?;
        Ok(initial)
    }) {
        Ok(initial) => initial,
        Err(failure) => {
            let result = Err(failure);
            runtime_activity::finish(paths, &mut intent.event, &result);
            return result;
        }
    };
    // Settle the superseded action only once the new intent replaced its file;
    // a new action rejected above leaves the saved one pending and unchanged.
    if let Some(mut previous) = superseded {
        let result = Err(RuntimeError::Invalid(format!(
            "Replaced by the requested {action} action."
        )));
        runtime_activity::finish(paths, &mut previous.event, &result);
    }
    settle(runner, paths, device, &mut intent, initial)
}
// The caller inspected this exact computer and confirmed Crashed while holding the
// operation gate. Dismissal must not leave a failed start queued for recovery.
pub(super) fn dismiss_crashed_intent(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    let target = path(paths, configuration.id());
    if let Some(intent) = load(&target)? {
        if intent.computer_id != configuration.id() || intent.name != configuration.name() {
            return Err(error(
                "Saved computer action has a different identity; it was preserved.",
            ));
        }
        fs::remove_file(target)
            .map_err(|_| error("The failed computer action could not be dismissed."))?;
        File::open(directory(paths))
            .and_then(|f| f.sync_all())
            .map_err(|_| error("The dismissed action could not be synced."))?;
    }
    Ok(())
}

// Called only after configuration deletion verified this exact ID and removed
// it from saved metadata, including its crash-recovery path.
pub(super) fn forget_removed(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    storage::forget_removed(paths, configuration.id());
    let target = path(paths, configuration.id());
    if let Some(mut intent) = load(&target)? {
        if intent.computer_id != configuration.id() || intent.name != configuration.name() {
            return Err(error(
                "Saved computer action has a different identity; it was preserved.",
            ));
        }
        runtime_activity::finish(
            paths,
            &mut intent.event,
            &Err(RuntimeError::Invalid(
                "Computer was removed before this action finished.".into(),
            )),
        );
        fs::remove_file(target)
            .map_err(|_| error("Removed computer action progress could not be cleared."))?;
        File::open(directory(paths))
            .and_then(|f| f.sync_all())
            .map_err(|_| error("Removed computer action progress could not be synced."))?;
    }
    Ok(())
}
/// Retire every saved action except those for the computers in `resuming`. Update
/// preparation promises to restore exactly the pre-update running set, so a failed
/// start kept for Retry must not start its computer at the next launch (D-22). An
/// unfinished activity entry is settled as cancelled; a finished one keeps its
/// outcome. Unreadable files are left for startup recovery to report.
pub(crate) fn retire_except(
    paths: &RuntimePaths,
    resuming: &HashSet<String>,
) -> Result<(), RuntimeError> {
    let entries = match fs::read_dir(directory(paths)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(error("Saved computer actions could not be read.")),
    };
    let mut retired = false;
    for entry in entries {
        let file = entry
            .map_err(|_| error("Saved computer actions could not be read."))?
            .path();
        if file.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Ok(Some(mut intent)) = load(&file) else {
            continue;
        };
        if file != path(paths, &intent.computer_id) || resuming.contains(&intent.computer_id) {
            continue;
        }
        runtime_activity::retire(paths, &mut intent.event);
        fs::remove_file(&file)
            .map_err(|_| error("A saved computer action could not be retired."))?;
        retired = true;
    }
    if retired {
        File::open(directory(paths))
            .and_then(|f| f.sync_all())
            .map_err(|_| error("Retired computer actions could not be synced."))?;
    }
    Ok(())
}

/// The outcome of launch recovery. Per-action failures never hide the rest (D-23).
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Recovered {
    /// Computers that launch auto-start must leave stopped: explicit stops that were
    /// resumed or could not be settled, and computers whose saved action is unreadable.
    pub(crate) keep_stopped: HashSet<String>,
    /// One line per saved action that could not be resumed; each was preserved.
    pub(crate) failures: Vec<String>,
}

pub(crate) fn recover(host: &dyn RuntimeHost) -> Result<Recovered, String> {
    let paths = runtime_paths(host)?;
    let _guard = OPERATIONS
        .device("Resuming computer actions")
        .map_err(|_| "Computer action recovery is busy.")?;
    let resources = device_resources().map_err(|e| e.to_string())?;
    let result = recover_with(&ProcessRunner, &paths, &resources);
    host.engine_context().publish(
        Event::new("silo://application-state-changed", Value::Null),
        Audience::All,
    );
    result.map_err(|e| e.to_string())
}
/// Settle every saved action. Only a failure to list them is an error; every other
/// failure is reported per action and recovery continues with the rest.
fn recover_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    device: &DeviceResources,
) -> Result<Recovered, RuntimeError> {
    let entries = match fs::read_dir(directory(paths)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Recovered::default()),
        Err(_) => return Err(error("Saved computer actions could not be read.")),
    };
    let mut recovered = Recovered::default();
    let mut unreadable = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            recovered
                .failures
                .push("A saved computer action could not be read.".into());
            continue;
        };
        if entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        // One unreadable or mismatched intent must not block the others;
        // it is preserved and reported with the other failures.
        let mut intent = match load(&entry.path()) {
            Ok(Some(intent)) => intent,
            Ok(None) => continue,
            Err(failure) => {
                recovered.failures.push(safe_activity_error(&failure));
                unreadable.push(entry.path());
                continue;
            }
        };
        if entry.path() != path(paths, &intent.computer_id) {
            recovered.failures.push(format!(
                "{}: Saved computer action identity is invalid.",
                intent.name
            ));
            unreadable.push(entry.path());
            continue;
        }
        let result = inspect(runner, paths, &intent)
            .and_then(|initial| settle(runner, paths, device, &mut intent, initial));
        if let Err(failure) = &result {
            recovered
                .failures
                .push(format!("{}: {}", intent.name, safe_activity_error(failure)));
        }
        // A resumed explicit stop, settled or not, wins over the launch selection.
        if intent.action == "stop" {
            recovered.keep_stopped.insert(intent.computer_id);
        }
    }
    // An unreadable action may be an explicit stop: keep its computer (named by the
    // file) out of automatic start rather than guess.
    if !unreadable.is_empty() {
        if let Ok(metadata) = read_metadata(&paths.metadata) {
            for configuration in metadata.computers.iter() {
                if unreadable.contains(&path(paths, configuration.id())) {
                    recovered.keep_stopped.insert(configuration.id().to_owned());
                }
            }
        }
    }
    Ok(recovered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_migrated_lifecycle_operation_loads() {
        let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
        let intent = load(&migrated.storage.join("lifecycle-operations/abc.json"))
            .unwrap()
            .unwrap();
        assert_eq!(
            intent.computer_id,
            crate::runtime_migration::vocabulary_tests::ID
        );
        assert_eq!(intent.name, "dev");
        assert!(intent.phase == Phase::StopPending);
    }
    use std::os::unix::fs::PermissionsExt;
    const ID: &str = "00000000-0000-4000-8000-000000000001";
    struct Fake {
        state: Mutex<String>,
        calls: Mutex<Vec<String>>,
        fail_start: bool,
        launch_start_once: AtomicBool,
        cancel_start: bool,
        stop_times_out: bool,
        start_wins: bool,
        replaced: bool,
        fail_history_on_mutation: bool,
    }
    impl Fake {
        fn new(state: &str) -> Self {
            Self {
                state: Mutex::new(state.into()),
                calls: Mutex::new(vec![]),
                fail_start: false,
                launch_start_once: AtomicBool::new(false),
                cancel_start: false,
                stop_times_out: false,
                start_wins: false,
                replaced: false,
                fail_history_on_mutation: false,
            }
        }
        fn mutations(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|s| *s != "inspect")
                .cloned()
                .collect()
        }
    }
    impl RuntimeRunner for Fake {
        fn run(
            &self,
            paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            let action = args[0].as_str();
            self.calls.lock().unwrap().push(action.into());
            if action != "inspect" && self.fail_history_on_mutation {
                fs::set_permissions(
                    paths.metadata.parent().unwrap(),
                    fs::Permissions::from_mode(0o555),
                )
                .unwrap();
            }
            if action == "start" && self.launch_start_once.swap(false, Ordering::SeqCst) {
                return Err(RuntimeError::Launch("Synthetic launch failure.".into()));
            }
            if action == "start" && self.cancel_start {
                return Err(RuntimeError::Cancelled {
                    operation: "start dev".into(),
                });
            }
            if action == "start" {
                if !self.fail_start || self.start_wins {
                    *self.state.lock().unwrap() = "Running".into();
                }
                if self.fail_start {
                    return Err(error("Synthetic start interruption."));
                }
            }
            if action == "stop" && self.stop_times_out {
                return Err(RuntimeError::TimedOut {
                    operation: "Stopping dev".into(),
                });
            }
            if action == "stop" {
                *self.state.lock().unwrap() = "Stopped".into();
            }
            Ok(CommandOutput {
                stdout: if action == "inspect" {
                    json!({"name":"dev","status":*self.state.lock().unwrap(),"config":{"labels":{"silo.managed":"true","silo.machine-id":if self.replaced {"different"} else {ID}},"resources":{"cpus":1,"max_cpus":1,"memory_mib":1024,"max_memory_mib":1024}}}).to_string()
                } else {
                    "null".into()
                },
                stderr: String::new(),
            })
        }
    }
    fn device() -> DeviceResources {
        DeviceResources {
            logical_cpus: 2,
            physical_memory_bytes: Some(4 * 1024 * 1024 * 1024),
        }
    }
    fn setup() -> (tempfile::TempDir, RuntimePaths, ComputerConfiguration) {
        let dir = tempfile::tempdir().unwrap();
        let paths = RuntimePaths {
            executable: dir.path().join("msb"),
            library: dir.path().join("library"),
            home: dir.path().join("home"),
            storage_home: None,
            guest_image: dir.path().join("image"),
            metadata: dir.path().join("computers.json"),
            volumes: dir.path().join("volumes"),
        };
        let configuration = ComputerConfiguration {
            id: ID.into(),
            name: "dev".into(),
            cpus: 1,
            max_cpus: 1,
            memory_gib: 1,
            max_memory_gib: 1,
            workspace_storage_gib: 1,
            runtime_storage_gib: 1,
            desktop: None,
        };
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![configuration.clone()],
            },
        )
        .unwrap();
        (dir, paths, configuration)
    }
    fn pending(paths: &RuntimePaths, action: &str, phase: Phase) -> Intent {
        let value = Intent {
            version: 1,
            computer_id: ID.into(),
            name: "dev".into(),
            action: action.into(),
            phase,
            event: runtime_activity::begin(paths, action, "dev", ID).unwrap(),
        };
        store(paths, &value).unwrap();
        value
    }
    #[test]
    fn lifecycle_retry_does_not_undo_a_newer_stop_on_the_same_computer() {
        let _test_state = crate::test_support::global_state();
        for same_computer in [true, false] {
            let (_dir, paths, _) = setup();
            let runner = Fake::new("Stopped");
            runner.launch_start_once.store(true, Ordering::SeqCst);
            let acquisitions = std::sync::atomic::AtomicUsize::new(0);
            let last_request = std::cell::Cell::new(None);
            let result = gated_auto_retry_with(
                &[Duration::ZERO],
                "Starting dev",
                |label| {
                    if acquisitions.fetch_add(1, Ordering::SeqCst) == 1 {
                        let stop = OPERATIONS
                            .kind(operation_gate::OperationKind::Lifecycle)
                            .computer(
                                if same_computer { ID } else { "other-computer" },
                                "dev",
                                "Stopping dev",
                            )
                            .unwrap();
                        if same_computer {
                            perform(&runner, &paths, &device(), "stop", "dev").unwrap();
                        }
                        drop(stop);
                    }
                    let guard = OPERATIONS
                        .kind(operation_gate::OperationKind::Lifecycle)
                        .retry_after(last_request.get())
                        .computer(ID, "dev", label)
                        .map_err(RuntimeError::from)?;
                    last_request.set(Some(guard.request_id()));
                    Ok(guard)
                },
                |_| {},
                || perform(&runner, &paths, &device(), "start", "dev"),
            );
            if same_computer {
                assert!(matches!(result, Err(RuntimeError::Cancelled { .. })));
                assert_eq!(*runner.state.lock().unwrap(), "Stopped");
                assert_eq!(runner.mutations(), vec!["start"]);
            } else {
                result.unwrap();
                assert_eq!(*runner.state.lock().unwrap(), "Running");
                assert_eq!(runner.mutations(), vec!["start", "start"]);
            }
            assert!(!has_intent(&paths, ID));
        }
    }

    #[test]
    fn malformed_history_does_not_block_lifecycle_actions() {
        let _test_state = crate::test_support::global_state();
        for (action, state, commands) in [
            ("stop", "Running", vec!["stop"]),
            ("start", "Stopped", vec!["start"]),
            ("restart", "Running", vec!["stop", "start"]),
        ] {
            let (_dir, paths, _) = setup();
            let history = paths.metadata.with_file_name("computer-activity.json");
            fs::write(&history, "{broken-json").unwrap();
            let runner = Fake::new(state);
            perform(&runner, &paths, &device(), action, "dev").unwrap();
            assert_eq!(runner.mutations(), commands);
            assert!(!has_intent(&paths, ID));
            assert_eq!(fs::read(&history).unwrap(), b"{broken-json");
            assert!(runtime_activity::read(&paths)
                .unwrap()
                .iter()
                .any(|event| event["title"] == "Activity history unavailable"
                    && event["tone"] == "warning"));
        }
    }

    #[test]
    fn history_failure_after_start_preserves_outcome_and_retires_intent() {
        let _test_state = crate::test_support::global_state();
        for cancelled in [false, true] {
            let (_dir, paths, _) = setup();
            let mut runner = Fake::new("Stopped");
            runner.cancel_start = cancelled;
            runner.fail_history_on_mutation = true;
            let result = perform(&runner, &paths, &device(), "start", "dev");
            fs::set_permissions(
                paths.metadata.parent().unwrap(),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            if cancelled {
                assert!(matches!(result, Err(RuntimeError::Cancelled { .. })));
            } else {
                result.unwrap();
            }
            assert_eq!(runner.mutations(), vec!["start"]);
            assert!(!has_intent(&paths, ID));
            // The history remains readable even though its completion could not be saved.
            let history = paths.metadata.with_file_name("computer-activity.json");
            let entries: Vec<Value> = serde_json::from_slice(&fs::read(history).unwrap()).unwrap();
            assert!(entries.iter().all(|event| event["completed"] == false));
            assert!(runtime_activity::read(&paths)
                .unwrap()
                .iter()
                .any(|event| event["title"] == "Activity history unavailable"
                    && event["tone"] == "warning"));
            let relaunch = Fake::new("Stopped");
            assert_eq!(
                recover_with(&relaunch, &paths, &device()).unwrap(),
                Recovered::default()
            );
            assert!(relaunch.mutations().is_empty());
        }
    }

    #[test]
    fn malformed_history_does_not_block_intent_retirement() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        pending(&paths, "start", Phase::StartPending);
        fs::write(
            paths.metadata.with_file_name("computer-activity.json"),
            "{broken-json",
        )
        .unwrap();
        retire_except(&paths, &HashSet::new()).unwrap();
        assert!(!has_intent(&paths, ID));
        let runner = Fake::new("Stopped");
        assert_eq!(
            recover_with(&runner, &paths, &device()).unwrap(),
            Recovered::default()
        );
        assert!(runner.mutations().is_empty());
    }

    #[test]
    fn intent_write_failure_records_a_failure_without_runtime_mutation() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        fs::write(directory(&paths), "not a directory").unwrap();
        let runner = Fake::new("Running");
        assert!(perform(&runner, &paths, &device(), "stop", "dev").is_err());
        assert!(runner.mutations().is_empty());
        let history = runtime_activity::read(&paths).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["status"], "completed");
        assert_eq!(history[0]["tone"], "danger");
        assert!(history[0]["detail"]
            .as_str()
            .unwrap()
            .contains("Computer action progress could not be saved."));
    }

    #[test]
    fn intent_reader_accepts_the_limit_and_preserves_oversized_input() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        pending(&paths, "stop", Phase::StopPending);
        let target = path(&paths, ID);
        let mut bytes = fs::read(&target).unwrap();
        bytes.resize(MAX_OUTPUT_BYTES as usize, b' ');
        fs::write(&target, &bytes).unwrap();
        assert!(load(&target).unwrap().is_some());
        bytes.push(b' ');
        fs::write(&target, &bytes).unwrap();
        assert!(load(&target)
            .err()
            .unwrap()
            .to_string()
            .contains("Saved computer action is too large."));
        assert_eq!(fs::read(&target).unwrap(), bytes);
    }

    #[test]
    fn oversized_intent_is_rejected_without_waiting_for_end_of_file() {
        use std::os::unix::ffi::OsStrExt;
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        fs::create_dir_all(directory(&paths)).unwrap();
        let target = path(&paths, ID);
        let name = std::ffi::CString::new(target.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let (completed, result) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            completed.send(load(&target).map(|_| ())).unwrap();
        });
        let mut writer = fs::OpenOptions::new()
            .write(true)
            .open(path(&paths, ID))
            .unwrap();
        writer
            .write_all(&vec![b' '; MAX_OUTPUT_BYTES as usize + 1])
            .unwrap();
        let rejected = result.recv_timeout(Duration::from_secs(2));
        drop(writer);
        reader.join().unwrap();
        let failure = rejected
            .expect("oversized intent should be rejected before its writer closes")
            .unwrap_err();
        assert!(failure
            .to_string()
            .contains("Saved computer action is too large."));
    }

    #[test]
    fn restart_recovers_each_checkpoint_without_repeating_a_finished_stop_or_restart() {
        let _test_state = crate::test_support::global_state();
        for (phase, state, commands) in [
            (Phase::StopPending, "Running", vec!["stop", "start"]),
            (Phase::StopPending, "Stopped", vec!["start"]),
            (Phase::StartPending, "Stopped", vec!["start"]),
            (Phase::StartPending, "Running", vec![]),
        ] {
            let (_dir, paths, _) = setup();
            let original = pending(&paths, "restart", phase);
            let event_before = serde_json::to_value(&original.event).unwrap();
            let runner = Fake::new(state);
            assert_eq!(
                recover_with(&runner, &paths, &device()).unwrap(),
                Recovered::default()
            );
            assert_eq!(runner.mutations(), commands);
            assert!(!path(&paths, ID).exists());
            let history = runtime_activity::read(&paths).unwrap();
            let event = history
                .iter()
                .find(|event| event["id"] == event_before["id"])
                .unwrap();
            assert_eq!(event["tone"], "success");
            assert_eq!(event["title"], "Computer restarted");
            assert_eq!(history.len(), 1);
            recover_with(&runner, &paths, &device()).unwrap();
            assert_eq!(runner.mutations(), commands);
        }
    }
    #[test]
    fn stop_is_resumed_and_crashed_is_already_stopped() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        pending(&paths, "stop", Phase::StopPending);
        let runner = Fake::new("Running");
        assert_eq!(
            recover_with(&runner, &paths, &device())
                .unwrap()
                .keep_stopped,
            HashSet::from([ID.into()])
        );
        assert_eq!(runner.mutations(), vec!["stop"]);
        let crashed = Fake::new("Crashed");
        perform(&crashed, &paths, &device(), "stop", "dev").unwrap();
        assert!(!path(&paths, ID).exists());
        assert!(crashed.mutations().is_empty());
    }
    #[test]
    fn dismissal_cancels_failed_start_recovery_and_preserves_activity() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, configuration) = setup();
        pending(&paths, "start", Phase::StartPending);
        let history = runtime_activity::read(&paths).unwrap();
        dismiss_crashed_intent(&paths, &configuration).unwrap();
        let crashed = Fake::new("Crashed");
        assert_eq!(
            recover_with(&crashed, &paths, &device()).unwrap(),
            Recovered::default()
        );
        assert!(crashed.mutations().is_empty());
        assert_eq!(runtime_activity::read(&paths).unwrap(), history);
    }

    #[test]
    fn stop_and_restart_clear_pending_secret_revocations_even_when_the_next_start_fails() {
        let _test_state = crate::test_support::global_state();
        for action in ["stop", "restart"] {
            let (dir, paths, _) = setup();
            let store = dir.path().join("secrets.json");
            fs::write(&store, r#"{"pendingRevocations":[{"secretId":"old","generation":"removed-value","name":"API_KEY","computer":"dev"}]}"#).unwrap();
            crate::secrets::use_test_store(Some(store));
            crate::secrets::use_test_vault(Some(Default::default()));
            let mut runner = Fake::new("Running");
            runner.fail_start = true;
            let result = perform(&runner, &paths, &device(), action, "dev");
            assert_eq!(result.is_ok(), action == "stop");
            assert!(crate::secrets::pending_names("dev").unwrap().is_empty());
            crate::secrets::use_test_store(None);
            crate::secrets::use_test_vault(None);
        }
    }

    #[test]
    fn restart_of_crashed_computer_starts_without_attempting_a_stop() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        let runner = Fake::new("Crashed");
        perform(&runner, &paths, &device(), "restart", "dev").unwrap();
        assert_eq!(runner.mutations(), vec!["start"]);
    }

    #[test]
    fn failed_start_keeps_intent_and_same_session_retry_reuses_activity() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        let mut runner = Fake::new("Stopped");
        runner.fail_start = true;
        assert!(perform(&runner, &paths, &device(), "start", "dev").is_err());
        assert!(path(&paths, ID).exists());
        runner.fail_start = false;
        perform(&runner, &paths, &device(), "start", "dev").unwrap();
        assert!(!path(&paths, ID).exists());
        let history = runtime_activity::read(&paths).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["tone"], "success");
    }
    #[test]
    fn cancelled_start_retires_its_intent_so_launch_does_not_resume_it() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        let mut runner = Fake::new("Stopped");
        runner.cancel_start = true;
        assert!(matches!(
            perform(&runner, &paths, &device(), "start", "dev"),
            Err(RuntimeError::Cancelled { .. })
        ));
        assert!(!path(&paths, ID).exists());
        let relaunch = Fake::new("Stopped");
        assert_eq!(
            recover_with(&relaunch, &paths, &device()).unwrap(),
            Recovered::default()
        );
        assert!(relaunch.mutations().is_empty());
    }
    #[test]
    fn one_invalid_intent_does_not_block_recovery_of_the_others() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        pending(&paths, "stop", Phase::StopPending);
        fs::write(directory(&paths).join("broken.json"), "{not json").unwrap();
        let runner = Fake::new("Running");
        let recovered = recover_with(&runner, &paths, &device()).unwrap();
        assert_eq!(recovered.failures.len(), 1);
        // The resumed explicit stop still keeps its computer out of launch auto-start.
        assert_eq!(recovered.keep_stopped, HashSet::from([ID.into()]));
        assert_eq!(runner.mutations(), vec!["stop"]);
        assert!(!path(&paths, ID).exists());
        assert!(directory(&paths).join("broken.json").exists());
    }
    #[test]
    fn an_unreadable_action_keeps_its_computer_out_of_launch_start_without_failing_recovery() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        // A future-version file, for example written by a newer Silo before a downgrade.
        fs::create_dir_all(directory(&paths)).unwrap();
        fs::write(path(&paths, ID), br#"{"version":2}"#).unwrap();
        let runner = Fake::new("Stopped");
        let recovered = recover_with(&runner, &paths, &device()).unwrap();
        assert_eq!(recovered.failures.len(), 1);
        assert_eq!(recovered.keep_stopped, HashSet::from([ID.into()]));
        assert!(runner.mutations().is_empty());
        assert!(
            path(&paths, ID).exists(),
            "the unreadable action is preserved"
        );
    }
    #[test]
    fn an_explicit_stop_that_cannot_be_resumed_still_wins_over_launch_start() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        pending(&paths, "stop", Phase::StopPending);
        let mut runner = Fake::new("Running");
        runner.replaced = true;
        let recovered = recover_with(&runner, &paths, &device()).unwrap();
        assert_eq!(recovered.failures.len(), 1);
        assert_eq!(recovered.keep_stopped, HashSet::from([ID.into()]));
    }
    #[test]
    fn an_explicit_action_replaces_a_saved_action_it_cannot_continue() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        fs::create_dir_all(directory(&paths)).unwrap();
        fs::write(path(&paths, ID), br#"{"version":2}"#).unwrap();
        let runner = Fake::new("Stopped");
        perform(&runner, &paths, &device(), "start", "dev").unwrap();
        assert_eq!(runner.mutations(), vec!["start"]);
        assert!(!path(&paths, ID).exists());
        // A saved action for the computer under an earlier name is settled, not resumed.
        store(
            &paths,
            &Intent {
                version: 1,
                computer_id: ID.into(),
                name: "old-dev".into(),
                action: "stop".into(),
                phase: Phase::StopPending,
                event: runtime_activity::begin(&paths, "stop", "old-dev", ID).unwrap(),
            },
        )
        .unwrap();
        let runner = Fake::new("Running");
        perform(&runner, &paths, &device(), "stop", "dev").unwrap();
        assert_eq!(runner.mutations(), vec!["stop"]);
        assert!(!path(&paths, ID).exists());
        let history = runtime_activity::read(&paths).unwrap();
        assert!(history.iter().any(|event| event["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("Replaced by"))));
    }
    #[test]
    fn update_preparation_retires_actions_for_computers_it_will_not_resume() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        // A failed start kept for Retry: its activity already records the failure.
        let mut runner = Fake::new("Stopped");
        runner.fail_start = true;
        assert!(perform(&runner, &paths, &device(), "start", "dev").is_err());
        assert!(path(&paths, ID).exists());
        let failed = runtime_activity::read(&paths).unwrap();
        // A computer that the update will resume keeps its saved action.
        retire_except(&paths, &HashSet::from([ID.into()])).unwrap();
        assert!(path(&paths, ID).exists());
        retire_except(&paths, &HashSet::new()).unwrap();
        assert!(!path(&paths, ID).exists());
        assert_eq!(
            runtime_activity::read(&paths).unwrap(),
            failed,
            "a finished outcome is kept"
        );
        let relaunch = Fake::new("Stopped");
        assert_eq!(
            recover_with(&relaunch, &paths, &device()).unwrap(),
            Recovered::default()
        );
        assert!(
            relaunch.mutations().is_empty(),
            "the next launch does not start it"
        );
        // An unfinished action is settled as cancelled; unreadable files stay for recovery.
        pending(&paths, "restart", Phase::StartPending);
        fs::write(directory(&paths).join("broken.json"), "{not json").unwrap();
        retire_except(&paths, &HashSet::new()).unwrap();
        assert!(!path(&paths, ID).exists());
        assert!(directory(&paths).join("broken.json").exists());
        let history = runtime_activity::read(&paths).unwrap();
        assert!(history
            .iter()
            .any(|event| event["title"] == "Restart cancelled"));
    }
    #[test]
    fn timed_out_stop_that_never_settles_is_not_retried_as_transient() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        let mut runner = Fake::new("Running");
        runner.stop_times_out = true;
        let failure = perform(&runner, &paths, &device(), "stop", "dev").unwrap_err();
        assert!(!crate::runtime::transient_runtime_error(&failure));
        assert_eq!(runner.mutations(), vec!["stop"]);
    }
    #[test]
    fn surviving_detached_start_can_win_without_being_restarted() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        pending(&paths, "start", Phase::StartPending);
        let mut runner = Fake::new("Stopped");
        runner.fail_start = true;
        runner.start_wins = true;
        recover_with(&runner, &paths, &device()).unwrap();
        assert_eq!(runner.mutations(), vec!["start"]);
        assert!(!path(&paths, ID).exists());
    }
    #[test]
    fn replacement_runtime_is_preserved_and_confirmed_deletion_retires_only_its_intent() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, configuration) = setup();
        pending(&paths, "restart", Phase::StopPending);
        let mut runner = Fake::new("Running");
        runner.replaced = true;
        assert_eq!(
            recover_with(&runner, &paths, &device())
                .unwrap()
                .failures
                .len(),
            1
        );
        assert!(runner.mutations().is_empty());
        assert!(path(&paths, ID).exists());
        forget_removed(&paths, &configuration).unwrap();
        assert!(!path(&paths, ID).exists());
        assert_eq!(
            recover_with(&runner, &paths, &device()).unwrap(),
            Recovered::default()
        );
    }
    #[test]
    fn rejected_new_action_leaves_the_superseded_intent_and_activity_unchanged() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        pending(&paths, "restart", Phase::StartPending);
        let mut runner = Fake::new("Running");
        runner.replaced = true;
        assert!(perform(&runner, &paths, &device(), "stop", "dev").is_err());
        assert!(load(&path(&paths, ID))
            .unwrap()
            .is_some_and(|saved| saved.action == "restart"));
        let history = runtime_activity::read(&paths).unwrap();
        assert!(!history.iter().any(|event| event["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("Replaced by"))));
    }
    #[test]
    fn explicit_new_action_settles_the_superseded_activity() {
        let _test_state = crate::test_support::global_state();
        let (_dir, paths, _) = setup();
        pending(&paths, "restart", Phase::StartPending);
        perform(&Fake::new("Running"), &paths, &device(), "stop", "dev").unwrap();
        let history = runtime_activity::read(&paths).unwrap();
        assert_eq!(history.len(), 2);
        assert!(history.iter().any(|event| event["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("Replaced by"))));
        assert!(history.iter().all(|event| event["status"] == "completed"));
    }
    fn live_paths(root: &Path) -> RuntimePaths {
        RuntimePaths {
            executable: PathBuf::from(std::env::var("SILO_TEST_MSB").expect("set SILO_TEST_MSB")),
            library: PathBuf::from(
                std::env::var("SILO_TEST_LIBKRUNFW").expect("set SILO_TEST_LIBKRUNFW"),
            ),
            home: root.join("msb"),
            storage_home: None,
            guest_image: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/guest-image"),
            metadata: root.join("computers.json"),
            volumes: root.join("volumes"),
        }
    }
    #[test]
    #[ignore = "child of the explicitly requested disposable lifecycle recovery test"]
    fn lifecycle_recovery_crash_child() {
        let _test_state = crate::test_support::global_state();
        crate::test_support::live::require_confirmation();
        let root = std::env::var("SILO_TEST_LIFECYCLE_ROOT").expect("missing isolated test root");
        let action =
            std::env::var("SILO_TEST_LIFECYCLE_ACTION").expect("missing isolated test action");
        assert!(matches!(action.as_str(), "start" | "stop"));
        let paths = live_paths(Path::new(&root));
        ProcessRunner
            .run(
                &paths,
                &[
                    action.clone(),
                    "lifecycle-recovery-test".into(),
                    "--quiet".into(),
                ],
                MUTATION_TIMEOUT,
            )
            .unwrap();
        let observed = inspect_computer(&ProcessRunner, &paths, "lifecycle-recovery-test").unwrap();
        assert_eq!(
            observed.status,
            if action == "start" {
                "Running"
            } else {
                "Stopped"
            }
        );
        // Exit without executing the next journal checkpoint or Rust cleanup.
        std::process::exit(73);
    }
    #[test]
    #[ignore = "requires signed bundled runtime, guest image and hardware virtualization; uses only a disposable computer"]
    fn lifecycle_recovery_survives_real_worker_exit_without_repeating_restart() {
        let _test_state = crate::test_support::global_state();
        crate::test_support::live::require_confirmation();
        let directory = tempfile::Builder::new()
            .prefix("silo-lifecycle-live-")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let paths = live_paths(directory.path());
        let name = "lifecycle-recovery-test";
        let device = device_resources().unwrap();
        let result = (|| -> Result<(), RuntimeError> {
            let configuration = create_disposable_test_computer(&paths, name)?;
            perform(&ProcessRunner, &paths, &device, "start", name)?;
            let boot = || {
                ProcessRunner
                    .run(
                        &paths,
                        &[
                            "exec".into(),
                            name.into(),
                            "--no-tty".into(),
                            "--quiet".into(),
                            "--".into(),
                            "cat".into(),
                            "/proc/sys/kernel/random/boot_id".into(),
                        ],
                        READ_TIMEOUT,
                    )
                    .map(|output| output.stdout)
            };
            let first_boot = boot()?;
            let checkpoint = |action: &str, phase: Phase| -> Result<(), RuntimeError> {
                store(
                    &paths,
                    &Intent {
                        version: 1,
                        computer_id: configuration.id().into(),
                        name: name.into(),
                        action: action.into(),
                        phase,
                        event: runtime_activity::begin(&paths, action, name, configuration.id())
                            .map_err(error)?,
                    },
                )
            };
            let exit_child = |action: &str| -> Result<(), RuntimeError> {
                let status = Command::new(
                    std::env::current_exe().map_err(|_| error("Cannot locate test executable."))?,
                )
                .args([
                    "runtime::lifecycle_recovery::tests::lifecycle_recovery_crash_child",
                    "--exact",
                    "--ignored",
                    "--test-threads=1",
                ])
                .env("SILO_TEST_LIFECYCLE_ROOT", directory.path())
                .env("SILO_TEST_LIFECYCLE_ACTION", action)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|_| error("Cannot launch isolated recovery worker."))?;
                if status.code() != Some(73) {
                    return Err(error("Isolated worker did not reach its crash checkpoint."));
                }
                Ok(())
            };
            let recover = || -> Result<Recovered, RuntimeError> {
                let recovered = recover_with(&ProcessRunner, &paths, &device)?;
                if recovered.failures.is_empty() {
                    Ok(recovered)
                } else {
                    Err(error(recovered.failures.join("\n")))
                }
            };
            checkpoint("restart", Phase::StopPending)?;
            exit_child("stop")?;
            recover()?;
            let restarted_boot = boot()?;
            if first_boot == restarted_boot {
                return Err(error(
                    "Recovered restart did not boot a new computer generation.",
                ));
            }
            checkpoint("restart", Phase::StartPending)?;
            recover()?;
            if boot()? != restarted_boot {
                return Err(error("Completed restart was repeated after recovery."));
            }
            checkpoint("stop", Phase::StopPending)?;
            if !recover()?.keep_stopped.contains(configuration.id()) {
                return Err(error(
                    "Recovered explicit stop was not excluded from automatic start.",
                ));
            }
            checkpoint("start", Phase::StartPending)?;
            exit_child("start")?;
            let child_boot = boot()?;
            recover()?;
            if boot()? != child_boot {
                return Err(error(
                    "Surviving detached start was restarted during recovery.",
                ));
            }
            if path(&paths, configuration.id()).exists() {
                return Err(error(
                    "Successful lifecycle recovery left a pending journal.",
                ));
            }
            Ok(())
        })();
        let _ = ProcessRunner.run(
            &paths,
            &["stop".into(), name.into(), "--quiet".into()],
            STOP_TIMEOUT,
        );
        let removed = ProcessRunner.run(
            &paths,
            &[
                "remove".into(),
                name.into(),
                "--force".into(),
                "--quiet".into(),
            ],
            STOP_TIMEOUT,
        );
        let absent = list_managed(&ProcessRunner, &paths).is_ok_and(|computers| {
            computers
                .iter()
                .all(|configuration| configuration.name != name)
        });
        assert!(
            removed.is_ok() && absent,
            "Disposable lifecycle computer cleanup failed."
        );
        assert!(result.is_ok(), "{}", result.unwrap_err());
    }
}
