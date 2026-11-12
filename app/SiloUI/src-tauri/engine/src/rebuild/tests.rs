//! Tests for the rebuild: the refusals, the journal at every step, the rollback, the name swap
//! and the undo computer, and the recipe replay. The fake environment keeps a table of
//! sandboxes the way `msb inspect` and `msb rename` see them.

use std::{
    any::Any,
    collections::HashMap,
    fs,
    sync::{Arc, Mutex},
    time::Duration,
};

use silo_api::methods::recipes::{RecipeOs, RecipePackages, RecipeService, RecipeSpec};

use super::{
    engine_env::{
        bind_undo_policy, can_clone, clone_workspace, copy_folder, ensure_undo_policy,
        forget_build_policies, parse_du, parse_pack_report, replay_steps, save_build_policies,
        swap_with_policy, PACK_HOME, UNPACK_HOME,
    },
    journal::{Journal, Plan, Record, Step},
    rebuild, recover_one, swap, undo_name, Info, RebuildEnv,
};
use crate::{
    changes::keep::{CheckpointRef, Os},
    recipes::{self, ExecOut, RunAs},
    runtime::ComputerConfiguration,
};

const WAIT: Duration = Duration::from_millis(100);

fn spec() -> RecipeSpec {
    RecipeSpec {
        os: RecipeOs::Linux,
        cpus: None,
        memory_gib: None,
        disk_gib: None,
        packages: RecipePackages {
            apt: vec!["git".into()],
            ..RecipePackages::default()
        },
        setup: vec!["echo one".into()],
        services: vec![RecipeService {
            name: "web".into(),
            run: "python3 -m http.server 8080".into(),
        }],
        ports: Vec::new(),
        desktop: None,
    }
}

// MARK: The name swap

/// One sandbox as the runtime has it: the name it is stored under, the Silo identity it
/// carries, and which of the two this test built it as.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Sandbox {
    name: String,
    label: String,
    tag: &'static str,
}

/// The runtime's sandboxes. `fail_after` lets that many renames and relabels succeed, then
/// fails the next.
#[derive(Default)]
struct Table {
    sandboxes: Mutex<Vec<Sandbox>>,
    log: Mutex<Vec<String>>,
    fail_after: Mutex<Option<usize>>,
}

impl Table {
    fn with(sandboxes: &[(&str, &str, &'static str)]) -> Self {
        let table = Table::default();
        for (name, label, tag) in sandboxes {
            table.sandboxes.lock().unwrap().push(Sandbox {
                name: (*name).into(),
                label: (*label).into(),
                tag,
            });
        }
        table
    }

    fn step(&self, what: String) -> Result<(), String> {
        let mut left = self.fail_after.lock().unwrap();
        if let Some(count) = left.as_mut() {
            if *count == 0 {
                return Err(format!("injected failure at {what}"));
            }
            *count -= 1;
        }
        self.log.lock().unwrap().push(what);
        Ok(())
    }

    fn named(&self, name: &str) -> Option<Sandbox> {
        self.sandboxes
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.name == name)
            .cloned()
    }

    fn tags(&self, names: &[&str]) -> Vec<Option<&'static str>> {
        names
            .iter()
            .map(|name| self.named(name).map(|s| s.tag))
            .collect()
    }
}

impl swap::Sandboxes for Table {
    fn owner(&self, name: &str) -> Result<Option<String>, String> {
        Ok(self.named(name).map(|s| s.label))
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        self.step(format!("rename {from} {to}"))?;
        let mut all = self.sandboxes.lock().unwrap();
        if all.iter().any(|s| s.name == to) {
            return Err(format!("{to} exists"));
        }
        all.iter_mut()
            .find(|s| s.name == from)
            .ok_or_else(|| format!("{from} is not a sandbox"))?
            .name = to.into();
        Ok(())
    }

    fn relabel(&self, name: &str, id: &str) -> Result<(), String> {
        self.step(format!("relabel {name} {id}"))?;
        self.sandboxes
            .lock()
            .unwrap()
            .iter_mut()
            .find(|s| s.name == name)
            .ok_or_else(|| format!("{name} is not a sandbox"))?
            .label = id.into();
        Ok(())
    }
}

fn plan() -> Plan {
    Plan {
        old_id: "id-old".into(),
        old_name: "dev".into(),
        new_id: "id-new".into(),
        new_name: "silo-rebuild-aaaa1111".into(),
        undo_id: "id-undo".into(),
        undo_name: "dev-undo".into(),
        image: "image-v5".into(),
        stage: "aaaa1111".into(),
    }
}

fn before() -> Table {
    Table::with(&[
        ("dev", "id-old", "old"),
        ("silo-rebuild-aaaa1111", "id-new", "new"),
    ])
}

fn after_tags(table: &Table) -> Vec<Option<&'static str>> {
    table.tags(&["dev", "dev-undo", "silo-rebuild-aaaa1111"])
}

#[test]
fn the_rebuilt_sandbox_takes_the_name_and_id_and_the_old_one_becomes_the_undo_computer() {
    let table = before();
    swap::apply(&table, &plan()).unwrap();
    assert_eq!(after_tags(&table), [Some("new"), Some("old"), None]);
    assert_eq!(table.named("dev").unwrap().label, "id-old");
    assert_eq!(table.named("dev-undo").unwrap().label, "id-undo");
    assert_eq!(swap::state(&table, &plan()), Ok(4));
    // Repeating it changes nothing.
    let done = table.log.lock().unwrap().len();
    swap::apply(&table, &plan()).unwrap();
    assert_eq!(table.log.lock().unwrap().len(), done);
}

#[test]
fn a_swap_stopped_after_any_change_is_finished_or_reverted() {
    for stop_after in 0..4 {
        let table = before();
        *table.fail_after.lock().unwrap() = Some(stop_after);
        assert!(swap::apply(&table, &plan()).is_err(), "stop {stop_after}");
        *table.fail_after.lock().unwrap() = None;
        assert_eq!(swap::state(&table, &plan()), Ok(stop_after as u8));
        swap::apply(&table, &plan()).unwrap();
        assert_eq!(after_tags(&table), [Some("new"), Some("old"), None]);

        let table = before();
        *table.fail_after.lock().unwrap() = Some(stop_after);
        let _ = swap::apply(&table, &plan());
        *table.fail_after.lock().unwrap() = None;
        swap::revert(&table, &plan()).unwrap();
        assert_eq!(after_tags(&table), [Some("old"), None, Some("new")]);
        assert_eq!(table.named("dev").unwrap().label, "id-old");
        assert_eq!(
            table.named("silo-rebuild-aaaa1111").unwrap().label,
            "id-new"
        );
    }
}

#[test]
fn a_swap_reverted_from_the_end_returns_every_name_and_id() {
    let table = before();
    swap::apply(&table, &plan()).unwrap();
    swap::revert(&table, &plan()).unwrap();
    assert_eq!(after_tags(&table), [Some("old"), None, Some("new")]);
    assert_eq!(table.named("dev").unwrap().label, "id-old");
    assert_eq!(
        table.named("silo-rebuild-aaaa1111").unwrap().label,
        "id-new"
    );
    // Reverting again changes nothing.
    let done = table.log.lock().unwrap().len();
    swap::revert(&table, &plan()).unwrap();
    assert_eq!(table.log.lock().unwrap().len(), done);
}

#[test]
fn sandboxes_that_are_not_the_expected_ones_are_left_alone() {
    // Somebody else owns the undo name.
    let table = Table::with(&[
        ("dev", "id-old", "old"),
        ("dev-undo", "id-other", "other"),
        ("silo-rebuild-aaaa1111", "id-new", "new"),
    ]);
    let error = swap::apply(&table, &plan()).unwrap_err();
    assert!(
        error.contains("not the computers the rebuild expected"),
        "{error}"
    );
    assert!(table.log.lock().unwrap().is_empty());
    assert!(swap::revert(&table, &plan()).is_err());
    assert!(table.log.lock().unwrap().is_empty());
}

fn configuration(id: &str, name: &str) -> ComputerConfiguration {
    ComputerConfiguration {
        id: id.into(),
        name: name.into(),
        cpus: 2,
        max_cpus: 4,
        memory_gib: 4,
        max_memory_gib: 8,
        workspace_storage_gib: 10,
        runtime_storage_gib: 4,
        desktop: None,
        runtime: None,
    }
}

fn ids(computers: &[ComputerConfiguration]) -> Vec<(&str, &str)> {
    computers.iter().map(|c| (c.id(), c.name())).collect()
}

#[test]
fn the_saved_computers_get_an_undo_entry_and_lose_the_build_entry() {
    let mut computers = vec![
        configuration("a", "alpha"),
        configuration("id-old", "dev"),
        configuration("id-new", "silo-rebuild-aaaa1111"),
        configuration("z", "zulu"),
    ];
    swap::swap_inventory(&mut computers, &plan());
    assert_eq!(
        ids(&computers),
        [
            ("a", "alpha"),
            ("id-old", "dev"),
            ("id-undo", "dev-undo"),
            ("z", "zulu")
        ]
    );
    // The undo computer has the old one's size and settings.
    let undo = computers.iter().find(|c| c.id() == "id-undo").unwrap();
    assert_eq!(
        (undo.cpus, undo.memory_gib, undo.workspace_storage_gib),
        (2, 4, 10)
    );
    let once = computers.clone();
    swap::swap_inventory(&mut computers, &plan());
    assert_eq!(computers, once);

    swap::unswap_inventory(&mut computers, &plan());
    assert_eq!(
        ids(&computers),
        [
            ("a", "alpha"),
            ("id-old", "dev"),
            ("id-new", "silo-rebuild-aaaa1111"),
            ("z", "zulu")
        ]
    );
    let back = computers.clone();
    swap::unswap_inventory(&mut computers, &plan());
    assert_eq!(computers, back);
}

#[test]
fn undo_and_build_names_fit_a_computer_name() {
    assert_eq!(undo_name("dev"), "dev-undo");
    let long = "a-very-long-computer-name-of-31c";
    assert_eq!(long.len(), 32);
    let undo = undo_name(long);
    assert!(undo.len() <= 32 && undo.ends_with("-undo"), "{undo}");
    // The build sandbox has a fixed-length name that Silo keeps out of lists.
    let build = super::build_name("0123abcd");
    assert_eq!(build, "silo-rebuild-0123abcd");
    assert!(build.len() <= 32);
    assert!(crate::computer_tree::hidden(&build));
    assert!(!crate::computer_tree::hidden("dev-undo"));
    // A name cut at a dash does not leave a double dash.
    let cut = format!("{}-b", "a".repeat(26));
    assert_eq!(undo_name(&cut), format!("{}-undo", "a".repeat(26)));
}

// MARK: The flow

struct Machine {
    name: String,
    running: bool,
}

/// A device with one computer, `dev`, and the sandboxes behind it.
struct Fake {
    machines: Mutex<HashMap<String, Machine>>,
    table: Table,
    calls: Arc<Mutex<Vec<String>>>,
    /// The image the computer runs on.
    image: Option<String>,
    /// Makes the operation gate unavailable.
    hold_fails: bool,
    /// Makes the next that many requests for the operation gate fail.
    hold_failures: Mutex<usize>,
    /// The home folder's size cannot be known without starting the computer.
    home_unknown: bool,
    /// The step label that makes the next cancellation check fail, once the label is shown.
    cancel_on: Mutex<Option<String>>,
    /// Asks to cancel from the moment the swap runs, as a person might while it does.
    cancel_during_swap: bool,
    cancelled: Mutex<bool>,
    labels: Mutex<Vec<String>>,
    notices: Mutex<Vec<Option<String>>>,
    shield: Mutex<bool>,
    /// The methods that ran without the environment's protection from cancellation.
    unshielded: Mutex<Vec<String>>,
    /// Fails the next call to this method, once.
    fail_on: Mutex<Option<String>>,
    /// Makes every removal of the build fail.
    broken_remove: Mutex<bool>,
    os: Os,
    container: bool,
    busy: Option<String>,
    refusal: Option<String>,
    taken: Vec<String>,
    home: u64,
    recipe: Option<RecipeSpec>,
    replayed: Mutex<Vec<(String, RecipeSpec)>>,
    ids: Mutex<Vec<String>>,
}

impl Fake {
    fn new(old_id: &str, running: bool) -> Fake {
        let mut machines = HashMap::new();
        machines.insert(
            old_id.to_owned(),
            Machine {
                name: "dev".into(),
                running,
            },
        );
        Fake {
            machines: Mutex::new(machines),
            table: Table::with(&[("dev", old_id, "old")]),
            calls: Arc::default(),
            image: Some("image-v4".into()),
            hold_fails: false,
            hold_failures: Mutex::default(),
            home_unknown: false,
            cancel_on: Mutex::default(),
            cancel_during_swap: false,
            cancelled: Mutex::default(),
            labels: Mutex::default(),
            notices: Mutex::default(),
            shield: Mutex::default(),
            unshielded: Mutex::default(),
            fail_on: Mutex::default(),
            broken_remove: Mutex::default(),
            os: Os::Linux,
            container: false,
            busy: None,
            refusal: None,
            taken: Vec::new(),
            home: 100 * 1024 * 1024,
            recipe: None,
            replayed: Mutex::default(),
            ids: Mutex::new(vec!["id-n2".into(), "id-n1".into()]),
        }
    }

    fn record(&self, call: impl Into<String>) -> Result<(), String> {
        let call = call.into();
        self.calls.lock().unwrap().push(call.clone());
        let mut fail = self.fail_on.lock().unwrap();
        if fail.as_deref() == call.split(' ').next() {
            *fail = None;
            return Err(format!("injected failure in {call}"));
        }
        Ok(())
    }

    fn fail_once_in(&self, method: &str) {
        *self.fail_on.lock().unwrap() = Some(method.to_owned());
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    /// Notes a method that must run protected from a request to cancel.
    fn guarded(&self, what: &str) {
        if !*self.shield.lock().unwrap() {
            self.unshielded.lock().unwrap().push(what.into());
        }
    }

    fn called(&self, name: &str) -> bool {
        self.calls()
            .iter()
            .any(|call| call.split(' ').next() == Some(name))
    }

    fn running(&self, id: &str) -> bool {
        self.machines
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|m| m.running)
    }

    /// The calls that changed something.
    fn changes(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|call| {
                !matches!(
                    call.split(' ').next(),
                    Some(
                        "hold"
                            | "release"
                            | "info"
                            | "refusal"
                            | "name_in_use"
                            | "recipe"
                            | "newest_image"
                            | "home_bytes"
                    )
                )
            })
            .collect()
    }
}

/// The operation gate the fake hands out: it logs when it is let go.
struct Gate {
    calls: Arc<Mutex<Vec<String>>>,
    id: String,
}

impl Drop for Gate {
    fn drop(&mut self) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("release {}", self.id));
    }
}

impl RebuildEnv for Fake {
    fn hold(&self, id: &str) -> Result<Box<dyn Any>, String> {
        self.calls.lock().unwrap().push(format!("hold {id}"));
        let mut failures = self.hold_failures.lock().unwrap();
        if self.hold_fails || *failures > 0 {
            *failures = failures.saturating_sub(1);
            return Err("Another computer operation is still running.".into());
        }
        Ok(Box::new(Gate {
            calls: self.calls.clone(),
            id: id.into(),
        }))
    }

    fn relabel(&self, label: &str) {
        self.labels.lock().unwrap().push(label.into());
    }

    fn check_cancelled(&self) -> Result<(), String> {
        let wanted = self.cancel_on.lock().unwrap().clone();
        let shown = self
            .labels
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap_or_default();
        if *self.cancelled.lock().unwrap() || wanted.is_some_and(|label| shown.contains(&label)) {
            return Err("The rebuild was cancelled.".into());
        }
        Ok(())
    }

    fn shielded(&self, work: &mut dyn FnMut()) {
        let before = std::mem::replace(&mut *self.shield.lock().unwrap(), true);
        work();
        *self.shield.lock().unwrap() = before;
    }

    fn clean_old_computer(&self, plan: &Plan) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("clean_old_computer {}", plan.old_id));
    }

    fn restore_original(&self, plan: &Plan, swapping: bool) -> Result<(), String> {
        self.record("restore_original")?;
        swap::restore_original(&self.table, plan, swapping)
    }

    fn stuck_notice(&self, _plan: &Plan, problem: Option<&str>) {
        self.notices
            .lock()
            .unwrap()
            .push(problem.map(str::to_owned));
    }

    fn info(&self, id: &str) -> Result<Info, String> {
        let machines = self.machines.lock().unwrap();
        let machine = machines.get(id).ok_or("gone")?;
        Ok(Info {
            id: id.into(),
            name: machine.name.clone(),
            os: self.os,
            container: self.container,
            running: machine.running,
            busy: self.busy.clone(),
            root_disk_gib: 4,
            image: self.image.clone(),
        })
    }

    fn refusal(&self, _info: &Info) -> Option<String> {
        self.calls.lock().unwrap().push("refusal".into());
        self.refusal.clone()
    }

    fn name_in_use(&self, name: &str) -> bool {
        self.calls
            .lock()
            .unwrap()
            .push(format!("name_in_use {name}"));
        self.taken.iter().any(|taken| taken == name)
    }

    fn recipe(&self, id: &str) -> Result<Option<RecipeSpec>, String> {
        self.calls.lock().unwrap().push(format!("recipe {id}"));
        Ok(self.recipe.clone())
    }

    fn home_bytes(&self, _id: &str) -> Result<Option<u64>, String> {
        self.calls.lock().unwrap().push("home_bytes".into());
        Ok((!self.home_unknown).then_some(self.home))
    }

    fn newest_image(&self) -> Result<String, String> {
        self.calls.lock().unwrap().push("newest_image".into());
        Ok("image-v5".into())
    }

    fn fresh_id(&self) -> String {
        self.ids.lock().unwrap().pop().unwrap()
    }

    fn stage_key(&self) -> String {
        "aaaa1111".into()
    }

    fn now_millis(&self) -> u64 {
        1
    }

    fn start(&self, id: &str) -> Result<(), String> {
        self.record(format!("start {id}"))?;
        self.machines
            .lock()
            .unwrap()
            .get_mut(id)
            .ok_or("gone")?
            .running = true;
        Ok(())
    }

    fn stop(&self, id: &str) -> Result<(), String> {
        self.record(format!("stop {id}"))?;
        self.machines
            .lock()
            .unwrap()
            .get_mut(id)
            .ok_or("gone")?
            .running = false;
        Ok(())
    }

    fn checkpoint(&self, id: &str, name: &str) -> Result<CheckpointRef, String> {
        self.record(format!("checkpoint {id} {name}"))?;
        assert!(!self.running(id), "the checkpoint is taken while stopped");
        Ok(CheckpointRef {
            id: "cp-1".into(),
            name: name.into(),
            created_at: "now".into(),
            scope: "disk".into(),
        })
    }

    fn create(&self, plan: &Plan) -> Result<(), String> {
        self.record(format!("create {} {}", plan.new_id, plan.new_name))?;
        assert_eq!(plan.image, "image-v5");
        self.machines.lock().unwrap().insert(
            plan.new_id.clone(),
            Machine {
                name: plan.new_name.clone(),
                running: false,
            },
        );
        self.table.sandboxes.lock().unwrap().push(Sandbox {
            name: plan.new_name.clone(),
            label: plan.new_id.clone(),
            tag: "new",
        });
        Ok(())
    }

    fn move_workspace(&self, plan: &Plan) -> Result<(), String> {
        self.record(format!("move_workspace {}", plan.new_id))?;
        assert!(!self.running(&plan.old_id) && !self.running(&plan.new_id));
        Ok(())
    }

    fn copy_ssh(&self, plan: &Plan) -> Result<(), String> {
        self.record(format!("copy_ssh {}", plan.new_id))
    }

    fn carry_home(&self, plan: &Plan) -> Result<(), String> {
        self.record(format!("carry_home {} {}", plan.old_id, plan.new_id))?;
        assert!(self.running(&plan.old_id) && self.running(&plan.new_id));
        Ok(())
    }

    fn replay_recipe(&self, id: &str, spec: &RecipeSpec) -> Result<(), String> {
        self.record(format!("replay_recipe {id}"))?;
        self.replayed
            .lock()
            .unwrap()
            .push((id.into(), spec.clone()));
        Ok(())
    }

    fn swap_apply(&self, plan: &Plan) -> Result<(), String> {
        self.guarded("swap_apply");
        self.record("swap_apply")?;
        assert!(!self.running(&plan.old_id) && !self.running(&plan.new_id));
        if self.cancel_during_swap {
            *self.cancelled.lock().unwrap() = true;
        }
        swap::apply(&self.table, plan)?;
        // The rebuilt sandbox now answers to the computer's id.
        let mut machines = self.machines.lock().unwrap();
        let Some(new) = machines.remove(&plan.new_id) else {
            return Ok(());
        };
        let old = machines.remove(&plan.old_id).ok_or("gone")?;
        machines.insert(
            plan.old_id.clone(),
            Machine {
                name: plan.old_name.clone(),
                running: new.running,
            },
        );
        machines.insert(
            plan.undo_id.clone(),
            Machine {
                name: plan.undo_name.clone(),
                running: old.running,
            },
        );
        Ok(())
    }

    fn swap_revert(&self, plan: &Plan) -> Result<(), String> {
        self.guarded("swap_revert");
        self.record("swap_revert")?;
        swap::revert(&self.table, plan)?;
        let mut machines = self.machines.lock().unwrap();
        if let Some(undo) = machines.remove(&plan.undo_id) {
            let rebuilt = machines.remove(&plan.old_id).ok_or("gone")?;
            machines.insert(
                plan.old_id.clone(),
                Machine {
                    name: plan.old_name.clone(),
                    running: undo.running,
                },
            );
            machines.insert(
                plan.new_id.clone(),
                Machine {
                    name: plan.new_name.clone(),
                    running: rebuilt.running,
                },
            );
        }
        Ok(())
    }

    fn settle_swapped(&self, _plan: &Plan) -> Result<(), String> {
        self.guarded("settle_swapped");
        self.record("settle_swapped")
    }

    fn remove_build(&self, plan: &Plan) -> Result<(), String> {
        self.guarded("remove_build");
        self.record("remove_build")?;
        if *self.broken_remove.lock().unwrap() {
            return Err("the build cannot be removed".into());
        }
        self.machines.lock().unwrap().remove(&plan.new_id);
        self.table
            .sandboxes
            .lock()
            .unwrap()
            .retain(|s| s.name != plan.new_name);
        Ok(())
    }
}

fn journal() -> (tempfile::TempDir, Journal) {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path().join("rebuild"));
    (dir, journal)
}

#[test]
fn every_refusal_comes_before_any_change() {
    type Setup = fn(&mut Fake);
    let cases: [(&str, Setup, &str); 6] = [
        ("c-mac", |f| f.os = Os::Macos, "Mac"),
        ("c-container", |f| f.container = true, "container"),
        (
            "c-busy",
            |f| f.busy = Some("a checkpoint restore is pending".into()),
            "busy",
        ),
        (
            "c-host",
            |f| f.refusal = Some("This device cannot clone.".into()),
            "cannot clone",
        ),
        (
            "c-name",
            |f| f.taken = vec!["dev-undo".into()],
            "already exists",
        ),
        ("c-home", |f| f.home = 3 * 1024 * 1024 * 1024, "too large"),
    ];
    for (id, setup, expect) in cases {
        let mut fake = Fake::new(id, true);
        setup(&mut fake);
        let (_dir, journal) = journal();
        let error = rebuild(&fake, &journal, id, false, WAIT).unwrap_err();
        assert!(error.contains(expect), "{id}: {error}");
        assert!(error.ends_with("Nothing was changed."), "{id}: {error}");
        assert_eq!(fake.changes(), Vec::<String>::new(), "{id}");
        assert!(journal.all().is_empty(), "{id}");
        assert!(fake.running(id), "{id} still runs");
        // The lane was let go: the same attempt reaches the same refusal, not a busy lane.
        assert_eq!(
            rebuild(&fake, &journal, id, false, WAIT).unwrap_err(),
            error
        );
    }
    let fake = Fake::new("c-gone", true);
    fake.machines.lock().unwrap().clear();
    let (_dir, journal) = journal();
    let error = rebuild(&fake, &journal, "c-gone", false, WAIT).unwrap_err();
    assert!(error.contains("no longer exists"), "{error}");
    assert_eq!(fake.changes(), Vec::<String>::new());
}

#[test]
fn the_refusal_for_a_taken_undo_name_names_it() {
    let mut fake = Fake::new("c-taken", true);
    fake.taken = vec!["dev-undo".into()];
    let (_dir, journal) = journal();
    let error = rebuild(&fake, &journal, "c-taken", false, WAIT).unwrap_err();
    assert!(
        error.contains("Computer 'dev-undo' already exists"),
        "{error}"
    );
    assert!(error.contains("rebuilding 'dev'"), "{error}");
}

#[test]
fn a_computer_already_on_the_newest_image_is_refused_unless_forced() {
    let mut fake = Fake::new("c-new", true);
    fake.image = Some("image-v5".into());
    let (_dir, journal) = journal();
    let error = rebuild(&fake, &journal, "c-new", false, WAIT).unwrap_err();
    assert!(error.contains("already on the newest image"), "{error}");
    assert!(error.contains("--force"), "{error}");
    assert!(error.ends_with("Nothing was changed."), "{error}");
    assert_eq!(fake.changes(), Vec::<String>::new());
    assert!(journal.all().is_empty());
    assert!(fake.running("c-new"));

    rebuild(&fake, &journal, "c-new", true, WAIT).unwrap();
    assert!(fake.called("swap_apply"));

    // An image the runtime does not report does not stop a rebuild.
    let mut fake = Fake::new("c-unknown", false);
    fake.image = None;
    rebuild(&fake, &journal, "c-unknown", false, WAIT).unwrap();
    assert!(fake.called("swap_apply"));
}

#[test]
fn nothing_is_changed_when_the_computers_gate_is_not_available() {
    let mut fake = Fake::new("c-gate", true);
    fake.hold_fails = true;
    let (_dir, journal) = journal();
    let error = rebuild(&fake, &journal, "c-gate", false, WAIT).unwrap_err();
    assert!(error.contains("still running"), "{error}");
    assert_eq!(fake.changes(), Vec::<String>::new());
    assert!(journal.all().is_empty());
}

#[test]
fn a_rebuild_is_rolled_back_with_the_gate_still_held() {
    let fake = Fake::new("c-held", true);
    fake.fail_once_in("move_workspace");
    let (_dir, journal) = journal();
    rebuild(&fake, &journal, "c-held", false, WAIT).unwrap_err();
    let calls = fake.calls();
    let released = calls.iter().position(|c| c.starts_with("release")).unwrap();
    let removed = calls.iter().position(|c| c == "remove_build").unwrap();
    assert!(removed < released, "{calls:?}");
    assert_eq!(released, calls.len() - 1);
}

#[test]
fn a_rebuild_runs_the_steps_in_order_and_swaps_the_names() {
    let mut fake = Fake::new("c-ok", true);
    fake.recipe = Some(spec());
    let (_dir, journal) = journal();
    let result = rebuild(&fake, &journal, "c-ok", false, WAIT).unwrap();
    assert_eq!(result.computer_id, "c-ok");
    assert_eq!(result.name, "dev");
    assert_eq!(result.undo_name, "dev-undo");
    assert_eq!(result.image, "image-v5");
    assert_eq!(
        fake.changes(),
        [
            "stop c-ok",
            "checkpoint c-ok Before rebuild on the newest image",
            "create id-n1 silo-rebuild-aaaa1111",
            "start id-n1",
            "stop id-n1",
            "start c-ok",
            "start id-n1",
            "carry_home c-ok id-n1",
            "stop c-ok",
            "stop id-n1",
            "move_workspace id-n1",
            "copy_ssh id-n1",
            "start id-n1",
            "replay_recipe id-n1",
            "stop id-n1",
            "swap_apply",
            "settle_swapped",
            "start c-ok",
        ]
    );
    // The gate is taken before the first change and kept to the very end.
    let calls = fake.calls();
    assert_eq!(calls.first().map(String::as_str), Some("hold c-ok"));
    assert_eq!(calls.last().map(String::as_str), Some("release c-ok"));
    assert_eq!(calls.iter().filter(|c| c.starts_with("hold")).count(), 1);
    // After the workspace disk is cloned, the old computer never runs until the rebuilt one
    // has taken its id.
    let cloned = calls
        .iter()
        .position(|c| c.starts_with("move_workspace"))
        .unwrap();
    let swapped = calls.iter().position(|c| c == "swap_apply").unwrap();
    assert!(
        !calls[cloned..swapped].iter().any(|c| c == "start c-ok"),
        "{calls:?}"
    );
    assert!(journal.all().is_empty());
    // The rebuilt sandbox is `dev` under the computer's id and runs again; the old one is the
    // stopped undo computer under its own id.
    assert_eq!(after_tags(&fake.table), [Some("new"), Some("old"), None]);
    assert!(fake.running("c-ok"));
    let machines = fake.machines.lock().unwrap();
    assert_eq!(machines["id-n2"].name, "dev-undo");
    assert!(!machines["id-n2"].running);
}

#[test]
fn a_stopped_computer_stays_stopped_and_one_without_a_recipe_replays_nothing() {
    let fake = Fake::new("c-stopped", false);
    let (_dir, journal) = journal();
    rebuild(&fake, &journal, "c-stopped", false, WAIT).unwrap();
    assert!(!fake.called("replay_recipe"));
    assert!(!fake.running("c-stopped"));
    assert_eq!(
        fake.changes().last().map(String::as_str),
        Some("settle_swapped")
    );
}

#[test]
fn the_recipe_is_replayed_in_the_new_computer_with_the_recorded_spec() {
    let mut fake = Fake::new("c-recipe", false);
    fake.recipe = Some(spec());
    let (_dir, journal) = journal();
    rebuild(&fake, &journal, "c-recipe", false, WAIT).unwrap();
    let replayed = fake.replayed.lock().unwrap().clone();
    assert_eq!(replayed, [("id-n1".to_owned(), spec())]);
    // After the home folder and the workspace arrived, before the swap.
    let calls = fake.calls();
    let at = |name: &str| calls.iter().position(|c| c.starts_with(name)).unwrap();
    assert!(
        at("carry_home") < at("move_workspace")
            && at("move_workspace") < at("replay_recipe")
            && at("replay_recipe") < at("swap_apply")
    );
}

#[test]
fn a_macos_recipe_is_not_replayed() {
    let mut fake = Fake::new("c-macrecipe", false);
    fake.recipe = Some(RecipeSpec {
        os: RecipeOs::Macos,
        ..spec()
    });
    let (_dir, journal) = journal();
    rebuild(&fake, &journal, "c-macrecipe", false, WAIT).unwrap();
    assert!(!fake.called("replay_recipe"));
}

/// The method a failure is injected in, and whether the swap had begun by then.
const FAILURES: [(&str, bool); 9] = [
    ("checkpoint", false),
    ("create", false),
    ("move_workspace", false),
    ("copy_ssh", false),
    ("carry_home", false),
    ("replay_recipe", false),
    ("swap_apply", true),
    ("stop", false),
    ("start", false),
];

#[test]
fn a_failure_at_any_step_rolls_back_to_the_computer_as_it_was() {
    for (n, (method, swapping)) in FAILURES.into_iter().enumerate() {
        let id = format!("c-fail-{n}");
        let mut fake = Fake::new(&id, true);
        fake.recipe = Some(spec());
        fake.fail_once_in(method);
        let (_dir, journal) = journal();
        let error = rebuild(&fake, &journal, &id, false, WAIT).unwrap_err();
        assert!(error.contains("injected failure"), "{method}: {error}");
        assert!(
            error.contains("rolled back; dev is as it was"),
            "{method}: {error}"
        );
        assert!(fake.called("remove_build"), "{method}");
        assert_eq!(fake.called("swap_revert"), swapping, "{method}");
        assert!(!fake.called("settle_swapped"), "{method}");
        assert!(journal.all().is_empty(), "{method}");
        // The computer is as it was: its name and id, and running.
        assert_eq!(fake.table.named("dev").unwrap().label, id, "{method}");
        assert!(fake.running(&id), "{method}");
        assert_eq!(fake.table.named("dev-undo"), None, "{method}");
        assert_eq!(fake.table.named("silo-rebuild-aaaa1111"), None, "{method}");
        assert!(
            !fake.machines.lock().unwrap().contains_key("id-n1"),
            "{method}"
        );
    }
}

#[test]
fn a_computer_that_was_stopped_is_not_started_by_a_rollback() {
    let fake = Fake::new("c-cold", false);
    fake.fail_once_in("carry_home");
    let (_dir, journal) = journal();
    rebuild(&fake, &journal, "c-cold", false, WAIT).unwrap_err();
    // Started once to carry the home folder, stopped again by the rollback, not brought back.
    assert!(!fake.running("c-cold"));
    assert!(fake.called("remove_build"));
}

#[test]
fn the_journal_follows_the_steps() {
    // A failure in the replay leaves the record at the last finished step while rolling back.
    struct Watching<'a> {
        inner: &'a Fake,
        journal: &'a Journal,
        seen: Mutex<Vec<Step>>,
    }
    impl RebuildEnv for Watching<'_> {
        fn info(&self, id: &str) -> Result<Info, String> {
            self.inner.info(id)
        }
        fn name_in_use(&self, name: &str) -> bool {
            self.inner.name_in_use(name)
        }
        fn recipe(&self, id: &str) -> Result<Option<RecipeSpec>, String> {
            self.inner.recipe(id)
        }
        fn home_bytes(&self, id: &str) -> Result<Option<u64>, String> {
            self.inner.home_bytes(id)
        }
        fn newest_image(&self) -> Result<String, String> {
            self.inner.newest_image()
        }
        fn fresh_id(&self) -> String {
            self.inner.fresh_id()
        }
        fn stage_key(&self) -> String {
            self.inner.stage_key()
        }
        fn now_millis(&self) -> u64 {
            self.inner.now_millis()
        }
        fn start(&self, id: &str) -> Result<(), String> {
            self.inner.start(id)
        }
        fn stop(&self, id: &str) -> Result<(), String> {
            self.inner.stop(id)
        }
        fn checkpoint(&self, id: &str, name: &str) -> Result<CheckpointRef, String> {
            self.inner.checkpoint(id, name)
        }
        fn create(&self, plan: &Plan) -> Result<(), String> {
            self.inner.create(plan)
        }
        fn move_workspace(&self, plan: &Plan) -> Result<(), String> {
            self.inner.move_workspace(plan)
        }
        fn copy_ssh(&self, plan: &Plan) -> Result<(), String> {
            self.inner.copy_ssh(plan)
        }
        fn carry_home(&self, plan: &Plan) -> Result<(), String> {
            self.inner.carry_home(plan)
        }
        fn replay_recipe(&self, id: &str, spec: &RecipeSpec) -> Result<(), String> {
            self.inner.replay_recipe(id, spec)
        }
        fn swap_apply(&self, plan: &Plan) -> Result<(), String> {
            // Recorded before the first name moves.
            self.seen
                .lock()
                .unwrap()
                .push(self.journal.get(&plan.old_id).unwrap().step);
            self.inner.swap_apply(plan)
        }
        fn swap_revert(&self, plan: &Plan) -> Result<(), String> {
            self.inner.swap_revert(plan)
        }
        fn settle_swapped(&self, plan: &Plan) -> Result<(), String> {
            self.seen
                .lock()
                .unwrap()
                .push(self.journal.get(&plan.old_id).unwrap().step);
            self.inner.settle_swapped(plan)
        }
        fn remove_build(&self, plan: &Plan) -> Result<(), String> {
            self.inner.remove_build(plan)
        }
    }
    let mut fake = Fake::new("c-journal", true);
    fake.recipe = Some(spec());
    let (_dir, journal) = journal();
    let watching = Watching {
        inner: &fake,
        journal: &journal,
        seen: Mutex::default(),
    };
    rebuild(&watching, &journal, "c-journal", false, WAIT).unwrap();
    assert_eq!(
        *watching.seen.lock().unwrap(),
        [Step::Swapping, Step::Swapped]
    );
}

#[test]
fn a_rollback_that_fails_keeps_the_journal_and_the_next_start_finishes_it() {
    let mut fake = Fake::new("c-stuck", true);
    fake.recipe = Some(spec());
    fake.fail_once_in("carry_home");
    *fake.broken_remove.lock().unwrap() = true;
    let (_dir, journal) = journal();
    let error = rebuild(&fake, &journal, "c-stuck", false, WAIT).unwrap_err();
    assert!(error.contains("Rolling back also failed"), "{error}");
    assert!(error.contains("restarting Silo retries it"), "{error}");
    let left = journal.get("c-stuck").unwrap();
    assert_eq!(left.step, Step::Created);
    assert!(fake.table.named("silo-rebuild-aaaa1111").is_some());

    // The next start: the removal works now.
    *fake.broken_remove.lock().unwrap() = false;
    let held = Mutex::new(0);
    recover_one(
        &fake,
        &journal,
        "c-stuck",
        &|| *held.lock().unwrap() += 1,
        1,
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(*held.lock().unwrap(), 1);
    assert!(journal.all().is_empty());
    assert_eq!(fake.table.named("silo-rebuild-aaaa1111"), None);
    assert_eq!(fake.table.named("dev").unwrap().label, "c-stuck");
    assert!(fake.running("c-stuck"));
}

// MARK: Recovery

const STEPS: [Step; 9] = [
    Step::Planned,
    Step::Stopped,
    Step::Checkpointed,
    Step::Created,
    Step::HomeCarried,
    Step::WorkspaceMoved,
    Step::RecipeReplayed,
    Step::Swapping,
    Step::Swapped,
];

/// A device the engine stopped on while a rebuild had reached `step`, and the journal that
/// says so.
fn interrupted(id: &str, step: Step, was_running: bool) -> (Fake, tempfile::TempDir, Journal) {
    let fake = Fake::new(id, false);
    let plan = Plan {
        old_id: id.into(),
        new_id: "id-new".into(),
        ..plan()
    };
    if step >= Step::Created {
        fake.create(&plan).unwrap();
    }
    if step >= Step::Swapping {
        // The names may or may not have moved when the engine stopped; here they had.
        fake.swap_apply(&plan).unwrap();
    }
    fake.calls.lock().unwrap().clear();
    fake.unshielded.lock().unwrap().clear();
    let (dir, journal) = journal();
    journal
        .save(&Record {
            plan,
            step,
            checkpoint: None,
            was_running,
            at: 1,
        })
        .unwrap();
    (fake, dir, journal)
}

#[test]
fn an_interrupted_rebuild_is_rolled_back_or_finished_by_the_step_it_reached() {
    for (n, step) in STEPS.into_iter().enumerate() {
        for was_running in [true, false] {
            let id = format!("c-rec-{n}-{was_running}");
            let (fake, _dir, journal) = interrupted(&id, step, was_running);
            assert_eq!(super::interrupted_ids(&journal), [id.clone()]);
            let held = Mutex::new(0);
            recover_one(
                &fake,
                &journal,
                &id,
                &|| *held.lock().unwrap() += 1,
                1,
                Duration::ZERO,
            )
            .unwrap();
            let label = format!("{step:?} running={was_running}");
            assert_eq!(*held.lock().unwrap(), 1, "{label}");
            assert!(journal.all().is_empty(), "{label}");
            // Whatever way it went, the computer keeps its name and id, and is running again
            // exactly when it was.
            assert_eq!(fake.table.named("dev").unwrap().label, id, "{label}");
            assert_eq!(fake.running(&id), was_running, "{label}");
            // The gate was taken before startup was told the computer is held.
            assert_eq!(fake.calls().first(), Some(&format!("hold {id}")), "{label}");
            if step >= Step::Swapping {
                // Once the swap has begun it is finished, never undone.
                assert!(fake.called("settle_swapped"), "{label}");
                assert!(!fake.called("remove_build"), "{label}");
                assert!(!fake.called("swap_revert"), "{label}");
                assert_eq!(
                    after_tags(&fake.table),
                    [Some("new"), Some("old"), None],
                    "{label}"
                );
            } else {
                assert!(fake.called("remove_build"), "{label}");
                assert!(!fake.called("settle_swapped"), "{label}");
                assert!(!fake.called("swap_revert"), "{label}");
                assert_eq!(
                    after_tags(&fake.table),
                    [Some("old"), None, None],
                    "{label}"
                );
            }
        }
    }
}

#[test]
fn a_swap_that_stopped_after_any_rename_is_finished_by_the_next_start() {
    for done in 0..4 {
        for was_running in [true, false] {
            let id = format!("c-partial-{done}-{was_running}");
            let fake = Fake::new(&id, false);
            let plan = Plan {
                old_id: id.clone(),
                new_id: "id-new".into(),
                ..plan()
            };
            fake.create(&plan).unwrap();
            // The engine stopped after `done` renames or relabels of the swap.
            *fake.table.fail_after.lock().unwrap() = Some(done);
            swap::apply(&fake.table, &plan).unwrap_err();
            *fake.table.fail_after.lock().unwrap() = None;
            fake.calls.lock().unwrap().clear();
            let (_dir, journal) = journal();
            journal
                .save(&Record {
                    plan,
                    step: Step::Swapping,
                    checkpoint: None,
                    was_running,
                    at: 1,
                })
                .unwrap();
            recover_one(&fake, &journal, &id, &|| {}, 1, Duration::ZERO).unwrap();
            let label = format!("{done} moves, running={was_running}");
            assert_eq!(
                after_tags(&fake.table),
                [Some("new"), Some("old"), None],
                "{label}"
            );
            assert_eq!(fake.table.named("dev").unwrap().label, id, "{label}");
            assert_eq!(
                fake.table.named("dev-undo").unwrap().label,
                "id-undo",
                "{label}"
            );
            assert!(fake.called("settle_swapped"), "{label}");
            assert!(
                !fake.called("swap_revert") && !fake.called("remove_build"),
                "{label}"
            );
            assert_eq!(fake.running(&id), was_running, "{label}");
            assert!(journal.all().is_empty(), "{label}");
        }
    }
}

#[test]
fn recovery_does_not_let_startup_go_without_the_computers_gate() {
    let (mut fake, _dir, journal) = interrupted("c-nogate", Step::Created, false);
    fake.hold_fails = true;
    let held = Mutex::new(0);
    let error = recover_one(
        &fake,
        &journal,
        "c-nogate",
        &|| *held.lock().unwrap() += 1,
        1,
        Duration::ZERO,
    )
    .unwrap_err();
    assert!(error.contains("still running"), "{error}");
    assert_eq!(*held.lock().unwrap(), 0);
    assert_eq!(fake.changes(), Vec::<String>::new());
    assert_eq!(journal.all().len(), 1);
}

#[test]
fn recovery_is_retried_while_the_runtime_is_not_ready() {
    let (fake, _dir, journal) = interrupted("c-retry", Step::Created, false);
    fake.fail_once_in("remove_build");
    recover_one(&fake, &journal, "c-retry", &|| {}, 3, Duration::ZERO).unwrap();
    assert!(journal.all().is_empty());
    assert_eq!(
        fake.calls().iter().filter(|c| *c == "remove_build").count(),
        2
    );
    // Giving up leaves the record for the next start.
    let (fake, _dir, journal) = interrupted("c-retry-2", Step::Created, false);
    *fake.broken_remove.lock().unwrap() = true;
    let error = recover_one(&fake, &journal, "c-retry-2", &|| {}, 2, Duration::ZERO).unwrap_err();
    assert!(error.contains("cannot be removed"), "{error}");
    assert_eq!(journal.all().len(), 1);
}

#[test]
fn a_rebuild_settles_an_interrupted_one_of_the_same_computer_first() {
    let (mut fake, _dir, journal) = interrupted("c-again", Step::Created, true);
    fake.ids = Mutex::new(vec!["id-n2".into(), "id-n1".into()]);
    fake.recipe = None;
    let result = rebuild(&fake, &journal, "c-again", false, WAIT).unwrap();
    assert_eq!(result.undo_name, "dev-undo");
    let calls = fake.calls();
    let first_removal = calls.iter().position(|c| c == "remove_build").unwrap();
    let first_stop = calls.iter().position(|c| c.starts_with("stop")).unwrap();
    assert!(first_removal < first_stop, "{calls:?}");
    assert!(journal.all().is_empty());
    assert_eq!(after_tags(&fake.table), [Some("new"), Some("old"), None]);
}

#[test]
fn the_journal_keeps_one_record_per_computer() {
    let (_dir, journal) = journal();
    let one = Record {
        plan: plan(),
        step: Step::Stopped,
        checkpoint: Some(CheckpointRef {
            id: "cp".into(),
            name: "before".into(),
            created_at: "now".into(),
            scope: "disk".into(),
        }),
        was_running: true,
        at: 5,
    };
    journal.save(&one).unwrap();
    assert_eq!(journal.get("id-old"), Some(one.clone()));
    let later = Record {
        step: Step::Created,
        ..one.clone()
    };
    journal.save(&later).unwrap();
    assert_eq!(journal.all(), [later]);
    journal.remove("id-old");
    assert_eq!(journal.get("id-old"), None);
    assert!(journal.all().is_empty());
}

// MARK: The engine side's pieces

#[test]
fn the_size_of_the_home_folder_is_read_from_du() {
    assert_eq!(parse_du(b"1234\t/home/silo\n"), Ok(1234));
    assert!(parse_du(b"").is_err());
    assert!(parse_du(b"du: cannot read\n").is_err());
    assert_eq!(super::home_limit(4), 1_932_735_283);
}

#[test]
fn the_home_folder_travels_as_a_tar_file_that_keeps_owners_links_and_sparse_files() {
    for flag in ["--sparse", "--xattrs", "--acls", "--numeric-owner"] {
        assert!(PACK_HOME.contains(flag), "{flag}");
    }
    for flag in ["--xattrs", "--acls", "--numeric-owner", "--same-owner"] {
        assert!(UNPACK_HOME.contains(flag), "{flag}");
    }
    // The file's name comes from the caller, never a fixed name in /tmp.
    assert!(!PACK_HOME.contains("/tmp/") && !UNPACK_HOME.contains("/tmp/"));
}

#[test]
fn a_partial_or_damaged_home_folder_is_not_carried() {
    // Only an exit of 0 or 1 from tar counts, the file must exist and list cleanly, it is
    // created exclusively and readable by root only.
    for needle in [
        "[ \"$rc\" -le 1 ] || exit \"$rc\"",
        "[ -s \"$file\" ]",
        "tar -tf \"$file\"",
        "set -C",
        "umask 077",
    ] {
        assert!(PACK_HOME.contains(needle), "{needle}");
    }
    assert!(!PACK_HOME.contains("; true"));
    // The unpacking compares the working account's numeric owner and the file's checksum with
    // what the packing reported, before anything is written.
    let unpack_check = UNPACK_HOME.find("sha256sum").unwrap();
    let extraction = UNPACK_HOME.find("tar -C /home -xpf").unwrap();
    assert!(unpack_check < extraction);
    assert!(UNPACK_HOME.contains("id -u silo") && UNPACK_HOME.contains("id -g silo"));

    let sum = "a".repeat(64);
    assert_eq!(
        parse_pack_report(format!("1000 1000 {sum}\n").as_bytes()),
        Ok(super::engine_env::PackReport {
            uid: "1000".into(),
            gid: "1000".into(),
            sum: sum.clone()
        })
    );
    for bad in [
        String::new(),
        "1000 1000\n".into(),
        format!("x 1000 {sum}"),
        format!("1000 1000 {}", "z".repeat(64)),
        format!("1000 1000 {}", "a".repeat(10)),
    ] {
        assert!(parse_pack_report(bad.as_bytes()).is_err(), "{bad}");
    }
}

#[test]
fn staged_home_folders_nobody_waits_for_are_swept() {
    let dir = tempfile::tempdir().unwrap();
    let stage = dir.path().join("stage");
    write(&stage.join("home-aaaa1111.tar"), "kept");
    write(&stage.join("home-dead0000.tar"), "stray");
    write(&stage.join("other"), "stray");
    let journal = Journal::new(dir.path().join("rebuild"));
    journal
        .save(&Record {
            plan: plan(),
            step: Step::Created,
            checkpoint: None,
            was_running: false,
            at: 1,
        })
        .unwrap();
    super::sweep_stage(&journal, &stage);
    let left: Vec<String> = fs::read_dir(&stage)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["home-aaaa1111.tar"]);
    // A folder that does not exist is not an error.
    super::sweep_stage(&journal, &dir.path().join("none"));
}

fn out(code: i32, stdout: &str, stderr: &str) -> ExecOut {
    ExecOut {
        code,
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[test]
fn the_recipe_replay_runs_the_steps_of_a_build_in_order_and_stops_at_the_first_failure() {
    let steps = recipes::steps_for(&spec());
    assert!(steps.len() >= 3);
    assert_eq!(steps[0].who, RunAs::Root);
    assert!(steps
        .iter()
        .any(|s| s.args.iter().any(|(_, value)| value.contains("git"))));

    let mut ran = Vec::new();
    replay_steps(&steps, &|_| {}, &mut |step| {
        ran.push(step.label.clone());
        Ok(out(0, "", ""))
    })
    .unwrap();
    assert_eq!(
        ran,
        steps.iter().map(|s| s.label.clone()).collect::<Vec<_>>()
    );

    let mut ran = Vec::new();
    let error = replay_steps(&steps, &|_| {}, &mut |step| {
        ran.push(step.label.clone());
        if ran.len() == 2 {
            Ok(out(3, "", "no such package"))
        } else {
            Ok(out(0, "", ""))
        }
    })
    .unwrap_err();
    assert_eq!(ran.len(), 2);
    assert_eq!(
        error,
        format!("{} failed (exit 3): no such package", steps[1].label)
    );

    let error =
        replay_steps(&steps, &|_| {}, &mut |_| Err("the computer stopped".into())).unwrap_err();
    assert_eq!(
        error,
        format!("{} failed: the computer stopped", steps[0].label)
    );
}

fn write(path: &std::path::Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A stopped sandbox folder: a workspace disk with `files`, and the runtime's journal of it.
fn sandbox(
    root: &std::path::Path,
    name: &str,
    volume_id: &str,
    files: &[(&str, &str)],
    layers: &[&str],
) {
    let folder = root.join(name);
    for (file, text) in files {
        write(&folder.join("owned-volumes/workspace_abc").join(file), text);
    }
    let layers: Vec<serde_json::Value> = layers
        .iter()
        .map(|path| serde_json::json!({"path": format!("owned-volumes/workspace_abc/{path}")}))
        .collect();
    write(
        &folder.join("runtime/owned-disks/workspace_abc/disk.json"),
        &serde_json::json!({"volume_id": volume_id, "generation": 2, "layers": layers}).to_string(),
    );
}

#[test]
fn the_workspace_disk_is_cloned_into_the_new_sandbox_with_its_own_volume_id() {
    let dir = tempfile::tempdir().unwrap();
    if !can_clone(dir.path()) {
        eprintln!("skipped: this disk cannot clone files");
        return;
    }
    sandbox(
        dir.path(),
        "old",
        "vol-old",
        &[("sealed-000.raw", "base"), ("writable.qcow2", "changes")],
        &["sealed-000.raw", "writable.qcow2"],
    );
    sandbox(
        dir.path(),
        "new",
        "vol-new",
        &[("disk.raw", "empty")],
        &["disk.raw"],
    );
    clone_workspace(&dir.path().join("old"), &dir.path().join("new")).unwrap();

    let disk = dir.path().join("new/owned-volumes/workspace_abc");
    assert_eq!(
        fs::read_to_string(disk.join("sealed-000.raw")).unwrap(),
        "base"
    );
    assert_eq!(
        fs::read_to_string(disk.join("writable.qcow2")).unwrap(),
        "changes"
    );
    assert!(!disk.join("disk.raw").exists());
    let journal: serde_json::Value = serde_json::from_slice(
        &fs::read(
            dir.path()
                .join("new/runtime/owned-disks/workspace_abc/disk.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(journal["volume_id"], "vol-new");
    let paths: Vec<&str> = journal["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "owned-volumes/workspace_abc/sealed-000.raw",
            "owned-volumes/workspace_abc/writable.qcow2"
        ]
    );
    // The disk that was there waits outside `owned-volumes`; nothing is left half-staged; the
    // source is untouched.
    assert_eq!(
        fs::read_to_string(dir.path().join("new/.workspace-before-swap/disk.raw")).unwrap(),
        "empty"
    );
    let left: Vec<String> = fs::read_dir(dir.path().join("new/owned-volumes"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["workspace_abc"]);
    assert!(!dir
        .path()
        .join("new/.workspace-swap-workspace_abc")
        .exists());
    assert_eq!(
        fs::read_to_string(
            dir.path()
                .join("old/owned-volumes/workspace_abc/writable.qcow2")
        )
        .unwrap(),
        "changes"
    );
}

#[test]
fn a_single_raw_workspace_disk_keeps_the_new_sandboxes_journal() {
    let dir = tempfile::tempdir().unwrap();
    if !can_clone(dir.path()) {
        return;
    }
    // The old computer never started, so it has a disk and no journal.
    write(
        &dir.path().join("old/owned-volumes/workspace_abc/disk.raw"),
        "data",
    );
    sandbox(
        dir.path(),
        "new",
        "vol-new",
        &[("disk.raw", "empty")],
        &["disk.raw"],
    );
    let journal = dir
        .path()
        .join("new/runtime/owned-disks/workspace_abc/disk.json");
    let before = fs::read(&journal).unwrap();
    clone_workspace(&dir.path().join("old"), &dir.path().join("new")).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("new/owned-volumes/workspace_abc/disk.raw")).unwrap(),
        "data"
    );
    assert_eq!(fs::read(&journal).unwrap(), before);
}

#[test]
fn the_owner_marker_of_a_workspace_disk_stays_the_new_sandboxs_own() {
    let dir = tempfile::tempdir().unwrap();
    if !can_clone(dir.path()) {
        return;
    }
    sandbox(
        dir.path(),
        "old",
        "vol-old",
        &[("disk.raw", "data"), (".disk-owner", "old")],
        &["disk.raw"],
    );
    sandbox(
        dir.path(),
        "new",
        "vol-new",
        &[("disk.raw", "empty"), (".disk-owner", "new")],
        &["disk.raw"],
    );
    clone_workspace(&dir.path().join("old"), &dir.path().join("new")).unwrap();
    let disk = dir.path().join("new/owned-volumes/workspace_abc");
    assert_eq!(fs::read_to_string(disk.join("disk.raw")).unwrap(), "data");
    assert_eq!(fs::read_to_string(disk.join(".disk-owner")).unwrap(), "new");

    // A new sandbox whose disk has no marker does not get the old one's.
    sandbox(
        dir.path(),
        "plain",
        "vol-plain",
        &[("disk.raw", "empty")],
        &["disk.raw"],
    );
    clone_workspace(&dir.path().join("old"), &dir.path().join("plain")).unwrap();
    assert!(!dir
        .path()
        .join("plain/owned-volumes/workspace_abc/.disk-owner")
        .exists());
}

#[test]
fn the_workspace_disk_is_not_touched_when_the_new_sandbox_cannot_take_it() {
    let dir = tempfile::tempdir().unwrap();
    // The new sandbox was never started: no journal of its own to take the volume id from.
    sandbox(
        dir.path(),
        "old",
        "vol-old",
        &[("disk.raw", "data")],
        &["disk.raw"],
    );
    write(
        &dir.path().join("new/owned-volumes/workspace_abc/disk.raw"),
        "empty",
    );
    let error = clone_workspace(&dir.path().join("old"), &dir.path().join("new")).unwrap_err();
    assert!(error.contains("not been started once"), "{error}");
    assert_eq!(
        fs::read_to_string(dir.path().join("new/owned-volumes/workspace_abc/disk.raw")).unwrap(),
        "empty"
    );
    assert!(!dir.path().join("new/.workspace-before-swap").exists());

    // Two workspace disks in one sandbox are not understood.
    write(
        &dir.path().join("old/owned-volumes/workspace_def/disk.raw"),
        "more",
    );
    let error = clone_workspace(&dir.path().join("old"), &dir.path().join("new")).unwrap_err();
    assert!(error.contains("exactly one workspace disk"), "{error}");
}

#[test]
fn the_ssh_folder_is_copied_with_its_modes_and_nothing_is_skipped() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("old/ssh");
    write(&from.join("host_ed25519"), "private");
    fs::set_permissions(from.join("host_ed25519"), fs::Permissions::from_mode(0o600)).unwrap();
    write(&from.join("host_ed25519.pub"), "public");
    let to = dir.path().join("new/ssh");
    write(&to.join("host_ed25519"), "generated by the first start");
    copy_folder(&from, &to).unwrap();
    assert_eq!(
        fs::read_to_string(to.join("host_ed25519")).unwrap(),
        "private"
    );
    assert_eq!(
        fs::read_to_string(to.join("host_ed25519.pub")).unwrap(),
        "public"
    );
    let mode = |path: &std::path::Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&to.join("host_ed25519")), 0o600);
    assert_eq!(mode(&to), 0o700);

    // A computer without SSH access has nothing to copy.
    copy_folder(&dir.path().join("none"), &dir.path().join("new/other")).unwrap();
    assert!(!dir.path().join("new/other").exists());

    // Anything but files is an error, not a silent skip.
    std::os::unix::fs::symlink("/etc/hosts", from.join("link")).unwrap();
    assert!(copy_folder(&from, &dir.path().join("third")).is_err());
}

// MARK: The build's network policy

use crate::network_policy::{
    self,
    store::{self, Stored},
    ExtraRule, Grants, NetworkPolicy, NetworkPolicyMode,
};

fn policy_store() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    store::use_test_path(Some(dir.path().join("network-policy.json")));
    dir
}

fn allowlist_stored() -> Stored {
    Stored::preserving_pins(
        &NetworkPolicy {
            mode: NetworkPolicyMode::Allowlist,
            hosts: vec!["example.com".into()],
            presets: Vec::new(),
        },
        None,
        true,
    )
}

fn closed_runtime_policy() -> serde_json::Value {
    serde_json::json!({"default_egress": "deny", "default_ingress": "deny", "rules": []})
}

#[test]
fn the_build_and_the_undo_computer_get_the_saved_policy_of_the_computer() {
    let _dir = policy_store();
    let plan = plan();
    store::set_stored(&plan.old_id, allowlist_stored()).unwrap();
    save_build_policies(&plan, None).unwrap();
    assert_eq!(
        store::get_stored(&plan.new_id).unwrap(),
        Some(allowlist_stored())
    );
    assert_eq!(
        store::get_stored(&plan.undo_id).unwrap(),
        Some(allowlist_stored())
    );
    store::use_test_path(None);
}

#[test]
fn a_computer_without_a_saved_policy_gives_its_effective_one_to_the_build() {
    let _dir = policy_store();
    let plan = plan();
    // An import that has not started yet carries its closed rules and no saved policy.
    save_build_policies(&plan, Some(&closed_runtime_policy())).unwrap();
    let built = store::get_stored(&plan.new_id)
        .unwrap()
        .expect("the build has a policy");
    assert_eq!(built.mode, NetworkPolicyMode::Closed);
    assert!(built.incoming_denied);
    assert_eq!(store::get_stored(&plan.undo_id).unwrap(), Some(built));
    // A computer saved before policies existed runs open, and the build says so explicitly.
    let open_plan = Plan {
        new_id: "id-new-2".into(),
        undo_id: "id-undo-2".into(),
        ..plan
    };
    save_build_policies(
        &open_plan,
        Some(&serde_json::json!({"default_egress": "allow"})),
    )
    .unwrap();
    let built = store::get_stored(&open_plan.new_id)
        .unwrap()
        .expect("the build has a policy");
    assert_eq!(built.mode, NetworkPolicyMode::Open);
    assert!(!built.incoming_denied);
    store::use_test_path(None);
}

#[test]
fn the_build_policy_is_not_saved_when_the_computers_policy_cannot_be_read() {
    let dir = policy_store();
    let plan = plan();
    std::fs::write(dir.path().join("network-policy.json"), b"not json").unwrap();
    assert!(save_build_policies(&plan, None).is_err());
    store::use_test_path(None);
}

#[test]
fn the_undo_computer_keeps_an_explicit_policy_when_the_original_had_none() {
    let _dir = policy_store();
    let plan = plan();
    save_build_policies(&plan, Some(&closed_runtime_policy())).unwrap();
    let saved = store::get_stored(&plan.undo_id).unwrap();
    assert!(saved.is_some());
    // The swap binds it again on every retry, and the original still has no saved policy.
    bind_undo_policy(&plan, &|| Ok(Some(closed_runtime_policy()))).unwrap();
    bind_undo_policy(&plan, &|| Ok(Some(closed_runtime_policy()))).unwrap();
    assert_eq!(store::get_stored(&plan.undo_id).unwrap(), saved);
    assert_eq!(store::get_stored(&plan.old_id).unwrap(), None);
    // An original with a saved policy gives it, with its approvals.
    store::set_stored(&plan.old_id, allowlist_stored()).unwrap();
    bind_undo_policy(&plan, &|| Ok(None)).unwrap();
    assert_eq!(
        store::get_stored(&plan.undo_id).unwrap(),
        Some(allowlist_stored())
    );
    store::use_test_path(None);
}

#[test]
fn the_undo_computer_does_not_keep_the_channel_rule_of_the_original() {
    let _dir = policy_store();
    let plan = plan();
    save_build_policies(&plan, Some(&closed_runtime_policy())).unwrap();
    bind_undo_policy(&plan, &|| Ok(Some(closed_runtime_policy()))).unwrap();
    // The original's sandbox holds the rule for its channel port.
    let held = store::get_stored(&plan.old_id).unwrap().unwrap_or_else(|| {
        Stored::of(
            network_policy::held_for(&plan.old_id, Some(&closed_runtime_policy()))
                .unwrap()
                .0,
            true,
        )
    });
    let mut original = network_policy::plan_held(
        &held.held(),
        &Grants::of_computer("dev"),
        &[ExtraRule::GatewayPort(40001)],
    );
    original.ingress_allowed = !held.incoming_denied;
    let runtime = serde_json::json!({"policy": original.policy_value(), "strict": original.strict});
    // The undo computer's saved policy plans no such rule, so its start finds the difference
    // and applies the plan.
    let undo = store::get_stored(&plan.undo_id)
        .unwrap()
        .expect("the undo computer has a policy");
    let mut wanted = network_policy::plan_held(&undo.held(), &Grants::of_computer("dev-undo"), &[]);
    wanted.ingress_allowed = !undo.incoming_denied;
    assert!(original.matches(&runtime));
    assert!(!wanted.matches(&runtime));
    store::use_test_path(None);
}

#[test]
fn the_policies_of_a_discarded_build_are_forgotten() {
    let _dir = policy_store();
    let plan = plan();
    save_build_policies(&plan, Some(&closed_runtime_policy())).unwrap();
    store::set_stored(&plan.old_id, allowlist_stored()).unwrap();
    forget_build_policies(&plan).unwrap();
    assert_eq!(store::get_stored(&plan.new_id).unwrap(), None);
    assert_eq!(store::get_stored(&plan.undo_id).unwrap(), None);
    assert_eq!(
        store::get_stored(&plan.old_id).unwrap(),
        Some(allowlist_stored())
    );
    store::use_test_path(None);
}

// MARK: Pausing, cancelling, settling and abandoning

#[test]
fn the_home_folder_is_packed_while_the_working_accounts_processes_are_paused() {
    let stop = PACK_HOME.find("signal STOP").unwrap();
    let tar = PACK_HOME.find("tar -C /home -cp").unwrap();
    let check = PACK_HOME.find("-cnewer").unwrap();
    let resume = PACK_HOME.rfind("signal CONT").unwrap();
    // Paused before tar reads, checked for later changes afterwards, and let go again.
    assert!(stop < tar && tar < check && check < resume);
    // The processes continue however the script ends.
    assert!(PACK_HOME.contains("trap resume EXIT"));
    // A change under the pause or a tar that saw one is tried once more, then fails.
    assert!(PACK_HOME.contains("for attempt in 1 2"));
    assert!(PACK_HOME.contains("kept changing while it was packed"));
    assert!(PACK_HOME.contains("[ \"$rc\" -eq 0 ] && [ -z \"$newer\" ]"));
    // The size limit is checked before anything is paused.
    assert!(PACK_HOME.find("exit 6").unwrap() < stop);
}

#[test]
fn a_person_can_cancel_between_the_steps_until_the_swap_begins() {
    for label in [
        "stopping the computer",
        "building the new computer",
        "carrying the home folder",
        "copying the workspace disk",
        "swapping the computers",
    ] {
        let id = format!("c-cancel-{}", label.len());
        let fake = Fake::new(&id, true);
        *fake.cancel_on.lock().unwrap() = Some(label.to_owned());
        let (_dir, journal) = journal();
        let error = rebuild(&fake, &journal, &id, false, WAIT).unwrap_err();
        assert!(error.contains("cancelled"), "{label}: {error}");
        assert!(
            error.contains("rolled back; dev is as it was"),
            "{label}: {error}"
        );
        assert!(!fake.called("swap_apply"), "{label}");
        assert!(journal.all().is_empty(), "{label}");
        assert_eq!(fake.table.named("dev").unwrap().label, id, "{label}");
        assert_eq!(fake.table.named("silo-rebuild-aaaa1111"), None, "{label}");
        assert!(fake.running(&id), "{label}");
        assert!(fake.unshielded.lock().unwrap().is_empty(), "{label}");
    }
}

#[test]
fn a_cancel_asked_while_the_swap_runs_does_not_stop_it() {
    let mut fake = Fake::new("c-late-cancel", true);
    fake.cancel_during_swap = true;
    let (_dir, journal) = journal();
    rebuild(&fake, &journal, "c-late-cancel", false, WAIT).unwrap();
    assert_eq!(after_tags(&fake.table), [Some("new"), Some("old"), None]);
    assert!(fake.called("settle_swapped") && !fake.called("swap_revert"));
    assert!(journal.all().is_empty());
    assert!(fake.running("c-late-cancel"));
}

#[test]
fn the_step_is_shown_where_the_rebuild_is_listed() {
    let fake = Fake::new("c-labels", false);
    let (_dir, journal) = journal();
    rebuild(&fake, &journal, "c-labels", false, WAIT).unwrap();
    let labels = fake.labels.lock().unwrap().clone();
    let order = [
        "Rebuilding dev: stopping the computer",
        "Rebuilding dev: saving a checkpoint to go back to",
        "Rebuilding dev: building the new computer",
        "Rebuilding dev: carrying the home folder",
        "Rebuilding dev: copying the workspace disk",
        "Rebuilding dev: installing the recipe on the new image",
        "Rebuilding dev: swapping the computers",
    ];
    assert_eq!(labels, order);
}

#[test]
fn the_rollback_the_swap_and_the_cleanup_run_protected_from_a_cancel() {
    // A rebuild that goes through, one rolled back, and the recoveries of both kinds.
    let fake = Fake::new("c-shield-ok", true);
    let (_dir, journal) = journal();
    rebuild(&fake, &journal, "c-shield-ok", false, WAIT).unwrap();
    assert_eq!(*fake.unshielded.lock().unwrap(), Vec::<String>::new());

    let fake = Fake::new("c-shield-bad", true);
    fake.fail_once_in("move_workspace");
    rebuild(&fake, &journal, "c-shield-bad", false, WAIT).unwrap_err();
    assert!(fake.called("remove_build"));
    assert_eq!(*fake.unshielded.lock().unwrap(), Vec::<String>::new());

    for (n, step) in [Step::Created, Step::Swapping, Step::Swapped]
        .into_iter()
        .enumerate()
    {
        let id = format!("c-shield-rec-{n}");
        let (fake, _dir, journal) = interrupted(&id, step, true);
        recover_one(&fake, &journal, &id, &|| {}, 1, Duration::ZERO).unwrap();
        assert_eq!(
            *fake.unshielded.lock().unwrap(),
            Vec::<String>::new(),
            "{step:?}"
        );
    }
}

#[test]
fn a_failed_cleanup_after_the_swap_keeps_the_journal_and_never_undoes_the_rebuild() {
    let fake = Fake::new("c-settle", true);
    fake.fail_once_in("settle_swapped");
    let (_dir, journal) = journal();
    let error = rebuild(&fake, &journal, "c-settle", false, WAIT).unwrap_err();
    assert!(error.contains("was rebuilt, but cleaning up"), "{error}");
    assert!(error.contains("next time it starts"), "{error}");
    // The swap stands, the computer runs, and the journal waits for the cleanup.
    assert_eq!(after_tags(&fake.table), [Some("new"), Some("old"), None]);
    assert!(!fake.called("swap_revert") && !fake.called("remove_build"));
    assert!(fake.running("c-settle"));
    let left = journal.get("c-settle").unwrap();
    assert_eq!(left.step, Step::Swapped);

    // The next start finishes it.
    recover_one(&fake, &journal, "c-settle", &|| {}, 1, Duration::ZERO).unwrap();
    assert!(journal.all().is_empty());
    assert_eq!(
        fake.calls()
            .iter()
            .filter(|c| *c == "settle_swapped")
            .count(),
        2
    );
    assert_eq!(after_tags(&fake.table), [Some("new"), Some("old"), None]);
}

#[test]
fn the_computer_is_started_again_even_when_the_cleanup_fails() {
    let fake = Fake::new("c-start-anyway", true);
    fake.fail_once_in("settle_swapped");
    let (_dir, journal) = journal();
    rebuild(&fake, &journal, "c-start-anyway", false, WAIT).unwrap_err();
    let calls = fake.calls();
    let settled = calls.iter().position(|c| c == "settle_swapped").unwrap();
    assert!(
        calls[settled..].iter().any(|c| c == "start c-start-anyway"),
        "{calls:?}"
    );
}

#[test]
fn a_home_folder_that_cannot_be_measured_without_starting_is_left_to_the_packing() {
    let mut fake = Fake::new("c-unknown-home", false);
    fake.home_unknown = true;
    let (_dir, journal) = journal();
    rebuild(&fake, &journal, "c-unknown-home", false, WAIT).unwrap();
    // Nothing was started before the journal existed.
    let calls = fake.calls();
    let first_start = calls.iter().position(|c| c.starts_with("start")).unwrap();
    let measured = calls.iter().position(|c| c == "home_bytes").unwrap();
    let stopped = calls
        .iter()
        .position(|c| c.starts_with("stop") || c.starts_with("checkpoint"));
    assert!(measured < first_start, "{calls:?}");
    assert!(stopped.is_some());
}

#[test]
fn a_rollback_from_the_home_folder_step_cleans_the_old_computer() {
    // A stop in the middle of carrying the home folder leaves the tar file in the old computer.
    let (fake, _dir, journal) = interrupted("c-leftover", Step::Created, false);
    recover_one(&fake, &journal, "c-leftover", &|| {}, 1, Duration::ZERO).unwrap();
    assert!(fake.called("clean_old_computer"));
    let calls = fake.calls();
    let cleaned = calls
        .iter()
        .position(|c| c.starts_with("clean_old_computer"))
        .unwrap();
    let removed = calls.iter().position(|c| c == "remove_build").unwrap();
    assert!(cleaned < removed);
    // Other steps never carried the home folder.
    let (fake, _dir, journal) = interrupted("c-no-leftover", Step::WorkspaceMoved, false);
    recover_one(&fake, &journal, "c-no-leftover", &|| {}, 1, Duration::ZERO).unwrap();
    assert!(!fake.called("clean_old_computer"));
}

#[test]
fn the_gate_is_taken_again_on_each_attempt_until_it_is_held() {
    let (fake, _dir, journal) = interrupted("c-late-gate", Step::Created, false);
    *fake.hold_failures.lock().unwrap() = 2;
    let held = Mutex::new(0);
    recover_one(
        &fake,
        &journal,
        "c-late-gate",
        &|| *held.lock().unwrap() += 1,
        4,
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(*held.lock().unwrap(), 1);
    assert_eq!(
        fake.calls()
            .iter()
            .filter(|c| c.starts_with("hold"))
            .count(),
        3
    );
    assert!(journal.all().is_empty());
    assert_eq!(*fake.notices.lock().unwrap(), [None]);
}

#[test]
fn the_owner_is_told_how_to_get_out_when_the_last_attempt_fails() {
    let (fake, _dir, journal) = interrupted("c-told", Step::Created, false);
    *fake.broken_remove.lock().unwrap() = true;
    recover_one(&fake, &journal, "c-told", &|| {}, 2, Duration::ZERO).unwrap_err();
    let notices = fake.notices.lock().unwrap().clone();
    assert_eq!(notices.len(), 1);
    let told = notices[0].clone().unwrap();
    assert!(told.contains("rebuild of dev was interrupted"), "{told}");
    assert!(told.contains("silo rebuild dev --abandon"), "{told}");
    assert!(told.contains("cannot be removed"), "{told}");
    // A rebuild asked for meanwhile says the same instead of failing without a word.
    let error = rebuild(&fake, &journal, "c-told", false, WAIT).unwrap_err();
    assert!(error.contains("--abandon"), "{error}");
}

/// A device that stopped in the middle of the swap and then lost the build sandbox, so the
/// swap is in none of the states it knows.
fn lost_build(id: &str, was_running: bool) -> (Fake, tempfile::TempDir, Journal) {
    let (fake, dir, journal) = interrupted(id, Step::Swapping, was_running);
    let plan = journal.get(id).unwrap().plan;
    // `dev` was renamed to `dev-undo` and the build is gone.
    *fake.table.sandboxes.lock().unwrap() = vec![Sandbox {
        name: plan.undo_name.clone(),
        label: plan.old_id.clone(),
        tag: "old",
    }];
    fake.machines.lock().unwrap().remove(&plan.new_id);
    fake.machines.lock().unwrap().remove(&plan.undo_id);
    fake.machines.lock().unwrap().get_mut(id).unwrap().running = false;
    fake.calls.lock().unwrap().clear();
    (fake, dir, journal)
}

#[test]
fn a_swap_in_no_known_state_blocks_every_start_and_rebuild_until_it_is_abandoned() {
    let (fake, _dir, journal) = lost_build("c-lost", true);
    let error = recover_one(&fake, &journal, "c-lost", &|| {}, 2, Duration::ZERO).unwrap_err();
    assert!(
        error.contains("not the computers the rebuild expected"),
        "{error}"
    );
    assert_eq!(journal.all().len(), 1);
    let error = rebuild(&fake, &journal, "c-lost", false, WAIT).unwrap_err();
    assert!(error.contains("--abandon"), "{error}");
    assert_eq!(journal.all().len(), 1);
    assert!(fake.notices.lock().unwrap().iter().any(Option::is_some));
}

#[test]
fn abandoning_puts_the_old_computer_back_by_its_identity_and_starts_it() {
    for was_running in [true, false] {
        let id = format!("c-abandon-{was_running}");
        let (fake, _dir, journal) = lost_build(&id, was_running);
        let result = super::abandon(&fake, &journal, &id, WAIT).unwrap();
        assert_eq!(result.name, "dev");
        assert_eq!(result.computer_id, id);
        assert!(result.undo_name.is_empty());
        // The old sandbox is `dev` again, under the computer's id.
        let dev = fake.table.named("dev").unwrap();
        assert_eq!((dev.label.as_str(), dev.tag), (id.as_str(), "old"));
        assert_eq!(fake.table.named("dev-undo"), None);
        assert!(fake.called("swap_revert") && fake.called("restore_original"));
        assert!(fake.called("remove_build"));
        assert_eq!(fake.running(&id), was_running, "{was_running}");
        assert!(journal.all().is_empty());
        assert_eq!(fake.notices.lock().unwrap().last(), Some(&None));
        assert_eq!(*fake.unshielded.lock().unwrap(), Vec::<String>::new());
        // The computer can be rebuilt again.
        let mut again = Fake::new(&id, was_running);
        again.ids = Mutex::new(vec!["id-n2".into(), "id-n1".into()]);
        let (_dir2, journal2) = self::journal();
        rebuild(&again, &journal2, &id, false, WAIT).unwrap();
    }
}

#[test]
fn abandoning_a_rebuild_whose_swap_finished_returns_the_old_computer_too() {
    let (fake, _dir, journal) = interrupted("c-abandon-done", Step::Swapped, true);
    super::abandon(&fake, &journal, "c-abandon-done", WAIT).unwrap();
    assert!(fake.called("swap_revert") && !fake.called("restore_original"));
    assert_eq!(after_tags(&fake.table), [Some("old"), None, None]);
    assert!(journal.all().is_empty());
}

#[test]
fn nothing_is_abandoned_when_no_rebuild_waits() {
    let fake = Fake::new("c-nothing", true);
    let (_dir, journal) = journal();
    let error = super::abandon(&fake, &journal, "c-nothing", WAIT).unwrap_err();
    assert!(
        error.contains("No rebuild of this computer is waiting"),
        "{error}"
    );
    assert_eq!(fake.changes(), Vec::<String>::new());
}

#[test]
fn the_old_sandbox_is_found_by_its_identity_in_every_state_a_crash_can_leave() {
    let plan = plan();
    let sandbox =
        |name: &str, label: &str, tag: &'static str| (name.to_owned(), label.to_owned(), tag);
    // (the sandboxes, whether the swap had begun)
    let cases: Vec<(Vec<(String, String, &'static str)>, bool)> = vec![
        // The build was renamed away from its name.
        (
            vec![
                sandbox("dev", "id-old", "old"),
                sandbox("other", "id-new", "new"),
            ],
            false,
        ),
        // The undo name was dropped after the swap.
        (
            vec![
                sandbox("dev-undo", "id-undo", "old"),
                sandbox("dev", "id-new", "new"),
            ],
            true,
        ),
        // The old sandbox is under its undo name, labelled with the computer's id.
        (
            vec![
                sandbox("dev-undo", "id-old", "old"),
                sandbox("silo-rebuild-aaaa1111", "id-new", "new"),
            ],
            true,
        ),
        // Fully swapped.
        (
            vec![
                sandbox("dev-undo", "id-undo", "old"),
                sandbox("dev", "id-old", "new"),
            ],
            true,
        ),
    ];
    for (n, (sandboxes, swapping)) in cases.into_iter().enumerate() {
        let table = Table::default();
        for (name, label, tag) in &sandboxes {
            table.sandboxes.lock().unwrap().push(Sandbox {
                name: name.clone(),
                label: label.clone(),
                tag,
            });
        }
        swap::restore_original(&table, &plan, swapping).unwrap_or_else(|e| panic!("case {n}: {e}"));
        let dev = table.named("dev").unwrap();
        assert_eq!((dev.label.as_str(), dev.tag), ("id-old", "old"), "case {n}");
        // The build keeps its own name and id.
        let build = table
            .sandboxes
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.tag == "new")
            .cloned();
        if let Some(build) = build {
            assert!(
                build.label == "id-new" && build.name != "dev",
                "case {n}: {build:?}"
            );
        }
    }
    // With nothing to tell the old sandbox by, nothing is touched.
    let table = Table::with(&[("dev", "id-old", "new")]);
    let error = swap::restore_original(&table, &plan, true).unwrap_err();
    assert!(error.contains("cannot tell which"), "{error}");
    assert!(table.log.lock().unwrap().is_empty());
}

#[test]
fn an_older_swap_gives_the_undo_computer_the_rules_its_sandbox_held() {
    let _dir = policy_store();
    let plan = plan();
    // Neither the computer nor the undo computer has a saved policy, and the sandbox that is
    // now the undo computer held closed rules.
    bind_undo_policy(&plan, &|| Ok(Some(closed_runtime_policy()))).unwrap();
    let undo = store::get_stored(&plan.undo_id)
        .unwrap()
        .expect("the undo computer has a policy");
    assert_eq!(undo.mode, NetworkPolicyMode::Closed);
    assert!(undo.incoming_denied);
    assert_eq!(store::get_stored(&plan.old_id).unwrap(), None);
    store::use_test_path(None);
}

#[test]
fn an_undo_computer_is_closed_to_the_network_when_the_rules_cannot_be_read() {
    let _dir = policy_store();
    let plan = plan();
    let error = bind_undo_policy(&plan, &|| Err("no such sandbox".into())).unwrap_err();
    assert!(error.contains("no such sandbox"));
    assert_eq!(
        store::get_stored(&plan.undo_id).unwrap(),
        Some(Stored::closed_without_incoming())
    );
    assert!(ensure_undo_policy(&plan, &|| Err("no such sandbox".into())).is_err());
    assert_eq!(
        store::get_stored(&plan.undo_id).unwrap(),
        Some(Stored::closed_without_incoming())
    );
    // The rules replace the closed policy once they can be read.
    let open = serde_json::json!({"default_egress": "allow"});
    ensure_undo_policy(&plan, &|| Ok(Some(open.clone()))).unwrap();
    let bound = store::get_stored(&plan.undo_id).unwrap().unwrap();
    assert_eq!(bound.mode, NetworkPolicyMode::Open);
    store::use_test_path(None);
}

/// The undo computer's saved policy at each rename or relabel of the swap.
struct Watched<'a> {
    table: &'a Table,
    undo_id: String,
    seen: Mutex<Vec<Option<Stored>>>,
}

impl Watched<'_> {
    fn note(&self) {
        self.seen
            .lock()
            .unwrap()
            .push(store::get_stored(&self.undo_id).unwrap());
    }
}

impl swap::Sandboxes for Watched<'_> {
    fn owner(&self, name: &str) -> Result<Option<String>, String> {
        self.table.owner(name)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        self.note();
        self.table.rename(from, to)
    }

    fn relabel(&self, name: &str, id: &str) -> Result<(), String> {
        self.note();
        self.table.relabel(name, id)
    }
}

#[test]
fn the_undo_policy_is_saved_before_its_sandbox_is_published() {
    let _dir = policy_store();
    let plan = plan();
    let table = before();
    let watched = Watched {
        table: &table,
        undo_id: plan.undo_id.clone(),
        seen: Mutex::new(Vec::new()),
    };
    let closed = closed_runtime_policy();
    swap_with_policy(&plan, &watched, &|| Ok(Some(closed.clone())), &|| Ok(())).unwrap();
    let seen = watched.seen.lock().unwrap();
    assert!(!seen.is_empty());
    assert!(
        seen.iter().all(Option::is_some),
        "a sandbox moved before the undo computer had a policy: {seen:?}"
    );
    store::use_test_path(None);
}

#[test]
fn an_older_swap_that_cannot_read_the_rules_publishes_a_closed_undo_computer() {
    let _dir = policy_store();
    let plan = plan();
    let table = before();
    let watched = Watched {
        table: &table,
        undo_id: plan.undo_id.clone(),
        seen: Mutex::new(Vec::new()),
    };
    // The first attempt publishes the undo computer, then stops on the unread rules.
    let error =
        swap_with_policy(&plan, &watched, &|| Err("unreadable".into()), &|| Ok(())).unwrap_err();
    assert!(error.contains("unreadable"));
    assert!(watched
        .seen
        .lock()
        .unwrap()
        .iter()
        .all(|seen| *seen == Some(Stored::closed_without_incoming())));
    assert_eq!(
        store::get_stored(&plan.undo_id).unwrap(),
        Some(Stored::closed_without_incoming())
    );
    // The retry reads them and the closed policy gives way.
    let open = serde_json::json!({"default_egress": "allow"});
    swap_with_policy(&plan, &watched, &|| Ok(Some(open.clone())), &|| Ok(())).unwrap();
    assert_eq!(
        store::get_stored(&plan.undo_id).unwrap().unwrap().mode,
        NetworkPolicyMode::Open
    );
    store::use_test_path(None);
}

#[test]
fn a_swap_stopped_halfway_is_closed_before_it_goes_on() {
    let _dir = policy_store();
    let plan = plan();
    // Stopped after the old sandbox took the undo name, with no policy saved for it.
    let table = before();
    swap::Sandboxes::rename(&table, &plan.old_name, &plan.undo_name).unwrap();
    let watched = Watched {
        table: &table,
        undo_id: plan.undo_id.clone(),
        seen: Mutex::new(Vec::new()),
    };
    let error =
        swap_with_policy(&plan, &watched, &|| Err("unreadable".into()), &|| Ok(())).unwrap_err();
    assert!(error.contains("unreadable"));
    assert!(watched.seen.lock().unwrap().iter().all(Option::is_some));
    store::use_test_path(None);
}

#[test]
fn the_original_is_found_under_whichever_name_the_swap_left_it() {
    let plan = plan();
    let table = before();
    assert_eq!(swap::original_name(&table, &plan).unwrap(), "dev");
    swap::apply(&table, &plan).unwrap();
    assert_eq!(swap::original_name(&table, &plan).unwrap(), "dev-undo");
    let strangers = Table::with(&[("dev", "someone-else", "old")]);
    assert!(swap::original_name(&strangers, &plan).is_err());
}

#[test]
fn settling_a_swap_binds_the_undo_policy_and_keeps_one_it_has() {
    let _dir = policy_store();
    let plan = plan();
    store::set_stored(&plan.old_id, allowlist_stored()).unwrap();
    ensure_undo_policy(&plan, &|| Ok(None)).unwrap();
    assert_eq!(
        store::get_stored(&plan.undo_id).unwrap(),
        Some(allowlist_stored())
    );
    // A policy the undo computer already holds is not replaced by a later change to the computer.
    let held = Stored::preserving_pins(
        &NetworkPolicy {
            mode: NetworkPolicyMode::Allowlist,
            hosts: vec!["other.example".into()],
            presets: Vec::new(),
        },
        None,
        false,
    );
    store::set_stored(&plan.undo_id, held.clone()).unwrap();
    ensure_undo_policy(&plan, &|| Ok(None)).unwrap();
    assert_eq!(store::get_stored(&plan.undo_id).unwrap(), Some(held));
    store::use_test_path(None);
}
