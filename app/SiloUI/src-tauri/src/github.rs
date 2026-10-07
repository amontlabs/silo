//! Host-only GitHub account and durable desired policy. No credential is exposed by a command.
#[path = "github_personal_token.rs"]
pub(crate) mod personal_token;

use crate::github_tokens::{Configuration, Operation};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Condvar, Mutex, MutexGuard, OnceLock, PoisonError,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};

static OPERATION: Mutex<()> = Mutex::new(());
// Serializes whole OAuth connection flows, including their waits for the browser.
static CONNECTION_FLOW: Mutex<()> = Mutex::new(());
// Never hold this lock during a GitHub request. It orders desired saves
// and local profile attachment so an older network result cannot restore access.
static STATE: Mutex<()> = Mutex::new(());
// Mutex poison policy (K-24): these serialization locks guard no data, and GitHub
// state is re-read from disk after taking them, so one panic must not disable GitHub,
// or block app updates through `update_guard`, until restart.
fn serialize(lock: &'static Mutex<()>) -> MutexGuard<'static, ()> {
    crate::sync::lock_or_recover(lock, "GitHub")
}
fn try_serialize(lock: &'static Mutex<()>) -> Option<MutexGuard<'static, ()>> {
    crate::sync::try_lock_or_recover(lock, "GitHub")
}
/// Runtime work that can take minutes (a guest command that may boot the computer, or
/// `msb modify`) never holds STATE, so saves, Disable access, disconnect, cancel and
/// forks are not held behind it. `check` runs under STATE and returns `None` when the
/// work is no longer current; the caller re-takes STATE and re-checks before recording
/// the result. The runtime's per-computer revision lock rejects an older attach that arrives
/// after a newer one.
fn outside_state<P, W>(
    check: impl FnOnce() -> Result<Option<P>, String>,
    work: impl FnOnce(P) -> W,
) -> Result<Option<W>, String> {
    let prepared = {
        let _state = serialize(&STATE);
        check()?
    };
    Ok(prepared.map(work))
}
static ACTIVE: OnceLock<Mutex<std::collections::HashMap<String, Vec<RuntimeGrant>>>> =
    OnceLock::new();
#[derive(Clone)]
struct IssuedToken {
    owner: u64,
    all: bool,
    write: bool,
    ids: Vec<u64>,
    token: String,
    expires_at: u64,
}
static ISSUED: OnceLock<Mutex<std::collections::HashMap<String, Vec<IssuedToken>>>> =
    OnceLock::new();
fn issued() -> &'static Mutex<std::collections::HashMap<String, Vec<IssuedToken>>> {
    ISSUED.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}
fn issued_matches(token: &IssuedToken, scope: &GrantScope, write: bool) -> bool {
    token.owner == scope.owner
        && token.all == scope.all
        && token.write == write
        && (scope.all
            || token.ids
                == if write {
                    scope.writes.clone()
                } else {
                    scope.ids.clone()
                })
        && token.expires_at > now() + 120
}
static PENDING: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
fn schedule(delay: Duration) {
    if let Ok(mut pending) = PENDING.get_or_init(|| Mutex::new(None)).lock() {
        *pending = Some(Instant::now() + delay);
    }
    wake_worker();
}
// The worker sleeps until its next deadline instead of polling; `schedule` wakes it.
static WORKER_WOKEN: Mutex<bool> = Mutex::new(false);
static WORKER_WAKE: Condvar = Condvar::new();
/// Longest worker sleep: bounds deadlines measured on the wall clock (which can jump,
/// for example after the device sleeps) and ones not announced by `schedule`.
const WORKER_MAX_SLEEP: Duration = Duration::from_secs(60);
const WORKER_MIN_SLEEP: Duration = Duration::from_millis(100);
fn wake_worker() {
    *WORKER_WOKEN.lock().unwrap_or_else(PoisonError::into_inner) = true;
    WORKER_WAKE.notify_all();
}
fn worker_sleep(timeout: Duration) {
    worker_sleep_with(timeout, || {});
}

// The hook observes the locked sleep predicate immediately before the atomic wait.
fn worker_sleep_with(timeout: Duration, before_wait: impl FnOnce()) {
    let woken = WORKER_WOKEN.lock().unwrap_or_else(PoisonError::into_inner);
    before_wait();
    let (mut woken, _) = WORKER_WAKE
        .wait_timeout_while(woken, timeout, |woken| !*woken)
        .unwrap_or_else(PoisonError::into_inner);
    *woken = false;
}
static RESTORED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// A computer just started from a checkpoint; its GitHub settings must be applied once.
pub(crate) fn computer_restored(name: &str) {
    if let Ok(mut restored) = RESTORED.lock() {
        if !restored.iter().any(|n| n == name) {
            restored.push(name.into());
        }
    }
    schedule(Duration::ZERO);
}
fn take_restored(name: &str) -> bool {
    RESTORED.lock().is_ok_and(|mut restored| {
        let before = restored.len();
        restored.retain(|n| n != name);
        restored.len() != before
    })
}
fn active() -> &'static Mutex<std::collections::HashMap<String, Vec<RuntimeGrant>>> {
    ACTIVE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}
fn active_key(app: &tauri::AppHandle, computer: &str) -> Result<String, String> {
    Ok(format!("{}:{computer}", path(app)?.display()))
}
static SESSION: OnceLock<String> = OnceLock::new();
fn session() -> &'static str {
    SESSION.get_or_init(|| uuid::Uuid::new_v4().to_string())
}
static CONNECTING: AtomicBool = AtomicBool::new(false);
static CANCELLATION: AtomicU64 = AtomicU64::new(0);
// Browser URLs contain OAuth state. Keep them in memory and never return them to the UI.
#[derive(Default)]
struct PendingAuthorization(Option<(u64, String)>);
impl PendingAuthorization {
    fn clear(&mut self, generation: u64) {
        if self
            .0
            .as_ref()
            .is_some_and(|(active, _)| *active == generation)
        {
            self.0 = None;
        }
    }
    fn url(&self, generation: u64) -> Result<String, String> {
        self.0
            .as_ref()
            .filter(|(active, _)| *active == generation)
            .map(|(_, url)| url.clone())
            .ok_or_else(|| "No browser authorization is waiting. Connect GitHub again.".into())
    }
}
static AUTHORIZATION: Mutex<PendingAuthorization> = Mutex::new(PendingAuthorization(None));
// Policy edits keep their submission order even if spawn_blocking starts its
// jobs out of order. This queue contains local updates only, never HTTP calls.
struct IntentQueue {
    issued: AtomicU64,
    turn: Mutex<Turn>,
    ready: Condvar,
}
/// The ticket whose turn it is, and tickets given up before their turn came.
struct Turn {
    next: u64,
    abandoned: std::collections::BTreeSet<u64>,
}
impl IntentQueue {
    const fn new() -> Self {
        Self {
            issued: AtomicU64::new(0),
            turn: Mutex::new(Turn {
                next: 1,
                abandoned: std::collections::BTreeSet::new(),
            }),
            ready: Condvar::new(),
        }
    }
    fn ticket(&self) -> IntentTicket<'_> {
        IntentTicket {
            queue: self,
            number: Some(self.issued.fetch_add(1, Ordering::SeqCst) + 1),
        }
    }
    fn advance(&self, turn: &mut Turn) {
        turn.next += 1;
        while turn.abandoned.remove(&turn.next) {
            turn.next += 1;
        }
        self.ready.notify_all();
    }
}
/// A place in the queue. Dropping it without waiting (an early return or a panic
/// before its turn) gives the place up instead of blocking every later intent.
struct IntentTicket<'a> {
    queue: &'a IntentQueue,
    number: Option<u64>,
}
impl<'a> IntentTicket<'a> {
    // The queue position is plain data that stays valid after a panic elsewhere (K-24).
    fn wait(self) -> Result<IntentTurn<'a>, String> {
        self.wait_with(|| {})
    }

    fn wait_with(mut self, before_wait: impl FnOnce()) -> Result<IntentTurn<'a>, String> {
        let mut before_wait = Some(before_wait);
        let queue = self.queue;
        let ticket = self
            .number
            .take()
            .ok_or("GitHub settings queue is unavailable.")?;
        let mut turn = queue.turn.lock().unwrap_or_else(PoisonError::into_inner);
        while turn.next != ticket {
            if let Some(before_wait) = before_wait.take() {
                before_wait();
            }
            turn = queue
                .ready
                .wait(turn)
                .unwrap_or_else(PoisonError::into_inner);
        }
        Ok(IntentTurn(queue))
    }
}
impl Drop for IntentTicket<'_> {
    fn drop(&mut self) {
        let Some(ticket) = self.number else { return };
        let mut turn = self
            .queue
            .turn
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if turn.next == ticket {
            self.queue.advance(&mut turn);
        } else if turn.next < ticket {
            turn.abandoned.insert(ticket);
        }
    }
}
struct IntentTurn<'a>(&'a IntentQueue);
impl Drop for IntentTurn<'_> {
    fn drop(&mut self) {
        let mut turn = self.0.turn.lock().unwrap_or_else(PoisonError::into_inner);
        self.0.advance(&mut turn);
    }
}
static INTENTS: IntentQueue = IntentQueue::new();
struct Connecting(tauri::AppHandle, u64);
impl Drop for Connecting {
    fn drop(&mut self) {
        if let Ok(mut pending) = AUTHORIZATION.lock() {
            pending.clear(self.1);
            if pending.0.is_none() {
                CONNECTING.store(false, Ordering::SeqCst);
            }
        }
        let _ = self.0.emit("silo://application-state-changed", ());
    }
}
const CLIENT_SECRET: Option<&str> = option_env!("SILO_GITHUB_CLIENT_SECRET");
const CLIENT_ID: Option<&str> = option_env!("SILO_GITHUB_CLIENT_ID");
const APP_SLUG: Option<&str> = option_env!("SILO_GITHUB_APP_SLUG");

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Credential {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: u64,
}
// One serialized Keychain read per entry per app session. Cache denials too:
// background reconciliation must never reopen a dismissed permission dialog.
// A failed write keeps the new value in memory (a rotated credential must stay
// usable) and marks it unsaved; the store is retried by `flush`, never by `write`.
struct SessionSecret<T>(Mutex<SecretSlot<T>>, Mutex<Option<Result<T, String>>>);
struct SecretSlot<T> {
    value: Option<Result<T, String>>,
    unsaved: Option<String>,
    blocked: bool,
}
impl<T: Clone + PartialEq> SessionSecret<T> {
    const fn new() -> Self {
        Self(
            Mutex::new(SecretSlot {
                value: None,
                unsaved: None,
                blocked: false,
            }),
            Mutex::new(None),
        )
    }
    /// Mirror the cached value for `peek`; the mirror is never held across the store.
    fn publish(&self, value: &Option<Result<T, String>>) {
        *self.1.lock().unwrap_or_else(PoisonError::into_inner) = value.clone();
    }
    /// The value already known this session, without opening the credential store or
    /// waiting for a read or permission prompt in progress. `None` means not read yet.
    fn peek(&self) -> Option<Result<T, String>> {
        self.1
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    fn read(&self, read: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "Credential state is unavailable.")?;
        let value = state.value.get_or_insert_with(read).clone();
        self.publish(&state.value);
        value
    }
    fn write(&self, value: T, write: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "Credential state is unavailable.")?;
        self.write_locked(&mut state, value, write)
    }
    /// Explicit replacements become visible only after durable storage succeeds.
    fn replace(&self, value: T, write: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "Credential state is unavailable.")?;
        if let Some(Err(error)) = state.value.as_ref() {
            return Err(error.clone());
        }
        if state.unsaved.is_none()
            && matches!(state.value.as_ref(), Some(Ok(current)) if current == &value)
        {
            return Ok(());
        }
        write()?;
        state.value = Some(Ok(value));
        state.unsaved = None;
        state.blocked = false;
        self.publish(&state.value);
        Ok(())
    }
    fn update(
        &self,
        read: impl FnOnce() -> Result<T, String>,
        update: impl FnOnce(&mut T),
        write: impl FnOnce(&T) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "Credential state is unavailable.")?;
        let value = state.value.get_or_insert_with(read).clone();
        self.publish(&state.value);
        let mut value = value?;
        update(&mut value);
        self.write_locked(&mut state, value.clone(), || write(&value))
    }
    fn write_locked(
        &self,
        state: &mut SecretSlot<T>,
        value: T,
        write: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        if let Some(Err(error)) = state.value.as_ref() {
            return Err(error.clone());
        }
        if state.blocked {
            if let Some(error) = state.unsaved.clone() {
                // Keep the newest value usable in memory; `flush` stores it later.
                state.value = Some(Ok(value));
                self.publish(&state.value);
                return Err(error);
            }
        }
        if state.unsaved.is_none()
            && matches!(state.value.as_ref(), Some(Ok(current)) if current == &value)
        {
            return Ok(());
        }
        let result = write();
        state.value = Some(Ok(value));
        self.publish(&state.value);
        state.unsaved = result.clone().err();
        state.blocked = state.unsaved.is_some();
        result
    }
    /// Store an in-memory value whose earlier write failed. Returns whether storage is current.
    fn flush(&self, write: impl FnOnce(&T) -> Result<(), String>) -> Result<(), String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "Credential state is unavailable.")?;
        if state.unsaved.is_none() {
            return Ok(());
        }
        let Some(Ok(value)) = state.value.clone() else {
            return Ok(());
        };
        let result = write(&value);
        state.unsaved = result.clone().err();
        state.blocked = state.unsaved.is_some();
        result
    }
    fn retry(&self) {
        if let Ok(mut state) = self.0.lock() {
            if matches!(state.value.as_ref(), Some(Err(_))) {
                state.value = None;
            }
            state.blocked = false;
            self.publish(&state.value);
        }
    }
}
static ACCOUNT_SECRET: SessionSecret<Option<Credential>> = SessionSecret::new();
static LEDGER_SECRET: SessionSecret<TokenLedger> = SessionSecret::new();
fn retry_credential_access() {
    ACCOUNT_SECRET.retry();
    LEDGER_SECRET.retry();
}
// Snapshot reads never open the credential store or wait for its permission UI.
// This observation contains public lifetime/error metadata only, never a token.
type CredentialObservation = Option<Result<Option<u64>, String>>;
static CREDENTIAL_OBSERVATION: Mutex<CredentialObservation> = Mutex::new(None);
static OBSERVATION_APP: OnceLock<tauri::AppHandle> = OnceLock::new();
fn publish_credential_observation(value: Result<Option<u64>, String>) {
    if let Ok(mut observed) = CREDENTIAL_OBSERVATION.lock() {
        if observed.as_ref() == Some(&value) {
            return;
        }
        *observed = Some(value);
    }
    if let Some(app) = OBSERVATION_APP.get() {
        let _ = app.emit("silo://application-state-changed", ());
    }
}
fn observe_credential_read(
    read: impl FnOnce() -> Result<Option<Credential>, String>,
    publish: impl FnOnce(Result<Option<u64>, String>),
) -> Result<Option<Credential>, String> {
    let result = read();
    publish(
        result
            .as_ref()
            .map(|c| c.as_ref().map(observed_expiry))
            .map_err(Clone::clone),
    );
    result
}
/// An expired access token with a refresh token is renewed on next use, so it still
/// counts as connected; showing it as disconnected would push users to re-authorize.
fn observed_expiry(c: &Credential) -> u64 {
    if c.refresh_token.is_some() {
        u64::MAX
    } else {
        c.expires_at
    }
}
fn observed_credential() -> CredentialObservation {
    CREDENTIAL_OBSERVATION
        .lock()
        .map(|state| state.clone())
        .unwrap_or_else(|_| Some(Err("GitHub credential state is unavailable.".into())))
}
// Policy revisions cross IPC as JavaScript numbers and must remain distinguishable.
const MAX_POLICY_REVISION: u64 = 9_007_199_254_740_991;

fn next_policy_revision(revision: u64) -> Result<u64, String> {
    revision
        .checked_add(1)
        .filter(|next| *next <= MAX_POLICY_REVISION)
        .ok_or_else(|| "GitHub settings revision exceeds the supported range.".into())
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    #[serde(default)]
    personal_token_removing: bool,
    revision: u64,
    access_enabled: bool,
    account: Option<String>,
    #[serde(default)]
    computers: Vec<Value>,
    #[serde(default)]
    repositories: Vec<Value>,
    #[serde(default)]
    operations: Vec<Value>,
    #[serde(default)]
    session: String,
    #[serde(default)]
    refresh_at: u64,
    #[serde(default)]
    catalog_error: Option<String>,
    #[serde(default)]
    catalog_refresh_at: u64,
    #[serde(default)]
    grants_issued: bool,
    #[serde(default)]
    identity_errors: std::collections::HashMap<String, String>,
    #[serde(default)]
    identity_pending: Vec<String>,
    #[serde(default)]
    access_pending: Vec<String>,
    #[serde(default)]
    access_errors: std::collections::HashMap<String, String>,
    #[serde(default)]
    disconnect_pending: bool,
    /// Server-imposed waiting deadlines per GitHub rate class, kept across relaunch.
    #[serde(default)]
    rate_retry: std::collections::BTreeMap<String, u64>,
    /// Per computer name: which revision last changed its saved choices, and from which
    /// view. Kept after a policy is removed so a stale save cannot bring it back.
    #[serde(default)]
    policy_stamps: std::collections::BTreeMap<String, PolicyStamp>,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyStamp {
    revision: u64,
    /// The `policyRevision` the writer's view was based on; `None` for changes Silo made
    /// itself (a fork copying an assignment, a computer deletion).
    #[serde(default)]
    base: Option<u64>,
}
/// Record that `computer`'s choices changed in the document's current revision.
fn stamp(d: &mut Document, computer: &str, base: Option<u64>) {
    d.policy_stamps.insert(
        computer.into(),
        PolicyStamp {
            revision: d.revision,
            base,
        },
    );
    // Bound stamps of removed computers; the oldest are the least likely to be raced.
    while d.policy_stamps.len() > 256 {
        let oldest = d
            .policy_stamps
            .iter()
            .filter(|(name, _)| {
                !d.computers
                    .iter()
                    .any(|w| w["computer"].as_str() == Some(name.as_str()))
            })
            .min_by_key(|(_, stamp)| stamp.revision)
            .map(|(name, _)| name.clone());
        match oldest {
            Some(name) => d.policy_stamps.remove(&name),
            None => break,
        };
    }
}
/// Whether a save based on `base` would overwrite choices it never saw: a change Silo
/// made after that view (a fork's copied assignment, a deletion), or a save from a newer
/// view. Saves from the same or an older view (such as rapid edits sent before the page
/// saw the previous result) are the user's own ordered intent and apply in order.
fn stale_save(d: &Document, computer: &str, base: Option<u64>) -> bool {
    let Some(base) = base else { return false };
    d.policy_stamps
        .get(computer)
        .is_some_and(|stamp| stamp.revision > base && stamp.base.is_none_or(|writer| writer > base))
}

/// Copy the source's current GitHub assignment for a stopped checkpoint fork.
/// The child obtains its own runtime identity and resolves credentials at Start.
pub(crate) fn fork_assignment(
    app: &tauri::AppHandle,
    source: &str,
    target: &str,
) -> Result<(), String> {
    let _state = serialize(&STATE);
    let mut document = load(app)?;
    if let Some(mut assignment) = document
        .computers
        .iter()
        .find(|value| value["computer"].as_str() == Some(source))
        .cloned()
    {
        assignment["computer"] = json!(target);
        document
            .computers
            .retain(|value| value["computer"].as_str() != Some(target));
        document.computers.push(assignment);
        document.revision = next_policy_revision(document.revision)?;
        stamp(&mut document, target, None);
        if !document.access_pending.iter().any(|name| name == target) {
            document.access_pending.push(target.into());
        }
        if !document.identity_pending.iter().any(|name| name == target) {
            document.identity_pending.push(target.into());
        }
        save(app, &document)?;
    }
    Ok(())
}

pub(crate) fn forget_fork_assignment(app: &tauri::AppHandle, target: &str) -> Result<(), String> {
    let _state = serialize(&STATE);
    let mut document = load(app)?;
    forget_computer(&mut document, target);
    document.revision = next_policy_revision(document.revision)?;
    stamp(&mut document, target, None);
    save(app, &document)
}

/// Remove every saved choice and pending result for a computer. Returns whether any existed.
fn forget_computer(d: &mut Document, computer: &str) -> bool {
    let named = |value: &Value| value["computer"].as_str() == Some(computer);
    let existed = d.computers.iter().any(named)
        || d.operations.iter().any(named)
        || d.access_pending
            .iter()
            .chain(&d.identity_pending)
            .any(|name| name == computer)
        || d.access_errors.contains_key(computer)
        || d.identity_errors.contains_key(computer);
    d.computers.retain(|value| !named(value));
    d.operations.retain(|value| !named(value));
    d.access_pending.retain(|name| name != computer);
    d.identity_pending.retain(|name| name != computer);
    d.access_errors.remove(computer);
    d.identity_errors.remove(computer);
    existed
}

/// A computer was deleted. GitHub choices are keyed by computer name, so a new computer
/// reusing the name must not inherit its repository or write access: remove its policy
/// and pending work, drop its cached attachments, and let the worker revoke the tokens
/// issued to it (they stay in the retirement ledger until GitHub confirms).
pub(crate) fn computer_removed(computer: &str) -> Result<(), String> {
    let Some(document) = document_path() else {
        return Ok(());
    };
    {
        let _state = serialize(&STATE);
        let mut d = load_at(&document)?;
        if forget_computer(&mut d, computer) {
            d.revision = next_policy_revision(d.revision)?;
            stamp(&mut d, computer, None);
            save_at(&document, &d)?;
        }
    }
    let key = format!("{}:{computer}", document.display());
    active()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&key);
    issued()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&key);
    personal_token::forget(&key);
    RESTORED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|name| name != computer);
    if let Some(app) = OBSERVATION_APP.get() {
        let _ = app.emit("silo://application-state-changed", ());
    }
    schedule(Duration::ZERO);
    Ok(())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn delete_account_credential() -> Result<(), String> {
    match ACCOUNT_SECRET.write(None, || match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Cannot remove GitHub credentials from the system credential store.".into()),
    }) {
        Ok(()) => {
            *PENDING_REFRESH
                .lock()
                .map_err(|_| "GitHub credential state is unavailable.")? = None;
            publish_credential_observation(Ok(None));
            Ok(())
        }
        Err(_) => {
            let message =
                "Cannot remove GitHub credentials from the system credential store.".to_string();
            publish_credential_observation(Err(message.clone()));
            Err(message)
        }
    }
}
fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(
        crate::channel::current().keychain_service(crate::channel::Keychain::Github),
        "account",
    )
    .map_err(|_| "The system credential store is unavailable.".into())
}
fn credential() -> Result<Option<Credential>, String> {
    observe_credential_read(
        || ACCOUNT_SECRET.read(|| read_entry(&entry()?)),
        publish_credential_observation,
    )
}
fn read_entry(entry: &keyring::Entry) -> Result<Option<Credential>, String> {
    match entry.get_password() {
        Ok(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|_| "Stored GitHub credentials are invalid. Reconnect GitHub.".into()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("Cannot read GitHub credentials from the system credential store.".into()),
    }
}
fn store(c: &Credential) -> Result<(), String> {
    let result = ACCOUNT_SECRET.write(Some(c.clone()), || {
        entry().and_then(|entry| store_entry(&entry, c))
    });
    publish_credential_observation(
        result
            .as_ref()
            .map(|_| Some(observed_expiry(c)))
            .map_err(Clone::clone),
    );
    result
}
fn replace_connection_credential(
    secret: &SessionSecret<Option<Credential>>,
    credential: &Credential,
    persist: impl FnOnce() -> Result<(), String>,
    publish: impl FnOnce(Result<Option<u64>, String>),
) -> Result<(), String> {
    secret.replace(Some(credential.clone()), persist)?;
    publish(Ok(Some(observed_expiry(credential))));
    Ok(())
}
/// Retry storing a credential whose earlier write failed (for example a renewed
/// credential after a refresh), at most every 15 minutes so a denied Keychain prompt
/// is not reopened in a loop. Until then the renewed credential is used in memory.
fn flush_account_credential() {
    static FLUSH_AT: AtomicU64 = AtomicU64::new(0);
    if now() < FLUSH_AT.load(Ordering::SeqCst) {
        return;
    }
    FLUSH_AT.store(now() + 900, Ordering::SeqCst);
    let _ = ACCOUNT_SECRET.flush(|credential| match credential {
        Some(c) => entry().and_then(|entry| store_entry(&entry, c)),
        None => match entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => {
                Err("Cannot remove GitHub credentials from the system credential store.".into())
            }
        },
    });
}
fn store_entry(entry: &keyring::Entry, c: &Credential) -> Result<(), String> {
    entry
        .set_password(&serde_json::to_string(c).map_err(|_| "Cannot encode GitHub credentials.")?)
        .map_err(|_| "Cannot save GitHub credentials in the system credential store.".into())
}
fn path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|_| "Cannot locate application data.")?
        .join("github.json"))
}
#[cfg(test)]
thread_local! {
    static TEST_DOCUMENT: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}
/// Tests on this thread use `path` as the GitHub document for app-less entry points.
#[cfg(test)]
pub(crate) fn use_test_document(path: Option<PathBuf>) {
    TEST_DOCUMENT.with(|test| *test.borrow_mut() = path);
}
/// The GitHub document for entry points without an app handle (the runtime's delete path).
/// `None` before `install`, when there is no GitHub state to change.
fn document_path() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(path) = TEST_DOCUMENT.with(|test| test.borrow().clone()) {
        return Some(path);
    }
    OBSERVATION_APP.get().and_then(|app| path(app).ok())
}
fn load(app: &tauri::AppHandle) -> Result<Document, String> {
    load_at(&path(app)?)
}
type FileIdentity = (u64, Option<std::time::SystemTime>, u64, i64, i64);
static OBSERVED: Mutex<Option<(std::path::PathBuf, FileIdentity, Document)>> = Mutex::new(None);
fn file_identity(path: &std::path::Path) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path).ok()?;
    Some((
        metadata.len(),
        metadata.modified().ok(),
        metadata.ino(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    ))
}
/// The saved configuration for a state refresh: reparsed only when the file changed (or
/// was saved by this process), since every refresh reads it.
fn load_observed(path: &std::path::Path) -> Result<Document, String> {
    let identity = file_identity(path);
    let mut observed = OBSERVED.lock().unwrap_or_else(PoisonError::into_inner);
    if let (Some(identity), Some((cached_path, cached_identity, document))) =
        (identity, observed.as_ref())
    {
        if cached_path == path && *cached_identity == identity {
            return Ok(document.clone());
        }
    }
    let document = load_at(path)?;
    *observed = identity.map(|identity| (path.to_path_buf(), identity, document.clone()));
    Ok(document)
}
const MAX_CONFIGURATION_BYTES: usize = 16 * 1024 * 1024;
fn load_at(path: &std::path::Path) -> Result<Document, String> {
    match fs::File::open(path) {
        Ok(file) => read_configuration(file),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Document::default()),
        Err(_) => Err("Cannot read GitHub configuration.".into()),
    }
}
fn read_configuration(reader: impl Read) -> Result<Document, String> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_CONFIGURATION_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read GitHub configuration.")?;
    if bytes.len() > MAX_CONFIGURATION_BYTES {
        return Err("GitHub configuration exceeds the supported size.".into());
    }
    let document: Document =
        serde_json::from_slice(&bytes).map_err(|_| "GitHub configuration is invalid.")?;
    if document.revision > MAX_POLICY_REVISION {
        return Err("GitHub settings revision exceeds the supported range.".into());
    }
    Ok(document)
}
fn save(app: &tauri::AppHandle, d: &Document) -> Result<(), String> {
    save_at(&path(app)?, d)?;
    let _ = app.emit("silo://application-state-changed", ());
    Ok(())
}
fn save_at(p: &std::path::Path, d: &Document) -> Result<(), String> {
    let mut saved = d.clone();
    for (class, until) in crate::github_http::retry_floors() {
        let floor = saved.rate_retry.entry(class).or_default();
        *floor = (*floor).max(until);
    }
    let at = now();
    saved.rate_retry.retain(|_, until| *until > at);
    let encoded = serde_json::to_vec(&saved).map_err(|_| "Cannot encode GitHub configuration.")?;
    if encoded.len() > MAX_CONFIGURATION_BYTES {
        return Err("GitHub configuration exceeds the supported size.".into());
    }
    let parent = p.parent().ok_or("Missing configuration directory.")?;
    fs::create_dir_all(parent).map_err(|_| "Cannot create configuration directory.")?;
    let mut f = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "Cannot write GitHub configuration.")?;
    f.write_all(&encoded)
        .map_err(|_| "Cannot write GitHub configuration.")?;
    f.as_file()
        .sync_all()
        .map_err(|_| "Cannot sync GitHub configuration.")?;
    *OBSERVED.lock().unwrap_or_else(PoisonError::into_inner) = None;
    f.persist(p)
        .map_err(|_| "Cannot save GitHub configuration.")?;
    *OBSERVED.lock().unwrap_or_else(PoisonError::into_inner) = None;
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|_| "Cannot sync GitHub configuration directory.".to_string())?;
    Ok(())
}
fn token_configuration() -> Result<Configuration, String> {
    Ok(Configuration {
        client_id: CLIENT_ID
            .filter(|v| !v.is_empty())
            .ok_or("GitHub connection is not configured in this build.")?
            .into(),
        client_secret: CLIENT_SECRET
            .filter(|v| !v.is_empty())
            .ok_or("GitHub connection is not configured in this build.")?
            .into(),
    })
}
fn token_operation(operation: Operation, body: Value) -> Result<Value, String> {
    crate::github_tokens::execute(&token_configuration()?, operation, body)
}
fn from_response(v: Value) -> Result<Credential, String> {
    let token = v["accessToken"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("GitHub returned no access credential.")?;
    Ok(Credential {
        access_token: token.into(),
        refresh_token: v["refreshToken"].as_str().map(str::to_owned),
        expires_at: now()
            .checked_add(
                v["expiresIn"]
                    .as_u64()
                    .filter(|n| *n > 0 && *n <= 86400)
                    .ok_or("GitHub returned an invalid credential lifetime.")?,
            )
            .ok_or("GitHub credential expiration overflow.")?,
    })
}
struct PendingRefresh {
    previous_access: String,
    renewed: Credential,
}
static PENDING_REFRESH: Mutex<Option<PendingRefresh>> = Mutex::new(None);

fn revocation_credential() -> Result<Option<Credential>, String> {
    let current = credential()?;
    let pending = PENDING_REFRESH
        .lock()
        .map_err(|_| "GitHub credential state is unavailable.")?;
    Ok(current.map(|current| {
        pending
            .as_ref()
            .filter(|p| p.previous_access == current.access_token)
            .map_or(current, |p| p.renewed.clone())
    }))
}

/// Whether two GitHub logins name the same account. One account has one authorization
/// (grant) for the App, shared by every credential issued to it.
fn same_account(previous: Option<&str>, login: &str) -> bool {
    previous.is_some_and(|previous| previous.eq_ignore_ascii_case(login))
}
/// Revoking a whole authorization also revokes the stored credential of the same
/// account, so a credential that shares it is revoked alone.
fn discard_operation(shares_grant: bool) -> Operation {
    if shares_grant {
        Operation::RevokeToken
    } else {
        Operation::RevokeAuthorization
    }
}
fn discard_credential(c: &Credential, shares_grant: bool) {
    let _ = token_operation(
        discard_operation(shares_grant),
        json!({"accessToken":c.access_token}),
    );
}
/// A credential from a completed code exchange that Silo has not kept. Dropping it (any
/// failure or cancellation after the exchange) revokes it, so a new authorization is
/// never left live without Silo knowing it.
struct Unstored {
    credential: Option<Credential>,
    /// Whether the stored credential may belong to the same account (and grant).
    shares_grant: bool,
    revoke: fn(&Credential, bool),
}
impl Unstored {
    fn new(credential: Credential, shares_grant: bool) -> Self {
        Self {
            credential: Some(credential),
            shares_grant,
            revoke: discard_credential,
        }
    }
    fn kept(&mut self) {
        self.credential = None;
    }
}
impl Drop for Unstored {
    fn drop(&mut self) {
        if let Some(credential) = self.credential.take() {
            (self.revoke)(&credential, self.shares_grant);
        }
    }
}
/// An access token that can still act for `c`'s authorization: the current one, or a
/// renewed one when it expired (GitHub answers 404 for an expired token, which revocation
/// would take for success). `None` when nothing can act for it any more: no refresh token,
/// or GitHub rejected the renewal because the authorization is already gone.
fn live_access_token(
    c: &Credential,
    at: u64,
    renew: impl FnOnce(&str) -> Result<Credential, String>,
) -> Result<Option<String>, String> {
    if c.expires_at > at + 120 {
        return Ok(Some(c.access_token.clone()));
    }
    let Some(refresh) = c.refresh_token.as_deref() else {
        return Ok(None);
    };
    match renew(refresh) {
        Ok(renewed) => Ok(Some(renewed.access_token)),
        Err(error) if crate::github_http::authorization_rejected(&error) => Ok(None),
        Err(error) => Err(error),
    }
}
/// The token that revokes the account's authorization on Disconnect. An expired token is
/// renewed first through `active` (which stores the renewal, so a failed revocation can be
/// retried with it): GitHub answers 404 for an expired token, which revocation would take
/// for success while the authorization and its refresh token stay live.
fn disconnect_token(
    current: Option<Credential>,
    at: u64,
    active: impl FnOnce() -> Result<Credential, String>,
) -> Result<Option<String>, String> {
    let Some(current) = current else {
        return Ok(None);
    };
    live_access_token(&current, at, |_| active())
}
/// Revoke the credential a reconnect replaced (best effort). The same account's new
/// credential shares its authorization, so only the old token is revoked then; another
/// account's authorization is revoked entirely, renewing an expired token first.
fn revoke_replaced(old: &Credential, same_account: bool) {
    let _ = if same_account {
        if old.expires_at <= now() {
            return;
        }
        token_operation(
            Operation::RevokeToken,
            json!({"accessToken":old.access_token}),
        )
        .map(|_| ())
    } else {
        live_access_token(old, now(), |refresh| {
            from_response(token_operation(
                Operation::Refresh,
                json!({"refreshToken":refresh}),
            )?)
        })
        .and_then(|token| {
            token.map_or(Ok(()), |token| {
                token_operation(Operation::RevokeAuthorization, json!({"accessToken":token}))
                    .map(|_| ())
            })
        })
    };
}

fn active_credential() -> Result<Credential, String> {
    let current = credential()?.ok_or("Connect GitHub first.")?;
    let mut pending = PENDING_REFRESH
        .lock()
        .map_err(|_| "GitHub credential state is unavailable.")?;
    refresh_credential_with(
        current,
        &mut pending,
        now(),
        |refresh| {
            from_response(token_operation(
                Operation::Refresh,
                json!({"refreshToken":refresh}),
            )?)
        },
        store,
    )
}

fn refresh_credential_with(
    current: Credential,
    pending: &mut Option<PendingRefresh>,
    at: u64,
    renew: impl FnOnce(&str) -> Result<Credential, String>,
    mut persist: impl FnMut(&Credential) -> Result<(), String>,
) -> Result<Credential, String> {
    if pending
        .as_ref()
        .is_some_and(|p| p.previous_access != current.access_token)
    {
        *pending = None;
    }
    if let Some(refresh) = pending.as_ref() {
        // GitHub already rotated the credential. Retry secure storage only,
        // never submit the consumed refresh token to GitHub a second time.
        persist(&refresh.renewed)?;
        return Ok(pending.take().unwrap().renewed);
    }
    if current.expires_at > at + 120 {
        return Ok(current);
    }
    let token = current
        .refresh_token
        .as_deref()
        .ok_or("GitHub access expired. Reconnect GitHub.")?;
    let renewed = renew(token)?;
    // GitHub consumed the old refresh token. Use the renewed credential even when
    // secure storage fails; `pending` keeps it so storage is retried later.
    if persist(&renewed).is_err() {
        *pending = Some(PendingRefresh {
            previous_access: current.access_token,
            renewed: renewed.clone(),
        });
    }
    Ok(renewed)
}

fn github(token: &str, path: &str) -> Result<Value, String> {
    crate::github_http::github(token, path)
}
fn catalog(c: &Credential) -> Result<Vec<Value>, String> {
    Ok(catalog_installations(c)?.0)
}
fn catalog_with_retry(
    c: &Credential,
    fetch: impl FnOnce(&Credential) -> Result<Vec<Value>, String>,
) -> Result<Vec<Value>, String> {
    crate::github_http::reset_catalog_retries(&c.access_token);
    fetch(c)
}
fn catalog_installations(c: &Credential) -> Result<(Vec<Value>, bool), String> {
    let slug = APP_SLUG.ok_or("GitHub App is not configured in this build.")?;
    let mut repos = Vec::new();
    let mut installed = false;
    for page in 1..=1000 {
        let v = github(
            &c.access_token,
            &format!("/user/installations?per_page=100&page={page}"),
        )?;
        let items = v["installations"]
            .as_array()
            .ok_or("Invalid GitHub installation catalog.")?;
        for i in items
            .iter()
            .filter(|i| i["app_slug"].as_str() == Some(slug))
        {
            installed = true;
            let id = i["id"]
                .as_u64()
                .ok_or("Invalid GitHub installation identifier.")?;
            for p in 1..=1000 {
                let r = github(
                    &c.access_token,
                    &format!("/user/installations/{id}/repositories?per_page=100&page={p}"),
                )?;
                let list = r["repositories"]
                    .as_array()
                    .ok_or("Invalid GitHub repository catalog.")?;
                repos.extend(list.iter().map(
                    |r| json!({"id":r["id"],"ownerId":r["owner"]["id"],"name":r["full_name"]}),
                ));
                if list.len() < 100 {
                    break;
                }
                if p == 1000 {
                    return Err("GitHub repository catalog exceeds the supported size.".into());
                }
            }
        }
        if items.len() < 100 {
            repos.sort_by_key(|repo| repo["id"].as_u64().unwrap_or(0));
            return Ok((repos, installed));
        }
    }
    Err("GitHub installation catalog exceeds the supported size.".into())
}

/// The device Git identity without waiting for Git: snapshots are taken on every state
/// refresh and sometimes under GitHub locks, so they never spawn Git themselves.
fn device_identity() -> Option<crate::device_identity::DeviceIdentity> {
    crate::device_identity::cached(|| {
        if let Some(app) = OBSERVATION_APP.get() {
            let _ = app.emit("silo://application-state-changed", ());
        }
    })
}
pub fn snapshot(app: &tauri::AppHandle) -> Result<Value, String> {
    let mut value = observed_snapshot(
        load_observed(&path(app)?)?,
        observed_credential(),
        device_identity(),
    );
    // Saved choices for a computer that has no runtime yet are not a failure; they apply
    // once it starts.
    if let Some(operations) = value["computerOperations"].as_array_mut() {
        operations.retain(|op| {
            !op["computer"]
                .as_str()
                .is_some_and(|name| is_pending_restore(app, name))
        });
    }
    Ok(value)
}
fn observed_snapshot(
    document: Document,
    observed: CredentialObservation,
    identity: Option<crate::device_identity::DeviceIdentity>,
) -> Value {
    let waiting = observed.is_none();
    let mut value = public_snapshot(
        document,
        observed
            .unwrap_or_else(|| Err("Waiting for access to the system credential store.".into())),
        identity,
    );
    if waiting {
        value["repositoryCatalogStatus"]["canRetry"] = json!(false);
    }
    value
}
fn public_snapshot(
    mut d: Document,
    stored: Result<Option<u64>, String>,
    identity: Option<crate::device_identity::DeviceIdentity>,
) -> Value {
    let connected = match stored {
        Ok(expires_at) => expires_at.is_some_and(|expiry| expiry > now()),
        Err(message) => {
            d.catalog_error = Some(message);
            false
        }
    };
    if d.session != session() {
        d.operations = d.computers.iter().map(|w|json!({"computer":w["computer"],"status":"failed","message":"GitHub access must be verified for this app session.","canRetry":true})).collect();
    }

    json!({"policyRevision":d.revision,"personalToken":personal_token::status(),"state":if CONNECTING.load(Ordering::SeqCst){"connecting"}else if connected{"connected"}else{"disconnected"},"account":d.account,"accessEnabled":d.access_enabled,"deviceIdentity":identity,"repositoryCatalog":d.repositories.iter().filter_map(|r|r["name"].as_str()).collect::<Vec<_>>(),"repositoryCatalogStatus":match &d.catalog_error { Some(message)=>json!({"status":"unavailable","message":message,"canRetry":true}),None=>json!({"status":"available"})},"computers":d.computers,"computerOperations":d.operations})
}
type TokenLedger = std::collections::HashMap<String, Vec<String>>;
fn ledger_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(
        crate::channel::current().keychain_service(crate::channel::Keychain::Github),
        "runtime-grants",
    )
    .map_err(|_| "The system credential store is unavailable.".into())
}
fn read_ledger(entry: &keyring::Entry) -> Result<TokenLedger, String> {
    match entry.get_password() {
        Ok(secret) => serde_json::from_str(&secret)
            .map_err(|_| "Stored GitHub runtime credentials are invalid.".into()),
        Err(keyring::Error::NoEntry) => Ok(TokenLedger::new()),
        Err(_) => {
            Err("Cannot read GitHub runtime credentials from the system credential store.".into())
        }
    }
}
fn save_ledger(entry: &keyring::Entry, ledger: &TokenLedger) -> Result<(), String> {
    entry
        .set_password(
            &serde_json::to_string(ledger)
                .map_err(|_| "Cannot encode GitHub runtime credentials.")?,
        )
        .map_err(|_| {
            "Cannot save GitHub runtime credentials in the system credential store.".into()
        })
}
// Record each successful issuance before making another network request. A
// later partial failure must not lose the only copy needed for revocation.
fn remember_token(app: &tauri::AppHandle, computer: &str, token: &str) -> Result<(), String> {
    let entry = ledger_entry()?;
    append_ledger_token(&LEDGER_SECRET, &entry, computer, token)?;
    let _state = serialize(&STATE);
    let mut document = load(app)?;
    document.grants_issued = true;
    save(app, &document)
}
fn profile(grants: &[RuntimeGrant]) -> Value {
    json!({"version":1,"owners":grants.iter().filter(|g| !g.read_token.is_empty()).map(|g|json!({"login":g.owner_login,"repositoryIds":g.repository_ids,"readToken":g.read_token,"writeToken":g.write_token,"expiresAt":g.expires_at})).collect::<Vec<_>>()})
}
fn retire_unused(app: &tauri::AppHandle, computer: &str) -> Result<(), String> {
    let key = active_key(app, computer)?;
    let retained: Vec<String> = active()
        .lock()
        .map_err(|_| "GitHub state is unavailable.")?
        .get(&key)
        .into_iter()
        .flatten()
        .flat_map(|g| std::iter::once(g.read_token.clone()).chain(g.write_token.clone()))
        .collect();
    let mut live = crate::runtime::scoped_cached_tokens(app, computer)?;
    live.extend(retained);
    let d = load(app)?;
    let scopes = d
        .computers
        .iter()
        .find(|w| w["computer"].as_str() == Some(computer))
        .and_then(|w| scopes(&d, w).ok())
        .unwrap_or_default();
    if let Some(tokens) = issued()
        .lock()
        .map_err(|_| "GitHub state is unavailable.")?
        .get_mut(&key)
    {
        tokens.retain(|token| {
            scopes.iter().any(|scope| {
                issued_matches(token, scope, token.write)
                    && (!token.write || !scope.writes.is_empty())
            })
        });
        live.extend(tokens.iter().map(|token| token.token.clone()));
    }
    let entry = ledger_entry()?;
    retire_ledger_tokens(
        &LEDGER_SECRET,
        &entry,
        computer,
        |token| {
            live.contains(token)
                || retirement()
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .active_pushes
                    .contains(token)
        },
        |token| token_operation(Operation::RevokeToken, json!({"accessToken":token})).map(|_| ()),
    )
}
fn append_ledger_token(
    secret: &SessionSecret<TokenLedger>,
    entry: &keyring::Entry,
    computer: &str,
    token: &str,
) -> Result<(), String> {
    secret.update(
        || read_ledger(entry),
        |ledger| {
            let tokens = ledger.entry(computer.into()).or_default();
            if !tokens.iter().any(|previous| previous == token) {
                tokens.push(token.into());
            }
        },
        |ledger| save_ledger(entry, ledger),
    )
}
fn retire_ledger_tokens(
    secret: &SessionSecret<TokenLedger>,
    entry: &keyring::Entry,
    computer: &str,
    live: impl Fn(&str) -> bool,
    mut revoke: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    let ledger = secret.read(|| read_ledger(entry))?;
    let tokens = ledger.get(computer).cloned().unwrap_or_default();
    let mut failure = None;
    let mut revoked = Vec::new();
    for token in tokens {
        if live(&token) {
            continue;
        }
        match revoke(&token) {
            Ok(()) => revoked.push(token),
            Err(message) => {
                failure = Some(message);
            }
        }
    }
    forget_ledger_tokens(secret, entry, computer, &revoked)?;
    failure.map_or(Ok(()), Err)
}

fn forget_ledger_tokens(
    secret: &SessionSecret<TokenLedger>,
    entry: &keyring::Entry,
    computer: &str,
    revoked: &[String],
) -> Result<(), String> {
    secret.update(
        || read_ledger(entry),
        |ledger| {
            if let Some(tokens) = ledger.get_mut(computer) {
                tokens.retain(|token| !revoked.contains(token));
                if tokens.is_empty() {
                    ledger.remove(computer);
                }
            }
        },
        |ledger| save_ledger(entry, ledger),
    )
}

fn finish_application(
    grants: Result<(), String>,
    explicit: bool,
    previous_error: Option<&str>,
    write: impl FnOnce() -> Result<(), String>,
) -> (Result<(), String>, Result<(), String>) {
    let identity = if explicit {
        write()
    } else {
        previous_error.map_or(Ok(()), |message| Err(message.into()))
    };
    let combined = match (grants, &identity) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(access), Err(identity)) => Err(format!("{access} Git identity: {identity}")),
        (Err(message), Ok(())) => Err(message),
        (Ok(()), Err(message)) => Err(message.clone()),
    };
    (combined, identity)
}

/// Settings are re-applied when the user changed them, once per app session, after a computer
/// restore, or when this computer's own grants are about to expire or its last attempt failed.
/// A global deadline reached by another computer's retry must not re-apply unchanged settings.
fn access_update_due(
    d: &Document,
    name: &str,
    at: u64,
    restored: bool,
    previous: &[RuntimeGrant],
) -> bool {
    if restored || d.access_pending.iter().any(|n| n == name) || d.session != session() {
        return true;
    }
    if at < d.refresh_at {
        return false;
    }
    let verified = d
        .operations
        .iter()
        .any(|op| op["computer"].as_str() == Some(name) && op["status"] == "succeeded");
    !verified
        || previous
            .iter()
            .any(|g| g.expires_at.saturating_sub(120) <= at)
}
/// Whether `identity` is still the saved Git identity for this computer.
fn identity_is_current(d: &Document, name: &str, identity: &Value) -> bool {
    d.computers
        .iter()
        .any(|w| w["computer"].as_str() == Some(name) && w["identity"] == *identity)
}
/// How long the worker can sleep before `worker_due` can next become true for `d`.
fn worker_wait(d: &Document, pending: Option<Instant>, at: u64, instant: Instant) -> Duration {
    let wait = match pending {
        Some(deadline) => deadline.saturating_duration_since(instant),
        None if d.session != session() => Duration::ZERO,
        None => {
            let mut next = d.refresh_at;
            if d.access_enabled && d.account.is_some() && !d.disconnect_pending {
                next = next.min(d.catalog_refresh_at);
            }
            Duration::from_secs(next.saturating_sub(at))
        }
    };
    wait.clamp(WORKER_MIN_SLEEP, WORKER_MAX_SLEEP)
}
fn worker_due(d: &Document, pending: Option<Instant>, at: u64, instant: Instant) -> bool {
    match pending {
        Some(deadline) => instant >= deadline,
        None => d.session != session() || at >= d.refresh_at || catalog_refresh_due(d, at),
    }
}
fn is_pending_restore(app: &tauri::AppHandle, name: &str) -> bool {
    crate::runtime::runtime_paths(app)
        .is_ok_and(|paths| crate::runtime::is_pending_restore(&paths, name))
}
fn apply(
    app: &tauri::AppHandle,
    _document: &mut Document,
    computer: Option<&str>,
    apply_identity: bool,
) -> Result<(), String> {
    let d = {
        let _state = serialize(&STATE);
        load(app)?
    };
    let narrowing_error = {
        let _state = serialize(&STATE);
        if load(app)?.revision != d.revision {
            schedule(Duration::ZERO);
            return Ok(());
        }
        narrow_each(app, &d)
    };
    let mut refresh_at = if now() < d.refresh_at {
        d.refresh_at
    } else {
        now() + 3600
    };
    for w in &d.computers {
        let name = w["computer"].as_str().ok_or("Invalid computer policy.")?;
        if computer.is_some_and(|target| target != name) {
            continue;
        }
        // A computer pending checkpoint restore has no runtime yet. Its saved choices stay
        // pending and apply when it starts (`computer_restored`); this is not a failure.
        if is_pending_restore(app, name) {
            let _state = serialize(&STATE);
            let mut current = load(app)?;
            if current.revision == d.revision
                && current
                    .operations
                    .iter()
                    .any(|op| op["computer"].as_str() == Some(name))
            {
                current
                    .operations
                    .retain(|op| op["computer"].as_str() != Some(name));
                save(app, &current)?;
            }
            continue;
        }
        // Identity changes are independent of token issuance, including offline edits.
        let identity_requested = apply_identity || d.identity_pending.iter().any(|n| n == name);
        if identity_requested {
            let identity = &w["identity"];
            let Some(result) = outside_state(
                || Ok((load(app)?.revision == d.revision).then_some(())),
                |()| crate::runtime::apply_github_identity(app, name, identity),
            )?
            else {
                schedule(Duration::from_millis(500));
                return Ok(());
            };
            let _state = serialize(&STATE);
            let mut current = load(app)?;
            // Record only the identity that is still requested; a newer edit made during
            // the guest command stays pending and is applied next.
            if identity_is_current(&current, name, identity) {
                current.identity_pending.retain(|n| n != name);
                match result {
                    Ok(()) => {
                        current.identity_errors.remove(name);
                    }
                    Err(error) => {
                        current.identity_errors.insert(name.into(), error);
                    }
                }
                save(app, &current)?;
            } else {
                schedule(Duration::from_millis(500));
            }
        }
        let key = active_key(app, name)?;
        let previous = active()
            .lock()
            .map_err(|_| "GitHub state is unavailable.")?
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let access_requested = access_update_due(&d, name, now(), take_restored(name), &previous);
        if !access_requested {
            if let Some(expiry) = previous.iter().map(|g| g.expires_at).min() {
                refresh_at = refresh_at.min(expiry.saturating_sub(120).max(now() + 30));
            }
        }
        let result = if access_requested {
            let result = if let Some(error) = narrowing_error.for_computer(name) {
                Err(error.clone())
            } else if personal_token::selected(w) {
                personal_token::apply(app, name, d.revision)
            } else {
                runtime_grants_for(app, &d, w, &previous).and_then(|grants| {
                    outside_state(
                        || {
                            if load(app)?.revision != d.revision {
                                return Err(
                                    "GitHub access changed. Applying your latest choices.".into()
                                );
                            }
                            if let Some(expiry) = grants.iter().map(|g| g.expires_at).min() {
                                refresh_at = refresh_at.min(expiry.saturating_sub(120));
                            }
                            let attached = profile(&grants);
                            // Comparing credentials too avoids reconnecting unchanged sessions.
                            if grants == previous
                                && d.session == session()
                                && crate::runtime::github_policy_is_cached(app, name, &attached)?
                            {
                                return Ok(None);
                            }
                            // Record the grants before attaching them: a save that narrows
                            // meanwhile (under STATE) must see, and remove, what this attach adds.
                            active()
                                .lock()
                                .map_err(|_| "GitHub state is unavailable.")?
                                .insert(key.clone(), grants.clone());
                            Ok(Some(attached))
                        },
                        |attached| {
                            crate::runtime::apply_github_policy(app, name, d.revision, &attached)
                        },
                    )?
                    .unwrap_or(Ok(()))
                })
            };
            let retirement = if d.grants_issued || load(app)?.grants_issued {
                retire_unused(app, name)
            } else {
                Ok(())
            };
            result.and(retirement)
        } else {
            d.access_errors
                .get(name)
                .map_or(Ok(()), |message| Err(message.clone()))
        };
        let _state = serialize(&STATE);
        let mut current = load(app)?;
        if current.revision != d.revision {
            schedule(Duration::from_millis(500));
            return Ok(());
        }
        if access_requested {
            current.access_pending.retain(|n| n != name);
            match &result {
                Ok(()) => {
                    current.access_errors.remove(name);
                }
                Err(error) => {
                    current.access_errors.insert(name.into(), error.clone());
                }
            }
        }
        let result = finish_application(
            result,
            false,
            current.identity_errors.get(name).map(String::as_str),
            || Ok(()),
        )
        .0;
        let operation = match result {
            Ok(()) => {
                json!({"computer":name,"status":"succeeded","message":"GitHub access verified."})
            }
            Err(message) => {
                let retry = crate::github_http::retry_at();
                refresh_at = refresh_at.min(if retry > 0 {
                    retry.max(now() + 1)
                } else {
                    now() + 300
                });
                json!({"computer":name,"status":"failed","message":message,"canRetry":true})
            }
        };
        current
            .operations
            .retain(|op| op["computer"].as_str() != Some(name));
        current.operations.push(operation);
        current.session = session().into();
        current.refresh_at = refresh_at;
        save(app, &current)?;
    }
    Ok(())
}

fn validate(computers: &[Value]) -> Result<(), String> {
    if computers.len() > 64 {
        return Err("Too many computer policies.".into());
    };
    let mut names = std::collections::HashSet::new();
    for w in computers {
        let fields = w.as_object().ok_or("Invalid computer policy.")?;
        if fields.keys().any(|k| {
            ![
                "computer",
                "identity",
                "repositories",
                "repositoryMode",
                "allRepositoriesAllowChanges",
                "authenticationMethod",
            ]
            .contains(&k.as_str())
        }) {
            return Err("Unknown computer policy field.".into());
        }
        if serde_json::to_vec(w).map_or(true, |b| b.len() > 1024 * 1024) {
            return Err("Computer policy is too large.".into());
        }
        let name = w["computer"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 200)
            .ok_or("Missing computer name.")?;
        crate::runtime::validate_name(name).map_err(|error| error.to_string())?;
        let identity = w["identity"]
            .as_object()
            .ok_or("Missing Git identity settings.")?;
        if identity
            .keys()
            .any(|key| !["name", "email", "apply"].contains(&key.as_str()))
            || !w["identity"]["apply"].is_boolean()
            || ["name", "email"].iter().any(|key| {
                w["identity"][key].as_str().is_none_or(|value| {
                    value.len() > 1024
                        || value.chars().any(char::is_control)
                        || (w["identity"]["apply"] == true && value.trim().is_empty())
                })
            })
        {
            return Err("Invalid Git identity settings.".into());
        }
        if !names.insert(name) {
            return Err("Duplicate computer policy.".into());
        };
        if !matches!(w["repositoryMode"].as_str(), Some("all" | "selected")) {
            return Err("Choose selected or all repositories.".into());
        };
        if !w["allRepositoriesAllowChanges"].is_boolean() {
            return Err("Invalid GitHub changes policy.".into());
        };
        if !matches!(
            w["authenticationMethod"].as_str(),
            None | Some("oauth" | "token")
        ) || (!w["authenticationMethod"].is_null() && !w["authenticationMethod"].is_string())
        {
            return Err("Choose GitHub OAuth or a personal token.".into());
        }
        let mut repository_names = std::collections::HashSet::new();
        for r in w["repositories"]
            .as_array()
            .ok_or("Invalid repository selection.")?
        {
            if r.as_object().is_none_or(|fields| {
                fields
                    .keys()
                    .any(|key| !["repository", "allowPushes"].contains(&key.as_str()))
            }) {
                return Err("Unknown repository policy field.".into());
            }
            let n = r["repository"].as_str().ok_or("Invalid repository name.")?;
            if !repository_names.insert(n.to_ascii_lowercase())
                || n.split('/').count() != 2
                || n.split('/').any(|part| {
                    part.is_empty()
                        || !part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                })
                || !r["allowPushes"].is_boolean()
            {
                return Err("Invalid repository policy.".into());
            }
        }
    }
    Ok(())
}

/// Only the host runtime IPC may serialize these short-lived credentials.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeGrant {
    pub owner_id: u64,
    pub owner_login: String,
    pub repository_ids: Vec<u64>,
    pub read_token: String,
    pub write_token: Option<String>,
    pub write_repository_ids: Vec<u64>,
    pub expires_at: u64,
    pub read_expires_at: u64,
    pub write_expires_at: u64,
    pub all_repositories: bool,
}
fn token_expiry(response: &Value) -> Result<u64, String> {
    let raw = response["expiresAt"]
        .as_str()
        .ok_or("Restricted credential has no expiration.")?;
    let seconds = time::OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339)
        .map_err(|_| "Restricted credential expiration is invalid.")?
        .unix_timestamp();
    let seconds = u64::try_from(seconds).map_err(|_| "Restricted credential has expired.")?;
    if seconds <= now() + 120 {
        return Err("Restricted credential expires too soon.".into());
    }
    Ok(seconds)
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct GrantScope {
    owner: u64,
    login: String,
    ids: Vec<u64>,
    writes: Vec<u64>,
    all: bool,
}
fn scopes(d: &Document, policy: &Value) -> Result<Vec<GrantScope>, String> {
    if !d.access_enabled || personal_token::selected(policy) {
        return Ok(Vec::new());
    }
    let all = policy["repositoryMode"].as_str() == Some("all");
    let selected = policy["repositories"]
        .as_array()
        .ok_or("Invalid repository selection.")?;
    let matches = |repo: &Value, selection: &Value| {
        repo["name"]
            .as_str()
            .zip(selection["repository"].as_str())
            .is_some_and(|(catalog, saved)| catalog.eq_ignore_ascii_case(saved))
    };
    if !all
        && selected
            .iter()
            .any(|s| !d.repositories.iter().any(|r| matches(r, s)))
    {
        return Err(
            "A selected repository is no longer authorized by GitHub. Update the selection.".into(),
        );
    }
    let mut groups = std::collections::BTreeMap::<u64, GrantScope>::new();
    for repo in &d.repositories {
        let selection = selected.iter().find(|s| matches(repo, s));
        if !all && selection.is_none() {
            continue;
        }
        let owner = repo["ownerId"]
            .as_u64()
            .ok_or("Invalid GitHub owner identifier.")?;
        let id = repo["id"]
            .as_u64()
            .ok_or("Invalid GitHub repository identifier.")?;
        let login = repo["name"]
            .as_str()
            .and_then(|s| s.split('/').next())
            .ok_or("Invalid repository name.")?;
        let group = groups.entry(owner).or_insert_with(|| GrantScope {
            owner,
            login: login.into(),
            ids: vec![],
            writes: vec![],
            all,
        });
        group.ids.push(id);
        if if all {
            policy["allRepositoriesAllowChanges"] == true
        } else {
            selection.is_some_and(|s| s["allowPushes"] == true)
        } {
            group.writes.push(id);
        }
    }
    for group in groups.values_mut() {
        group.ids.sort_unstable();
        group.ids.dedup();
        group.writes.sort_unstable();
        group.writes.dedup();
    }
    Ok(groups.into_values().collect())
}
fn read_matches(g: &RuntimeGrant, s: &GrantScope) -> bool {
    g.owner_id == s.owner
        && g.all_repositories == s.all
        && (s.all || g.repository_ids == s.ids)
        && g.read_expires_at > now() + 120
        && !g.read_token.is_empty()
}
fn write_matches(g: &RuntimeGrant, s: &GrantScope) -> bool {
    g.owner_id == s.owner
        && g.all_repositories == s.all
        && (s.all || g.write_repository_ids == s.writes)
        && g.write_expires_at > now() + 120
        && g.write_token.is_some()
}
fn runtime_grants_for(
    app: &tauri::AppHandle,
    d: &Document,
    policy: &Value,
    previous: &[RuntimeGrant],
) -> Result<Vec<RuntimeGrant>, String> {
    if !d.access_enabled || d.account.is_none() {
        return Ok(Vec::new());
    }
    let desired = scopes(d, policy)?;
    let mut credential = None;
    reconcile_grants(
        &desired,
        previous,
        |scope, write| {
            if credential.is_none() {
                credential = Some(active_credential()?);
            }
            mint(
                app,
                policy["computer"]
                    .as_str()
                    .ok_or("Invalid computer policy.")?,
                credential.as_ref().ok_or("Connect GitHub first.")?,
                scope,
                write,
            )
        },
        || load(app).is_ok_and(|current| current.revision == d.revision),
    )
}
fn reconcile_grants(
    desired: &[GrantScope],
    previous: &[RuntimeGrant],
    mut issue: impl FnMut(&GrantScope, bool) -> Result<(String, u64), String>,
    current: impl Fn() -> bool,
) -> Result<Vec<RuntimeGrant>, String> {
    let mut grants = Vec::new();
    for s in desired {
        if !current() {
            return Err("GitHub access changed. Applying your latest choices.".into());
        }
        let prior = previous.iter().find(|g| g.owner_id == s.owner);
        let (read_token, read_expiry) = if let Some(g) = prior.filter(|g| read_matches(g, s)) {
            (g.read_token.clone(), g.read_expires_at)
        } else {
            issue(s, false)?
        };
        if !current() {
            return Err("GitHub access changed. Applying your latest choices.".into());
        }
        let (write_token, write_expiry) = if s.writes.is_empty() {
            (None, u64::MAX)
        } else if let Some(g) = prior.filter(|g| write_matches(g, s)) {
            (g.write_token.clone(), g.write_expires_at)
        } else {
            let (token, expiry) = issue(s, true)?;
            (Some(token), expiry)
        };
        grants.push(RuntimeGrant {
            owner_id: s.owner,
            owner_login: s.login.clone(),
            repository_ids: s.ids.clone(),
            read_token,
            write_token,
            write_repository_ids: s.writes.clone(),
            expires_at: read_expiry.min(write_expiry),
            read_expires_at: read_expiry,
            write_expires_at: write_expiry,
            all_repositories: s.all,
        });
    }
    Ok(grants)
}

fn mint(
    app: &tauri::AppHandle,
    computer: &str,
    c: &Credential,
    s: &GrantScope,
    write: bool,
) -> Result<(String, u64), String> {
    let key = active_key(app, computer)?;
    if let Some(token) = issued()
        .lock()
        .map_err(|_| "GitHub state is unavailable.")?
        .get(&key)
        .and_then(|tokens| tokens.iter().find(|token| issued_matches(token, s, write)))
        .cloned()
    {
        return Ok((token.token, token.expires_at));
    }
    let response = crate::github_tokens::execute_for_computer(
        &token_configuration()?,
        Operation::Scope,
        json!({"accessToken":c.access_token,"ownerId":s.owner,"repositoryIds":if s.all{vec![]}else if write{s.writes.clone()}else{s.ids.clone()},"allRepositories":s.all,"allowChanges":write}),
        computer,
    )?;
    let token = response["accessToken"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("GitHub returned no restricted credential.")?
        .to_owned();
    if let Err(error) = remember_token(app, computer, &token) {
        let _ = token_operation(Operation::RevokeToken, json!({"accessToken":token}));
        return Err(error);
    }
    let expiry = token_expiry(&response)?;
    issued()
        .lock()
        .map_err(|_| "GitHub state is unavailable.")?
        .entry(key)
        .or_default()
        .push(IssuedToken {
            owner: s.owner,
            all: s.all,
            write,
            ids: if write {
                s.writes.clone()
            } else {
                s.ids.clone()
            },
            token: token.clone(),
            expires_at: expiry,
        });
    Ok((token, expiry))
}
// Dropping an entire affected token is necessary: its GitHub scope cannot be
// narrowed locally by changing routing hints, especially for GraphQL requests.
fn narrow(grants: &[RuntimeGrant], desired: &[GrantScope]) -> Vec<RuntimeGrant> {
    grants
        .iter()
        .filter_map(|g| {
            let s = desired.iter().find(|s| s.owner == g.owner_id)?;
            let read_safe = (!g.all_repositories || s.all)
                && (s.all || g.repository_ids.iter().all(|id| s.ids.contains(id)));
            let mut retained = g.clone();
            if !read_safe {
                retained.read_token.clear();
                retained.repository_ids.clear();
                retained.read_expires_at = 0;
            }
            let write_safe = (!g.all_repositories || s.all)
                && (if s.all {
                    !s.writes.is_empty()
                } else {
                    g.write_repository_ids
                        .iter()
                        .all(|id| s.writes.contains(id))
                });
            if !write_safe {
                retained.write_token = None;
                retained.write_repository_ids.clear();
                retained.write_expires_at = u64::MAX;
                retained.expires_at = retained.read_expires_at;
            }
            Some(retained)
        })
        .collect()
}
/// Per-computer narrowing failures. One computer's failure never leaves other computers with
/// authority, and never blocks grants for the other computers.
#[derive(Default, Debug)]
pub(super) struct NarrowErrors {
    computers: std::collections::BTreeMap<String, String>,
    all: Option<String>,
}
impl NarrowErrors {
    pub(super) fn record(&mut self, computer: &str, error: String) {
        self.computers.entry(computer.into()).or_insert(error);
    }
    /// A failure that is not specific to one computer.
    pub(super) fn record_all(&mut self, error: String) {
        self.all.get_or_insert(error);
    }
    fn for_computer(&self, computer: &str) -> Option<&String> {
        self.computers.get(computer).or(self.all.as_ref())
    }
    pub(super) fn into_result(self) -> Result<(), String> {
        match self.all.or_else(|| self.computers.into_values().next()) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
/// Run `detach` for every computer, recording each failure under its own computer.
pub(super) fn each_computer<'a, T: 'a>(
    items: impl IntoIterator<Item = (&'a str, T)>,
    errors: &mut NarrowErrors,
    mut detach: impl FnMut(&str, T) -> Result<(), String>,
) {
    for (name, item) in items {
        if let Err(error) = detach(name, item) {
            errors.record(name, error);
        }
    }
}
/// A removed computer has no authority left to detach. Its cache entry is dropped instead of
/// failing every later narrowing until the app restarts.
fn is_device_removed(app: &tauri::AppHandle, name: &str) -> bool {
    crate::runtime::runtime_paths(app).is_ok_and(|paths| {
        matches!(
            crate::runtime::resolve_computer_id(&paths, name),
            Err(crate::runtime::RuntimeError::Invalid(_))
        )
    })
}
pub(super) fn detach_result(
    app: &tauri::AppHandle,
    name: &str,
    result: Result<(), String>,
) -> Result<(), String> {
    match result {
        Err(_) if is_device_removed(app, name) => Ok(()),
        other => other,
    }
}
fn narrow_now(app: &tauri::AppHandle, d: &mut Document) -> Result<(), String> {
    narrow_each(app, d).into_result()
}
fn narrow_each(app: &tauri::AppHandle, d: &Document) -> NarrowErrors {
    let mut errors = NarrowErrors::default();
    // Token computers and OAuth computers are narrowed independently; neither blocks the other.
    personal_token::narrow(app, d, &mut errors);
    let prefix = match path(app) {
        Ok(path) => format!("{}:", path.display()),
        Err(error) => {
            errors.record_all(error);
            return errors;
        }
    };
    if d.session != session() && d.grants_issued {
        for w in &d.computers {
            if let Some(name) = w["computer"].as_str() {
                let attached = match active_key(app, name).and_then(|key| {
                    Ok(active()
                        .lock()
                        .map_err(|_| "GitHub state is unavailable.")?
                        .contains_key(&key))
                }) {
                    Ok(attached) => attached,
                    Err(error) => {
                        errors.record(name, error);
                        continue;
                    }
                };
                if !is_pending_restore(app, name) && !attached {
                    if let Err(error) = detach_result(
                        app,
                        name,
                        crate::runtime::apply_github_policy(app, name, d.revision, &profile(&[])),
                    ) {
                        errors.record(name, error);
                    }
                }
            }
        }
    }
    let cached = match active().lock() {
        Ok(cached) => cached.clone(),
        Err(_) => {
            errors.record_all("GitHub state is unavailable.".into());
            return errors;
        }
    };
    for (key, previous) in cached.iter().filter(|(key, _)| key.starts_with(&prefix)) {
        let name = &key[prefix.len()..];
        let desired = d
            .computers
            .iter()
            .find(|w| w["computer"].as_str() == Some(name))
            .map(|w| scopes(d, w))
            .transpose()
            .map(Option::unwrap_or_default);
        // A stale catalog or invalid other selection must never preserve access
        // that the user just removed. Detach first, retain the validation error.
        let (retained, validation_error) = narrow_checked(previous, desired);
        if let Some(error) = validation_error {
            errors.record(name, error);
        }
        if retained != *previous {
            let result =
                crate::runtime::apply_github_policy(app, name, d.revision, &profile(&retained));
            if result.is_err() && is_device_removed(app, name) {
                if let Ok(mut active) = active().lock() {
                    active.remove(key);
                }
                continue;
            }
            if let Err(error) = result {
                errors.record(name, error);
                continue;
            }
            match active().lock() {
                Ok(mut active) => {
                    active.insert(key.clone(), retained);
                }
                Err(_) => errors.record(name, "GitHub state is unavailable.".into()),
            }
        }
    }
    errors
}

fn narrow_checked(
    previous: &[RuntimeGrant],
    desired: Result<Vec<GrantScope>, String>,
) -> (Vec<RuntimeGrant>, Option<String>) {
    match desired {
        Ok(scopes) => (narrow(previous, &scopes), None),
        Err(error) => (Vec::new(), Some(error)),
    }
}

const RETIREMENT_RETRY_AFTER: u64 = 30;
#[derive(Default)]
struct TokenRetirement {
    active_pushes: std::collections::HashSet<String>,
    retry_at: u64,
}
impl TokenRetirement {
    fn wait(&self, at: u64) -> Duration {
        Duration::from_secs(self.retry_at.saturating_sub(at))
    }
    fn finished(&mut self, token: &str, at: u64) {
        self.active_pushes.remove(token);
        self.retry_at = at;
    }
}
fn retirement() -> &'static Mutex<TokenRetirement> {
    static RETIREMENT: OnceLock<Mutex<TokenRetirement>> = OnceLock::new();
    RETIREMENT.get_or_init(|| Mutex::new(TokenRetirement::default()))
}
fn begin_host_push_token(
    retirement: &Mutex<TokenRetirement>,
    token: &str,
    remember: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    retirement
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .active_pushes
        .insert(token.into());
    remember()
}

/// A credential for one explicit host push. Silo's scoped token is revoked as
/// soon as the push ends (owner decision 1); a personal token is never revoked.
pub(crate) struct HostPushCredential {
    token: String,
    repository: String,
    expires_at: Option<u64>,
    retire: Option<(tauri::AppHandle, String)>,
}
impl HostPushCredential {
    pub(crate) fn token(&self) -> &str {
        &self.token
    }
    /// GitHub's canonical `owner/name` for the authorized repository.
    pub(crate) fn repository(&self) -> &str {
        &self.repository
    }
    /// Unix time after which GitHub rejects Silo's token; `None` for a personal token.
    pub(crate) fn expires_at(&self) -> Option<u64> {
        self.expires_at
    }
}
impl Drop for HostPushCredential {
    fn drop(&mut self) {
        if let Some((_app, computer)) = self.retire.take() {
            let result = retire_host_push_token(
                &self.token,
                |token| {
                    token_operation(Operation::RevokeToken, json!({"accessToken":token}))
                        .map(|_| ())
                },
                |token| {
                    let entry = ledger_entry()?;
                    forget_ledger_tokens(&LEDGER_SECRET, &entry, &computer, &[token.into()])
                },
            );
            if let Err(error) = result {
                eprintln!("Could not retire the host push credential: {error}");
            }
            retirement()
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .finished(&self.token, now());
            wake_worker();
        }
    }
}
/// Only a confirmed revocation removes a pre-recorded token from the ledger.
fn retire_host_push_token(
    token: &str,
    revoke: impl FnOnce(&str) -> Result<(), String>,
    forget: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    revoke(token)?;
    forget(token)
}
/// Scoped tokens end with the account credential. Renew an account credential
/// that would expire within this time so a long push keeps a working token.
const HOST_PUSH_LIFETIME: u64 = 30 * 60;
fn host_push_account_credential() -> Result<Credential, String> {
    let current = credential()?.ok_or("Connect GitHub first.")?;
    let mut pending = PENDING_REFRESH
        .lock()
        .map_err(|_| "GitHub credential state is unavailable.")?;
    refresh_credential_with(
        current,
        &mut pending,
        now() + HOST_PUSH_LIFETIME,
        |refresh| {
            from_response(token_operation(
                Operation::Refresh,
                json!({"refreshToken":refresh}),
            )?)
        },
        store,
    )
}
/// `contents: write` and `metadata: read` for exactly one repository.
fn host_push_scope(access_token: &str, owner: u64, repository_id: u64) -> Value {
    json!({"accessToken":access_token,"ownerId":owner,"repositoryIds":[repository_id],"allowChanges":true,"purpose":"hostPush"})
}

/// Explicit host Push only: does not grant write access to the guest or modify its policy.
pub(crate) fn host_push_credential(
    app: &tauri::AppHandle,
    computer: &str,
    repository: &str,
) -> Result<HostPushCredential, String> {
    let _guard = crate::sync::lock_or_recover(&OPERATION, "GitHub operation");
    let d = {
        let _state = serialize(&STATE);
        load(app)?
    };
    let policy = d
        .computers
        .iter()
        .find(|w| w["computer"].as_str() == Some(computer))
        .ok_or("This computer has no GitHub repository authorization.")?;
    validate(std::slice::from_ref(policy))?;
    // Disable access and the per-repository push grant apply to every sign-in method.
    if !d.access_enabled {
        return Err("Enable GitHub access before pushing.".into());
    }
    push_authorized(policy, repository)?;
    let current = || {
        let _state = serialize(&STATE);
        if load(app)?.revision != d.revision {
            return Err("GitHub access changed. Retry the push with your latest choices.".into());
        }
        Ok(())
    };
    issue_host_push(
        || {
            if personal_token::selected(policy) {
                return Ok(HostPushCredential {
                    token: personal_token::value()?,
                    repository: repository.into(),
                    expires_at: None,
                    retire: None,
                });
            }
            let c = host_push_account_credential()?;
            let catalog = catalog(&c)?;
            let repo = catalog
                .iter()
                .find(|r| {
                    r["name"]
                        .as_str()
                        .is_some_and(|name| name.eq_ignore_ascii_case(repository))
                })
                .ok_or("GitHub no longer authorizes this repository.")?;
            let name = repo["name"]
                .as_str()
                .ok_or("Invalid repository name.")?
                .to_owned();
            let owner = repo["ownerId"]
                .as_u64()
                .ok_or("Invalid repository owner.")?;
            let id = repo["id"]
                .as_u64()
                .ok_or("Invalid repository identifier.")?;
            let ledger = ledger_entry()?;
            LEDGER_SECRET.read(|| read_ledger(&ledger))?;
            current()?;
            let response = token_operation(
                Operation::Scope,
                host_push_scope(&c.access_token, owner, id),
            )?;
            let token = response["accessToken"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("GitHub returned no restricted push credential.")?
                .to_owned();
            // From here on, dropping the credential revokes the token.
            let mut credential = HostPushCredential {
                token,
                repository: name,
                expires_at: None,
                retire: Some((app.clone(), computer.into())),
            };
            begin_host_push_token(retirement(), &credential.token, || {
                remember_token(app, computer, &credential.token)
            })?;
            credential.expires_at = Some(token_expiry(&response)?);
            Ok(credential)
        },
        &current,
    )
}

fn issue_host_push<T>(
    issue: impl FnOnce() -> Result<T, String>,
    current: impl Fn() -> Result<(), String>,
) -> Result<T, String> {
    current()?;
    let credential = issue()?;
    // An obsolete scoped credential is dropped and retired before it reaches the push.
    current()?;
    Ok(credential)
}

/// Host push publishes changes, so the computer needs a push (write) grant for
/// the repository, not only read access. GitHub names are case-insensitive.
fn push_authorized(policy: &Value, repository: &str) -> Result<(), String> {
    let allowed = if policy["repositoryMode"].as_str() == Some("all") {
        policy["allRepositoriesAllowChanges"] == true
    } else {
        policy["repositories"].as_array().is_some_and(|repos| {
            repos.iter().any(|r| {
                r["allowPushes"] == true
                    && r["repository"]
                        .as_str()
                        .is_some_and(|name| name.eq_ignore_ascii_case(repository))
            })
        })
    };
    if allowed {
        Ok(())
    } else {
        Err("This computer is not allowed to push to this repository.".into())
    }
}

#[cfg(test)]
mod host_push_authorization_tests {
    use serde_json::json;

    #[test]
    fn host_push_requests_a_contents_only_token_for_one_repository() {
        let _test_state = crate::test_support::global_state();
        let request = super::host_push_scope("parent", 7, 11);
        assert_eq!(request["purpose"], "hostPush");
        assert_eq!(request["repositoryIds"], json!([11]));
        assert!(request.get("allRepositories").is_none());
    }

    #[test]
    fn host_push_tokens_leave_the_ledger_only_after_confirmed_revocation() {
        let _test_state = crate::test_support::global_state();
        let forgotten = std::cell::RefCell::new(Vec::<String>::new());
        super::retire_host_push_token(
            "revoked",
            |token| {
                assert_eq!(token, "revoked");
                Ok(())
            },
            |token| {
                forgotten.borrow_mut().push(token.into());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*forgotten.borrow(), ["revoked"]);
        assert!(super::retire_host_push_token(
            "offline",
            |_| Err("GitHub is unreachable.".into()),
            |token| {
                forgotten.borrow_mut().push(token.into());
                Ok(())
            },
        )
        .is_err());
        assert_eq!(*forgotten.borrow(), ["revoked"]);
    }

    #[test]
    fn host_push_drops_issued_credentials_when_policy_changes_before_handoff() {
        use std::cell::Cell;
        struct ScopedToken<'a>(&'a Cell<usize>);
        impl Drop for ScopedToken<'_> {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let revision = Cell::new(7);
        let revoked = Cell::new(0);
        let handed_off = Cell::new(false);
        let result = super::issue_host_push(
            || {
                // A completed Disable access or policy save increments the revision
                // while the network request is issuing the credential.
                revision.set(8);
                Ok(ScopedToken(&revoked))
            },
            || {
                if revision.get() != 7 {
                    Err("GitHub access changed.".into())
                } else {
                    Ok(())
                }
            },
        );
        if result.is_ok() {
            handed_off.set(true);
        }
        assert!(!handed_off.get());
        assert_eq!(revoked.get(), 1);
    }

    #[test]
    fn host_push_does_not_issue_after_authorization_already_changed() {
        let mut issued = false;
        let result = super::issue_host_push(
            || {
                issued = true;
                Ok("credential")
            },
            || Err("GitHub access changed.".into()),
        );
        assert!(result.is_err());
        assert!(!issued);
    }

    #[test]
    fn host_push_hands_off_credentials_when_authorization_stays_current() {
        assert_eq!(
            super::issue_host_push(|| Ok("credential"), || Ok(())),
            Ok("credential")
        );
    }

    #[test]
    fn host_push_requires_a_write_grant_and_ignores_name_case() {
        let _test_state = crate::test_support::global_state();
        let read_only = json!({"repositoryMode":"selected","repositories":[{"repository":"owner/repo","allowPushes":false}]});
        assert!(super::push_authorized(&read_only, "owner/repo").is_err());
        let write = json!({"repositoryMode":"selected","repositories":[{"repository":"owner/repo","allowPushes":true}]});
        assert!(super::push_authorized(&write, "Owner/Repo").is_ok());
        assert!(super::push_authorized(&write, "owner/other").is_err());
        assert!(super::push_authorized(&json!({"repositoryMode":"all"}), "owner/repo").is_err());
        assert!(super::push_authorized(
            &json!({"repositoryMode":"all","allRepositoriesAllowChanges":true}),
            "owner/repo"
        )
        .is_ok());
    }
}

fn callback(request: &str, state: &str) -> Result<Option<String>, String> {
    let Some(line) = request.lines().next() else {
        return Ok(None);
    };
    let parts: Vec<_> = line.split_whitespace().collect();
    if parts.len() != 3
        || parts[0] != "GET"
        || !matches!(parts[2], "HTTP/1.0" | "HTTP/1.1")
        || !parts[1].starts_with('/')
        || parts[1].starts_with("//")
    {
        return Ok(None);
    }
    let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{}", parts[1])) else {
        return Ok(None);
    };
    if url.path() != "/github/callback" {
        return Ok(None);
    };
    let pairs: Vec<_> = url.query_pairs().collect();
    if pairs.iter().filter(|(k, _)| k == "state").count() != 1
        || !pairs.iter().any(|(k, v)| k == "state" && v == state)
    {
        return Ok(None);
    }
    if pairs.iter().any(|(k, _)| k == "error") {
        return Err("GitHub authorization was declined.".into());
    };
    if pairs.iter().filter(|(k, _)| k == "code").count() != 1 {
        return Err("Invalid GitHub authorization response.".into());
    };
    let code = pairs
        .iter()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.as_ref())
        .unwrap_or_default();
    if code.is_empty() || code.len() > 1024 || !code.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("Invalid GitHub authorization response.".into());
    }
    Ok(Some(code.into()))
}

// An unauthenticated local connection is not an OAuth result. Read a complete,
// bounded header before parsing it; ignore incomplete or malformed traffic.
/// Prepare an accepted callback connection for a bounded blocking read. On macOS an
/// accepted socket inherits the listener's non-blocking mode, which ignores the read
/// timeout: an early read fails with WouldBlock and the single-use code is lost.
fn callback_stream(stream: TcpStream) -> std::io::Result<TcpStream> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    Ok(stream)
}
fn read_callback_request(reader: &mut impl Read) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut bytes = Vec::new();
    let mut chunk = [0; 1024];
    while bytes.len() < 8192 && Instant::now() < deadline {
        let remaining = (8192 - bytes.len()).min(chunk.len());
        let length = match reader.read(&mut chunk[..remaining]) {
            Ok(length) => length,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        if length == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..length]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            return String::from_utf8(bytes[..end + 4].to_vec()).ok();
        }
    }
    None
}
/// Open GitHub pages with the browser chosen in Settings. The platform launcher returns
/// once the browser was asked to open, so the callback wait is never blocked by it.
/// One bounded network or store step of a connection: it holds the GitHub network lock
/// and the update guard only while it runs, never across a wait for the user.
fn network_step<T>(step: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let _update = crate::updates::operation_guard()?;
    let _network = serialize(&OPERATION);
    step()
}
fn open_browser(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
    crate::applications::open_browser(app, url)
}
fn open_authorization_browser(
    app: &tauri::AppHandle,
    generation: u64,
    url: &str,
) -> Result<(), String> {
    {
        let mut pending = AUTHORIZATION
            .lock()
            .map_err(|_| "GitHub connection is unavailable.")?;
        if CANCELLATION.load(Ordering::SeqCst) != generation {
            return Err("GitHub connection cancelled.".into());
        }
        pending.0 = Some((generation, url.to_owned()));
    }
    open_browser(app, url)
}

fn connect(app: &tauri::AppHandle, generation: u64) -> Result<Value, String> {
    {
        let _state = serialize(&STATE);
        if CANCELLATION.load(Ordering::SeqCst) != generation {
            return Err("GitHub connection cancelled.".into());
        }
        CONNECTING.store(true, Ordering::SeqCst);
    }
    let _connecting = Connecting(app.clone(), generation);
    let _ = app.emit("silo://application-state-changed", ());
    let configuration = token_configuration()?;
    let client_id = &configuration.client_id;
    entry()?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|_| "Cannot start the GitHub callback listener.")?;
    listener
        .set_nonblocking(true)
        .map_err(|_| "Cannot configure GitHub callback listener.")?;
    let redirect = format!(
        "http://127.0.0.1:{}/github/callback",
        listener
            .local_addr()
            .map_err(|_| "Cannot read callback address.")?
            .port()
    );
    let state = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = reqwest::Url::parse("https://github.com/login/oauth/authorize").unwrap();
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", &redirect)
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    open_authorization_browser(app, generation, url.as_str())?;
    let deadline = Instant::now() + Duration::from_secs(300);
    let code = loop {
        if CANCELLATION.load(Ordering::SeqCst) != generation {
            return Err("GitHub connection cancelled.".into());
        }
        if Instant::now() > deadline {
            return Err("GitHub connection timed out. Try again.".into());
        };
        match listener.accept() {
            Ok((stream, _)) => {
                let Ok(mut stream) = callback_stream(stream) else {
                    continue;
                };
                let result = read_callback_request(&mut stream)
                    .map(|request| callback(&request, &state))
                    .unwrap_or(Ok(None));
                let valid = matches!(&result, Ok(Some(_)));
                let text = if valid {
                    "GitHub authorization received. Return to Silo."
                } else {
                    "Invalid GitHub callback."
                };
                let _=write!(stream,"HTTP/1.1 {}\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",if valid{"200 OK"}else{"400 Bad Request"},text.len(),text);
                if let Some(code) = result? {
                    break code;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(_) => return Err("GitHub callback listener failed.".into()),
        }
    };
    AUTHORIZATION
        .lock()
        .map_err(|_| "GitHub connection is unavailable.")?
        .clear(generation);
    if CANCELLATION.load(Ordering::SeqCst) != generation {
        return Err("GitHub connection cancelled.".into());
    }
    // Only the exchange and the store steps hold the GitHub network lock and the update
    // guard. The browser and App-installation waits (up to 5 minutes each) must not block
    // token renewal, repository refresh, host push or app updates.
    let previous_account = load(app).ok().and_then(|d| d.account);
    let (c, mut unstored, account, mut repos, installed) = network_step(|| {
        let c = from_response(token_operation(
            Operation::Exchange,
            json!({"code":code,"codeVerifier":verifier,"redirectUri":redirect}),
        )?)?;
        // From here every failure or cancellation revokes the new credential. Until its
        // account is known, assume it may share the stored connection's authorization.
        let mut unstored = Unstored::new(c.clone(), previous_account.is_some());
        let user = github(&c.access_token, "/user")?;
        let login: String = user["login"]
            .as_str()
            .ok_or("GitHub account name is missing.")?
            .into();
        unstored.shares_grant = same_account(previous_account.as_deref(), &login);
        let (repos, installed) = catalog_installations(&c)?;
        Ok((c, unstored, Some(login), repos, installed))
    })?;
    if !installed {
        let slug = APP_SLUG.ok_or("GitHub App is not configured in this build.")?;
        if !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err("GitHub App identifier is invalid.".into());
        }
        open_authorization_browser(
            app,
            generation,
            &format!("https://github.com/apps/{slug}/installations/new"),
        )?;
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            if CANCELLATION.load(Ordering::SeqCst) != generation {
                return Err("GitHub connection cancelled.".into());
            }
            if Instant::now() >= deadline {
                return Err("GitHub repository authorization timed out. Connect again after authorizing the App.".into());
            }
            std::thread::sleep(Duration::from_secs(3));
            let result = catalog_installations(&c)?;
            if result.1 {
                repos = result.0;
                break;
            }
        }
    }
    if CANCELLATION.load(Ordering::SeqCst) != generation {
        return Err("GitHub connection cancelled.".into());
    }
    let replaced = network_step(|| {
        // The credential this connection replaces, read before taking STATE.
        let replaced = revocation_credential().ok().flatten();
        let _state = serialize(&STATE);
        if CANCELLATION.load(Ordering::SeqCst) != generation {
            return Err("GitHub connection cancelled.".into());
        }
        let mut d = load(app)?;
        let revision = next_policy_revision(d.revision)?;
        // A failed explicit replacement must leave the previous account and its access intact.
        replace_connection_credential(
            &ACCOUNT_SECRET,
            &c,
            || entry().and_then(|entry| store_entry(&entry, &c)),
            publish_credential_observation,
        )?;
        unstored.kept();
        // Reconnecting creates a new account authorization. Never reuse old
        // grants, even if the account name and repository choices are identical.
        let prefix = format!("{}:", path(app)?.display());
        // Best effort per computer: a stale policy (removed computer, computer needing recreation) must not
        // drop the new credential. Failing computers get a per-computer error and are re-applied
        // with the new grants by the worker, which replaces the old profile.
        let mut detach_errors = NarrowErrors::default();
        each_computer(
            d.computers
                .iter()
                .filter(|w| !personal_token::selected(w))
                .filter_map(|w| w["computer"].as_str())
                .map(|name| (name, ())),
            &mut detach_errors,
            |name, ()| {
                detach_result(
                    app,
                    name,
                    crate::runtime::apply_github_policy(app, name, revision, &profile(&[])),
                )
            },
        );
        active()
            .lock()
            .map_err(|_| "GitHub state is unavailable.")?
            .retain(|key, _| !key.starts_with(&prefix));
        issued()
            .lock()
            .map_err(|_| "GitHub state is unavailable.")?
            .retain(|key, _| !key.starts_with(&prefix));
        let same = account
            .as_deref()
            .is_some_and(|login| same_account(d.account.as_deref(), login));
        record_connection(&mut d, account, repos)?;
        for (name, error) in detach_errors.computers {
            d.access_errors.insert(name, error);
        }
        save(app, &d)?;
        Ok(replaced
            .filter(|old| old.access_token != c.access_token)
            .map(|old| (old, same)))
    })?;
    // The replaced credential is no longer stored anywhere; revoke it rather than leave
    // its authorization (and a refresh token valid for months) live.
    if let Some((old, same)) = replaced {
        revoke_replaced(&old, same);
    }
    schedule(Duration::from_millis(500));
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    drop(_connecting);
    snapshot(app)
}

fn record_connection(
    d: &mut Document,
    account: Option<String>,
    repositories: Vec<Value>,
) -> Result<(), String> {
    let revision = next_policy_revision(d.revision)?;
    d.access_enabled = true;
    d.disconnect_pending = false;
    d.account = account;
    d.repositories = repositories;
    d.catalog_error = None;
    d.catalog_refresh_at = now() + 300;
    d.revision = revision;
    mark_pending(d);
    Ok(())
}

fn catalog_refresh_due(d: &Document, now: u64) -> bool {
    d.access_enabled && d.account.is_some() && !d.disconnect_pending && now >= d.catalog_refresh_at
}

fn sweep_retirement(
    retirement: &Mutex<TokenRetirement>,
    at: u64,
    flush: impl FnOnce() -> Result<(), String>,
    read: impl FnOnce() -> Result<TokenLedger, String>,
    mut retire: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    {
        let mut state = retirement.lock().unwrap_or_else(PoisonError::into_inner);
        if at < state.retry_at {
            return Ok(());
        }
        // Advance before network work so a completed push can request an earlier pass.
        state.retry_at = at + RETIREMENT_RETRY_AFTER;
    }
    // Store a ledger whose earlier write failed first, even when it has no keys left.
    flush()?;
    let mut failure = None;
    for name in read()?.keys() {
        if let Err(error) = retire(name) {
            failure = Some(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

/// Re-establish host-only grants after relaunch and renew them before expiry.
pub fn install(app: &tauri::AppHandle) {
    let _ = OBSERVATION_APP.set(app.clone());
    // Start the first device identity read now so the first GitHub view already has it.
    device_identity();
    if let Ok(document) = load(app) {
        crate::github_http::restore_retry_floors(&document.rate_retry);
    }
    let app = app.clone();
    std::thread::spawn(move || loop {
        let pending_deadline = PENDING
            .get_or_init(|| Mutex::new(None))
            .lock()
            .ok()
            .and_then(|p| *p);
        let pending_due = pending_deadline.is_some_and(|time| Instant::now() >= time);
        personal_token::check(&app);
        flush_account_credential();
        // Another GitHub operation holds the network lock: look again shortly.
        let mut wait = Duration::from_secs(1);
        if let Some(_network) = try_serialize(&OPERATION) {
            let observed = {
                let _state = serialize(&STATE);
                load(&app)
            };
            if let Ok(mut d) = observed {
                let due = worker_due(&d, pending_deadline, now(), Instant::now());
                if due {
                    if pending_due {
                        if let Ok(mut pending) = PENDING.get_or_init(|| Mutex::new(None)).lock() {
                            *pending = None;
                        }
                    }
                    // Refresh account/catalog independently. Never overwrite a newer desired document.
                    if !d.disconnect_pending
                        && d.account.is_some()
                        && credential()
                            .is_ok_and(|c| c.is_some_and(|c| c.expires_at <= now() + 120))
                    {
                        if let Err(message) = active_credential() {
                            {
                                let _state = serialize(&STATE);
                                if let Ok(mut current) = load(&app) {
                                    current.catalog_error = Some(message);
                                    let _ = save(&app, &current);
                                }
                            }
                        }
                    }
                    if catalog_refresh_due(&d, now()) && credential().is_ok_and(|c| c.is_some()) {
                        let result = active_credential().and_then(|c| catalog(&c));
                        {
                            let _state = serialize(&STATE);
                            if let Ok(mut current) = load(&app) {
                                match result {
                                    Ok(repos) => {
                                        if current.repositories != repos {
                                            current.access_pending = current
                                                .computers
                                                .iter()
                                                .filter_map(|w| {
                                                    w["computer"].as_str().map(str::to_owned)
                                                })
                                                .collect();
                                        }
                                        current.repositories = repos;
                                        current.catalog_error = None;
                                        current.catalog_refresh_at = now() + 300;
                                    }
                                    Err(message) => {
                                        current.catalog_error = Some(message);
                                        current.catalog_refresh_at =
                                            crate::github_http::retry_at().max(now() + 30);
                                    }
                                }
                                // A catalog can remove authority too; narrow before doing network work.
                                if let Err(message) = narrow_now(&app, &mut current) {
                                    current.catalog_error = Some(message);
                                }
                                let _ = save(&app, &current);
                                d = current;
                            }
                        }
                    }
                    if d.disconnect_pending {
                        let result = revocation_credential()
                            .and_then(|c| disconnect_token(c, now(), active_credential))
                            .and_then(|token| {
                                token.map_or(Ok(()), |token| {
                                    token_operation(
                                        Operation::RevokeAuthorization,
                                        json!({"accessToken":token}),
                                    )
                                    .map(|_| ())
                                })
                            });
                        // The credential store can wait on a permission prompt; never
                        // delete from it under STATE. Connect cannot interleave: this
                        // worker pass holds OPERATION.
                        let result = result.and_then(|()| delete_account_credential());
                        {
                            let _state = serialize(&STATE);
                            if let Ok(mut current) = load(&app) {
                                match result {
                                    Ok(()) => {
                                        current.disconnect_pending = false;
                                        current.account = None;
                                        current.repositories.clear();
                                        current.catalog_error = None;
                                    }
                                    Err(message) => {
                                        current.catalog_error = Some(message);
                                    }
                                }
                                let _ = save(&app, &current);
                                d = current;
                            }
                        }
                    }
                    let _ = apply(&app, &mut d, None, false);
                    let account_credential = credential();
                    {
                        let _state = serialize(&STATE);
                        if let Ok(mut current) = load(&app) {
                            current.session = session().into();
                            if current.computers.is_empty() {
                                current.refresh_at = now() + 3600;
                            }
                            let retry = crate::github_http::retry_at();
                            if retry > 0 {
                                current.refresh_at = current.refresh_at.min(retry.max(now() + 1));
                            }
                            if current.disconnect_pending {
                                current.refresh_at = current.refresh_at.min(if retry > 0 {
                                    retry.max(now() + 1)
                                } else {
                                    now() + 300
                                });
                            }
                            if let Ok(Some(c)) = account_credential {
                                current.refresh_at = current
                                    .refresh_at
                                    .min(c.expires_at.saturating_sub(120).max(now() + 30));
                            }
                            let _ = save(&app, &current);
                        }
                    }
                    // Re-evaluate promptly against the document this pass saved.
                    wait = WORKER_MIN_SLEEP;
                } else {
                    wait = worker_wait(&d, pending_deadline, now(), Instant::now());
                }
            } else {
                wait = Duration::from_secs(5);
            }
            let _ = sweep_retirement(
                retirement(),
                now(),
                || {
                    LEDGER_SECRET.flush(|ledger| {
                        ledger_entry().and_then(|entry| save_ledger(&entry, ledger))
                    })
                },
                || LEDGER_SECRET.read(|| ledger_entry().and_then(|entry| read_ledger(&entry))),
                |name| retire_unused(&app, name),
            );
        }
        // Sleep until the next deadline (not a 100 ms poll that re-read the whole
        // document for the app's lifetime); `schedule` wakes the worker early.
        let retirement_wait = retirement()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .wait(now());
        worker_sleep(
            wait.min(personal_token::next_check())
                .min(retirement_wait)
                .max(WORKER_MIN_SLEEP),
        );
    });
}

fn require_main(label: &str) -> Result<(), String> {
    if label == "main" {
        Ok(())
    } else {
        Err("Only the main window can manage GitHub access.".into())
    }
}

async fn run(
    app: tauri::AppHandle,
    f: fn(&tauri::AppHandle) -> Result<Value, String>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _update = crate::updates::operation_guard()?;
        let _guard = serialize(&OPERATION);
        f(&app)
    })
    .await
    .map_err(|_| "GitHub operation failed.")?
}
#[tauri::command]
pub async fn read_github_state(app: tauri::AppHandle) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || snapshot(&app))
        .await
        .map_err(|_| "GitHub state could not be read.")?
}
#[tauri::command]
pub async fn connect_github(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Value, String> {
    require_main(window.label())?;
    retry_credential_access();
    let generation = CANCELLATION.load(Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        // One connection flow at a time; it takes OPERATION only for its network steps.
        let _flow = serialize(&CONNECTION_FLOW);
        connect(&app, generation)
    })
    .await
    .map_err(|_| "GitHub operation failed.")?
}
#[tauri::command]
pub async fn cancel_github_connection(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Value, String> {
    require_main(window.label())?;
    tauri::async_runtime::spawn_blocking(move || {
        // Serialize with credential publication, not with the browser/network wait.
        let _state = serialize(&STATE);
        let mut pending = AUTHORIZATION
            .lock()
            .map_err(|_| "GitHub connection is unavailable.")?;
        CANCELLATION.fetch_add(1, Ordering::SeqCst);
        pending.0 = None;
        CONNECTING.store(false, Ordering::SeqCst);
        let result = snapshot(&app);
        let _ = app.emit("silo://application-state-changed", ());
        result
    })
    .await
    .map_err(|_| "GitHub cancellation failed.")?
}

#[tauri::command]
pub async fn reopen_github_authorization(window: tauri::WebviewWindow) -> Result<(), String> {
    require_main(window.label())?;
    tauri::async_runtime::spawn_blocking(move || {
        let url = AUTHORIZATION
            .lock()
            .map_err(|_| "GitHub connection is unavailable.")?
            .url(CANCELLATION.load(Ordering::SeqCst))?;
        open_browser(window.app_handle(), &url)
    })
    .await
    .map_err(|_| "Cannot reopen GitHub authorization.")?
}

#[tauri::command]
pub async fn manage_github_repositories(window: tauri::WebviewWindow) -> Result<(), String> {
    require_main(window.label())?;
    tauri::async_runtime::spawn_blocking(move || {
        let slug = APP_SLUG.ok_or("GitHub App is not configured in this build.")?;
        if !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err("Invalid GitHub App slug.".into());
        }
        open_browser(
            window.app_handle(),
            &format!("https://github.com/apps/{slug}/installations/new"),
        )
    })
    .await
    .map_err(|_| "Cannot open GitHub repository access.")?
}

#[tauri::command]
pub async fn refresh_github_repositories(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Value, String> {
    require_main(window.label())?;
    retry_credential_access();
    run(app, |app| {
        let result = active_credential().and_then(|c| catalog_with_retry(&c, catalog));
        let _state = serialize(&STATE);
        let mut d = load(app)?;
        match result {
            Ok(repos) => {
                if d.repositories != repos {
                    mark_pending(&mut d);
                }
                d.repositories = repos;
                d.catalog_error = None;
                d.catalog_refresh_at = now() + 300;
            }
            Err(message) => {
                d.catalog_error = Some(message);
            }
        }
        if let Err(message) = narrow_now(app, &mut d) {
            d.catalog_error = Some(message);
        }
        save(app, &d)?;
        schedule(Duration::from_millis(500));
        snapshot(app)
    })
    .await
}
#[tauri::command]
pub async fn disconnect_github(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Value, String> {
    require_main(window.label())?;
    retry_credential_access();
    let ticket = INTENTS.ticket();
    CANCELLATION.fetch_add(1, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        let _turn = ticket.wait()?;
        let _update = crate::updates::operation_guard()?;
        let _state = serialize(&STATE);
        let mut d = load(&app)?;
        d.revision = next_policy_revision(d.revision)?;
        d.access_enabled = false;
        d.disconnect_pending = true;
        mark_pending(&mut d);
        save(&app, &d)?;
        let result = narrow_now(&app, &mut d);
        schedule(Duration::ZERO);
        result?;
        snapshot(&app)
    })
    .await
    .map_err(|_| "GitHub operation failed.")?
}
#[tauri::command]
pub async fn set_github_access_enabled(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    enabled: bool,
) -> Result<Value, String> {
    require_main(window.label())?;
    let ticket = INTENTS.ticket();
    if !enabled {
        CANCELLATION.fetch_add(1, Ordering::SeqCst);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let _turn = ticket.wait()?;
        let _update = crate::updates::operation_guard()?;
        let _state = serialize(&STATE);
        let mut d = load(&app)?;
        if d.access_enabled == enabled {
            return snapshot(&app);
        }
        d.revision = next_policy_revision(d.revision)?;
        d.access_enabled = enabled;
        mark_pending(&mut d);
        save(&app, &d)?;
        let result = narrow_now(&app, &mut d);
        schedule(Duration::from_millis(500));
        result?;
        snapshot(&app)
    })
    .await
    .map_err(|_| "GitHub operation failed.")?
}
fn validate_method_change(
    previous: Option<&Value>,
    policy: &Value,
    oauth_connected: bool,
    token_connected: bool,
) -> Result<(), String> {
    let previous = previous
        .and_then(|w| w["authenticationMethod"].as_str())
        .unwrap_or("oauth");
    let method = policy["authenticationMethod"].as_str().unwrap_or("oauth");
    if method != previous {
        if method == "token" && !token_connected {
            return Err("Connect a personal token before selecting it.".into());
        }
        if method == "oauth" && !oauth_connected {
            return Err("Connect GitHub OAuth before selecting it.".into());
        }
    }
    Ok(())
}

fn access_choice(policy: &Value) -> Value {
    let all = policy["repositoryMode"] == "all";
    let mut repos = policy["repositories"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    repos.sort_by(|a, b| a["repository"].as_str().cmp(&b["repository"].as_str()));
    if all {
        json!({"method":policy["authenticationMethod"].as_str().unwrap_or("oauth"),"all":true,"changes":policy["allRepositoriesAllowChanges"]})
    } else {
        json!({"method":policy["authenticationMethod"].as_str().unwrap_or("oauth"),"all":false,"repositories":repos})
    }
}
fn mark_pending_for(d: &mut Document, names: &[String]) {
    for name in names {
        d.operations
            .retain(|op| op["computer"].as_str() != Some(name));
        d.operations.push(
            json!({"computer":name,"status":"applying","message":"Applying GitHub settings."}),
        );
    }
}
fn mark_pending(d: &mut Document) {
    d.access_pending = d
        .computers
        .iter()
        .filter_map(|w| w["computer"].as_str().map(str::to_owned))
        .collect();
    mark_pending_for(d, &d.access_pending.clone());
}
/// Apply saved computer choices as per-computer patches: computers not listed keep their
/// choices (for example an assignment a fork just copied), and a patch built from a view
/// older than a change it would overwrite is refused (see `stale_save`). Returns the
/// computers now pending, or `None` when nothing changed.
fn apply_patches(
    d: &mut Document,
    patches: &[Value],
    base: Option<u64>,
    oauth_connected: bool,
    token_connected: bool,
) -> Result<Option<Vec<String>>, String> {
    let mut changed_policies = Vec::new();
    let mut access_changed = d.access_pending.clone();
    let mut identity_changed = Vec::new();
    for w in patches {
        let name = w["computer"].as_str().ok_or("Invalid computer policy.")?;
        let previous = d
            .computers
            .iter()
            .find(|old| old["computer"] == w["computer"]);
        if previous == Some(w) {
            continue;
        }
        if stale_save(d, name, base) {
            return Err(format!(
                "GitHub settings for {name} changed while you were editing them. Review them and try again."
            ));
        }
        validate_method_change(previous, w, oauth_connected, token_connected)?;
        if previous.is_none_or(|old| access_choice(old) != access_choice(w))
            && !access_changed.iter().any(|n| n == name)
        {
            access_changed.push(name.to_owned());
        }
        if previous.is_none_or(|old| old["identity"] != w["identity"]) {
            identity_changed.push(name.to_owned());
        }
        changed_policies.push(w);
    }
    if changed_policies.is_empty() {
        return Ok(None);
    }
    let mut computers = d.computers.clone();
    for w in &changed_policies {
        match computers
            .iter_mut()
            .find(|old| old["computer"] == w["computer"])
        {
            Some(slot) => *slot = (*w).clone(),
            None => computers.push((*w).clone()),
        }
    }
    validate(&computers)?;
    let revision = next_policy_revision(d.revision)?;
    d.computers = computers;
    d.revision = revision;
    for w in &changed_policies {
        if let Some(name) = w["computer"].as_str() {
            stamp(d, name, base);
        }
    }
    for name in identity_changed {
        if !d.identity_pending.contains(&name) {
            d.identity_pending.push(name);
        }
    }
    let mut changed = access_changed.clone();
    changed.extend(d.identity_pending.iter().cloned());
    changed.sort();
    changed.dedup();
    mark_pending_for(d, &changed);
    d.access_pending = access_changed;
    Ok(Some(changed))
}
/// Save computer choices: `computers` holds only the computers the caller changed, and
/// `baseRevision` the `policyRevision` its view was based on. Access on/off is never
/// changed here (a stale save must not undo Disable access); use
/// `set_github_access_enabled`. An `accessEnabled` field is ignored.
#[tauri::command]
pub async fn save_github_configuration(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    configuration: Value,
) -> Result<Value, String> {
    require_main(window.label())?;
    let ws = configuration["computers"]
        .as_array()
        .ok_or("Missing computer policies.")?;
    validate(ws)?;
    let base = match &configuration["baseRevision"] {
        Value::Null => None,
        value => Some(value.as_u64().ok_or("Invalid GitHub settings revision.")?),
    };
    let ticket = INTENTS.ticket();
    tauri::async_runtime::spawn_blocking(move || {
        let ws = configuration["computers"]
            .as_array()
            .ok_or("Missing computer policies.")?;
        let _turn = ticket.wait()?;
        let _update = crate::updates::operation_guard()?;
        let _state = serialize(&STATE);
        let mut d = load(&app)?;
        let oauth_connected = observed_credential()
            .is_some_and(|v| v.is_ok_and(|expiry| expiry.is_some_and(|at| at > now())));
        let Some(changed) = apply_patches(
            &mut d,
            ws,
            base,
            oauth_connected,
            personal_token::connected(),
        )?
        else {
            return snapshot(&app);
        };
        CANCELLATION.fetch_add(1, Ordering::SeqCst);
        // Persist first so a worker completing concurrently cannot publish old choices.
        save(&app, &d)?;
        let result = narrow_now(&app, &mut d);
        schedule(Duration::from_millis(500));
        if let Err(message) = result {
            for op in d.operations.iter_mut().filter(|op| {
                op["computer"]
                    .as_str()
                    .is_some_and(|name| changed.iter().any(|changed| changed == name))
            }) {
                op["status"] = json!("failed");
                op["message"] = json!(&message);
                op["canRetry"] = json!(true);
            }
            save(&app, &d)?;
        }
        snapshot(&app)
    })
    .await
    .map_err(|_| "GitHub operation failed.")?
}
fn prepare_retry(d: &mut Document, computer: Option<&str>) {
    let names: Vec<String> = d
        .computers
        .iter()
        .filter_map(|w| w["computer"].as_str())
        .filter(|name| computer.is_none_or(|target| target == *name))
        .map(str::to_owned)
        .collect();
    for name in &names {
        if d.identity_errors.contains_key(name) && !d.identity_pending.contains(name) {
            d.identity_pending.push(name.clone());
        }
        if !d.access_pending.contains(name) {
            d.access_pending.push(name.clone());
        }
    }
    mark_pending_for(d, &names);
}

#[tauri::command]
pub async fn retry_github_configuration(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    computer: Option<String>,
) -> Result<Value, String> {
    require_main(window.label())?;
    retry_credential_access();
    let ticket = INTENTS.ticket();
    tauri::async_runtime::spawn_blocking(move || {
        let _turn = ticket.wait()?;
        let _update = crate::updates::operation_guard()?;
        let _state = serialize(&STATE);
        let mut d = load(&app)?;
        if let Some(computer) = computer.as_deref() {
            crate::github_http::reset_computer_retries(computer);
        } else {
            crate::github_http::reset_retries();
        }
        prepare_retry(&mut d, computer.as_deref());
        save(&app, &d)?;
        schedule(Duration::ZERO);
        snapshot(&app)
    })
    .await
    .map_err(|_| "GitHub operation failed.")?
}

#[cfg(test)]
mod tests {

    #[test]
    fn state_refreshes_reparse_the_configuration_only_when_it_changes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("github.json");
        let write = |revision: u64| {
            let document = super::Document {
                revision,
                ..super::Document::default()
            };
            super::save_at(&path, &document).unwrap();
        };
        write(1);
        assert_eq!(super::load_observed(&path).unwrap().revision, 1);
        // An unchanged file keeps its cached reading.
        assert_eq!(
            super::OBSERVED.lock().unwrap().as_ref().unwrap().1,
            super::file_identity(&path).unwrap()
        );
        // A change in place, even one that keeps the size and modification time, is seen.
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        let modified = file.metadata().unwrap().modified().unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, original.replace("\"revision\":1", "\"revision\":2")).unwrap();
        file.set_modified(modified).unwrap();
        assert_eq!(super::load_observed(&path).unwrap().revision, 2);
        // A save replaces the file and the next read sees it.
        write(3);
        assert_eq!(super::load_observed(&path).unwrap().revision, 3);
    }

    #[test]
    fn migrated_repository_settings_load() {
        let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
        let document = super::load_at(&migrated.app_data.join("github.json")).unwrap();
        assert_eq!(document.computers.len(), 1);
        assert_eq!(document.computers[0]["computer"], "dev");
        assert_eq!(document.operations[0]["computer"], "dev");
    }
    #[test]
    fn repository_refresh_keeps_unrelated_token_operations_stopped() {
        crate::github_http::assert_bearer_retry_isolated(|token| {
            let credential = fixture_credential(token, super::now() + 3600);
            let result = super::catalog_with_retry(&credential, |credential| {
                assert_eq!(credential.access_token, token);
                Ok(vec![serde_json::json!({"fixture": true})])
            })
            .unwrap();
            assert_eq!(result, vec![serde_json::json!({"fixture": true})]);
        });
    }
    #[test]
    fn a_panic_under_the_github_locks_does_not_block_updates_or_settings() {
        let _test_state = crate::test_support::global_state();
        for lock in [&super::OPERATION, &super::STATE] {
            let _ = std::thread::spawn(move || {
                let _guard = lock.lock().unwrap();
                panic!("simulated panic while holding a GitHub lock");
            })
            .join();
            assert!(lock.is_poisoned());
        }
        // K-24: a poisoned lock is not "busy" forever.
        drop(super::update_guard().unwrap());
        assert!(crate::sync::try_lock_or_recover(&super::STATE, "GitHub state").is_some());
        assert!(!super::OPERATION.is_poisoned() && !super::STATE.is_poisoned());
    }

    #[test]
    fn one_failing_detach_does_not_stop_the_others() {
        let _test_state = crate::test_support::global_state();
        let mut errors = super::NarrowErrors::default();
        let mut detached = Vec::new();
        super::each_computer([("a", 1), ("b", 2), ("c", 3)], &mut errors, |name, _| {
            detached.push(name.to_owned());
            if name == "b" {
                Err("b failed".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(detached, ["a", "b", "c"]);
        assert_eq!(
            errors.for_computer("b").map(String::as_str),
            Some("b failed")
        );
        assert_eq!(errors.for_computer("a"), None);
        assert_eq!(errors.for_computer("c"), None);
        assert_eq!(errors.into_result(), Err("b failed".into()));
    }
    #[test]
    fn a_general_narrowing_failure_applies_to_every_computer() {
        let _test_state = crate::test_support::global_state();
        let mut errors = super::NarrowErrors::default();
        errors.record_all("storage".into());
        assert_eq!(
            errors.for_computer("any").map(String::as_str),
            Some("storage")
        );
    }
    #[test]
    fn session_secret_reads_once_and_writes_only_changes() {
        let _test_state = crate::test_support::global_state();
        let cache = super::SessionSecret::new();
        assert_eq!(cache.read(|| Ok(Some(1))).unwrap(), Some(1));
        assert_eq!(
            cache.read(|| panic!("Repeated Keychain read")).unwrap(),
            Some(1)
        );
        cache
            .write(Some(1), || panic!("Unchanged Keychain write"))
            .unwrap();
        cache.write(Some(2), || Ok(())).unwrap();
        assert_eq!(cache.read(|| panic!("Read after write")).unwrap(), Some(2));
        cache.write(None, || Ok(())).unwrap();
        assert_eq!(
            cache.read(|| panic!("Read after disconnect")).unwrap(),
            None
        );
    }
    #[test]
    fn denied_keychain_access_waits_for_explicit_retry() {
        let _test_state = crate::test_support::global_state();
        let cache = super::SessionSecret::<Option<u64>>::new();
        assert!(cache.read(|| Err("Denied".into())).is_err());
        assert!(cache.read(|| panic!("Automatic permission retry")).is_err());
        assert!(cache
            .write(Some(1), || panic!("Write after denial"))
            .is_err());
        cache.retry();
        assert_eq!(cache.read(|| Ok(Some(1))).unwrap(), Some(1));
        assert!(cache.write(Some(2), || Err("Write denied".into())).is_err());
        assert!(cache
            .write(Some(2), || panic!("Automatic write retry"))
            .is_err());
        cache.retry();
        cache.write(Some(2), || Ok(())).unwrap();
        assert_eq!(cache.read(|| panic!("Read after write")).unwrap(), Some(2));
    }
    #[test]
    fn failed_reconnection_preserves_the_previous_credential_and_observation() {
        let secret = SessionSecret::new();
        let old = fixture_credential("previous-account", now() + 600);
        let new = fixture_credential("new-account", now() + 900);
        let stored = std::cell::RefCell::new(Some(old.clone()));
        let observed = std::cell::RefCell::new(Ok(Some(observed_expiry(&old))));
        secret.read(|| Ok(stored.borrow().clone())).unwrap();
        assert!(replace_connection_credential(
            &secret,
            &new,
            || Err("store denied".into()),
            |value| *observed.borrow_mut() = value,
        )
        .is_err());
        assert!(secret.peek() == Some(Ok(Some(old.clone()))));
        assert!(*observed.borrow() == Ok(Some(observed_expiry(&old))));
        secret
            .flush(|_| panic!("failed reconnect must not be retried as a renewal"))
            .unwrap();
        assert!(*stored.borrow() == Some(old));
        replace_connection_credential(
            &secret,
            &new,
            || {
                *stored.borrow_mut() = Some(new.clone());
                Ok(())
            },
            |value| *observed.borrow_mut() = value,
        )
        .unwrap();
        assert!(secret.peek() == Some(Ok(Some(new.clone()))));
        assert!(*stored.borrow() == Some(new.clone()));
        assert!(*observed.borrow() == Ok(Some(observed_expiry(&new))));
    }
    #[test]
    fn expired_credential_with_refresh_token_stays_connected() {
        let _test_state = crate::test_support::global_state();
        let mut c = super::Credential {
            access_token: "a".into(),
            refresh_token: Some("r".into()),
            expires_at: 1,
        };
        assert!(super::observed_expiry(&c) > super::now());
        c.refresh_token = None;
        assert_eq!(super::observed_expiry(&c), 1);
    }
    #[test]
    fn failed_store_keeps_the_new_value_usable_and_retries_on_flush() {
        let _test_state = crate::test_support::global_state();
        let cache = super::SessionSecret::new();
        assert_eq!(cache.read(|| Ok(Some(1))).unwrap(), Some(1));
        assert!(cache.write(Some(2), || Err("store locked".into())).is_err());
        // The renewed value is used in memory; storage is not retried by later writes.
        assert_eq!(
            cache.read(|| panic!("Read after failed write")).unwrap(),
            Some(2)
        );
        assert!(cache
            .write(Some(2), || panic!("Automatic write retry"))
            .is_err());
        cache
            .flush(|value| {
                assert_eq!(*value, Some(2));
                Ok(())
            })
            .unwrap();
        cache
            .flush(|_| panic!("Flush after successful store"))
            .unwrap();
        cache
            .write(Some(2), || panic!("Unchanged write after flush"))
            .unwrap();
    }
    #[test]
    fn peeking_a_secret_never_waits_for_the_credential_store() {
        let _test_state = crate::test_support::global_state();
        let cache = super::SessionSecret::<Option<u64>>::new();
        assert_eq!(cache.peek(), None);
        let (started, reading) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                cache.read(move || {
                    started.send(()).unwrap();
                    released.recv().unwrap();
                    Ok(Some(1))
                })
            });
            reading.recv_timeout(Duration::from_secs(5)).unwrap();
            // A read waiting on the store (or its permission prompt) does not block peek.
            assert_eq!(cache.peek(), None);
            release.send(()).unwrap();
        });
        assert_eq!(cache.peek(), Some(Ok(Some(1))));
        // A failed write keeps the newest value in memory, and peek sees it too.
        assert!(cache.write(Some(2), || Err("store locked".into())).is_err());
        assert_eq!(cache.peek(), Some(Ok(Some(2))));
    }
    #[test]
    fn concurrent_secret_reads_share_one_keychain_request() {
        let _test_state = crate::test_support::global_state();
        let cache = super::SessionSecret::new();
        let reads = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    assert_eq!(
                        cache
                            .read(|| {
                                reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                Ok(Some(1))
                            })
                            .unwrap(),
                        Some(1)
                    )
                });
            }
        });
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    use super::*;
    fn test_scope() -> GrantScope {
        GrantScope {
            owner: 1,
            login: "owner".into(),
            ids: vec![1, 2, 3],
            writes: vec![2],
            all: false,
        }
    }
    fn test_grant() -> RuntimeGrant {
        RuntimeGrant {
            owner_id: 1,
            owner_login: "owner".into(),
            repository_ids: vec![1, 2, 3],
            read_token: "read".into(),
            write_token: Some("write".into()),
            write_repository_ids: vec![2],
            expires_at: now() + 1000,
            read_expires_at: now() + 1000,
            write_expires_at: now() + 1000,
            all_repositories: false,
        }
    }
    #[test]
    fn rapid_edits_delay_network_until_latest_half_second_deadline() {
        let _test_state = crate::test_support::global_state();
        let d = Document {
            session: session().into(),
            refresh_at: 0,
            ..Default::default()
        };
        let first = Instant::now();
        let latest = first + Duration::from_millis(400);
        assert!(!worker_due(
            &d,
            Some(latest + Duration::from_millis(500)),
            100,
            first + Duration::from_millis(500)
        ));
        assert!(worker_due(
            &d,
            Some(latest + Duration::from_millis(500)),
            100,
            first + Duration::from_millis(900)
        ));
    }
    fn saved_policy(name: &str, all: bool) -> Value {
        json!({"computer":name,"repositoryMode":if all {"all"} else {"selected"},"allRepositoriesAllowChanges":all,
            "repositories":[],"identity":{"name":"","email":"","apply":false}})
    }
    #[test]
    fn additive_github_preferences_survive_a_known_setting_change() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("github.json");
        let saved = json!({
            "revision": 4,
            "accessEnabled": false,
            "account": "fixture-account",
            "computers": [saved_policy("dev", false)],
            "futurePreference": {"mode": "newer", "enabled": true}
        });
        let bytes = serde_json::to_vec(&saved).unwrap();
        fs::write(&path, &bytes).unwrap();
        let mut document = load_at(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        document.access_enabled = true;
        save_at(&path, &document).unwrap();
        let reloaded: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(reloaded["futurePreference"], saved["futurePreference"]);
        assert_eq!(reloaded["computers"], saved["computers"]);
        assert_eq!(reloaded["account"], saved["account"]);
        assert!(load_at(&path).unwrap().access_enabled);
    }

    #[test]
    fn legacy_github_preferences_keep_safe_defaults_after_save_and_reload() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("github.json");
        fs::write(&path, br#"{"revision":0,"accessEnabled":false}"#).unwrap();
        let document = load_at(&path).unwrap();
        assert!(!document.access_enabled);
        assert!(document.account.is_none());
        assert!(document.computers.is_empty());
        assert!(document.policy_stamps.is_empty());
        assert!(!document.grants_issued);
        save_at(&path, &document).unwrap();
        let reloaded = load_at(&path).unwrap();
        assert!(!reloaded.access_enabled);
        assert!(reloaded.account.is_none());
        assert!(reloaded.computers.is_empty());
        assert!(reloaded.policy_stamps.is_empty());
        assert!(!reloaded.grants_issued);
    }

    #[test]
    fn unsafe_saved_policy_revisions_are_refused_without_rewriting_the_document() {
        for revision in [9_007_199_254_740_992, u64::MAX] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("github.json");
            let bytes = serde_json::to_vec(&Document {
                revision,
                ..Default::default()
            })
            .unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(load_at(&path).is_err(), "revision {revision} was accepted");
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
    #[test]
    fn policy_revision_exhaustion_refuses_edits_without_changing_saved_choices() {
        let _test_state = crate::test_support::global_state();
        for revision in [9_007_199_254_740_991, u64::MAX] {
            let mut d = Document {
                revision,
                computers: vec![saved_policy("dev", false)],
                ..Default::default()
            };
            let before = serde_json::to_value(&d).unwrap();
            assert!(
                apply_patches(&mut d, &[saved_policy("dev", true)], None, true, false).is_err()
            );
            assert_eq!(serde_json::to_value(&d).unwrap(), before);
        }
    }
    #[test]
    fn the_last_safe_policy_revision_is_saved_and_noop_edits_still_succeed() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("github.json");
        let mut d = Document {
            revision: 9_007_199_254_740_990,
            computers: vec![saved_policy("dev", false)],
            ..Default::default()
        };
        assert!(
            apply_patches(&mut d, &[saved_policy("dev", true)], None, true, false)
                .unwrap()
                .is_some()
        );
        assert_eq!(d.revision, 9_007_199_254_740_991);
        assert_eq!(d.policy_stamps["dev"].revision, 9_007_199_254_740_991);
        save_at(&path, &d).unwrap();
        assert_eq!(load_at(&path).unwrap().revision, 9_007_199_254_740_991);
        assert_eq!(
            apply_patches(&mut d, &[saved_policy("dev", true)], None, true, false).unwrap(),
            None
        );
    }
    #[test]
    fn a_save_patches_only_its_computers_and_never_changes_access() {
        let _test_state = crate::test_support::global_state();
        let mut d = Document {
            revision: 5,
            access_enabled: false,
            computers: vec![saved_policy("dev", false)],
            ..Default::default()
        };
        // A fork copied its source's assignment after the page last read the settings.
        d.computers.push(saved_policy("fork", true));
        stamp(&mut d, "fork", None);
        let changed = apply_patches(&mut d, &[saved_policy("dev", true)], Some(4), true, false)
            .unwrap()
            .unwrap();
        assert_eq!(changed, vec!["dev".to_string()]);
        assert_eq!(
            d.computers,
            vec![saved_policy("dev", true), saved_policy("fork", true)],
            "the fork's copied assignment was dropped"
        );
        assert!(
            !d.access_enabled,
            "a save re-enabled access after Disable access"
        );
        assert_eq!(d.revision, 6);
        assert_eq!(
            d.policy_stamps["dev"],
            PolicyStamp {
                revision: 6,
                base: Some(4)
            }
        );
        // Saving what is already stored changes nothing.
        assert_eq!(
            apply_patches(&mut d, &[saved_policy("dev", true)], Some(4), true, false).unwrap(),
            None
        );
        assert_eq!(d.revision, 6);
    }
    #[test]
    fn a_save_from_a_view_older_than_a_change_it_would_overwrite_is_refused() {
        let _test_state = crate::test_support::global_state();
        let mut d = Document {
            revision: 5,
            computers: vec![saved_policy("fork", true)],
            ..Default::default()
        };
        stamp(&mut d, "fork", None);
        // The page still showed the fork without its copied assignment.
        assert!(
            apply_patches(&mut d, &[saved_policy("fork", false)], Some(4), true, false).is_err()
        );
        assert_eq!(d.computers, vec![saved_policy("fork", true)]);
        assert_eq!(d.revision, 5);
        // After seeing it, the same edit applies.
        assert!(
            apply_patches(&mut d, &[saved_policy("fork", false)], Some(5), true, false)
                .unwrap()
                .is_some()
        );
        // Rapid edits sent from one view before its first result arrived apply in order.
        let mut edit = saved_policy("fork", false);
        edit["identity"] = json!({"name":"Name","email":"name@example.test","apply":true});
        assert!(apply_patches(&mut d, &[edit.clone()], Some(5), true, false)
            .unwrap()
            .is_some());
        assert_eq!(d.computers, vec![edit.clone()]);
        // A save from a newer view wins over a later-arriving one from an older view.
        assert!(
            apply_patches(&mut d, &[saved_policy("fork", true)], Some(7), true, false)
                .unwrap()
                .is_some()
        );
        assert!(apply_patches(&mut d, &[edit], Some(5), true, false).is_err());
        // A deleted computer's stale choices are not brought back for a new one with its name.
        forget_computer(&mut d, "fork");
        d.revision += 1;
        stamp(&mut d, "fork", None);
        assert!(
            apply_patches(&mut d, &[saved_policy("fork", true)], Some(8), true, false).is_err()
        );
        assert!(d.computers.is_empty());
        // A caller without a base revision is not checked.
        assert!(
            apply_patches(&mut d, &[saved_policy("fork", true)], None, true, false)
                .unwrap()
                .is_some()
        );
    }
    #[test]
    fn a_deleted_computer_leaves_no_assignment_or_attachment_for_a_new_one_with_its_name() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let document = directory.path().join("github.json");
        let policy = |name: &str| {
            json!({"computer":name,"repositoryMode":"all","allRepositoriesAllowChanges":true,"repositories":[],
            "identity":{"name":"","email":"","apply":false}})
        };
        let d = Document {
            revision: 4,
            access_enabled: true,
            computers: vec![policy("dev"), policy("other")],
            access_pending: vec!["dev".into()],
            identity_pending: vec!["dev".into()],
            operations: vec![
                json!({"computer":"dev","status":"failed","message":"old","canRetry":true}),
            ],
            access_errors: [("dev".to_string(), "old".to_string())].into(),
            identity_errors: [("dev".to_string(), "old".to_string())].into(),
            ..Default::default()
        };
        save_at(&document, &d).unwrap();
        let key = format!("{}:dev", document.display());
        active()
            .lock()
            .unwrap()
            .insert(key.clone(), vec![test_grant()]);
        issued().lock().unwrap().insert(
            key.clone(),
            vec![IssuedToken {
                owner: 1,
                all: true,
                write: true,
                ids: vec![],
                token: "old-write".into(),
                expires_at: now() + 1000,
            }],
        );
        use_test_document(Some(document.clone()));
        let result = computer_removed("dev");
        use_test_document(None);
        result.unwrap();
        let after = load_at(&document).unwrap();
        assert_eq!(after.computers, vec![policy("other")]);
        assert!(after.access_pending.is_empty() && after.identity_pending.is_empty());
        assert!(
            after.operations.is_empty()
                && after.access_errors.is_empty()
                && after.identity_errors.is_empty()
        );
        assert!(after.revision > 4);
        // No grant or issued token of the deleted computer is reused or kept live; the
        // retirement ledger revokes them because no policy names "dev" any more.
        assert!(!active().lock().unwrap().contains_key(&key));
        assert!(!issued().lock().unwrap().contains_key(&key));
        assert!(after.computers.iter().all(|w| w["computer"] != "dev"));
        *PENDING.get_or_init(|| Mutex::new(None)).lock().unwrap() = None;
        // Without an installed document there is nothing to forget.
        assert!(computer_removed("dev").is_ok());
    }
    #[test]
    fn idle_worker_sleeps_until_its_next_deadline_instead_of_polling() {
        let _test_state = crate::test_support::global_state();
        let instant = Instant::now();
        let mut d = Document {
            session: session().into(),
            refresh_at: 1_000 + 3_600,
            ..Default::default()
        };
        // Nothing due for an hour: sleep the longest bounded interval, not 100 ms.
        assert_eq!(worker_wait(&d, None, 1_000, instant), WORKER_MAX_SLEEP);
        d.refresh_at = 1_010;
        assert_eq!(
            worker_wait(&d, None, 1_000, instant),
            Duration::from_secs(10)
        );
        // A connected account also wakes for its catalog refresh.
        d.access_enabled = true;
        d.account = Some("owner".into());
        d.catalog_refresh_at = 1_003;
        assert_eq!(
            worker_wait(&d, None, 1_000, instant),
            Duration::from_secs(3)
        );
        // A scheduled edit wakes at its (debounced) deadline.
        let deadline = instant + Duration::from_millis(500);
        assert_eq!(
            worker_wait(&d, Some(deadline), 1_000, instant),
            Duration::from_millis(500)
        );
        // A new app session is due at once (never faster than the minimum sleep).
        d.session = "older".into();
        assert_eq!(worker_wait(&d, None, 1_000, instant), WORKER_MIN_SLEEP);
    }
    #[test]
    fn scheduling_wakes_a_sleeping_worker() {
        let _test_state = crate::test_support::global_state();
        // Other tests schedule work without a worker; start from a consumed wake-up.
        *WORKER_WOKEN.lock().unwrap() = false;
        let (done, finished) = std::sync::mpsc::channel();
        let (waiting, ready) = std::sync::mpsc::sync_channel(0);
        let sleeper = std::thread::spawn(move || {
            let started = Instant::now();
            worker_sleep_with(Duration::from_secs(30), || waiting.send(()).unwrap());
            done.send(started.elapsed()).unwrap();
        });
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        // schedule must acquire the predicate lock after the worker enters its wait.
        schedule(Duration::ZERO);
        assert!(finished.recv_timeout(Duration::from_secs(5)).unwrap() < Duration::from_secs(5));
        sleeper.join().unwrap();
        // The wake-up was consumed: the next sleep waits for its timeout.
        let started = Instant::now();
        worker_sleep(Duration::from_millis(50));
        assert!(started.elapsed() >= Duration::from_millis(50));
        *PENDING.get_or_init(|| Mutex::new(None)).lock().unwrap() = None;
    }
    #[test]
    fn identity_only_edit_does_not_request_access_or_postpone_renewal() {
        let _test_state = crate::test_support::global_state();
        let d = Document {
            session: session().into(),
            refresh_at: 160,
            identity_pending: vec!["dev".into()],
            ..Default::default()
        };
        assert!(!access_update_due(&d, "dev", 100, false, &[]));
        assert!(access_update_due(&d, "dev", 160, false, &[]));
        assert_eq!(d.refresh_at, 160);
    }
    #[test]
    fn verified_computer_is_not_reapplied_at_another_computers_retry_deadline() {
        let _test_state = crate::test_support::global_state();
        let mut d = Document {
            session: session().into(),
            refresh_at: 160,
            operations: vec![
                json!({"computer":"dev","status":"succeeded","message":"GitHub access verified."}),
            ],
            ..Document::default()
        };
        let grant = |expires_at| RuntimeGrant {
            owner_id: 1,
            owner_login: "o".into(),
            repository_ids: vec![],
            read_token: "r".into(),
            write_token: None,
            write_repository_ids: vec![],
            expires_at,
            read_expires_at: expires_at,
            write_expires_at: expires_at,
            all_repositories: false,
        };
        // Deadline reached, nothing changed and grants are still valid: no re-apply.
        assert!(!access_update_due(&d, "dev", 200, false, &[grant(3600)]));
        // Its own grants near expiry, a restore, a pending edit or a failed attempt still apply.
        assert!(access_update_due(&d, "dev", 200, false, &[grant(300)]));
        assert!(access_update_due(&d, "dev", 100, true, &[grant(3600)]));
        d.access_pending.push("dev".into());
        assert!(access_update_due(&d, "dev", 100, false, &[]));
        d.access_pending.clear();
        d.operations[0]["status"] = json!("failed");
        assert!(access_update_due(&d, "dev", 200, false, &[grant(3600)]));
        // A stale session is verified again.
        d.operations[0]["status"] = json!("succeeded");
        d.session = "older".into();
        assert!(access_update_due(&d, "dev", 100, false, &[grant(3600)]));
    }
    #[test]
    fn restored_computer_is_taken_once() {
        let _test_state = crate::test_support::global_state();
        computer_restored("restored-once");
        assert!(take_restored("restored-once"));
        assert!(!take_restored("restored-once"));
    }
    #[test]
    fn repository_catalog_has_independent_refresh_deadline() {
        let _test_state = crate::test_support::global_state();
        let d = Document {
            session: session().into(),
            access_enabled: true,
            account: Some("owner".into()),
            refresh_at: 3600,
            catalog_refresh_at: 300,
            computers: vec![json!({"repositoryMode":"all"})],
            ..Default::default()
        };
        assert!(!worker_due(&d, None, 299, Instant::now()));
        assert!(worker_due(&d, None, 300, Instant::now()));
    }
    #[test]
    fn local_edits_preserve_submission_order_without_holding_network_lock() {
        let _test_state = crate::test_support::global_state();
        let queue = IntentQueue::new();
        let first = queue.ticket();
        let second = queue.ticket();
        let first_turn = first.wait().unwrap();
        let (sent, received) = std::sync::mpsc::channel();
        let (waiting, ready) = std::sync::mpsc::sync_channel(0);
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let _turn = second.wait_with(|| waiting.send(()).unwrap()).unwrap();
                sent.send("second applied").unwrap();
            });
            let waiting = ready.recv_timeout(Duration::from_secs(5));
            let premature = received.try_recv();
            // Release before asserting, so a failed barrier cannot strand the scoped worker.
            drop(first_turn);
            waiting.unwrap();
            assert!(matches!(
                premature,
                Err(std::sync::mpsc::TryRecvError::Empty)
            ));
            assert_eq!(
                received.recv_timeout(Duration::from_secs(5)).unwrap(),
                "second applied"
            );
        });
    }
    thread_local! {
        static DISCARDED: std::cell::RefCell<Vec<(String, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    fn record_discard(c: &Credential, shares_grant: bool) {
        DISCARDED.with(|d| d.borrow_mut().push((c.access_token.clone(), shares_grant)));
    }
    fn fixture_credential(token: &str, expires_at: u64) -> Credential {
        Credential {
            access_token: token.into(),
            refresh_token: Some(format!("{token}-refresh")),
            expires_at,
        }
    }
    #[test]
    fn a_new_credential_is_revoked_unless_it_was_kept() {
        let _test_state = crate::test_support::global_state();
        DISCARDED.with(|d| d.borrow_mut().clear());
        // A failure after the exchange (catalog, App-install timeout, cancellation, store).
        drop(Unstored {
            credential: Some(fixture_credential("new", 0)),
            shares_grant: false,
            revoke: record_discard,
        });
        let mut kept = Unstored {
            credential: Some(fixture_credential("stored", 0)),
            shares_grant: true,
            revoke: record_discard,
        };
        kept.kept();
        drop(kept);
        assert_eq!(
            DISCARDED.with(|d| d.borrow().clone()),
            vec![("new".to_string(), false)]
        );
        // The same account shares one authorization with the stored credential: revoking
        // the whole authorization would disconnect it too.
        assert!(matches!(discard_operation(true), Operation::RevokeToken));
        assert!(matches!(
            discard_operation(false),
            Operation::RevokeAuthorization
        ));
        assert!(same_account(Some("Octo-Cat"), "octo-cat"));
        assert!(!same_account(Some("octo-cat"), "other"));
        assert!(!same_account(None, "octo-cat"));
    }
    #[test]
    fn revocation_renews_an_expired_token_and_skips_a_gone_authorization() {
        let _test_state = crate::test_support::global_state();
        let at = 1_000;
        let live = fixture_credential("live", at + 600);
        assert_eq!(
            live_access_token(&live, at, |_| panic!("renewed a live token")).unwrap(),
            Some("live".into())
        );
        // An expired token would get a 404 that looks like success; renew it first.
        let expired = fixture_credential("expired", at);
        let renewed = live_access_token(&expired, at, |refresh| {
            assert_eq!(refresh, "expired-refresh");
            Ok(fixture_credential("renewed", at + 600))
        });
        assert_eq!(renewed.unwrap(), Some("renewed".into()));
        // GitHub rejected the refresh token: the authorization is already gone.
        let gone = live_access_token(&expired, at, |_| {
            Err("GitHub rejected the authorization. Connect GitHub again. Automatic retries stopped.".into())
        });
        assert_eq!(gone.unwrap(), None);
        // A network failure is retried later instead of skipping the revocation.
        assert!(live_access_token(&expired, at, |_| Err("Cannot reach GitHub.".into())).is_err());
        let mut no_refresh = expired.clone();
        no_refresh.refresh_token = None;
        assert_eq!(
            live_access_token(&no_refresh, at, |_| panic!(
                "renewed without a refresh token"
            ))
            .unwrap(),
            None
        );
    }
    #[test]
    fn disconnect_revokes_with_a_renewed_token_when_the_stored_one_expired() {
        let _test_state = crate::test_support::global_state();
        let at = 1_000;
        assert_eq!(
            disconnect_token(None, at, || panic!("renewed without a credential")).unwrap(),
            None
        );
        let live = fixture_credential("live", at + 600);
        assert_eq!(
            disconnect_token(Some(live), at, || panic!("renewed a live token")).unwrap(),
            Some("live".into())
        );
        let expired = fixture_credential("expired", at);
        let renewed = disconnect_token(Some(expired.clone()), at, || {
            Ok(fixture_credential("renewed", at + 600))
        });
        assert_eq!(renewed.unwrap(), Some("renewed".into()));
        // A renewal GitHub rejects means the authorization is gone: finish disconnecting.
        let gone = disconnect_token(Some(expired.clone()), at, || {
            Err("GitHub rejected the authorization. Connect GitHub again.".into())
        });
        assert_eq!(gone.unwrap(), None);
        // A network failure keeps Disconnect pending so it is retried.
        assert!(
            disconnect_token(Some(expired), at, || Err("Cannot reach GitHub.".into())).is_err()
        );
    }
    #[test]
    fn a_connection_holds_the_network_lock_only_for_its_steps() {
        let _test_state = crate::test_support::global_state();
        // Waiting for the browser or App installation holds nothing.
        drop(update_guard().expect("the network lock is held outside a step"));
        let during = network_step(|| Ok(update_guard().is_err())).unwrap();
        assert!(
            during,
            "a network step must block updates and other GitHub operations"
        );
        drop(update_guard().expect("a network step kept the lock after it finished"));
        assert!(network_step(|| Err::<(), _>("exchange failed".into())).is_err());
        drop(update_guard().expect("a failed step kept the lock"));
    }
    #[test]
    fn runtime_work_runs_without_the_state_lock_after_a_checked_revision() {
        let _test_state = crate::test_support::global_state();
        let result = outside_state(
            || {
                assert!(
                    STATE.try_lock().is_err(),
                    "the revision check must hold STATE"
                );
                Ok(Some(7))
            },
            // A guest command or `msb modify` here can take minutes.
            |value| (value, STATE.try_lock().is_ok()),
        )
        .unwrap();
        assert_eq!(result, Some((7, true)), "runtime work held STATE");
        assert_eq!(
            outside_state(|| Ok(None::<()>), |()| panic!("stale work ran")).unwrap(),
            None
        );
        assert!(outside_state(
            || Err::<Option<()>, _>("changed".into()),
            |()| panic!("stale work ran")
        )
        .is_err());
    }
    #[test]
    fn identity_written_during_a_newer_edit_is_not_recorded_as_applied() {
        let _test_state = crate::test_support::global_state();
        let old = json!({"name":"Old","email":"old@example.test","apply":true});
        let new = json!({"name":"New","email":"new@example.test","apply":true});
        let mut d = Document {
            computers: vec![json!({"computer":"dev","identity":old.clone()})],
            ..Default::default()
        };
        assert!(identity_is_current(&d, "dev", &old));
        d.computers[0]["identity"] = new.clone();
        assert!(!identity_is_current(&d, "dev", &old));
        assert!(identity_is_current(&d, "dev", &new));
        assert!(!identity_is_current(&d, "other", &new));
    }
    #[test]
    fn a_panic_under_a_github_lock_does_not_disable_github_or_block_updates() {
        let _test_state = crate::test_support::global_state();
        for lock in [&OPERATION, &STATE] {
            let _ = std::thread::spawn(move || {
                let _guard = lock.lock().unwrap();
                panic!("test panic while holding a GitHub lock");
            })
            .join();
            assert!(lock.is_poisoned());
        }
        drop(update_guard().expect("a poisoned operation lock blocked app updates"));
        drop(try_serialize(&STATE).expect("a poisoned state lock reported busy"));
        drop(serialize(&STATE));
        let queue = IntentQueue::new();
        let ticket = queue.ticket();
        std::thread::scope(|scope| {
            let _ = scope
                .spawn(|| {
                    let _turn = queue.turn.lock().unwrap();
                    panic!("test panic while holding the intent queue");
                })
                .join();
        });
        drop(
            ticket
                .wait()
                .expect("a poisoned intent queue stopped settings changes"),
        );
        let next = queue.ticket();
        drop(next.wait().unwrap());
        OPERATION.clear_poison();
        STATE.clear_poison();
    }
    #[test]
    fn an_abandoned_intent_never_blocks_later_intents() {
        let _test_state = crate::test_support::global_state();
        let queue = IntentQueue::new();
        let first = queue.ticket();
        let second = queue.ticket();
        let third = queue.ticket();
        let fourth = queue.ticket();
        // Given up before its turn (for example an early return before waiting).
        drop(second);
        let first_turn = first.wait().unwrap();
        let (sent, received) = std::sync::mpsc::channel();
        let (waiting, ready) = std::sync::mpsc::sync_channel(0);
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let _turn = third.wait_with(|| waiting.send(()).unwrap()).unwrap();
                sent.send("third applied").unwrap();
            });
            let waiting = ready.recv_timeout(Duration::from_secs(5));
            let premature = received.try_recv();
            // Release before asserting, so a failed barrier cannot strand the scoped worker.
            drop(first_turn);
            waiting.unwrap();
            assert!(matches!(
                premature,
                Err(std::sync::mpsc::TryRecvError::Empty)
            ));
            assert_eq!(
                received.recv_timeout(Duration::from_secs(5)).unwrap(),
                "third applied"
            );
        });
        // Given up exactly at its turn.
        drop(fourth);
        let fifth = queue.ticket();
        let (sent, received) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let _turn = fifth.wait().unwrap();
                sent.send("fifth applied").unwrap();
            });
            assert_eq!(
                received.recv_timeout(Duration::from_secs(5)).unwrap(),
                "fifth applied"
            );
        });
    }
    #[test]
    fn selected_scopes_preserve_access_when_catalog_capitalization_changes() {
        let _test_state = crate::test_support::global_state();
        let d = Document {
            access_enabled: true,
            account: Some("owner".into()),
            repositories: vec![
                json!({"name":"OWNER/Project","ownerId":7,"id":11}),
                json!({"name":"OWNER/Other","ownerId":7,"id":12}),
            ],
            ..Default::default()
        };
        let policy = json!({"repositoryMode":"selected","repositories":[{"repository":"owner/project","allowPushes":true}]});
        let desired = scopes(&d, &policy).unwrap();
        assert_eq!(desired.len(), 1);
        assert_eq!(desired[0].owner, 7);
        assert_eq!(desired[0].ids, vec![11]);
        assert_eq!(desired[0].writes, vec![11]);
        assert_eq!(desired[0].login, "OWNER");
    }

    #[test]
    fn invalid_remaining_selection_never_preserves_removed_repository_access() {
        let _test_state = crate::test_support::global_state();
        let previous = test_grant();
        let document = Document {
            access_enabled: true,
            account: Some("owner".into()),
            repositories: vec![],
            ..Default::default()
        };
        let policy = json!({"repositoryMode":"selected","repositories":[{"repository":"owner/no-longer-authorized","allowPushes":false}]});
        let desired = scopes(&document, &policy);
        assert!(desired.is_err());
        let (retained, error) = narrow_checked(&[previous], desired);
        assert!(retained.is_empty());
        assert!(error.is_some());
    }

    #[test]
    fn unchanged_scopes_do_not_issue_or_refresh_credentials() {
        let _test_state = crate::test_support::global_state();
        let grants = reconcile_grants(
            &[test_scope()],
            &[test_grant()],
            |_, _| panic!("unchanged scope made a network call"),
            || true,
        )
        .unwrap();
        assert_eq!(grants[0].read_token, "read");
        assert_eq!(grants[0].write_token.as_deref(), Some("write"));
    }
    #[test]
    fn adding_read_repository_only_replaces_read_group() {
        let _test_state = crate::test_support::global_state();
        let mut scope = test_scope();
        scope.ids.push(4);
        let mut calls = Vec::new();
        let grants = reconcile_grants(
            &[scope],
            &[test_grant()],
            |s, write| {
                calls.push((s.owner, write));
                Ok(("new-read".into(), now() + 1000))
            },
            || true,
        )
        .unwrap();
        assert_eq!(calls, vec![(1, false)]);
        assert_eq!(grants[0].write_token.as_deref(), Some("write"));
    }
    #[test]
    fn removing_read_repository_detaches_old_read_but_reuses_safe_write() {
        let _test_state = crate::test_support::global_state();
        let mut scope = test_scope();
        scope.ids = vec![1, 2];
        let old = test_grant();
        let retained = narrow(&[old], &[scope.clone()]);
        assert!(profile(&retained)["owners"].as_array().unwrap().is_empty());
        assert_eq!(retained[0].write_token.as_deref(), Some("write"));
        let mut calls = vec![];
        let grants = reconcile_grants(
            &[scope],
            &retained,
            |_, write| {
                calls.push(write);
                Ok(("narrow-read".into(), now() + 1000))
            },
            || true,
        )
        .unwrap();
        assert_eq!(calls, vec![false]);
        assert_eq!(grants[0].write_token.as_deref(), Some("write"));
    }
    #[test]
    fn disabling_writes_detaches_write_without_reminting_read() {
        let _test_state = crate::test_support::global_state();
        let mut scope = test_scope();
        scope.writes.clear();
        let retained = narrow(&[test_grant()], &[scope.clone()]);
        assert_eq!(retained[0].write_token, None);
        let grants = reconcile_grants(
            &[scope],
            &retained,
            |_, _| panic!("write removal minted a token"),
            || true,
        )
        .unwrap();
        assert_eq!(grants[0].read_token, "read");
        assert_eq!(grants[0].write_token, None);
    }
    #[test]
    fn obsolete_read_result_never_causes_write_issuance() {
        let _test_state = crate::test_support::global_state();
        let current = std::cell::Cell::new(true);
        let mut calls = 0;
        let result = reconcile_grants(
            &[test_scope()],
            &[],
            |_, write| {
                assert!(!write);
                calls += 1;
                current.set(false);
                Ok(("read".into(), now() + 1000))
            },
            || current.get(),
        );
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }
    #[test]
    fn all_to_selected_never_reuses_all_repository_tokens() {
        let _test_state = crate::test_support::global_state();
        let mut old = test_grant();
        old.all_repositories = true;
        let retained = narrow(&[old], &[test_scope()]);
        assert!(retained[0].read_token.is_empty());
        assert!(retained[0].write_token.is_none());
        let mut calls = vec![];
        reconcile_grants(
            &[test_scope()],
            &retained,
            |_, write| {
                calls.push(write);
                Ok(("new".into(), now() + 1000))
            },
            || true,
        )
        .unwrap();
        assert_eq!(calls, vec![false, true]);
    }
    #[test]
    fn targeted_retry_preserves_other_pending_grants_and_verified_operations() {
        let _test_state = crate::test_support::global_state();
        let mut d = Document {
            session: session().into(),
            refresh_at: now() + 3600,
            computers: vec![
                saved_policy("retry", false),
                saved_policy("pending", false),
                saved_policy("healthy", false),
            ],
            access_pending: vec!["pending".into()],
            operations: vec![
                json!({"computer":"retry","status":"failed"}),
                json!({"computer":"pending","status":"applying"}),
                json!({"computer":"healthy","status":"succeeded"}),
            ],
            ..Default::default()
        };
        let healthy = d.operations[2].clone();
        prepare_retry(&mut d, Some("retry"));
        assert!(access_update_due(&d, "pending", now(), false, &[]));
        assert!(access_update_due(&d, "retry", now(), false, &[]));
        assert_eq!(
            d.operations.iter().find(|op| op["computer"] == "healthy"),
            Some(&healthy)
        );
        assert!(!access_update_due(&d, "healthy", now(), false, &[]));
    }

    #[test]
    fn unrelated_owner_and_computer_status_stays_unchanged() {
        let _test_state = crate::test_support::global_state();
        let mut d = Document {
            operations: vec![
                json!({"computer":"dev","status":"succeeded"}),
                json!({"computer":"other","status":"succeeded"}),
            ],
            ..Default::default()
        };
        mark_pending_for(&mut d, &["dev".into()]);
        assert_eq!(
            d.operations
                .iter()
                .find(|op| op["computer"] == "other")
                .unwrap()["status"],
            "succeeded"
        );
    }
    #[test]
    fn identity_and_inactive_selection_changes_do_not_change_access_choice() {
        let _test_state = crate::test_support::global_state();
        let mut first = json!({"repositoryMode":"all","allRepositoriesAllowChanges":false,"repositories":[],"identity":{"name":"Old"}});
        let choice = access_choice(&first);
        first["identity"] = json!({"name":"New"});
        first["repositories"] = json!([{"repository":"owner/other","allowPushes":true}]);
        assert_eq!(access_choice(&first), choice);
    }
    #[test]
    fn partially_issued_read_is_reusable_only_for_its_exact_scope() {
        let _test_state = crate::test_support::global_state();
        let token = IssuedToken {
            owner: 1,
            all: false,
            write: false,
            ids: vec![1, 2, 3],
            token: "temporary".into(),
            expires_at: now() + 1000,
        };
        assert!(issued_matches(&token, &test_scope(), false));
        assert!(!issued_matches(&token, &test_scope(), true));
        let mut other = test_scope();
        other.ids.push(4);
        assert!(!issued_matches(&token, &other, false));
    }
    #[test]
    fn background_grant_refresh_never_writes_git_identity() {
        let _test_state = crate::test_support::global_state();
        let (result, _) = finish_application(Ok(()), false, None, || {
            panic!("background renewal must not run guest identity commands")
        });
        assert!(result.is_ok());
        let (result, _) =
            finish_application(Ok(()), false, Some("previous identity error"), || {
                panic!("background renewal must not retry identity")
            });
        assert_eq!(result, Err("previous identity error".into()));
    }
    #[test]
    fn explicit_identity_application_is_independent_of_github_access() {
        let _test_state = crate::test_support::global_state();
        let mut applied = false;
        let (result, identity_result) =
            finish_application(Err("GitHub is unavailable".into()), true, None, || {
                applied = true;
                Ok(())
            });
        assert_eq!(result, Err("GitHub is unavailable".into()));
        assert!(identity_result.is_ok());
        assert!(applied);
    }
    #[test]
    fn optional_github_keeps_verified_setup_when_secure_store_is_unavailable() {
        let _test_state = crate::test_support::global_state();
        let d = Document {
            session: session().into(),
            operations: vec![json!({"computer":"dev","status":"succeeded"})],
            ..Default::default()
        };
        let state = public_snapshot(
            d,
            Err("The system credential store is unavailable.".into()),
            None,
        );
        assert_eq!(state["state"], "disconnected");
        assert_eq!(state["repositoryCatalogStatus"]["status"], "unavailable");
        assert_eq!(state["computerOperations"][0]["status"], "succeeded");
    }
    #[test]
    fn pending_push_retirement_retries_without_grant_renewal_or_enabled_access() {
        for enabled in [true, false] {
            let secret = SessionSecret::new();
            let entry = keyring::Entry::new_with_credential(Box::new(
                keyring::mock::MockCredential::default(),
            ));
            append_ledger_token(&secret, &entry, "dev", "pending_push").unwrap();
            let d = Document {
                session: session().into(),
                access_enabled: enabled,
                refresh_at: 3600,
                catalog_refresh_at: 3600,
                computers: vec![json!({"computer":"dev"})],
                ..Document::default()
            };
            assert!(!worker_due(&d, None, 30, Instant::now()));
            let retirement = Mutex::new(TokenRetirement {
                retry_at: 30,
                ..Default::default()
            });
            sweep_retirement(
                &retirement,
                29,
                || panic!("flush before deadline"),
                || panic!("retirement before deadline"),
                |_| panic!("early revocation"),
            )
            .unwrap();
            assert!(sweep_retirement(
                &retirement,
                30,
                || Ok(()),
                || secret.read(|| panic!("unexpected store read")),
                |name| retire_ledger_tokens(
                    &secret,
                    &entry,
                    name,
                    |_| false,
                    |_| Err("offline".into())
                )
            )
            .is_err());
            assert_eq!(retirement.lock().unwrap().wait(30), Duration::from_secs(30));
            assert!(!worker_due(&d, None, 60, Instant::now()));
            sweep_retirement(
                &retirement,
                59,
                || panic!("flush before retry"),
                || panic!("retirement before retry"),
                |_| panic!("early retry"),
            )
            .unwrap();
            sweep_retirement(
                &retirement,
                60,
                || Ok(()),
                || secret.read(|| panic!("unexpected store read")),
                |name| retire_ledger_tokens(&secret, &entry, name, |_| false, |_| Ok(())),
            )
            .unwrap();
            assert!(secret
                .read(|| panic!("unexpected store read"))
                .unwrap()
                .is_empty());
        }
    }
    #[test]
    fn retirement_worker_flushes_an_unsaved_empty_ledger_after_storage_recovers() {
        let secret = SessionSecret::new();
        let entry =
            keyring::Entry::new_with_credential(Box::new(keyring::mock::MockCredential::default()));
        append_ledger_token(&secret, &entry, "dev", "push").unwrap();
        let mock = entry
            .get_credential()
            .downcast_ref::<keyring::mock::MockCredential>()
            .unwrap();
        mock.set_error(keyring::Error::NoStorageAccess(Box::new(
            std::io::Error::other("denied"),
        )));
        assert!(retire_ledger_tokens(&secret, &entry, "dev", |_| false, |_| Ok(())).is_err());
        assert!(secret
            .read(|| panic!("unexpected store read"))
            .unwrap()
            .is_empty());
        let retirement = Mutex::new(TokenRetirement::default());
        let sweep = |at| {
            sweep_retirement(
                &retirement,
                at,
                || secret.flush(|ledger| save_ledger(&entry, ledger)),
                || secret.read(|| panic!("unexpected store read")),
                |_| panic!("empty ledger has no tokens to retire"),
            )
        };
        mock.set_error(keyring::Error::NoStorageAccess(Box::new(
            std::io::Error::other("denied"),
        )));
        assert!(sweep(0).is_err());
        assert_eq!(retirement.lock().unwrap().wait(0), Duration::from_secs(30));
        sweep(30).unwrap();
        assert!(read_ledger(&entry).unwrap().is_empty());
        begin_host_push_token(&retirement, "next", || {
            append_ledger_token(&secret, &entry, "dev", "next")
        })
        .unwrap();
        assert_eq!(read_ledger(&entry).unwrap()["dev"], ["next"]);
    }
    #[test]
    fn push_token_is_durable_before_use_and_survives_a_crash() {
        let secret = SessionSecret::new();
        let entry =
            keyring::Entry::new_with_credential(Box::new(keyring::mock::MockCredential::default()));
        let retirement = Mutex::new(TokenRetirement::default());
        begin_host_push_token(&retirement, "push", || {
            append_ledger_token(&secret, &entry, "dev", "push")
        })
        .unwrap();
        assert_eq!(read_ledger(&entry).unwrap()["dev"], ["push"]);
        sweep_retirement(
            &retirement,
            1,
            || Ok(()),
            || secret.read(|| panic!("unexpected store read")),
            |name| {
                retire_ledger_tokens(
                    &secret,
                    &entry,
                    name,
                    |token| retirement.lock().unwrap().active_pushes.contains(token),
                    |_| panic!("active push revoked"),
                )
            },
        )
        .unwrap();
        // A new session has no active pushes and reads the durable record left without Drop.
        let restarted = SessionSecret::new();
        let restarted_retirement = Mutex::new(TokenRetirement::default());
        sweep_retirement(
            &restarted_retirement,
            2,
            || Ok(()),
            || restarted.read(|| read_ledger(&entry)),
            |name| retire_ledger_tokens(&restarted, &entry, name, |_| false, |_| Ok(())),
        )
        .unwrap();
        assert!(read_ledger(&entry).unwrap().is_empty());
    }
    #[test]
    fn failed_push_token_storage_prevents_use_and_keeps_retirement_pending() {
        let secret = SessionSecret::new();
        let entry =
            keyring::Entry::new_with_credential(Box::new(keyring::mock::MockCredential::default()));
        secret.read(|| read_ledger(&entry)).unwrap();
        entry
            .get_credential()
            .downcast_ref::<keyring::mock::MockCredential>()
            .unwrap()
            .set_error(keyring::Error::NoStorageAccess(Box::new(
                std::io::Error::other("denied"),
            )));
        let retirement = Mutex::new(TokenRetirement::default());
        let used = std::cell::Cell::new(false);
        let attempt = || -> Result<(), String> {
            begin_host_push_token(&retirement, "push", || {
                append_ledger_token(&secret, &entry, "dev", "push")
            })?;
            used.set(true);
            Ok(())
        };
        assert!(attempt().is_err());
        assert!(!used.get());
        assert_eq!(
            secret.read(|| panic!("unexpected store read")).unwrap()["dev"],
            ["push"]
        );
        retirement.lock().unwrap().finished("push", 5);
        assert!(retirement.lock().unwrap().active_pushes.is_empty());
        assert_eq!(retirement.lock().unwrap().wait(5), Duration::ZERO);
    }
    #[test]
    fn retirement_preserves_ledger_tokens_appended_during_revocation() {
        let secret = SessionSecret::new();
        let entry =
            keyring::Entry::new_with_credential(Box::new(keyring::mock::MockCredential::default()));
        append_ledger_token(&secret, &entry, "dev", "old").unwrap();
        retire_ledger_tokens(
            &secret,
            &entry,
            "dev",
            |_| false,
            |_| {
                append_ledger_token(&secret, &entry, "dev", "new").unwrap();
                append_ledger_token(&secret, &entry, "other", "unrelated").unwrap();
                Ok(())
            },
        )
        .unwrap();
        let ledger = secret.read(|| panic!("unexpected store read")).unwrap();
        assert_eq!(ledger["dev"], ["new"]);
        assert_eq!(ledger["other"], ["unrelated"]);
        assert_eq!(read_ledger(&entry).unwrap(), ledger);
    }
    #[test]
    fn concurrent_ledger_appends_survive_failed_storage() {
        let secret = SessionSecret::new();
        let entry =
            keyring::Entry::new_with_credential(Box::new(keyring::mock::MockCredential::default()));
        secret.read(|| Ok(TokenLedger::new())).unwrap();
        entry
            .get_credential()
            .downcast_ref::<keyring::mock::MockCredential>()
            .unwrap()
            .set_error(keyring::Error::NoStorageAccess(Box::new(
                std::io::Error::other("denied"),
            )));
        let barrier = std::sync::Barrier::new(3);
        std::thread::scope(|scope| {
            for token in ["one", "two"] {
                let secret = &secret;
                let entry = &entry;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    let _ = append_ledger_token(secret, entry, "dev", token);
                });
            }
            barrier.wait();
        });
        let mut tokens = secret.read(|| panic!("unexpected store read")).unwrap()["dev"].clone();
        tokens.sort();
        assert_eq!(tokens, ["one", "two"]);
        secret.flush(|ledger| save_ledger(&entry, ledger)).unwrap();
        assert_eq!(read_ledger(&entry).unwrap()["dev"].len(), 2);
    }
    #[test]
    fn runtime_token_ledger_survives_reload_only_in_secure_store() {
        let _test_state = crate::test_support::global_state();
        let entry =
            keyring::Entry::new_with_credential(Box::new(keyring::mock::MockCredential::default()));
        assert!(read_ledger(&entry).unwrap().is_empty());
        let mut ledger = TokenLedger::new();
        ledger.insert("dev".into(), vec!["test-scoped-token".into()]);
        save_ledger(&entry, &ledger).unwrap();
        assert_eq!(read_ledger(&entry).unwrap(), ledger);
        let mock = entry
            .get_credential()
            .downcast_ref::<keyring::mock::MockCredential>()
            .unwrap();
        mock.set_error(keyring::Error::Invalid(
            "locked".into(),
            "private detail".into(),
        ));
        assert!(read_ledger(&entry).is_err());
        assert_eq!(read_ledger(&entry).unwrap(), ledger);
    }
    #[test]
    fn connection_enables_access_without_selecting_repositories() {
        let _test_state = crate::test_support::global_state();
        let mut document = Document::default();
        record_connection(&mut document, Some("account".into()), vec![]).unwrap();
        assert!(document.access_enabled);
        assert!(document.computers.is_empty());
        assert!(document.access_pending.is_empty());

        document.access_enabled = false;
        document.disconnect_pending = true;
        document.computers = vec![json!({"computer":"dev"})];
        record_connection(&mut document, Some("account".into()), vec![]).unwrap();
        assert!(document.access_enabled);
        assert!(!document.disconnect_pending);
        assert_eq!(document.access_pending, vec!["dev"]);
        assert_eq!(document.operations[0]["status"], "applying");
    }

    #[test]
    fn connected_catalog_refreshes_before_token_expiry_for_every_selection_mode() {
        let _test_state = crate::test_support::global_state();
        let mut d = Document {
            session: session().into(),
            access_enabled: true,
            account: Some("owner".into()),
            computers: vec![json!({"repositoryMode":"all"})],
            catalog_refresh_at: 100,
            refresh_at: 3600,
            ..Default::default()
        };
        assert!(!catalog_refresh_due(&d, 99));
        assert!(catalog_refresh_due(&d, 100));
        d.computers[0]["repositoryMode"] = json!("selected");
        assert!(worker_due(&d, None, 100, Instant::now()));
        d.computers.clear();
        assert!(worker_due(&d, None, 100, Instant::now()));
        d.access_enabled = false;
        assert!(!catalog_refresh_due(&d, 100));
        d.access_enabled = true;
        d.account = None;
        assert!(!catalog_refresh_due(&d, 100));
        d.account = Some("owner".into());
        d.disconnect_pending = true;
        assert!(!catalog_refresh_due(&d, 100));
    }
    #[test]
    fn secure_store_failure_does_not_become_disconnected_or_save_plaintext() {
        let _test_state = crate::test_support::global_state();
        let entry =
            keyring::Entry::new_with_credential(Box::new(keyring::mock::MockCredential::default()));
        let mock = entry
            .get_credential()
            .downcast_ref::<keyring::mock::MockCredential>()
            .unwrap();
        mock.set_error(keyring::Error::Invalid(
            "unavailable".into(),
            "sensitive diagnostic".into(),
        ));
        let error = read_entry(&entry).err().unwrap();
        assert!(!error.contains("sensitive diagnostic"));
        assert!(read_entry(&entry).unwrap().is_none());
        mock.set_error(keyring::Error::Invalid(
            "unavailable".into(),
            "sensitive diagnostic".into(),
        ));
        assert!(store_entry(
            &entry,
            &Credential {
                access_token: "test-only".into(),
                refresh_token: None,
                expires_at: now() + 300
            }
        )
        .is_err());
        assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
    }
    #[test]
    fn configuration_reader_stops_at_the_size_limit() {
        struct CountingReader {
            remaining: usize,
            read: usize,
        }
        impl Read for CountingReader {
            fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
                let length = target.len().min(self.remaining);
                target[..length].fill(b' ');
                self.remaining -= length;
                self.read += length;
                Ok(length)
            }
        }
        let mut reader = CountingReader {
            remaining: MAX_CONFIGURATION_BYTES * 2,
            read: 0,
        };
        assert!(read_configuration(&mut reader).is_err());
        assert_eq!(reader.read, MAX_CONFIGURATION_BYTES + 1);
        let document = Document::default();
        let encoded = serde_json::to_vec(&document).unwrap();
        assert!(read_configuration(encoded.as_slice()).is_ok());
        assert!(read_configuration(&b"not-json"[..]).is_err());
    }
    #[test]
    fn oversized_configuration_save_preserves_the_last_readable_document() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("github.json");
        let mut document = Document {
            account: Some("previous-account".into()),
            ..Default::default()
        };
        save_at(&path, &document).unwrap();
        let previous = fs::read(&path).unwrap();
        document.account = Some("new-account".into());
        let name = format!("{}/{}", "o".repeat(39), "r".repeat(100));
        document.repositories = (1..=99_900)
            .map(|id| json!({"id":id,"ownerId":7,"name":name}))
            .collect();
        assert!(serde_json::to_vec(&document).unwrap().len() > 16 * 1024 * 1024);
        assert!(save_at(&path, &document).is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert_eq!(
            load_at(&path).unwrap().account.as_deref(),
            Some("previous-account")
        );
        document.repositories.clear();
        save_at(&path, &document).unwrap();
        assert_eq!(
            load_at(&path).unwrap().account.as_deref(),
            Some("new-account")
        );
    }
    #[test]
    fn durable_document_contains_no_credentials() {
        let _test_state = crate::test_support::global_state();
        let d = Document::default();
        let encoded = serde_json::to_string(&d).unwrap();
        assert!(!encoded.contains("accessToken"));
        assert!(!encoded.contains("refreshToken"));
    }
    #[test]
    fn rejects_missing_or_expired_lifetimes() {
        let _test_state = crate::test_support::global_state();
        assert!(from_response(json!({"accessToken":"test-only"})).is_err());
        assert!(token_expiry(&json!({"expiresAt":"2020-01-01T00:00:00Z"})).is_err());
        assert!(token_expiry(&json!({"expiresAt":"not-a-date"})).is_err());
    }
    #[test]
    fn refresh_storage_retry_never_reuses_a_consumed_refresh_token() {
        let _test_state = crate::test_support::global_state();
        let old = Credential {
            access_token: "old".into(),
            refresh_token: Some("old-refresh".into()),
            expires_at: 100,
        };
        let mut pending = None;
        let result = refresh_credential_with(
            old.clone(),
            &mut pending,
            100,
            |_| {
                Ok(Credential {
                    access_token: "new".into(),
                    refresh_token: Some("new-refresh".into()),
                    expires_at: 1000,
                })
            },
            |_| Err("secure store locked".into()),
        );
        assert_eq!(result.unwrap().access_token, "new");
        assert!(pending.is_some());
        let restored = refresh_credential_with(
            old,
            &mut pending,
            101,
            |_| panic!("a consumed refresh token was replayed"),
            |credential| {
                assert_eq!(credential.access_token, "new");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(restored.refresh_token.as_deref(), Some("new-refresh"));
        assert!(pending.is_none());
    }
    #[test]
    fn new_login_does_not_restore_another_accounts_pending_refresh() {
        let _test_state = crate::test_support::global_state();
        let mut pending = Some(PendingRefresh {
            previous_access: "old-account".into(),
            renewed: Credential {
                access_token: "old-renewed".into(),
                refresh_token: None,
                expires_at: 1000,
            },
        });
        let new = Credential {
            access_token: "new-account".into(),
            refresh_token: None,
            expires_at: 1000,
        };
        let result = refresh_credential_with(
            new,
            &mut pending,
            100,
            |_| panic!("unneeded refresh"),
            |_| panic!("stale credential persisted"),
        )
        .unwrap();
        assert_eq!(result.access_token, "new-account");
        assert!(pending.is_none());
    }
    #[test]
    fn only_main_window_can_manage_github() {
        let _test_state = crate::test_support::global_state();
        assert!(require_main("main").is_ok());
        for label in ["status", "other", ""] {
            assert!(require_main(label).is_err());
        }
    }
    #[test]
    fn authorization_reopens_same_attempt_and_tracks_installation_page() {
        let _test_state = crate::test_support::global_state();
        let mut pending = PendingAuthorization(Some((
            7,
            "https://github.com/login/oauth/authorize?state=example".into(),
        )));
        assert_eq!(
            pending.url(7).unwrap(),
            "https://github.com/login/oauth/authorize?state=example"
        );
        assert!(pending.url(8).is_err());
        pending.0 = Some((
            7,
            "https://github.com/apps/example/installations/new".into(),
        ));
        assert_eq!(
            pending.url(7).unwrap(),
            "https://github.com/apps/example/installations/new"
        );
        pending.clear(7);
        assert!(pending.url(7).is_err());
    }

    #[test]
    fn finished_old_authorization_cannot_clear_a_new_attempt() {
        let _test_state = crate::test_support::global_state();
        let mut pending = PendingAuthorization(Some((8, "https://github.com/new-attempt".into())));
        pending.clear(7);
        assert_eq!(pending.url(8).unwrap(), "https://github.com/new-attempt");
        assert!(pending.url(7).is_err());
        pending.0 = None;
        assert!(pending.url(8).is_err());
    }

    #[test]
    fn callback_ignores_unrelated_traffic_and_rejects_empty_authenticated_code() {
        let _test_state = crate::test_support::global_state();
        for request in [
            "",
            "garbage",
            "GET //evil/github/callback?state=right&code=x HTTP/1.1",
            "GET /github/callback?state=wrong&error=denied HTTP/1.1",
        ] {
            assert_eq!(callback(request, "right").unwrap(), None);
        }
        for request in [
            "GET /github/callback?state=right&code= HTTP/1.1",
            "GET /github/callback?state=right&code=%20 HTTP/1.1",
            "GET /github/callback?state=right&error=denied HTTP/1.1",
        ] {
            assert!(callback(request, "right").is_err());
        }
    }
    #[test]
    fn callback_retries_interrupted_reads_without_losing_partial_headers() {
        struct Interrupted<'a> {
            bytes: &'a [u8],
            interrupt: bool,
        }
        impl Read for Interrupted<'_> {
            fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
                self.interrupt = !self.interrupt;
                if self.interrupt {
                    return Err(std::io::ErrorKind::Interrupted.into());
                }
                let length = self.bytes.len().min(target.len()).min(3);
                target[..length].copy_from_slice(&self.bytes[..length]);
                self.bytes = &self.bytes[length..];
                Ok(length)
            }
        }
        let request =
            b"GET /github/callback?state=right&code=x HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        let mut reader = Interrupted {
            bytes: request,
            interrupt: false,
        };
        assert_eq!(
            read_callback_request(&mut reader).as_deref(),
            Some(std::str::from_utf8(request).unwrap())
        );
    }

    #[test]
    fn callback_requires_complete_bounded_headers_across_fragments() {
        let _test_state = crate::test_support::global_state();
        struct Fragmented<'a>(&'a [u8]);
        impl Read for Fragmented<'_> {
            fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
                let length = target.len().min(3).min(self.0.len());
                target[..length].copy_from_slice(&self.0[..length]);
                self.0 = &self.0[length..];
                Ok(length)
            }
        }
        let request =
            b"GET /github/callback?state=right&code=x HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        let complete = read_callback_request(&mut Fragmented(request)).unwrap();
        assert_eq!(callback(&complete, "right").unwrap(), Some("x".into()));
        assert!(read_callback_request(&mut Fragmented(&request[..request.len() - 2])).is_none());
        assert!(read_callback_request(&mut Fragmented(&vec![b'a'; 9000])).is_none());
    }
    #[test]
    fn callback_waits_for_a_browser_that_sends_its_request_late() {
        let _test_state = crate::test_support::global_state();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let (reading, ready) = std::sync::mpsc::sync_channel(0);
        let browser = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            ready.recv_timeout(Duration::from_secs(5)).unwrap();
            stream
                .write_all(
                    b"GET /github/callback?state=right&code=x HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                )
                .unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        // Accepted sockets can inherit the listener's non-blocking mode (macOS); an
        // early read must wait for the request instead of losing the single-use code.
        struct StartingRead {
            stream: TcpStream,
            reading: Option<std::sync::mpsc::SyncSender<()>>,
        }
        impl Read for StartingRead {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                if let Some(reading) = self.reading.take() {
                    reading.send(()).unwrap();
                }
                self.stream.read(bytes)
            }
        }
        let mut stream = StartingRead {
            stream: callback_stream(stream).unwrap(),
            reading: Some(reading),
        };
        let request = read_callback_request(&mut stream).expect("the early read lost the callback");
        assert_eq!(callback(&request, "right").unwrap(), Some("x".into()));
        browser.join().unwrap();
    }
    #[test]
    fn callback_rejects_wrong_state_and_duplicate_code() {
        let _test_state = crate::test_support::global_state();
        assert!(callback(
            "GET /github/callback?state=wrong&code=x HTTP/1.1\r\n",
            "right"
        )
        .unwrap()
        .is_none());
        assert!(callback(
            "GET /github/callback?state=right&code=x&code=y HTTP/1.1\r\n",
            "right"
        )
        .is_err());
        assert_eq!(
            callback(
                "GET /github/callback?state=right&code=x HTTP/1.1\r\n",
                "right"
            )
            .unwrap(),
            Some("x".into())
        );
    }
    #[test]
    fn reject_ambiguous_policy() {
        let _test_state = crate::test_support::global_state();
        assert!(validate(&[json!({"computer":"a","repositoryMode":"all","allRepositoriesAllowChanges":false,"repositories":[],"identity":{"name":"","email":"","apply":false}})]).is_ok());
        assert!(validate(&[json!({"computer":"a","repositoryMode":"anything","allRepositoriesAllowChanges":false,"repositories":[]})]).is_err());
    }
    #[test]
    fn rejects_unknown_nested_policy_data_and_invalid_identity() {
        let _test_state = crate::test_support::global_state();
        let good = json!({"computer":"dev","repositoryMode":"selected","allRepositoriesAllowChanges":false,
            "repositories":[{"repository":"owner/repo","allowPushes":false}],
            "identity":{"name":"Name","email":"name@example.invalid","apply":true}});
        assert!(validate(&[good.clone()]).is_ok());
        let mut injected = good.clone();
        injected["identity"]["accessToken"] = json!("must-not-persist");
        assert!(validate(&[injected]).is_err());
        let mut injected = good.clone();
        injected["repositories"][0]["credential"] = json!("must-not-persist");
        assert!(validate(&[injected]).is_err());
        let mut invalid = good;
        invalid["identity"]["name"] = json!("name\ncommand");
        assert!(validate(&[invalid]).is_err());
    }

    #[test]
    fn blocked_credential_read_never_blocks_public_snapshot() {
        let _test_state = crate::test_support::global_state();
        for previous in [None, Some(Ok(Some(now() + 600)))] {
            let observation = std::sync::Arc::new(Mutex::new(previous.clone()));
            let observed = observation.clone();
            let (started, reading) = std::sync::mpsc::channel();
            let (release, released) = std::sync::mpsc::channel();
            let (published, update) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                observe_credential_read(
                    || {
                        started.send(()).unwrap();
                        released.recv().unwrap();
                        Err(
                            "Cannot read GitHub credentials from the system credential store."
                                .into(),
                        )
                    },
                    |value| {
                        *observed.lock().unwrap() = Some(value);
                        published.send(()).unwrap();
                    },
                )
            });
            reading.recv_timeout(Duration::from_secs(5)).unwrap();
            // A pending OS permission prompt cannot hold the snapshot state lock.
            let state = observation.try_lock().unwrap().clone();
            let snapshot = observed_snapshot(Document::default(), state, None);
            assert_eq!(
                snapshot["state"],
                if previous.is_some() {
                    "connected"
                } else {
                    "disconnected"
                }
            );
            if previous.is_none() {
                assert_eq!(snapshot["repositoryCatalogStatus"]["status"], "unavailable");
                assert_eq!(snapshot["repositoryCatalogStatus"]["canRetry"], false);
            }
            assert!(update.try_recv().is_err());
            release.send(()).unwrap();
            update.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(reader.join().unwrap().is_err());
            let snapshot = observed_snapshot(
                Document::default(),
                observation.lock().unwrap().clone(),
                None,
            );
            assert_eq!(snapshot["state"], "disconnected");
            assert_eq!(snapshot["repositoryCatalogStatus"]["status"], "unavailable");
            assert_eq!(snapshot["repositoryCatalogStatus"]["canRetry"], true);
        }
    }
    #[test]
    fn credential_observation_exposes_only_lifetime_and_absence() {
        let _test_state = crate::test_support::global_state();
        let expiry = now() + 600;
        let mut observed = None;
        let result = observe_credential_read(
            || {
                Ok(Some(Credential {
                    access_token: "private-access".into(),
                    refresh_token: Some("private-refresh".into()),
                    expires_at: expiry,
                }))
            },
            |value| observed = Some(value),
        );
        assert!(result.unwrap().is_some());
        // A renewable credential is observed as connected, never with its token.
        assert_eq!(observed, Some(Ok(Some(u64::MAX))));
        observe_credential_read(|| Ok(None), |value| observed = Some(value)).unwrap();
        assert_eq!(observed, Some(Ok(None)));
    }
    #[test]
    fn missing_token_response_is_error() {
        let _test_state = crate::test_support::global_state();
        assert!(from_response(json!({})).is_err());
    }
    #[test]
    fn nullable_github_authentication_matches_wire_contract() {
        let _test_state = crate::test_support::global_state();
        let computers = [Value::Null, json!("oauth"), json!("token")]
            .into_iter()
            .enumerate()
            .map(|(index, method)| {
                json!({
                    "computer": format!("dev-{index}"),
                    "authenticationMethod": method,
                    "repositoryMode": "selected",
                    "allRepositoriesAllowChanges": false,
                    "repositories": [],
                    "identity": {"name": "", "email": "", "apply": false}
                })
            })
            .collect::<Vec<_>>();
        validate(&computers).unwrap();
        let document = Document {
            session: session().into(),
            computers,
            ..Default::default()
        };
        let state = public_snapshot(document, Ok(None), None);
        crate::runtime::contract_tests::assert_fixture("github-authentication.json", vec![state]);
    }

    #[test]
    fn public_github_state_matches_frontend_contract() {
        let _test_state = crate::test_support::global_state();
        let states: Vec<Value> =
            serde_json::from_str(include_str!("../../src/test/contracts/github-state.json"))
                .unwrap();
        for expected in states {
            let d = Document {
                revision: 7,
                access_enabled: true,
                account: Some("test-account".into()),
                session: session().into(),
                computers: expected["computers"].as_array().unwrap().clone(),
                repositories: vec![json!({"id":1,"ownerId":2,"name":"test-owner/repo"})],
                operations: expected["computerOperations"].as_array().unwrap().clone(),
                ..Default::default()
            };
            let stored = if expected["state"] == "connected" {
                Some(now() + 600)
            } else {
                None
            };
            let actual = public_snapshot(d, Ok(stored), None);
            assert_eq!(actual, expected);
            assert!(!actual.to_string().contains("contract-token-not-real"));
        }
    }
}

pub(crate) fn update_guard() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    try_serialize(&OPERATION)
        .ok_or_else(|| "Wait for the GitHub operation to finish before updating.".into())
}
