//! One request frame: token, scope, the bookkeeping of forks, starts and jobs, the engine call,
//! and the answer.

use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use silo_api::methods::guest_client::{GuestErrorBody, GuestRequestFrame, GuestResponseFrame};
use silo_engine::guest_client::{self, now_secs, Filter, Record, TokenStore, Topology};

use super::{Runner, Snapshot};

/// A start that has not shown as running yet still holds its place under the running cap.
const START_HOLD: Duration = Duration::from_secs(300);
/// A fork with no job after this long, or an unconfirmed one after [`UNCONFIRMED`], is dropped.
const NO_JOB: u64 = 120;
const UNCONFIRMED: u64 = 1800;
/// A job nobody acknowledged is acknowledged for its client after this long.
const JOB_AGE: u64 = 1800;

/// Starts issued for each computer's forks and not yet seen running.
#[derive(Default)]
pub struct Reserved(Mutex<HashMap<String, HashMap<String, Instant>>>);

impl Reserved {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, HashMap<String, Instant>>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn add(&self, computer: &str, fork: &str) {
        self.lock()
            .entry(computer.to_owned())
            .or_default()
            .insert(fork.to_owned(), Instant::now());
    }

    fn remove(&self, computer: &str, fork: &str) {
        if let Some(held) = self.lock().get_mut(computer) {
            held.remove(fork);
        }
    }

    fn ids(&self, computer: &str) -> HashSet<String> {
        self.lock()
            .get(computer)
            .into_iter()
            .flatten()
            .filter(|(_, at)| at.elapsed() < START_HOLD)
            .map(|(id, _)| id.clone())
            .collect()
    }
}

/// What a frame is judged against.
pub struct Frames<'a> {
    pub runner: &'a dyn Runner,
    pub store: &'a TokenStore,
    pub snapshot: &'a Snapshot,
    pub requests: &'a dyn Fn(&str) -> Option<String>,
    pub reserved: &'a Reserved,
}

struct World<'a> {
    env: &'a Frames<'a>,
    record: &'a Record,
}

impl Topology for World<'_> {
    fn created(&self, computer: &str) -> HashSet<String> {
        let below = self.env.snapshot.descendants_of(computer);
        self.record
            .fork_ids
            .iter()
            .filter(|id| below.contains(*id))
            .cloned()
            .collect()
    }
    fn running(&self, computer: &str) -> usize {
        let mut running: HashSet<String> = self.env.reserved.ids(computer);
        for id in self.created(computer) {
            if self
                .env
                .snapshot
                .states
                .get(&id)
                .is_some_and(|state| state == "running")
            {
                running.insert(id);
            }
        }
        // A fork still being made will be started next.
        running.len() + self.record.pending.len()
    }
    fn forks_asked(&self, _computer: &str) -> usize {
        self.record.fork_ids.len() + self.record.pending.len()
    }
    fn name_taken(&self, name: &str) -> bool {
        self.env
            .snapshot
            .names
            .values()
            .any(|taken| taken.eq_ignore_ascii_case(name))
            || self
                .record
                .pending
                .iter()
                .any(|pending| pending.name.eq_ignore_ascii_case(name))
    }
    fn jobs_outstanding(&self, _computer: &str) -> usize {
        self.record.jobs.len()
    }
    fn request_fork(&self, request_id: &str) -> Option<String> {
        (self.env.requests)(request_id)
    }
    fn owns_job(&self, _caller: &str, job_id: &str) -> bool {
        self.record.owns_job(job_id)
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

/// Confirms the forks this token asked for from their finished jobs, drops the ones that failed
/// or never started, and forgets forks that no longer exist. Returns the record as it is now.
async fn settle(env: &Frames<'_>, computer: &str, record: Record) -> Record {
    let below = env.snapshot.descendants_of(computer);
    let mut adopt: Vec<(String, String)> = Vec::new();
    let mut drop_names: Vec<String> = Vec::new();
    let now = now_secs();
    for pending in &record.pending {
        let age = now.saturating_sub(pending.at);
        let Some(job) = &pending.job_id else {
            if age > NO_JOB {
                drop_names.push(pending.name.clone());
            }
            continue;
        };
        match env
            .runner
            .run("operations.job.get", json!({ "jobId": job }))
            .await
        {
            Ok(reply) if reply["state"] == "finished" => {
                if reply["outcome"]["error"].is_null() {
                    let made = env.snapshot.names.iter().find(|(id, name)| {
                        **name == pending.name
                            && below.contains(*id)
                            && !record.fork_ids.contains(*id)
                    });
                    match made {
                        Some((id, _)) => adopt.push((pending.name.clone(), id.clone())),
                        None if age > UNCONFIRMED => drop_names.push(pending.name.clone()),
                        None => {}
                    }
                } else {
                    drop_names.push(pending.name.clone());
                }
            }
            Ok(_) => {}
            Err(_) if age > UNCONFIRMED => drop_names.push(pending.name.clone()),
            Err(_) => {}
        }
    }
    let healthy = !env.snapshot.names.is_empty();
    let gone = healthy
        && record
            .fork_ids
            .iter()
            .any(|id| !env.snapshot.names.contains_key(id));
    if adopt.is_empty() && drop_names.is_empty() && !gone {
        return record;
    }
    let changed = env.store.update(computer, |record| {
        for (name, id) in &adopt {
            record.pending.retain(|pending| pending.name != *name);
            if !record.fork_ids.contains(id) {
                record.fork_ids.push(id.clone());
            }
        }
        record
            .pending
            .retain(|pending| !drop_names.contains(&pending.name));
        if healthy {
            record
                .fork_ids
                .retain(|id| env.snapshot.names.contains_key(id));
        }
        Ok(())
    });
    if let Err(error) = changed {
        eprintln!("silod: guest client of {computer}: {error}");
    }
    env.store.status(computer).ok().flatten().unwrap_or(record)
}

/// Answers one request from `computer`'s bridge folder. A frame with no valid token gets no
/// answer at all, so a guest that cannot prove itself cannot fill the folder.
pub async fn handle_frame(
    env: &Frames<'_>,
    computer: &str,
    frame: GuestRequestFrame,
) -> Option<GuestResponseFrame> {
    let id = frame.id.clone();
    let Some(record) = env.store.verify(&frame.token, computer) else {
        eprintln!("silod: guest client of {computer}: dropped a call (token not valid)");
        return None;
    };
    let record = settle(env, computer, record).await;
    // Only a method of the allow-list is ever written to the log; the guest chose the rest.
    let method = guest_client::ALLOWED
        .iter()
        .find(|allowed| **allowed == frame.method)
        .copied()
        .unwrap_or("(a method that is not offered)");
    let world = World {
        env,
        record: &record,
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
            eprintln!("silod: guest client of {computer}: refused {method}");
            return Some(failure(&id, "permission_denied", refusal.0));
        }
    };
    let text = |key: &str| authorized.params[key].as_str().map(str::to_owned);
    let fork_name = (method == "checkpoints.fork")
        .then(|| text("newName"))
        .flatten();
    if let Some(name) = &fork_name {
        match env.store.update(computer, |record| record.begin_fork(name)) {
            Ok(Some(())) => {}
            Ok(None) => {
                return Some(failure(
                    &id,
                    "permission_denied",
                    "This computer has no token.",
                ))
            }
            Err(message) => return Some(failure(&id, "permission_denied", message)),
        }
    }
    let started = (method == "computers.start")
        .then(|| text("computerId"))
        .flatten();
    if let Some(fork) = &started {
        env.reserved.add(computer, fork);
    }
    if method == "computers.stop" {
        if let Some(fork) = text("computerId") {
            env.reserved.remove(computer, &fork);
        }
    }
    eprintln!("silod: guest client of {computer}: {method}");
    match env
        .runner
        .run(&frame.method, authorized.params.clone())
        .await
    {
        Ok(mut result) => {
            let job = result
                .get("jobId")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let acknowledged = (method == "operations.job.acknowledge")
                .then(|| text("jobId"))
                .flatten();
            let noted = env.store.update(computer, |record| {
                if let (true, Some(job)) = (authorized.starts_job, &job) {
                    record.note_job(job);
                    if let Some(name) = &fork_name {
                        if let Some(pending) = record
                            .pending
                            .iter_mut()
                            .find(|pending| pending.name == *name)
                        {
                            pending.job_id = Some(job.clone());
                        }
                    }
                }
                if let Some(done) = &acknowledged {
                    record.drop_job(done);
                }
                Ok(())
            });
            if let Err(error) = noted {
                eprintln!("silod: guest client of {computer}: {error}");
            }
            if authorized.filter != Filter::None {
                let world = World {
                    env,
                    record: &record,
                };
                guest_client::filter_result(authorized.filter, &mut result, computer, &world);
            }
            Some(GuestResponseFrame {
                v: 1,
                id,
                ok: true,
                result: Some(result),
                error: None,
            })
        }
        Err(error) => {
            if let Some(name) = &fork_name {
                let _ = env.store.update(computer, |record| {
                    record.pending.retain(|pending| pending.name != *name);
                    Ok(())
                });
            }
            if let Some(fork) = &started {
                env.reserved.remove(computer, fork);
            }
            Some(failure(
                &id,
                &serde_json::to_value(error.code)
                    .ok()
                    .and_then(|code| code.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "internal".into()),
                error.message,
            ))
        }
    }
}

/// Acknowledges, for a client that never did, the jobs it started long ago and that are over,
/// so no job of a guest stays in the engine's queue.
pub async fn reap(runner: &dyn Runner, store: &TokenStore, computer: &str) {
    let Some(record) = store.status(computer).ok().flatten() else {
        return;
    };
    let now = now_secs();
    let mut done: Vec<String> = Vec::new();
    for job in record
        .jobs
        .iter()
        .filter(|job| now.saturating_sub(job.at) > JOB_AGE)
    {
        match runner
            .run("operations.job.get", json!({ "jobId": job.id }))
            .await
        {
            Ok(reply) if reply["state"] != "finished" => {}
            Ok(_) | Err(_) => {
                let _ = runner
                    .run("operations.job.acknowledge", json!({ "jobId": job.id }))
                    .await;
                done.push(job.id.clone());
            }
        }
    }
    if !done.is_empty() {
        let _ = store.update(computer, |record| {
            for job in &done {
                record.drop_job(job);
            }
            Ok(())
        });
    }
}
