//! The engine's upkeep of the channels: at start it moves what the earlier in-guest client
//! recorded into the policy's record and rebinds each computer's port; while it runs it checks
//! each running computer's endpoint file after every start (and now and then), forgets the
//! channel of a computer that is gone, and acknowledges jobs a computer started and never
//! followed.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use silo_api::methods::MethodName;
use silo_engine::{
    context::EngineContext,
    guest_client::{self, endpoint, now_secs, TokenStore},
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
/// A failed look is retried after this long.
const RETRY: Duration = Duration::from_secs(5);
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

/// Rebinds every computer's port at engine start: the port it had when it is free, another
/// when something else took it. A computer that moves gets no valid token until its endpoint
/// file is written again, and its network rule changes at its next start.
pub(super) fn rebind(store: &TokenStore, channels: &dyn guest_client::Channels) {
    for record in store.enabled() {
        let id = record.computer_id.as_str();
        let (listener, port) = match guest_client::bind(record.port) {
            Ok(bound) => bound,
            Err(error) => {
                eprintln!("silod: channel of {id}: no port: {error}");
                continue;
            }
        };
        if record.port != Some(port) {
            eprintln!(
                "silod: channel of {id}: port {:?} was taken; it moves to {port} with a new token and new network rules at its next start",
                record.port
            );
            if let Err(error) = store.enable(id, port) {
                eprintln!("silod: channel of {id}: {error}");
                continue;
            }
        }
        channels.serve(id, listener);
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

struct Watcher {
    context: Arc<SilodContext>,
    handle: tokio::runtime::Handle,
    store: TokenStore,
    policy: PolicyStore,
    seen: HashMap<String, Seen>,
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

    fn check(&self, computer_id: &str) -> Result<bool, String> {
        let runtime = silo_engine::runtime::runtime_paths(self.context.runtime_host().as_ref());
        let target =
            silo_engine::guest::resolve(runtime, &self.context.paths().app_data, computer_id)
                .map_err(|error| error.message)?;
        endpoint::ensure(&target, &self.store)
    }

    fn tick(&mut self) {
        let enabled = self.store.enabled();
        self.seen
            .retain(|id, _| enabled.iter().any(|record| &record.computer_id == id));
        if enabled.is_empty() {
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
                // A computer being created is not listed yet. One that was never listed and never
                // got a token, missing for a while, is a creation that failed and is forgotten; a
                // computer that was deleted is forgotten by its delete.
                let since = *seen.missing_since.get_or_insert(now);
                if !seen.listed && record.hash.is_empty() && now.duration_since(since) >= GONE_AFTER
                {
                    eprintln!("silod: channel of {id}: the computer is gone; its tools are off");
                    let _ = guest_client::turn_off(&[id.as_str()]);
                    self.seen.remove(&id);
                } else {
                    self.seen.insert(id, seen);
                }
                continue;
            };
            seen.missing_since = None;
            seen.listed = true;
            let generation = silo_engine::generation::generation(&id);
            if due(&seen, running, generation, now) {
                match self.check(&id) {
                    Ok(written) => {
                        if written {
                            eprintln!("silod: channel of {id}: wrote the endpoint file");
                        }
                        seen.retry_at = None;
                    }
                    Err(error) => {
                        eprintln!("silod: channel of {id}: endpoint file not checked: {error}");
                        seen.retry_at = Some(now + RETRY);
                    }
                }
                seen.checked = Some(now);
                seen.generation = generation;
            }
            seen.running = running;
            self.seen.insert(id, seen);
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

/// Installs the engine's channels, rebinds the ports of computers whose tools are on, and
/// starts the watcher.
pub fn start(context: Arc<SilodContext>) {
    let Some(handle) = methods::remote::runtime_handle() else {
        return;
    };
    let app_data = context.paths().app_data.clone();
    let channels = Arc::new(Channels::for_engine(context.clone(), handle.clone()));
    guest_client::install_channels(channels.clone());
    let store = TokenStore::new(&app_data);
    let policy = PolicyStore::new(&app_data);
    adopt_earlier_records(&store, &policy);
    rebind(&store, channels.as_ref());
    let mut watcher = Watcher {
        context,
        handle,
        store,
        policy,
        seen: HashMap::new(),
    };
    let (wake, woken) = std::sync::mpsc::sync_channel::<()>(1);
    watcher
        .handle
        .spawn(follow_changes(watcher.context.clone(), wake));
    let spawned = std::thread::Builder::new()
        .name("silod-channels".into())
        .spawn(move || {
            let mut housekept = Instant::now();
            loop {
                if woken.recv_timeout(TICK).is_ok() {
                    std::thread::sleep(SETTLE);
                    while woken.try_recv().is_ok() {}
                }
                watcher.tick();
                if housekept.elapsed() > HOUSEKEEPING {
                    watcher.reap();
                    housekept = Instant::now();
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
