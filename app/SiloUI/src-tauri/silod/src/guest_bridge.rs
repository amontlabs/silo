//! Serves the in-guest client: reads the request files computers write into their bridge
//! folders, checks each against the computer's token and scope, runs the allowed ones as the
//! engine's own methods and writes the answer back (`docs/SiloUI-IN-GUEST-CLIENT.md`).

use std::{
    collections::{HashMap, HashSet, VecDeque},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use serde_json::{json, Value};
use silo_api::{
    methods::{
        guest_client::{GuestErrorBody, GuestRequestFrame, GuestResponseFrame},
        MethodName,
    },
    transport::PROTOCOL_VERSION,
};
use silo_engine::{
    context::EngineContext,
    guest_client::{self, Filter, Spool, TokenStore, Topology},
};

use crate::{
    context::SilodContext,
    dispatcher::{DispatchError, RequestContext},
    methods::{self, HandlerContext},
};

const POLL: Duration = Duration::from_millis(300);
const REFRESH: Duration = Duration::from_secs(5);
const KEPT_JOBS: usize = 256;

/// Runs one method as the engine, for a request that passed its scope check.
pub trait Runner: Send + Sync {
    fn run<'a>(
        &'a self,
        method: &'a str,
        params: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, DispatchError>> + Send + 'a>>;
}

/// Which computers exist, what they are called and which were made from which.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub names: HashMap<String, String>,
    pub linux: HashSet<String>,
    pub parents: HashMap<String, Option<String>>,
}

impl Snapshot {
    pub fn from_values(list: &Value, tree: &Value) -> Self {
        let mut snapshot = Self::default();
        for computer in list
            .pointer("/linux/computers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = computer
                .pointer("/configuration/id")
                .and_then(Value::as_str)
            {
                snapshot.linux.insert(id.to_owned());
            }
        }
        for node in tree
            .get("nodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = node.get("id").and_then(Value::as_str) else {
                continue;
            };
            if let Some(name) = node.get("name").and_then(Value::as_str) {
                snapshot.names.insert(id.to_owned(), name.to_owned());
            }
            let parent = node
                .get("parent")
                .and_then(Value::as_str)
                .map(str::to_owned);
            snapshot.parents.insert(id.to_owned(), parent);
        }
        snapshot
    }

    pub fn descendants_of(&self, root: &str) -> HashSet<String> {
        let mut found = HashSet::new();
        let mut frontier = vec![root.to_owned()];
        while let Some(parent) = frontier.pop() {
            for (id, of) in &self.parents {
                if of.as_deref() == Some(parent.as_str()) && id != root && found.insert(id.clone())
                {
                    frontier.push(id.clone());
                }
            }
        }
        found
    }
}

/// The job ids each computer's client started, newest last, so a client can follow only those.
#[derive(Default)]
pub struct Jobs(Mutex<HashMap<String, VecDeque<String>>>);

impl Jobs {
    fn note(&self, computer: &str, job: &str) {
        let mut all = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let list = all.entry(computer.to_owned()).or_default();
        list.push_back(job.to_owned());
        while list.len() > KEPT_JOBS {
            list.pop_front();
        }
    }

    fn owns(&self, computer: &str, job: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(computer)
            .is_some_and(|list| list.iter().any(|known| known == job))
    }
}

struct World<'a> {
    snapshot: &'a Snapshot,
    requests: &'a dyn Fn(&str) -> Option<String>,
    jobs: &'a Jobs,
}

impl Topology for World<'_> {
    fn descendants(&self, computer: &str) -> HashSet<String> {
        self.snapshot.descendants_of(computer)
    }
    fn request_fork(&self, request_id: &str) -> Option<String> {
        (self.requests)(request_id)
    }
    fn owns_job(&self, caller: &str, job_id: &str) -> bool {
        self.jobs.owns(caller, job_id)
    }
}

fn failure(id: &str, code: &str, message: impl Into<String>) -> GuestResponseFrame {
    GuestResponseFrame {
        v: 1,
        id: id.to_owned(),
        ok: false,
        result: None,
        error: Some(GuestErrorBody {
            code: code.to_owned(),
            message: message.into(),
        }),
    }
}

/// Answers one request from `computer`'s bridge folder.
pub async fn handle_frame(
    runner: &dyn Runner,
    store: &TokenStore,
    jobs: &Jobs,
    snapshot: &Snapshot,
    requests: &dyn Fn(&str) -> Option<String>,
    computer: &str,
    frame: GuestRequestFrame,
) -> GuestResponseFrame {
    let id = frame.id.clone();
    let Some(record) = store.verify(&frame.token, computer) else {
        eprintln!(
            "silod: guest client of {computer}: refused {} (token not valid)",
            frame.method
        );
        return failure(
            &id,
            "permission_denied",
            "This token is not valid for this computer. Ask the person who runs Silo to install it again.",
        );
    };
    let world = World {
        snapshot,
        requests,
        jobs,
    };
    let authorized = match guest_client::authorize(
        computer,
        &record.policy,
        &frame.method,
        &frame.params,
        &world,
    ) {
        Ok(authorized) => authorized,
        Err(refusal) => {
            eprintln!(
                "silod: guest client of {computer}: refused {}",
                frame.method
            );
            return failure(&id, "permission_denied", refusal.0);
        }
    };
    eprintln!("silod: guest client of {computer}: {}", frame.method);
    match runner.run(&frame.method, authorized.params).await {
        Ok(mut result) => {
            if authorized.starts_job {
                if let Some(job) = result.get("jobId").and_then(Value::as_str) {
                    jobs.note(computer, job);
                }
            }
            if authorized.filter != Filter::None {
                guest_client::filter_result(authorized.filter, &mut result, computer, &world);
            }
            GuestResponseFrame {
                v: 1,
                id,
                ok: true,
                result: Some(result),
                error: None,
            }
        }
        Err(error) => failure(
            &id,
            &serde_json::to_value(error.code)
                .ok()
                .and_then(|code| code.as_str().map(str::to_owned))
                .unwrap_or_else(|| "internal".into()),
            error.message,
        ),
    }
}

/// Runs methods through the engine's own handlers as the client `guest-client`.
pub struct EngineRunner {
    context: HandlerContext,
}

impl EngineRunner {
    pub fn new(engine: Arc<SilodContext>) -> Self {
        Self {
            // No socket client asked, so what would answer only the asker (a list's event
            // barrier) is announced to every subscriber, as for another device's request.
            context: HandlerContext::for_owner_bridge(
                engine,
                RequestContext {
                    client_id: "guest-client".to_owned(),
                    protocol: PROTOCOL_VERSION,
                    client: None,
                },
            ),
        }
    }

    pub async fn snapshot(&self) -> Result<Snapshot, DispatchError> {
        let list = self.run("computers.list", json!({})).await?;
        let tree = self.run("computers.tree", json!({})).await?;
        Ok(Snapshot::from_values(&list, &tree))
    }
}

impl Runner for EngineRunner {
    fn run<'a>(
        &'a self,
        method: &'a str,
        params: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, DispatchError>> + Send + 'a>> {
        Box::pin(async move {
            let method = silo_api::methods::method_registry()
                .iter()
                .find(|descriptor| descriptor.name == method)
                .map(|descriptor| descriptor.method)
                .ok_or_else(|| DispatchError::unsupported(method))?;
            if method == MethodName::GuestClientEnable {
                return Err(DispatchError::unsupported("guestClient.enable"));
            }
            methods::dispatch(&self.context, method, &params)
                .await
                .map(|output| output.value)
        })
    }
}

/// Starts the loop that serves the bridge folders of computers that have a token.
pub fn start(context: Arc<SilodContext>) {
    let Some(handle) = methods::remote::runtime_handle() else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("silod-guest-bridge".into())
        .spawn(move || serve(&context, &handle));
    if let Err(error) = spawned {
        eprintln!("silod: could not start the guest client bridge: {error}");
    }
}

fn serve(context: &Arc<SilodContext>, handle: &tokio::runtime::Handle) {
    let app_data = context.paths().app_data.clone();
    let store = TokenStore::new(&app_data);
    let runner = EngineRunner::new(context.clone());
    let jobs = Jobs::default();
    let requests = move |id: &str| {
        silo_engine::fold_requests::Store::new(&app_data)
            .get(id)
            .ok()
            .flatten()
            .map(|request| request.fork_id)
    };
    let mut snapshot = Snapshot::default();
    let mut refreshed: Option<Instant> = None;
    let mut swept = Instant::now();
    loop {
        std::thread::sleep(POLL);
        let records = store.enabled();
        if records.is_empty() {
            continue;
        }
        let Ok(home) = silo_engine::runtime::runtime_paths(context.runtime_host().as_ref())
            .map(|paths| paths.home)
        else {
            continue;
        };
        if refreshed.is_none_or(|at| at.elapsed() > REFRESH) {
            if let Ok(fresh) = handle.block_on(runner.snapshot()) {
                snapshot = fresh;
            }
            refreshed = Some(Instant::now());
        }
        for record in &records {
            let Some(name) = snapshot.names.get(&record.computer_id) else {
                continue;
            };
            if !snapshot.linux.contains(&record.computer_id) {
                continue;
            }
            let spool = Spool::new(guest_client::spool_dir(&home, name));
            for taken in spool.take() {
                let reply = match taken.frame {
                    Ok(frame) => handle.block_on(handle_frame(
                        &runner,
                        &store,
                        &jobs,
                        &snapshot,
                        &requests,
                        &record.computer_id,
                        frame,
                    )),
                    Err(message) => failure(&taken.id, "invalid", message),
                };
                if let Err(error) = spool.respond(&reply) {
                    eprintln!("silod: guest client of {}: {error}", record.computer_id);
                }
            }
            if swept.elapsed() > Duration::from_secs(60) {
                spool.sweep(SystemTime::now());
            }
        }
        if swept.elapsed() > Duration::from_secs(60) {
            swept = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests;
