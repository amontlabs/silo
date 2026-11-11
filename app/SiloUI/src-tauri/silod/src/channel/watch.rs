//! The engine's upkeep of the channels: at start it moves what the earlier in-guest client
//! recorded into the policy's record, forgets every token hash (a token learned while the engine
//! was not running is dead) and rebinds each computer's port and each held port; while it runs
//! it checks each running computer's endpoint file after every start (and now and then), serves
//! a computer that waited for a restart once its rules match, lets a held port go once its
//! computer runs with rules that no longer name it, forgets the channel of a computer that is
//! gone, and acknowledges jobs a computer started and never followed.
//!
//! The checks run in a computer (a command as root, or a look at its network), so each runs on
//! a thread of its own, at most [`MAX_CHECKS`] at once and one per computer: a computer that
//! stalls holds up only itself, and one that keeps failing is tried less and less often.

use std::{
    collections::{HashMap, HashSet},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use silo_api::methods::{network::NetworkPolicyTiming, MethodName};
use silo_engine::{
    context::EngineContext,
    guest_client::{self, endpoint, now_secs, tokens, HeldPort, TokenStore},
    policy::store::PolicyStore,
};

use super::Channels;
use crate::{
    context::SilodContext,
    dispatcher::{DispatchError, RequestContext},
    methods::{self, HandlerContext},
};

/// The watcher looks after every change the engine announces about computers or checkpoints,
/// and at least this often.
const TICK: Duration = Duration::from_secs(15);
/// Changes announced together are looked at once, this long after the first.
const SETTLE: Duration = Duration::from_millis(500);
/// A computer that changed is looked at again after this long at the soonest.
const AFTER_CHANGE: Duration = Duration::from_secs(30);
/// A running computer is looked at again after this long whatever happened.
const EVERY: Duration = Duration::from_secs(600);
/// A failed look is retried after this long, doubled for each failure in a row up to
/// [`MAX_RETRY`].
const RETRY: Duration = Duration::from_secs(5);
const MAX_RETRY: Duration = Duration::from_secs(300);
/// The most checks running at once.
pub(super) const MAX_CHECKS: usize = 4;
/// A computer missing from the engine's list for this long loses its channel.
const GONE_AFTER: Duration = Duration::from_secs(600);
const HOUSEKEEPING: Duration = Duration::from_secs(60);
/// A job nobody acknowledged is acknowledged for its computer after this long.
const JOB_AGE: u64 = 1800;

/// What the watcher last saw of one computer.
#[derive(Clone, Debug, Default)]
pub(super) struct Seen {
    running: bool,
    generation: u64,
    checked: Option<Instant>,
    retry_at: Option<Instant>,
    missing_since: Option<Instant>,
    /// Whether the watcher has seen the computer in the engine's list.
    listed: bool,
    /// Failed looks in a row.
    failures: u32,
}

/// When a look that failed `failures` times in a row is tried again.
pub(super) fn retry_after(failures: u32) -> Duration {
    RETRY
        .saturating_mul(1 << failures.saturating_sub(1).min(8))
        .min(MAX_RETRY)
}

#[cfg(test)]
impl Seen {
    pub(super) fn for_test(
        running: bool,
        generation: u64,
        checked: Instant,
        retry_at: Option<Instant>,
    ) -> Self {
        Self {
            running,
            generation,
            checked: Some(checked),
            retry_at,
            missing_since: None,
            listed: true,
            failures: 0,
        }
    }
}

/// Whether a running computer's endpoint file is due a look: the first time the watcher sees
/// it running (a start, or the engine's own start), after it changed (at most every
/// [`AFTER_CHANGE`]), every [`EVERY`], and when a failed look is due again.
pub(super) fn due(seen: &Seen, running: bool, generation: u64, now: Instant) -> bool {
    if !running {
        return false;
    }
    if let Some(retry) = seen.retry_at {
        return now >= retry;
    }
    match seen.checked {
        None => true,
        Some(at) => {
            !seen.running
                || (generation != seen.generation && now.duration_since(at) >= AFTER_CHANGE)
                || now.duration_since(at) >= EVERY
        }
    }
}

/// Prepares the channels at engine start: adopts what the earlier client recorded, forgets
/// every token hash, and rebinds the ports.
pub(super) fn prepare(
    store: &TokenStore,
    policy: &PolicyStore,
    channels: &dyn guest_client::Channels,
) {
    adopt_earlier_records(store, policy);
    if let Err(error) = store.clear_hashes() {
        eprintln!("silod: the tokens of Silo's tools could not be reset: {error}");
    }
    rebind(store, channels);
}

/// Rebinds every computer's port at engine start: the port it had when it is free, another
/// when something else took it. A computer that moves gets no valid token, holds its new port
/// without serving it and waits for a restart: its network still names the old port, which
/// another program has. Every held port is bound again when it is free.
pub(super) fn rebind(store: &TokenStore, channels: &dyn guest_client::Channels) {
    let locked = store.locked();
    let (mut records, mut held) = match (locked.records(), locked.held()) {
        (Ok(records), Ok(held)) => (records, held),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("silod: the channels were not rebound: {error}");
            return;
        }
    };
    let mut moved = false;
    let ids: Vec<(String, u16)> = records
        .iter()
        .filter_map(|record| Some((record.computer_id.clone(), record.port?)))
        .collect();
    for (id, wanted) in ids {
        let (listener, port) = match guest_client::bind(Some(wanted)) {
            Ok(bound) => bound,
            Err(error) => {
                eprintln!("silod: channel of {id}: no port: {error}");
                continue;
            }
        };
        if port != wanted {
            eprintln!(
                "silod: channel of {id}: port {wanted} was taken; it moves to {port} once the computer restarts"
            );
            if let Err(error) = tokens::set_port(&mut records, &mut held, &id, port) {
                eprintln!("silod: channel of {id}: {error}");
                continue;
            }
            moved = true;
        }
        let Some(record) = records.iter_mut().find(|record| record.computer_id == id) else {
            continue;
        };
        record.restart_needed |= port != wanted;
        let bound = if record.restart_needed {
            channels.hold(&id, listener)
        } else {
            channels.serve(&id, listener)
        };
        if let Err(error) = bound {
            eprintln!("silod: channel of {id}: {error}");
        }
    }
    if moved {
        if let Err(error) = locked.save_held(&held).and_then(|()| locked.save(&records)) {
            eprintln!("silod: the moved channels were not saved: {error}");
        }
    }
    hold_free(&held, channels);
}

/// Binds each held port that is free and not bound yet.
fn hold_free(held: &[HeldPort], channels: &dyn guest_client::Channels) {
    for entry in held {
        if channels.bound(entry.port).is_some() {
            continue;
        }
        if let Ok(listener) = guest_client::bind_exact(entry.port) {
            if let Err(error) = channels.hold(&entry.computer_id, listener) {
                eprintln!("silod: channel of {}: {error}", entry.computer_id);
            }
        }
    }
}

/// Moves the forks and jobs the earlier in-guest client recorded with its tokens into the
/// policy's record, then drops the records that have no port: those computers' tools stay off
/// until the owner turns them on.
pub(super) fn adopt_earlier_records(store: &TokenStore, policy: &PolicyStore) {
    let Ok(records) = store.all() else {
        return;
    };
    let mut earlier = Vec::new();
    for record in records {
        let jobs: Vec<(String, u64)> = record
            .jobs
            .iter()
            .map(|job| (job.id.clone(), job.at))
            .collect();
        let adopted = policy
            .import_legacy(&record.computer_id, &record.fork_ids, &jobs)
            .and_then(|()| {
                store.update(&record.computer_id, |record| {
                    record.fork_ids.clear();
                    record.pending.clear();
                    record.jobs.clear();
                    Ok(())
                })
            });
        match adopted {
            Ok(_) if record.port.is_none() => earlier.push(record.computer_id),
            Ok(_) => {}
            Err(error) => eprintln!("silod: channel of {}: {error}", record.computer_id),
        }
    }
    if !earlier.is_empty() {
        let ids: Vec<&str> = earlier.iter().map(String::as_str).collect();
        if let Err(error) = store.revoke(&ids) {
            eprintln!("silod: could not remove the earlier in-guest client's tokens: {error}");
        }
    }
}

/// A check that runs in a computer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Check {
    /// Its endpoint file.
    Endpoint(String),
    /// Whether its network now names its new port, so its channel can be served.
    Restart(String),
    /// Whether its network no longer names a held port, so the port can go.
    Held(HeldPort),
}

impl Check {
    fn computer(&self) -> &str {
        match self {
            Self::Endpoint(id) | Self::Restart(id) => id,
            Self::Held(entry) => &entry.computer_id,
        }
    }
}

/// What a check found: `Ok(true)` when it changed something.
type Outcome = Result<bool, String>;

/// The checks running, at most one per computer and [`MAX_CHECKS`] in all.
#[derive(Default)]
pub(super) struct Running {
    computers: Mutex<HashSet<String>>,
}

impl Running {
    /// Takes a place for `computer`, unless it has a check running or every place is taken.
    pub(super) fn begin(&self, computer: &str) -> bool {
        let mut computers = self
            .computers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        computers.len() < MAX_CHECKS && computers.insert(computer.to_owned())
    }

    pub(super) fn end(&self, computer: &str) {
        self.computers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(computer);
    }
}

/// What a check needs, cloned onto its thread.
#[derive(Clone)]
struct Checker {
    context: Arc<SilodContext>,
    channels: Arc<super::Channels>,
}

impl Checker {
    fn store(&self) -> TokenStore {
        TokenStore::new(&self.context.paths().app_data)
    }

    fn runtime(&self) -> Result<silo_engine::runtime::RuntimePaths, String> {
        silo_engine::runtime::runtime_paths(self.context.runtime_host().as_ref())
    }

    fn endpoint(&self, computer_id: &str) -> Outcome {
        let target = silo_engine::guest::resolve(
            self.runtime(),
            &self.context.paths().app_data,
            computer_id,
        )
        .map_err(|error| error.message)?;
        endpoint::ensure(&target, &self.store())
    }

    /// Whether the computer runs with the rules its plan asks for now.
    fn rules_match(&self, computer_id: &str) -> Result<bool, String> {
        crate::methods::guest_client::network_now(&self.runtime(), computer_id)
            .map(|timing| timing == NetworkPolicyTiming::Applied)
    }

    fn restart(&self, computer_id: &str) -> Outcome {
        if !self.rules_match(computer_id)? {
            return Ok(false);
        }
        serve_after_restart(&self.store(), self.channels.as_ref(), computer_id)?;
        eprintln!("silod: channel of {computer_id}: the computer restarted; its tools are served");
        self.endpoint(computer_id).map(|_| true)
    }

    fn held(&self, entry: &HeldPort) -> Outcome {
        if !self.rules_match(&entry.computer_id)? {
            return Ok(false);
        }
        release_held(&self.store(), self.channels.as_ref(), entry)?;
        Ok(true)
    }

    fn run(&self, check: &Check) -> Outcome {
        match check {
            Check::Endpoint(id) => self.endpoint(id),
            Check::Restart(id) => self.restart(id),
            Check::Held(entry) => self.held(entry),
        }
    }
}

/// Serves a computer that waited for a restart on its port, once its network names it.
pub(super) fn serve_after_restart(
    store: &TokenStore,
    channels: &dyn guest_client::Channels,
    computer_id: &str,
) -> Result<(), String> {
    let locked = store.locked();
    let mut records = locked.records()?;
    let Some(record) = records
        .iter_mut()
        .find(|record| record.computer_id == computer_id && record.restart_needed)
    else {
        return Ok(());
    };
    let port = record
        .port
        .ok_or("Silo's tools are not on in that computer.")?;
    if !channels.serve_held(computer_id, port) {
        let listener = guest_client::bind_exact(port)
            .map_err(|error| format!("its port {port} is not free: {error}"))?;
        channels.serve(computer_id, listener)?;
    }
    record.restart_needed = false;
    locked.save(&records)
}

/// Lets a held port go, unless it is its computer's channel again.
pub(super) fn release_held(
    store: &TokenStore,
    channels: &dyn guest_client::Channels,
    entry: &HeldPort,
) -> Result<(), String> {
    let locked = store.locked();
    let mut held = locked.held()?;
    if !held.contains(entry) {
        return Ok(());
    }
    held.retain(|other| other != entry);
    locked.save_held(&held)?;
    let live = locked
        .records()?
        .iter()
        .any(|record| record.computer_id == entry.computer_id && record.port == Some(entry.port));
    if !live && channels.bound(entry.port).as_deref() == Some(entry.computer_id.as_str()) {
        channels.release(entry.port);
    }
    Ok(())
}

struct Watcher {
    context: Arc<SilodContext>,
    handle: tokio::runtime::Handle,
    store: TokenStore,
    policy: PolicyStore,
    checker: Checker,
    seen: HashMap<String, Seen>,
    held_seen: HashMap<HeldPort, Seen>,
    running: Arc<Running>,
    results: mpsc::Receiver<(Check, Outcome)>,
    report: mpsc::Sender<(Check, Outcome)>,
    wake: mpsc::SyncSender<()>,
}

impl Watcher {
    fn owner_call(&self, method: MethodName, params: Value) -> Result<Value, DispatchError> {
        let context = HandlerContext::for_owner_bridge(
            self.context.clone(),
            RequestContext::owner("silo-tools"),
        );
        self.handle.block_on(async move {
            methods::dispatch(&context, method, &params)
                .await
                .map(|output| output.value)
        })
    }

    /// Each computer's id and whether it is running, as the engine lists them.
    fn states(&self) -> Option<HashMap<String, bool>> {
        let tree = self.owner_call(MethodName::ComputersTree, json!({})).ok()?;
        let nodes = tree.get("nodes")?.as_array()?;
        Some(
            nodes
                .iter()
                .filter_map(|node| {
                    Some((
                        node.get("id")?.as_str()?.to_owned(),
                        node.get("state").and_then(Value::as_str) == Some("running"),
                    ))
                })
                .collect(),
        )
    }

    /// Starts `check` on a thread of its own when its computer has none running and a place
    /// is free. Returns whether it started.
    fn launch(&self, check: Check) -> bool {
        if !self.running.begin(check.computer()) {
            return false;
        }
        let (checker, running, report, wake) = (
            self.checker.clone(),
            self.running.clone(),
            self.report.clone(),
            self.wake.clone(),
        );
        let computer = check.computer().to_owned();
        let spawned = std::thread::Builder::new()
            .name("silod-channel-check".into())
            .spawn(move || {
                let outcome = checker.run(&check);
                running.end(check.computer());
                let _ = report.send((check, outcome));
                let _ = wake.try_send(());
            });
        if spawned.is_err() {
            self.running.end(&computer);
            return false;
        }
        true
    }

    /// Takes in what finished checks found.
    fn collect(&mut self) {
        let now = Instant::now();
        while let Ok((check, outcome)) = self.results.try_recv() {
            let seen = match &check {
                Check::Endpoint(id) | Check::Restart(id) => self.seen.get_mut(id),
                Check::Held(entry) => self.held_seen.get_mut(entry),
            };
            let Some(seen) = seen else {
                continue;
            };
            match outcome {
                // A computer still running with its old rules is looked at again soon after
                // a start, then less and less often.
                Ok(false) if !matches!(check, Check::Endpoint(_)) => {
                    seen.failures = seen.failures.saturating_add(1);
                    seen.retry_at = Some(now + retry_after(seen.failures));
                }
                Ok(changed) => {
                    if changed {
                        if let Check::Endpoint(id) = &check {
                            eprintln!("silod: channel of {id}: wrote the endpoint file");
                        }
                    }
                    seen.failures = 0;
                    seen.retry_at = None;
                }
                Err(error) => {
                    seen.failures = seen.failures.saturating_add(1);
                    let wait = retry_after(seen.failures);
                    eprintln!(
                        "silod: channel of {}: not checked ({error}); tried again in {wait:?}",
                        check.computer()
                    );
                    seen.retry_at = Some(now + wait);
                }
            }
        }
    }

    fn tick(&mut self) {
        self.collect();
        let enabled = self.store.enabled();
        let held = self.store.locked().held().unwrap_or_default();
        self.seen
            .retain(|id, _| enabled.iter().any(|record| &record.computer_id == id));
        self.held_seen.retain(|entry, _| held.contains(entry));
        hold_free(&held, self.checker.channels.as_ref());
        if enabled.is_empty() && held.is_empty() {
            return;
        }
        let Some(states) = self.states() else {
            return;
        };
        let now = Instant::now();
        for record in enabled {
            let id = record.computer_id;
            let mut seen = self.seen.get(&id).cloned().unwrap_or_default();
            let Some(&running) = states.get(&id) else {
                // A computer being created is not listed yet. One that was never listed, missing
                // for a while, is a creation that failed and is forgotten; a computer that was
                // deleted is forgotten by its delete.
                let since = *seen.missing_since.get_or_insert(now);
                if !seen.listed && now.duration_since(since) >= GONE_AFTER {
                    eprintln!("silod: channel of {id}: the computer is gone; its tools are off");
                    let _ = guest_client::forget_in(&self.store, &id);
                    self.seen.remove(&id);
                } else {
                    self.seen.insert(id, seen);
                }
                continue;
            };
            seen.missing_since = None;
            seen.listed = true;
            let generation = silo_engine::generation::generation(&id);
            // A running computer without a token (the engine started, or an export ended it)
            // gets one as soon as it can.
            let tokenless = running
                && record.hash.is_empty()
                && !record.restart_needed
                && seen.retry_at.is_none_or(|retry| now >= retry);
            if due(&seen, running, generation, now) || tokenless {
                let check = if record.restart_needed {
                    Check::Restart(id.clone())
                } else {
                    Check::Endpoint(id.clone())
                };
                if self.launch(check) {
                    seen.checked = Some(now);
                    seen.generation = generation;
                    // Cleared or set again when the result comes in.
                    seen.retry_at = Some(now + MAX_RETRY);
                }
            }
            seen.running = running;
            self.seen.insert(id, seen);
        }
        for entry in held {
            let mut seen = self.held_seen.get(&entry).cloned().unwrap_or_default();
            let Some(&running) = states.get(&entry.computer_id) else {
                // A computer deleted while the engine was not running.
                let since = *seen.missing_since.get_or_insert(now);
                if now.duration_since(since) >= GONE_AFTER {
                    let _ = release_held(&self.store, self.checker.channels.as_ref(), &entry);
                    self.held_seen.remove(&entry);
                } else {
                    self.held_seen.insert(entry, seen);
                }
                continue;
            };
            seen.missing_since = None;
            let generation = silo_engine::generation::generation(&entry.computer_id);
            if due(&seen, running, generation, now) && self.launch(Check::Held(entry.clone())) {
                seen.checked = Some(now);
                seen.generation = generation;
                seen.retry_at = Some(now + MAX_RETRY);
            }
            seen.running = running;
            self.held_seen.insert(entry, seen);
        }
    }

    /// Acknowledges, for a computer that never did, the jobs it started long ago and that are
    /// over, so no job of a computer stays in the engine's queue.
    fn reap(&self) {
        let Ok(document) = self.policy.snapshot() else {
            return;
        };
        let now = now_secs();
        for record in self.store.enabled() {
            let computer = record.computer_id.as_str();
            for job in document
                .jobs_of(computer)
                .into_iter()
                .filter(|job| now.saturating_sub(job.at) > JOB_AGE)
            {
                let state =
                    self.owner_call(MethodName::OperationsJobGet, json!({ "jobId": job.id }));
                if matches!(&state, Ok(reply) if reply["state"] != "finished") {
                    continue;
                }
                let _ = self.owner_call(
                    MethodName::OperationsJobAcknowledge,
                    json!({ "jobId": job.id }),
                );
                if let Err(error) = self.policy.drop_job(computer, &job.id) {
                    eprintln!("silod: channel of {computer}: {error}");
                }
            }
        }
    }
}

/// Installs the engine's channels, forgets the tokens, rebinds the ports of computers whose
/// tools are on and the held ones, and starts the watcher.
pub fn start(context: Arc<SilodContext>) {
    let Some(handle) = methods::remote::runtime_handle() else {
        return;
    };
    let app_data = context.paths().app_data.clone();
    let channels = Arc::new(Channels::for_engine(context.clone(), handle.clone()));
    guest_client::install_channels(channels.clone());
    let store = TokenStore::new(&app_data);
    let policy = PolicyStore::new(&app_data);
    prepare(&store, &policy, channels.as_ref());
    let (wake, woken) = mpsc::sync_channel::<()>(1);
    let (report, results) = mpsc::channel();
    let mut watcher = Watcher {
        checker: Checker {
            context: context.clone(),
            channels,
        },
        context,
        handle,
        store,
        policy,
        seen: HashMap::new(),
        held_seen: HashMap::new(),
        running: Arc::new(Running::default()),
        results,
        report,
        wake: wake.clone(),
    };
    watcher
        .handle
        .spawn(follow_changes(watcher.context.clone(), wake));
    let spawned = std::thread::Builder::new()
        .name("silod-channels".into())
        .spawn(move || {
            let mut housekept = Instant::now();
            loop {
                watcher.tick();
                if housekept.elapsed() > HOUSEKEEPING {
                    watcher.reap();
                    housekept = Instant::now();
                }
                if woken.recv_timeout(TICK).is_ok() {
                    std::thread::sleep(SETTLE);
                    while woken.try_recv().is_ok() {}
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("silod: could not start the channel watcher: {error}");
    }
}

/// Wakes the watcher on every event about computers or checkpoints (a start, a stop, a restore).
async fn follow_changes(context: Arc<SilodContext>, wake: std::sync::mpsc::SyncSender<()>) {
    let client = silo_engine::context::ClientId::new("silo-tools-watch".to_owned());
    let mut events = context.connect_event_stream(client.clone());
    let subscribed = context
        .subscribe_events(&client, vec!["computers.*".into(), "checkpoints.*".into()])
        .and_then(|subscription| context.activate_events(&client, &subscription));
    if let Err(error) = subscribed {
        eprintln!(
            "silod: the channel watcher follows no events ({error}); it looks every {TICK:?}"
        );
        return;
    }
    while events.recv().await.is_some() {
        // A full queue already holds a wake.
        let _ = wake.try_send(());
    }
}
