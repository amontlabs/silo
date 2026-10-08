//! Ordered admission for operations that change computer runtime state on this device.
//!
//! Callers wait their turn instead of failing with "busy". Work on different computers
//! runs concurrently; device-wide work waits for everything else. Admission is
//! first-come, first-served among conflicting requests, so a waiting device-wide
//! operation is never starved by a stream of per-computer operations.
//!
//! This is the sole in-process gate for computer-changing work. It does not replace the
//! OS file lock that coordinates cooperating runtime processes, nor short data locks.
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    /// Changes shared state: computer inventory, device networking, runtime generation, updates.
    Device,
    /// Changes one computer's runtime or guest state only. Keyed by the stable computer id so a
    /// rename never lets two operations on the same computer run concurrently, and so
    /// id-addressed work (checkpoints, remote lifecycle) shares the same lane as the
    /// name-addressed lifecycle commands.
    Computer { id: String },
}

impl Scope {
    fn conflicts(&self, other: &Scope) -> bool {
        match (self, other) {
            (Scope::Computer { id: a }, Scope::Computer { id: b }) => a == b,
            _ => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GateError {
    /// An identical request is already waiting; the caller should not run it twice.
    AlreadyQueued,
    /// The current thread already holds an operation; waiting would deadlock.
    Nested,
    /// A `try_` request found conflicting work.
    Busy,
    /// The caller stopped waiting (for example, the user cancelled) before its turn.
    Abandoned,
    /// The operation was cancelled by the user (while waiting or, if cancellable, while running).
    Cancelled,
    /// A running operation was asked to cancel but is not marked cancellable.
    NotCancellable,
}

impl std::fmt::Display for GateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::AlreadyQueued => "This action is already waiting to run.",
            Self::Nested => "Computer operation ordering failed.",
            Self::Busy => "Another computer operation is still running.",
            Self::Abandoned => "The operation stopped waiting for its turn.",
            Self::Cancelled => "The operation was cancelled.",
            Self::NotCancellable => "This operation can't be cancelled.",
        })
    }
}

impl std::error::Error for GateError {}

/// What an operation is, so observers never parse its display label.
#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    /// Start, stop, restart, or dismiss-error on one computer, or a device-wide start.
    Lifecycle,
    CheckpointCapture,
    CheckpointRestore,
    CheckpointFork,
    CheckpointDelete,
    Export,
    Import,
    StorageReclaim,
    GithubApply,
    Push,
    PortPublish,
    PortRemove,
    /// Stopping local computers for quit or update.
    Shutdown,
    /// Adding, changing, deleting, or reordering computers; progress arrives as setup events.
    ComputerConfiguration,
    #[default]
    Other,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationEntry {
    pub id: u64,
    pub label: String,
    pub kind: OperationKind,
    /// Stable computer id this operation is scoped to; `None` for device-wide operations.
    pub computer_id: Option<String>,
    /// Computer display name captured when the operation was admitted; `None` for
    /// device-wide operations. For display only — ordering keys on `computer_id`.
    pub computer_name: Option<String>,
    /// Milliseconds since the Unix epoch when the operation started running or began waiting.
    pub since_ms: u64,
    /// Whether the user may cancel this operation now. Waiting entries are always
    /// cancellable; a running entry is cancellable only when its work opted in.
    pub cancellable: bool,
    /// Expected maximum duration in milliseconds, used to flag slow operations.
    /// `None` when the operation carried no expectation.
    pub expected_ms: Option<u64>,
    /// True for a waiting entry whose turn is currently held up by internal background
    /// maintenance that is itself hidden from this queue. Lets the UI explain the wait
    /// without naming an operation the user never started. Always false for running entries.
    #[serde(default)]
    pub blocked_by_hidden: bool,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationQueue {
    pub running: Vec<OperationEntry>,
    /// In admission order.
    pub waiting: Vec<OperationEntry>,
}

struct Entry {
    id: u64,
    scope: Scope,
    /// Computer display name for per-computer entries; `None` for device-wide entries.
    computer_name: Option<String>,
    label: String,
    kind: OperationKind,
    key: Option<String>,
    since: Instant,
    since_ms: u64,
    /// When the entry was admitted to run (or queued, while waiting). Unlike `since`,
    /// never carried over from an earlier attempt: it bounds the cancel grace (D-32).
    admitted: Instant,
    /// True while an operation runs longer than its owner opted to allow cancelling.
    /// Waiting entries report `true` regardless; a running entry reports this flag.
    cancellable: bool,
    /// Set true when a cancel is requested: for a waiting entry it makes the waiter
    /// leave the queue with `Cancelled`; for a cancellable running entry the working
    /// code observes it and stops. Shared with the guard so other threads can read it.
    cancel: Arc<AtomicBool>,
    /// Expected maximum duration, for stuck-operation flagging in the UI.
    expected: Option<Duration>,
    /// Internal background housekeeping that must not surface in the published queue
    /// snapshot. It still holds the gate for mutual exclusion; only its visibility differs.
    hidden: bool,
    /// Ids of the computers this device-wide operation deletes, so work running on one
    /// of them can yield to it (see `removal_queued`). Empty for everything else.
    removes: Vec<String>,
}

impl Entry {
    fn public(&self, running: bool) -> OperationEntry {
        OperationEntry {
            id: self.id,
            label: self.label.clone(),
            kind: self.kind,
            computer_id: match &self.scope {
                Scope::Device => None,
                Scope::Computer { id } => Some(id.clone()),
            },
            computer_name: self.computer_name.clone(),
            since_ms: self.since_ms,
            // Waiting entries can always be cancelled; running entries only when opted in.
            cancellable: if running { self.cancellable } else { true },
            expected_ms: self.expected.map(|value| value.as_millis() as u64),
            // Set per waiting entry in `snapshot`, where the full queue is visible.
            blocked_by_hidden: false,
        }
    }
}

#[derive(Default)]
struct State {
    next_id: u64,
    running: Vec<Entry>,
    waiting: VecDeque<Entry>,
    /// Bumped whenever a visible device-wide entry is queued, starts, or finishes.
    device_generation: u64,
    /// Same, for entries scoped to one computer. Absent means never touched.
    computer_generations: BTreeMap<String, u64>,
    /// The last lifecycle request on each computer, including requests between retries.
    lifecycle_requests: BTreeMap<String, u64>,
}

impl State {
    /// Record that an operation touched `scope`, so observers can tell a state change
    /// was caused by Silo. Hidden housekeeping is not attributed.
    fn touch(&mut self, scope: &Scope, hidden: bool) {
        if hidden {
            return;
        }
        match scope {
            Scope::Device => self.device_generation += 1,
            Scope::Computer { id } => {
                *self.computer_generations.entry(id.clone()).or_default() += 1
            }
        }
    }

    fn generation(&self, id: &str) -> u64 {
        self.device_generation
            + self
                .computer_generations
                .get(id)
                .copied()
                .unwrap_or_default()
    }

    /// A request may run when it conflicts with no running work and no earlier waiter.
    fn admissible(&self, index: usize) -> bool {
        let scope = &self.waiting[index].scope;
        !self
            .running
            .iter()
            .any(|entry| entry.scope.conflicts(scope))
            && !self
                .waiting
                .iter()
                .take(index)
                .any(|entry| entry.scope.conflicts(scope))
    }

    fn free(&self, scope: &Scope) -> bool {
        !self
            .running
            .iter()
            .any(|entry| entry.scope.conflicts(scope))
            && !self
                .waiting
                .iter()
                .any(|entry| entry.scope.conflicts(scope))
    }

    fn entry(
        &mut self,
        scope: Scope,
        computer_name: Option<String>,
        kind: OperationKind,
        label: &str,
        key: Option<String>,
    ) -> Entry {
        self.next_id += 1;
        if kind == OperationKind::Lifecycle {
            if let Scope::Computer { id } = &scope {
                self.lifecycle_requests.insert(id.clone(), self.next_id);
            }
        }
        Entry {
            id: self.next_id,
            scope,
            computer_name,
            label: label.to_owned(),
            kind,
            key,
            since: Instant::now(),
            since_ms: now_ms(),
            admitted: Instant::now(),
            cancellable: false,
            cancel: Arc::new(AtomicBool::new(false)),
            expected: None,
            hidden: false,
            removes: Vec::new(),
        }
    }
}

pub(crate) struct OperationGate {
    state: Mutex<State>,
    changed: Condvar,
    listener: OnceLock<Box<dyn Fn() + Send + Sync>>,
    /// Count of finished visible operations, for observers that sleep until activity.
    activity: Mutex<u64>,
    activity_changed: Condvar,
}

/// A copy of the gate's generation counters. Compares equal to a later
/// `OperationGate::generation` only if nothing touched the computer in between.
pub(crate) struct Generations {
    device: u64,
    computers: BTreeMap<String, u64>,
}

impl Generations {
    pub(crate) fn of(&self, id: &str) -> u64 {
        self.device + self.computers.get(id).copied().unwrap_or_default()
    }
}

/// The gate with the operation kind fixed, so the kind is known at admission.
#[derive(Clone, Copy)]
pub(crate) struct Kinded<'a> {
    gate: &'a OperationGate,
    kind: OperationKind,
    retry_after: Option<u64>,
    hidden: bool,
}

impl<'a> Kinded<'a> {
    /// Keep this operation out of the published queue snapshot while it waits and runs. It
    /// still takes its turn and excludes conflicting work; only its visibility differs.
    pub(crate) fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    /// Retry only while this remains the latest lifecycle request on its computer.
    pub(crate) fn retry_after(mut self, request: Option<u64>) -> Self {
        self.retry_after = request;
        self
    }

    pub(crate) fn device(&self, label: &str) -> Result<OperationGuard<'a>, GateError> {
        self.acquire(Scope::Device, None, label, None)
    }

    pub(crate) fn computer(
        &self,
        id: &str,
        name: &str,
        label: &str,
    ) -> Result<OperationGuard<'a>, GateError> {
        self.acquire(
            Scope::Computer { id: id.to_owned() },
            Some(name.to_owned()),
            label,
            None,
        )
    }

    pub(crate) fn acquire(
        &self,
        scope: Scope,
        computer_name: Option<String>,
        label: &str,
        key: Option<String>,
    ) -> Result<OperationGuard<'a>, GateError> {
        self.gate.acquire_inner(
            scope,
            computer_name,
            self.kind,
            label,
            key,
            None,
            self.retry_after,
            self.hidden,
        )
    }

    pub(crate) fn acquire_while(
        &self,
        scope: Scope,
        computer_name: Option<String>,
        label: &str,
        keep_waiting: &dyn Fn() -> bool,
    ) -> Result<OperationGuard<'a>, GateError> {
        self.gate.acquire_inner(
            scope,
            computer_name,
            self.kind,
            label,
            None,
            Some(keep_waiting),
            self.retry_after,
            self.hidden,
        )
    }
}

thread_local! {
    static HELD: Cell<usize> = const { Cell::new(0) };
    /// Cancel token of the operation the current thread is executing, set while its
    /// guard is held and cleared on drop. Nesting is rejected, so at most one is set.
    static CURRENT: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
    /// The gate and id of that operation, for `OperationGate::labelled`.
    static RUNNING: Cell<Option<(*const OperationGate, u64)>> = const { Cell::new(None) };
    /// Depth of `uncancellable` sections on this thread.
    static MASKED: Cell<usize> = const { Cell::new(0) };
    /// Computers the next admission on this thread deletes (see `OperationGate::removing`).
    static REMOVES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// True when the current thread holds an operation guard. Entry points documented
/// as "the caller holds the operation gate" `debug_assert!` it (D-43).
pub(crate) fn held() -> bool {
    HELD.with(Cell::get) > 0
}

/// Run `work` with cancellation masked: a cancel requested meanwhile is not
/// observed (children are not killed) until `work` returns, and is then honoured
/// at the next check. Used for steps such as `msb stop` that must not be cut short.
pub(crate) fn uncancellable<T>(work: impl FnOnce() -> T) -> T {
    struct Unmask;
    impl Drop for Unmask {
        fn drop(&mut self) {
            MASKED.with(|masked| masked.set(masked.get() - 1));
        }
    }
    MASKED.with(|masked| masked.set(masked.get() + 1));
    let _unmask = Unmask;
    work()
}

/// True when the operation running on this thread has been asked to cancel. Only ever
/// true for operations that opted in with `OperationGuard::allow_cancel`.
pub(crate) fn cancel_requested() -> bool {
    if MASKED.with(Cell::get) > 0 {
        return false;
    }
    CURRENT.with(|current| {
        current
            .borrow()
            .as_ref()
            .is_some_and(|token| token.load(Ordering::SeqCst))
    })
}

/// `Err(GateError::Cancelled)` when the current thread's operation was asked to cancel.
pub(crate) fn check_cancelled() -> Result<(), GateError> {
    if cancel_requested() {
        Err(GateError::Cancelled)
    } else {
        Ok(())
    }
}

thread_local! {
    /// Condition for work that must start promptly or not at all (see `StartCondition`).
    static START: RefCell<Option<StartCondition>> = const { RefCell::new(None) };
}

/// Work that is only wanted if it starts while a condition holds, such as a request from
/// another device that must start before its deadline and while its sender is connected.
/// The first admission under it waits only while the condition holds, and is refused if it
/// no longer holds when the turn arrives. Once any admission succeeds the work has started,
/// so later steps and retries of the same work wait normally.
#[derive(Clone)]
pub(crate) struct StartCondition(Arc<StartState>);

struct StartState {
    wanted: Box<dyn Fn() -> bool + Send + Sync>,
    started: AtomicBool,
    expired: AtomicBool,
}

impl StartCondition {
    pub(crate) fn new(wanted: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(StartState {
            wanted: Box::new(wanted),
            started: AtomicBool::new(false),
            expired: AtomicBool::new(false),
        }))
    }

    /// True once work under this condition was admitted.
    pub(crate) fn started(&self) -> bool {
        self.0.started.load(Ordering::SeqCst)
    }

    /// True when the gate turned this work away because the condition stopped holding.
    pub(crate) fn expired(&self) -> bool {
        self.0.expired.load(Ordering::SeqCst)
    }

    /// Run `work` on this thread with `condition` applying to its gate admissions.
    pub(crate) fn scope<T>(condition: Option<StartCondition>, work: impl FnOnce() -> T) -> T {
        struct Restore(Option<StartCondition>);
        impl Drop for Restore {
            fn drop(&mut self) {
                START.with(|start| *start.borrow_mut() = self.0.take());
            }
        }
        let _restore = Restore(START.with(|start| start.replace(condition)));
        work()
    }

    /// The condition applying on this thread, to hand to a worker thread.
    pub(crate) fn current() -> Option<StartCondition> {
        START.with(|start| start.borrow().clone())
    }

    /// Still waiting to start, and the condition holds.
    fn pending(this: &Option<StartCondition>) -> Option<&StartCondition> {
        this.as_ref().filter(|condition| !condition.started())
    }
}

/// `tauri::async_runtime::spawn_blocking` that carries this thread's start condition, so
/// work handed to a worker thread still starts only while it is wanted.
pub(crate) fn spawn_blocking<F, R>(work: F) -> tauri::async_runtime::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let condition = StartCondition::current();
    tauri::async_runtime::spawn_blocking(move || StartCondition::scope(condition, work))
}
/// How long after admission a cancel waits for the work to declare itself
/// cancellable. Owners do so immediately after acquiring, so this only bounds a race.
const ADMISSION_GRACE: Duration = Duration::from_millis(250);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

impl OperationGate {
    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                next_id: 0,
                running: Vec::new(),
                waiting: VecDeque::new(),
                device_generation: 0,
                computer_generations: BTreeMap::new(),
                lifecycle_requests: BTreeMap::new(),
            }),
            changed: Condvar::new(),
            listener: OnceLock::new(),
            activity: Mutex::new(0),
            activity_changed: Condvar::new(),
        }
    }

    /// Fix the operation kind for the entries this request creates.
    pub(crate) fn kind(&self, kind: OperationKind) -> Kinded<'_> {
        Kinded {
            gate: self,
            kind,
            retry_after: None,
            hidden: false,
        }
    }

    /// A counter that changes whenever an operation touching this computer (or device-wide
    /// state, which touches every computer) was queued, started, or finished. Equal values
    /// before and after mean no Silo operation affected the computer in between.
    pub(crate) fn generation(&self, id: &str) -> u64 {
        self.lock().generation(id)
    }

    /// Every computer's generation right now, for comparing across a slow read.
    pub(crate) fn generations(&self) -> Generations {
        let state = self.lock();
        Generations {
            device: state.device_generation,
            computers: state.computer_generations.clone(),
        }
    }

    /// Current activity counter; advances each time a visible operation finishes or
    /// `signal_activity` is called.
    pub(crate) fn activity(&self) -> u64 {
        *self
            .activity
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Sleep until the activity counter differs from `seen` or `timeout` elapses, and
    /// return the current counter. Lets observers act soon after work finishes.
    pub(crate) fn wait_for_activity(&self, seen: u64, timeout: Duration) -> u64 {
        let guard = self
            .activity
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (guard, _) = self
            .activity_changed
            .wait_timeout_while(guard, timeout, |current| *current == seen)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard
    }

    /// Advance the activity counter. Also used when something outside the gate, such as a
    /// computer stopping on its own, should wake observers.
    pub(crate) fn signal_activity(&self) {
        *self
            .activity
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
        self.activity_changed.notify_all();
    }

    /// Called after every queue change, outside the internal lock. Set once at startup.
    pub(crate) fn set_listener(&self, listener: impl Fn() + Send + Sync + 'static) {
        let _ = self.listener.set(Box::new(listener));
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // The state is a plain list; a panic while holding it cannot leave it half-updated.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn notify(&self) {
        self.changed.notify_all();
        if let Some(listener) = self.listener.get() {
            listener();
        }
    }

    /// True when the waiting entry `id` may run now or was asked to cancel; either
    /// way its waiter must not give up its place as abandoned.
    fn admissible_or_cancelled(&self, state: &State, id: u64) -> bool {
        state
            .waiting
            .iter()
            .position(|entry| entry.id == id)
            .is_some_and(|index| {
                state.admissible(index) || state.waiting[index].cancel.load(Ordering::SeqCst)
            })
    }

    /// Wait for a turn to change shared device state.
    pub(crate) fn device(&self, label: &str) -> Result<OperationGuard<'_>, GateError> {
        self.kind(OperationKind::Other).device(label)
    }

    /// Wait for a device-wide turn that deletes the computers `ids`, so that work
    /// running on one of them can see the deletion queued (`removal_queued`).
    pub(crate) fn removing(
        &self,
        ids: &[String],
        label: &str,
    ) -> Result<OperationGuard<'_>, GateError> {
        REMOVES.with(|removes| *removes.borrow_mut() = ids.to_vec());
        let guard = self
            .kind(OperationKind::ComputerConfiguration)
            .device(label);
        REMOVES.with(|removes| removes.borrow_mut().clear());
        guard
    }

    /// Whether a deletion of computer `id` is waiting for its turn.
    pub(crate) fn removal_queued(&self, id: &str) -> bool {
        self.lock()
            .waiting
            .iter()
            .any(|entry| entry.removes.iter().any(|removed| removed == id))
    }

    /// The dedup keys of lifecycle operations waiting for computer `id`, in admission order. A
    /// lifecycle key is `computer:<id>:<action>`; `None` marks an entry admitted without one.
    pub(crate) fn waiting_lifecycle_keys(&self, id: &str) -> Vec<Option<String>> {
        self.lock()
            .waiting
            .iter()
            .filter(|entry| {
                entry.kind == OperationKind::Lifecycle
                    && matches!(&entry.scope, Scope::Computer { id: scoped } if scoped == id)
            })
            .map(|entry| entry.key.clone())
            .collect()
    }

    /// Wait for a turn to change one computer, identified by its stable `id`. `name` is the
    /// current display name captured for the queue; ordering keys on `id` alone.
    pub(crate) fn computer(
        &self,
        id: &str,
        name: &str,
        label: &str,
    ) -> Result<OperationGuard<'_>, GateError> {
        self.kind(OperationKind::Other).computer(id, name, label)
    }

    /// Wait for a turn, rejecting the request when an identical `key` is already waiting.
    /// `computer_name` is the display name for per-computer scopes (`None` for device scope).
    pub(crate) fn acquire(
        &self,
        scope: Scope,
        computer_name: Option<String>,
        label: &str,
        key: Option<String>,
    ) -> Result<OperationGuard<'_>, GateError> {
        self.acquire_inner(
            scope,
            computer_name,
            OperationKind::Other,
            label,
            key,
            None,
            None,
            false,
        )
    }

    /// Wait for a turn while `keep_waiting` returns true; otherwise leave the queue
    /// with `GateError::Abandoned`. Checked at least every 100 ms.
    pub(crate) fn acquire_while(
        &self,
        scope: Scope,
        computer_name: Option<String>,
        label: &str,
        keep_waiting: &dyn Fn() -> bool,
    ) -> Result<OperationGuard<'_>, GateError> {
        self.acquire_inner(
            scope,
            computer_name,
            OperationKind::Other,
            label,
            None,
            Some(keep_waiting),
            None,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn acquire_inner(
        &self,
        scope: Scope,
        computer_name: Option<String>,
        kind: OperationKind,
        label: &str,
        key: Option<String>,
        keep_waiting: Option<&dyn Fn() -> bool>,
        retry_after: Option<u64>,
        hidden: bool,
    ) -> Result<OperationGuard<'_>, GateError> {
        if HELD.with(Cell::get) > 0 {
            return Err(GateError::Nested);
        }
        let mut state = self.lock();
        if kind == OperationKind::Lifecycle {
            if let (Scope::Computer { id }, Some(request)) = (&scope, retry_after) {
                if state.lifecycle_requests.get(id) != Some(&request) {
                    return Err(GateError::Cancelled);
                }
            }
        }
        if key.is_some() && state.waiting.iter().any(|entry| entry.key == key) {
            return Err(GateError::AlreadyQueued);
        }
        let mut entry = state.entry(scope, computer_name, kind, label, key);
        entry.removes = REMOVES.with(|removes| std::mem::take(&mut *removes.borrow_mut()));
        entry.hidden = hidden;
        let id = entry.id;
        state.touch(&entry.scope, hidden);
        state.waiting.push_back(entry);
        drop(state);
        self.notify();
        // Work that must start promptly waits only while it is still wanted.
        let start = StartCondition::current();
        let pending = StartCondition::pending(&start).cloned();
        let expired = Cell::new(false);
        let still_wanted = || {
            let wanted = pending
                .as_ref()
                .is_none_or(|condition| condition.started() || (condition.0.wanted)());
            expired.set(!wanted);
            wanted && keep_waiting.is_none_or(|keep| keep())
        };
        let keep_waiting: Option<&dyn Fn() -> bool> =
            (pending.is_some() || keep_waiting.is_some()).then_some(&still_wanted);
        let mut state = self.lock();
        // Whether a start condition was confirmed since the last wait.
        let mut confirmed = false;
        let token = loop {
            let index = state
                .waiting
                .iter()
                .position(|entry| entry.id == id)
                .expect("a waiting operation is removed only when admitted");
            // A cancel while waiting removes this entry and reports it to the caller.
            if state.waiting[index].cancel.load(Ordering::SeqCst) {
                state.waiting.remove(index);
                drop(state);
                self.notify();
                return Err(GateError::Cancelled);
            }
            if state.admissible(index) {
                // Work no longer wanted when its turn arrives never starts. The condition is
                // asked without the state lock held (D-33); the next pass re-checks the turn.
                if let Some(condition) = pending
                    .as_ref()
                    .filter(|condition| !confirmed && !condition.started())
                {
                    drop(state);
                    let wanted = (condition.0.wanted)();
                    state = self.lock();
                    if !wanted {
                        condition.0.expired.store(true, Ordering::SeqCst);
                        state.waiting.retain(|entry| entry.id != id);
                        drop(state);
                        self.notify();
                        return Err(GateError::Abandoned);
                    }
                    confirmed = true;
                    continue;
                }
                if let Some(condition) = &start {
                    condition.0.started.store(true, Ordering::SeqCst);
                }
                let mut entry = state.waiting.remove(index).expect("index is in range");
                entry.since = Instant::now();
                entry.since_ms = now_ms();
                entry.admitted = entry.since;
                let token = entry.cancel.clone();
                state.touch(&entry.scope, entry.hidden);
                state.running.push(entry);
                break token;
            }
            confirmed = false;
            match keep_waiting {
                None => {
                    state = self
                        .changed
                        .wait(state)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
                Some(keep_waiting) => {
                    state = self
                        .changed
                        .wait_timeout(state, std::time::Duration::from_millis(100))
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .0;
                    // Re-check admission first: a turn that arrived is not given up.
                    if self.admissible_or_cancelled(&state, id) {
                        continue;
                    }
                    // Ask the caller without the state lock held: the closure may read
                    // the gate (or anything that does) without deadlocking (D-33).
                    drop(state);
                    let keep = keep_waiting();
                    state = self.lock();
                    // The turn may have arrived (or a cancel) while the closure ran;
                    // the next loop pass admits or cancels it instead of giving it up.
                    if !keep && !self.admissible_or_cancelled(&state, id) {
                        // A start condition that stopped holding: the work never starts.
                        if let Some(condition) = pending.as_ref().filter(|_| expired.get()) {
                            condition.0.expired.store(true, Ordering::SeqCst);
                        }
                        state.waiting.retain(|entry| entry.id != id);
                        drop(state);
                        self.notify();
                        return Err(GateError::Abandoned);
                    }
                }
            }
        };
        drop(state);
        HELD.with(|held| held.set(held.get() + 1));
        CURRENT.with(|current| *current.borrow_mut() = Some(token.clone()));
        RUNNING.set(Some((self, id)));
        self.notify();
        Ok(OperationGuard {
            gate: self,
            id,
            token,
            _thread_bound: std::marker::PhantomData,
        })
    }

    /// Run only when nothing conflicting is running or waiting. For background work
    /// that should skip busy periods rather than accumulate.
    pub(crate) fn try_acquire(
        &self,
        scope: Scope,
        computer_name: Option<String>,
        label: &str,
    ) -> Result<OperationGuard<'_>, GateError> {
        self.try_acquire_inner(scope, computer_name, label, false)
    }

    fn try_acquire_inner(
        &self,
        scope: Scope,
        computer_name: Option<String>,
        label: &str,
        hidden: bool,
    ) -> Result<OperationGuard<'_>, GateError> {
        if HELD.with(Cell::get) > 0 {
            return Err(GateError::Nested);
        }
        let mut state = self.lock();
        if !state.free(&scope) {
            return Err(GateError::Busy);
        }
        let mut entry = state.entry(scope, computer_name, OperationKind::Other, label, None);
        entry.hidden = hidden;
        let id = entry.id;
        let token = entry.cancel.clone();
        state.touch(&entry.scope, hidden);
        state.running.push(entry);
        drop(state);
        HELD.with(|held| held.set(held.get() + 1));
        CURRENT.with(|current| *current.borrow_mut() = Some(token.clone()));
        RUNNING.set(Some((self, id)));
        self.notify();
        Ok(OperationGuard {
            gate: self,
            id,
            token,
            _thread_bound: std::marker::PhantomData,
        })
    }

    pub(crate) fn try_device(&self, label: &str) -> Result<OperationGuard<'_>, GateError> {
        self.try_acquire(Scope::Device, None, label)
    }

    /// Like `try_device`, but for internal background housekeeping: the operation still
    /// holds the gate for mutual exclusion, yet never appears in the published queue
    /// snapshot, so opportunistic maintenance cannot flash a status in the UI.
    pub(crate) fn try_device_hidden(&self, label: &str) -> Result<OperationGuard<'_>, GateError> {
        self.try_acquire_inner(Scope::Device, None, label, true)
    }

    /// Like `try_computer`, but hidden from the published queue snapshot (see `try_device_hidden`).
    pub(crate) fn try_computer_hidden(
        &self,
        id: &str,
        name: &str,
        label: &str,
    ) -> Result<OperationGuard<'_>, GateError> {
        self.try_acquire_inner(
            Scope::Computer { id: id.to_owned() },
            Some(name.to_owned()),
            label,
            true,
        )
    }

    /// Run work for one computer only when nothing conflicting for that computer is running or
    /// waiting. For background repair that should skip busy computers rather than queue.
    /// Identified by the stable `id`; `name` is the display name for the queue.
    pub(crate) fn try_computer(
        &self,
        id: &str,
        name: &str,
        label: &str,
    ) -> Result<OperationGuard<'_>, GateError> {
        self.try_acquire(
            Scope::Computer { id: id.to_owned() },
            Some(name.to_owned()),
            label,
        )
    }

    /// True when no operation is running or waiting. Observers use this to discard
    /// readings that overlapped a change.
    pub(crate) fn is_idle(&self) -> bool {
        let state = self.lock();
        state.running.is_empty() && state.waiting.is_empty()
    }

    /// True when no visible device-wide operation is running or waiting. Hidden
    /// housekeeping and per-computer work never add or remove computers, so state readers
    /// use this instead of `is_idle` and keep other computers' rows current.
    pub(crate) fn is_device_idle(&self) -> bool {
        let state = self.lock();
        !state
            .running
            .iter()
            .chain(state.waiting.iter())
            .any(|entry| !entry.hidden && matches!(entry.scope, Scope::Device))
    }

    /// True when no operation affecting the computer with stable `id` is running or waiting.
    pub(crate) fn is_computer_idle(&self, id: &str) -> bool {
        self.lock().free(&Scope::Computer { id: id.to_owned() })
    }

    /// True when no visible operation other than the calling thread's own is running or
    /// waiting for the computer with stable `id` (device-wide work affects every computer). State
    /// readers use this to decide whether a computer's reading is settled: hidden housekeeping
    /// never makes a reading stale, and work reading state after its own change is not
    /// waiting on itself.
    pub(crate) fn is_computer_quiet(&self, id: &str) -> bool {
        let scope = Scope::Computer { id: id.to_owned() };
        let own = CURRENT.with(|current| current.borrow().clone());
        let state = self.lock();
        !state
            .running
            .iter()
            .chain(state.waiting.iter())
            .any(|entry| {
                !entry.hidden
                    && entry.scope.conflicts(&scope)
                    && !own
                        .as_ref()
                        .is_some_and(|token| Arc::ptr_eq(token, &entry.cancel))
            })
    }

    /// The queue as the UI sees it. Hidden internal-housekeeping entries are excluded, but
    /// they still gate real work: a visible waiter held up only by a hidden entry is flagged
    /// with `blocked_by_hidden` so the UI can explain the wait generically.
    pub(crate) fn snapshot(&self) -> OperationQueue {
        let state = self.lock();
        let running = state
            .running
            .iter()
            .filter(|entry| !entry.hidden)
            .map(|entry| entry.public(true))
            .collect();
        let waiting = state
            .waiting
            .iter()
            .enumerate()
            .filter(|(_, entry)| !entry.hidden)
            .map(|(index, entry)| {
                let mut public = entry.public(false);
                // A hidden running entry, or a hidden earlier waiter, that conflicts with
                // this waiter is holding up its turn without appearing in the queue.
                public.blocked_by_hidden = state
                    .running
                    .iter()
                    .any(|other| other.hidden && other.scope.conflicts(&entry.scope))
                    || state
                        .waiting
                        .iter()
                        .take(index)
                        .any(|other| other.hidden && other.scope.conflicts(&entry.scope));
                public
            })
            .collect();
        OperationQueue { running, waiting }
    }

    /// Ask to cancel the operation with `id`.
    ///
    /// A *waiting* entry is signalled to leave the queue; its waiter returns
    /// `GateError::Cancelled`. A *running* entry is signalled only when it opted in as
    /// cancellable (`OperationGuard::allow_cancel`); otherwise `GateError::NotCancellable`
    /// is returned and nothing changes. An unknown id is treated as already finished.
    ///
    /// Work declares itself cancellable right after admission, so a cancel can race
    /// that declaration: the user cancelled a waiting entry just as its turn came, or
    /// a start before its owner called `allow_cancel`. Such a cancel waits up to
    /// `ADMISSION_GRACE` after admission for the work to opt in and is then honoured
    /// (D-32); only work that stays non-cancellable reports `NotCancellable`.
    pub(crate) fn cancel(&self, id: u64) -> Result<(), GateError> {
        let mut state = self.lock();
        if let Some(entry) = state.waiting.iter().find(|entry| entry.id == id) {
            entry.cancel.store(true, Ordering::SeqCst);
            drop(state);
            // Wake the waiter so it observes the flag and leaves the queue.
            self.notify();
            return Ok(());
        }
        loop {
            let Some(entry) = state.running.iter().find(|entry| entry.id == id) else {
                return Ok(());
            };
            if entry.cancellable {
                entry.cancel.store(true, Ordering::SeqCst);
                drop(state);
                self.notify();
                return Ok(());
            }
            let remaining = ADMISSION_GRACE.saturating_sub(entry.admitted.elapsed());
            if remaining.is_zero() {
                return Err(GateError::NotCancellable);
            }
            state = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }

    /// Signal every waiting entry to leave the queue with `GateError::Cancelled`.
    /// Running work is left untouched. Used by Quit: once admission is refused a
    /// waiter would only be rejected when its turn came, so it is cancelled at once,
    /// and again before Quit releases the gate so work requested before its computers
    /// stopped never runs after a failed Quit (D-30).
    pub(crate) fn cancel_all_waiting(&self) {
        let state = self.lock();
        if state.waiting.is_empty() {
            return;
        }
        for entry in state.waiting.iter() {
            entry.cancel.store(true, Ordering::SeqCst);
        }
        drop(state);
        // Wake every waiter so it observes the flag and leaves the queue.
        self.notify();
    }

    /// Longest-running operation and its age, for stuck-operation reporting.
    pub(crate) fn oldest_running(&self) -> Option<(String, std::time::Duration)> {
        let state = self.lock();
        state
            .running
            .iter()
            .min_by_key(|entry| entry.since)
            .map(|entry| (entry.label.clone(), entry.since.elapsed()))
    }

    fn release(&self, id: u64) {
        let mut state = self.lock();
        let finished = state
            .running
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| (entry.scope.clone(), entry.hidden));
        state.running.retain(|entry| entry.id != id);
        if let Some((scope, hidden)) = &finished {
            state.touch(scope, *hidden);
        }
        drop(state);
        HELD.with(|held| held.set(held.get().saturating_sub(1)));
        CURRENT.with(|current| *current.borrow_mut() = None);
        RUNNING.set(None);
        if finished.is_some_and(|(_, hidden)| !hidden) {
            self.signal_activity();
        }
        self.notify();
    }

    /// Mark a running operation cancellable and record it in the queue. Used by
    /// `OperationGuard::allow_cancel`.
    fn mark_cancellable(&self, id: u64) {
        let mut state = self.lock();
        if let Some(entry) = state.running.iter_mut().find(|entry| entry.id == id) {
            entry.cancellable = true;
        }
        drop(state);
        self.notify();
    }

    /// Change a running operation's queue label, for work that reports its progress.
    /// Show `label` for the operation this thread runs on this gate while `work` runs, then
    /// restore its label. For a long step in shared code that the operation's owner does
    /// not know about, such as the account setup after a boot. Without one, runs `work`.
    pub(crate) fn labelled<T>(&self, label: &str, work: impl FnOnce() -> T) -> T {
        let own = RUNNING
            .get()
            .filter(|(gate, _)| std::ptr::eq(*gate, self))
            .map(|(_, id)| id);
        let previous = own.and_then(|id| {
            let state = self.lock();
            let entry = state.running.iter().find(|entry| entry.id == id)?;
            Some(entry.label.clone())
        });
        if let Some(id) = own {
            self.relabel(id, label);
        }
        let result = work();
        if let (Some(id), Some(previous)) = (own, previous) {
            self.relabel(id, &previous);
        }
        result
    }

    fn relabel(&self, id: u64, label: &str) {
        let mut state = self.lock();
        if let Some(entry) = state.running.iter_mut().find(|entry| entry.id == id) {
            entry.label = label.to_owned();
        }
        drop(state);
        self.notify();
    }

    /// Record the expected maximum duration of a running operation for stuck reporting.
    fn set_expected(&self, id: u64, expected: Duration) {
        let mut state = self.lock();
        if let Some(entry) = state.running.iter_mut().find(|entry| entry.id == id) {
            entry.expected = Some(expected);
        }
        drop(state);
        self.notify();
    }

    fn since_of(&self, id: u64) -> Since {
        let state = self.lock();
        state
            .running
            .iter()
            .find(|entry| entry.id == id)
            .map_or_else(
                || Since {
                    at: Instant::now(),
                    ms: now_ms(),
                },
                |entry| Since {
                    at: entry.since,
                    ms: entry.since_ms,
                },
            )
    }

    /// Report a running operation as started at `since`, for a later attempt of one
    /// logical operation (D-27). Slow-operation flagging then counts the whole sequence.
    fn set_since(&self, id: u64, since: Since) {
        let mut state = self.lock();
        if let Some(entry) = state.running.iter_mut().find(|entry| entry.id == id) {
            entry.since = since.at;
            entry.since_ms = since.ms;
        }
        drop(state);
        self.notify();
    }

    /// Point a running operation's cancel flag at an externally owned one, so a subsystem
    /// with its own cancellation (for example backups) and the gate agree on one bit.
    fn replace_token(&self, id: u64, token: Arc<AtomicBool>) {
        let mut state = self.lock();
        if let Some(entry) = state.running.iter_mut().find(|entry| entry.id == id) {
            // Preserve an already-requested cancel across the swap.
            if entry.cancel.load(Ordering::SeqCst) {
                token.store(true, Ordering::SeqCst);
            }
            entry.cancel = token;
        }
        drop(state);
    }
}

/// When an operation started running, carried from the first attempt of a retry
/// sequence to the later ones so the queue keeps one continuous start time.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Since {
    at: Instant,
    ms: u64,
}

/// Held for the duration of one operation, including all of its internal steps.
#[must_use = "the operation ends when the guard is dropped"]
pub(crate) struct OperationGuard<'a> {
    gate: &'a OperationGate,
    id: u64,
    /// Shared cancel flag for this operation. Cloneable so work on other threads
    /// (spawn_blocking, std::thread::spawn) can observe cancellation explicitly.
    token: Arc<AtomicBool>,
    /// Nesting is tracked per thread, so a guard must be released where it was taken.
    _thread_bound: std::marker::PhantomData<*const ()>,
}

impl OperationGuard<'_> {
    pub(crate) fn request_id(&self) -> u64 {
        self.id
    }

    /// Allow the user to cancel this operation while it runs. The work must observe
    /// cancellation via `operation_gate::check_cancelled`/`cancel_requested` (same
    /// thread) or the token from `cancel_token` (other threads); nothing is force-killed
    /// except child processes in the runtime polling loops.
    pub(crate) fn allow_cancel(&self) {
        self.gate.mark_cancellable(self.id);
    }

    /// Declare the expected maximum duration so the UI can flag the operation as slow.
    pub(crate) fn expect_within(&self, expected: Duration) {
        self.gate.set_expected(self.id, expected);
    }

    /// Show `label` for this operation in the queue from now on, for example the step
    /// a long device-wide operation is on. Observers are notified.
    pub(crate) fn relabel(&self, label: &str) {
        self.gate.relabel(self.id, label);
    }

    /// When this operation started running.
    pub(crate) fn since(&self) -> Since {
        self.gate.since_of(self.id)
    }

    /// Continue an earlier attempt's start time: a retry of the same operation is one
    /// continuous piece of work in the queue, so "taking longer than expected" can fire.
    pub(crate) fn continue_since(&self, since: Since) {
        self.gate.set_since(self.id, since);
    }

    /// A cloneable handle to this operation's cancel flag, for passing to work that runs
    /// on other threads where the thread-local current-operation token is not set.
    pub(crate) fn cancel_token(&self) -> Arc<AtomicBool> {
        self.token.clone()
    }

    /// Share an externally owned cancel flag with this operation, so a cancel through
    /// either the gate or the owning subsystem flips the same bit. Used by backups.
    pub(crate) fn adopt_cancel_token(&mut self, token: Arc<AtomicBool>) {
        self.gate.replace_token(self.id, token.clone());
        // Keep the thread-local current-operation token in sync so `cancel_requested`
        // and `check_cancelled` observe the shared flag on this thread.
        CURRENT.with(|current| *current.borrow_mut() = Some(token.clone()));
        self.token = token;
    }
}

impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        self.gate.release(self.id);
    }
}

impl std::fmt::Debug for OperationGuard<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OperationGuard")
            .field("id", &self.id)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread;
    use std::time::Duration;

    fn leak() -> &'static OperationGate {
        Box::leak(Box::new(OperationGate::new()))
    }

    /// Nesting is per thread, so a second concurrent caller needs its own thread.
    fn elsewhere<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
        thread::spawn(work).join().unwrap()
    }

    fn wait_until(gate: &OperationGate, predicate: impl Fn(&OperationQueue) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !predicate(&gate.snapshot()) {
            assert!(
                Instant::now() < deadline,
                "gate did not reach expected state"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn rejected_old_lifecycle_retry_does_not_supersede_the_newer_request() {
        let gate = OperationGate::new();
        let lifecycle = gate.kind(OperationKind::Lifecycle);
        let old = lifecycle.computer("id-a", "a", "Starting a").unwrap();
        let old_id = old.request_id();
        drop(old);
        let new = lifecycle.computer("id-a", "a", "Stopping a").unwrap();
        let new_id = new.request_id();
        drop(new);
        assert!(matches!(
            gate.kind(OperationKind::Lifecycle)
                .retry_after(Some(old_id))
                .computer("id-a", "a", "Retrying start"),
            Err(GateError::Cancelled)
        ));
        let retry = gate
            .kind(OperationKind::Lifecycle)
            .retry_after(Some(new_id))
            .computer("id-a", "a", "Retrying stop")
            .unwrap();
        drop(retry);
        assert!(gate.is_idle());
    }

    #[test]
    fn different_computers_run_concurrently() {
        let gate = leak();
        let a = gate.computer("id-a", "a", "Start a").unwrap();
        let (sent, received) = mpsc::channel();
        thread::spawn(move || {
            let _b = gate.computer("id-b", "b", "Start b").unwrap();
            sent.send(()).unwrap();
        });
        received.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(a);
    }

    #[test]
    fn same_computer_waits_instead_of_failing() {
        let gate = leak();
        let first = gate.computer("id-a", "a", "Start a").unwrap();
        let handle = thread::spawn(move || {
            let _second = gate.computer("id-a", "a", "Stop a").unwrap();
        });
        wait_until(gate, |queue| {
            queue.waiting.len() == 1 && queue.waiting[0].label == "Stop a"
        });
        drop(first);
        handle.join().unwrap();
        assert!(gate.is_idle());
    }

    #[test]
    fn same_computer_id_conflicts_even_when_the_name_differs() {
        // A rename mid-flight keeps the stable id, so a second operation on the same
        // Computer waits its turn even though it carries a different display name.
        let gate = leak();
        let first = gate.computer("id-a", "old-name", "Start old-name").unwrap();
        let handle = thread::spawn(move || {
            let _second = gate.computer("id-a", "new-name", "Stop new-name").unwrap();
        });
        wait_until(gate, |queue| {
            queue.waiting.len() == 1 && queue.waiting[0].label == "Stop new-name"
        });
        // The running entry reports the id it is keyed on and the name it captured.
        let running = &gate.snapshot().running[0];
        assert_eq!(running.computer_id.as_deref(), Some("id-a"));
        assert_eq!(running.computer_name.as_deref(), Some("old-name"));
        drop(first);
        handle.join().unwrap();
        assert!(gate.is_idle());
    }

    #[test]
    fn different_ids_sharing_a_name_do_not_conflict() {
        // Two computers that transiently share a display name still run concurrently: the
        // gate keys on id, not name.
        let gate = leak();
        let a = gate.computer("id-a", "shared", "Start id-a").unwrap();
        let (sent, received) = mpsc::channel();
        thread::spawn(move || {
            let _b = gate.computer("id-b", "shared", "Start id-b").unwrap();
            sent.send(()).unwrap();
        });
        received.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(a);
    }

    #[test]
    fn device_operation_is_not_starved_by_later_computer_work() {
        let gate = leak();
        let order = Arc::new(Mutex::new(Vec::new()));
        let a = gate.computer("id-a", "a", "Backup a").unwrap();
        let device = {
            let order = order.clone();
            thread::spawn(move || {
                let _guard = gate.device("Update").unwrap();
                order.lock().unwrap().push("update");
            })
        };
        wait_until(gate, |queue| queue.waiting.len() == 1);
        let later = {
            let order = order.clone();
            thread::spawn(move || {
                let _guard = gate.computer("id-b", "b", "Start b").unwrap();
                order.lock().unwrap().push("start b");
            })
        };
        wait_until(gate, |queue| queue.waiting.len() == 2);
        drop(a);
        device.join().unwrap();
        later.join().unwrap();
        assert_eq!(*order.lock().unwrap(), ["update", "start b"]);
    }

    #[test]
    fn unrelated_computer_bypasses_a_waiter_for_another_computer() {
        let gate = leak();
        let a = gate.computer("id-a", "a", "Backup a").unwrap();
        let waiter = thread::spawn(move || drop(gate.computer("id-a", "a", "Stop a").unwrap()));
        wait_until(gate, |queue| queue.waiting.len() == 1);
        assert!(elsewhere(move || gate
            .computer("id-b", "b", "Stop b")
            .is_ok()));
        drop(a);
        waiter.join().unwrap();
    }

    #[test]
    fn identical_waiting_request_is_rejected() {
        let gate = leak();
        let running = gate.device("Edit").unwrap();
        let key = Some("computer:id-a:start".to_owned());
        let first = {
            let key = key.clone();
            thread::spawn(move || {
                drop(
                    gate.acquire(
                        Scope::Computer { id: "id-a".into() },
                        Some("a".into()),
                        "Start a",
                        key,
                    )
                    .unwrap(),
                )
            })
        };
        wait_until(gate, |queue| queue.waiting.len() == 1);
        assert_eq!(
            elsewhere(move || gate
                .acquire(
                    Scope::Computer { id: "id-a".into() },
                    Some("a".into()),
                    "Start a",
                    key
                )
                .unwrap_err()),
            GateError::AlreadyQueued
        );
        drop(running);
        first.join().unwrap();
    }

    #[test]
    fn held_reports_whether_this_thread_holds_an_operation() {
        let gate = leak();
        assert!(!held());
        let guard = gate.computer("id-a", "a", "Start a").unwrap();
        assert!(held());
        assert!(!elsewhere(held), "holding is per thread");
        drop(guard);
        assert!(!held());
        drop(gate.try_device_hidden("Reconciling SSH access").unwrap());
        assert!(!held());
    }

    #[test]
    fn nested_acquire_fails_instead_of_deadlocking() {
        let gate = leak();
        let _outer = gate.computer("id-a", "a", "Fork a").unwrap();
        assert_eq!(gate.device("Inner").unwrap_err(), GateError::Nested);
        assert_eq!(
            gate.computer("id-b", "b", "Inner").unwrap_err(),
            GateError::Nested
        );
    }

    #[test]
    fn try_acquire_skips_busy_and_waiting_work() {
        let gate = leak();
        let a = gate.computer("id-a", "a", "Start a").unwrap();
        assert_eq!(
            elsewhere(move || gate.try_device("Reclaim").unwrap_err()),
            GateError::Busy
        );
        assert!(elsewhere(move || gate
            .try_computer("id-b", "b", "Sync b")
            .is_ok()));
        assert_eq!(
            elsewhere(move || gate.try_computer("id-a", "a", "Sync a").unwrap_err()),
            GateError::Busy
        );
        assert!(!gate.is_computer_idle("id-a"));
        assert!(gate.is_computer_idle("id-b"));
        drop(a);
        assert!(elsewhere(move || gate.try_device("Reclaim").is_ok()));
    }

    #[test]
    fn abandoned_waiter_leaves_the_queue_and_unblocks_later_work() {
        let gate = leak();
        let running = gate.device("Update").unwrap();
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let waiter = {
            let cancelled = cancelled.clone();
            thread::spawn(move || {
                gate.acquire_while(Scope::Device, None, "Backup", &|| {
                    !cancelled.load(Ordering::SeqCst)
                })
                .unwrap_err()
            })
        };
        wait_until(gate, |queue| queue.waiting.len() == 1);
        cancelled.store(true, Ordering::SeqCst);
        assert_eq!(waiter.join().unwrap(), GateError::Abandoned);
        assert!(gate.snapshot().waiting.is_empty());
        drop(running);
        assert!(gate.is_idle());
    }

    #[test]
    fn keep_waiting_may_read_the_gate_without_deadlocking() {
        let gate = leak();
        let running = gate.device("Update").unwrap();
        let asked = Arc::new(AtomicUsize::new(0));
        let (done, finished) = mpsc::channel();
        {
            let asked = asked.clone();
            thread::spawn(move || {
                // The closure reads the gate, as a future caller might (D-33).
                let result = gate.acquire_while(Scope::Device, None, "Backup", &|| {
                    asked.fetch_add(1, Ordering::SeqCst);
                    gate.snapshot().waiting.len() == 1
                });
                done.send(result.map(drop)).unwrap();
            });
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while asked.load(Ordering::SeqCst) < 2 {
            assert!(
                Instant::now() < deadline,
                "keep_waiting was not asked while waiting"
            );
            thread::sleep(Duration::from_millis(5));
        }
        drop(running);
        assert_eq!(
            finished.recv_timeout(Duration::from_secs(5)).unwrap(),
            Ok(())
        );
        assert!(gate.is_idle());
    }

    #[test]
    fn listener_observes_changes() {
        let gate = leak();
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        gate.set_listener(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        drop(gate.device("Edit").unwrap());
        assert!(calls.load(Ordering::SeqCst) >= 2);
        assert!(gate.oldest_running().is_none());
    }

    #[test]
    fn cancelling_a_waiting_entry_makes_its_waiter_return_cancelled() {
        let gate = leak();
        let running = gate.device("Update").unwrap();
        let waiter = thread::spawn(move || gate.device("Backup").unwrap_err());
        wait_until(gate, |queue| queue.waiting.len() == 1);
        let waiting_id = gate.snapshot().waiting[0].id;
        // Waiting entries always report themselves as cancellable.
        assert!(gate.snapshot().waiting[0].cancellable);
        gate.cancel(waiting_id).unwrap();
        assert_eq!(waiter.join().unwrap(), GateError::Cancelled);
        assert!(gate.snapshot().waiting.is_empty());
        drop(running);
        assert!(gate.is_idle());
    }

    #[test]
    fn cancel_all_waiting_cancels_every_waiter_and_leaves_running_work() {
        let gate = leak();
        // One running entry that must be left untouched.
        let running = gate.device("Exporting computer").unwrap();
        let first = thread::spawn(move || gate.computer("id-a", "a", "Stop a").unwrap_err());
        wait_until(gate, |queue| queue.waiting.len() == 1);
        let second = thread::spawn(move || gate.computer("id-b", "b", "Stop b").unwrap_err());
        wait_until(gate, |queue| queue.waiting.len() == 2);
        gate.cancel_all_waiting();
        assert_eq!(first.join().unwrap(), GateError::Cancelled);
        assert_eq!(second.join().unwrap(), GateError::Cancelled);
        let queue = gate.snapshot();
        assert!(queue.waiting.is_empty());
        assert_eq!(queue.running.len(), 1);
        assert_eq!(queue.running[0].label, "Exporting computer");
        drop(running);
        assert!(gate.is_idle());
    }

    #[test]
    fn cancelling_a_cancellable_running_entry_kills_its_child() {
        let gate = leak();
        let (ready, started) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        thread::spawn(move || {
            let guard = gate.computer("id-a", "a", "Starting a").unwrap();
            guard.allow_cancel();
            // Stand in for a runtime child: a long sleep that a polling loop kills when
            // the current operation is cancel-requested, exactly like run_msb_process.
            let mut child = std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap();
            ready.send(child.id()).unwrap();
            let killed = loop {
                if cancel_requested() {
                    let _ = child.kill();
                    let _ = child.wait();
                    break true;
                }
                if child.try_wait().unwrap().is_some() {
                    break false;
                }
                thread::sleep(Duration::from_millis(5));
            };
            done.send(killed).unwrap();
        });
        let _child_pid = started.recv_timeout(Duration::from_secs(5)).unwrap();
        wait_until(gate, |queue| {
            queue.running.iter().any(|entry| entry.cancellable)
        });
        let running_id = gate.snapshot().running[0].id;
        gate.cancel(running_id).unwrap();
        assert!(finished.recv_timeout(Duration::from_secs(5)).unwrap());
    }

    #[test]
    fn uncancellable_section_defers_a_cancel_until_it_returns() {
        let gate = leak();
        let guard = gate.computer("id-a", "a", "Restarting a").unwrap();
        guard.allow_cancel();
        gate.cancel(gate.snapshot().running[0].id).unwrap();
        assert!(!uncancellable(cancel_requested));
        assert!(cancel_requested());
        drop(guard);
    }

    #[test]
    fn a_cancel_racing_admission_is_honoured_once_the_work_opts_in() {
        let gate = leak();
        let (admitted, admission) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let worker = thread::spawn(move || {
            let guard = gate.computer("id-a", "a", "Starting a").unwrap();
            admitted.send(()).unwrap();
            // The owner declares cancellability only after the cancel arrived.
            released.recv().unwrap();
            guard.allow_cancel();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !cancel_requested() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(2));
            }
            cancel_requested()
        });
        admission.recv_timeout(Duration::from_secs(5)).unwrap();
        let id = gate.snapshot().running[0].id;
        let canceller = thread::spawn(move || gate.cancel(id));
        thread::sleep(Duration::from_millis(20));
        release.send(()).unwrap();
        assert_eq!(canceller.join().unwrap(), Ok(()));
        assert!(worker.join().unwrap(), "the racing cancel reaches the work");
    }

    #[test]
    fn cancelling_a_non_cancellable_running_entry_is_rejected() {
        let gate = leak();
        let running = gate.device("Stopping").unwrap();
        let id = gate.snapshot().running[0].id;
        assert!(!gate.snapshot().running[0].cancellable);
        assert_eq!(gate.cancel(id).unwrap_err(), GateError::NotCancellable);
        drop(running);
    }

    #[test]
    fn hidden_housekeeping_is_excluded_from_the_snapshot_but_still_exclusive() {
        let gate = leak();
        // A hidden background reconcile holds the device gate but never surfaces.
        let housekeeping = gate.try_device_hidden("Reconciling SSH access").unwrap();
        let snapshot = gate.snapshot();
        assert!(
            snapshot.running.is_empty(),
            "hidden entry must not appear in the queue"
        );
        assert!(snapshot.waiting.is_empty());
        // It is not idle, though: a conflicting try still finds the gate busy.
        assert!(!gate.is_idle());
        assert_eq!(
            elsewhere(move || gate.try_device("Reclaim").unwrap_err()),
            GateError::Busy
        );
        drop(housekeeping);
        assert!(gate.is_idle());
        assert!(elsewhere(move || gate.try_device("Reclaim").is_ok()));
    }

    #[test]
    fn a_visible_waiter_blocked_only_by_hidden_work_is_flagged() {
        let gate = leak();
        // Hidden device-wide housekeeping is running.
        let housekeeping = gate.try_device_hidden("Cleaning up expired logs").unwrap();
        // A user-initiated per-computer operation arrives and must wait behind it.
        let waiter = thread::spawn(move || drop(gate.computer("id-a", "a", "Start a").unwrap()));
        wait_until(gate, |queue| {
            // The waiter is visible; the hidden blocker is not, so it is flagged instead.
            queue.waiting.len() == 1
                && queue.waiting[0].label == "Start a"
                && queue.waiting[0].blocked_by_hidden
        });
        assert!(
            gate.snapshot().running.is_empty(),
            "the hidden blocker stays out of the queue"
        );
        drop(housekeeping);
        waiter.join().unwrap();
        assert!(gate.is_idle());
    }

    #[test]
    fn a_hidden_queued_operation_never_surfaces_while_waiting_or_running() {
        let gate = leak();
        let first = gate.computer("id-a", "a", "Starting a").unwrap();
        let hidden = thread::spawn(move || {
            let turn = gate
                .kind(OperationKind::Other)
                .hidden()
                .computer("id-a", "a", "Setting up computer use in a")
                .unwrap();
            // Running, it still holds the computer's turn but is not published.
            assert!(gate.snapshot().running.is_empty());
            drop(turn);
        });
        // Waiting behind the visible start, it is held out of the published queue too.
        thread::sleep(Duration::from_millis(100));
        let queue = gate.snapshot();
        assert_eq!(queue.running.len(), 1);
        assert!(queue.waiting.is_empty(), "{queue:?}");
        // A visible operation queued behind it explains the wait generically.
        let behind = thread::spawn(move || drop(gate.computer("id-a", "a", "Stopping a").unwrap()));
        wait_until(gate, |queue| {
            queue.waiting.len() == 1 && queue.waiting[0].blocked_by_hidden
        });
        drop(first);
        hidden.join().unwrap();
        behind.join().unwrap();
        assert!(gate.is_idle());
    }

    #[test]
    fn a_waiter_blocked_by_visible_work_is_not_flagged_as_hidden() {
        let gate = leak();
        let visible = gate.device("Updating").unwrap();
        let waiter = thread::spawn(move || drop(gate.computer("id-a", "a", "Start a").unwrap()));
        wait_until(gate, |queue| {
            queue.waiting.len() == 1 && queue.waiting[0].label == "Start a"
        });
        assert!(!gate.snapshot().waiting[0].blocked_by_hidden);
        drop(visible);
        waiter.join().unwrap();
    }

    #[test]
    fn a_later_attempt_can_continue_the_first_attempts_start_time() {
        let gate = leak();
        let first = gate.computer("id-a", "a", "Stopping a").unwrap();
        let since = first.since();
        let reported = gate.snapshot().running[0].since_ms;
        drop(first);
        thread::sleep(Duration::from_millis(5));
        let second = gate
            .computer("id-a", "a", "Stopping a (attempt 2 of 3)")
            .unwrap();
        assert!(gate.snapshot().running[0].since_ms >= reported);
        second.continue_since(since);
        assert_eq!(gate.snapshot().running[0].since_ms, reported);
        assert!(gate.oldest_running().unwrap().1 >= Duration::from_millis(5));
        drop(second);
    }

    #[test]
    fn shared_code_labels_the_operation_running_on_its_thread_while_it_works() {
        let gate = leak();
        let other = leak();
        // Outside any operation the work still runs.
        assert_eq!(gate.labelled("Setting up", || 1), 1);
        let guard = gate.computer("a", "a", "Start a").unwrap();
        let shown = gate.labelled("Setting up the silo account in a", || {
            // Another gate's operation is not this thread's.
            other.labelled("Elsewhere", || ());
            gate.snapshot().running[0].label.clone()
        });
        assert_eq!(shown, "Setting up the silo account in a");
        assert_eq!(gate.snapshot().running[0].label, "Start a");
        drop(guard);
        let _later = gate.computer("b", "b", "Start b").unwrap();
        elsewhere(move || gate.labelled("Not this thread", || ()));
        assert_eq!(gate.snapshot().running[0].label, "Start b");
    }

    #[test]
    fn a_running_operation_can_report_its_current_step_in_the_queue() {
        let gate = leak();
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        gate.set_listener(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        let guard = gate
            .kind(OperationKind::Shutdown)
            .device("Stopping local computers")
            .unwrap();
        let before = calls.load(Ordering::SeqCst);
        guard.relabel("Stopping dev (1 of 2)");
        assert_eq!(gate.snapshot().running[0].label, "Stopping dev (1 of 2)");
        assert_eq!(gate.snapshot().running[0].kind, OperationKind::Shutdown);
        assert!(
            calls.load(Ordering::SeqCst) > before,
            "observers learn about the new step"
        );
        drop(guard);
    }

    #[test]
    fn expected_duration_is_reported_in_the_queue() {
        let gate = leak();
        let guard = gate.device("Working").unwrap();
        guard.expect_within(Duration::from_secs(120));
        assert_eq!(gate.snapshot().running[0].expected_ms, Some(120_000));
        drop(guard);
    }

    #[test]
    fn kinds_are_known_while_waiting_and_serialize_camel_case() {
        let gate = leak();
        let first = gate
            .kind(OperationKind::Lifecycle)
            .computer("id-a", "a", "Start a")
            .unwrap();
        let waiter = thread::spawn(move || {
            drop(
                gate.kind(OperationKind::CheckpointCapture)
                    .computer("id-a", "a", "Creating checkpoint")
                    .unwrap(),
            );
        });
        wait_until(gate, |queue| queue.waiting.len() == 1);
        let queue = gate.snapshot();
        assert_eq!(queue.running[0].kind, OperationKind::Lifecycle);
        assert_eq!(queue.waiting[0].kind, OperationKind::CheckpointCapture);
        let json = serde_json::to_value(&queue).unwrap();
        assert_eq!(json["running"][0]["kind"], "lifecycle");
        assert_eq!(json["waiting"][0]["kind"], "checkpointCapture");
        drop(first);
        waiter.join().unwrap();
        let other = gate.device("Anything").unwrap();
        assert_eq!(
            serde_json::to_value(gate.snapshot()).unwrap()["running"][0]["kind"],
            "other"
        );
        drop(other);
        for (kind, name) in [
            (OperationKind::CheckpointRestore, "checkpointRestore"),
            (OperationKind::CheckpointFork, "checkpointFork"),
            (OperationKind::CheckpointDelete, "checkpointDelete"),
            (OperationKind::StorageReclaim, "storageReclaim"),
            (OperationKind::GithubApply, "githubApply"),
            (OperationKind::PortPublish, "portPublish"),
            (OperationKind::PortRemove, "portRemove"),
            (OperationKind::Shutdown, "shutdown"),
            (
                OperationKind::ComputerConfiguration,
                "computerConfiguration",
            ),
        ] {
            assert_eq!(serde_json::to_value(kind).unwrap(), name);
        }
    }

    #[test]
    fn generation_changes_when_an_operation_is_queued_runs_or_finishes() {
        let gate = leak();
        let start = gate.generation("id-a");
        let first = gate.computer("id-a", "a", "Start a").unwrap();
        let running = gate.generation("id-a");
        assert_ne!(running, start);
        assert_eq!(gate.generation("id-b"), start.min(gate.generation("id-b")));
        drop(first);
        assert_ne!(gate.generation("id-a"), running);
        let settled = gate.generation("id-a");
        assert_eq!(
            gate.generation("id-a"),
            settled,
            "reading is stable while idle"
        );
    }

    #[test]
    fn a_computer_is_quiet_unless_visible_work_other_than_the_callers_own_touches_it() {
        let gate = leak();
        assert!(gate.is_computer_quiet("id-a"));
        let hidden = gate.try_device_hidden("Cleaning up expired logs").unwrap();
        assert!(
            elsewhere(move || gate.is_computer_quiet("id-a")),
            "hidden housekeeping never settles a computer"
        );
        drop(hidden);
        let own = gate.computer("id-a", "a", "Creating checkpoint").unwrap();
        assert!(
            gate.is_computer_quiet("id-a"),
            "a thread reading after its own change"
        );
        assert!(
            !elsewhere(move || gate.is_computer_quiet("id-a")),
            "other readers see the computer busy"
        );
        assert!(
            elsewhere(move || gate.is_computer_quiet("id-b")),
            "other computers stay quiet"
        );
        drop(own);
        let change = gate.device("Applying computer changes").unwrap();
        assert!(gate.is_computer_quiet("id-b"));
        assert!(
            !elsewhere(move || gate.is_computer_quiet("id-b")),
            "device-wide work touches every computer"
        );
        drop(change);
    }

    #[test]
    fn a_device_wide_operation_touches_every_computer_and_hidden_ones_touch_none() {
        let gate = leak();
        let before = gate.generation("id-a");
        drop(gate.device("Updating").unwrap());
        assert_ne!(gate.generation("id-a"), before);
        let other = gate.generation("id-b");
        let snapshot = gate.generations();
        drop(gate.try_computer("id-a", "a", "Sync a").unwrap());
        assert_eq!(gate.generation("id-b"), other);
        assert_ne!(gate.generation("id-a"), snapshot.of("id-a"));
        assert_eq!(gate.generation("id-b"), snapshot.of("id-b"));
        let stable = gate.generation("id-a");
        drop(gate.try_device_hidden("Reconciling SSH access").unwrap());
        drop(
            gate.try_computer_hidden("id-a", "a", "Reconciling ports on a")
                .unwrap(),
        );
        assert_eq!(gate.generation("id-a"), stable);
    }

    #[test]
    fn activity_wakes_observers_when_a_visible_operation_finishes() {
        let gate = leak();
        let seen = gate.activity();
        let guard = gate.computer("id-a", "a", "Start a").unwrap();
        assert_eq!(
            gate.wait_for_activity(seen, Duration::from_millis(20)),
            seen
        );
        let waiter = thread::spawn(move || gate.wait_for_activity(seen, Duration::from_secs(5)));
        thread::sleep(Duration::from_millis(20));
        drop(guard);
        assert_ne!(waiter.join().unwrap(), seen);
        let seen = gate.activity();
        drop(gate.try_device_hidden("Reconciling SSH access").unwrap());
        assert_eq!(gate.activity(), seen);
    }

    #[test]
    fn work_that_is_no_longer_wanted_leaves_the_queue_without_starting() {
        let gate = leak();
        let busy = gate.computer("id-a", "a", "Long work").unwrap();
        let wanted = Arc::new(AtomicBool::new(true));
        let condition = StartCondition::new({
            let wanted = wanted.clone();
            move || wanted.load(Ordering::SeqCst)
        });
        let waiter = {
            let condition = condition.clone();
            thread::spawn(move || {
                StartCondition::scope(Some(condition), || {
                    gate.computer("id-a", "a", "Remote start").map(drop)
                })
            })
        };
        wait_until(gate, |queue| queue.waiting.len() == 1);
        wanted.store(false, Ordering::SeqCst);
        assert_eq!(waiter.join().unwrap(), Err(GateError::Abandoned));
        assert!(condition.expired() && !condition.started());
        assert!(gate.snapshot().waiting.is_empty());
        drop(busy);
    }

    #[test]
    fn a_turn_that_arrives_after_the_condition_lapsed_is_refused() {
        let gate = leak();
        let busy = gate.computer("id-a", "a", "Long work").unwrap();
        let wanted = Arc::new(AtomicBool::new(true));
        let condition = StartCondition::new({
            let wanted = wanted.clone();
            move || wanted.load(Ordering::SeqCst)
        });
        let waiter = {
            let condition = condition.clone();
            thread::spawn(move || {
                StartCondition::scope(Some(condition), || {
                    gate.computer("id-a", "a", "Remote start").map(drop)
                })
            })
        };
        wait_until(gate, |queue| queue.waiting.len() == 1);
        // Hold the gate's lock so the release and the lapse land before the waiter looks.
        {
            let _state = gate.lock();
            wanted.store(false, Ordering::SeqCst);
        }
        drop(busy);
        assert_eq!(waiter.join().unwrap(), Err(GateError::Abandoned));
        assert!(condition.expired() && !condition.started());
        // An immediately free turn is refused as well.
        assert_eq!(
            StartCondition::scope(Some(StartCondition::new(|| false)), || gate
                .computer("id-b", "b", "Remote start")
                .map(drop)),
            Err(GateError::Abandoned)
        );
    }

    #[test]
    fn an_explicit_wait_predicate_does_not_bypass_an_expired_start_condition() {
        let gate = leak();
        let condition = StartCondition::new(|| false);
        let result = StartCondition::scope(Some(condition.clone()), || {
            gate.acquire_while(Scope::Device, None, "Remote change", &|| true)
                .map(drop)
        });
        assert_eq!(result, Err(GateError::Abandoned));
        assert!(condition.expired());
        assert!(!condition.started());
        assert!(gate.is_idle());
    }

    #[test]
    fn explicit_waits_stop_when_the_remote_start_condition_expires() {
        let gate = leak();
        let busy = gate.computer("id-a", "a", "Long work").unwrap();
        let wanted = Arc::new(AtomicBool::new(true));
        let condition = StartCondition::new({
            let wanted = wanted.clone();
            move || wanted.load(Ordering::SeqCst)
        });
        let waiter = {
            let condition = condition.clone();
            thread::spawn(move || {
                StartCondition::scope(Some(condition), || {
                    gate.acquire_while(Scope::Device, None, "Remote change", &|| true)
                        .map(drop)
                })
            })
        };
        wait_until(gate, |queue| queue.waiting.len() == 1);
        wanted.store(false, Ordering::SeqCst);
        assert_eq!(waiter.join().unwrap(), Err(GateError::Abandoned));
        assert!(condition.expired() && !condition.started());
        assert!(gate.snapshot().waiting.is_empty());
        drop(busy);
    }

    #[test]
    fn abandoning_an_explicit_wait_does_not_expire_a_valid_remote_request() {
        let gate = leak();
        let busy = gate.computer("id-a", "a", "Long work").unwrap();
        let condition = StartCondition::new(|| true);
        let waiter = {
            let condition = condition.clone();
            thread::spawn(move || {
                StartCondition::scope(Some(condition), || {
                    gate.acquire_while(Scope::Device, None, "Remote change", &|| false)
                        .map(drop)
                })
            })
        };
        assert_eq!(waiter.join().unwrap(), Err(GateError::Abandoned));
        assert!(!condition.expired() && !condition.started());
        assert!(gate.snapshot().waiting.is_empty());
        drop(busy);
    }

    #[test]
    fn started_work_and_other_threads_wait_normally() {
        let gate = leak();
        let wanted = Arc::new(AtomicBool::new(true));
        let condition = StartCondition::new({
            let wanted = wanted.clone();
            move || wanted.load(Ordering::SeqCst)
        });
        StartCondition::scope(Some(condition.clone()), || {
            drop(gate.computer("id-a", "a", "Attempt 1").unwrap());
            assert!(condition.started());
            // A later attempt of started work waits its turn even after the condition lapsed.
            wanted.store(false, Ordering::SeqCst);
            let (held, release) = (mpsc::channel(), mpsc::channel::<()>());
            let other = thread::spawn(move || {
                let guard = gate.computer("id-a", "a", "Other").unwrap();
                held.0.send(()).unwrap();
                release.1.recv().unwrap();
                drop(guard);
            });
            held.1.recv().unwrap();
            let attempt = thread::scope(|scope| {
                let attempt = scope.spawn(|| {
                    StartCondition::scope(Some(condition.clone()), || {
                        gate.computer("id-a", "a", "Attempt 2").map(drop)
                    })
                });
                wait_until(gate, |queue| queue.waiting.len() == 1);
                release.0.send(()).unwrap();
                attempt.join().unwrap()
            });
            other.join().unwrap();
            assert_eq!(attempt, Ok(()));
        });
        assert!(!condition.expired());
        // The condition applies only inside its scope.
        assert!(StartCondition::current().is_none());
        drop(gate.computer("id-a", "a", "Local work").unwrap());
    }

    #[test]
    fn a_waiting_step_continues_when_another_worker_has_started_the_same_request() {
        let gate = leak();
        let busy = gate.computer("id-a", "a", "Long work").unwrap();
        let wanted = Arc::new(AtomicBool::new(true));
        let condition = StartCondition::new({
            let wanted = wanted.clone();
            move || wanted.load(Ordering::SeqCst)
        });
        let waiter = {
            let condition = condition.clone();
            thread::spawn(move || {
                StartCondition::scope(Some(condition), || {
                    gate.computer("id-a", "a", "Request step A").map(drop)
                })
            })
        };
        wait_until(gate, |queue| queue.waiting.len() == 1);
        let other_step = {
            let condition = condition.clone();
            thread::spawn(move || {
                StartCondition::scope(Some(condition), || {
                    gate.computer("id-b", "b", "Request step B").map(drop)
                })
            })
        };
        assert_eq!(other_step.join().unwrap(), Ok(()));
        assert!(condition.started());
        wanted.store(false, Ordering::SeqCst);
        drop(busy);
        assert_eq!(waiter.join().unwrap(), Ok(()));
        assert!(!condition.expired());
        assert!(gate.is_idle());
    }

    #[test]
    fn worker_threads_inherit_the_start_condition() {
        let condition = StartCondition::new(|| true);
        let original = condition.clone();
        let inherited = StartCondition::scope(Some(condition), || {
            tauri::async_runtime::block_on(spawn_blocking(move || {
                StartCondition::current().map(|current| Arc::ptr_eq(&current.0, &original.0))
            }))
            .unwrap()
        });
        assert_eq!(inherited, Some(true));
        let unscoped =
            tauri::async_runtime::block_on(spawn_blocking(|| StartCondition::current().is_some()))
                .unwrap();
        assert!(!unscoped);
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn operation_queue_matches_wire_contract() {
        use OperationKind::*;
        let kinds = [
            Lifecycle,
            CheckpointCapture,
            CheckpointRestore,
            CheckpointFork,
            CheckpointDelete,
            Export,
            Import,
            StorageReclaim,
            GithubApply,
            Push,
            PortPublish,
            PortRemove,
            Shutdown,
            ComputerConfiguration,
            Other,
        ];
        let entries: Vec<_> = kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| OperationEntry {
                id: index as u64 + 1,
                label: format!("Contract operation {}", index + 1),
                kind,
                computer_id: (kind != Shutdown)
                    .then(|| "00000000-0000-4000-8000-000000000001".into()),
                computer_name: (kind != Shutdown).then(|| "dev".into()),
                since_ms: 1767225600000,
                cancellable: index % 2 == 0,
                expected_ms: (index % 2 == 0).then_some(180000),
                blocked_by_hidden: false,
            })
            .collect();
        let queue = OperationQueue {
            running: entries,
            waiting: vec![OperationEntry {
                id: 16,
                label: "Waiting contract operation".into(),
                kind: Lifecycle,
                computer_id: Some("00000000-0000-4000-8000-000000000002".into()),
                computer_name: Some("other".into()),
                since_ms: 1767225601000,
                cancellable: true,
                expected_ms: None,
                blocked_by_hidden: true,
            }],
        };
        crate::runtime::contract_tests::assert_fixture(
            "operation-queue.json",
            vec![OperationQueue::default(), queue],
        );
    }
}
