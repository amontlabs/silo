//! Model-based test: a call that needs the owner's approval runs only once the owner allowed
//! it, at most once, and the limits on asking and the memory set aside for allowed calls are
//! never exceeded, lost or counted twice.
//!
//! Random sequences of operations run against the real `approvals.*` handlers, the gate that
//! turns a held call into an approval, the engine's approval store and the memory reservations
//! an allowed call takes. Two computers ask: `A`, and `B`, which `A` made. A reference model
//! written here keeps what each approval must be; after every step the stored approvals (read
//! without the sweep a handler runs), their files, the notices, the calls the stand-in engine
//! ran and the memory reserved are compared with it.
//!
//! The rules, from docs/SiloUI-POLICY.md ("Asking you", "Limits"):
//!
//! 1. A held call never runs until the owner allows its approval, and runs at most once: a
//!    second answer, a restart or a recovery never runs it again. An allow the engine stopped
//!    during ends as interrupted and is never run.
//! 2. Only the owner answers. Only the computer that asked writes its note or withdraws it (the
//!    owner withdraws any); a computer sees its own approvals and those of computers it made.
//! 3. Denied, withdrawn, expired and answered are final: nothing changes them again, and an
//!    answer to one that is not waiting is an error.
//! 4. Asking again for the same call while it waits gives back the same approval; a secret's
//!    value is not part of the call, and the first value is the one that runs.
//! 5. At most 20 approvals wait for one computer; at most 30 are created by one computer in any
//!    hour, whatever became of them, and the count survives a restart.
//! 6. A waiting approval expires after seven days, whether or not anything swept it yet: it can
//!    no longer be answered, withdrawn or noted. Ended approvals are removed 30 days after they
//!    ended; one its computer withdrew is removed at once.
//! 7. Deleting a computer withdraws what it was waiting for; an allowed call whose computer or
//!    target is gone is not run.
//! 8. A secret's value is stored only while its approval waits.
//! 9. A waiting approval has its notice; any other state has none.
//! 10. On allow the call is checked again: a call that takes memory reserves it before it runs
//!     and is not run when the device has no room above its floor. The reservation is given
//!     back when the call fails or starts no job, and is held while the job it started runs.
//!     Reserved memory never exceeds what the device had above its floor when it was taken,
//!     and is never lost or counted twice.
//!
//! Deep run: `PROPTEST_CASES=5000 cargo nextest run -p silod approvals_mbt`.
//!
//! Entry points, and whether this test drives them:
//!
//! | Entry point | Covered |
//! | --- | --- |
//! | `gate::admit` turning a held call into an approval (`approvals::request`): `network.publish`, `secrets.save`, `computers.create` (container) | yes |
//! | the memory check of a held call that takes memory (`assess_headroom` at ask) | yes |
//! | `approvals.decide` allow and deny, by the owner and refused for a computer | yes |
//! | `approvals::execute`: second check, computer or target gone, memory reserved and settled, the stand-in engine's result, error or job | yes |
//! | `approvals.cancel` (computer and owner), `approvals.note`, `approvals.get`, `approvals.list` | yes |
//! | `approvals::recover` at engine start, after a clean stop and after an allow the engine stopped during | yes |
//! | `policy::store::computer_removed` (delete) withdrawing what the computer asked | yes, for `B` |
//! | expiry and removal by the sweep, and the lazy expiry an answer checks | yes, with the store's test clock |
//! | jobs that hold a reservation ending in the job registry | yes |
//! | merges (`changes.keep`, review requests), promote and undo pins | no: they need the changes machinery; `tests.rs` has examples |
//! | `approvals.decide` racing another answer on two threads | no: the store's lock is taken per call; this test is sequential |
//! | the job places a computer's own calls take (`reserve_job_slot`) | no: an allowed call runs as the owner and takes none; direct calls need the real engine |
//! | the hour's count across devices, remote relays | no: approvals are answered on the device that asked |
//!
//! The test shares the crate's engine fixture and holds the approvals tests' `serial` lock and
//! the gate's device lock, as the tests beside it do; under nextest it runs alone in its process.

use std::sync::{Arc, Mutex};

use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest_state_machine::{prop_state_machine, ReferenceStateMachine, StateMachineTest};
use serde_json::{json, Value};
use silo_api::{jobs::ErrorCode, methods::MethodName};
use silo_engine::{
    approvals::{
        self, test_clock, Store, ASKS_PER_HOUR, KEPT_FOR_MS, PENDING_FOR_MS, PENDING_LIMIT,
    },
    context::{Audience, EngineContext},
    policy::{store::PolicyStore, Caller},
    resources::{FixedHost, Host, GIB},
};

use super::{app_data, call, fixture, owner, TestRun, TEST_RUN};
use crate::methods::gate;

// MARK: The universe

const MINUTE: u64 = 60 * 1000;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;
/// The memory a computer the agents create asks for.
const CREATE_GIB: u64 = 4;
/// The device's memory and the floor Silo keeps free on it (the larger of 4 GiB and 10 %).
const DEVICE_TOTAL_GIB: u64 = 32;
const FLOOR_GIB: u64 = 4;
/// Memory available at each device level: room for six computers, for one, and below the floor.
const AVAILABLE_GIB: [u64; 3] = [28, 10, 2];
/// How far the clock moves in one step. Every step is a multiple of ten minutes, so the real
/// time a case takes never carries an approval over a boundary the model has not crossed.
const ADVANCES: [u64; 9] = [
    10 * MINUTE,
    50 * MINUTE,
    HOUR,
    6 * HOUR,
    DAY,
    7 * DAY - 10 * MINUTE,
    7 * DAY,
    23 * DAY,
    30 * DAY,
];
const PORTS: u16 = 4;
const SECRET_NAMES: u8 = 2;
const SECRET_VALUES: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Who {
    A,
    B,
}

/// A held call a computer makes. Every create names a new computer, so it is never the same
/// call as an earlier one.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
    Publish { port: u16 },
    Secret { name: u8, value: u8 },
    Create { n: u32 },
}

impl Call {
    /// What makes two calls the same request: a secret's value is not part of it.
    fn key(&self) -> Call {
        match self {
            Call::Secret { name, .. } => Call::Secret {
                name: *name,
                value: 0,
            },
            other => other.clone(),
        }
    }

    fn targets_b(&self) -> bool {
        !matches!(self, Call::Create { .. })
    }

    /// The run the stand-in engine records for this call with `value` as the secret's value
    /// and `reserved` GiB reserved while it ran.
    fn run_line(&self, kept: Option<u8>, reserved: u64) -> String {
        match self {
            Call::Publish { port } => format!("network.publish {} {reserved}", 9000 + port),
            Call::Secret { name, .. } => {
                format!("secrets.save s{name} v{} {reserved}", kept.unwrap_or(99))
            }
            Call::Create { n } => format!("computers.create {n} {reserved}"),
        }
    }
}

/// What the stand-in engine answers an allowed call with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunPlan {
    Done,
    Fails,
    StartsJob,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Pending,
    Allowed,
    Denied,
    Cancelled,
    Expired,
}

impl State {
    fn wire(self) -> &'static str {
        match self {
            State::Pending => "pending",
            State::Allowed => "allowed",
            State::Denied => "denied",
            State::Cancelled => "cancelled",
            State::Expired => "expired",
        }
    }
}

/// What an allowed approval's outcome holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    /// None stored yet.
    None,
    /// The call ran and answered.
    Result,
    /// The call ran and started a job.
    Job,
    /// The call ran and failed, or the check before it refused it, or the engine stopped.
    Error,
}

// MARK: The reference model

#[derive(Clone, Debug)]
struct Approval {
    asker: Who,
    call: Call,
    /// The secret value stored with it; `None` once scrubbed.
    value: Option<u8>,
    state: State,
    created: u64,
    ended: Option<u64>,
    outcome: Outcome,
    note: Option<u8>,
    removed: bool,
}

impl Approval {
    fn past_due(&self, now: u64) -> bool {
        self.state == State::Pending && now - self.created >= PENDING_FOR_MS
    }
}

#[derive(Clone, Debug)]
struct Reservation {
    gib: u64,
    job: usize,
}

/// Why an ask is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
    Limit,
    Rate,
    /// The policy or the memory check refused it before it became an approval.
    Before,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Asked {
    Same(usize),
    New(usize),
    Refused(Why),
}

/// What the engine must answer to the last operation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Expect {
    #[default]
    Nothing,
    Asks(Vec<Asked>),
    Ok(bool),
    /// A computer reads an approval: whether it sees it, and its state as the read leaves it.
    Read(Option<State>),
}

#[derive(Clone, Debug)]
struct Model {
    now: u64,
    approvals: Vec<Approval>,
    /// When each computer asked, `A` then `B`.
    asks: [Vec<u64>; 2],
    b_deleted: bool,
    device: usize,
    /// What the stand-in engine ran, in order.
    runs: Vec<String>,
    reservations: Vec<Reservation>,
    /// Whether each job an allowed call started still runs.
    jobs: Vec<bool>,
    creates: u32,
    fresh_port: u16,
    expect: Expect,
}

fn slot(who: Who) -> usize {
    match who {
        Who::A => 0,
        Who::B => 1,
    }
}

impl Model {
    fn headroom_gib(&self) -> Option<u64> {
        let available = AVAILABLE_GIB[self.device];
        (available >= FLOOR_GIB).then(|| available - FLOOR_GIB)
    }

    fn reserved_gib(&self) -> u64 {
        self.reservations
            .iter()
            .filter(|r| self.jobs[r.job])
            .map(|r| r.gib)
            .sum()
    }

    /// The reservations the next memory check drops: those whose job ended.
    fn prune(&mut self) {
        let jobs = self.jobs.clone();
        self.reservations.retain(|r| jobs[r.job]);
    }

    fn fits(&mut self, gib: u64) -> bool {
        self.prune();
        self.headroom_gib()
            .is_some_and(|room| self.reserved_gib() + gib <= room)
    }

    /// Whether `who` sees `approval`: its own, and those of a computer it made that still
    /// exists.
    fn sees(&self, who: Who, approval: &Approval) -> bool {
        approval.asker == who || (who == Who::A && !self.b_deleted)
    }

    /// What a handler's sweep does: ends waiting approvals past their time, removes ended ones
    /// past theirs, and drops a secret's value from what ended.
    fn sweep(&mut self) {
        let now = self.now;
        for approval in self.approvals.iter_mut().filter(|a| !a.removed) {
            match approval.state {
                State::Pending if now - approval.created >= PENDING_FOR_MS => {
                    approval.state = State::Expired;
                    approval.ended = Some(now);
                    approval.value = None;
                }
                State::Pending => {}
                State::Allowed if approval.outcome == Outcome::None => {}
                _ => {
                    if now - approval.ended.unwrap_or(approval.created) >= KEPT_FOR_MS {
                        approval.removed = true;
                    }
                }
            }
        }
    }

    fn ask(&mut self, who: Who, call: Call) -> Asked {
        let gone = self.b_deleted && (who == Who::B || call.targets_b());
        if gone {
            return Asked::Refused(Why::Before);
        }
        if matches!(call, Call::Create { .. }) && !self.fits(CREATE_GIB) {
            return Asked::Refused(Why::Before);
        }
        self.sweep();
        let key = call.key();
        if let Some(same) = self.approvals.iter().position(|a| {
            !a.removed && a.state == State::Pending && a.asker == who && a.call.key() == key
        }) {
            return Asked::Same(same);
        }
        let waiting = self
            .approvals
            .iter()
            .filter(|a| !a.removed && a.state == State::Pending && a.asker == who)
            .count();
        if waiting >= PENDING_LIMIT {
            return Asked::Refused(Why::Limit);
        }
        let now = self.now;
        let asks = &mut self.asks[slot(who)];
        if asks.iter().filter(|at| now - **at < HOUR).count() >= ASKS_PER_HOUR {
            return Asked::Refused(Why::Rate);
        }
        asks.push(now);
        let value = match call {
            Call::Secret { value, .. } => Some(value),
            _ => None,
        };
        self.approvals.push(Approval {
            asker: who,
            call,
            value,
            state: State::Pending,
            created: now,
            ended: None,
            outcome: Outcome::None,
            note: None,
            removed: false,
        });
        Asked::New(self.approvals.len() - 1)
    }

    /// The approval an operation names, when one exists.
    fn pick(&self, index: u8) -> Option<usize> {
        (!self.approvals.is_empty()).then(|| usize::from(index) % self.approvals.len())
    }

    /// Whether the approval can still change: it exists and waits, and is not past its time.
    fn changeable(&self, index: usize) -> bool {
        let approval = &self.approvals[index];
        !approval.removed && approval.state == State::Pending && !approval.past_due(self.now)
    }

    fn allow(&mut self, index: usize, plan: RunPlan) {
        let now = self.now;
        let approval = &mut self.approvals[index];
        approval.state = State::Allowed;
        approval.ended = Some(now);
        let (asker, call, kept) = (approval.asker, approval.call.clone(), approval.value);
        let gone = self.b_deleted && (asker == Who::B || call.targets_b());
        let reserves = matches!(call, Call::Create { .. });
        let outcome = if gone || (reserves && !self.fits(CREATE_GIB)) {
            Outcome::Error
        } else {
            let gib = if reserves { CREATE_GIB } else { 0 };
            self.runs
                .push(call.run_line(kept, self.reserved_gib() + gib));
            match plan {
                RunPlan::Done => Outcome::Result,
                RunPlan::Fails => Outcome::Error,
                RunPlan::StartsJob => {
                    self.jobs.push(true);
                    if reserves {
                        self.reservations.push(Reservation {
                            gib,
                            job: self.jobs.len() - 1,
                        });
                    }
                    Outcome::Job
                }
            }
        };
        let approval = &mut self.approvals[index];
        approval.outcome = outcome;
        approval.value = None;
    }

    /// An engine start: memory reservations and running jobs are gone, an allowed call without
    /// an outcome ends as interrupted, and the sweep runs.
    fn restart(&mut self) {
        self.reservations.clear();
        for job in &mut self.jobs {
            *job = false;
        }
        for approval in &mut self.approvals {
            if !approval.removed
                && approval.state == State::Allowed
                && approval.outcome == Outcome::None
            {
                approval.outcome = Outcome::Error;
                approval.value = None;
            }
        }
        self.sweep();
    }
}

#[derive(Clone, Debug)]
enum Op {
    Ask {
        who: Who,
        call: Call,
    },
    /// Asks for `n` calls never asked before, to reach the limits.
    AskMany {
        who: Who,
        n: u8,
    },
    /// Asks for `n` calls never asked before and withdraws each one it made, which frees its
    /// place among those waiting but not its place in the hour's count.
    AskAndWithdraw {
        who: Who,
        n: u8,
    },
    Decide {
        index: u8,
        allow: bool,
        plan: RunPlan,
    },
    /// A computer tries to answer.
    DecideAsComputer {
        index: u8,
        who: Who,
    },
    Cancel {
        index: u8,
        by: Option<Who>,
    },
    Note {
        index: u8,
        who: Who,
        text: Option<u8>,
    },
    /// The owner lists every approval.
    List,
    /// A computer reads one approval.
    Get {
        index: u8,
        who: Who,
    },
    Advance {
        step: usize,
    },
    Restart,
    /// The engine records an allow and stops before the call runs.
    AllowThenStop {
        index: u8,
    },
    DeleteB,
    Device {
        level: usize,
    },
    FinishJob {
        index: u8,
    },
}

#[derive(Clone, Debug)]
enum Transition {
    Op(Op),
}

fn who() -> impl Strategy<Value = Who> {
    prop_oneof![Just(Who::A), Just(Who::B)]
}

fn plan() -> impl Strategy<Value = RunPlan> {
    prop_oneof![
        3 => Just(RunPlan::Done),
        1 => Just(RunPlan::Fails),
        2 => Just(RunPlan::StartsJob),
    ]
}

struct ApprovalsModel;

impl ReferenceStateMachine for ApprovalsModel {
    type State = Model;
    type Transition = Transition;

    fn init_state() -> BoxedStrategy<Model> {
        Just(Model {
            now: 0,
            approvals: Vec::new(),
            asks: [Vec::new(), Vec::new()],
            b_deleted: false,
            device: 0,
            runs: Vec::new(),
            reservations: Vec::new(),
            jobs: Vec::new(),
            creates: 0,
            fresh_port: 0,
            expect: Expect::Nothing,
        })
        .boxed()
    }

    fn transitions(_: &Model) -> BoxedStrategy<Transition> {
        let index = any::<u8>;
        let call = prop_oneof![
            3 => (0..PORTS).prop_map(|port| Call::Publish { port }),
            2 => (0..SECRET_NAMES, 0..SECRET_VALUES).prop_map(|(name, value)| Call::Secret { name, value }),
            3 => Just(Call::Create { n: 0 }),
        ];
        prop_oneof![
            10 => (who(), call).prop_map(|(who, call)| Op::Ask { who, call }),
            2 => (who(), 1u8..=22).prop_map(|(who, n)| Op::AskMany { who, n }),
            1 => (who(), 1u8..=32).prop_map(|(who, n)| Op::AskAndWithdraw { who, n }),
            8 => (index(), any::<bool>(), plan()).prop_map(|(index, allow, plan)| Op::Decide { index, allow, plan }),
            1 => (index(), who()).prop_map(|(index, who)| Op::DecideAsComputer { index, who }),
            3 => (index(), proptest::option::of(who())).prop_map(|(index, by)| Op::Cancel { index, by }),
            2 => (index(), who(), proptest::option::of(0u8..3)).prop_map(|(index, who, text)| Op::Note { index, who, text }),
            1 => Just(Op::List),
            2 => (index(), who()).prop_map(|(index, who)| Op::Get { index, who }),
            4 => (0..ADVANCES.len()).prop_map(|step| Op::Advance { step }),
            2 => Just(Op::Restart),
            1 => index().prop_map(|index| Op::AllowThenStop { index }),
            1 => Just(Op::DeleteB),
            2 => (0..AVAILABLE_GIB.len()).prop_map(|level| Op::Device { level }),
            2 => index().prop_map(|index| Op::FinishJob { index }),
        ]
        .prop_map(Transition::Op)
        .boxed()
    }

    fn preconditions(state: &Model, transition: &Transition) -> bool {
        let Transition::Op(op) = transition;
        match op {
            // A deleted computer makes no calls in this model.
            Op::Ask { who: Who::B, .. }
            | Op::AskMany { who: Who::B, .. }
            | Op::AskAndWithdraw { who: Who::B, .. }
            | Op::DecideAsComputer { who: Who::B, .. }
            | Op::Cancel {
                by: Some(Who::B), ..
            }
            | Op::Note { who: Who::B, .. }
            | Op::Get { who: Who::B, .. } => !state.b_deleted,
            Op::DeleteB => !state.b_deleted,
            _ => true,
        }
    }

    fn apply(mut state: Model, transition: &Transition) -> Model {
        let Transition::Op(op) = transition;
        state.expect = model_apply(&mut state, op);
        state
    }
}

fn model_apply(state: &mut Model, op: &Op) -> Expect {
    match op {
        Op::Ask { who, call } => {
            let call = match call {
                Call::Create { .. } => {
                    state.creates += 1;
                    Call::Create { n: state.creates }
                }
                other => other.clone(),
            };
            Expect::Asks(vec![state.ask(*who, call)])
        }
        Op::AskMany { who, n } | Op::AskAndWithdraw { who, n } => {
            let withdraw = matches!(op, Op::AskAndWithdraw { .. });
            let mut asked = Vec::new();
            for _ in 0..*n {
                state.fresh_port += 1;
                let answer = state.ask(
                    *who,
                    Call::Publish {
                        port: 100 + state.fresh_port,
                    },
                );
                if let (true, Asked::New(index)) = (withdraw, &answer) {
                    let approval = &mut state.approvals[*index];
                    approval.state = State::Cancelled;
                    approval.ended = Some(state.now);
                    approval.removed = true;
                }
                asked.push(answer);
            }
            Expect::Asks(asked)
        }
        Op::Decide { index, allow, plan } => {
            let Some(index) = state.pick(*index) else {
                return Expect::Nothing;
            };
            if !state.changeable(index) {
                return Expect::Ok(false);
            }
            if *allow {
                state.allow(index, *plan);
            } else {
                let now = state.now;
                let approval = &mut state.approvals[index];
                approval.state = State::Denied;
                approval.ended = Some(now);
                approval.value = None;
            }
            Expect::Ok(true)
        }
        Op::DecideAsComputer { index, .. } => match state.pick(*index) {
            Some(_) => Expect::Ok(false),
            None => Expect::Nothing,
        },
        Op::Cancel { index, by } => {
            let Some(index) = state.pick(*index) else {
                return Expect::Nothing;
            };
            let approval = &state.approvals[index];
            let allowed = match by {
                Some(who) => state.sees(*who, approval) && approval.asker == *who,
                None => true,
            };
            if !allowed || !state.changeable(index) {
                return Expect::Ok(false);
            }
            let now = state.now;
            let approval = &mut state.approvals[index];
            approval.state = State::Cancelled;
            approval.ended = Some(now);
            approval.value = None;
            approval.removed = by.is_some();
            Expect::Ok(true)
        }
        Op::Note { index, who, text } => {
            let Some(index) = state.pick(*index) else {
                return Expect::Nothing;
            };
            let approval = &state.approvals[index];
            if approval.asker != *who || !state.changeable(index) {
                return Expect::Ok(false);
            }
            state.approvals[index].note = *text;
            Expect::Ok(true)
        }
        Op::List => {
            state.sweep();
            Expect::Nothing
        }
        Op::Get { index, who } => {
            let Some(index) = state.pick(*index) else {
                return Expect::Nothing;
            };
            state.sweep();
            let approval = &state.approvals[index];
            Expect::Read(
                (!approval.removed && state.sees(*who, approval)).then_some(approval.state),
            )
        }
        Op::Advance { step } => {
            state.now += ADVANCES[*step];
            Expect::Nothing
        }
        Op::Restart => {
            state.restart();
            Expect::Nothing
        }
        Op::AllowThenStop { index } => {
            let Some(index) = state.pick(*index) else {
                return Expect::Nothing;
            };
            let allowed = state.changeable(index);
            if allowed {
                let now = state.now;
                let approval = &mut state.approvals[index];
                approval.state = State::Allowed;
                approval.ended = Some(now);
            }
            state.restart();
            Expect::Ok(allowed)
        }
        Op::DeleteB => {
            state.b_deleted = true;
            let now = state.now;
            for approval in state.approvals.iter_mut().filter(|a| {
                !a.removed && a.asker == Who::B && a.state == State::Pending && !a.past_due(now)
            }) {
                approval.state = State::Cancelled;
                approval.ended = Some(now);
                approval.value = None;
            }
            Expect::Nothing
        }
        Op::Device { level } => {
            state.device = *level;
            Expect::Nothing
        }
        Op::FinishJob { index } => {
            if let Some(job) =
                (!state.jobs.is_empty()).then(|| usize::from(*index) % state.jobs.len())
            {
                state.jobs[job] = false;
            }
            Expect::Nothing
        }
    }
}

// MARK: The engine under test

/// The calls the stand-in engine ran, as [`Call::run_line`] writes them.
type Runs = Arc<Mutex<Vec<String>>>;

struct Engine {
    a: String,
    b: String,
    /// The engine's id of each approval, by the model's index.
    ids: Vec<String>,
    /// The engine's id of each job an allowed call started, by the model's index.
    jobs: Arc<Mutex<Vec<String>>>,
    runs: Runs,
    /// What the stand-in engine answers the next allowed call with.
    plan: Arc<Mutex<RunPlan>>,
    run: TestRun,
    runtime: tokio::runtime::Runtime,
    host: Option<FixedHost>,
    _device: std::sync::MutexGuard<'static, ()>,
    _serial: std::sync::MutexGuard<'static, ()>,
}

fn host(level: usize) -> FixedHost {
    FixedHost::new(Host {
        logical_cpus: 8,
        memory_total: DEVICE_TOTAL_GIB * GIB,
        memory_available: Some(AVAILABLE_GIB[level] * GIB),
        pressure: None,
        disk_free: 400 * GIB,
        disk_total: 500 * GIB,
    })
}

fn store() -> Store {
    Store::new(&app_data())
}

fn engine_context() -> &'static dyn EngineContext {
    fixture().context.as_ref()
}

/// The stand-in for the engine running an allowed call: it records the call with the memory
/// reserved while it runs, and answers as `plan` says.
fn stand_in(runs: Runs, plan: Arc<Mutex<RunPlan>>, jobs: Arc<Mutex<Vec<String>>>) -> TestRun {
    Arc::new(move |method, params: &Value| {
        let reserved = gate::reserved_memory_now(engine_context()) / GIB;
        let line = match method {
            MethodName::NetworkPublish => format!("network.publish {} {reserved}", params["port"]),
            MethodName::SecretsSave => format!(
                "secrets.save {} {} {reserved}",
                params["request"]["name"].as_str().unwrap_or_default(),
                params["request"]["value"].as_str().unwrap_or("(none)"),
            ),
            MethodName::ComputersCreate => {
                let name = params["configuration"]["name"].as_str().unwrap_or_default();
                format!(
                    "computers.create {} {reserved}",
                    name.rsplit('-').next().unwrap_or_default()
                )
            }
            other => format!("{} {reserved}", other.name()),
        };
        runs.lock().unwrap().push(line);
        match *plan.lock().unwrap() {
            RunPlan::Done => Ok(match method {
                MethodName::ComputersCreate => {
                    json!({"computer": {"id": uuid::Uuid::new_v4().to_string(), "name": "made"}})
                }
                _ => json!({"computers": []}),
            }),
            RunPlan::Fails => Err(crate::dispatcher::DispatchError::internal(
                "The stand-in engine failed.",
            )),
            RunPlan::StartsJob => {
                let job = silo_engine::jobs::allocate(engine_context(), method.name())
                    .expect("a job is allocated")
                    .job_id;
                jobs.lock().unwrap().push(job.as_str().to_owned());
                Ok(json!({"jobId": job.as_str()}))
            }
        }
    })
}

impl Engine {
    fn caller(&self, who: Who) -> Caller {
        Caller::Computer {
            id: match who {
                Who::A => self.a.clone(),
                Who::B => self.b.clone(),
            },
        }
    }

    fn id_of(&self, who: Who) -> &str {
        match who {
            Who::A => &self.a,
            Who::B => &self.b,
        }
    }

    fn params(&self, call: &Call) -> (MethodName, Value) {
        match call {
            Call::Publish { port } => (
                MethodName::NetworkPublish,
                json!({"computerId": self.b, "port": 9000 + port}),
            ),
            Call::Secret { name, value } => (
                MethodName::SecretsSave,
                json!({"request": {"operation": "add", "name": format!("s{name}"),
                    "value": format!("v{value}"), "computerIds": [self.b],
                    "allowedDomains": ["deploy.example.com"]}}),
            ),
            Call::Create { n } => (
                MethodName::ComputersCreate,
                json!({"kind": "linux", "configuration": {
                    "name": format!("m{}-{n}", &self.a[4..12]),
                    "cpus": 2, "maxCPUs": 2, "memoryGiB": CREATE_GIB, "maxMemoryGiB": CREATE_GIB,
                    "workspaceStorageGiB": 10, "runtimeStorageGiB": 10, "runtime": "container"}}),
            ),
        }
    }

    fn block<T>(&self, future: impl std::future::Future<Output = T>) -> T {
        self.runtime
            .block_on(TEST_RUN.scope(self.run.clone(), future))
    }

    /// An engine start: what the process held in memory is gone, the jobs it ran with it, and
    /// the approvals folder is read again.
    fn restart(&self) {
        store().forget_loaded();
        gate::clear_reservations();
        self.finish_jobs(|_| true);
        super::super::recover(fixture().context.as_ref());
    }

    fn finish_jobs(&self, which: impl Fn(usize) -> bool) {
        let registry = engine_context().job_registry().unwrap();
        for (index, job) in self.jobs.lock().unwrap().iter().enumerate() {
            if !which(index) {
                continue;
            }
            let job = silo_api::jobs::JobId::new(job).unwrap();
            if matches!(
                registry.state_of(&job),
                Ok(Some(silo_api::jobs::JobState::Finished)) | Ok(None)
            ) {
                continue;
            }
            registry
                .finish(
                    &job,
                    &silo_api::jobs::JobFinished {
                        job_id: job.clone(),
                        result: Some(json!({})),
                        error: None,
                        outcome_chunked: false,
                    },
                )
                .unwrap();
        }
    }

    fn ask(&mut self, who: Who, call: &Call, want: &Asked) {
        let (method, params) = self.params(call);
        let answer = self.block(call_as(&self.caller(who), method, params));
        match (want, answer) {
            (Asked::Same(index), Err(error)) if error.code == ErrorCode::ApprovalRequired => {
                let id = error.details.unwrap().approval_id.unwrap();
                assert_eq!(id, self.ids[*index], "asking again gives the same approval");
            }
            (Asked::New(index), Err(error)) if error.code == ErrorCode::ApprovalRequired => {
                let id = error.details.unwrap().approval_id.unwrap();
                assert!(
                    !self.ids.contains(&id),
                    "a new ask made a new approval: {id}"
                );
                assert_eq!(*index, self.ids.len());
                self.ids.push(id);
            }
            (Asked::Refused(why), Err(error)) if error.code != ErrorCode::ApprovalRequired => {
                match why {
                    Why::Limit => assert!(
                        error.message.contains("requests waiting"),
                        "{}",
                        error.message
                    ),
                    Why::Rate => {
                        assert!(error.message.contains("last hour"), "{}", error.message)
                    }
                    Why::Before => assert!(
                        !error.message.contains("requests waiting")
                            && !error.message.contains("last hour"),
                        "{}",
                        error.message
                    ),
                }
            }
            (want, answer) => panic!("asked {call:?} as {who:?}: want {want:?}, got {answer:?}"),
        }
    }
}

async fn call_as(
    caller: &Caller,
    method: MethodName,
    params: Value,
) -> Result<Value, crate::dispatcher::DispatchError> {
    call(caller, method, params).await
}

fn engine_apply(engine: &mut Engine, after: &Model, op: &Op) {
    let id = |index: u8| -> Option<String> {
        (!engine.ids.is_empty()).then(|| engine.ids[usize::from(index) % engine.ids.len()].clone())
    };
    let ok = |want: &Expect, got: bool, what: &str| {
        if let Expect::Ok(want) = want {
            assert_eq!(*want, got, "{what}");
        }
    };
    match op {
        Op::Ask { who, .. } => {
            // The model numbered the create; the engine names it the same way.
            let Expect::Asks(asked) = &after.expect else {
                unreachable!()
            };
            let call = match op {
                Op::Ask {
                    call: Call::Create { .. },
                    ..
                } => Call::Create { n: after.creates },
                Op::Ask { call, .. } => call.clone(),
                _ => unreachable!(),
            };
            engine.ask(*who, &call, &asked[0]);
        }
        Op::AskMany { who, n } | Op::AskAndWithdraw { who, n } => {
            let Expect::Asks(asked) = &after.expect else {
                unreachable!()
            };
            let first = after.fresh_port - u16::from(*n) + 1;
            for (offset, want) in asked.iter().enumerate() {
                let call = Call::Publish {
                    port: 100 + first + offset as u16,
                };
                engine.ask(*who, &call, want);
                if let (Op::AskAndWithdraw { .. }, Asked::New(index)) = (op, want) {
                    let id = engine.ids[*index].clone();
                    let withdrawn = engine.block(call_as(
                        &engine.caller(*who),
                        MethodName::ApprovalsCancel,
                        json!({"id": id}),
                    ));
                    assert!(withdrawn.is_ok(), "{who:?} withdraws {id}: {withdrawn:?}");
                }
            }
        }
        Op::Decide { index, allow, plan } => {
            let Some(id) = id(*index) else { return };
            *engine.plan.lock().unwrap() = *plan;
            let decision = if *allow { "allow" } else { "deny" };
            let answer = engine.block(call_as(
                &owner(),
                MethodName::ApprovalsDecide,
                json!({"id": id, "decision": decision}),
            ));
            ok(
                &after.expect,
                answer.is_ok(),
                &format!("decide {decision} {id}: {answer:?}"),
            );
        }
        Op::DecideAsComputer { index, who } => {
            let Some(id) = id(*index) else { return };
            let answer = engine.block(call_as(
                &engine.caller(*who),
                MethodName::ApprovalsDecide,
                json!({"id": id, "decision": "allow"}),
            ));
            ok(&after.expect, answer.is_ok(), "a computer answered");
        }
        Op::Cancel { index, by } => {
            let Some(id) = id(*index) else { return };
            let caller = by.map_or_else(owner, |who| engine.caller(who));
            let answer = engine.block(call_as(
                &caller,
                MethodName::ApprovalsCancel,
                json!({"id": id}),
            ));
            ok(
                &after.expect,
                answer.is_ok(),
                &format!("cancel {id} by {by:?}: {answer:?}"),
            );
        }
        Op::Note { index, who, text } => {
            let Some(id) = id(*index) else { return };
            let note = text.map_or_else(String::new, |text| format!("note {text}"));
            let answer = engine.block(call_as(
                &engine.caller(*who),
                MethodName::ApprovalsNote,
                json!({"id": id, "note": note}),
            ));
            ok(
                &after.expect,
                answer.is_ok(),
                &format!("note {id} by {who:?}: {answer:?}"),
            );
        }
        Op::List => {
            let listed = engine
                .block(call_as(&owner(), MethodName::ApprovalsList, json!({})))
                .unwrap();
            let mine: std::collections::BTreeSet<String> = listed["approvals"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|approval| {
                    let computer = approval["computerId"].as_str().unwrap_or_default();
                    computer == engine.a || computer == engine.b
                })
                .map(|approval| approval["id"].as_str().unwrap().to_owned())
                .collect();
            let want: std::collections::BTreeSet<String> = after
                .approvals
                .iter()
                .zip(&engine.ids)
                .filter(|(approval, _)| !approval.removed)
                .map(|(_, id)| id.clone())
                .collect();
            assert_eq!(mine, want, "the owner lists every approval that exists");
        }
        Op::Get { index, who } => {
            let Some(id) = id(*index) else { return };
            let answer = engine.block(call_as(
                &engine.caller(*who),
                MethodName::ApprovalsGet,
                json!({"id": id}),
            ));
            let Expect::Read(want) = &after.expect else {
                unreachable!()
            };
            match (want, answer) {
                (Some(state), Ok(read)) => assert_eq!(read["state"], state.wire(), "{read}"),
                (None, Err(error)) => assert_eq!(error.code, ErrorCode::NotFound, "{error:?}"),
                (want, got) => panic!("{who:?} reads {id}: want {want:?}, got {got:?}"),
            }
        }
        Op::Advance { step } => test_clock::advance(ADVANCES[*step]),
        Op::Restart => engine.restart(),
        Op::AllowThenStop { index } => {
            let Some(id) = id(*index) else { return };
            let allowed = store().allow(&id, "app", None);
            ok(
                &after.expect,
                allowed.is_ok(),
                &format!("allow {id}: {allowed:?}"),
            );
            engine.restart();
        }
        Op::DeleteB => {
            silo_engine::policy::store::computer_removed(&engine.b);
            gate::test_nodes::remove(&engine.b);
        }
        Op::Device { level } => {
            // The old one clears the fixed device when it drops, so it goes first.
            engine.host = None;
            engine.host = Some(host(*level));
        }
        Op::FinishJob { index } => {
            let count = engine.jobs.lock().unwrap().len();
            if count > 0 {
                let chosen = usize::from(*index) % count;
                engine.finish_jobs(|job| job == chosen);
            }
        }
    }
}

/// Compares what the engine stored with the model, without the sweep a handler would run.
fn check(engine: &Engine, model: &Model) {
    assert_eq!(engine.ids.len(), model.approvals.len());
    let notices: std::collections::BTreeSet<String> = silo_engine::notices::list(engine_context())
        .unwrap()
        .into_iter()
        .map(|notice| notice.key)
        .collect();
    for (approval, id) in model.approvals.iter().zip(&engine.ids) {
        let stored = store().get(id).unwrap();
        let file = app_data().join("approvals").join(format!("{id}.json"));
        let noticed = notices.contains(&approvals::notice_key(id));
        if approval.removed {
            assert!(stored.is_none(), "{id} is removed: {stored:?}");
            assert!(!file.exists(), "{id}'s file is removed");
            assert!(!noticed, "{id} is removed and keeps no notice");
            continue;
        }
        let stored = stored.unwrap_or_else(|| panic!("{id} exists: {approval:?}"));
        let view = serde_json::to_value(&stored.approval).unwrap();
        assert_eq!(view["state"], approval.state.wire(), "{id}: {view}");
        assert_eq!(
            stored.computer(),
            engine.id_of(approval.asker),
            "{id}'s computer"
        );
        assert_eq!(
            noticed,
            approval.state == State::Pending,
            "{id} has a notice only while it waits"
        );
        let outcome = stored.approval.outcome.as_ref();
        let found = match outcome {
            None => Outcome::None,
            Some(outcome) if outcome.job_id.is_some() => Outcome::Job,
            Some(outcome) if outcome.error.is_some() => Outcome::Error,
            Some(_) => Outcome::Result,
        };
        assert_eq!(found, approval.outcome, "{id}'s outcome: {view}");
        assert_eq!(
            stored.approval.note,
            approval.note.map(|text| format!("note {text}")),
            "{id}'s note"
        );
        // The value is what the file on disk holds, not only the copy in memory.
        let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        let value = saved
            .pointer("/params/request/value")
            .and_then(Value::as_str)
            .map(str::to_owned);
        assert_eq!(
            value,
            approval.value.map(|value| format!("v{value}")),
            "{id}'s stored secret value"
        );
        if approval.state != State::Pending {
            assert!(value.is_none(), "{id} ended and keeps no secret value");
        }
    }
    assert_eq!(
        *engine.runs.lock().unwrap(),
        model.runs,
        "the calls the engine ran"
    );
    let reserved = gate::reserved_memory_now(engine_context());
    // The model reserves only what fits above the floor, so equal is also never above it.
    assert_eq!(reserved, model.reserved_gib() * GIB, "the memory reserved");
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Leaves the shared fixture as the next case or test expects it.
        for id in &self.ids {
            let _ = std::fs::remove_file(app_data().join("approvals").join(format!("{id}.json")));
            let _ =
                engine_context().clear_notice(&approvals::notice_key(id), &self.a, Audience::All);
        }
        self.finish_jobs(|_| true);
        gate::clear_reservations();
        store().forget_loaded();
        let _ = PolicyStore::new(&app_data()).remove_computers(&[&self.a, &self.b]);
        gate::test_nodes::remove(&self.a);
        gate::test_nodes::remove(&self.b);
        test_clock::reset();
        print_stats();
    }
}

thread_local! {
    /// How often each outcome happened, printed with `SILO_MBT_STATS=1`.
    static STATS: std::cell::RefCell<std::collections::BTreeMap<String, usize>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

fn note(what: impl Into<String>) {
    STATS.with(|stats| *stats.borrow_mut().entry(what.into()).or_default() += 1);
}

/// The name of an operation, for the counts.
fn op_name(op: &Op) -> String {
    format!("{op:?}")
        .split([' ', '{'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn note_outcome(after: &Model, op: &Op) {
    match &after.expect {
        Expect::Asks(asked) => {
            for asked in asked {
                note(match asked {
                    Asked::Same(_) => "ask: same".to_owned(),
                    Asked::New(_) => "ask: new".to_owned(),
                    Asked::Refused(why) => format!("ask: refused {why:?}"),
                });
            }
        }
        Expect::Ok(ok) => note(format!(
            "{}: {}",
            op_name(op),
            if *ok { "done" } else { "refused" }
        )),
        Expect::Read(read) => note(format!("get: {read:?}")),
        Expect::Nothing => note(op_name(op)),
    }
    if let (
        Op::Decide {
            index, allow: true, ..
        },
        Expect::Ok(true),
    ) = (op, &after.expect)
    {
        if let Some(index) = after.pick(*index) {
            let approval = &after.approvals[index];
            let kind = format!("{:?}", approval.call);
            note(format!(
                "allowed {}: {:?}",
                kind.split(' ').next().unwrap_or_default(),
                approval.outcome
            ));
        }
    }
}

fn print_stats() {
    if std::env::var_os("SILO_MBT_STATS").is_some() {
        STATS.with(|stats| {
            for (what, count) in stats.borrow().iter() {
                eprintln!("approvals_mbt: {what}: {count}");
            }
        });
    }
}

struct ApprovalsTest;

impl StateMachineTest for ApprovalsTest {
    type SystemUnderTest = Engine;
    type Reference = ApprovalsModel;

    fn init_test(_: &Model) -> Engine {
        let serial = super::serial();
        let device = gate::test_exclusive();
        test_clock::reset();
        gate::clear_reservations();
        let tree = super::Tree::new();
        let runs: Runs = Arc::default();
        let plan = Arc::new(Mutex::new(RunPlan::Done));
        let jobs: Arc<Mutex<Vec<String>>> = Arc::default();
        let run = stand_in(runs.clone(), plan.clone(), jobs.clone());
        Engine {
            a: tree.me,
            b: tree.fork,
            ids: Vec::new(),
            jobs,
            runs,
            plan,
            run,
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            host: Some(host(0)),
            _device: device,
            _serial: serial,
        }
    }

    fn apply(mut engine: Engine, after: &Model, transition: Transition) -> Engine {
        let Transition::Op(op) = transition;
        note_outcome(after, &op);
        // Durability is not under test: the job registry, which keeps the notices, skips
        // flushing each commit to disk.
        engine_context()
            .job_registry()
            .unwrap()
            .without_flushing(|| engine_apply(&mut engine, after, &op));
        engine
    }

    fn check_invariants(engine: &Engine, model: &Model) {
        check(engine, model);
    }
}

/// `PROPTEST_CASES`, else `default`.
fn cases(default: u32) -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(default)
}

prop_state_machine! {
    #![proptest_config(Config {
        cases: cases(128),
        max_shrink_iters: 4096,
        failure_persistence: None,
        .. Config::default()
    })]

    /// Held calls run only once allowed, at most once, and the limits hold.
    #[test]
    fn approvals_and_limits_hold(sequential 1..40 => ApprovalsTest);
}
