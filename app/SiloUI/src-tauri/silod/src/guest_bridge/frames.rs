//! One request frame: the token, the bridge's cap on unacknowledged jobs, the call (judged by
//! the policy as the computer's own), and the answer.

use serde_json::json;
use silo_api::{
    jobs::ErrorCode,
    methods::{
        guest_client::{GuestErrorBody, GuestRequestFrame, GuestResponseFrame},
        method_registry, MethodKind, MethodName,
    },
    transport::ApprovalRequiredDetails,
};
use silo_engine::{
    guest_client::{now_secs, TokenStore, MAX_JOBS},
    policy::store::PolicyStore,
};

use super::{Runner, Snapshot};

/// A job nobody acknowledged is acknowledged for its client after this long.
const JOB_AGE: u64 = 1800;

/// What a frame is judged against.
pub struct Frames<'a> {
    pub runner: &'a dyn Runner,
    pub store: &'a TokenStore,
    pub policy: &'a PolicyStore,
    pub snapshot: &'a Snapshot,
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

/// The bridge's own limit: the jobs a computer has not acknowledged. A job nobody follows
/// still takes a place in the engine's job admission.
fn within_job_cap(env: &Frames<'_>, computer: &str, method: &str) -> Result<(), String> {
    let document = env.policy.snapshot()?;
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
    if MethodName::from_name(&frame.method).is_none() {
        env.runner
            .refused(computer, method, ErrorCode::UnsupportedRemoteOperation);
    }
    if let Err(message) = within_job_cap(env, computer, &frame.method) {
        eprintln!("silod: guest client of {computer}: refused {method}");
        env.runner
            .refused(computer, method, ErrorCode::PermissionDenied);
        return Some(failure(&id, "permission_denied", message, None));
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
