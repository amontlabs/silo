//! The `changes.promote` job, and the promote side of `changes.undo`.

use std::sync::Arc;

use silo_api::jobs::JobId;

use super::{engine_env::EngineEnv, Journal, Request, LANE_WAIT};
use crate::{
    changes::keep::{self, env::ManifestStore, service as keep_service},
    checkpoints::{Failure, Service},
    context::EngineContext,
    runtime::RuntimeHost,
};

pub fn journal_for(context: &dyn EngineContext) -> Journal {
    Journal::new(context.paths().app_data.join("changes").join("promote"))
}

/// Which promote or keep an undo of `computer` would take back now: it changes when another
/// merge or promote into the computer finishes.
pub fn undo_pin(context: &dyn EngineContext, computer: &str) -> String {
    let promoted = super::latest_at(&journal_for(context), computer);
    let journal = keep_service::journal_for(context);
    let kept = journal
        .latest_undoable(computer)
        .map(|checkpoint| (checkpoint.id, journal.latest_undoable_at(computer)));
    format!("promote:{promoted:?};keep:{kept:?}")
}

/// The state of `fork` that a promote of it acts on now: it changes when the fork does.
pub fn promote_pin(fork: &str) -> String {
    format!("generation:{}", crate::generation::generation(fork))
}

/// Refuses a call whose `expected` pin is no longer the `current` one.
fn pinned(expected: Option<&str>, current: impl FnOnce() -> String, refusal: &str) -> Result<(), String> {
    match expected {
        Some(expected) if current() != expected => Err(refusal.to_owned()),
        _ => Ok(()),
    }
}

/// The check a promote of `fork` makes once it holds its lanes: the fork is still as
/// [`promote_pin`] saw it when `expected_pin` is given.
pub fn promote_guard(expected_pin: Option<String>, fork: String) -> impl Fn() -> Result<(), String> {
    move || {
        pinned(
            expected_pin.as_deref(),
            || promote_pin(&fork),
            "Nothing was replaced: the fork changed after you were asked. The computer can ask again.",
        )
    }
}

/// `changes.promote`. `expected_pin` is the [`promote_pin`] an approval was given for; the job
/// refuses to change anything when the fork has moved since, which it checks holding the lanes.
pub fn start_promote(
    host: Arc<dyn RuntimeHost>,
    service: Service,
    store: Arc<dyn ManifestStore>,
    request: Request,
    expected_pin: Option<String>,
) -> Result<JobId, Failure> {
    keep_service::start(host, "changes.promote", move |host| {
        let context = host.engine_context();
        let journal = journal_for(context.as_ref());
        let env = EngineEnv::new(
            host.clone(),
            service,
            keep_service::journal_for(context.as_ref()),
        );
        let guard = promote_guard(expected_pin, request.fork.clone());
        // Once the lanes are held and the fork is as approved, the promote is a change to
        // every computer: it moves their generation.
        let under_lane = || {
            guard()?;
            crate::generation::bump_all();
            Ok(())
        };
        super::promote_checked(&env, store.as_ref(), &journal, &request, LANE_WAIT, &under_lane)
            .map(|outcome| outcome.to_json())
    })
}

/// Swaps a promote back when `computer` is part of one; the keep undo runs otherwise.
/// `expected_pin` is the [`undo_pin`] an approval was given for; the job refuses to change
/// anything when another merge or replacement happened since, which it checks holding the lane.
pub fn start_undo(
    host: Arc<dyn RuntimeHost>,
    service: Service,
    store: Arc<dyn ManifestStore>,
    computer: String,
    expected_pin: Option<String>,
) -> Result<JobId, Failure> {
    keep_service::start(host, "changes.undo", move |host| {
        let context = host.engine_context();
        let journal = journal_for(context.as_ref());
        let keep_journal = keep_service::journal_for(context.as_ref());
        let env = EngineEnv::new(host.clone(), service.clone(), keep_journal.clone());
        // `silo undo` takes back the newest thing done to the computer: a promote or a keep.
        // An interrupted keep is put back first, whatever came before it.
        let promoted = super::latest_at(&journal, &computer);
        let kept = keep_journal.latest_undoable_at(&computer);
        let under_lane = || {
            pinned(
                expected_pin.as_deref(),
                || undo_pin(context.as_ref(), &computer),
                "Nothing was undone: another merge or replacement happened after you were asked. The computer can ask again.",
            )
        };
        if super::promote_is_newest(promoted, kept, keep_journal.has_pending(&computer)) {
            if let Some(done) =
                super::undo_checked(&env, &journal, &computer, LANE_WAIT, &under_lane)?
            {
                return Ok(done.to_json());
            }
        }
        let guest = keep::guest_env::GuestEnv::new(
            host.clone(),
            service,
            keep_journal.clone(),
            Box::new(|_| {}),
        );
        keep::undo_checked(
            &guest as &dyn keep::KeepEnv,
            store.as_ref(),
            &keep_journal,
            &computer,
            keep::LANE_WAIT,
            &under_lane,
        )
        .map(|outcome| outcome.to_json())
    })
}
