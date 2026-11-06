//! The loops: one thread watches which computers have a token, and each of those computers has
//! a worker of its own that serves its bridge folder.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime},
};

use silo_engine::{
    context::EngineContext,
    guest_client::{self, Spool, TokenStore},
};

use super::{
    frames::{handle_frame, reap, Frames, Reserved},
    EngineRunner, Snapshot,
};
use crate::{context::SilodContext, methods};

const POLL: Duration = Duration::from_millis(300);
/// How often a computer's folder name is looked up while it is not known.
const REFRESH: Duration = Duration::from_secs(5);
/// A look at the computers is reused for this long by the calls of one burst.
const FRESH: Duration = Duration::from_millis(300);
const HOUSEKEEPING: Duration = Duration::from_secs(60);

struct Shared {
    context: Arc<SilodContext>,
    handle: tokio::runtime::Handle,
    store: TokenStore,
    runner: EngineRunner,
    app_data: PathBuf,
    reserved: Reserved,
    looked: Mutex<(Snapshot, Option<Instant>)>,
}

impl Shared {
    fn requests(&self, id: &str) -> Option<String> {
        silo_engine::fold_requests::Store::new(&self.app_data)
            .get(id)
            .ok()
            .flatten()
            .map(|request| request.fork_id)
    }

    fn cached(&self) -> Snapshot {
        self.looked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0
            .clone()
    }

    /// The computers as the engine lists them, looked at again when the last look is older than
    /// `max_age`. A token whose computer is gone is removed, so a delete that succeeded ends it.
    fn fresh(&self, max_age: Duration) -> Snapshot {
        let mut looked = self
            .looked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if looked.1.is_none_or(|at| at.elapsed() > max_age) {
            if let Ok(fresh) = self.handle.block_on(self.runner.snapshot()) {
                if !fresh.names.is_empty() {
                    let gone: Vec<String> = self
                        .store
                        .enabled()
                        .into_iter()
                        .map(|record| record.computer_id)
                        .filter(|id| !fresh.names.contains_key(id))
                        .collect();
                    if !gone.is_empty() {
                        let ids: Vec<&str> = gone.iter().map(String::as_str).collect();
                        if let Err(error) = self.store.revoke(&ids) {
                            eprintln!(
                                "silod: could not remove tokens of deleted computers: {error}"
                            );
                        }
                    }
                }
                looked.0 = fresh;
            }
            looked.1 = Some(Instant::now());
        }
        looked.0.clone()
    }
}

/// Starts the loop that serves the bridge folders of computers that have a token.
pub fn start(context: Arc<SilodContext>) {
    let Some(handle) = methods::remote::runtime_handle() else {
        return;
    };
    let app_data = context.paths().app_data.clone();
    let shared = Arc::new(Shared {
        store: TokenStore::new(&app_data),
        runner: EngineRunner::new(context.clone()),
        context,
        handle,
        app_data,
        reserved: Reserved::default(),
        looked: Mutex::new((Snapshot::default(), None)),
    });
    let spawned = std::thread::Builder::new()
        .name("silod-guest-bridge".into())
        .spawn(move || supervise(&shared));
    if let Err(error) = spawned {
        eprintln!("silod: could not start the guest client bridge: {error}");
    }
}

fn supervise(shared: &Arc<Shared>) {
    let mut workers: HashMap<String, JoinHandle<()>> = HashMap::new();
    loop {
        std::thread::sleep(Duration::from_secs(1));
        workers.retain(|_, worker| !worker.is_finished());
        for record in shared.store.enabled() {
            if workers.contains_key(&record.computer_id) {
                continue;
            }
            let (computer, shared) = (record.computer_id.clone(), shared.clone());
            let name = format!("silod-guest-{}", &computer[..computer.len().min(8)]);
            match std::thread::Builder::new()
                .name(name)
                .spawn(move || work(&shared, &computer))
            {
                Ok(worker) => {
                    workers.insert(record.computer_id, worker);
                }
                Err(error) => eprintln!("silod: could not start a guest client worker: {error}"),
            }
        }
    }
}

/// Serves one computer until its token is gone.
fn work(shared: &Shared, computer: &str) {
    let mut housekept = Instant::now();
    let mut looked_up: Option<Instant> = None;
    loop {
        std::thread::sleep(POLL);
        if shared.store.status(computer).ok().flatten().is_none() {
            return;
        }
        let Ok(home) = silo_engine::runtime::runtime_paths(shared.context.runtime_host().as_ref())
            .map(|paths| paths.home)
        else {
            continue;
        };
        let mut snapshot = shared.cached();
        if !snapshot.names.contains_key(computer) {
            // The folder is named after the computer, which this look finds.
            if looked_up.is_none_or(|at| at.elapsed() > REFRESH) {
                snapshot = shared.fresh(Duration::ZERO);
                looked_up = Some(Instant::now());
            }
        }
        let Some(name) = snapshot
            .names
            .get(computer)
            .filter(|_| snapshot.linux.contains(computer))
        else {
            continue;
        };
        let spool = Spool::new(guest_client::spool_dir(&home, name));
        spool.seal();
        if guest_client::bridge_held(name) {
            // A full capture of the running computer is being made.
            continue;
        }
        if spool.has_work() {
            let snapshot = shared.fresh(FRESH);
            let requests = |id: &str| shared.requests(id);
            let env = Frames {
                runner: &shared.runner,
                store: &shared.store,
                snapshot: &snapshot,
                requests: &requests,
                reserved: &shared.reserved,
            };
            for taken in spool.take() {
                let reply = match taken.frame {
                    Ok(frame) => shared.handle.block_on(handle_frame(&env, computer, frame)),
                    Err(_) => {
                        eprintln!(
                            "silod: guest client of {computer}: dropped an unreadable request"
                        );
                        None
                    }
                };
                if let Some(reply) = reply {
                    if let Err(error) = spool.respond(&reply, taken.owner) {
                        eprintln!("silod: guest client of {computer}: {error}");
                    }
                }
            }
        }
        if housekept.elapsed() > HOUSEKEEPING {
            spool.sweep(SystemTime::now());
            shared
                .handle
                .block_on(reap(&shared.runner, &shared.store, computer));
            housekept = Instant::now();
        }
    }
}
