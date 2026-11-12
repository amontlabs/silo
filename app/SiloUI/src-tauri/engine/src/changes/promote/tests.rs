//! Promote and undo against a fake device: names, name-keyed stores, lineage, quiet parking and
//! the macOS proxy state are plain maps, so a test can compare the whole device before a
//! promote and after its undo, and stop the "engine" (a panic) between any two steps.

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use serde_json::{json, Value};

use super::{
    journal::{Journal, State, Step, Swap},
    lineage::Lineage,
    promote, promote_is_newest, recover_all, recover_one, undo, Info, Outcome, PromoteEnv, Request,
};
use crate::changes::keep::{lane, CheckpointRef, ManifestStore, Os};
use crate::policy::{
    handoff::Handoff,
    store::{Created, Document, OwnedJob, Pending},
};

const MAIN: &str = "id-main";
const FORK: &str = "id-fork";
const STORES: [&str; 4] = ["secrets", "github", "urls", "ssh"];

#[derive(Clone, Debug, Default, PartialEq)]
struct Device {
    names: BTreeMap<String, String>,
    running: BTreeSet<String>,
    /// Computers that were asked to stop and still report running.
    stopping: BTreeSet<String>,
    /// A stop takes until the next `settle` to finish.
    slow_stop: bool,
    /// A stop never finishes.
    never_stops: bool,
    busy: BTreeMap<String, String>,
    /// store -> computer name -> what it holds
    stores: BTreeMap<String, BTreeMap<String, Value>>,
    /// computer id -> published ports (id-keyed)
    ports: BTreeMap<String, Value>,
    /// SSH entry (named after a computer id) -> the computer it connects to
    alias: BTreeMap<String, String>,
    /// computer id -> hostname on its disk
    hostname: BTreeMap<String, String>,
    /// computer id -> the proxy CA it had before it carried main's
    ca_aside: BTreeMap<String, String>,
    /// computer id -> quiet parking still in place
    parked: BTreeMap<String, bool>,
    /// computer id -> the fork changed what was parked
    quiet_changed: BTreeSet<String>,
    /// computer id -> the proxy CA it presents
    ca: BTreeMap<String, String>,
    /// computer name -> cached proxy policy
    proxy_policy: BTreeSet<String>,
    checkpoints: Vec<(String, String)>,
    /// Which computer's agent made which, with the pending computers and jobs it started.
    makers: Document,
}

/// One promote test at a time (a test may hold several fakes): the lanes they take are keyed
/// by the shared computer ids.
struct Serial;

static SERIAL: (Mutex<(Option<std::thread::ThreadId>, usize)>, Condvar) =
    (Mutex::new((None, 0)), Condvar::new());

impl Serial {
    fn take() -> Self {
        let me = std::thread::current().id();
        let mut state = SERIAL.0.lock().unwrap_or_else(|e| e.into_inner());
        while state.0.is_some_and(|owner| owner != me) {
            state = SERIAL.1.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        *state = (Some(me), state.1 + 1);
        Serial
    }
}

impl Drop for Serial {
    fn drop(&mut self) {
        let mut state = SERIAL.0.lock().unwrap_or_else(|e| e.into_inner());
        state.1 -= 1;
        if state.1 == 0 {
            state.0 = None;
            SERIAL.1.notify_all();
        }
    }
}

struct Fake {
    /// Promote and undo take lanes keyed by computer id, shared by every test in the process.
    _serial: Serial,
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
            device
                .ports
                .insert(id.into(), json!([format!("port-of-{id}")]));
            device.alias.insert(id.into(), id.into());
            device.hostname.insert(id.into(), format!("host-{id}"));
        }
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
        let made = |id: &str, by: &str| Created {
            id: id.into(),
            created_by: by.into(),
            at: 7,
        };
        let job = |id: &str, computer: Option<&str>| OwnedJob {
            caller: MAIN.into(),
            id: id.into(),
            at: 9,
            computer: computer.map(str::to_owned),
        };
        device.makers = Document {
            version: 1,
            created: vec![
                made(FORK, MAIN),
                made("kid", MAIN),
                made("grandkid", "kid"),
                made("fork-kid", FORK),
            ],
            pending: vec![Pending {
                created_by: MAIN.into(),
                name: "coming".into(),
                id: None,
                source: None,
                job_id: Some("job:coming".into()),
                at: 8,
            }],
            jobs: vec![
                job("job:kid", Some("kid")),
                job("job:coming", None),
                job("job:fork", Some(FORK)),
                job("job:self", Some(MAIN)),
                job("job:unknown", None),
            ],
            kept: Vec::new(),
        };
        Arc::new(Self {
            _serial: Serial::take(),
            os,
            device: Mutex::new(device),
            crash_after: Mutex::default(),
            fail_apply: Mutex::default(),
        })
    }

    fn snapshot(&self) -> Device {
        let mut device = self.device.lock().unwrap().clone();
        device.makers.created.sort_by(|a, b| a.id.cmp(&b.id));
        device.makers.pending.sort_by(|a, b| a.name.cmp(&b.name));
        device.makers.jobs.sort_by(|a, b| a.id.cmp(&b.id));
        device
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
        let mut d = self.device.lock().unwrap();
        if d.slow_stop {
            d.stopping.insert(id.into());
        } else {
            d.running.remove(id);
        }
        Ok(())
    }

    fn start(&self, id: &str) -> Result<(), String> {
        self.device.lock().unwrap().running.insert(id.into());
        Ok(())
    }

    fn settle(&self) {
        // A computer that was asked to stop finishes stopping the next time it is looked at.
        let mut d = self.device.lock().unwrap();
        if d.never_stops {
            return;
        }
        let done: Vec<String> = d.stopping.iter().cloned().collect();
        for id in done {
            d.running.remove(&id);
            d.stopping.remove(&id);
        }
    }

    fn hold(&self, _id: &str, _label: &str) -> Result<Box<dyn Any>, String> {
        Ok(Box::new(()))
    }

    fn export_bindings(&self, id: &str, name: &str) -> Result<Value, String> {
        let d = self.device.lock().unwrap();
        let mut out = serde_json::Map::new();
        out.insert(
            "ports".into(),
            d.ports.get(id).cloned().unwrap_or(Value::Null),
        );
        out.insert(
            "hostnames".into(),
            json!(d.hostname.get(id).cloned().unwrap_or_default()),
        );
        for store in STORES {
            if let Some(value) = d.stores.get(store).and_then(|s| s.get(name)) {
                out.insert(store.into(), value.clone());
            }
        }
        Ok(Value::Object(out))
    }

    fn export_makers(&self, fork_id: &str, target_id: &str) -> Result<Handoff, String> {
        Ok(self
            .device
            .lock()
            .unwrap()
            .makers
            .handoff(fork_id, target_id))
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
                    let main_ports = d.ports.get(&swap.target_id).cloned().unwrap();
                    d.ports.insert(swap.fork_id.clone(), main_ports);
                    d.ports.insert(swap.target_id.clone(), json!([]));
                }
                Step::Alias => {
                    d.alias.insert(swap.target_id.clone(), swap.fork_id.clone());
                    d.alias.insert(swap.fork_id.clone(), swap.target_id.clone());
                }
                Step::Lineage => {
                    d.makers
                        .hand_over(&swap.fork_id, &swap.target_id, &swap.makers);
                }
                Step::MacosIdentity => {
                    d.proxy_policy.clear();
                    let main_ca = d.ca[&swap.target_id].clone();
                    if !d.ca_aside.contains_key(&swap.fork_id) {
                        let own = d.ca[&swap.fork_id].clone();
                        d.ca_aside.insert(swap.fork_id.clone(), own);
                    }
                    d.ca.insert(swap.fork_id.clone(), main_ca);
                    let (main_host, fork_host) = (
                        swap.target_bindings["hostnames"]
                            .as_str()
                            .unwrap()
                            .to_owned(),
                        swap.fork_bindings["hostnames"].as_str().unwrap().to_owned(),
                    );
                    d.hostname.insert(swap.fork_id.clone(), main_host);
                    d.hostname.insert(swap.target_id.clone(), fork_host);
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
                    // Ports changed while promoted stay with the main role.
                    let mine = swap.fork_bindings["ports"].clone();
                    let now = d.ports[&swap.fork_id].clone();
                    if now != mine {
                        d.ports.insert(swap.target_id.clone(), now);
                    }
                    d.ports.insert(swap.fork_id.clone(), mine);
                }
                Step::Alias => {
                    d.alias
                        .insert(swap.target_id.clone(), swap.target_id.clone());
                    d.alias.insert(swap.fork_id.clone(), swap.fork_id.clone());
                }
                Step::Lineage => {
                    d.makers
                        .hand_back(&swap.fork_id, &swap.target_id, &swap.makers);
                }
                Step::MacosIdentity => {
                    d.proxy_policy.insert(swap.target_name.clone());
                    d.proxy_policy.insert(swap.fork_name.clone());
                    if let Some(own) = d.ca_aside.remove(&swap.fork_id) {
                        d.ca.insert(swap.fork_id.clone(), own);
                    }
                    let (main_host, fork_host) = (
                        swap.target_bindings["hostnames"]
                            .as_str()
                            .unwrap()
                            .to_owned(),
                        swap.fork_bindings["hostnames"].as_str().unwrap().to_owned(),
                    );
                    d.hostname.insert(swap.fork_id.clone(), fork_host);
                    d.hostname.insert(swap.target_id.clone(), main_host);
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

/// The sources the stores recorded when the forks were made (they never change).
struct Source(BTreeMap<String, String>);

impl Source {
    fn standard() -> Self {
        Self([(FORK.to_owned(), MAIN.to_owned())].into())
    }
}

impl ManifestStore for Source {
    fn fork_time(&self, _: &str) -> Result<String, String> {
        Ok(String::new())
    }
    fn capture(&self, _: &str) -> Result<String, String> {
        Ok(String::new())
    }
    fn source(&self, fork: &str) -> Result<Option<String>, String> {
        Ok(self.0.get(fork).cloned())
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
    promote(
        fake,
        &Lineage::new(Arc::new(Source::standard()), journal.clone()),
        journal,
        request,
        Duration::from_secs(1),
    )
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
    assert_eq!(
        (after.ports[FORK].clone(), after.ports[MAIN].clone()),
        (before.ports[MAIN].clone(), json!([])),
        "main's published ports move to the new main"
    );
    assert_eq!(
        (after.alias[MAIN].as_str(), after.alias[FORK].as_str()),
        (FORK, MAIN),
        "main's SSH entry reaches the new main"
    );
    assert_eq!(
        after.running,
        [FORK.to_owned()].into(),
        "the new main runs again and the archive stays stopped"
    );
    assert_eq!(
        after.checkpoints,
        vec![(MAIN.to_owned(), "Before promote of try-it".to_owned())]
    );
    assert!(!after.proxy_policy.contains("try-it"));
    let record = &journal.all()[0];
    assert_eq!((record.state, record.steps.len()), (State::Promoted, 5));
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
fn the_archive_name_stays_within_the_limit_for_any_number_of_collisions_and_an_empty_head() {
    let fake = Fake::new(Os::Linux);
    {
        let mut device = fake.device.lock().unwrap();
        let mut taken = vec!["main-before-20261105-1432".to_owned()];
        for n in 2..1500 {
            taken.push(format!("main-before-20261105-1432-{n}"));
        }
        for (index, name) in taken.into_iter().enumerate() {
            device.names.insert(format!("other-{index}"), name);
        }
    }
    let name = super::archive_name(&*fake, "a-long-computer-name-here");
    assert!(name.len() <= 32, "{name}");
    assert!(!fake.name_in_use(&name), "{name}");
    for empty in ["", "---"] {
        let name = super::archive_name(&*fake, empty);
        assert!(
            name.len() <= 32 && !name.starts_with('-') && name.contains("-before-"),
            "{name}"
        );
    }
}

#[test]
fn the_archive_name_of_a_long_name_is_still_a_valid_computer_name() {
    let fake = Fake::new(Os::Linux);
    for name in [
        "e2e-p5fix-main",
        "a-computer-with-a-name-of-exactly-32",
        "x--------------------------------",
    ] {
        let archive = super::archive_name(&*fake, name);
        assert!(archive.len() <= 32, "{archive}");
        assert!(archive.ends_with("-before-20261105-1432"), "{archive}");
        assert!(!archive.starts_with('-'), "{archive}");
    }
    assert_eq!(
        super::archive_name(&*fake, "main"),
        "main-before-20261105-1432"
    );
}

#[test]
fn undo_swaps_everything_back_exactly() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    let before = fake.snapshot();
    promoted(&fake, &journal);
    // Whichever of the two the user names finds the record.
    for id in [FORK, MAIN] {
        let fake = Fake::new(Os::Linux);
        let (_dir, journal) = scratch_journal();
        promoted(&fake, &journal);
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
        assert_eq!(now, expected, "undo by {id}");
        assert_eq!(journal.all()[0].state, State::Over);
        // A second undo finds nothing to swap back.
        assert!(undo(fake.as_ref(), &journal, id, Duration::from_secs(1))
            .unwrap()
            .is_none());
    }
}

#[test]
fn an_undo_or_promote_whose_pin_moved_under_the_lanes_changes_nothing() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    promoted(&fake, &journal);
    let before = fake.snapshot();
    let moved = || Err("the pin moved".to_owned());
    assert_eq!(
        super::undo_checked(fake.as_ref(), &journal, FORK, Duration::from_secs(1), &moved).unwrap_err(),
        "the pin moved"
    );
    assert_eq!(fake.snapshot(), before);
    assert_eq!(journal.all()[0].state, State::Promoted);
    let fresh = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    let before = fresh.snapshot();
    assert_eq!(
        super::promote_checked(
            fresh.as_ref(),
            &Lineage::new(Arc::new(Source::standard()), journal.clone()),
            &journal,
            &request(),
            Duration::from_secs(1),
            &moved
        )
        .unwrap_err(),
        "the pin moved"
    );
    assert_eq!(fresh.snapshot(), before);
}

#[test]
fn a_promote_waits_for_both_computers_to_finish_stopping() {
    let fake = Fake::new(Os::Linux);
    fake.device.lock().unwrap().slow_stop = true;
    let (_dir, journal) = scratch_journal();
    let after = promoted(&fake, &journal);
    assert!(!after.running.contains(MAIN) && after.stopping.is_empty());
    assert_eq!(after.names[FORK], "main");
}

#[test]
fn a_computer_that_never_finishes_stopping_refuses_the_promote() {
    let fake = Fake::new(Os::Linux);
    {
        let mut d = fake.device.lock().unwrap();
        d.slow_stop = true;
        d.never_stops = true;
    }
    let (_dir, journal) = scratch_journal();
    let before = fake.snapshot();
    let error = run(&fake, &journal, &request()).unwrap_err();
    assert!(error.contains("did not finish stopping"), "{error}");
    assert_eq!(fake.snapshot().names, before.names);
}

#[test]
fn undo_starts_again_the_computers_that_were_running_before_the_promote() {
    for (main_ran, fork_ran) in [(true, true), (false, true), (true, false), (false, false)] {
        let fake = Fake::new(Os::Linux);
        fake.device.lock().unwrap().slow_stop = true;
        {
            let mut d = fake.device.lock().unwrap();
            if !main_ran {
                d.running.remove(MAIN);
            }
            if !fork_ran {
                d.running.remove(FORK);
            }
        }
        let (_dir, journal) = scratch_journal();
        promoted(&fake, &journal);
        assert_eq!(fake.snapshot().running.contains(FORK), main_ran);
        assert!(!fake.snapshot().running.contains(MAIN));
        undo(fake.as_ref(), &journal, MAIN, Duration::from_secs(1))
            .unwrap()
            .unwrap();
        let running = fake.snapshot().running;
        assert_eq!(
            running.contains(MAIN),
            main_ran,
            "main {main_ran}/{fork_ran}"
        );
        assert_eq!(
            running.contains(FORK),
            fork_ran,
            "fork {main_ran}/{fork_ran}"
        );
    }
}

#[test]
fn a_promote_leaves_the_new_main_in_the_running_state_of_the_old_one() {
    for os in [Os::Linux, Os::Macos] {
        for (main_ran, fork_ran) in [(true, true), (true, false), (false, true), (false, false)] {
            let fake = Fake::new(os);
            {
                let mut d = fake.device.lock().unwrap();
                if !main_ran {
                    d.running.remove(MAIN);
                }
                if !fork_ran {
                    d.running.remove(FORK);
                }
            }
            let (_dir, journal) = scratch_journal();
            let after = promoted(&fake, &journal);
            assert_eq!(after.running.contains(FORK), main_ran, "{os:?} {main_ran}");
            assert!(!after.running.contains(MAIN), "the archive stays stopped");
        }
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
        [
            "names",
            "name-keyed settings",
            "SSH access",
            "lineage",
            "quiet parking"
        ]
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
    for step in [Step::Names, Step::Bindings, Step::Alias, Step::Lineage] {
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
        (Step::Alias, false),
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
        // Recovery at engine start leaves every computer stopped.
        want.running.clear();
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
            target_bindings: fake.export_bindings(MAIN, "main").unwrap(),
            fork_source: Some(MAIN.into()),
            target_source: None,
            checkpoint: CheckpointRef {
                id: "cp".into(),
                name: "n".into(),
                created_at: String::new(),
                scope: String::new(),
            },
            fork_was_running: false,
            target_was_running: false,
            makers: Default::default(),
        },
        state: State::Promoting,
        at: 1,
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
fn on_macos_the_new_main_presents_mains_proxy_ca_and_hostname() {
    let fake = Fake::new(Os::Macos);
    let (_dir, journal) = scratch_journal();
    let before = fake.snapshot();
    let after = promoted(&fake, &journal);
    assert_eq!(
        after.ca[FORK], before.ca[MAIN],
        "the new main presents main's CA"
    );
    assert_eq!(after.ca[MAIN], before.ca[MAIN], "the archive keeps its own");
    assert_eq!(after.hostname[FORK], before.hostname[MAIN]);
    assert_eq!(after.hostname[MAIN], before.hostname[FORK]);
    assert!(
        after.proxy_policy.is_empty(),
        "cached proxy policy keyed by name is dropped"
    );
    assert!(journal.all()[0].swap.macos);
    assert_eq!(journal.all()[0].steps.len(), 6);
    undo(fake.as_ref(), &journal, FORK, Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let now = fake.snapshot();
    assert_eq!(now.ca, before.ca);
    assert_eq!(now.hostname, before.hostname);
    assert!(now.ca_aside.is_empty());
    assert!(now.proxy_policy.contains("main") && now.proxy_policy.contains("try-it"));
}

#[test]
fn a_crash_while_carrying_the_ca_keeps_the_forks_own_for_the_undo() {
    let fake = Fake::new(Os::Macos);
    fake.crash_after(Step::MacosIdentity, false);
    let (_dir, journal) = scratch_journal();
    let before = stopped(fake.snapshot());
    assert!(catch_unwind(AssertUnwindSafe(|| run(&fake, &journal, &request()))).is_err());
    assert!(recover_all(fake.as_ref(), &journal).is_empty());
    let mut now = fake.snapshot();
    now.checkpoints.clear();
    let mut want = before;
    want.checkpoints.clear();
    assert_eq!(now, want);
}

#[test]
fn ports_changed_while_promoted_stay_with_the_main_role_on_undo() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    let before = fake.snapshot();
    promoted(&fake, &journal);
    fake.device
        .lock()
        .unwrap()
        .ports
        .insert(FORK.into(), json!(["added-while-main"]));
    undo(fake.as_ref(), &journal, MAIN, Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let now = fake.snapshot();
    assert_eq!(now.ports[MAIN], json!(["added-while-main"]));
    assert_eq!(now.ports[FORK], before.ports[FORK]);
}

// MARK: Nested promotes and undo order

const THIRD: &str = "id-third";

fn add_third(fake: &Fake) {
    let mut d = fake.device.lock().unwrap();
    d.names.insert(THIRD.into(), "second-try".into());
    d.running.insert(THIRD.into());
    d.ca.insert(THIRD.into(), format!("ca-{THIRD}"));
    d.ports.insert(THIRD.into(), json!(["port-of-third"]));
    d.alias.insert(THIRD.into(), THIRD.into());
    d.hostname.insert(THIRD.into(), "host-third".into());
    d.proxy_policy.insert("second-try".into());
    d.parked.insert(THIRD.into(), true);
}

fn no_duplicate_names(fake: &Fake) {
    let names: Vec<String> = fake.snapshot().names.values().cloned().collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        names.len(),
        "two computers share a name: {names:?}"
    );
}

fn promote_third(fake: &Fake, journal: &Journal) {
    let outcome = promote(
        fake,
        &Lineage::new(
            Arc::new(Source([(THIRD.to_owned(), FORK.to_owned())].into())),
            journal.clone(),
        ),
        journal,
        &Request {
            fork: THIRD.into(),
            target: None,
            dry_run: false,
        },
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(matches!(outcome, Outcome::Promoted(_)), "{outcome:?}");
}

#[test]
fn an_undo_never_reverts_a_promote_that_a_later_one_built_on() {
    let fake = Fake::new(Os::Linux);
    add_third(&fake);
    let (_dir, journal) = scratch_journal();
    let before = stopped(fake.snapshot());
    promoted(&fake, &journal);
    // The first fork is main; a fork of it replaces it in turn.
    let journal_for_third = journal.clone();
    {
        let mut d = fake.device.lock().unwrap();
        d.running.insert(THIRD.into());
    }
    promote_third(&fake, &journal_for_third);
    no_duplicate_names(&fake);
    assert_eq!(fake.name_of(THIRD), "main");
    assert_eq!(fake.name_of(FORK), "main-before-20261105-1432-2");

    // The archive of the first promote cannot swap back under the second.
    let after_both = fake.snapshot();
    let error = undo(fake.as_ref(), &journal, MAIN, Duration::from_secs(1)).unwrap_err();
    assert!(error.contains("Undo that one first"), "{error}");
    assert_eq!(fake.snapshot(), after_both, "nothing changed");
    no_duplicate_names(&fake);

    // Undoing in order puts everything back, never with two computers named main.
    undo(fake.as_ref(), &journal, THIRD, Duration::from_secs(1))
        .unwrap()
        .unwrap();
    no_duplicate_names(&fake);
    assert_eq!(fake.name_of(FORK), "main");
    undo(fake.as_ref(), &journal, MAIN, Duration::from_secs(1))
        .unwrap()
        .unwrap();
    no_duplicate_names(&fake);
    let mut now = fake.snapshot();
    now.checkpoints.clear();
    let mut want = before;
    want.checkpoints.clear();
    // Everything that ran before each promote runs again.
    want.running = [MAIN, FORK, THIRD].map(String::from).into();
    assert_eq!(now, want);
}

#[test]
fn an_undo_by_the_fork_that_holds_main_picks_the_latest_promote() {
    let fake = Fake::new(Os::Linux);
    add_third(&fake);
    let (_dir, journal) = scratch_journal();
    promoted(&fake, &journal);
    promote_third(&fake, &journal);
    // FORK is in both records; the latest one (third over it) is the one to undo.
    let undone = undo(fake.as_ref(), &journal, FORK, Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(undone.fork, "second-try");
    assert_eq!(fake.name_of(THIRD), "second-try");
    assert_eq!(fake.name_of(FORK), "main");
}

#[test]
fn an_undo_refuses_when_the_promoted_computers_were_renamed_or_a_name_was_taken() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    promoted(&fake, &journal);
    let promoted_state = fake.snapshot();

    // The user renamed the new main.
    fake.device
        .lock()
        .unwrap()
        .names
        .insert(FORK.into(), "renamed".into());
    let renamed = fake.snapshot();
    let error = undo(fake.as_ref(), &journal, FORK, Duration::from_secs(1)).unwrap_err();
    assert!(error.contains("renamed"), "{error}");
    assert_eq!(fake.snapshot(), renamed);

    // Back to the promoted names, but another computer took the fork's old name.
    fake.device
        .lock()
        .unwrap()
        .names
        .insert(FORK.into(), "main".into());
    fake.device
        .lock()
        .unwrap()
        .names
        .insert("id-other".into(), "try-it".into());
    let error = undo(fake.as_ref(), &journal, FORK, Duration::from_secs(1)).unwrap_err();
    assert!(
        error.contains("Another computer is named try-it"),
        "{error}"
    );
    no_duplicate_names(&fake);
    let mut expected = promoted_state;
    expected.names.insert("id-other".into(), "try-it".into());
    assert_eq!(fake.snapshot(), expected);
    assert_eq!(journal.all()[0].state, State::Promoted);
}

#[test]
fn a_fork_with_main_s_name_is_not_promoted() {
    let fake = Fake::new(Os::Linux);
    fake.device
        .lock()
        .unwrap()
        .names
        .insert(FORK.into(), "Main".into());
    let (_dir, journal) = scratch_journal();
    let before = fake.snapshot();
    let Outcome::Refused { reason, .. } = run(&fake, &journal, &request()).unwrap() else {
        panic!("not refused")
    };
    assert_eq!(reason, "name");
    assert_eq!(fake.snapshot(), before);
}

#[test]
fn undo_takes_the_newest_of_a_promote_and_a_keep() {
    // (promote, keep, interrupted keep) -> whether the promote is undone
    for (promoted, kept, pending, expected) in [
        (Some(5), None, false, true),
        (Some(5), Some(3), false, true),
        (Some(5), Some(9), false, false),
        (Some(5), Some(3), true, false),
        (None, Some(3), false, false),
        (None, None, false, false),
    ] {
        assert_eq!(
            promote_is_newest(promoted, kept, pending),
            expected,
            "{promoted:?} {kept:?} {pending}"
        );
    }
}

// MARK: Lineage

#[test]
fn forks_of_the_old_main_resolve_to_the_new_main_and_back_after_an_undo() {
    let fake = Fake::new(Os::Linux);
    add_third(&fake);
    let (_dir, journal) = scratch_journal();
    let sources = Arc::new(Source(
        [
            (FORK.to_owned(), MAIN.to_owned()),
            (THIRD.to_owned(), MAIN.to_owned()),
            ("id-grandchild".to_owned(), "id-ancestor".to_owned()),
        ]
        .into(),
    ));
    let lineage = Lineage::new(sources.clone(), journal.clone());
    let source = |id: &str| lineage.source(id).unwrap();
    assert_eq!(source(THIRD).as_deref(), Some(MAIN));

    promote(
        fake.as_ref(),
        &lineage,
        &journal,
        &request(),
        Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(
        source(THIRD).as_deref(),
        Some(FORK),
        "a fork of the old main now belongs to the computer that is main"
    );
    assert_eq!(
        source(FORK),
        None,
        "the new main has the old main's source (none)"
    );
    assert_eq!(source("id-grandchild").as_deref(), Some("id-ancestor"));

    // `silo keep`/`fold` of the other fork now targets the new main.
    let target = lineage.source(THIRD).unwrap().unwrap();
    assert_eq!(fake.name_of(&target), "main");

    undo(fake.as_ref(), &journal, FORK, Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(source(THIRD).as_deref(), Some(MAIN));
    assert_eq!(source(FORK).as_deref(), Some(MAIN));
}

#[test]
fn the_new_main_takes_the_old_mains_own_source() {
    let fake = Fake::new(Os::Linux);
    // Main was itself forked from an earlier computer.
    let sources = Arc::new(Source(
        [
            (FORK.to_owned(), MAIN.to_owned()),
            (MAIN.to_owned(), "id-origin".to_owned()),
        ]
        .into(),
    ));
    let (_dir, journal) = scratch_journal();
    let lineage = Lineage::new(sources, journal.clone());
    promote(
        fake.as_ref(),
        &lineage,
        &journal,
        &request(),
        Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(lineage.source(FORK).unwrap().as_deref(), Some("id-origin"));
    assert_eq!(lineage.source(MAIN).unwrap().as_deref(), Some("id-origin"));
}

#[test]
fn the_ownership_check_follows_the_lineage_after_a_promote() {
    use crate::fold_requests::check_fork_belongs;
    let fake = Fake::new(Os::Linux);
    add_third(&fake);
    let sources = Arc::new(Source(
        [
            (FORK.to_owned(), MAIN.to_owned()),
            (THIRD.to_owned(), MAIN.to_owned()),
            (MAIN.to_owned(), "id-origin".to_owned()),
        ]
        .into(),
    ));
    let (_dir, journal) = scratch_journal();
    let lineage = Lineage::new(sources, journal.clone());
    let state = tempfile::tempdir().unwrap();
    let owns = |fork: &str, target: &str| {
        let source = lineage.source(fork).unwrap();
        check_fork_belongs(fork, target, source.as_deref(), state.path()).is_ok()
    };
    assert!(owns(THIRD, MAIN) && owns(FORK, MAIN));
    promote(
        fake.as_ref(),
        &lineage,
        &journal,
        &request(),
        Duration::from_secs(1),
    )
    .unwrap();
    // A fork of the old main belongs to the computer that is main now, not to the archive.
    assert!(owns(THIRD, FORK) && !owns(THIRD, MAIN));
    // The promoted fork is main: a fork of main's own source, no longer of the archive.
    assert!(owns(FORK, "id-origin") && !owns(FORK, MAIN));
}

// MARK: Recovery holds the computers

#[test]
fn recovery_holds_both_computers_until_it_is_done() {
    let fake = Fake::new(Os::Linux);
    fake.crash_after(Step::Names, false);
    let (_dir, journal) = scratch_journal();
    assert!(catch_unwind(AssertUnwindSafe(|| run(&fake, &journal, &request()))).is_err());
    let key = (FORK.to_owned(), MAIN.to_owned());

    let held = std::cell::Cell::new(false);
    recover_one(
        fake.as_ref(),
        &journal,
        &key,
        &|| {
            held.set(true);
            for id in [FORK, MAIN] {
                assert!(
                    lane::acquire(id, Duration::from_millis(30)).is_err(),
                    "{id} must be held while it is recovered"
                );
            }
        },
        1,
        Duration::ZERO,
    )
    .unwrap();
    assert!(held.get());
    assert!(journal.interrupted().is_empty());
    for id in [FORK, MAIN] {
        assert!(
            lane::acquire(id, Duration::from_millis(30)).is_ok(),
            "{id} is free again"
        );
    }
}

// MARK: The real operation gate

/// The fake with the engine's gate behind it: the stop and checkpoint take the gate the way
/// the engine's do, the holds are real admissions, and applying a step needs one held.
struct Gated {
    fake: Arc<Fake>,
    gate: &'static crate::runtime::operation_gate::OperationGate,
    /// Hold each computer in turn, as the engine's environment once did.
    one_by_one: bool,
}

impl Gated {
    fn new(fake: Arc<Fake>, one_by_one: bool) -> Self {
        Self {
            fake,
            gate: Box::leak(Box::new(
                crate::runtime::operation_gate::OperationGate::new(),
            )),
            one_by_one,
        }
    }

    fn admit(&self, id: &str, label: &str) -> Result<Box<dyn Any>, String> {
        let name = self.fake.name_of(id);
        self.gate
            .computer(id, &name, label)
            .map(|guard| Box::new(guard) as Box<dyn Any>)
            .map_err(|error| error.to_string())
    }
}

impl PromoteEnv for Gated {
    fn info(&self, id: &str) -> Result<Info, String> {
        self.fake.info(id)
    }
    fn name_in_use(&self, name: &str) -> bool {
        self.fake.name_in_use(name)
    }
    fn stamp(&self) -> String {
        self.fake.stamp()
    }
    fn checkpoint(&self, id: &str, name: &str) -> Result<CheckpointRef, String> {
        let _turn = self.admit(id, "Checkpoint")?;
        self.fake.checkpoint(id, name)
    }
    fn stop(&self, id: &str) -> Result<(), String> {
        let _turn = self.admit(id, "Stop")?;
        self.fake.stop(id)
    }
    fn hold(&self, id: &str, label: &str) -> Result<Box<dyn Any>, String> {
        self.admit(id, label)
    }
    fn hold_pair(&self, first: &str, second: &str, label: &str) -> Result<Box<dyn Any>, String> {
        if self.one_by_one {
            return Ok(Box::new((
                self.hold(first, label)?,
                self.hold(second, label)?,
            )));
        }
        self.gate
            .device(label)
            .map(|guard| Box::new(guard) as Box<dyn Any>)
            .map_err(|error| error.to_string())
    }
    fn export_bindings(&self, id: &str, name: &str) -> Result<Value, String> {
        self.fake.export_bindings(id, name)
    }
    fn export_makers(&self, fork_id: &str, target_id: &str) -> Result<Handoff, String> {
        self.fake.export_makers(fork_id, target_id)
    }
    fn apply(&self, step: Step, swap: &Swap) -> Result<(), String> {
        assert!(
            crate::runtime::operation_gate::held(),
            "{step:?} runs under a held admission"
        );
        self.fake.apply(step, swap)
    }
    fn revert(&self, step: Step, swap: &Swap) -> Result<(), String> {
        assert!(
            crate::runtime::operation_gate::held(),
            "{step:?} is undone under a held admission"
        );
        self.fake.revert(step, swap)
    }
}

fn run_gated(env: &Gated, journal: &Journal) -> Result<Outcome, String> {
    promote(
        env,
        &Lineage::new(Arc::new(Source::standard()), journal.clone()),
        journal,
        &request(),
        Duration::from_secs(1),
    )
}

#[test]
fn holding_the_computers_one_by_one_fails_the_real_gate() {
    let env = Gated::new(Fake::new(Os::Linux), true);
    let (_dir, journal) = scratch_journal();
    let error = run_gated(&env, &journal).unwrap_err();
    assert_eq!(error, "Computer operation ordering failed.");
}

#[test]
fn a_promote_and_its_undo_run_through_the_real_gate() {
    let env = Gated::new(Fake::new(Os::Linux), false);
    let mut before = env.fake.snapshot();
    let (_dir, journal) = scratch_journal();
    let outcome = run_gated(&env, &journal).unwrap();
    before.checkpoints = env.fake.snapshot().checkpoints;
    assert!(matches!(outcome, Outcome::Promoted(_)), "{outcome:?}");
    assert_eq!(env.fake.name_of(FORK), "main");
    assert!(env.gate.is_idle(), "the holds are released afterwards");

    undo(&env, &journal, FORK, Duration::from_secs(1))
        .unwrap()
        .expect("the promote is undone");
    assert_eq!(stopped(env.fake.snapshot()), stopped(before));
    assert!(env.gate.is_idle());
}

fn made_by(device: &Device, creator: &str) -> BTreeSet<String> {
    device.makers.made_by(creator).into_iter().collect()
}

fn ids(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn job_callers(device: &Device) -> BTreeMap<String, String> {
    device
        .makers
        .jobs
        .iter()
        .map(|job| (job.id.clone(), job.caller.clone()))
        .collect()
}

fn assert_moved_to_the_fork(after: &Device) {
    assert!(
        made_by(after, MAIN).is_empty(),
        "the old main makes nothing"
    );
    assert_eq!(
        made_by(after, FORK),
        ids(&["kid", "grandkid", "fork-kid"]),
        "the new main makes what the old main made, and what it made itself"
    );
    assert!(
        !after.makers.created.iter().any(|record| record.id == FORK),
        "the new main is made by no one"
    );
    assert!(
        !after.makers.created.iter().any(|record| record.id == MAIN),
        "the old main is made by no one"
    );
    assert_eq!(after.makers.pending[0].created_by, FORK);
    assert_eq!(
        job_callers(after),
        [
            ("job:kid", FORK),
            ("job:coming", FORK),
            ("job:fork", FORK),
            ("job:self", MAIN),
            ("job:unknown", MAIN),
        ]
        .map(|(job, caller)| (job.to_owned(), caller.to_owned()))
        .into(),
        "jobs about the moved computers move with them"
    );
}

#[test]
fn a_promote_gives_what_the_old_main_made_to_the_new_main() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    assert_eq!(made_by(&fake.snapshot(), MAIN).len(), 4);
    let after = promoted(&fake, &journal);
    assert_moved_to_the_fork(&after);
}

#[test]
fn an_undo_puts_back_who_made_which_exactly() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    let before = fake.snapshot().makers;
    promoted(&fake, &journal);
    undo(fake.as_ref(), &journal, FORK, Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(fake.snapshot().makers, {
        let mut want = before;
        want.created.sort_by(|a, b| a.id.cmp(&b.id));
        want.pending.sort_by(|a, b| a.name.cmp(&b.name));
        want.jobs.sort_by(|a, b| a.id.cmp(&b.id));
        want
    });
}

#[test]
fn a_computer_deleted_while_promoted_stays_deleted_after_an_undo() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    promoted(&fake, &journal);
    fake.device
        .lock()
        .unwrap()
        .makers
        .created
        .retain(|r| r.id != "kid");
    undo(fake.as_ref(), &journal, FORK, Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(!fake
        .snapshot()
        .makers
        .created
        .iter()
        .any(|record| record.id == "kid"));
}

#[test]
fn the_new_main_takes_the_old_mains_place_under_the_computer_that_made_it() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    fake.device.lock().unwrap().makers.created.push(Created {
        id: MAIN.into(),
        created_by: "outer".into(),
        at: 3,
    });
    let before = fake.snapshot().makers;
    let after = promoted(&fake, &journal);
    let record = |id: &str| after.makers.created.iter().find(|r| r.id == id);
    assert_eq!(record(FORK).map(|r| r.created_by.as_str()), Some("outer"));
    assert!(record(MAIN).is_none(), "the old main is made by no one");
    assert_eq!(
        made_by(&after, "outer"),
        ids(&[FORK, "kid", "grandkid", "fork-kid"])
    );
    assert!(made_by(&after, MAIN).is_empty());

    undo(fake.as_ref(), &journal, MAIN, Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let mut want = before;
    want.created.sort_by(|a, b| a.id.cmp(&b.id));
    want.pending.sort_by(|a, b| a.name.cmp(&b.name));
    want.jobs.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(fake.snapshot().makers, want);
}

#[test]
fn a_promote_finished_by_recovery_has_moved_who_made_which() {
    let fake = Fake::new(Os::Linux);
    fake.crash_after(Step::Quiet, false);
    let (_dir, journal) = scratch_journal();
    assert!(catch_unwind(AssertUnwindSafe(|| run(&fake, &journal, &request()))).is_err());
    assert!(recover_all(fake.as_ref(), &journal).is_empty());
    assert_eq!(journal.all()[0].state, State::Promoted);
    assert_moved_to_the_fork(&fake.snapshot());

    // The engine stopped after the step ran but before the journal marked it: the repeat
    // changes nothing more.
    let again = fake.snapshot();
    let record = journal.all().remove(0);
    fake.device
        .lock()
        .unwrap()
        .makers
        .hand_over(FORK, MAIN, &record.swap.makers);
    assert_eq!(fake.snapshot().makers, again.makers);
}

#[test]
fn a_promote_rolled_back_by_recovery_leaves_who_made_which_as_it_was() {
    let fake = Fake::new(Os::Linux);
    let before = fake.snapshot().makers;
    fake.crash_after(Step::Lineage, false);
    let (_dir, journal) = scratch_journal();
    assert!(catch_unwind(AssertUnwindSafe(|| run(&fake, &journal, &request()))).is_err());
    assert!(recover_all(fake.as_ref(), &journal).is_empty());
    assert_eq!(journal.all()[0].state, State::Over);
    let mut want = before;
    want.created.sort_by(|a, b| a.id.cmp(&b.id));
    want.pending.sort_by(|a, b| a.name.cmp(&b.name));
    want.jobs.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(fake.snapshot().makers, want);
}

#[test]
fn an_undo_finished_by_recovery_puts_back_who_made_which() {
    let fake = Fake::new(Os::Linux);
    let (_dir, journal) = scratch_journal();
    let mut want = fake.snapshot().makers;
    promoted(&fake, &journal);
    fake.crash_after(Step::Lineage, true);
    assert!(catch_unwind(AssertUnwindSafe(|| {
        undo(fake.as_ref(), &journal, FORK, Duration::from_secs(1))
    }))
    .is_err());
    assert!(recover_all(fake.as_ref(), &journal).is_empty());
    want.created.sort_by(|a, b| a.id.cmp(&b.id));
    want.pending.sort_by(|a, b| a.name.cmp(&b.name));
    want.jobs.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(fake.snapshot().makers, want);
}
