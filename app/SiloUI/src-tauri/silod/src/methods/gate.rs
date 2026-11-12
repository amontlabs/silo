//! The one place a request passes before its handler runs, and again after. [`admit`] asks
//! the policy (`silo_engine::policy`) whether the caller may make the call, records what a
//! computer's agent asked for, and refuses or holds the call; [`finish`] cuts the answer to
//! what the caller may see, records the jobs and computers a computer's agent started, and
//! writes the audit log. No handler is reached any other way, so none can forget either.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, LazyLock},
};

use serde_json::{json, Value};
use silo_api::{
    jobs::{ErrorCode, JobId, JobState as ApiJobState},
    methods::{audit::AuditOutcome, method_registry, MethodKind, MethodName},
};
use silo_engine::{
    audit::{AuditLog, Row},
    context::EngineContext,
    policy::{
        self,
        store::{Document, JobState, Pending, PolicyStore},
        Access, Caller, Creation, Decision, Filter, Resources,
    },
    resources,
};

use super::HandlerContext;
use crate::dispatcher::{DispatchError, HandlerOutput, RequestContext};

static KINDS: LazyLock<HashMap<MethodName, MethodKind>> = LazyLock::new(|| {
    method_registry()
        .iter()
        .map(|descriptor| (descriptor.method, descriptor.kind))
        .collect()
});

fn kind_of(method: MethodName) -> MethodKind {
    KINDS.get(&method).copied().unwrap_or(MethodKind::Command)
}

/// A call the policy let through, and what to do with its answer.
pub(crate) struct Admitted {
    params: Option<Value>,
    filter: Filter,
    /// The caller and the computers it made, for a computer's call.
    visible: Option<Arc<BTreeSet<String>>>,
    creates: Option<Creation>,
    target: Option<String>,
    kind: MethodKind,
    /// The job an acknowledgement names.
    job: Option<String>,
    held: Held,
}

impl Admitted {
    /// The parameters to run: the caller's, or the policy's rewrite of them.
    pub(crate) fn params<'a>(&'a self, original: &'a Value) -> &'a Value {
        self.params.as_ref().unwrap_or(original)
    }

    pub(crate) fn visible(&self) -> Option<Arc<BTreeSet<String>>> {
        self.visible.clone()
    }
}

fn app_data(context: &HandlerContext) -> PathBuf {
    context.silod_context().paths().app_data.clone()
}

pub(crate) fn error_code(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "internal".into())
}

struct Record<'a> {
    method: MethodName,
    target: Option<String>,
    /// The computers a computer's agent may reach: a row names a target only inside this set.
    visible: Option<Arc<BTreeSet<String>>>,
    outcome: AuditOutcome,
    error: Option<&'a DispatchError>,
    job_id: Option<String>,
    /// The approval the call ran under.
    approval_id: Option<String>,
}

fn audit(context: &HandlerContext, record: Record<'_>) {
    let app_data = app_data(context);
    // An engine always has an absolute data folder; a test context without one keeps no log.
    if !app_data.is_absolute() {
        return;
    }
    let request = context.client_identity();
    let caller = &request.caller;
    let client = match caller {
        Caller::Owner { client } => Some(client.clone()),
        _ => request.client.as_ref().map(|client| client.name.clone()),
    };
    AuditLog::shared(&app_data).record(Row {
        at_ms: silo_engine::audit::now_ms(),
        caller_kind: caller.kind(),
        caller_id: caller.id().map(str::to_owned),
        client,
        method: record.method.name().to_owned(),
        // A computer's call names no computer outside the ones it made, so the log tells
        // nothing a refused probe guessed.
        computer_id: match (&caller, &record.visible) {
            (Caller::Computer { .. }, Some(visible)) => {
                record.target.filter(|target| visible.contains(target))
            }
            (Caller::Computer { .. }, None) => None,
            _ => record.target,
        },
        outcome: record.outcome,
        error_code: record.error.map(|error| error_code(error.code)),
        job_id: record.job_id,
        approval_id: record.approval_id.or_else(|| {
            record
                .error
                .and_then(|error| error.details.as_ref())
                .and_then(|details| details.approval_id.clone())
        }),
    });
}

/// Records a call the owner allowed, run by the engine under the approval `approval_id`.
pub(crate) fn record_approved(
    context: &HandlerContext,
    method: MethodName,
    params: &Value,
    asker: &str,
    result: &Result<HandlerOutput, DispatchError>,
    approval_id: &str,
) {
    let (outcome, error) = match result {
        Ok(_) => (AuditOutcome::Ok, None),
        Err(error) => (AuditOutcome::Error, Some(error)),
    };
    let job_id = result
        .as_ref()
        .ok()
        .and_then(|output| output.value.get("jobId"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    audit(
        context,
        Record {
            method,
            // A call that names no computer is recorded against the computer that asked.
            target: policy::target_computer(params).or_else(|| Some(asker.to_owned())),
            outcome,
            error,
            job_id,
            approval_id: Some(approval_id.to_owned()),
            visible: None,
        },
    );
}

/// Notes a computer `creator` asked for, recorded as its own once its job has finished well.
pub(crate) fn begin_creation(
    app_data: &std::path::Path,
    creator: &str,
    creation: &Creation,
) -> Result<(), String> {
    PolicyStore::new(app_data).begin(
        creator,
        Pending {
            created_by: creator.to_owned(),
            name: creation.name.clone(),
            id: creation.id.clone(),
            source: creation.source.clone(),
            job_id: None,
            at: 0,
        },
    )
}

/// Records what became of a computer `creator` asked for, from the call's answer.
pub(crate) fn settle_creation(
    store: &PolicyStore,
    creator: &str,
    creation: &Creation,
    result: &Result<HandlerOutput, DispatchError>,
    job: Option<&str>,
) -> Result<(), String> {
    match (result, job) {
        (Ok(output), _) if output.value.pointer("/computer/id").is_some() => {
            let made = output.value.pointer("/computer/id").and_then(Value::as_str);
            store.confirm(creator, &creation.name, made.unwrap_or_default())
        }
        (Ok(_), Some(job)) => store.attach_job(creator, &creation.name, job),
        (Ok(_), None) => Ok(()),
        (Err(_), _) => store.withdraw(creator, &creation.name),
    }
}

/// A computer's name, when it exists.
pub(crate) async fn computer_name(context: &HandlerContext, id: &str) -> Option<String> {
    let world = EngineWorld::load(context, MethodName::ComputersAccess, None).await;
    policy::World::name(&world, id)
}

/// Computers a test adds to the device the policy sees, where no runtime lists any.
#[cfg(test)]
pub(crate) mod test_nodes {
    use std::sync::Mutex;

    type Node = (String, String, Option<String>);
    static NODES: Mutex<Vec<Node>> = Mutex::new(Vec::new());

    fn nodes() -> std::sync::MutexGuard<'static, Vec<Node>> {
        NODES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn add(id: &str, name: &str, parent: Option<&str>) {
        nodes().push((id.into(), name.into(), parent.map(str::to_owned)));
    }

    pub(crate) fn remove(id: &str) {
        nodes().retain(|(node, _, _)| node != id);
    }

    pub(crate) fn all() -> Vec<Node> {
        nodes().clone()
    }
}

#[cfg(test)]
pub(crate) mod test_resources {
    use super::Resources;
    use std::sync::Mutex;

    static RESOURCES: Mutex<Vec<(String, Resources)>> = Mutex::new(Vec::new());

    fn res() -> std::sync::MutexGuard<'static, Vec<(String, Resources)>> {
        RESOURCES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn add(id: &str, resources: Resources) {
        res().push((id.into(), resources));
    }

    pub(crate) fn clear() {
        res().clear();
    }

    pub(crate) fn all() -> Vec<(String, Resources)> {
        res().clone()
    }
}

/// The memory, in MiB, a test says a computer's pending full restore resumes at.
#[cfg(test)]
pub(crate) mod test_resumes {
    use std::sync::Mutex;

    type Resume = (String, Result<u64, String>);
    static RESUMES: Mutex<Vec<Resume>> = Mutex::new(Vec::new());

    fn resumes() -> std::sync::MutexGuard<'static, Vec<Resume>> {
        RESUMES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn add(id: &str, resume: Result<u64, String>) {
        resumes().push((id.into(), resume));
    }

    pub(crate) fn all() -> Vec<Resume> {
        resumes().clone()
    }
}

/// The computers a test says hold their memory on the device now.
#[cfg(test)]
pub(crate) mod test_resident {
    use std::{collections::HashSet, sync::Mutex};

    static RESIDENT: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn resident() -> std::sync::MutexGuard<'static, Vec<String>> {
        RESIDENT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn add(id: &str) {
        resident().push(id.into());
    }

    pub(crate) fn remove(id: &str) {
        resident().retain(|resident| resident != id);
    }

    pub(crate) fn all() -> HashSet<String> {
        resident().iter().cloned().collect()
    }
}

/// The device now and the floors that apply to an agent inside a computer.
pub(crate) fn host_and_floors(
    context: &HandlerContext,
) -> Result<(resources::Host, resources::Floors), DispatchError> {
    let host = resources::measure(&app_data(context)).ok_or_else(|| {
        DispatchError::internal("This device's memory and disk could not be measured.")
    })?;
    // Settings that cannot be read leave the default floors, never none.
    let (memory, disk) = silo_engine::settings::resource_floors_for(context.engine_context())
        .unwrap_or((None, None));
    let floors = resources::Floors::resolve(&host, memory, disk);
    Ok((host, floors))
}

/// A call that starts, creates or checkpoints a computer, or that can restart the computers an
/// undo or a promote swaps, which takes memory or disk.
pub(crate) fn takes_headroom(method: MethodName) -> bool {
    matches!(
        method,
        MethodName::ComputersStart
            | MethodName::ComputersRestart
            | MethodName::ComputersCreate
            | MethodName::CheckpointsFork
            | MethodName::CheckpointsCreate
            | MethodName::CheckpointsRestore
            | MethodName::ChangesUndo
            | MethodName::ChangesPromote
            | MethodName::MacosSetupRetry
    )
}

#[derive(Clone, Debug)]
struct Reservation {
    id: u64,
    computer_id: Option<String>,
    /// The computer the memory is for, when it is not resident yet: once it is, the device's
    /// own measurement holds its memory and the reservation would count it twice.
    boots: Option<String>,
    job_id: Option<String>,
    memory_bytes: u64,
    created_at: std::time::Instant,
    /// When the registry was first seen to have the reservation's job running, which is when
    /// the work behind it starts: a job waits in the registry for its turn before that.
    running_since: Option<std::time::Instant>,
}

/// The computers that hold their memory on the device, as of when the list they come from was
/// loaded. A list older than a reservation says nothing about the computer the reservation
/// is for, since the computer may have stopped after the list was read.
#[derive(Clone, Debug)]
struct Residency {
    ids: HashSet<String>,
    loaded_at: std::time::Instant,
}

impl Residency {
    fn contains(&self, computer: &str) -> bool {
        self.ids.contains(computer)
    }
}

impl Default for Residency {
    fn default() -> Self {
        Self {
            ids: HashSet::new(),
            loaded_at: std::time::Instant::now(),
        }
    }
}

static RESERVATIONS: std::sync::Mutex<Vec<Reservation>> = std::sync::Mutex::new(Vec::new());
static NEXT_RESERVATION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Longest a start takes, after which a reservation that nothing settled is dropped.
const RESERVATION_LIFE: std::time::Duration = std::time::Duration::from_secs(300);

/// Longest a reservation tied to a job is kept, counted from when the job starts running, or
/// from the reservation while it still waits for its turn: a job whose worker died without
/// finishing it would otherwise hold its memory until the engine restarts. It outlasts a
/// download of a macOS restore image on a slow connection.
const JOB_RESERVATION_CEILING: std::time::Duration = std::time::Duration::from_secs(2 * 60 * 60);

/// Whether a reservation is still wanted. One tied to a job lasts until the job ends, the
/// computer it is for is resident in a list loaded after the reservation was made, or
/// [`JOB_RESERVATION_CEILING`]; one nothing settled lasts [`RESERVATION_LIFE`].
fn reservation_alive(reservation: &Reservation, over: &[u64], resident: &Residency) -> bool {
    if over.contains(&reservation.id)
        || reservation.boots.as_ref().is_some_and(|computer| {
            resident.loaded_at >= reservation.created_at && resident.contains(computer)
        })
    {
        return false;
    }
    if reservation.job_id.is_none() {
        return reservation.created_at.elapsed() <= RESERVATION_LIFE;
    }
    reservation
        .running_since
        .unwrap_or(reservation.created_at)
        .elapsed()
        <= JOB_RESERVATION_CEILING
}

/// Where the job registry has a job.
#[derive(Clone, Copy, PartialEq, Eq)]
enum JobPhase {
    Over,
    Waiting,
    Running,
}

/// Where the job registry has `job`. A registry that cannot be read says nothing about the
/// job, so the job is taken to run.
fn job_phase(engine: &dyn EngineContext, job: &str) -> JobPhase {
    let Ok(job) = JobId::new(job) else {
        return JobPhase::Over;
    };
    let Ok(registry) = engine.job_registry() else {
        return JobPhase::Running;
    };
    match registry.state_of(&job) {
        Ok(None | Some(ApiJobState::Finished)) => JobPhase::Over,
        Ok(Some(ApiJobState::Waiting)) => JobPhase::Waiting,
        _ => JobPhase::Running,
    }
}

/// Drops the reservations whose job is over, whose computer is resident, or that outlived their
/// life, and starts the ceiling of those whose job has begun to run. The job registry is read
/// with no lock held; a reservation whose job ends meanwhile is dropped by the next call.
fn prune_reservations(engine: &dyn EngineContext, resident: &Residency) {
    let jobs: Vec<(u64, String)> = RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter_map(|r| r.job_id.clone().map(|job| (r.id, job)))
        .collect();
    let phases: Vec<(u64, JobPhase)> = jobs
        .into_iter()
        .map(|(id, job)| (id, job_phase(engine, &job)))
        .collect();
    let over: Vec<u64> = phases
        .iter()
        .filter(|(_, phase)| *phase == JobPhase::Over)
        .map(|(id, _)| *id)
        .collect();
    let now = std::time::Instant::now();
    let mut reservations = RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for r in reservations.iter_mut() {
        if r.running_since.is_none()
            && phases
                .iter()
                .any(|(id, phase)| *id == r.id && *phase == JobPhase::Running)
        {
            r.running_since = Some(now);
        }
    }
    reservations.retain(|r| reservation_alive(r, &over, resident));
}

fn current_reserved_memory(engine: &dyn EngineContext, resident: &Residency) -> u64 {
    prune_reservations(engine, resident);
    RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|r| r.memory_bytes)
        .sum()
}

/// Reserves `memory_bytes` when the device has that much above its floor after the memory
/// already reserved. The check and the reservation are one step under one lock, so calls made
/// at the same moment cannot all pass on the same headroom. `boots` is the computer the memory
/// is for when it is not resident yet. `Ok(None)` when nothing is to be reserved.
fn reserve_if_fits(
    engine: &dyn EngineContext,
    resident: &Residency,
    host: &resources::Host,
    floors: &resources::Floors,
    computer_id: Option<String>,
    boots: Option<String>,
    memory_bytes: u64,
) -> Result<Option<u64>, String> {
    prune_reservations(engine, resident);
    let mut reservations = RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reservations.retain(|r| reservation_alive(r, &[], resident));
    let reserved: u64 = reservations.iter().map(|r| r.memory_bytes).sum();
    if let Some(refusal) = resources::refusal_for(host, floors, reserved, memory_bytes) {
        return Err(refusal);
    }
    if memory_bytes == 0 {
        return Ok(None);
    }
    let id = NEXT_RESERVATION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    reservations.push(Reservation {
        id,
        computer_id,
        boots,
        job_id: None,
        memory_bytes,
        created_at: std::time::Instant::now(),
        running_since: None,
    });
    Ok(Some(id))
}

/// Ties the reservation `id` to the job it started. `made` is the computer the call created,
/// which a call that names no computer beforehand learns only from its answer.
fn attach_reservation_job(id: u64, job_id: &str, made: Option<&str>) {
    let mut reservations = RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(r) = reservations.iter_mut().find(|r| r.id == id) {
        r.job_id = Some(job_id.to_owned());
        if r.boots.is_none() {
            r.boots = made.map(str::to_owned);
        }
    }
}

/// The id of the computer a call's answer says it created.
pub(crate) fn computer_made(result: &Result<HandlerOutput, DispatchError>) -> Option<String> {
    result
        .as_ref()
        .ok()?
        .value
        .pointer("/computer/id")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn release_reservation(id: u64) {
    let mut reservations = RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reservations.retain(|r| r.id != id);
}

/// What is reserved for a call: memory, and for a call that starts a job a place among the jobs
/// its computer may hold. Both are given back when this goes out of scope, as when the call is
/// refused, fails, panics or its caller goes away, unless [`Held::settle`] tied the memory to the
/// job the call started and [`Held::settle_slot`] kept the place for a job that was not recorded.
pub(crate) struct Held {
    id: Option<u64>,
    slot: Option<u64>,
    settled: std::sync::atomic::AtomicBool,
    slot_settled: std::sync::atomic::AtomicBool,
}

impl Held {
    fn of(id: Option<u64>) -> Self {
        Self {
            id,
            slot: None,
            settled: std::sync::atomic::AtomicBool::new(false),
            slot_settled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Ties the reservation to the job the call started, or gives it back when it started none.
    /// `made` is the computer the call created, if its answer names one.
    pub(crate) fn settle(&self, job: Option<&str>, made: Option<&str>) {
        if self.settled.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        match (self.id, job) {
            (Some(id), Some(job)) => attach_reservation_job(id, job, made),
            (Some(id), None) => release_reservation(id),
            (None, _) => {}
        }
    }

    /// Gives back the place reserved for a job once the record holds the job, or the call
    /// started none. A job the record could not take keeps its place under the job's name.
    fn settle_slot(&self, job: Option<&str>, recorded: bool) {
        if self
            .slot_settled
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        match (self.slot, job) {
            (Some(slot), Some(job)) if !recorded => attach_job_slot(slot, job),
            (Some(slot), _) => release_job_slot(slot),
            (None, _) => {}
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        if !*self.settled.get_mut() {
            if let Some(id) = self.id {
                release_reservation(id);
            }
        }
        if !*self.slot_settled.get_mut() {
            if let Some(slot) = self.slot {
                release_job_slot(slot);
            }
        }
    }
}

/// A device a test fixes the numbers of, held by one test at a time because the reservations
/// are shared by every test in the process.
#[cfg(test)]
pub(crate) struct FixedDevice(resources::FixedHost, std::sync::MutexGuard<'static, ()>);

#[cfg(test)]
pub(crate) fn fixed_device(host: resources::Host) -> FixedDevice {
    FixedDevice(resources::FixedHost::new(host), test_exclusive())
}

#[cfg(test)]
pub(crate) fn test_exclusive() -> std::sync::MutexGuard<'static, ()> {
    static EXCLUSIVE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    EXCLUSIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Makes the reservation `id` look `age` old.
#[cfg(test)]
pub(crate) fn backdate_reservation(id: u64, age: std::time::Duration) {
    let mut reservations = RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(r) = reservations.iter_mut().find(|r| r.id == id) {
        r.created_at = std::time::Instant::now()
            .checked_sub(age)
            .expect("the machine has been up for longer than the age");
    }
}

/// Makes the job of the reservation `id` look to have been running for `age`.
#[cfg(test)]
pub(crate) fn backdate_running(id: u64, age: std::time::Duration) {
    let mut reservations = RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(r) = reservations.iter_mut().find(|r| r.id == id) {
        r.running_since = Some(
            std::time::Instant::now()
                .checked_sub(age)
                .expect("the machine has been up for longer than the age"),
        );
    }
}

#[cfg(test)]
pub(crate) fn clear_reservations() {
    let mut reservations = RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reservations.clear();
}

/// What the gate reads about the computers before it sizes a call.
#[derive(Clone, Default)]
struct Sizes {
    /// Each computer's saved sizes.
    resources: HashMap<String, Resources>,
    /// The memory, in MiB, a computer with a full checkpoint restore pending resumes at, or why
    /// that could not be read.
    resumes: HashMap<String, Result<u64, String>>,
    /// The computer each computer was forked from.
    parents: HashMap<String, Option<String>>,
    /// The computers that hold their memory on the device now, so the device's own measurement
    /// of free memory already counts it.
    resident: Residency,
}

const MIB: u64 = 1024 * 1024;

/// The memory a call takes when it runs, from the parameters its handler will decode and, for
/// a call on an existing computer, from that computer's stored size, or for one that resumes a
/// full checkpoint, from the memory the checkpoint recorded. A field the handler ignores never
/// lowers it. A call that does not decode takes nothing, since its handler refuses it. The
/// error says why no size could be found for a computer the call names. `swapped` is every
/// computer an undo may put back, found by the caller.
fn requested_memory_bytes(
    method: MethodName,
    params: &Value,
    sizes: &Sizes,
    swapped: &[String],
) -> Result<u64, String> {
    use silo_api::methods::{
        checkpoints::{CheckpointCreateParams, CheckpointForkParams, CheckpointTargetParams},
        common::ComputerTargetParams,
        computers::ComputerCreateParams,
        keep::ChangesPromoteParams,
    };
    fn decode<T: serde::de::DeserializeOwned>(params: &Value) -> Option<T> {
        serde_json::from_value(params.clone()).ok()
    }
    let unreadable = || {
        "Silo can't read this computer's size, so it cannot tell whether it fits. Try again, or ask the person who runs Silo."
            .to_owned()
    };
    // The memory a Linux computer takes is the size it boots with: nothing in Silo raises a
    // running computer's memory toward its ceiling, and a change of ceiling needs it stopped.
    let stored = |id: &str| {
        sizes
            .resources
            .get(id)
            .map(|sizes| sizes.memory_gib.saturating_mul(resources::GIB))
            .ok_or_else(unreadable)
    };
    let sized = |id: Option<String>| id.map_or(Ok(0), |id| stored(&id));
    // A computer with a full checkpoint restore pending boots at the memory the capture holds. One
    // that holds its memory now needs only what the boot takes beyond that, normally nothing.
    let started = |id: Option<String>| match id {
        Some(id) => {
            let boots = match sizes.resumes.get(&id) {
                Some(resume) => resume.clone().map(|mib| mib.saturating_mul(MIB))?,
                None => stored(&id)?,
            };
            if sizes.resident.contains(&id) {
                Ok(boots.saturating_sub(stored(&id)?))
            } else {
                Ok(boots)
            }
        }
        None => Ok(0),
    };
    let all = |ids: Vec<String>| {
        ids.iter().try_fold(0_u64, |sum, id| {
            Ok::<u64, String>(sum.saturating_add(stored(id)?))
        })
    };
    match method {
        MethodName::ComputersCreate => Ok(match decode::<ComputerCreateParams>(params) {
            Some(ComputerCreateParams::Linux { configuration, .. }) => {
                u64::from(configuration.memory_gib).saturating_mul(resources::GIB)
            }
            Some(ComputerCreateParams::Macos { request, .. }) => {
                request.memory_gib.saturating_mul(resources::GIB)
            }
            None => 0,
        }),
        MethodName::ComputersStart | MethodName::ComputersRestart => {
            started(decode::<ComputerTargetParams>(params).map(|target| target.computer_id))
        }
        MethodName::MacosSetupRetry => {
            sized(decode::<ComputerTargetParams>(params).map(|target| target.computer_id))
        }
        // A capture of a computer that already holds its memory takes no more.
        MethodName::CheckpointsCreate => {
            match decode::<CheckpointCreateParams>(params).map(|target| target.computer_id) {
                Some(id) if sizes.resident.contains(&id) => Ok(0),
                id => sized(id),
            }
        }
        MethodName::CheckpointsRestore => {
            started(decode::<CheckpointTargetParams>(params).map(|target| target.computer_id))
        }
        // A fork boots at the size its source has now; a checkpoint records none.
        MethodName::CheckpointsFork => {
            sized(decode::<CheckpointForkParams>(params).map(|source| source.computer_id))
        }
        // An undo resumes full checkpoints and may restart the other computer of a promote.
        MethodName::ChangesUndo => swapped.iter().try_fold(0_u64, |sum, id| {
            Ok(sum.saturating_add(started(Some(id.clone()))?))
        }),
        // A promote restarts the fork and the computer it replaces; a dry run starts nothing.
        MethodName::ChangesPromote => Ok(match decode::<ChangesPromoteParams>(params) {
            Some(promote) if promote.dry_run != Some(true) => {
                let target = promote
                    .target_id
                    .clone()
                    .or_else(|| sizes.parents.get(&promote.fork_id).cloned().flatten());
                let mut ids = vec![promote.fork_id];
                ids.extend(target);
                all(ids)?
            }
            _ => 0,
        }),
        _ => Ok(0),
    }
}

/// The computer a call boots whose memory the device's measurement does not hold yet, when the
/// call names it beforehand. A macOS computer a call creates is named by its answer.
fn booted_computer(method: MethodName, params: &Value, sizes: &Sizes) -> Option<String> {
    use silo_api::methods::{common::ComputerTargetParams, computers::ComputerCreateParams};
    let id = match method {
        MethodName::ComputersStart | MethodName::ComputersRestart | MethodName::MacosSetupRetry => {
            serde_json::from_value::<ComputerTargetParams>(params.clone())
                .ok()?
                .computer_id
        }
        MethodName::ComputersCreate => {
            match serde_json::from_value::<ComputerCreateParams>(params.clone()).ok()? {
                ComputerCreateParams::Linux { configuration, .. } => configuration.id,
                ComputerCreateParams::Macos { .. } => return None,
            }
        }
        _ => return None,
    };
    (!sizes.resident.contains(&id)).then_some(id)
}

/// Checks a call that takes memory or disk against the device and, when `reserve_for` names
/// who it is for, reserves the memory it needs in the same step. The error is the reason the
/// call must fail now: the device has less free than a floor, the memory exceeds the headroom
/// left, or the device cannot be measured so nobody can tell.
fn assess_headroom(
    context: &HandlerContext,
    method: MethodName,
    params: &Value,
    sizes: &Sizes,
    reserve_for: Option<Option<String>>,
) -> Result<Held, String> {
    let Ok((host, floors)) = host_and_floors(context) else {
        return Err(
            "Silo cannot measure this device's memory and disk, so it cannot tell whether another computer fits. Ask the person who runs Silo."
                .to_owned(),
        );
    };
    let swapped = match method {
        MethodName::ChangesUndo => {
            serde_json::from_value::<silo_api::methods::keep::ChangesUndoParams>(params.clone())
                .map(|undo| super::guest_client::swap_participants(context, &undo.computer_id))
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };
    let requested = match requested_memory_bytes(method, params, sizes, &swapped) {
        Ok(requested) => requested,
        Err(unreadable) => {
            // A device short of its floors is told so before anything else.
            let reserved = current_reserved_memory(context.engine_context(), &sizes.resident);
            return Err(resources::refusal_for(&host, &floors, reserved, 0).unwrap_or(unreadable));
        }
    };
    match reserve_for {
        Some(computer) => reserve_if_fits(
            context.engine_context(),
            &sizes.resident,
            &host,
            &floors,
            computer,
            booted_computer(method, params, sizes),
            requested,
        )
        .map(Held::of),
        None => {
            let reserved = current_reserved_memory(context.engine_context(), &sizes.resident);
            match resources::refusal_for(&host, &floors, reserved, requested) {
                Some(refusal) => Err(refusal),
                None => Ok(Held::of(None)),
            }
        }
    }
}

/// Checks a call about to run, with the parameters it will run with, against the device and
/// reserves its memory in the same step, reading the computers' stored sizes now. The error is
/// why it must not run.
pub(crate) async fn reserve_now(
    context: &HandlerContext,
    method: MethodName,
    params: &Value,
) -> Result<Held, String> {
    let world = EngineWorld::load(context, method, None).await;
    assess_headroom(
        context,
        method,
        params,
        &sizes_of(context, method, params, &world).await,
        Some(policy::target_computer(params)),
    )
}

/// The memory reserved for calls about `computer`.
#[cfg(test)]
pub(crate) fn reserved_memory_of(computer: &str) -> u64 {
    RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|r| r.computer_id.as_deref() == Some(computer))
        .map(|r| r.memory_bytes)
        .sum()
}

/// The memory reserved for all calls now.
#[cfg(test)]
pub(crate) fn reserved_memory() -> u64 {
    RESERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|r| r.memory_bytes)
        .sum()
}

/// Every computer's open jobs and kept results together that computers may hold in the job
/// registry. The owner's jobs are never counted against it.
const MAX_COMPUTER_JOBS: usize = 512;

/// Whether the job registry has let go of `job`: it holds no result or record of it any more.
/// A registry that cannot be read has not let go of anything. Only the job's state is read, so
/// no result is loaded.
fn released(engine: &dyn EngineContext, job: &str) -> bool {
    let Ok(job) = JobId::new(job) else {
        return true;
    };
    engine
        .job_registry()
        .is_ok_and(|registry| matches!(registry.state_of(&job), Ok(None)))
}

/// The record of the jobs computers hold, with every job the registry has released forgotten,
/// open or kept. A result that was kept before the engine last started is kept by the registry
/// again, which gives it a deadline, so the charge it carries clears when the result does.
fn current_charges(
    engine: &dyn EngineContext,
    store: &PolicyStore,
    mut document: Document,
) -> Document {
    let gone: Vec<String> = document
        .jobs
        .iter()
        .chain(&document.kept)
        .filter(|job| released(engine, &job.id))
        .map(|job| job.id.clone())
        .collect();
    if !gone.is_empty() {
        if let Err(error) = store.release_jobs(&gone) {
            eprintln!("silod: {error}");
        }
        document.jobs.retain(|job| !gone.contains(&job.id));
        document.kept.retain(|job| !gone.contains(&job.id));
    }
    if let Ok(registry) = engine.job_registry() {
        for job in &document.kept {
            let Ok(id) = JobId::new(&job.id) else {
                continue;
            };
            if let Err(error) = registry.restore_hold(&id, std::slice::from_ref(&job.caller)) {
                eprintln!("silod: {}", error.message());
            }
        }
    }
    document
}

/// Moves the job `computer` acknowledged from its open jobs to the results kept for the owner,
/// or drops it when the registry keeps no result of it.
fn keep_charging(
    engine: &dyn EngineContext,
    store: &PolicyStore,
    computer: &str,
    job: &str,
) -> Result<(), String> {
    if released(engine, job) {
        store.drop_job(computer, job)
    } else {
        store.keep_job(computer, job)
    }
}

/// Why a computer that has `open` unacknowledged jobs and `kept` acknowledged jobs whose
/// results are still kept may not start another, when all computers together hold `all` jobs.
fn job_refusal(open: usize, kept: usize, all: usize) -> Option<String> {
    if open + kept >= silo_engine::guest_client::MAX_JOBS {
        return Some(format!(
            "{} jobs of this computer are not finished and acknowledged yet, or their results are still kept for the person who runs Silo.",
            silo_engine::guest_client::MAX_JOBS
        ));
    }
    (all >= MAX_COMPUTER_JOBS).then(|| {
        "Computers together hold as many jobs as Silo allows them. Try again when some are over."
            .to_owned()
    })
}

/// A place among the jobs a computer may hold, taken by a call until the record holds its job.
#[derive(Debug)]
struct JobSlot {
    id: u64,
    caller: String,
    job_id: Option<String>,
    created_at: std::time::Instant,
}

static JOB_SLOTS: std::sync::Mutex<Vec<JobSlot>> = std::sync::Mutex::new(Vec::new());
static NEXT_JOB_SLOT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Takes a place for a job `caller` is about to start, when it and all computers together hold
/// fewer jobs than they may, counting the places other calls hold. The check and the place are
/// one step under one lock, so calls made at the same moment cannot all pass on the same count.
fn reserve_job_slot(
    engine: &dyn EngineContext,
    store: &PolicyStore,
    caller: &str,
) -> Result<u64, String> {
    let mut slots = JOB_SLOTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let document = store
        .snapshot()
        .map(|document| current_charges(engine, store, document))
        .map_err(|_| "Silo cannot read this computer's jobs right now.".to_owned())?;
    // A place whose job the record holds now is counted there, and one nothing settled in the
    // longest a call takes is dropped.
    slots.retain(|slot| {
        slot.created_at.elapsed() <= RESERVATION_LIFE
            && slot
                .job_id
                .as_deref()
                .is_none_or(|job| !document.owns_job(&slot.caller, job))
    });
    let held = slots.iter().filter(|slot| slot.caller == caller).count();
    let kept = document
        .kept
        .iter()
        .filter(|job| job.caller == caller)
        .count();
    if let Some(refusal) = job_refusal(
        document.jobs_of(caller).len() + held,
        kept,
        document.jobs.len() + document.kept.len() + slots.len(),
    ) {
        return Err(refusal);
    }
    let id = NEXT_JOB_SLOT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    slots.push(JobSlot {
        id,
        caller: caller.to_owned(),
        job_id: None,
        created_at: std::time::Instant::now(),
    });
    Ok(id)
}

fn attach_job_slot(id: u64, job_id: &str) {
    let mut slots = JOB_SLOTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(slot) = slots.iter_mut().find(|slot| slot.id == id) {
        slot.job_id = Some(job_id.to_owned());
    }
}

fn release_job_slot(id: u64) {
    JOB_SLOTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|slot| slot.id != id);
}

/// The places held for the jobs `caller` is about to start.
#[cfg(test)]
pub(crate) fn job_slots_of(caller: &str) -> usize {
    JOB_SLOTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|slot| slot.caller == caller)
        .count()
}

/// Asks the policy again about a call `computer` made, as at the time it asked, and returns
/// its answer with the computers `computer` may reach (itself and the ones it made). `None`
/// when that computer no longer exists.
pub(crate) async fn judge(
    context: &HandlerContext,
    computer: &str,
    method: MethodName,
    params: &Value,
) -> Option<(Decision, BTreeSet<String>)> {
    let world = EngineWorld::load(context, method, Some((computer, params))).await;
    policy::World::name(&world, computer)?;
    let caller = Caller::Computer {
        id: computer.to_owned(),
    };
    let mut tree: BTreeSet<String> = policy::World::made(&world, computer).into_iter().collect();
    tree.insert(computer.to_owned());
    Some((policy::decide(&caller, method, params, &world), tree))
}

/// Asks the policy about one call. A refused or held call is recorded and answered here.
pub(crate) async fn admit(
    context: &HandlerContext,
    method: MethodName,
    params: &Value,
) -> Result<Admitted, DispatchError> {
    let caller = context.client_identity().caller.clone();
    let target = policy::target_computer(params);
    let (decision, visible, name, sizes) = match &caller {
        Caller::Computer { id } => {
            let world = EngineWorld::load(context, method, Some((id, params))).await;
            let mut visible: BTreeSet<String> =
                policy::World::made(&world, id).into_iter().collect();
            visible.insert(id.clone());
            (
                policy::decide(&caller, method, params, &world),
                Some(Arc::new(visible)),
                policy::World::name(&world, id),
                sizes_of(context, method, params, &world).await,
            )
        }
        _ => (
            policy::decide(&caller, method, params, &NoWorld),
            None,
            None,
            Sizes::default(),
        ),
    };
    let seen = visible.clone();
    let refuse = |error: DispatchError, outcome: AuditOutcome| {
        audit(
            context,
            Record {
                method,
                target: target.clone(),
                visible: seen.clone(),
                outcome,
                error: Some(&error),
                job_id: None,
                approval_id: None,
            },
        );
        Err(error)
    };
    // A call that would be held for approval is refused too: allowing it later would start a
    // computer on a device that has nothing left for it. An allowed call reserves its memory in
    // the same step as the check, and gives it back if it is refused further on.
    let mut held = Held::of(None);
    if matches!(caller, Caller::Computer { .. })
        && takes_headroom(method)
        && matches!(decision, Decision::Allow(_) | Decision::Ask { .. })
    {
        let sized = match &decision {
            Decision::Allow(allowed) => allowed.params.as_ref().unwrap_or(params),
            Decision::Ask {
                params: Some(rewritten),
                ..
            } => rewritten,
            _ => params,
        };
        let reserve_for = matches!(decision, Decision::Allow(_)).then(|| target.clone());
        match assess_headroom(context, method, sized, &sizes, reserve_for) {
            Ok(reservation) => held = reservation,
            Err(message) => {
                return refuse(
                    DispatchError::new(ErrorCode::ResourcesLow, message),
                    AuditOutcome::Refused,
                );
            }
        }
    }
    // A computer may have only so many jobs it has not acknowledged, counting the results of
    // acknowledged ones that the engine still keeps for the owner: a job nobody follows still
    // takes a place in the engine's job admission. The place is taken now, so calls made at the
    // same moment cannot all pass on the same count.
    if let Caller::Computer { id } = &caller {
        if kind_of(method) == MethodKind::Job && matches!(decision, Decision::Allow(_)) {
            let store = PolicyStore::new(&app_data(context));
            match reserve_job_slot(context.engine_context(), &store, id) {
                Ok(slot) => held.slot = Some(slot),
                Err(message) => {
                    return refuse(
                        DispatchError::new(ErrorCode::PermissionDenied, message),
                        AuditOutcome::Refused,
                    );
                }
            }
        }
    }
    match decision {
        Decision::Allow(allowed) => {
            if let (Some(creation), Caller::Computer { id }) = (&allowed.creates, &caller) {
                // The computer is the caller's once its job has finished well (`settle`).
                let begun = begin_creation(&app_data(context), id, creation);
                if let Err(message) = begun {
                    return refuse(
                        DispatchError::new(ErrorCode::PermissionDenied, message),
                        AuditOutcome::Refused,
                    );
                }
            }
            let job = (method == MethodName::OperationsJobAcknowledge)
                .then(|| {
                    params
                        .get("jobId")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .flatten();
            Ok(Admitted {
                params: allowed.params,
                filter: allowed.filter,
                visible: visible.or(allowed.visible.map(Arc::new)),
                creates: allowed.creates,
                target,
                kind: kind_of(method),
                job,
                held,
            })
        }
        Decision::Refuse { reason } => refuse(
            DispatchError::new(ErrorCode::PermissionDenied, reason),
            AuditOutcome::Refused,
        ),
        Decision::Ask {
            kind,
            summary,
            computer_summary,
            params: rewritten,
            creates,
        } => {
            let Caller::Computer { id } = &caller else {
                return refuse(
                    DispatchError::new(ErrorCode::PermissionDenied, summary),
                    AuditOutcome::Refused,
                );
            };
            let asked = super::approvals::Asked {
                computer: id,
                tree: visible.as_deref().cloned().unwrap_or_default(),
                computer_name: name.unwrap_or_else(|| id.clone()),
                method,
                asked: params,
                params: rewritten.unwrap_or_else(|| params.clone()),
                kind,
                summary,
                computer_summary,
                creates,
            };
            match super::approvals::request(context, asked).await {
                Ok(error) => refuse(error, AuditOutcome::Asked),
                Err(error) => refuse(error, AuditOutcome::Refused),
            }
        }
    }
}

/// Completes an admitted call: records what a computer's agent started, cuts the answer for a
/// computer, and writes the audit row of a call that can change something.
pub(crate) fn finish(
    context: &HandlerContext,
    method: MethodName,
    admitted: &Admitted,
    mut result: Result<HandlerOutput, DispatchError>,
) -> Result<HandlerOutput, DispatchError> {
    let job = result
        .as_ref()
        .ok()
        .and_then(|output| output.value.get("jobId"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    admitted
        .held
        .settle(job.as_deref(), computer_made(&result).as_deref());
    if let Caller::Computer { id } = &context.client_identity().caller {
        let store = PolicyStore::new(&app_data(context));
        let mut noted = Vec::new();
        if let Some(creation) = &admitted.creates {
            noted.push(settle_creation(
                &store,
                id,
                creation,
                &result,
                job.as_deref(),
            ));
        }
        let mut recorded = true;
        if let Some(job) = &job {
            let note = store.note_job_about(id, job, admitted.target.as_deref());
            recorded = note.is_ok();
            noted.push(note);
        }
        admitted.held.settle_slot(job.as_deref(), recorded);
        if let (Ok(_), Some(done)) = (&result, &admitted.job) {
            noted.push(keep_charging(context.engine_context(), &store, id, done));
        }
        for error in noted.into_iter().filter_map(Result::err) {
            eprintln!("silod: computer {id}: {error}");
        }
        if let (Ok(output), Some(visible)) = (&mut result, &admitted.visible) {
            policy::filter_result(admitted.filter, &mut output.value, visible);
            policy::cut_for_computer(&mut output.value, visible);
        }
    }
    if matches!(
        admitted.kind,
        MethodKind::Command | MethodKind::Job | MethodKind::Stream
    ) {
        let (outcome, error) = match &result {
            Ok(_) => (AuditOutcome::Ok, None),
            Err(error) => (AuditOutcome::Error, Some(error)),
        };
        audit(
            context,
            Record {
                method,
                target: admitted.target.clone(),
                visible: admitted.visible.clone(),
                outcome,
                error,
                job_id: job,
                approval_id: None,
            },
        );
    }
    result
}

/// The policy's view for a caller it never asks about the device.
struct NoWorld;

impl policy::World for NoWorld {
    fn made(&self, _creator: &str) -> std::collections::HashSet<String> {
        Default::default()
    }
    fn parent(&self, _computer: &str) -> Option<String> {
        None
    }
    fn name(&self, _computer: &str) -> Option<String> {
        None
    }
    fn owns_job(&self, _caller: &str, _job_id: &str) -> bool {
        false
    }
    fn request_fork(&self, _request_id: &str) -> Option<String> {
        None
    }
    fn request_target(&self, _request_id: &str) -> Option<String> {
        None
    }
    fn resources(&self, _computer: &str) -> Option<Resources> {
        None
    }
    fn ssh_settings(&self, _computer: &str) -> Option<(u16, String)> {
        None
    }
    fn access(&self, _computer: &str) -> Access {
        Access::default()
    }
    fn github_policies(&self) -> Option<Vec<Value>> {
        None
    }
    fn new_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

/// The device as the engine sees it now: its computers, who made which, and the jobs
/// computers started.
struct EngineWorld {
    names: HashMap<String, String>,
    parents: HashMap<String, Option<String>>,
    document: Document,
    app_data: PathBuf,
    github: Option<Vec<Value>>,
    /// Every computer's sizes, read only for a call that makes a computer.
    resources: HashMap<String, Resources>,
    /// The memory a pending full checkpoint restore resumes each computer at, read only for a
    /// call that starts a computer or undoes a change.
    resumes: HashMap<String, Result<u64, String>>,
    /// The computers that hold their memory on the device now, read only for a call that
    /// makes a computer.
    resident: Residency,
    /// The saved SSH port and bind address of each computer, read only for `sshAccess.save`.
    ssh: HashMap<String, (u16, String)>,
    /// The saved SSH public keys of each computer, read with the above.
    ssh_keys: HashMap<String, Vec<String>>,
    /// The network policy of the caller and of every computer the call names, read only for a
    /// call that compares network policies. A computer whose policy could not be read is absent.
    network: HashMap<String, silo_engine::network_policy::Held>,
}

impl EngineWorld {
    async fn load(
        context: &HandlerContext,
        method: MethodName,
        call: Option<(&str, &Value)>,
    ) -> Self {
        let internal = HandlerContext::for_owner_bridge(
            context.engine.clone(),
            RequestContext::owner("policy"),
        );
        let tree = super::route(&internal, MethodName::ComputersTree, &json!({}))
            .await
            .map(|output| output.value)
            .unwrap_or(Value::Null);
        let mut names = HashMap::new();
        let mut parents = HashMap::new();
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
                names.insert(id.to_owned(), name.to_owned());
            }
            parents.insert(
                id.to_owned(),
                node.get("parent")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            );
        }
        #[cfg(test)]
        for (id, name, parent) in test_nodes::all() {
            names.insert(id.clone(), name);
            parents.insert(id, parent);
        }
        let github = if method == MethodName::GitHubConfigurationSave {
            super::route(&internal, MethodName::GitHubState, &json!({}))
                .await
                .ok()
                .and_then(|output| output.value.get("computers").cloned())
                .and_then(|computers| computers.as_array().cloned())
        } else {
            None
        };
        let mut resources = HashMap::new();
        let mut resumes = HashMap::new();
        let mut resident = HashSet::new();
        let resident_loaded_at = std::time::Instant::now();
        if method == MethodName::ComputersCreate || takes_headroom(method) {
            let list = super::route(&internal, MethodName::ComputersList, &json!({}))
                .await
                .map(|output| output.value)
                .unwrap_or(Value::Null);
            let computers = |kind: &str| {
                list.pointer(&format!("/{kind}/computers"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
            };
            for computer in computers("linux") {
                let configuration = &computer["configuration"];
                let id = configuration.get("id").and_then(Value::as_str);
                if let (Some(id), Some(sizes)) = (id, Resources::of_linux(configuration)) {
                    resources.insert(id.to_owned(), sizes);
                }
                if let (Some(id), Some("running")) = (id, computer["state"].as_str()) {
                    resident.insert(id.to_owned());
                }
            }
            for computer in computers("macos") {
                let id = computer.get("id").and_then(Value::as_str);
                if let (Some(id), Some(sizes)) = (id, Resources::of_macos(&computer)) {
                    resources.insert(id.to_owned(), sizes);
                }
                // The installer and the setup run the computer, so they hold its memory too.
                if let (Some(id), Some("installing" | "setting-up" | "running")) =
                    (id, computer["state"].as_str())
                {
                    resident.insert(id.to_owned());
                }
            }
            if matches!(
                method,
                MethodName::ComputersStart | MethodName::ComputersRestart | MethodName::ChangesUndo
            ) {
                let waiting: Vec<(String, String)> = list
                    .pointer("/linux/computers")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|computer| {
                        computer.pointer("/pendingCheckpointRestore/state") == Some(&json!("full"))
                    })
                    .filter_map(|computer| {
                        let configuration = &computer["configuration"];
                        Some((
                            configuration.get("id")?.as_str()?.to_owned(),
                            configuration.get("name")?.as_str()?.to_owned(),
                        ))
                    })
                    .collect();
                if !waiting.is_empty() {
                    let host = context.silod_context().runtime_host();
                    resumes = tokio::task::spawn_blocking(move || {
                        let paths = silo_engine::runtime::runtime_paths(host.as_ref());
                        waiting
                            .into_iter()
                            .filter_map(|(id, name)| {
                                let resume = paths.clone().and_then(|paths| {
                                    silo_engine::runtime::pending_resume_memory_mib(
                                        &paths, &id, &name,
                                    )
                                });
                                resume.transpose().map(|resume| (id, resume))
                            })
                            .collect::<HashMap<_, _>>()
                    })
                    .await
                    .unwrap_or_default();
                }
            }
        }
        #[cfg(test)]
        for (id, r) in test_resources::all() {
            resources.insert(id, r);
        }
        #[cfg(test)]
        for (id, resume) in test_resumes::all() {
            resumes.insert(id, resume);
        }
        #[cfg(test)]
        resident.extend(test_resident::all());
        let mut ssh = HashMap::new();
        let mut ssh_keys = HashMap::new();
        if method == MethodName::SshAccessSave {
            let state = super::route(&internal, MethodName::SshAccessState, &json!({}))
                .await
                .map(|output| output.value)
                .unwrap_or(Value::Null);
            for computer in state
                .get("computers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let saved = (
                    computer
                        .get("port")
                        .and_then(Value::as_u64)
                        .and_then(|port| u16::try_from(port).ok()),
                    computer.get("bindAddress").and_then(Value::as_str),
                    computer.get("computerId").and_then(Value::as_str),
                );
                if let (Some(port), Some(address), Some(id)) = saved {
                    ssh.insert(id.to_owned(), (port, address.to_owned()));
                    let keys = computer
                        .get("keys")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect();
                    ssh_keys.insert(id.to_owned(), keys);
                }
            }
        }
        let mut network = HashMap::new();
        if let (true, Some((caller, params))) = (compares_networks(method), call) {
            let mut ids = vec![caller.to_owned()];
            ids.extend(
                params
                    .as_object()
                    .into_iter()
                    .flat_map(|object| object.values())
                    .filter_map(Value::as_str)
                    .filter(|value| names.contains_key(*value))
                    .map(str::to_owned),
            );
            ids.sort();
            ids.dedup();
            network = tokio::task::spawn_blocking(move || {
                ids.into_iter()
                    .filter_map(|id| {
                        super::access::judged_network_policy(&id).map(|held| (id, held))
                    })
                    .collect::<HashMap<_, _>>()
            })
            .await
            .unwrap_or_default();
        }
        let app_data = app_data(context);
        let engine = context.engine.clone();
        let settled = {
            let (names, parents, app_data) = (names.clone(), parents.clone(), app_data.clone());
            tokio::task::spawn_blocking(move || {
                let job = |job: &str| job_state(engine.as_ref(), job);
                let resolve = |pending: &Pending| resolve(&names, &parents, pending);
                PolicyStore::new(&app_data).settle(&job, &resolve)
            })
            .await
        };
        let document = match settled {
            Ok(Ok(document)) => document,
            Ok(Err(error)) => {
                eprintln!("silod: {error}");
                Document::default()
            }
            Err(_) => Document::default(),
        };
        Self {
            names,
            parents,
            document,
            app_data,
            github,
            resources,
            resumes,
            resident: Residency {
                ids: resident,
                loaded_at: resident_loaded_at,
            },
            ssh,
            ssh_keys,
            network,
        }
    }
}

/// Whether a call's decision compares network policies.
fn compares_networks(method: MethodName) -> bool {
    matches!(
        method,
        MethodName::ComputersCreate | MethodName::CheckpointsFork | MethodName::NetworkPolicySet
    )
}

fn job_state(engine: &dyn EngineContext, job: &str) -> JobState {
    let Ok(job) = JobId::new(job) else {
        return JobState::Unknown;
    };
    match silo_engine::jobs::get_at(engine, &job, None) {
        Ok(Some(found)) if found.state == ApiJobState::Finished => {
            let failed = found
                .outcome
                .as_ref()
                .is_some_and(|outcome| outcome.error.is_some());
            if failed {
                JobState::Failed
            } else {
                JobState::Succeeded
            }
        }
        Ok(Some(_)) => JobState::Running,
        Ok(None) | Err(_) => JobState::Unknown,
    }
}

/// The id of a computer that was pending: the one the engine chose, or the computer of that
/// name whose parent is the fork's source. A name alone proves nothing, since another
/// computer may already have it, so a pending computer with neither is never matched; the
/// call that made it answers with its id.
fn resolve(
    names: &HashMap<String, String>,
    parents: &HashMap<String, Option<String>>,
    pending: &Pending,
) -> Option<String> {
    if let Some(id) = &pending.id {
        return names.contains_key(id).then(|| id.clone());
    }
    let source = pending.source.as_ref()?;
    names
        .iter()
        .filter(|(_, name)| name.eq_ignore_ascii_case(&pending.name))
        .map(|(id, _)| id)
        .find(|id| parents.get(*id).cloned().flatten().as_ref() == Some(source))
        .cloned()
}

/// Why the memory an undo resumes a computer with is unknown.
const UNDO_UNREADABLE: &str = "Silo can't read the memory this undo would resume the computer with, so it cannot tell whether it fits. Try again, or ask the person who runs Silo.";

/// The memory, in MiB, that undoing the last keep of `computer` resumes it at, when that undo
/// restores a full checkpoint. `None` when the undo takes back a promote (`promoted` is when
/// the latest one finished), restores a disk checkpoint, or has nothing to restore. A journal
/// or a capture that cannot be read is an error, so the undo is not admitted on a guess.
/// `memory` reads what a checkpoint of the computer resumes at.
fn keep_undo_resume(
    journal: &silo_engine::changes::keep::Journal,
    computer: &str,
    promoted: Option<u128>,
    memory: &dyn Fn(&str) -> Result<Option<u64>, String>,
) -> Option<Result<u64, String>> {
    let unreadable = || UNDO_UNREADABLE.to_owned();
    let pending = match journal.pending(computer) {
        Ok(pending) => pending,
        Err(_) => return Some(Err(unreadable())),
    };
    if silo_engine::changes::promote::promote_is_newest(
        promoted,
        journal.latest_undoable_at(computer),
        pending.is_some(),
    ) {
        return None;
    }
    let (scope, id) = match pending {
        Some(entry) => (entry.checkpoint_scope, entry.checkpoint_id),
        None => {
            let checkpoint = journal.latest_undoable(computer)?;
            (checkpoint.scope, checkpoint.id)
        }
    };
    (scope == "full").then(|| {
        memory(&id)
            .and_then(|mib| mib.ok_or_else(unreadable))
            .map_err(|_| unreadable())
    })
}

/// The memory, in MiB, a full checkpoint of a computer resumes it at. A macOS computer resumes
/// saved memory only in the configuration it was saved in, so it boots at its configured
/// memory; `read` is asked for any other computer.
fn undo_checkpoint_memory(
    saved: Option<&Resources>,
    read: impl FnOnce() -> Result<Option<u64>, String>,
) -> Result<Option<u64>, String> {
    match saved {
        Some(saved) if !saved.linux => Ok(Some(saved.memory_gib.saturating_mul(1024))),
        _ => read(),
    }
}

/// What an undo of the computer `params` names resumes it at, when it restores a full
/// checkpoint, by computer. Added to what a pending restore resumes the computers at.
/// `saved` is each computer's saved sizes.
async fn undo_resumes(
    context: &HandlerContext,
    params: &Value,
    saved: &HashMap<String, Resources>,
) -> HashMap<String, Result<u64, String>> {
    let Ok(undo) =
        serde_json::from_value::<silo_api::methods::keep::ChangesUndoParams>(params.clone())
    else {
        return HashMap::new();
    };
    let engine = context.engine.clone();
    let host = context.silod_context().runtime_host();
    let own = saved.get(&undo.computer_id).copied();
    tokio::task::spawn_blocking(move || {
        let promoted = silo_engine::changes::promote::latest_at(
            &silo_engine::changes::promote::service::journal_for(engine.as_ref()),
            &undo.computer_id,
        );
        let journal = silo_engine::changes::keep::service::journal_for(engine.as_ref());
        let paths = silo_engine::runtime::runtime_paths(host.as_ref());
        let memory = |checkpoint: &str| {
            undo_checkpoint_memory(own.as_ref(), || {
                let paths = paths.as_ref().map_err(|_| UNDO_UNREADABLE.to_owned())?;
                silo_engine::runtime::checkpoint_resume_memory_mib(
                    paths,
                    &undo.computer_id,
                    checkpoint,
                )
            })
        };
        keep_undo_resume(&journal, &undo.computer_id, promoted, &memory)
            .map(|resume| (undo.computer_id.clone(), resume))
            .into_iter()
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// What the gate reads about the computers before it sizes a call made by `world`'s load.
async fn sizes_of(
    context: &HandlerContext,
    method: MethodName,
    params: &Value,
    world: &EngineWorld,
) -> Sizes {
    let mut sizes = world.sizes();
    if method == MethodName::ChangesUndo {
        sizes
            .resumes
            .extend(undo_resumes(context, params, &world.resources).await);
    }
    sizes
}

impl EngineWorld {
    fn sizes(&self) -> Sizes {
        Sizes {
            resident: self.resident.clone(),
            resources: self.resources.clone(),
            resumes: self.resumes.clone(),
            parents: self.parents.clone(),
        }
    }
}

impl policy::World for EngineWorld {
    fn made(&self, creator: &str) -> std::collections::HashSet<String> {
        self.document.made_by(creator)
    }
    fn parent(&self, computer: &str) -> Option<String> {
        self.parents.get(computer).cloned().flatten()
    }
    fn name(&self, computer: &str) -> Option<String> {
        self.names.get(computer).cloned()
    }
    fn owns_job(&self, caller: &str, job_id: &str) -> bool {
        self.document.owns_job(caller, job_id)
    }
    fn request_fork(&self, request_id: &str) -> Option<String> {
        silo_engine::fold_requests::Store::new(&self.app_data)
            .get(request_id)
            .ok()
            .flatten()
            .map(|request| request.fork_id)
    }
    fn request_target(&self, request_id: &str) -> Option<String> {
        silo_engine::fold_requests::Store::new(&self.app_data)
            .get(request_id)
            .ok()
            .flatten()
            .map(|request| request.target)
    }
    fn resources(&self, computer: &str) -> Option<Resources> {
        self.resources.get(computer).copied()
    }
    fn ssh_settings(&self, computer: &str) -> Option<(u16, String)> {
        self.ssh.get(computer).cloned()
    }
    fn ssh_keys(&self, computer: &str) -> Vec<String> {
        self.ssh_keys.get(computer).cloned().unwrap_or_default()
    }
    fn network_held(&self, computer: &str) -> Option<silo_engine::network_policy::Held> {
        self.network.get(computer).cloned()
    }
    fn access(&self, computer: &str) -> Access {
        self.names
            .get(computer)
            .map(|name| policy::access::of_computer(name))
            .unwrap_or_default()
    }
    fn github_policies(&self) -> Option<Vec<Value>> {
        self.github.clone()
    }
    fn github_token_name(&self, id: &str) -> Option<String> {
        silo_engine::secrets::github_token_name(id)
    }
    fn new_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

#[cfg(test)]
mod tests;
