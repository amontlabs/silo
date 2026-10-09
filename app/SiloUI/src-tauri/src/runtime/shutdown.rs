//! Quit takes the same operation gate as normal computer operations. It never
//! follows saved SSH connections or sends commands to another device.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static QUITTING: AtomicBool = AtomicBool::new(false);
/// Counts shutdowns that began (Quit or update installation), including ones later
/// cancelled. Work requested before a shutdown compares it to decide not to run
/// after that shutdown stopped the computers, even when a failed Quit reopened admission.
static QUIT_GENERATION: AtomicU64 = AtomicU64::new(0);
static MAINTENANCE_DEADLINE: Mutex<Option<Instant>> = Mutex::new(None);

pub(crate) fn begin() {
    if let Ok(mut deadline) = MAINTENANCE_DEADLINE.lock() {
        *deadline = Some(Instant::now() + storage::TRIM_BUDGET);
    }
    QUIT_GENERATION.fetch_add(1, Ordering::SeqCst);
    QUITTING.store(true, Ordering::SeqCst);
    crate::transfer::cancel_all();
}

/// The current shutdown generation. A retry sequence captures it before its first
/// attempt and stops once it changes (D-30).
pub(crate) fn generation() -> u64 {
    QUIT_GENERATION.load(Ordering::SeqCst)
}
pub(crate) fn cancel() {
    QUITTING.store(false, Ordering::SeqCst);
    crate::macos_computers::reopen();
    if let Ok(mut deadline) = MAINTENANCE_DEADLINE.lock() {
        *deadline = None;
    }
}

pub(super) fn maintenance_budget() -> Duration {
    MAINTENANCE_DEADLINE
        .lock()
        .map(|deadline| {
            deadline.map_or(storage::TRIM_BUDGET, |at| {
                at.saturating_duration_since(Instant::now())
            })
        })
        .unwrap_or(Duration::ZERO)
}

/// Quit-safe only after taking the operation gate: every admitted operation must
/// call this once its turn arrives, so admission cannot race Quit. Callers may also
/// call it earlier to fail fast (for example before queueing), which is why it does
/// not assert `operation_gate::held()` (D-43).
pub(crate) fn ensure_accepting_operations() -> Result<(), String> {
    if QUITTING.load(Ordering::SeqCst) {
        Err(
            "Silo is quitting and stopping its local computers. Wait for shutdown to finish."
                .into(),
        )
    } else {
        Ok(())
    }
}

/// The longest Quit waits for a cancelled file transfer to remove its partial files.
pub(crate) const TRANSFER_DRAIN: Duration = Duration::from_secs(5);
/// Time the drain leaves for closing connections, stopping computers and saving state.
const STOP_RESERVE: Duration = Duration::from_secs(3);

/// The wait for cancelled transfers: at most `TRANSFER_DRAIN`, and only the part of
/// `remaining` beyond `STOP_RESERVE`, which can be zero.
fn transfer_drain_budget(remaining: Duration) -> Duration {
    remaining.saturating_sub(STOP_RESERVE).min(TRANSFER_DRAIN)
}

/// The time left until the earlier of the maintenance budget and `deadline`.
fn remaining_budget(deadline: Option<Instant>, now: Instant) -> Duration {
    let maintenance = maintenance_budget();
    deadline.map_or(maintenance, |at| {
        maintenance.min(at.saturating_duration_since(now))
    })
}

/// Waits for the transfer that shutdown cancelled to finish its cleanup, which needs
/// its connection to the computer, so it must precede closing connections and
/// stopping computers. `deadline` is the session-end limit, if one applies.
fn drain_transfers(deadline: Option<Instant>) {
    crate::transfer::close_all(transfer_drain_budget(remaining_budget(
        deadline,
        Instant::now(),
    )));
}

/// Runs Quit's `stop` after `drain` has let the cancelled transfers clean up.
fn drain_then<T>(drain: impl FnOnce(), stop: impl FnOnce() -> T) -> T {
    drain();
    stop()
}

pub(crate) fn stop_local_computers(
    app: &AppHandle,
    deadline: Option<Instant>,
) -> Result<(), String> {
    let result = drain_then(
        || drain_transfers(deadline),
        || {
            while_quitting(&OPERATIONS, |guard| {
                // Quit has stopped admission and holds the operation gate: the SSH monitor
                // cannot restore listeners while local computer shutdown is in progress.
                crate::ssh_access::close_all();
                crate::desktop_viewer::close_all();
                // macOS computers live in this process. Quit runs on a worker thread, so the
                // main thread stays free for the framework's callbacks while they stop.
                let macos = crate::macos_computers::stop_all(app, deadline);
                // With the storage migration unfinished no runtime is in use, so no computer of this
                // Silo can be running and Quit has nothing to stop.
                let Some(paths) = runtime_paths_if_in_use(app)? else {
                    return macos;
                };
                // The quit overlay follows the queue and shows which computer is stopping (D-29).
                let progress = |name: &str, index: usize, total: usize| {
                    guard.relabel(&format!("Stopping {name} ({index} of {total})"));
                };
                stop_local_computers_with(&ProcessRunner, &paths, &progress)
                    .map_err(|error| safe_activity_error(&error))?;
                macos
            })
        },
    );
    let _ = app.emit("silo://application-state-changed", ());
    result
}

/// Run Quit's shutdown `work` holding the device-wide gate.
fn while_quitting<T>(
    gate: &'static operation_gate::OperationGate,
    work: impl FnOnce(&operation_gate::OperationGuard<'static>) -> Result<T, String>,
) -> Result<T, String> {
    // Admission is already refused, so any waiter would only be rejected when its turn
    // came. Cancel every waiting entry up front so Quit is not queued behind work that
    // can no longer start, and so the quit overlay reflects only the running blockers.
    gate.cancel_all_waiting();
    let guard = gate
        .kind(operation_gate::OperationKind::Shutdown)
        .device("Stopping local computers")
        .map_err(|_| {
            "A computer operation failed unexpectedly. Check local computer status before retrying Quit."
                .to_string()
        })?;
    let result = work(&guard);
    // Work that queued behind Quit was requested before its computers stopped. If Quit
    // fails and admission reopens, it must not start a computer Quit just stopped (D-30),
    // so it leaves the queue before the gate is released.
    gate.cancel_all_waiting();
    drop(guard);
    result
}

/// Stop every present local computer. `progress` receives each computer that needs a stop with
/// its one-based position among them, before that computer's stop starts.
fn stop_local_computers_with(
    runner: &(dyn RuntimeRunner + Sync),
    paths: &RuntimePaths,
    progress: &dyn Fn(&str, usize, usize),
) -> Result<(), RuntimeError> {
    let committed = read_metadata(&paths.metadata)?.computers;
    let mut computers = committed.clone();
    for pending in configuration_recovery::shutdown_computers(paths)? {
        if !computers
            .iter()
            .any(|configuration| configuration.id() == pending.id())
        {
            computers.push(pending);
        }
    }
    if runtime_never_initialized(paths) || (computers.is_empty() && !paths.home.exists()) {
        return Ok(());
    }
    let present: HashSet<_> = list_managed(runner, paths)?
        .into_iter()
        .map(|computer| computer.name)
        .collect();
    // Stopping does not require host capacity, unlike creating or starting.
    let device = DeviceResources {
        logical_cpus: 0,
        physical_memory_bytes: None,
    };
    let mut failures: Vec<String> = present.iter()
        .filter(|name| !computers.iter().any(|configuration| configuration.name() == name.as_str()))
        .map(|name| format!("{name}: Silo found a managed computer without a matching saved identity. Repair its configuration before quitting."))
        .collect();
    let mut targets = Vec::new();
    for configuration in computers
        .iter()
        .filter(|configuration| present.contains(configuration.name()))
    {
        let committed_computer = committed
            .iter()
            .any(|entry| entry.id() == configuration.id());
        // A replacement may reuse a removed computer's name. Its journal retains both
        // identities, but only the identity actually present needs to stop.
        if computers
            .iter()
            .any(|other| other.name() == configuration.name() && other.id() != configuration.id())
            && inspect_computer(runner, paths, configuration.name()).is_ok_and(|observed| {
                observed.name == configuration.name()
                    && ensure_managed(&observed).is_ok()
                    && computers.iter().any(|other| {
                        other.name() == configuration.name()
                            && other.id() != configuration.id()
                            && observed
                                .config
                                .pointer("/labels/silo.machine-id")
                                .and_then(Value::as_str)
                                == Some(other.id())
                    })
            })
        {
            continue;
        }
        // A computer that is already stopped with no saved action needs no stop and
        // no "Computer stopped" activity entry. Anything else goes through
        // perform, which verifies identity and settles transitions.
        if committed_computer
            && !lifecycle_recovery::has_intent(paths, configuration.id())
            && inspect_computer(runner, paths, configuration.name()).is_ok_and(|computer| {
                matches!(
                    computer.status.to_ascii_lowercase().as_str(),
                    "stopped" | "created" | "crashed"
                )
            })
        {
            continue;
        }
        targets.push((configuration, committed_computer));
    }
    if !targets.is_empty() {
        // One cross-process worker flock covers the entire shutdown transaction.
        // Separate flock acquisitions in the workers would serialize every stop.
        let worker_lock = configuration_recovery::command_lock(paths, STOP_TIMEOUT)?;
        let locks = targets
            .iter()
            .map(|_| worker_lock.duplicate_for_shutdown())
            .collect::<Result<Vec<_>, _>>()?;
        let device = &device;
        thread::scope(|scope| {
            let mut workers = Vec::new();
            for (index, ((configuration, committed_computer), lock)) in
                targets.iter().zip(locks).enumerate()
            {
                progress(configuration.name(), index + 1, targets.len());
                workers.push((
                    configuration.name(),
                    scope.spawn(move || {
                        with_shutdown_worker_lock(lock, || {
                            // Every worker verifies ownership and immutable identity before
                            // stopping, while the parent retains the device operation gate.
                            if *committed_computer {
                                checkpoints::release_paused_restore(runner, paths, configuration);
                                lifecycle_recovery::perform(
                                    runner,
                                    paths,
                                    device,
                                    "stop",
                                    configuration.name(),
                                )
                            } else {
                                stop_uncommitted_computer(runner, paths, configuration)
                            }
                        })
                    }),
                ));
            }
            for (name, worker) in workers {
                let result = worker.join().unwrap_or_else(|_| {
                    Err(RuntimeError::Unavailable(
                        "The shutdown worker failed unexpectedly.".into(),
                    ))
                });
                if let Err(error) = result {
                    failures.push(format!("{name}: {}", safe_activity_error(&error)));
                }
            }
        });
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(RuntimeError::Unavailable(format!(
            "Some local computers could not stop:\n{}",
            failures.join("\n")
        )))
    }
}

// Failed first-run recovery in older versions can leave a directory in place
// of the runtime alias. Only bypass the runtime when BOTH locations contain no
// runtime state, and no surviving command owns one of the bootstrap locks.
fn runtime_never_initialized(paths: &RuntimePaths) -> bool {
    use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
    let Some(storage) = paths.storage_home.as_deref() else {
        return false;
    };
    let mut locks = Vec::new();
    for path in [&paths.home, storage] {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Ok(metadata) if metadata.is_dir() => {}
            _ => return false,
        }
        let Ok(entries) = fs::read_dir(path) else {
            return false;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return false;
            };
            if !matches!(
                entry.file_name().to_str(),
                Some(".silo-configuration-worker.lock" | ".silo-backup-worker.lock")
            ) {
                return false;
            }
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                return false;
            }
            let Ok(file) = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(entry.path())
            else {
                return false;
            };
            // SAFETY: the open file owns the descriptor. Keep every lock until
            // both directories have been checked, without waiting on children.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return false;
            }
            locks.push(file);
        }
    }
    true
}

// A failed guest verification can leave a real computer before metadata publication.
// Stop that exact journal-owned computer, preserving the unfinished configuration.
fn stop_uncommitted_computer(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    let inspect = || {
        let observed = inspect_computer(runner, paths, configuration.name())?;
        ensure_managed(&observed)?;
        if observed.name != configuration.name()
            || observed
                .config
                .pointer("/labels/silo.machine-id")
                .and_then(Value::as_str)
                != Some(configuration.id())
        {
            return Err(RuntimeError::Invalid(
                "The unfinished computer changed identity. No stop was requested.".into(),
            ));
        }
        Ok(observed)
    };
    let until = Instant::now() + MUTATION_TIMEOUT;
    let mut requested_stop = false;
    loop {
        let observed = inspect()?;
        match observed.status.to_ascii_lowercase().as_str() {
            "stopped" | "created" | "crashed" => return Ok(()),
            "running" if !requested_stop => {
                let result = runner.run(
                    paths,
                    &["stop".into(), configuration.name().into(), "--quiet".into()],
                    STOP_TIMEOUT,
                );
                requested_stop = true;
                if let Err(error) = result {
                    // The command may have stopped the computer before its client failed.
                    // Confirm the exact identity and terminal state before accepting it.
                    let observed = inspect()?;
                    if matches!(
                        observed.status.to_ascii_lowercase().as_str(),
                        "stopped" | "created" | "crashed"
                    ) {
                        return Ok(());
                    }
                    return Err(error);
                }
                continue;
            }
            "starting" | "stopping" | "draining" => {}
            _ => {
                return Err(RuntimeError::Unavailable(
                    "The unfinished computer did not stop. Check its status and retry Quit.".into(),
                ))
            }
        }
        if Instant::now() >= until {
            return Err(RuntimeError::TimedOut {
                operation: "Stopping the unfinished computer".into(),
            });
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn cancelled_transfers_drain_before_computers_stop_within_the_shutdown_budget() {
        let order = std::cell::RefCell::new(Vec::new());
        drain_then(
            || order.borrow_mut().push("drain"),
            || order.borrow_mut().push("stop"),
        );
        assert_eq!(*order.borrow(), ["drain", "stop"]);
        assert_eq!(
            transfer_drain_budget(Duration::from_secs(60)),
            TRANSFER_DRAIN
        );
        assert_eq!(transfer_drain_budget(Duration::ZERO), Duration::ZERO);
    }

    #[test]
    fn a_short_session_deadline_shrinks_the_transfer_drain_and_keeps_a_stop_reserve() {
        let now = Instant::now();
        // logind grants about 4.25 s; the drain may not consume the stop reserve.
        let logind = Duration::from_millis(4250);
        let drain = transfer_drain_budget(logind);
        assert_eq!(drain, Duration::from_millis(1250));
        assert!(logind - drain >= STOP_RESERVE);
        // A deadline inside the reserve leaves nothing to drain.
        let tight = remaining_budget(Some(now + Duration::from_secs(2)), now);
        assert_eq!(transfer_drain_budget(tight), Duration::ZERO);
        let passed = remaining_budget(Some(now), now + Duration::from_secs(1));
        assert_eq!(transfer_drain_budget(passed), Duration::ZERO);
        // Without a session deadline the maintenance budget applies.
        assert!(remaining_budget(None, now) <= storage::TRIM_BUDGET);
        assert!(
            remaining_budget(Some(now + Duration::from_secs(100)), now) <= storage::TRIM_BUDGET
        );
    }

    struct Runner {
        states: Mutex<HashMap<String, String>>,
        calls: Mutex<Vec<Vec<String>>>,
        fail: Option<String>,
        stop_barrier: Option<std::sync::Barrier>,
    }
    impl RuntimeRunner for Runner {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            if args[0] == "stop" {
                if let Some(barrier) = &self.stop_barrier {
                    barrier.wait();
                }
            }
            let mut states = self.states.lock().unwrap();
            let stdout = match args[0].as_str() {
                "list" => serde_json::to_string(&states.keys().map(|name| json!({"name":name})).collect::<Vec<_>>()).unwrap(),
                "inspect" => json!({"name":args[1],"status":states[&args[1]],"config":{"labels":{"silo.managed":"true","silo.machine-id":id(&args[1])}}}).to_string(),
                "stop" => {
                    if self.fail.as_deref() == Some(args[1].as_str()) { return Err(RuntimeError::Unavailable("Stop failed".into())); }
                    states.insert(args[1].clone(), "Stopped".into()); String::new()
                }
                _ => panic!("Unexpected command: {args:?}"),
            };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }
    fn id(name: &str) -> String {
        format!(
            "00000000-0000-4000-8000-{:012}",
            if name == "first" { 1 } else { 2 }
        )
    }
    fn setup(directory: &tempfile::TempDir) -> RuntimePaths {
        let paths = super::super::tests::paths(directory);
        let computers = ["first", "second"].map(|name| json!({"id":id(name),"name":name,"cpus":1,"maxCPUs":1,"memoryGiB":1,"maxMemoryGiB":1,"workspaceStorageGiB":10,"runtimeStorageGiB":10}));
        fs::write(
            &paths.metadata,
            json!({"schemaVersion":1,"computers":computers}).to_string(),
        )
        .unwrap();
        paths
    }
    fn runner(fail: Option<&str>) -> Runner {
        Runner {
            states: Mutex::new(HashMap::from([
                ("first".into(), "Running".into()),
                ("second".into(), "Running".into()),
            ])),
            calls: Mutex::new(vec![]),
            fail: fail.map(str::to_owned),
            stop_barrier: None,
        }
    }
    #[test]
    fn work_queued_behind_a_failed_quit_does_not_run_when_it_releases_the_gate() {
        let _test_state = crate::test_support::global_state();
        let gate: &'static operation_gate::OperationGate =
            Box::leak(Box::new(operation_gate::OperationGate::new()));
        let mut waiter = None;
        let result: Result<(), String> = while_quitting(gate, |_guard| {
            waiter = Some(std::thread::spawn(move || {
                gate.computer("id-a", "a", "Starting a").map(drop)
            }));
            let deadline = Instant::now() + Duration::from_secs(5);
            while gate.snapshot().waiting.is_empty() {
                assert!(
                    Instant::now() < deadline,
                    "the start never queued behind Quit"
                );
                thread::sleep(Duration::from_millis(2));
            }
            Err("Some local computers could not stop.".into())
        });
        assert!(result.is_err());
        assert_eq!(
            waiter.unwrap().join().unwrap(),
            Err(operation_gate::GateError::Cancelled)
        );
        assert!(gate.is_idle());
    }

    #[test]
    fn quit_accepts_crashed_computer_and_still_stops_running_computer() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = setup(&dir);
        let runner = runner(None);
        runner
            .states
            .lock()
            .unwrap()
            .insert("first".into(), "Crashed".into());
        stop_local_computers_with(&runner, &paths, &|_, _, _| {}).unwrap();
        assert_eq!(runner.states.lock().unwrap()["first"], "Crashed");
        assert_eq!(runner.states.lock().unwrap()["second"], "Stopped");
        let calls = runner.calls.lock().unwrap();
        assert!(!calls
            .iter()
            .any(|args| args[0] == "stop" && args[1] == "first"));
        assert!(calls
            .iter()
            .any(|args| args[0] == "stop" && args[1] == "second"));
        assert_eq!(read_metadata(&paths.metadata).unwrap().computers.len(), 2);
    }

    #[test]
    fn quit_reports_each_computer_it_stops_with_its_position() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = setup(&dir);
        let runner = runner(None);
        runner
            .states
            .lock()
            .unwrap()
            .insert("first".into(), "Stopped".into());
        let seen = Mutex::new(Vec::new());
        stop_local_computers_with(&runner, &paths, &|name, index, total| {
            // Reported before the stop starts.
            assert_eq!(runner.states.lock().unwrap()[name], "Running");
            seen.lock().unwrap().push((name.to_owned(), index, total));
        })
        .unwrap();
        assert_eq!(
            seen.into_inner().unwrap(),
            vec![("second".to_owned(), 1, 1)]
        );
    }

    #[test]
    fn quit_skips_already_stopped_computers_without_recording_a_stop() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = setup(&dir);
        let runner = runner(None);
        runner
            .states
            .lock()
            .unwrap()
            .insert("first".into(), "Stopped".into());
        stop_local_computers_with(&runner, &paths, &|_, _, _| {}).unwrap();
        let calls = runner.calls.lock().unwrap();
        assert!(!calls
            .iter()
            .any(|args| args[0] == "stop" && args[1] == "first"));
        assert!(calls
            .iter()
            .any(|args| args[0] == "stop" && args[1] == "second"));
        let history = runtime_activity::read(&paths).unwrap();
        assert!(!history.iter().any(|event| event["computer"] == "first"));
        assert!(history.iter().any(|event| event["computer"] == "second"));
    }

    #[test]
    fn history_failure_does_not_block_graceful_quit() {
        let _test_state = crate::test_support::global_state();
        for malformed in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let paths = setup(&dir);
            let history = paths.metadata.with_file_name("computer-activity.json");
            if malformed {
                fs::write(&history, "{broken-json").unwrap();
            } else {
                fs::create_dir(&history).unwrap();
            }
            let runner = runner(None);
            stop_local_computers_with(&runner, &paths, &|_, _, _| {}).unwrap();
            assert!(runner
                .states
                .lock()
                .unwrap()
                .values()
                .all(|state| state == "Stopped"));
            assert!(!lifecycle_recovery::has_intent(&paths, &id("first")));
            assert!(!lifecycle_recovery::has_intent(&paths, &id("second")));
            assert!(runtime_activity::read(&paths)
                .unwrap()
                .iter()
                .any(|event| event["title"] == "Activity history unavailable"
                    && event["tone"] == "warning"));
        }
    }

    #[test]
    fn quit_stops_and_verifies_each_local_computer_without_removing_it() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = setup(&dir);
        let runner = runner(None);
        stop_local_computers_with(&runner, &paths, &|_, _, _| {}).unwrap();
        assert!(runner
            .states
            .lock()
            .unwrap()
            .values()
            .all(|state| state == "Stopped"));
        assert_eq!(read_metadata(&paths.metadata).unwrap().computers.len(), 2);
        assert_eq!(
            runner
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|args| args[0] == "stop")
                .count(),
            2
        );
    }
    #[test]
    fn quit_dispatches_all_stops_before_waiting_for_one_to_finish() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = setup(&dir);
        let mut runner = runner(None);
        runner.stop_barrier = Some(std::sync::Barrier::new(2));
        let (done, received) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let result = stop_local_computers_with(&runner, &paths, &|_, _, _| {});
            done.send((result, runtime_activity::read(&paths).unwrap()))
                .unwrap();
        });
        let (result, history) = received
            .recv_timeout(Duration::from_secs(5))
            .expect("both stop commands must enter before either finishes");
        worker.join().unwrap();
        result.unwrap();
        let first = history
            .iter()
            .find(|event| event["computer"] == "first")
            .expect("first stop activity is retained");
        let second = history
            .iter()
            .find(|event| event["computer"] == "second")
            .expect("second stop activity is retained");
        assert_eq!(first["status"], "completed");
        assert_eq!(second["status"], "completed");
        assert_ne!(first["id"], second["id"]);
    }

    #[test]
    fn one_failed_stop_preserves_failure_and_still_stops_other_computers() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = setup(&dir);
        let runner = runner(Some("first"));
        let error = stop_local_computers_with(&runner, &paths, &|_, _, _| {})
            .unwrap_err()
            .to_string();
        assert!(error.contains("first"));
        assert_eq!(runner.states.lock().unwrap()["first"], "Running");
        assert_eq!(runner.states.lock().unwrap()["second"], "Stopped");
    }
    #[test]
    fn replaced_computer_is_not_stopped_and_prevents_successful_quit() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = setup(&dir);
        let runner = runner(None);
        let mut metadata = read_metadata(&paths.metadata).unwrap();
        {
            let ComputerConfiguration { id, .. } = &mut metadata.computers[0];
            *id = "00000000-0000-4000-8000-000000000004".into();
        }
        write_metadata(&paths.metadata, &metadata).unwrap();
        assert!(stop_local_computers_with(&runner, &paths, &|_, _, _| {}).is_err());
        assert_eq!(runner.states.lock().unwrap()["first"], "Running");
        assert_eq!(runner.states.lock().unwrap()["second"], "Stopped");
    }

    #[test]
    fn quit_verifies_an_uncommitted_stop_after_a_command_timeout() {
        let _test_state = crate::test_support::global_state();
        struct Timeout {
            inner: Runner,
            stopped: bool,
        }
        impl RuntimeRunner for Timeout {
            fn run(
                &self,
                paths: &RuntimePaths,
                args: &[String],
                timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                if args[0] == "stop" {
                    if self.stopped {
                        self.inner
                            .states
                            .lock()
                            .unwrap()
                            .insert(args[1].clone(), "Stopped".into());
                    }
                    return Err(RuntimeError::TimedOut {
                        operation: "Stopping the computer".into(),
                    });
                }
                self.inner.run(paths, args, timeout)
            }
        }
        for stopped in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let paths = setup(&dir);
            let candidate = read_metadata(&paths.metadata).unwrap();
            fs::remove_file(&paths.metadata).unwrap();
            configuration_recovery::begin(&paths, &candidate).unwrap();
            let runner = Timeout {
                inner: runner(None),
                stopped,
            };
            let result = stop_local_computers_with(&runner, &paths, &|_, _, _| {});
            if stopped {
                result.unwrap();
            } else {
                assert!(
                    result.is_err(),
                    "a still-running computer must prevent Quit"
                );
            }
            assert_eq!(
                configuration_recovery::shutdown_computers(&paths).unwrap(),
                candidate.computers
            );
            assert!(runner
                .inner
                .states
                .lock()
                .unwrap()
                .values()
                .all(|state| state == if stopped { "Stopped" } else { "Running" }));
        }
    }

    #[test]
    fn quit_stops_the_present_identity_when_configuration_reuses_a_removed_computers_name() {
        let _test_state = crate::test_support::global_state();
        struct Replacement(Runner, String);
        impl RuntimeRunner for Replacement {
            fn run(
                &self,
                paths: &RuntimePaths,
                args: &[String],
                timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                let mut output = self.0.run(paths, args, timeout)?;
                if args[0] == "inspect" && args[1] == "first" {
                    let mut value: Value = serde_json::from_str(&output.stdout).unwrap();
                    value["config"]["labels"]["silo.machine-id"] = json!(self.1);
                    output.stdout = value.to_string();
                }
                Ok(output)
            }
        }
        for committed in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let paths = setup(&dir);
            let mut replacement = read_metadata(&paths.metadata).unwrap();
            let new_id = uuid::Uuid::new_v4().to_string();
            {
                let ComputerConfiguration { id, .. } = &mut replacement.computers[0];
                *id = new_id.clone();
            }
            configuration_recovery::begin(&paths, &replacement).unwrap();
            if committed {
                write_metadata(&paths.metadata, &replacement).unwrap();
            } else {
                let mut after_removal = replacement.clone();
                after_removal.computers.remove(0);
                write_metadata(&paths.metadata, &after_removal).unwrap();
            }
            let runner = Replacement(runner(None), new_id);
            stop_local_computers_with(&runner, &paths, &|_, _, _| {}).unwrap();
            assert!(runner
                .0
                .states
                .lock()
                .unwrap()
                .values()
                .all(|status| status == "Stopped"));
            assert!(configuration_recovery::pending_request(&paths)
                .unwrap()
                .is_some());
        }
    }

    #[test]
    fn quit_stops_created_computers_when_guest_verification_failed_before_metadata_commit() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = setup(&dir);
        let candidate = read_metadata(&paths.metadata).unwrap();
        fs::remove_file(&paths.metadata).unwrap();
        configuration_recovery::begin(&paths, &candidate).unwrap();
        let runner = runner(None);
        stop_local_computers_with(&runner, &paths, &|_, _, _| {}).unwrap();
        assert!(runner
            .states
            .lock()
            .unwrap()
            .values()
            .all(|state| state == "Stopped"));
        assert!(read_metadata(&paths.metadata).unwrap().computers.is_empty());
        assert_eq!(
            configuration_recovery::shutdown_computers(&paths).unwrap(),
            candidate.computers
        );
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| matches!(args[0].as_str(), "start" | "create" | "remove" | "exec")));
    }

    #[test]
    fn quit_does_not_silently_leave_an_unidentified_managed_computer_running() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = setup(&dir);
        let mut metadata = read_metadata(&paths.metadata).unwrap();
        metadata.computers.truncate(1);
        write_metadata(&paths.metadata, &metadata).unwrap();
        let runner = runner(None);
        let error = stop_local_computers_with(&runner, &paths, &|_, _, _| {})
            .unwrap_err()
            .to_string();
        assert!(error.contains("second"));
        assert_eq!(runner.states.lock().unwrap()["first"], "Stopped");
        assert_eq!(runner.states.lock().unwrap()["second"], "Running");
    }

    #[test]
    fn missing_metadata_does_not_hide_managed_computers_in_an_existing_runtime() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&dir);
        fs::create_dir_all(&paths.home).unwrap();
        let runner = runner(None);
        let error = stop_local_computers_with(&runner, &paths, &|_, _, _| {})
            .unwrap_err()
            .to_string();
        assert!(error.contains("matching saved identity"));
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args[0] == "stop"));
    }

    #[test]
    fn quit_after_failed_first_setup_does_not_require_a_working_alias() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let mut paths = setup(&dir);
        let pending = read_metadata(&paths.metadata).unwrap();
        fs::remove_file(&paths.metadata).unwrap();
        configuration_recovery::begin(&paths, &pending).unwrap();
        paths.storage_home = Some(dir.path().join("storage"));
        fs::create_dir(&paths.home).unwrap();
        fs::write(paths.home.join(".silo-configuration-worker.lock"), b"").unwrap();
        let runner = runner(None);
        stop_local_computers_with(&runner, &paths, &|_, _, _| {}).unwrap();
        assert!(runner.calls.lock().unwrap().is_empty());
        assert_eq!(
            configuration_recovery::shutdown_computers(&paths).unwrap(),
            pending.computers
        );
    }

    #[test]
    fn bootstrap_shortcut_rejects_runtime_state_in_either_location() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let mut paths = super::super::tests::paths(&dir);
        let storage = dir.path().join("storage");
        paths.storage_home = Some(storage.clone());
        for location in [&paths.home, &storage] {
            fs::create_dir_all(location).unwrap();
            let state = location.join("run");
            fs::create_dir(&state).unwrap();
            assert!(!runtime_never_initialized(&paths));
            fs::remove_dir(state).unwrap();
        }
        assert!(runtime_never_initialized(&paths));
    }

    #[test]
    fn bootstrap_shortcut_rejects_active_workers_and_symlinks() {
        let _test_state = crate::test_support::global_state();
        use std::os::fd::AsRawFd;
        let dir = tempfile::tempdir().unwrap();
        let mut paths = super::super::tests::paths(&dir);
        paths.storage_home = Some(dir.path().join("storage"));
        fs::create_dir(&paths.home).unwrap();
        let lock_path = paths.home.join(".silo-configuration-worker.lock");
        let file = File::create(&lock_path).unwrap();
        assert_eq!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        assert!(!runtime_never_initialized(&paths));
        drop(file);
        assert!(runtime_never_initialized(&paths));
        fs::remove_file(&lock_path).unwrap();
        std::os::unix::fs::symlink(dir.path().join("elsewhere"), &lock_path).unwrap();
        assert!(!runtime_never_initialized(&paths));
        fs::remove_file(lock_path).unwrap();
        fs::remove_dir(&paths.home).unwrap();
        std::os::unix::fs::symlink(dir.path().join("elsewhere"), &paths.home).unwrap();
        assert!(!runtime_never_initialized(&paths));
    }

    #[test]
    fn empty_configuration_quits_without_needing_runtime() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&dir);
        let runner = runner(None);
        stop_local_computers_with(&runner, &paths, &|_, _, _| {}).unwrap();
        assert!(runner.calls.lock().unwrap().is_empty());
    }
}
