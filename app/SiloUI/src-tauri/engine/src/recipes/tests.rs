use std::cell::RefCell;

use silo_api::methods::recipes::{RecipeOs, RecipePackages, RecipeService, RecipeSpec};

use super::*;

fn spec() -> RecipeSpec {
    RecipeSpec {
        os: RecipeOs::Linux,
        cpus: None,
        memory_gib: None,
        disk_gib: None,
        packages: RecipePackages {
            apt: vec!["git".into(), "curl".into()],
            npm: vec!["typescript".into()],
            ..RecipePackages::default()
        },
        setup: vec!["echo one".into(), "echo two".into()],
        services: vec![RecipeService {
            name: "web".into(),
            run: "python3 -m http.server 8080".into(),
        }],
        ports: vec![8080],
    }
}

#[test]
fn normalization_ignores_order_repeats_whitespace_and_default_sizes() {
    let mut other = spec();
    other.packages.apt = vec![" curl".into(), "git".into(), "git".into()];
    other.ports = vec![8080, 8080];
    other.cpus = Some(2);
    other.memory_gib = Some(4);
    other.disk_gib = Some(10);
    other.setup = vec!["echo one ".into(), "".into(), "echo two".into()];
    assert_eq!(hash(&spec()), hash(&other));
}

#[test]
fn the_hash_changes_with_anything_that_changes_the_build() {
    let base = hash(&spec());
    let mut changed = spec();
    changed.setup.reverse();
    assert_ne!(base, hash(&changed), "setup order is the run order");
    let mut changed = spec();
    changed.packages.apt.push("jq".into());
    assert_ne!(base, hash(&changed));
    let mut changed = spec();
    changed.cpus = Some(8);
    assert_ne!(base, hash(&changed));
    let mut changed = spec();
    changed.os = RecipeOs::Macos;
    assert_ne!(base, hash(&changed));
}

#[test]
fn validation_refuses_what_cannot_be_built() {
    let mut bad = spec();
    bad.packages.brew = vec!["wget".into()];
    assert!(validate(&bad).unwrap_err().contains("brew"));
    let mut bad = spec();
    bad.packages.apt = vec!["git; rm -rf /".into()];
    assert!(validate(&bad).unwrap_err().contains("apt"));
    let mut bad = spec();
    bad.services[0].name = "Web App".into();
    assert!(validate(&bad).unwrap_err().contains("service"));
    assert!(validate(&spec()).is_ok());
}

#[derive(Default)]
struct Fake {
    log: RefCell<Vec<String>>,
    fail_on: Option<&'static str>,
    macos_running: usize,
    existing: RefCell<Vec<(String, String)>>,
}

impl Fake {
    fn log(&self, line: impl Into<String>) {
        self.log.borrow_mut().push(line.into());
    }
    fn calls(&self, prefix: &str) -> usize {
        self.log
            .borrow()
            .iter()
            .filter(|line| line.starts_with(prefix))
            .count()
    }
}

impl BuildEnv for Fake {
    fn image_version(&self) -> String {
        "v5".into()
    }
    fn macos_running(&self) -> Result<usize, String> {
        Ok(self.macos_running)
    }
    fn find(&self, name: &str) -> Result<Option<String>, String> {
        Ok(self
            .existing
            .borrow()
            .iter()
            .find(|(_, existing)| existing == name)
            .map(|(id, _)| id.clone()))
    }
    fn create(
        &self,
        name: &str,
        _os: RecipeOs,
        cpus: u32,
        memory: u32,
        disk: u32,
    ) -> Result<Created, String> {
        self.log(format!("create {name} {cpus}/{memory}/{disk}"));
        let id = format!("id-{}", self.calls("create"));
        self.existing.borrow_mut().push((id.clone(), name.into()));
        Ok(Created {
            id,
            name: name.into(),
        })
    }
    fn run(
        &self,
        _computer: &str,
        who: RunAs,
        script: &str,
        _args: &[(String, String)],
    ) -> Result<ExecOut, String> {
        self.log(format!("run {who:?}"));
        if self.fail_on.is_some_and(|needle| script.contains(needle)) {
            return Ok(ExecOut {
                code: 7,
                stdout: Vec::new(),
                stderr: b"no such package".to_vec(),
            });
        }
        Ok(ExecOut::default())
    }
    fn checkpoint(&self, _computer: &str, name: &str) -> Result<String, String> {
        self.log(format!("checkpoint {name}"));
        Ok("cp-1".into())
    }
    fn checkpoint_exists(&self, _computer: &str, checkpoint: &str) -> bool {
        checkpoint == "cp-1"
    }
    fn stop(&self, computer: &str) -> Result<(), String> {
        self.log(format!("stop {computer}"));
        Ok(())
    }
    fn delete(&self, computer: &str) -> Result<(), String> {
        self.log(format!("delete {computer}"));
        self.existing.borrow_mut().retain(|(id, _)| id != computer);
        Ok(())
    }
    fn note(&self, _line: &str) {}
}

#[test]
fn a_build_runs_every_step_checkpoints_and_caches() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    let env = Fake::default();
    let outcome = build(&env, &cache, "dev", &spec(), false).unwrap();
    assert!(outcome.built);
    assert_eq!(outcome.entry.checkpoint_id, "cp-1");
    assert_eq!(outcome.entry.computer_name, "silo-build-dev");
    // apt, npm, two setup commands, one service, then two package checks and one service check.
    assert_eq!(env.calls("run"), 2 + 2 + 1 + 3);
    assert_eq!(env.calls("create silo-build-dev 2/4/10"), 1);
    assert_eq!(env.calls("stop"), 1);
    assert_eq!(cache.get("dev").unwrap().unwrap(), outcome.entry);
    assert_eq!(cache.list().len(), 1);
}

#[test]
fn the_same_spec_on_the_same_image_is_a_cache_hit() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    let env = Fake::default();
    build(&env, &cache, "dev", &spec(), false).unwrap();
    let again = build(&env, &cache, "dev", &spec(), false).unwrap();
    assert!(!again.built);
    assert_eq!(env.calls("create"), 1);
    let forced = build(&env, &cache, "dev", &spec(), true).unwrap();
    assert!(forced.built);
    assert_eq!(env.calls("create"), 2);
    // The rebuilt recipe replaced the first builder.
    assert_eq!(env.calls("delete id-1"), 1);
}

#[test]
fn a_failing_step_leaves_no_cache_entry_and_drops_the_builder() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    let env = Fake {
        fail_on: Some("npm install"),
        ..Fake::default()
    };
    let error = build(&env, &cache, "dev", &spec(), false).unwrap_err();
    assert!(
        error.contains("npm packages") && error.contains("exit 7"),
        "{error}"
    );
    assert!(error.contains("no such package"), "{error}");
    assert!(cache.get("dev").unwrap().is_none());
    assert_eq!(env.calls("delete id-1"), 1);
    assert_eq!(env.calls("checkpoint"), 0);
    assert!(env.existing.borrow().is_empty());
}

#[test]
fn a_failing_check_fails_the_build() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    let env = Fake {
        fail_on: Some("dpkg -s"),
        ..Fake::default()
    };
    let error = build(&env, &cache, "dev", &spec(), false).unwrap_err();
    assert!(error.contains("Checking the apt packages"), "{error}");
    assert!(cache.get("dev").unwrap().is_none());
    assert!(env.existing.borrow().is_empty());
}

#[test]
fn a_failed_rebuild_keeps_the_previous_build() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    let env = Fake::default();
    let first = build(&env, &cache, "dev", &spec(), false).unwrap();
    let broken = Fake {
        fail_on: Some("apt-get"),
        existing: RefCell::new(env.existing.borrow().clone()),
        ..Fake::default()
    };
    let mut changed = spec();
    changed.packages.apt.push("jq".into());
    assert!(build(&broken, &cache, "dev", &changed, false).is_err());
    assert_eq!(cache.get("dev").unwrap().unwrap(), first.entry);
}

#[test]
fn a_macos_build_needs_a_free_virtual_machine() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    let env = Fake {
        macos_running: 2,
        ..Fake::default()
    };
    let mut mac = spec();
    mac.os = RecipeOs::Macos;
    mac.packages = RecipePackages {
        brew: vec!["jq".into()],
        ..RecipePackages::default()
    };
    mac.services.clear();
    let error = build(&env, &cache, "mac", &mac, false).unwrap_err();
    assert!(error.contains("Two macOS"), "{error}");
    assert_eq!(env.calls("create"), 0);
}

#[test]
fn remove_drops_the_builder_and_the_entry() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    let env = Fake::default();
    build(&env, &cache, "dev", &spec(), false).unwrap();
    remove(&env, &cache, "dev").unwrap();
    assert!(cache.get("dev").unwrap().is_none());
    assert!(remove(&env, &cache, "dev").is_err());
}

#[test]
fn names_work_as_file_names() {
    assert!(valid_name("linux-dev"));
    assert!(!valid_name("../x"));
    assert!(!valid_name("Dev"));
    assert!(!valid_name(""));
}
