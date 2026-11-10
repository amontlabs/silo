//! One request frame: the token, the bridge's caps on forks, running forks and jobs, the call
//! (judged by the policy as the computer's own), and the answer.

use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use silo_api::{
    methods::{
        guest_client::{GuestErrorBody, GuestRequestFrame, GuestResponseFrame},
        method_registry, MethodKind, MethodName,
    },
    transport::ApprovalRequiredDetails,
};
use silo_engine::{
    guest_client::{now_secs, TokenStore, MAX_FORKS, MAX_JOBS, MAX_RUNNING},
    policy::store::PolicyStore,
};

use super::{Runner, Snapshot};

/// A start that has not shown as running yet still holds its place under the running cap.
const START_HOLD: Duration = Duration::from_secs(300);
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
    pub policy: &'a PolicyStore,
    pub snapshot: &'a Snapshot,
    pub reserved: &'a Reserved,
}

fn failure(
    id: &str,
    code: &str,
    message: impl Into<String>,
    details: Option<ApprovalRequiredDetails>,
) -> GuestResponseFrame {
    GuestResponseFrame {
        v: 1,
        id: id.to_owned(),
        ok: false,
        result: None,
        error: Some(GuestErrorBody {
            code: code.to_owned(),
            message: message.into(),
            details,
        }),
    }
}

fn starts_job(method: &str) -> bool {
    method_registry()
        .iter()
        .any(|descriptor| descriptor.name == method && descriptor.kind == MethodKind::Job)
}

/// The bridge's own limits: the computers one computer has made, how many of them run, and
/// the jobs it has not acknowledged.
fn within_caps(env: &Frames<'_>, computer: &str, method: &str) -> Result<(), String> {
    let document = env.policy.snapshot()?;
    let made: HashSet<String> = document
        .made_directly(computer)
        .into_iter()
        .filter(|id| env.snapshot.names.contains_key(id))
        .collect();
    let pending = document.pending_of(computer);
    let mut running = env.reserved.ids(computer);
    for id in &made {
        if env
            .snapshot
            .states
            .get(id)
            .is_some_and(|state| state == "running")
        {
            running.insert(id.clone());
        }
    }
    // A computer still being made will be started next.
    let running = running.len() + pending;
    if matches!(method, "checkpoints.fork" | "computers.create") {
        if made.len() + pending >= MAX_FORKS {
            return Err(format!(
                "This computer has made {MAX_FORKS} computers already; delete one first."
            ));
        }
        if running >= MAX_RUNNING {
            return Err(format!(
                "{MAX_RUNNING} of the computers this one made are running; stop one first."
            ));
        }
    }
    if method == "computers.start" && running >= MAX_RUNNING {
        return Err(format!(
            "{MAX_RUNNING} of the computers this one made are running; stop one first."
        ));
    }
    // A job nobody follows still takes a place in the engine's job admission.
    if starts_job(method) && document.jobs_of(computer).len() >= MAX_JOBS {
        return Err(format!(
            "{MAX_JOBS} jobs of this computer's client are not finished and acknowledged yet."
        ));
    }
    Ok(())
}

/// Answers one request from `computer`'s bridge folder. A frame with no valid token gets no
/// answer at all, so a guest that cannot prove itself cannot fill the folder.
pub async fn handle_frame(
    env: &Frames<'_>,
    computer: &str,
    frame: GuestRequestFrame,
) -> Option<GuestResponseFrame> {
    let id = frame.id.clone();
    if env.store.verify(&frame.token, computer).is_none() {
        eprintln!("silod: guest client of {computer}: dropped a call (token not valid)");
        return None;
    }
    // Only a method of the registry is ever written to the log; the guest chose the rest.
    let method = MethodName::from_name(&frame.method)
        .map(MethodName::name)
        .unwrap_or("(a method that is not offered)");
    if let Err(message) = within_caps(env, computer, &frame.method) {
        eprintln!("silod: guest client of {computer}: refused {method}");
        return Some(failure(&id, "permission_denied", message, None));
    }
    let target = frame
        .params
        .get("computerId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let started = (method == "computers.start").then_some(target.clone()).flatten();
    if let Some(fork) = &started {
        env.reserved.add(computer, fork);
    }
    if method == "computers.stop" {
        if let Some(fork) = &target {
            env.reserved.remove(computer, fork);
        }
    }
    eprintln!("silod: guest client of {computer}: {method}");
    match env
        .runner
        .run(Some(computer), &frame.method, frame.params)
        .await
    {
        Ok(result) => Some(GuestResponseFrame {
            v: 1,
            id,
            ok: true,
            result: Some(result),
            error: None,
        }),
        Err(error) => {
            if let Some(fork) = &started {
                env.reserved.remove(computer, fork);
            }
            let code = serde_json::to_value(error.code)
                .ok()
                .and_then(|code| code.as_str().map(str::to_owned))
                .unwrap_or_else(|| "internal".into());
            Some(failure(&id, &code, error.message, error.details))
        }
    }
}

/// Acknowledges, for a client that never did, the jobs it started long ago and that are over,
/// so no job of a guest stays in the engine's queue.
pub async fn reap(runner: &dyn Runner, policy: &PolicyStore, computer: &str) {
    let Ok(document) = policy.snapshot() else {
        return;
    };
    let now = now_secs();
    let mut done: Vec<String> = Vec::new();
    for job in document
        .jobs_of(computer)
        .into_iter()
        .filter(|job| now.saturating_sub(job.at) > JOB_AGE)
    {
        match runner
            .run(None, "operations.job.get", json!({ "jobId": job.id }))
            .await
        {
            Ok(reply) if reply["state"] != "finished" => {}
            Ok(_) | Err(_) => {
                let _ = runner
                    .run(
                        None,
                        "operations.job.acknowledge",
                        json!({ "jobId": job.id }),
                    )
                    .await;
                done.push(job.id.clone());
            }
        }
    }
    for job in done {
        if let Err(error) = policy.drop_job(computer, &job) {
            eprintln!("silod: guest client of {computer}: {error}");
        }
    }
}
