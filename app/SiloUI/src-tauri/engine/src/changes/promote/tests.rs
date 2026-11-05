//! Promote and undo against a fake device: names, name-keyed stores, lineage, quiet parking and
//! the macOS proxy state are plain maps, so a test can compare the whole device before a
//! promote and after its undo, and stop the "engine" (a panic) between any two steps.

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{json, Value};

use super::{
    journal::{Journal, State, Step, Swap},
    promote, recover_all, undo, Info, Outcome, PromoteEnv, Request,
};
use crate::changes::keep::{CheckpointRef, ManifestStore, Os};

const MAIN: &str = "id-main";
const FORK: &str = "id-fork";
const STORES: [&str; 5] = ["secrets", "github", "ports", "urls", "ssh"];

#[derive(Clone, Debug, Default, PartialEq)]
struct Device {
    names: BTreeMap<String, String>,
    running: BTreeSet<String>,
    busy: BTreeMap<String, String>,
    /// store -> computer name -> what it holds
    stores: BTreeMap<String, BTreeMap<String, Value>>,
    /// computer id -> id of the computer it was forked from
    source: BTreeMap<String, String>,
    /// computer id -> id of the fork that replaced it
    replaced_by: BTreeMap<String, String>,
    /// computer id -> quiet parking still in place
    parked: BTreeMap<String, bool>,
    /// computer id -> the fork changed what was parked
    quiet_changed: BTreeSet<String>,
    /// computer id -> proxy CA fingerprint (id-keyed: never moves)
    ca: BTreeMap<String, String>,
    /// computer name -> cached proxy policy
    proxy_policy: BTreeSet<String>,
    checkpoints: Vec<(String, String)>,
}

struct Fake {
    os: Os,
    device: Mutex<Device>,
    /// Stop (panic) right after this step's work, before the journal marks it.
    crash_after: Mutex<Option<(Step, bool)>>,
    fail_apply: Mutex<Option<Step>>,
}

impl Fake {
    fn new(os: Os) -> Arc<Self> {
        let mut device = Device::default();
        for (id, name) in [(MAIN, "main"), (FORK, "try-it")] {
            device.names.insert(id.into(), name.into());
            device.parked.insert(id.into(), id == FORK);
            device.ca.insert(id.into(), format!("ca-{id}"));
            device.running.insert(id.into());
        }
        device.source.insert(FORK.into(), MAIN.into());
        for store in STORES {
            let entry = device.stores.entry(store.into()).or_default();
            entry.insert("main".into(), json!({"from": "main", "store": store}));
            entry.insert(
                "try-it".into(),
                json!({"from": "fork", "store": store, "extra": true}),
            );
        }
        device
            .proxy_policy
            .extend(["main".to_owned(), "try-it".to_owned()]);
        Arc::new(Self {
            os,
            device: Mutex::new(device),
            crash_after: Mutex::default(),
            fail_apply: Mutex::default(),
        })
    }

    fn snapshot(&self) -> Device {
        self.device.lock().unwrap().clone()
    }

    fn name_of(&self, id: &str) -> String {
        self.snapshot().names[id].clone()
    }

    fn crash_after(&self, step: Step, on_revert: bool) {
        *self.crash_after.lock().unwrap() = Some((step, on_revert));
    }

    fn maybe_crash(&self, step: Step, revert: bool) {
        let mut crash = self.crash_after.lock().unwrap();
        if *crash == Some((step, revert)) {
            *crash = None;
            drop(crash);
            panic!("the engine stopped");
        }
    }
}

impl PromoteEnv for Fake {
    fn info(&self, id: &str) -> Result<Info, String> {
        let d = self.device.lock().unwrap();
        let name = d.names.get(id).ok_or("unknown computer")?.clone();
        Ok(Info {
            id: id.into(),
            name,
            os: self.os,
            running: d.running.contains(id),
            busy: d.busy.get(id).cloned(),
        })
    }

    fn name_in_use(&self, name: &str) -> bool {
        self.device
            .lock()
            .unwrap()
            .names
            .values()
            .any(|n| n == name)
    }

    fn stamp(&self) -> String {
        "20261105-1432".into()
    }

    fn checkpoint(&self, id: &str, name: &str) -> Result<CheckpointRef, String> {
        let mut d = self.device.lock().unwrap();
        d.checkpoints.push((id.into(), name.into()));
        Ok(CheckpointRef {
            id: format!("cp{}", d.checkpoints.len()),
            name: name.into(),
            created_at: String::new(),
            scope: String::new(),
        })
    }

    fn stop(&self, id: &str) -> Result<(), String> {
        self.device.lock().unwrap().running.remove(id);
        Ok(())
    }

    fn hold(&self, _id: &str, _label: &str) -> Result<Box<dyn Any>, String> {
        Ok(Box::new(()))
    }

    fn export_bindings(&self, id: &str, name: &str) -> Result<Value, String> {
        let d = self.device.lock().unwrap();
        let mut out = serde_json::Map::new();
        for store in STORES {
            if let Some(value) = d.stores.get(store).and_then(|s| s.get(name)) {
                out.insert(store.into(), value.clone());
            }
        }
        Ok(Value::Object(out))
    }

    fn apply(&self, step: Step, swap: &Swap) -> Result<(), String> {
        if *self.fail_apply.lock().unwrap() == Some(step) {
            // The failure happens after part of the work, as a real one would.
            self.device
                .lock()
                .unwrap()
                .names
                .insert(swap.fork_id.clone(), "half".into());
            return Err(format!("{} failed.", step.label()));
        }
        {
            let mut d = self.device.lock().unwrap();
            match step {
                Step::Names => {
                    d.names
                        .insert(swap.target_id.clone(), swap.archive_name.clone());
                    d.names
                        .insert(swap.fork_id.clone(), swap.target_name.clone());
                }
                Step::Bindings => {
                    for store in STORES {
                        if let Some(entries) = d.stores.get_mut(store) {
                            // Main's settings stay under the name the fork now has.
                            entries.remove(&swap.fork_name);
                        }
                    }
                    d.proxy_policy.remove(&swap.fork_name);
                }
                Step::Lineage => {
                    d.source.remove(&swap.fork_id);
                    d.replaced_by
                        .insert(swap.target_id.clone(), swap.fork_id.clone());
                }
                Step::MacosIdentity => {
                    d.proxy_policy.clear();
                }
                Step::Quiet => {
                    if !d.quiet_changed.contains(&swap.fork_id) {
                        d.parked.insert(swap.fork_id.clone(), false);
                    }
                }
            }
        }
        self.maybe_crash(step, false);
        Ok(())
    }

    fn revert(&self, step: Step, swap: &Swap) -> Result<(), String> {
        {
            let mut d = self.device.lock().unwrap();
            match step {
                Step::Names => {
                    d.names
                        .insert(swap.target_id.clone(), swap.target_name.clone());
                    d.names.insert(swap.fork_id.clone(), swap.fork_name.clone());
                }
                Step::Bindings => {
                    for store in STORES {
                        if let Some(value) = swap.fork_bindings.get(store) {
                            d.stores
                                .entry(store.into())
                                .or_default()
                                .insert(swap.fork_name.clone(), value.clone());
                        }
                    }
                    d.proxy_policy.insert(swap.fork_name.clone());
                }
                Step::Lineage => {
                    if let Some(source) = &swap.fork_source {
                        d.source.insert(swap.fork_id.clone(), source.clone());
                    }
                    d.replaced_by.remove(&swap.target_id);
                }
                Step::MacosIdentity => {
                    d.proxy_policy.insert(swap.target_name.clone());
                    d.proxy_policy.insert(swap.fork_name.clone());
                }
                Step::Quiet => {
                    if !d.quiet_changed.contains(&swap.fork_id) {
                        d.parked.insert(swap.fork_id.clone(), true);
                    }
                }
            }
        }
        self.maybe_crash(step, true);
        Ok(())
    }
}

struct Source;
impl ManifestStore for Source {
    fn fork_time(&self, _: &str) -> Result<String, String> {
        Ok(String::new())
    }
    fn capture(&self, _: &str) -> Result<String, String> {
        Ok(String::new())
    }
    fn source(&self, fork: &str) -> Result<Option<String>, String> {
        Ok((fork == FORK).then(|| MAIN.to_owned()))
    }
}

fn request() -> Request {
    Request {
        fork: FORK.into(),
        target: None,
        dry_run: false,
    }
}

fn scratch_journal() -> (tempfile::TempDir, Journal) {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path().join("promote"));
    (dir, journal)
}

fn run(fake: &Fake, journal: &Journal, request: &Request) -> Result<Outcome, String> {
    promote(fake, &Source, journal, request, Duration::from_secs(1))
}

fn promoted(fake: &Fake, journal: &Journal) -> Device {
    let outcome = run(fake, journal, &request()).unwrap();
    assert!(matches!(outcome, Outcome::Promoted(_)), "{outcome:?}");
    fake.snapshot()
}

/// A device never ran, so it is compared with every computer stopped.
fn stopped(mut device: Device) -> Device {
    device.running.clear();
    device
}

#[test]
fn a_promote_moves_every_name_keyed_item_to_the_fork() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    let before = fake.snapshot();
    let after = promoted(&fake, &journal);

    assert_eq!(after.names[FORK], "main");
    assert_eq!(after.names[MAIN], "main-before-20261105-1432");
    for store in STORES {
        let entries = &after.stores[store];
        assert_eq!(
            entries["main"], before.stores[store]["main"],
            "{store}: main's settings stay with the name"
        );
        assert!(
            !entries.contains_key("try-it"),
            "{store}: the fork's own grants are dropped"
        );
        assert!(
            !entries.contains_key("main-before-20261105-1432"),
            "{store}: the archive has none"
        );
    }
    assert!(
        !after.source.contains_key(FORK),
        "the fork is no longer a fork"
    );
    assert_eq!(after.replaced_by[MAIN], FORK);
    assert!(after.running.is_empty(), "both computers are stopped");
    assert_eq!(
        after.checkpoints,
        vec![(MAIN.to_owned(), "Before promote of try-it".to_owned())]
    );
    assert!(!after.proxy_policy.contains("try-it"));
    let record = &journal.all()[0];
    assert_eq!((record.state, record.steps.len()), (State::Promoted, 4));
}

#[test]
fn the_archive_name_avoids_a_taken_name() {
    let fake = Fake::new(Os::Linux);
    fake.device
        .lock()
        .unwrap()
        .names
        .insert("other".into(), "main-before-20261105-1432".into());
    let (_dir, journal) = scratch_journal();
    promoted(&fake, &journal);
    assert_eq!(fake.name_of(MAIN), "main-before-20261105-1432-2");
}

#[test]
fn undo_swaps_everything_back_exactly() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    let before = stopped(fake.snapshot());
    promoted(&fake, &journal);
    // Whichever of the two the user names finds the record.
    for id in [FORK, MAIN] {
        let fake = Fake::new(Os::Linux);
        let (_dir, journal) = scratch_journal();
        promoted(&fake, &journal);
        fake.device.lock().unwrap().running.insert(FORK.into());
        let undone = undo(fake.as_ref(), &journal, id, Duration::from_secs(1))
            .unwrap()
            .unwrap();
        assert_eq!(
            (undone.computer.as_str(), undone.archive.as_str()),
            ("main", "main-before-20261105-1432")
        );
        let mut now = fake.snapshot();
        now.checkpoints.clear();
        let mut expected = before.clone();
        expected.checkpoints.clear();
        expected.replaced_by.clear();
        now.replaced_by.clear();
        assert_eq!(now, expected, "undo by {id}");
        assert_eq!(journal.all()[0].state, State::Over);
        // A second undo finds nothing to swap back.
        assert!(undo(fake.as_ref(), &journal, id, Duration::from_secs(1))
            .unwrap()
            .is_none());
    }
}

#[test]
fn a_busy_computer_refuses_and_nothing_changes() {
    for busy in [MAIN, FORK] {
        let fake = Fake::new(Os::Linux);
        fake.device
            .lock()
            .unwrap()
            .busy
            .insert(busy.into(), "a checkpoint is being saved".into());
        let (_dir, journal) = scratch_journal();
        let before = fake.snapshot();
        let Outcome::Refused {
            reason, message, ..
        } = run(&fake, &journal, &request()).unwrap()
        else {
            panic!("not refused")
        };
        assert_eq!(reason, "busy");
        assert!(message.contains("checkpoint is being saved"), "{message}");
        assert_eq!(fake.snapshot(), before, "no checkpoint, no stop, no rename");
        assert!(journal.all().is_empty());
    }
}

#[test]
fn a_dry_run_reports_the_archive_name_and_changes_nothing() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    let before = fake.snapshot();
    let outcome = run(
        &fake,
        &journal,
        &Request {
            dry_run: true,
            ..request()
        },
    )
    .unwrap();
    let Outcome::DryRun(report) = outcome else {
        panic!("not a dry run")
    };
    assert_eq!(report.archive, "main-before-20261105-1432");
    assert_eq!(
        report.steps,
        ["names", "name-keyed settings", "lineage", "quiet parking"]
    );
    assert_eq!(fake.snapshot(), before);
}

#[test]
fn a_computer_cannot_replace_a_different_system_or_itself() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    let result = run(
        &fake,
        &journal,
        &Request {
            fork: FORK.into(),
            target: Some(FORK.into()),
            dry_run: false,
        },
    );
    assert!(result.unwrap_err().contains("cannot replace itself"));
}

#[test]
fn a_failing_step_rolls_every_step_back() {
    for step in [Step::Names, Step::Bindings, Step::Lineage] {
        let fake = Fake::new(Os::Linux);
        *fake.fail_apply.lock().unwrap() = Some(step);
        let (_dir, journal) = scratch_journal();
        let before = stopped(fake.snapshot());
        let error = run(&fake, &journal, &request()).unwrap_err();
        assert!(error.contains("rolled back"), "{error}");
        let mut now = fake.snapshot();
        now.checkpoints.clear();
        let mut expected = before;
        expected.checkpoints.clear();
        assert_eq!(now, expected, "{step:?}");
        assert_eq!(journal.all()[0].state, State::Over);
    }
}

#[test]
fn a_failing_quiet_step_does_not_fail_the_promote() {
    let fake = Fake::new(Os::Linux);
    *fake.fail_apply.lock().unwrap() = Some(Step::Quiet);
    let (_dir, journal) = scratch_journal();
    let Outcome::Promoted(report) = run(&fake, &journal, &request()).unwrap() else {
        panic!()
    };
    assert!(report
        .notes
        .iter()
        .any(|n| n.contains("Quiet parking was not restored")));
}

#[test]
fn a_crash_at_every_step_is_rolled_forward_or_back_at_start() {
    let expected_forward = {
        let fake = Fake::new(Os::Linux);
        let (_dir, journal) = scratch_journal();
        promoted(&fake, &journal)
    };
    let initial = stopped(Fake::new(Os::Linux).snapshot());
    for (step, forward) in [
        (Step::Names, false),
        (Step::Bindings, false),
        (Step::Lineage, false),
        (Step::Quiet, true),
    ] {
        let fake = Fake::new(Os::Linux);
        fake.crash_after(step, false);
        let (_dir, journal) = scratch_journal();
        let crashed = catch_unwind(AssertUnwindSafe(|| run(&fake, &journal, &request())));
        assert!(crashed.is_err(), "{step:?} should have stopped the engine");
        assert_eq!(journal.interrupted().len(), 1, "{step:?}");

        let failures = recover_all(fake.as_ref(), &journal);
        assert!(failures.is_empty(), "{failures:?}");
        assert!(journal.interrupted().is_empty());
        let mut now = fake.snapshot();
        let mut want = if forward {
            expected_forward.clone()
        } else {
            initial.clone()
        };
        now.checkpoints.clear();
        want.checkpoints.clear();
        assert_eq!(now, want, "crash after {step:?}");
        let state = journal.all()[0].state;
        assert_eq!(
            state,
            if forward {
                State::Promoted
            } else {
                State::Over
            },
            "{step:?}"
        );
    }
}

#[test]
fn a_crash_before_any_step_leaves_the_device_as_it_was() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    let record = super::journal::Record {
        swap: Swap {
            fork_id: FORK.into(),
            fork_name: "try-it".into(),
            target_id: MAIN.into(),
            target_name: "main".into(),
            archive_name: "main-before-x".into(),
            macos: false,
            fork_bindings: fake.export_bindings(FORK, "try-it").unwrap(),
            target_bindings: Value::Null,
            fork_source: Some(MAIN.into()),
            checkpoint: CheckpointRef {
                id: "cp".into(),
                name: "n".into(),
                created_at: String::new(),
                scope: String::new(),
            },
        },
        state: State::Promoting,
        steps: Vec::new(),
    };
    journal.save(&record).unwrap();
    let before = fake.snapshot();
    assert!(recover_all(fake.as_ref(), &journal).is_empty());
    assert_eq!(fake.snapshot(), before);
    assert_eq!(journal.all()[0].state, State::Over);
}

#[test]
fn a_crash_during_an_undo_is_finished_at_start() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    let before = stopped(fake.snapshot());
    promoted(&fake, &journal);
    fake.crash_after(Step::Bindings, true);
    let crashed = catch_unwind(AssertUnwindSafe(|| {
        undo(fake.as_ref(), &journal, FORK, Duration::from_secs(1))
    }));
    assert!(crashed.is_err());
    assert_eq!(journal.interrupted()[0].state, State::Undoing);
    assert!(recover_all(fake.as_ref(), &journal).is_empty());
    let mut now = fake.snapshot();
    now.checkpoints.clear();
    now.replaced_by.clear();
    let mut want = before;
    want.checkpoints.clear();
    assert_eq!(now, want);
}

#[test]
fn the_next_promote_settles_an_interrupted_one_first() {
    let fake = Fake::new(Os::Linux);
    fake.crash_after(Step::Names, false);
    let (_dir, journal) = scratch_journal();
    assert!(catch_unwind(AssertUnwindSafe(|| run(&fake, &journal, &request()))).is_err());
    // Names were swapped and not marked; the next call rolls that back, then promotes.
    let outcome = run(&fake, &journal, &request()).unwrap();
    assert!(matches!(outcome, Outcome::Promoted(_)));
    assert_eq!(fake.name_of(FORK), "main");
    assert_eq!(fake.name_of(MAIN), "main-before-20261105-1432");
}

#[test]
fn quiet_parking_comes_back_unless_the_fork_changed_it() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    assert!(fake.snapshot().parked[FORK]);
    assert!(!promoted(&fake, &journal).parked[FORK], "restored");

    let fake = Fake::new(Os::Linux);
    fake.device
        .lock()
        .unwrap()
        .quiet_changed
        .insert(FORK.into());
    let (_dir, journal) = scratch_journal();
    assert!(
        promoted(&fake, &journal).parked[FORK],
        "the fork's own change is kept"
    );
}

#[test]
fn on_macos_the_proxy_identity_stays_with_the_computer_and_the_name_keyed_cache_resets() {
    let fake = Fake::new(Os::Macos);
    let (_dir, journal) = scratch_journal();
    let before = fake.snapshot();
    let after = promoted(&fake, &journal);
    assert_eq!(
        after.ca, before.ca,
        "the proxy CA is keyed by computer id and does not move"
    );
    assert!(
        after.proxy_policy.is_empty(),
        "cached proxy policy keyed by name is dropped"
    );
    assert!(journal.all()[0].swap.macos);
    assert_eq!(journal.all()[0].steps.len(), 5);
    undo(fake.as_ref(), &journal, FORK, Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let now = fake.snapshot();
    assert_eq!(now.ca, before.ca);
    assert!(now.proxy_policy.contains("main") && now.proxy_policy.contains("try-it"));
}
