//! The `changes.*` jobs: capture a computer, diff it against its fork-time base, compute the
//! no-op baseline of a source checkpoint.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use serde_json::Value;
use silo_api::{
    jobs::{ErrorCode, JobId},
    methods::changes::{
        ChangesBaselineResult, ChangesCaptureResult, ChangesDiffResult, ChangesRole,
    },
};

use super::{
    collect::{self, GuestRunner, Options},
    diff as diff_module, keep,
    store::{self, Slot, Store},
    IgnoreList, Manifest,
};
use crate::{
    context::{Audience, EngineContext},
    guest::GuestError,
};

/// How long a fork sits idle before the baseline capture, counted from its base capture.
pub const DEFAULT_IDLE_SECONDS: u64 = 60;
/// The length of the readable diff unless the caller asks for another.
pub const DEFAULT_MAX_LINES: usize = 15;

#[derive(Clone, Debug)]
pub enum Work {
    Capture {
        role: ChangesRole,
        source: Option<String>,
    },
    Diff {
        max_lines: Option<u32>,
        baseline: bool,
        refresh: bool,
    },
    Baseline {
        source: Option<String>,
        idle_seconds: Option<u64>,
    },
}

/// One capture at a time per computer: a second would only duplicate the first's scan.
struct InFlight(String);

fn in_flight() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

impl InFlight {
    fn claim(computer: &str) -> Result<Self, GuestError> {
        let mut set = in_flight()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !set.insert(computer.to_owned()) {
            return Err(GuestError::new(
                ErrorCode::Busy,
                "This computer is already being captured.",
            ));
        }
        Ok(Self(computer.to_owned()))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        in_flight()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.0);
    }
}

fn internal(message: String) -> GuestError {
    GuestError::internal(message)
}

fn parse(text: &str) -> Result<Manifest, GuestError> {
    Manifest::parse(text)
        .map_err(|error| internal(format!("A stored manifest is unreadable: {error}")))
}

fn no_base(store: &Store, computer: &str) -> GuestError {
    let retake =
        "Retake it with changes.capture role=base while the computer is still as it was forked.";
    match store.get_failure(computer) {
        Some(why) => GuestError::not_found(format!(
            "The fork-time record of this computer could not be taken ({why}). {retake}"
        )),
        None => GuestError::not_found(format!(
            "This computer has no fork-time record; forks have one taken when they first start. {retake}"
        )),
    }
}

/// Runs one piece of work to its JSON result.
pub fn run(
    runner: &dyn GuestRunner,
    store: &Store,
    computer: &str,
    work: Work,
    options: &Options,
) -> Result<Value, GuestError> {
    let _claim = InFlight::claim(computer)?;
    match work {
        Work::Capture { role, source } => capture(runner, store, computer, role, source, options),
        Work::Diff {
            max_lines,
            baseline,
            refresh,
        } => diff(
            runner, store, computer, max_lines, baseline, refresh, options,
        ),
        Work::Baseline {
            source,
            idle_seconds,
        } => baseline(runner, store, computer, source, idle_seconds, options),
    }
}

fn to_value(value: impl serde::Serialize) -> Result<Value, GuestError> {
    serde_json::to_value(value).map_err(|error| internal(error.to_string()))
}

/// Captures the computer and, unless it is a base, attributes what changed since `base`.
fn fresh(
    runner: &dyn GuestRunner,
    base: Option<&str>,
    options: &Options,
) -> Result<String, GuestError> {
    let manifest = collect::capture(runner, options)?;
    Ok(match base {
        Some(base) => collect::with_owners(runner, base, manifest),
        None => manifest,
    })
}

fn capture(
    runner: &dyn GuestRunner,
    store: &Store,
    computer: &str,
    role: ChangesRole,
    source: Option<String>,
    options: &Options,
) -> Result<Value, GuestError> {
    let (slot, base) = match role {
        ChangesRole::Base => (Slot::Base, None),
        ChangesRole::Latest => (
            Slot::Latest,
            store.get(computer, Slot::Base).map_err(internal)?,
        ),
    };
    let text = fresh(
        runner,
        base.as_ref().map(|(text, _)| text.as_str()),
        options,
    )?;
    let meta = store
        .put(computer, slot, &text, source.as_deref())
        .map_err(internal)?;
    to_value(ChangesCaptureResult {
        role,
        captured_at: meta.captured_at,
        records: meta.records,
        bytes: meta.bytes,
    })
}

fn diff(
    runner: &dyn GuestRunner,
    store: &Store,
    computer: &str,
    max_lines: Option<u32>,
    use_baseline: bool,
    refresh: bool,
    options: &Options,
) -> Result<Value, GuestError> {
    let (base_text, base_meta) = store
        .get(computer, Slot::Base)
        .map_err(internal)?
        .ok_or_else(|| no_base(store, computer))?;
    let (current_text, current_meta) = if refresh {
        let text = fresh(runner, Some(&base_text), options)?;
        let meta = store
            .put(computer, Slot::Latest, &text, None)
            .map_err(internal)?;
        (text, meta)
    } else {
        store
            .get(computer, Slot::Latest)
            .map_err(internal)?
            .ok_or_else(|| GuestError::not_found("This computer has not been captured yet."))?
    };
    let (result, noise_applied, baseline_available) =
        compare(store, &base_text, &base_meta, &current_text, use_baseline)?;
    let elapsed_seconds = store::unix_seconds(&base_meta.captured_at)
        .zip(store::unix_seconds(&current_meta.captured_at))
        .map(|(base, now)| now.saturating_sub(base).max(0) as u64);
    to_value(ChangesDiffResult {
        base_source: base_meta.source.clone(),
        elapsed_seconds,
        diff: to_value(&result)?,
        text: result.render(max_lines.map_or(DEFAULT_MAX_LINES, |lines| lines as usize)),
        base_captured_at: base_meta.captured_at,
        captured_at: current_meta.captured_at,
        baseline_applied: noise_applied,
        baseline_available,
        review_digest: keep::review_digest(&result),
    })
}

/// The fork's changes since its base, less the idle noise of its source when `use_baseline` is
/// set and a baseline is stored. Also says whether noise was subtracted and whether a baseline
/// exists.
fn compare(
    store: &Store,
    base_text: &str,
    base_meta: &store::Meta,
    current_text: &str,
    use_baseline: bool,
) -> Result<(diff_module::Diff, bool, bool), GuestError> {
    let ignore = IgnoreList::builtin();
    let cached = match base_meta.source.as_deref() {
        Some(source) => store
            .get_baseline_for(source, base_meta.origin.as_deref())
            .map_err(internal)?,
        None => None,
    };
    let noise = match cached.as_ref().filter(|_| use_baseline) {
        Some((before, idle)) => Some(diff_module::diff(
            &parse(before)?,
            &parse(idle)?,
            None,
            &ignore,
        )),
        None => None,
    };
    let result = diff_module::diff(
        &parse(base_text)?,
        &parse(current_text)?,
        noise.as_ref(),
        &ignore,
    );
    Ok((result, noise.is_some(), cached.is_some()))
}

/// The number of changes in each bucket of the diff the stored captures give, when its review
/// digest is `digest`: the counts a review request shows for the diff that was reviewed.
pub fn review_counts(
    store: &Store,
    computer: &str,
    digest: &str,
) -> Option<std::collections::BTreeMap<String, u64>> {
    let (base_text, base_meta) = store.get(computer, Slot::Base).ok().flatten()?;
    let (current_text, _) = store.get(computer, Slot::Latest).ok().flatten()?;
    [true, false].into_iter().find_map(|use_baseline| {
        let (result, _, _) =
            compare(store, &base_text, &base_meta, &current_text, use_baseline).ok()?;
        if keep::review_digest(&result) != digest {
            return None;
        }
        let document = to_value(&result).ok()?;
        Some(silo_api::methods::review_requests::bucket_counts(&document))
    })
}

fn baseline(
    runner: &dyn GuestRunner,
    store: &Store,
    computer: &str,
    source: Option<String>,
    idle_seconds: Option<u64>,
    options: &Options,
) -> Result<Value, GuestError> {
    let (base_text, base_meta) = store
        .get(computer, Slot::Base)
        .map_err(internal)?
        .ok_or_else(|| no_base(store, computer))?;
    let source = source.or(base_meta.source.clone()).ok_or_else(|| {
        GuestError::invalid("Name the checkpoint this computer was forked from (source).")
    })?;
    let idle = idle_seconds.unwrap_or(DEFAULT_IDLE_SECONDS);
    // The fork is judged idle from its base capture, so one that has already sat that long
    // is captured at once. A fork that has been running longer than that has most likely been
    // used: what it holds is the task's changes as much as idle noise, so nothing is measured.
    if let Some(started) = store::unix_seconds(&base_meta.captured_at) {
        let due = started + idle as i64;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        if now > due + BASELINE_WINDOW_SECONDS {
            return Err(too_late());
        }
        if due > now {
            std::thread::sleep(Duration::from_secs((due - now) as u64));
        }
    }
    let text = fresh(runner, None, options)?;
    // Idle noise never moves a repository or installs a package; a fork where either
    // happened was used, and its capture would hide the task's own changes.
    let settled = diff_module::diff(
        &parse(&base_text)?,
        &parse(&text)?,
        None,
        &IgnoreList::builtin(),
    );
    if !settled.code.is_empty() || !settled.packages.is_empty() {
        return Err(too_late());
    }
    let meta = store
        .put_baseline_for(&source, base_meta.origin.as_deref(), &base_text, &text)
        .map_err(internal)?;
    to_value(ChangesBaselineResult {
        source,
        idle_seconds: idle,
        captured_at: meta.captured_at,
    })
}

/// How long after the idle time a baseline may still be measured on the fork itself.
pub const BASELINE_WINDOW_SECONDS: i64 = 120;

fn too_late() -> GuestError {
    GuestError::invalid(
        "No baseline: this fork has been used since it was forked, so its idle noise cannot be told from its changes. A baseline is measured on a fork that has just started and sits untouched.",
    )
}

/// Accepts the work as a job and runs it on its own thread.
pub fn start(
    context: Arc<dyn EngineContext>,
    runner: Arc<dyn GuestRunner>,
    computer_id: String,
    method: &str,
    work: Work,
) -> Result<JobId, GuestError> {
    start_with(context, runner, computer_id, method, work, Box::new(Ok))
}

/// What a job does with the work's result before it is published.
pub type Finish = Box<dyn FnOnce(Value) -> Result<Value, GuestError> + Send>;

/// [`start`], with `finish` turning the work's result into the job's result.
pub fn start_with(
    context: Arc<dyn EngineContext>,
    runner: Arc<dyn GuestRunner>,
    computer_id: String,
    method: &str,
    work: Work,
    finish: Finish,
) -> Result<JobId, GuestError> {
    let registry = context.job_registry().map_err(internal)?;
    let accepted = crate::jobs::allocate(context.as_ref(), method)
        .map_err(|error| internal(error.to_string()))?;
    let job_id = accepted.job_id;
    registry
        .mark_running(&job_id)
        .map_err(|error| internal(error.to_string()))?;
    let store = Store::for_engine(context.as_ref());
    let id = job_id.clone();
    std::thread::spawn(move || {
        let outcome = run(
            runner.as_ref(),
            &store,
            &computer_id,
            work,
            &Options::default(),
        )
        .and_then(finish);
        let _ = match outcome {
            Ok(value) => {
                crate::jobs::finish_with_result(context.as_ref(), id, value, Audience::All)
            }
            Err(error) => crate::jobs::finish_with_error(
                context.as_ref(),
                id,
                error.code,
                error.message,
                Audience::All,
            ),
        };
    });
    Ok(job_id)
}

#[cfg(test)]
mod tests;
