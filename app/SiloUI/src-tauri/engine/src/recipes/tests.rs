use std::sync::Mutex;

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
        desktop: None,
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
    assert_eq!(hash(&spec(), "v5"), hash(&other, "v5"));
}

#[test]
fn the_hash_changes_with_anything_that_changes_the_build() {
    let base = hash(&spec(), "v5");
    let mut changed = spec();
    changed.setup.reverse();
    assert_ne!(base, hash(&changed, "v5"), "setup order is the run order");
    let mut changed = spec();
    changed.packages.apt.push("jq".into());
    assert_ne!(base, hash(&changed, "v5"));
    let mut changed = spec();
    changed.cpus = Some(8);
    assert_ne!(base, hash(&changed, "v5"));
    let mut changed = spec();
    changed.os = RecipeOs::Macos;
    assert_ne!(base, hash(&changed, "v5"));
}

#[test]
fn only_an_off_desktop_changes_the_build() {
    let base = hash(&spec(), "v5");
    let mut on = spec();
    on.desktop = Some(true);
    assert_eq!(base, hash(&on, "v5"), "an on desktop is the default");
    let mut off = spec();
    off.desktop = Some(false);
    assert_ne!(base, hash(&off, "v5"));
    assert_eq!(normalize(&off).desktop, Some(false));
}

#[test]
fn a_recipe_without_a_desktop_builds_its_computer_without_one() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    let env = Fake::default();
    let mut lean = spec();
    lean.services.clear();
    lean.desktop = Some(false);
    build(&env, &cache, "lean", &lean, false).unwrap();
    assert!(env.log.lock().unwrap()[0].ends_with(" 2/4/10 no-desktop"));
    let with = Fake::default();
    build(&with, &cache, "dev", &spec(), false).unwrap();
    assert!(with.log.lock().unwrap()[0].ends_with(" 2/4/10"));
}

#[test]
fn a_macos_recipe_cannot_turn_the_desktop_off() {
    let mut mac = spec();
    mac.os = RecipeOs::Macos;
    mac.packages = RecipePackages::default();
    mac.desktop = Some(false);
    assert!(validate(&mac).unwrap_err().contains("macOS"));
    mac.desktop = Some(true);
    assert!(validate(&mac).is_ok());
}

#[test]
fn a_linux_recipe_without_a_desktop_cannot_have_services() {
    let mut lean = spec();
    lean.desktop = Some(false);
    assert!(validate(&lean).unwrap_err().contains("services"));
    lean.services.clear();
    assert!(validate(&lean).is_ok());
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
    log: Mutex<Vec<String>>,
    fail_on: Option<&'static str>,
    macos_running: usize,
    existing: Mutex<Vec<(String, String)>>,
    /// Added to the image version, so a test can move the image on.
    image: Mutex<String>,
    /// Slows `create` down, so builds that overlap would show.
    slow: bool,
}

static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

impl Fake {
    fn log(&self, line: impl Into<String>) {
        self.log.lock().unwrap().push(line.into());
    }
    fn calls(&self, prefix: &str) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.starts_with(prefix))
            .count()
    }
    fn names(&self) -> Vec<String> {
        self.existing
            .lock()
            .unwrap()
            .iter()
            .map(|(_, name)| name.clone())
            .collect()
    }
    fn add(&self, name: &str) -> String {
        let id = format!(
            "id-{}",
            NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        );
        self.existing
            .lock()
            .unwrap()
            .push((id.clone(), name.into()));
        id
    }
}

impl BuildEnv for Fake {
    fn image_version(&self, _os: RecipeOs) -> String {
        format!("v5{}", self.image.lock().unwrap())
    }
    fn macos_running(&self) -> Result<usize, String> {
        Ok(self.macos_running)
    }
    fn computers(&self) -> Result<Vec<(String, String)>, String> {
        Ok(self.existing.lock().unwrap().clone())
    }
    fn create(
        &self,
        name: &str,
        _os: RecipeOs,
        cpus: u32,
        memory: u32,
        disk: u32,
        desktop: bool,
    ) -> Result<Created, String> {
        self.log(format!(
            "create {name} {cpus}/{memory}/{disk}{}",
            if desktop { "" } else { " no-desktop" }
        ));
        if self.slow {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let id = self.add(name);
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
        self.existing
            .lock()
            .unwrap()
            .retain(|(id, _)| id != computer);
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
    assert!(is_builder_of(&outcome.entry.computer_name, "dev"));
    // apt, npm, two setup commands, one service, then two package checks and one service check.
    assert_eq!(env.calls("run"), 2 + 2 + 1 + 3);
    assert_eq!(env.calls("create silo-build-dev-"), 1);
    assert!(env.log.lock().unwrap()[0].ends_with(" 2/4/10"));
    assert_eq!(env.calls("stop"), 1);
    assert_eq!(cache.get("dev").unwrap().unwrap(), outcome.entry);
    assert_eq!(cache.list().len(), 1);
}

#[test]
fn the_same_spec_on_the_same_image_is_a_cache_hit() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    let env = Fake::default();
    let first = build(&env, &cache, "dev", &spec(), false).unwrap();
    let again = build(&env, &cache, "dev", &spec(), false).unwrap();
    assert!(!again.built);
    assert_eq!(env.calls("create"), 1);
    let forced = build(&env, &cache, "dev", &spec(), true).unwrap();
    assert!(forced.built);
    assert_eq!(env.calls("create"), 2);
    assert_ne!(first.entry.computer_name, forced.entry.computer_name);
    // The rebuilt recipe replaced the first builder, once the new one was made.
    assert_eq!(env.calls(&format!("delete {}", first.entry.computer_id)), 1);
    assert_eq!(env.names(), [forced.entry.computer_name]);
    let log = env.log.lock().unwrap();
    let created = log
        .iter()
        .position(|l| l.starts_with("create") && l != &log[0]);
    let dropped = log
        .iter()
        .position(|l| *l == format!("delete {}", first.entry.computer_id));
    assert!(created < dropped, "{log:?}");
}

#[test]
fn a_new_image_is_a_new_cache_entry() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    let env = Fake::default();
    let first = build(&env, &cache, "dev", &spec(), false).unwrap();
    assert_ne!(hash(&spec(), "v5"), hash(&spec(), "v6"));
    *env.image.lock().unwrap() = "-next".into();
    let second = build(&env, &cache, "dev", &spec(), false).unwrap();
    assert!(second.built);
    assert_ne!(first.entry.hash, second.entry.hash);
    assert_eq!(second.entry.image_version, "v5-next");
}

#[test]
fn builders_are_named_per_build_and_matched_exactly() {
    assert!(is_builder_of("silo-build-dev-0123abcd", "dev"));
    assert!(!is_builder_of("silo-build-dev-more-0123abcd", "dev"));
    assert!(is_builder_of("silo-build-dev-more-0123abcd", "dev-more"));
    assert!(!is_builder_of("silo-build-dev-0123abcg", "dev"));
    assert!(!is_builder_of("silo-build-dev", "dev"));
    assert_ne!(builder_name("dev"), builder_name("dev"));
}

#[test]
fn leftover_builders_of_this_recipe_are_dropped_and_no_others() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    let env = Fake::default();
    env.add("silo-build-dev-deadbeef");
    env.add("silo-build-dev-more-deadbeef");
    env.add("main");
    let outcome = build(&env, &cache, "dev", &spec(), false).unwrap();
    let names = env.names();
    assert!(!names.contains(&"silo-build-dev-deadbeef".to_owned()));
    assert!(names.contains(&"silo-build-dev-more-deadbeef".to_owned()));
    assert!(names.contains(&"main".to_owned()));
    assert!(names.contains(&outcome.entry.computer_name));
}

#[test]
fn builds_of_one_recipe_run_one_at_a_time_and_never_share_a_builder() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    let env = Fake {
        slow: true,
        ..Fake::default()
    };
    let (first, second) = std::thread::scope(|scope| {
        let one = scope.spawn(|| build(&env, &cache, "dev", &spec(), false).unwrap());
        let two = scope.spawn(|| build(&env, &cache, "dev", &spec(), false).unwrap());
        (one.join().unwrap(), two.join().unwrap())
    });
    // The second finds the first one's build in the cache.
    assert_eq!(env.calls("create"), 1);
    assert_eq!(
        [first.built, second.built].iter().filter(|b| **b).count(),
        1
    );
    assert_eq!(first.entry, second.entry);
    // Forced builds in turn each get their own builder, and none deletes one still in use.
    let (one, two) = std::thread::scope(|scope| {
        let one = scope.spawn(|| build(&env, &cache, "dev", &spec(), true).unwrap());
        let two = scope.spawn(|| build(&env, &cache, "dev", &spec(), true).unwrap());
        (one.join().unwrap(), two.join().unwrap())
    });
    assert_ne!(one.entry.computer_name, two.entry.computer_name);
    assert_eq!(env.calls("create"), 3);
    let kept = cache.get("dev").unwrap().unwrap();
    assert_eq!(env.names(), [kept.computer_name]);
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
    assert_eq!(env.calls("delete"), 1);
    assert_eq!(env.calls("checkpoint"), 0);
    assert!(env.names().is_empty());
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
    assert!(env.names().is_empty());
}

#[test]
fn a_failed_rebuild_keeps_the_previous_build_and_its_builder() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    let env = Fake::default();
    let first = build(&env, &cache, "dev", &spec(), false).unwrap();
    let broken = Fake {
        fail_on: Some("apt-get"),
        existing: Mutex::new(env.existing.lock().unwrap().clone()),
        ..Fake::default()
    };
    let mut changed = spec();
    changed.packages.apt.push("jq".into());
    assert!(build(&broken, &cache, "dev", &changed, false).is_err());
    assert_eq!(cache.get("dev").unwrap().unwrap(), first.entry);
    assert_eq!(broken.names(), [first.entry.computer_name.clone()]);
    // A forced rebuild that fails keeps it too.
    assert!(build(&broken, &cache, "dev", &spec(), true).is_err());
    assert_eq!(broken.names(), [first.entry.computer_name]);
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

/// The recipe builds made before the checkpoint itself wrote the guest disk out are rebuilt.
#[test]
fn the_build_version_retires_builds_made_before_the_disk_flush() {
    assert_ne!(BUILD_VERSION, "silo-recipe-build-2");
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

// MARK: Scripts

fn service_spec(os: RecipeOs, run: &str) -> RecipeSpec {
    RecipeSpec {
        os,
        cpus: None,
        memory_gib: None,
        disk_gib: None,
        packages: RecipePackages::default(),
        setup: Vec::new(),
        services: vec![RecipeService {
            name: "poller".into(),
            run: run.into(),
        }],
        ports: Vec::new(),
        desktop: None,
    }
}

fn run_script(step: &scripts::Step, env: &[(&str, &std::path::Path)]) -> std::process::Output {
    let mut command = std::process::Command::new("bash");
    command.arg("-c").arg(&step.script);
    for (name, value) in env {
        command.env(name, value);
    }
    for (name, value) in &step.args {
        command.env(format!("A_{name}"), value);
    }
    command.output().unwrap()
}

#[test]
fn a_linux_service_is_named_after_the_service_and_runs_its_own_command() {
    let step = &scripts::service_steps(&service_spec(
        RecipeOs::Linux,
        "/usr/local/bin/poller --every 5",
    ))[0];
    let root = tempfile::tempdir().unwrap();
    let out = run_script(step, &[("A_ROOT", root.path())]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let entry =
        std::fs::read_to_string(root.path().join("etc/xdg/autostart/poller.desktop")).unwrap();
    assert!(entry.contains("Name=poller\n"), "{entry}");
    let exec = entry.lines().find_map(|l| l.strip_prefix("Exec=")).unwrap();
    // `quiet: [poller]` matches the entry by file name and by the base name of its command.
    assert_eq!(exec.rsplit('/').next(), Some("poller"));
    let launcher =
        std::fs::read_to_string(root.path().join("usr/local/libexec/silo-services/poller"))
            .unwrap();
    assert_eq!(
        launcher,
        "#!/bin/sh\nexec /usr/local/bin/poller --every 5\n"
    );
}

#[test]
fn a_service_with_shell_syntax_runs_as_written() {
    for run in [
        "cd /app && ./poll",
        "FOO=1 poll",
        "poll > /var/log/poll.log",
    ] {
        let step = &scripts::service_steps(&service_spec(RecipeOs::Linux, run))[0];
        let root = tempfile::tempdir().unwrap();
        let out = run_script(step, &[("A_ROOT", root.path())]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let launcher =
            std::fs::read_to_string(root.path().join("usr/local/libexec/silo-services/poller"))
                .unwrap();
        assert_eq!(launcher, format!("#!/bin/sh\n{run}\n"));
    }
}

#[test]
fn a_service_check_looks_for_the_entry_named_after_the_service() {
    let checks = scripts::verify_steps(&service_spec(RecipeOs::Linux, "poller"));
    assert!(checks[0]
        .script
        .contains("/etc/xdg/autostart/$A_NAME.desktop"));
    let mac = scripts::verify_steps(&service_spec(RecipeOs::Macos, "poller"));
    assert!(mac[0].script.contains("LaunchAgents/$A_NAME.plist"));
}

#[cfg(target_os = "macos")]
#[test]
fn a_macos_service_has_a_launch_agent_labelled_with_the_service_name() {
    let step = &scripts::service_steps(&service_spec(RecipeOs::Macos, "/opt/bin/poller -v"))[0];
    let home = tempfile::tempdir().unwrap();
    let out = run_script(step, &[("HOME", home.path())]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let plist =
        plist::Value::from_file(home.path().join("Library/LaunchAgents/poller.plist")).unwrap();
    let dictionary = plist.as_dictionary().unwrap();
    assert_eq!(
        dictionary.get("Label").and_then(plist::Value::as_string),
        Some("poller")
    );
    let program = dictionary
        .get("ProgramArguments")
        .and_then(plist::Value::as_array)
        .and_then(|a| a.first())
        .and_then(plist::Value::as_string)
        .unwrap();
    assert!(program.ends_with("/silo-services/poller"), "{program}");
}

#[test]
fn the_pipx_check_uses_the_pipx_home_the_install_used() {
    let spec = RecipeSpec {
        packages: RecipePackages {
            pipx: vec!["ruff".into()],
            ..RecipePackages::default()
        },
        ..service_spec(RecipeOs::Linux, "x")
    };
    let install = &scripts::install_steps(&spec)[0];
    let check = &scripts::verify_steps(&spec)[0];
    for script in [&install.script, &check.script] {
        assert!(
            script.contains("PIPX_HOME=/opt/pipx PIPX_BIN_DIR=/usr/local/bin"),
            "{script}"
        );
    }
}

#[test]
fn macos_recipes_install_homebrew_before_they_need_it() {
    let mut spec = service_spec(RecipeOs::Macos, "x");
    spec.services.clear();
    spec.packages = RecipePackages {
        brew: vec!["jq".into()],
        pipx: vec!["ruff".into()],
        npm: vec!["typescript".into()],
        ..RecipePackages::default()
    };
    let steps = scripts::install_steps(&spec);
    assert_eq!(steps.len(), 3);
    for step in steps {
        assert!(step.script.contains("Homebrew/install"), "{}", step.script);
        let install = step.script.rfind("brew install").unwrap();
        let ensure = step
            .script
            .rfind("ensure_brew;")
            .or_else(|| step.script.rfind("ensure_brew\n"));
        assert!(ensure.unwrap() < install, "{}", step.script);
    }
    let linux = scripts::install_steps(&RecipeSpec {
        packages: RecipePackages {
            apt: vec!["jq".into()],
            ..RecipePackages::default()
        },
        ..service_spec(RecipeOs::Linux, "x")
    });
    assert!(!linux[0].script.contains("brew"));
}

mod stored {
    use silo_api::methods::{
        keep::KeepHooks,
        recipes::{RecipeDefinition, RecipeRecord},
    };

    use super::*;
    use crate::recipes::{
        additions::manager_key, append, built_in, hide_commands, hooks_of, record_merge, records,
        unrecord_merge, Additions, Library,
    };

    fn definition() -> RecipeDefinition {
        RecipeDefinition {
            spec: spec(),
            quiet: vec!["poller".into()],
            scrub: vec!["/home/silo/.env".into()],
            keep: Some(KeepHooks {
                restart: vec!["web".into()],
                migrate: vec!["npm run migrate".into()],
                after: vec!["npm test".into()],
            }),
            test: Some("npm test".into()),
            files: vec!["/home/silo/.bashrc".into()],
            commands_hidden: None,
        }
    }

    fn folder() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("silo-recipes-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn record(name: &str) -> RecipeRecord {
        RecipeRecord {
            name: name.into(),
            hash: "h".into(),
            built_at: "2026-11-11T00:00:00Z".into(),
            recipe: definition(),
        }
    }

    #[test]
    fn hiding_commands_keeps_names_packages_and_files() {
        let mut recipe = definition();
        recipe.spec.setup = vec!["export TOKEN=hunter2".into()];
        recipe.spec.services = vec![RecipeService {
            name: "web".into(),
            run: "web --token hunter2".into(),
        }];
        let shown = hide_commands(recipe.clone());
        assert_eq!(shown.commands_hidden, Some(true));
        assert!(shown.spec.setup.is_empty());
        assert_eq!(shown.spec.services[0].name, "web");
        assert!(shown.spec.services[0].run.is_empty());
        let keep = shown.keep.unwrap();
        assert_eq!(keep.restart, ["web"]);
        assert!(keep.migrate.is_empty() && keep.after.is_empty());
        assert_eq!(shown.test, recipe.test);
        assert_eq!(shown.files, recipe.files);
        assert_eq!(shown.spec.packages, recipe.spec.packages);
        assert_eq!(shown.quiet, recipe.quiet);
        assert_eq!(recipe.commands_hidden, None);
    }

    #[test]
    fn a_saved_recipe_keeps_every_key_and_replaces_the_shipped_one() {
        let app = folder();
        let library = Library::new(&app);
        assert!(library.get("web").unwrap().is_none());
        library.save("web", &definition()).unwrap();
        let (got, built_in_flag) = library.get("web").unwrap().unwrap();
        assert_eq!(got, definition());
        assert!(!built_in_flag);

        assert!(library.get("linux-dev").unwrap().unwrap().1);
        let mut mine = definition();
        mine.spec.packages.apt = vec!["vim".into()];
        library.save("linux-dev", &mine).unwrap();
        assert!(!library.get("linux-dev").unwrap().unwrap().1);
        assert!(library.remove("linux-dev").unwrap());
        assert_eq!(
            library.get("linux-dev").unwrap().unwrap().0,
            built_in("linux-dev").unwrap()
        );
        assert!(!library.remove("linux-dev").unwrap());

        let names: Vec<String> = library
            .list()
            .into_iter()
            .map(|(name, _, _)| name)
            .collect();
        assert_eq!(names, ["linux-dev", "macos-xcode", "web"]);
        std::fs::remove_dir_all(app).unwrap();
    }

    #[test]
    fn a_recipe_with_a_bad_name_or_entry_is_not_stored() {
        let library = Library::new(&folder());
        assert!(library.save("Bad Name", &definition()).is_err());
        let mut bad = definition();
        bad.scrub = vec!["relative/path".into()];
        assert!(library.save("web", &bad).unwrap_err().contains("absolute"));
        let mut bad = definition();
        bad.keep.as_mut().unwrap().after = vec!["one\ntwo".into()];
        assert!(library.save("web", &bad).is_err());
        let mut bad = definition();
        bad.spec.packages.apt = vec!["--evil".into()];
        assert!(library.save("web", &bad).is_err());
        assert_eq!(built_in_names(), ["linux-dev", "macos-xcode"]);
    }

    #[test]
    fn a_record_is_inherited_by_forks_and_forgotten_with_the_computer() {
        let app = folder();
        records::use_test_path(Some(app.join("recipe-records.json")));
        records::set("builder", &record("web")).unwrap();
        records::inherit("builder", "fork").unwrap();
        assert_eq!(records::get("fork").unwrap().unwrap().name, "web");
        // A fork of a computer with no record has none, even if its id had one before.
        records::inherit("nothing", "fork").unwrap();
        assert!(records::get("fork").unwrap().is_none());
        records::computer_removed("builder");
        assert!(records::get("builder").unwrap().is_none());
        records::use_test_path(None);
        std::fs::remove_dir_all(app).unwrap();
    }

    #[test]
    fn the_hooks_of_a_recipe_are_its_keep_section() {
        let hooks = hooks_of(&definition());
        assert_eq!(hooks.restart, ["web"]);
        assert_eq!(hooks.migrate, ["npm run migrate"]);
        let mut none = definition();
        none.keep = None;
        assert!(hooks_of(&none).after.is_empty());
    }

    #[test]
    fn a_fork_gets_the_quiet_and_scrub_lists_of_its_sources_recipe_once() {
        let app = folder();
        records::use_test_path(Some(app.join("recipe-records.json")));
        records::set("builder", &record("web")).unwrap();
        let asked = crate::runtime::checkpoints::ForkOptions {
            stop: vec!["poller".into(), "extra".into()],
            scrub: vec!["/tmp/mine".into()],
            ..Default::default()
        };
        let options =
            crate::checkpoints::options_with_recipe_rules("builder", asked.clone()).unwrap();
        assert_eq!(options.stop, ["poller", "extra"]);
        assert_eq!(options.scrub, ["/tmp/mine", "/home/silo/.env"]);
        let plain =
            crate::checkpoints::options_with_recipe_rules("someone-else", asked.clone()).unwrap();
        assert_eq!(plain, asked);
        records::use_test_path(None);
        std::fs::remove_dir_all(app).unwrap();
    }

    #[test]
    fn a_corrupt_records_file_refuses_options_with_recipe_rules() {
        let app = folder();
        let path = app.join("recipe-records.json");
        std::fs::write(&path, "corrupt-json").unwrap();
        records::use_test_path(Some(path));
        let asked = crate::runtime::checkpoints::ForkOptions::default();
        let result = crate::checkpoints::options_with_recipe_rules("builder", asked);
        assert!(result.is_err());
        records::use_test_path(None);
        std::fs::remove_dir_all(app).unwrap();
    }

    #[test]
    fn a_merge_appends_new_packages_and_files_to_the_recipe_and_the_record() {
        let app = folder();
        records::use_test_path(Some(app.join("recipe-records.json")));
        let library = Library::new(&app);
        library.save("web", &definition()).unwrap();
        records::set("main", &record("web")).unwrap();

        let mut adds = Additions::default();
        adds.add_package("dpkg", "jq");
        adds.add_package("apt", "git");
        adds.add_package("pip3", "requests");
        adds.add_package("snap", "lxd");
        adds.add_file("/home/silo/.bashrc");
        adds.add_file("/home/silo/.gitconfig");
        let recorded = record_merge(&library, "main", &adds, true)
            .unwrap()
            .unwrap();
        let fresh = recorded.record;
        assert_eq!(recorded.name, "web");
        assert_eq!(fresh.packages["apt"], ["jq"]);
        assert_eq!(fresh.packages["pip"], ["requests"]);
        assert!(!fresh.packages.contains_key("snap"));
        assert_eq!(fresh.files, ["/home/silo/.gitconfig"]);

        let stored = library.saved("web").unwrap().unwrap();
        assert!(stored.spec.packages.apt.contains(&"jq".to_owned()));
        assert!(stored.files.contains(&"/home/silo/.gitconfig".to_owned()));
        let on_main = records::get("main").unwrap().unwrap();
        assert_eq!(on_main.recipe, stored);
        // The same again adds nothing.
        assert!(record_merge(&library, "main", &adds, true)
            .unwrap()
            .is_none());
        // A computer with no recipe has nothing to append to.
        assert!(record_merge(&library, "nobody", &adds, true)
            .unwrap()
            .is_none());
        records::use_test_path(None);
        std::fs::remove_dir_all(app).unwrap();
    }

    #[test]
    fn record_merge_skips_saving_recipe_when_save_recipe_is_false() {
        let app = folder();
        records::use_test_path(Some(app.join("recipe-records.json")));
        let library = Library::new(&app);
        library.save("web", &definition()).unwrap();
        records::set("main", &record("web")).unwrap();

        let mut adds = Additions::default();
        adds.add_package("apt", "jq");
        let recorded = record_merge(&library, "main", &adds, false)
            .unwrap()
            .unwrap();
        assert_eq!(recorded.name, "web");
        assert_eq!(recorded.record.packages["apt"], ["jq"]);

        let stored = library.saved("web").unwrap().unwrap();
        assert!(!stored.spec.packages.apt.contains(&"jq".to_owned()));
        let on_main = records::get("main").unwrap().unwrap();
        assert!(on_main.recipe.spec.packages.apt.contains(&"jq".to_owned()));
        records::use_test_path(None);
        std::fs::remove_dir_all(app).unwrap();
    }

    #[test]
    fn a_recipe_that_ships_with_silo_is_never_edited_by_a_merge() {
        let app = folder();
        records::use_test_path(Some(app.join("recipe-records.json")));
        let library = Library::new(&app);
        let mut main = record("linux-dev");
        main.recipe = built_in("linux-dev").unwrap();
        records::set("main", &main).unwrap();
        let mut adds = Additions::default();
        adds.add_package("npm", "typescript");
        let recorded = record_merge(&library, "main", &adds, true)
            .unwrap()
            .unwrap();
        assert_eq!(recorded.record.packages["npm"], ["typescript"]);
        assert!(library.saved("linux-dev").unwrap().is_none());
        assert_eq!(
            built_in("linux-dev").unwrap().spec.packages.npm,
            Vec::<String>::new()
        );
        assert_eq!(
            records::get("main")
                .unwrap()
                .unwrap()
                .recipe
                .spec
                .packages
                .npm,
            ["typescript"]
        );
        records::use_test_path(None);
        std::fs::remove_dir_all(app).unwrap();
    }

    #[test]
    fn undoing_a_merge_removes_exactly_what_it_appended() {
        let app = folder();
        records::use_test_path(Some(app.join("recipe-records.json")));
        let library = Library::new(&app);
        library.save("web", &definition()).unwrap();
        records::set("main", &record("web")).unwrap();

        let mut adds = Additions::default();
        adds.add_package("apt", "git");
        adds.add_package("apt", "jq");
        adds.add_package("pip", "requests");
        adds.add_file("/home/silo/.bashrc");
        adds.add_file("/home/silo/.gitconfig");
        let recorded = record_merge(&library, "main", &adds, true)
            .unwrap()
            .unwrap();
        assert_eq!(recorded.record.packages["apt"], ["jq"]);
        assert_eq!(recorded.saved, recorded.record);

        // Something the owner added to the saved recipe after the merge stays.
        let mut saved = library.saved("web").unwrap().unwrap();
        saved.spec.packages.apt.push("tmux".into());
        library.save("web", &saved).unwrap();

        unrecord_merge(&library, "main", &recorded).unwrap();
        let after = library.saved("web").unwrap().unwrap();
        assert_eq!(after.spec.packages.apt, ["git", "curl", "tmux"]);
        assert!(after.spec.packages.pip.is_empty());
        assert_eq!(after.files, ["/home/silo/.bashrc"]);
        let on_main = records::get("main").unwrap().unwrap();
        assert_eq!(on_main.recipe.spec.packages.apt, ["git", "curl"]);
        assert!(on_main.recipe.spec.packages.pip.is_empty());
        assert_eq!(on_main.recipe.files, ["/home/silo/.bashrc"]);
        // Undoing again, or after the entries are gone, changes nothing.
        unrecord_merge(&library, "main", &recorded).unwrap();
        assert_eq!(library.saved("web").unwrap().unwrap(), after);
        records::use_test_path(None);
        std::fs::remove_dir_all(app).unwrap();
    }

    #[test]
    fn undoing_a_merge_leaves_the_saved_recipe_alone_when_the_merge_did_not_save() {
        let app = folder();
        records::use_test_path(Some(app.join("recipe-records.json")));
        let library = Library::new(&app);
        let mut saved = definition();
        saved.spec.packages.apt.push("jq".into());
        library.save("web", &saved).unwrap();
        records::set("main", &record("web")).unwrap();

        let mut adds = Additions::default();
        adds.add_package("apt", "jq");
        let recorded = record_merge(&library, "main", &adds, false)
            .unwrap()
            .unwrap();
        assert!(recorded.saved.is_empty());
        unrecord_merge(&library, "main", &recorded).unwrap();
        assert_eq!(library.saved("web").unwrap().unwrap(), saved);
        let on_main = records::get("main").unwrap().unwrap();
        assert!(!on_main.recipe.spec.packages.apt.contains(&"jq".to_owned()));
        records::use_test_path(None);
        std::fs::remove_dir_all(app).unwrap();
    }

    #[test]
    fn undoing_a_merge_does_not_touch_a_record_of_another_recipe() {
        let app = folder();
        records::use_test_path(Some(app.join("recipe-records.json")));
        let library = Library::new(&app);
        records::set("main", &record("web")).unwrap();
        let mut adds = Additions::default();
        adds.add_package("npm", "eslint");
        let recorded = record_merge(&library, "main", &adds, true)
            .unwrap()
            .unwrap();
        // The computer was rebuilt from another recipe that happens to list the same package.
        let mut other = record("other");
        other.recipe.spec.packages.npm.push("eslint".into());
        records::set("main", &other).unwrap();
        unrecord_merge(&library, "main", &recorded).unwrap();
        assert_eq!(records::get("main").unwrap().unwrap(), other);
        records::use_test_path(None);
        std::fs::remove_dir_all(app).unwrap();
    }

    #[test]
    fn append_skips_additions_that_would_exceed_limits() {
        let mut recipe = definition();
        recipe.spec.packages.apt = (0..250).map(|i| format!("pkg{i}")).collect();
        let mut adds = Additions::default();
        for i in 250..270 {
            adds.add_package("apt", &format!("pkg{i}"));
        }
        let (updated, fresh) = append(&recipe, &adds);
        assert_eq!(updated.spec.packages.apt.len(), super::library::MAX_LIST);
        assert_eq!(fresh.packages["apt"].len(), 6);

        let recipe = definition();
        let big_str = "a".repeat(100);
        let mut adds = Additions::default();
        for i in 0..3000 {
            adds.add_package("apt", &format!("pkg{i}_{big_str}"));
        }
        let (updated, _) = append(&recipe, &adds);
        let size = serde_json::to_vec(&updated).unwrap().len();
        assert!(size <= super::library::MAX_BYTES);
    }

    #[test]
    fn packages_a_system_cannot_install_are_not_appended() {
        let mut adds = Additions::default();
        adds.add_package("brew", "wget");
        adds.add_package("cargo", "ripgrep");
        let (linux, fresh) = append(&definition(), &adds);
        assert!(linux.spec.packages.brew.is_empty());
        assert_eq!(fresh.packages.keys().collect::<Vec<_>>(), ["cargo"]);
        assert_eq!(manager_key("dpkg"), Some("apt"));
        assert_eq!(manager_key("snap"), None);
    }
}
