//! Owner-side record of changes requested by other devices.
//!
//! A controller names each change with an `operationId` that stays the same when it
//! retries after losing the connection. Acceptance is recorded under a short lock; the
//! change then waits for its turn in the operation gate like local work, so changes to
//! different computers run concurrently. A change that has not started before its deadline, or
//! whose controller disconnected, never starts. A retry of a known change attaches to it
//! and receives its result instead of running it again. Results stay in memory for an
//! hour; a small marker per change on disk keeps a restart from replaying it.
use crate::bridge_error::BridgeError;
use crate::runtime::operation_gate::StartCondition;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::Path,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::{Duration, Instant, SystemTime},
};

/// How long finished results and on-disk markers are kept.
pub(super) const RETENTION: Duration = Duration::from_secs(60 * 60);
/// A queued change survives a dropped connection this long, so a retry can attach.
pub(super) const RECONNECT_GRACE: Duration = Duration::from_secs(10);
/// Finished results kept at most, oldest dropped first.
const FINISHED_LIMIT: usize = 256;
/// How often the on-disk markers are pruned.
const PRUNE_INTERVAL: Duration = Duration::from_secs(60);
/// Allows older records containing full request and result frames.
const MAX_MARKER_BYTES: u64 = 16 * 1024 * 1024;

pub(super) const EXPIRED: &str = "This change did not start on the other device before the request expired, so nothing changed. Try again.";
pub(super) const REUSED: &str = "Remote request identity was reused for a different operation.";
const UNCERTAIN: &str = "This operation was already accepted. Its result is uncertain; refresh the computer state before making another change.";
const ALREADY_FINISHED: &str =
    "This change already finished on the other device. Refresh to see its result.";
const STILL_RUNNING: &str =
    "This change is still running on the other device. Refresh to see its result.";

/// Whether something is still true right now, such as a connection being open.
pub(super) type Probe = Arc<dyn Fn() -> bool + Send + Sync>;

enum Status {
    Pending,
    Done(Result<Value, BridgeError>),
    Expired,
}

struct Operation {
    fingerprint: Value,
    status: Status,
    /// The change must start before this instant.
    deadline: Instant,
    /// Connections waiting for this change's result.
    connections: Vec<(u64, Probe)>,
    /// When a waiting connection was last seen open.
    connected_at: Instant,
    reconnect_grace: Duration,
    finished_at: Option<Instant>,
}

#[derive(Default)]
struct State {
    operations: BTreeMap<String, Operation>,
    next_connection: u64,
    pruned_at: Option<Instant>,
}

pub(super) struct Registry {
    state: Mutex<State>,
    finished: Condvar,
}

/// What one request carries besides its change.
pub(super) struct Submission<'a> {
    /// `~/.silo/desktop-remote/operations`: one small marker per accepted change.
    pub journal: &'a Path,
    pub id: &'a str,
    pub method: &'a str,
    pub params: &'a Value,
    /// The change must start within this long of acceptance.
    pub start_within: Duration,
    /// True while this request's connection is open.
    pub connection: Probe,
    /// True while this device still accepts remote changes.
    pub allowed: Probe,
    /// How long a retry waits for a change that is already running.
    pub wait: Duration,
    /// How long a queued change waits for a dropped connection to come back.
    pub reconnect_grace: Duration,
}

enum Role {
    Run,
    Attach(u64),
}

impl Registry {
    pub(super) const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                operations: BTreeMap::new(),
                next_connection: 0,
                pruned_at: None,
            }),
            finished: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Plain maps; a panic elsewhere cannot leave them half-updated.
        crate::sync::lock_or_recover(&self.state, "remote changes")
    }

    /// Accept, run once, or attach to one change named by `submission.id`.
    pub(super) fn submit(
        &'static self,
        submission: Submission<'_>,
        execute: impl FnOnce() -> Result<Value, BridgeError>,
    ) -> Result<Value, BridgeError> {
        let fingerprint = json!({"method": submission.method, "params": submission.params});
        let role = self.accept(&submission, fingerprint)?;
        match role {
            Role::Attach(connection) => self.attach(submission.id, connection, &submission),
            Role::Run => self.run(&submission, execute),
        }
    }

    /// The short acceptance step: record the change, or find the existing one.
    fn accept(&self, submission: &Submission<'_>, fingerprint: Value) -> Result<Role, BridgeError> {
        let now = Instant::now();
        let mut state = self.lock();
        state.prune(now, submission.journal);
        state.next_connection += 1;
        let connection = state.next_connection;
        if let Some(operation) = state.operations.get_mut(submission.id) {
            if operation.fingerprint != fingerprint {
                return Err(REUSED.into());
            }
            operation
                .connections
                .push((connection, submission.connection.clone()));
            operation.connected_at = now;
            return Ok(Role::Attach(connection));
        }
        let marker = submission.journal.join(format!("{}.json", submission.id));
        if let Some(previous) = read_marker(&marker) {
            return Err(match previous.as_str() {
                "finished" => ALREADY_FINISHED,
                "expired" => EXPIRED,
                _ => UNCERTAIN,
            }
            .into());
        }
        write_marker(submission.journal, &marker, submission.method, "accepted")?;
        state.operations.insert(
            submission.id.to_owned(),
            Operation {
                fingerprint,
                status: Status::Pending,
                deadline: now + submission.start_within,
                connections: vec![(connection, submission.connection.clone())],
                connected_at: now,
                reconnect_grace: submission.reconnect_grace,
                finished_at: None,
            },
        );
        Ok(Role::Run)
    }

    /// True while the change may still start: before its deadline, with access still
    /// allowed and a connection open (or recently open, so a retry can attach).
    fn wanted(&self, id: &str, allowed: &Probe) -> bool {
        if !allowed() {
            return false;
        }
        let mut state = self.lock();
        let Some(operation) = state.operations.get_mut(id) else {
            return false;
        };
        operation.connections.retain(|(_, open)| open());
        if !operation.connections.is_empty() {
            operation.connected_at = Instant::now();
        }
        let (deadline, connected_at, reconnect_grace) = (
            operation.deadline,
            operation.connected_at,
            operation.reconnect_grace,
        );
        drop(state);
        if !allowed() {
            return false;
        }
        let now = Instant::now();
        now < deadline && now.duration_since(connected_at) < reconnect_grace
    }

    fn run(
        &'static self,
        submission: &Submission<'_>,
        execute: impl FnOnce() -> Result<Value, BridgeError>,
    ) -> Result<Value, BridgeError> {
        let id = submission.id.to_owned();
        let allowed = submission.allowed.clone();
        let marker = submission.journal.join(format!("{id}.json"));
        if !self.wanted(&id, &allowed) {
            self.finish(submission, &marker, Status::Expired);
            return Err(EXPIRED.into());
        }
        let condition = StartCondition::new({
            let id = id.clone();
            move || self.wanted(&id, &allowed)
        });
        let result = StartCondition::scope(Some(condition.clone()), execute);
        if condition.expired() && !condition.started() {
            self.finish(submission, &marker, Status::Expired);
            return Err(EXPIRED.into());
        }
        self.finish(submission, &marker, Status::Done(result.clone()));
        result
    }

    fn finish(&self, submission: &Submission<'_>, marker: &Path, status: Status) {
        let label = if matches!(status, Status::Expired) {
            "expired"
        } else {
            "finished"
        };
        let mut state = self.lock();
        if let Some(operation) = state.operations.get_mut(submission.id) {
            operation.status = status;
            operation.finished_at = Some(Instant::now());
            operation.connections.clear();
        }
        if let Err(error) = write_marker(submission.journal, marker, submission.method, label) {
            eprintln!("Could not record a remote change result: {error}");
        }
        drop(state);
        self.finished.notify_all();
    }

    /// A retry waits for the change it names, while its own connection stays open.
    fn attach(
        &self,
        id: &str,
        connection: u64,
        submission: &Submission<'_>,
    ) -> Result<Value, BridgeError> {
        let until = Instant::now() + submission.wait;
        let mut state = self.lock();
        let outcome = loop {
            let Some(operation) = state.operations.get(id) else {
                break Err(BridgeError::from(UNCERTAIN));
            };
            match &operation.status {
                Status::Done(result) => break result.clone(),
                Status::Expired => break Err(BridgeError::from(EXPIRED)),
                Status::Pending => {}
            }
            if Instant::now() >= until || !(submission.connection)() {
                break Err(BridgeError::from(STILL_RUNNING));
            }
            state = self
                .finished
                .wait_timeout(state, Duration::from_millis(250))
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        };
        if let Some(operation) = state.operations.get_mut(id) {
            operation
                .connections
                .retain(|(other, _)| *other != connection);
        }
        outcome
    }
}

impl State {
    /// Drop results older than the retention, then the oldest past the limit; prune markers.
    fn prune(&mut self, now: Instant, journal: &Path) {
        self.operations.retain(|_, operation| {
            operation
                .finished_at
                .is_none_or(|finished| now.duration_since(finished) < RETENTION)
        });
        let mut finished: Vec<(Instant, String)> = self
            .operations
            .iter()
            .filter_map(|(id, operation)| operation.finished_at.map(|at| (at, id.clone())))
            .collect();
        if finished.len() > FINISHED_LIMIT {
            finished.sort();
            for (_, id) in &finished[..finished.len() - FINISHED_LIMIT] {
                self.operations.remove(id);
            }
        }
        if self
            .pruned_at
            .is_none_or(|at| now.duration_since(at) >= PRUNE_INTERVAL)
        {
            self.pruned_at = Some(now);
            prune_markers(journal, SystemTime::now());
        }
    }
}

/// Removes markers (and records written by earlier versions) older than the retention.
fn prune_markers(journal: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(journal) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= RETENTION);
        if old && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// The status a marker records: "accepted", "finished" or "expired". Records written by
/// earlier versions hold the full request, and a result once finished.
fn read_marker(path: &Path) -> Option<String> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => return Some("accepted".into()),
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Some("accepted".into());
    }
    let mut bytes = Vec::new();
    if file
        .take(MAX_MARKER_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_MARKER_BYTES
    {
        return Some("accepted".into());
    }
    let record: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Some(match record["status"].as_str() {
        Some(status) => status.to_owned(),
        None if record.get("result").is_some() => "finished".into(),
        None => "accepted".into(),
    })
}

/// Writes a marker without the request's parameters or result.
fn write_marker(journal: &Path, path: &Path, method: &str, status: &str) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new_in(journal).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, &json!({"method": method, "status": status}))
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    fs::File::open(journal)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::operation_gate::OperationGate;

    #[test]
    fn fifo_marker_is_uncertain_without_waiting_for_a_writer() {
        const CHILD_PATH: &str = "SILO_TEST_REMOTE_FIFO_MARKER";
        if let Some(path) = std::env::var_os(CHILD_PATH) {
            assert_eq!(read_marker(Path::new(&path)).as_deref(), Some("accepted"));
            return;
        }
        use std::os::unix::ffi::OsStrExt;
        use std::process::{Command, Stdio};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("marker.json");
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let module = module_path!().split_once("::").unwrap().1;
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("{module}::fifo_marker_is_uncertain_without_waiting_for_a_writer"),
            ])
            .env(CHILD_PATH, &path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if Instant::now() >= deadline {
                // The owned fixture cannot finish while the pre-fix reader waits for a writer.
                assert_eq!(
                    unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) },
                    0
                );
                child.wait().unwrap();
                break None;
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert!(
            status.is_some_and(|status| status.success()),
            "marker inspection blocked on a FIFO"
        );
    }

    #[test]
    fn marker_symlinks_are_uncertain_without_following_the_target() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("external.json");
        let bytes = br#"{"status":"finished"}"#;
        fs::write(&target, bytes).unwrap();
        let link = directory.path().join("marker.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(read_marker(&link).as_deref(), Some("accepted"));
        assert_eq!(fs::read(&target).unwrap(), bytes);
        fs::remove_file(&target).unwrap();
        assert_eq!(read_marker(&link).as_deref(), Some("accepted"));
    }

    #[test]
    fn an_existing_dangling_marker_never_replays_the_change() {
        let fixture = Fixture::new();
        let marker = fixture.journal.path().join(format!("{}.json", fixture.id));
        std::os::unix::fs::symlink(fixture.journal.path().join("missing"), &marker).unwrap();
        let executions = AtomicUsize::new(0);
        let result = registry().submit(fixture.submission(always(), always()), || {
            executions.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        });
        assert!(result.is_err());
        assert_eq!(executions.load(Ordering::SeqCst), 0);
        assert!(fs::symlink_metadata(marker)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn marker_limit_preserves_legacy_records_and_treats_oversized_records_as_uncertain() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("marker.json");
        assert_eq!(read_marker(&path), None);
        let mut bytes = br#"{"request":{"params":{}},"result":{"Ok":{}}}"#.to_vec();
        bytes.resize(16 * 1024 * 1024, b' ');
        fs::write(&path, &bytes).unwrap();
        assert_eq!(read_marker(&path).as_deref(), Some("finished"));
        bytes.push(b' ');
        fs::write(&path, &bytes).unwrap();
        assert_eq!(read_marker(&path).as_deref(), Some("accepted"));
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread;

    // Registry tests own their admission state: a process-wide device operation
    // must not split the two-computer barrier or cancel an unrelated queued change.
    fn gate() -> &'static OperationGate {
        Box::leak(Box::new(OperationGate::new()))
    }

    fn registry() -> &'static Registry {
        Box::leak(Box::new(Registry::new()))
    }
    fn flag(value: bool) -> (Arc<AtomicBool>, Probe) {
        let flag = Arc::new(AtomicBool::new(value));
        let probe = flag.clone();
        (flag, Arc::new(move || probe.load(Ordering::SeqCst)))
    }
    fn always() -> Probe {
        Arc::new(|| true)
    }
    struct Fixture {
        journal: tempfile::TempDir,
        id: String,
        params: Value,
    }
    impl Fixture {
        fn new() -> Self {
            Self {
                journal: tempfile::tempdir().unwrap(),
                id: uuid::Uuid::new_v4().to_string(),
                params: json!({"computerId": uuid::Uuid::new_v4().to_string()}),
            }
        }
        fn computer(&self) -> String {
            self.params["computerId"].as_str().unwrap().to_owned()
        }
        fn submission(&self, connection: Probe, allowed: Probe) -> Submission<'_> {
            Submission {
                journal: self.journal.path(),
                id: &self.id,
                method: "runtime.action",
                params: &self.params,
                start_within: Duration::from_secs(60),
                connection,
                allowed,
                wait: Duration::from_secs(10),
                reconnect_grace: Duration::from_millis(300),
            }
        }
    }
    fn wait_for(predicate: impl Fn() -> bool) {
        let until = Instant::now() + Duration::from_secs(5);
        while !predicate() {
            assert!(Instant::now() < until, "condition not reached");
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn waiting_for(gate: &OperationGate, computer: &str) -> bool {
        gate.snapshot()
            .waiting
            .iter()
            .any(|entry| entry.computer_id.as_deref() == Some(computer))
    }
    /// Runs like a remote change: takes the computer's gate on this thread, then works.
    fn change_on(
        gate: &'static OperationGate,
        computer: String,
        runs: &'static AtomicUsize,
    ) -> impl FnOnce() -> Result<Value, BridgeError> {
        move || {
            let _guard = gate
                .computer(&computer, "computer", "Remote change")
                .map_err(|e| e.to_string())?;
            runs.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"computer": computer}))
        }
    }
    fn counter() -> &'static AtomicUsize {
        Box::leak(Box::new(AtomicUsize::new(0)))
    }

    #[test]
    fn changes_to_two_computers_run_concurrently() {
        let gate = gate();
        let registry = registry();
        let (a, b) = (Fixture::new(), Fixture::new());
        let entered = Arc::new(std::sync::Barrier::new(2));
        let result = thread::scope(|scope| {
            let runs = [&a, &b].map(|fixture| {
                let entered = entered.clone();
                scope.spawn(move || {
                    registry.submit(fixture.submission(always(), always()), || {
                        let _guard = gate
                            .computer(&fixture.computer(), "computer", "Remote change")
                            .map_err(|e| e.to_string())?;
                        // Both changes hold their computer's turn at once, or this never returns.
                        entered.wait();
                        Ok(json!(fixture.computer()))
                    })
                })
            });
            runs.map(|run| run.join().unwrap())
        });
        assert_eq!(result, [Ok(json!(a.computer())), Ok(json!(b.computer()))]);
    }

    #[test]
    fn changes_to_the_same_computer_run_in_turn() {
        let gate = gate();
        let registry = registry();
        let first = Fixture::new();
        let mut second = Fixture::new();
        second.params = first.params.clone();
        let (release, hold) = std::sync::mpsc::channel::<()>();
        let order = Arc::new(Mutex::new(Vec::new()));
        let computer = first.computer();
        thread::scope(|scope| {
            let (first, second, computer) = (&first, &second, &computer);
            let order_first = order.clone();
            let running = scope.spawn(move || {
                registry.submit(first.submission(always(), always()), move || {
                    let _guard = gate
                        .computer(computer, "computer", "First")
                        .map_err(|e| e.to_string())?;
                    order_first.lock().unwrap().push("first started");
                    hold.recv().unwrap();
                    order_first.lock().unwrap().push("first finished");
                    Ok(Value::Null)
                })
            });
            wait_for(|| order.lock().unwrap().len() == 1);
            let order_second = order.clone();
            let queued = scope.spawn(move || {
                registry.submit(second.submission(always(), always()), move || {
                    let _guard = gate
                        .computer(computer, "computer", "Second")
                        .map_err(|e| e.to_string())?;
                    order_second.lock().unwrap().push("second started");
                    Ok(Value::Null)
                })
            });
            wait_for(|| waiting_for(gate, computer));
            release.send(()).unwrap();
            running.join().unwrap().unwrap();
            queued.join().unwrap().unwrap();
        });
        assert_eq!(
            *order.lock().unwrap(),
            ["first started", "first finished", "second started"]
        );
    }

    #[test]
    fn access_revoked_during_the_connection_check_prevents_the_change() {
        let gate = gate();
        let registry = registry();
        let fixture = Fixture::new();
        let (enabled, allowed) = flag(true);
        let checked = AtomicUsize::new(0);
        let connection = Arc::new(move || {
            if checked.fetch_add(1, Ordering::SeqCst) == 1 {
                enabled.store(false, Ordering::SeqCst);
            }
            true
        });
        let runs = counter();

        let result = registry.submit(
            fixture.submission(connection, allowed),
            change_on(gate, fixture.computer(), runs),
        );

        assert_eq!(result, Err(EXPIRED.into()));
        assert_eq!(runs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_connection_check_that_outlasts_the_deadline_never_starts_the_change() {
        let gate = gate();
        let registry = registry();
        let fixture = Fixture::new();
        let checked = Arc::new(AtomicUsize::new(0));
        let deadline = Arc::new(std::sync::OnceLock::<Instant>::new());
        let connection = {
            let checked = checked.clone();
            let deadline = deadline.clone();
            Arc::new(move || {
                if checked.fetch_add(1, Ordering::SeqCst) == 1 {
                    // Pause the admission check until the accepted request has expired.
                    let deadline = *deadline.get().unwrap();
                    thread::sleep(deadline.saturating_duration_since(Instant::now()));
                }
                true
            }) as Probe
        };
        let submission = fixture.submission(connection, always());
        registry
            .accept(
                &submission,
                json!({"method": submission.method, "params": submission.params}),
            )
            .unwrap();
        let cutoff = Instant::now() + Duration::from_millis(300);
        registry
            .lock()
            .operations
            .get_mut(&fixture.id)
            .unwrap()
            .deadline = cutoff;
        deadline.set(cutoff).unwrap();
        let runs = counter();

        let result = registry.run(&submission, change_on(gate, fixture.computer(), runs));

        assert_eq!(result, Err(EXPIRED.into()));
        assert_eq!(runs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_queued_change_whose_deadline_passes_never_runs() {
        let gate = gate();
        let registry = registry();
        let fixture = Fixture::new();
        let busy = {
            let computer = fixture.computer();
            let (held, release) = (std::sync::mpsc::channel(), std::sync::mpsc::channel::<()>());
            let thread = thread::spawn(move || {
                let guard = gate
                    .computer(&computer, "computer", "Long local work")
                    .unwrap();
                held.0.send(()).unwrap();
                release.1.recv().unwrap();
                drop(guard);
            });
            held.1.recv().unwrap();
            (thread, release.0)
        };
        let runs = counter();
        let mut submission = fixture.submission(always(), always());
        submission.start_within = Duration::from_millis(300);
        let result = registry.submit(submission, change_on(gate, fixture.computer(), runs));
        assert_eq!(result, Err(EXPIRED.into()));
        assert!(!waiting_for(gate, &fixture.computer()));
        busy.1.send(()).unwrap();
        busy.0.join().unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 0);
        // A retry learns the change expired instead of running it.
        assert_eq!(
            registry.submit(
                fixture.submission(always(), always()),
                change_on(gate, fixture.computer(), runs)
            ),
            Err(EXPIRED.into())
        );
        assert_eq!(runs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_queued_change_stops_when_its_connection_closes_or_access_is_revoked() {
        let gate = gate();
        for revoke_access in [false, true] {
            let registry = registry();
            let fixture = Fixture::new();
            let (held, release) = (std::sync::mpsc::channel(), std::sync::mpsc::channel::<()>());
            let busy = {
                let computer = fixture.computer();
                thread::spawn(move || {
                    let guard = gate
                        .computer(&computer, "computer", "Long local work")
                        .unwrap();
                    held.0.send(()).unwrap();
                    release.1.recv().unwrap();
                    drop(guard);
                })
            };
            held.1.recv().unwrap();
            let (open, connection) = flag(true);
            let (enabled, allowed) = flag(true);
            let runs = counter();
            let started = Instant::now();
            let result = thread::scope(|scope| {
                let queued = scope.spawn(|| {
                    registry.submit(
                        fixture.submission(connection, allowed),
                        change_on(gate, fixture.computer(), runs),
                    )
                });
                wait_for(|| waiting_for(gate, &fixture.computer()));
                if revoke_access {
                    enabled.store(false, Ordering::SeqCst)
                } else {
                    open.store(false, Ordering::SeqCst)
                }
                queued.join().unwrap()
            });
            assert_eq!(result, Err(EXPIRED.into()));
            if revoke_access {
                assert!(
                    started.elapsed() < Duration::from_millis(300),
                    "revoked access stops a change at once"
                );
            } else {
                assert!(
                    started.elapsed() >= Duration::from_millis(300),
                    "a dropped connection has time to reconnect"
                );
            }
            release.0.send(()).unwrap();
            busy.join().unwrap();
            assert_eq!(runs.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn a_retry_attaches_to_the_queued_change_and_receives_its_result() {
        let gate = gate();
        let registry = registry();
        let fixture = Fixture::new();
        let (held, release) = (std::sync::mpsc::channel(), std::sync::mpsc::channel::<()>());
        let busy = {
            let computer = fixture.computer();
            thread::spawn(move || {
                let guard = gate
                    .computer(&computer, "computer", "Long local work")
                    .unwrap();
                held.0.send(()).unwrap();
                release.1.recv().unwrap();
                drop(guard);
            })
        };
        held.1.recv().unwrap();
        let runs = counter();
        let (first_open, first_connection) = flag(true);
        let (first, retry) = thread::scope(|scope| {
            let first = scope.spawn(|| {
                registry.submit(
                    fixture.submission(first_connection, always()),
                    change_on(gate, fixture.computer(), runs),
                )
            });
            wait_for(|| waiting_for(gate, &fixture.computer()));
            // The first connection is lost; the controller retries with the same identity.
            first_open.store(false, Ordering::SeqCst);
            let retry = scope.spawn(|| {
                registry.submit(fixture.submission(always(), always()), || {
                    panic!("a retry must not run the change again")
                })
            });
            thread::sleep(Duration::from_millis(100));
            release.0.send(()).unwrap();
            (first.join().unwrap(), retry.join().unwrap())
        });
        busy.join().unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(first, Ok(json!({"computer": fixture.computer()})));
        assert_eq!(retry, first);
        // A later retry is answered from the kept result.
        assert_eq!(
            registry.submit(fixture.submission(always(), always()), || panic!(
                "must not replay"
            )),
            first
        );
    }

    #[test]
    fn identities_are_bound_to_one_change_and_survive_a_restart_without_replaying() {
        let fixture = Fixture::new();
        let registry = registry();
        registry
            .submit(fixture.submission(always(), always()), || Ok(json!(1)))
            .unwrap();
        let mut other = fixture.submission(always(), always());
        let changed = json!({"computerId": "other"});
        other.params = &changed;
        assert_eq!(
            registry.submit(other, || panic!("must not run")),
            Err(REUSED.into())
        );
        // After a restart only the marker is left: the change is not run again.
        let restarted = self::registry();
        assert_eq!(
            restarted.submit(fixture.submission(always(), always()), || panic!(
                "must not replay"
            )),
            Err(ALREADY_FINISHED.into())
        );
        let interrupted = Fixture::new();
        write_marker(
            interrupted.journal.path(),
            &interrupted
                .journal
                .path()
                .join(format!("{}.json", interrupted.id)),
            "runtime.action",
            "accepted",
        )
        .unwrap();
        assert_eq!(
            restarted.submit(interrupted.submission(always(), always()), || panic!(
                "must not replay"
            )),
            Err(UNCERTAIN.into())
        );
        // Markers hold no parameters or results.
        let marker: Value = serde_json::from_slice(
            &fs::read(fixture.journal.path().join(format!("{}.json", fixture.id))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            marker,
            json!({"method": "runtime.action", "status": "finished"})
        );
    }

    #[test]
    fn records_older_than_the_retention_are_pruned() {
        let journal = tempfile::tempdir().unwrap();
        let old = journal.path().join("old.json");
        let fresh = journal.path().join("fresh.json");
        fs::write(
            &old,
            br#"{"request":{"params":{"secret":"x"}},"result":{"Ok":{}}}"#,
        )
        .unwrap();
        fs::write(&fresh, b"{}").unwrap();
        prune_markers(
            journal.path(),
            SystemTime::now() + RETENTION + Duration::from_secs(1),
        );
        assert!(!old.exists() && !fresh.exists());
        fs::write(&old, b"{}").unwrap();
        prune_markers(journal.path(), SystemTime::now());
        assert!(old.exists());

        let mut state = State::default();
        let now = Instant::now();
        for index in 0..FINISHED_LIMIT + 2 {
            state.operations.insert(
                index.to_string(),
                Operation {
                    fingerprint: Value::Null,
                    status: Status::Done(Ok(Value::Null)),
                    deadline: now,
                    connections: vec![],
                    connected_at: now,
                    reconnect_grace: RECONNECT_GRACE,
                    finished_at: Some(now + Duration::from_millis(index as u64)),
                },
            );
        }
        state.operations.insert(
            "pending".into(),
            Operation {
                fingerprint: Value::Null,
                status: Status::Pending,
                deadline: now,
                connections: vec![],
                connected_at: now,
                reconnect_grace: RECONNECT_GRACE,
                finished_at: None,
            },
        );
        state.prune(now + Duration::from_secs(1), journal.path());
        assert_eq!(state.operations.len(), FINISHED_LIMIT + 1);
        assert!(!state.operations.contains_key("0") && !state.operations.contains_key("1"));
        state.prune(now + RETENTION + Duration::from_secs(1), journal.path());
        assert_eq!(state.operations.keys().collect::<Vec<_>>(), ["pending"]);
    }
}
