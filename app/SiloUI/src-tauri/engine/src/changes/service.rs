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
    diff as diff_module,
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

fn no_base() -> GuestError {
    GuestError::not_found(
        "This computer has no fork-time record. Computers forked with silo fork have one.",
    )
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
        .ok_or_else(no_base)?;
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
    let ignore = IgnoreList::builtin();
    let cached = match base_meta.source.as_deref() {
        Some(source) => store.get_baseline(source).map_err(internal)?,
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
        &parse(&base_text)?,
        &parse(&current_text)?,
        noise.as_ref(),
        &ignore,
    );
    to_value(ChangesDiffResult {
        diff: to_value(&result)?,
        text: result.render(max_lines.map_or(DEFAULT_MAX_LINES, |lines| lines as usize)),
        base_captured_at: base_meta.captured_at,
        captured_at: current_meta.captured_at,
        baseline_applied: noise.is_some(),
        baseline_available: cached.is_some(),
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
        .ok_or_else(no_base)?;
    let source = source.or(base_meta.source.clone()).ok_or_else(|| {
        GuestError::invalid("Name the checkpoint this computer was forked from (source).")
    })?;
    let idle = idle_seconds.unwrap_or(DEFAULT_IDLE_SECONDS);
    // The fork is judged idle from its base capture, so one that has already sat that long
    // is captured at once.
    if let Some(started) = store::unix_seconds(&base_meta.captured_at) {
        let due = started + idle as i64;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        if due > now {
            std::thread::sleep(Duration::from_secs((due - now) as u64));
        }
    }
    let text = fresh(runner, None, options)?;
    let meta = store
        .put_baseline(&source, &base_text, &text)
        .map_err(internal)?;
    to_value(ChangesBaselineResult {
        source,
        idle_seconds: idle,
        captured_at: meta.captured_at,
    })
}

/// Accepts the work as a job and runs it on its own thread.
pub fn start(
    context: Arc<dyn EngineContext>,
    runner: Arc<dyn GuestRunner>,
    computer_id: String,
    method: &str,
    work: Work,
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
        );
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
