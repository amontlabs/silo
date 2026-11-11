//! Unit tests for the rebuild flow, journal recovery, and refusal conditions.

use std::{
    any::Any,
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use silo_api::methods::recipes::{RecipeDefinition, RecipeOs, RecipeRecord, RecipeSpec};

use super::{
    journal::{Journal, RebuildRecord, RebuildState, RebuildStep},
    rebuild, recover_all, resume_or_rollback, ComputerInfo, RebuildEnv,
};

#[derive(Default)]
struct FakeRebuildEnv {
    computers: Mutex<HashMap<String, ComputerInfo>>,
    reflink_supported: Mutex<bool>,
    home_bytes: Mutex<u64>,
    calls: Mutex<Vec<String>>,
    snapshots: Mutex<Vec<String>>,
    replayed_recipes: Mutex<Vec<String>>,
    fail_at_step: Mutex<Option<String>>,
}

impl FakeRebuildEnv {
    fn new(info: ComputerInfo) -> Self {
        let mut map = HashMap::new();
        map.insert(info.name.clone(), info);
        Self {
            computers: Mutex::new(map),
            reflink_supported: Mutex::new(true),
            home_bytes: Mutex::new(100 * 1024 * 1024), // 100 MB default
            calls: Mutex::new(Vec::new()),
            snapshots: Mutex::new(Vec::new()),
            replayed_recipes: Mutex::new(Vec::new()),
            fail_at_step: Mutex::new(None),
        }
    }

    fn check_fail(&self, step: &str) -> Result<(), String> {
        let fail = self.fail_at_step.lock().unwrap();
        if let Some(target) = fail.as_ref() {
            if target == step {
                return Err(format!("Injected failure at {step}"));
            }
        }
        Ok(())
    }
}

impl RebuildEnv for FakeRebuildEnv {
    fn computer_info(&self, computer: &str) -> Result<ComputerInfo, String> {
        self.calls.lock().unwrap().push(format!("info:{computer}"));
        self.computers
            .lock()
            .unwrap()
            .get(computer)
            .cloned()
            .ok_or_else(|| format!("Unknown computer: {computer}"))
    }

    fn computer_exists(&self, name: &str) -> bool {
        self.computers.lock().unwrap().contains_key(name)
    }

    fn supports_reflink(&self) -> bool {
        *self.reflink_supported.lock().unwrap()
    }

    fn measure_home_bytes(&self, _computer: &str) -> Result<u64, String> {
        Ok(*self.home_bytes.lock().unwrap())
    }

    fn hold_lane(&self, computer_id: &str) -> Result<Box<dyn Any>, String> {
        self.calls.lock().unwrap().push(format!("hold_lane:{computer_id}"));
        Ok(Box::new(()))
    }

    fn stop_computer(&self, computer: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("stop:{computer}"));
        self.check_fail(&format!("stop:{computer}"))?;
        if let Some(c) = self.computers.lock().unwrap().get_mut(computer) {
            c.running = false;
        }
        Ok(())
    }

    fn start_computer(&self, computer: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("start:{computer}"));
        self.check_fail(&format!("start:{computer}"))?;
        if let Some(c) = self.computers.lock().unwrap().get_mut(computer) {
            c.running = true;
        }
        Ok(())
    }

    fn checkpoint_integrity(&self, computer: &str, name: &str) -> Result<String, String> {
        self.calls.lock().unwrap().push(format!("checkpoint:{computer}:{name}"));
        self.check_fail("checkpoint")?;
        self.snapshots.lock().unwrap().push(name.to_owned());
        Ok(name.to_owned())
    }

    fn newest_image(&self) -> Result<String, String> {
        Ok("ghcr.io/amontlabs/silo-guest:ubuntu-24.04-v5".into())
    }

    fn create_computer(
        &self,
        name: &str,
        id: &str,
        _image: &str,
        info: &ComputerInfo,
    ) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("create:{name}"));
        self.check_fail("create")?;
        let new_info = ComputerInfo {
            id: id.to_owned(),
            name: name.to_owned(),
            is_linux: true,
            running: false,
            cpus: info.cpus,
            max_cpus: info.max_cpus,
            memory_gib: info.memory_gib,
            max_memory_gib: info.max_memory_gib,
            root_disk_gib: info.root_disk_gib,
            workspace_storage_gib: info.workspace_storage_gib,
            desktop: info.desktop,
            has_checkpoint_layers: false,
        };
        self.computers.lock().unwrap().insert(name.to_owned(), new_info);
        Ok(())
    }

    fn setup_account(&self, computer: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("setup_account:{computer}"));
        self.check_fail("setup_account")?;
        Ok(())
    }

    fn move_workspace_disk(&self, from: &str, to: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("move_workspace:{from}->{to}"));
        self.check_fail("move_workspace")?;
        Ok(())
    }

    fn carry_home(&self, from: &str, to: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("carry_home:{from}->{to}"));
        self.check_fail("carry_home")?;
        Ok(())
    }

    fn copy_ssh_folder(&self, from: &str, to: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("copy_ssh:{from}->{to}"));
        self.check_fail("copy_ssh")?;
        Ok(())
    }

    fn replay_recipe(&self, computer: &str, spec: &RecipeSpec) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("replay_recipe:{computer}"));
        self.check_fail("replay_recipe")?;
        self.replayed_recipes.lock().unwrap().push(computer.to_owned());
        Ok(())
    }

    fn swap_names_and_relabel(
        &self,
        old_name: &str,
        old_id: &str,
        rebuild_name: &str,
        rebuild_id: &str,
        undo_name: &str,
        undo_id: &str,
    ) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("swap_names:{old_name}<->{rebuild_name}"));
        self.check_fail("swap_names")?;

        let mut comps = self.computers.lock().unwrap();
        if let Some(mut old_c) = comps.remove(old_name) {
            old_c.name = undo_name.to_owned();
            old_c.id = undo_id.to_owned();
            comps.insert(undo_name.to_owned(), old_c);
        }
        if let Some(mut reb_c) = comps.remove(rebuild_name) {
            reb_c.name = old_name.to_owned();
            reb_c.id = old_id.to_owned();
            comps.insert(old_name.to_owned(), reb_c);
        }
        Ok(())
    }

    fn delete_computer(&self, computer: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("delete:{computer}"));
        self.computers.lock().unwrap().remove(computer);
        Ok(())
    }
}

fn sample_computer(name: &str, id: &str) -> ComputerInfo {
    ComputerInfo {
        id: id.to_owned(),
        name: name.to_owned(),
        is_linux: true,
        running: true,
        cpus: 2,
        max_cpus: 2,
        memory_gib: 4,
        max_memory_gib: 4,
        root_disk_gib: 4,
        workspace_storage_gib: 10,
        desktop: true,
        has_checkpoint_layers: false,
    }
}

fn sample_recipe() -> RecipeRecord {
    RecipeRecord {
        name: "test-rec".into(),
        hash: "abcdef123456".into(),
        built_at: "2026-11-10T12:00:00Z".into(),
        recipe: RecipeDefinition {
            spec: RecipeSpec {
                os: RecipeOs::Linux,
                cpus: Some(2),
                memory_gib: Some(4),
                disk_gib: Some(10),
                packages: Default::default(),
                setup: vec!["echo hello".into()],
                services: Vec::new(),
                ports: Vec::new(),
                desktop: Some(true),
            },
            quiet: Vec::new(),
            scrub: Vec::new(),
            keep: None,
            test: None,
            files: Vec::new(),
        },
    }
}

#[test]
fn rebuild_succeeds_end_to_end_and_replays_recipe() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path());
    let env = FakeRebuildEnv::new(sample_computer("box", "box-id"));
    let rec = sample_recipe();

    let result = rebuild(&env, &journal, "box", Some(&rec)).expect("rebuild succeeded");

    assert_eq!(result.name, "box");
    assert_eq!(result.undo_name, "box-undo");
    assert_eq!(result.computer_id, "box-id");

    // Replay happened
    assert_eq!(env.replayed_recipes.lock().unwrap().len(), 1);

    // Old computer is now box-undo
    assert!(env.computer_exists("box-undo"));
    assert!(env.computer_exists("box"));

    // Integrity checkpoint was taken
    assert_eq!(env.snapshots.lock().unwrap().len(), 1);

    // Journal was cleaned up upon completion
    assert!(journal.all().is_empty());
}

#[test]
fn rebuild_refuses_macos_computer() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path());
    let mut info = sample_computer("mac-box", "mac-id");
    info.is_linux = false;
    let env = FakeRebuildEnv::new(info);

    let err = rebuild(&env, &journal, "mac-box", None).unwrap_err();
    assert!(err.contains("only supported for Linux computers"));
    assert!(journal.all().is_empty());
}

#[test]
fn rebuild_refuses_when_undo_computer_already_exists() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path());
    let env = FakeRebuildEnv::new(sample_computer("box", "box-id"));
    // Pre-insert box-undo
    env.computers.lock().unwrap().insert(
        "box-undo".into(),
        sample_computer("box-undo", "box-undo-id"),
    );

    let err = rebuild(&env, &journal, "box", None).unwrap_err();
    assert!(err.contains("already exists"));
    assert!(journal.all().is_empty());
}

#[test]
fn rebuild_refuses_when_layered_disk_and_no_reflink() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path());
    let mut info = sample_computer("box", "box-id");
    info.has_checkpoint_layers = true;
    let env = FakeRebuildEnv::new(info);
    *env.reflink_supported.lock().unwrap() = false;

    let err = rebuild(&env, &journal, "box", None).unwrap_err();
    assert!(err.contains("reflink"));
    assert!(journal.all().is_empty());
}

#[test]
fn rebuild_allows_single_file_disk_without_reflink() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path());
    let mut info = sample_computer("box", "box-id");
    info.has_checkpoint_layers = false;
    let env = FakeRebuildEnv::new(info);
    *env.reflink_supported.lock().unwrap() = false;

    let result = rebuild(&env, &journal, "box", None).expect("allowed when single file");
    assert_eq!(result.name, "box");
}

#[test]
fn rebuild_refuses_when_home_too_large() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path());
    let info = sample_computer("box", "box-id");
    let env = FakeRebuildEnv::new(info);
    // Home is 2.5 GB on a 4 GB root disk
    *env.home_bytes.lock().unwrap() = 2_500_000_000;

    let err = rebuild(&env, &journal, "box", None).unwrap_err();
    assert!(err.contains("too large for the new root disk"));
    assert!(journal.all().is_empty());
}

#[test]
fn rebuild_rolls_back_cleanly_on_failure_before_swap() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path());
    let env = FakeRebuildEnv::new(sample_computer("box", "box-id"));
    *env.fail_at_step.lock().unwrap() = Some("move_workspace".into());

    let err = rebuild(&env, &journal, "box", None).unwrap_err();
    assert!(err.contains("Injected failure at move_workspace"));

    // Rebuild computer was removed
    let comps = env.computers.lock().unwrap();
    assert!(comps.contains_key("box"));
    assert!(!comps.iter().any(|(k, _)| k.contains("-rebuild-")));

    // Journal reflects failure or rollback
    let rec = journal.get("box-id").unwrap();
    assert_eq!(rec.state, RebuildState::Failed);
}

#[test]
fn resume_or_rollback_cleans_up_interrupted_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path());
    let env = FakeRebuildEnv::new(sample_computer("box", "box-id"));

    // Pre-insert interrupted rebuild record
    let record = RebuildRecord {
        computer_id: "box-id".into(),
        computer_name: "box".into(),
        rebuild_id: "box-rebuild-123".into(),
        rebuild_name: "box-rebuild-123".into(),
        undo_id: "box-undo".into(),
        undo_name: "box-undo".into(),
        image: "ubuntu-24.04-v5".into(),
        state: RebuildState::Rebuilding,
        step: RebuildStep::WorkspaceMoved,
        checkpoint_id: Some("ck-1".into()),
        was_running: true,
        created_at: 1000,
    };
    journal.save(&record).unwrap();
    // Simulate rebuild sandbox existing
    env.computers.lock().unwrap().insert(
        "box-rebuild-123".into(),
        sample_computer("box-rebuild-123", "box-rebuild-123"),
    );

    let recovered = recover_all(&env, &journal).unwrap();
    assert_eq!(recovered, 1);

    // Sandbox removed
    assert!(!env.computer_exists("box-rebuild-123"));
    // Journal cleared
    assert!(journal.all().is_empty());
}
