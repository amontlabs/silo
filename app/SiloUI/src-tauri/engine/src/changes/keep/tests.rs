//! Keep and undo against fake computers. The fake interprets the tagged guest commands the
//! keep sends (`scripts.rs`), so these tests cover the order of the steps, what stops a
//! keep, and what a stopped, failed or interrupted keep leaves behind.

use std::{
    collections::BTreeMap,
    io::Read,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

use super::{
    env::{CheckpointRef, ExecOut, GuestCmd, KeepEnv, ManifestStore, Os, RunAs},
    journal::{Entry, Journal},
    plan, scripts, Hooks, Outcome, Request, UndoOutcome,
};
use crate::changes::{diff::diff, ignore::IgnoreList, manifest::Manifest};

// ------------------------------------------------------------------ fake world

#[derive(Clone, Default, Debug)]
struct Comp {
    files: BTreeMap<String, (Vec<u8>, String)>,
    links: BTreeMap<String, String>,
    crontab: BTreeMap<String, String>,
    /// "manager name" -> version
    pkgs: BTreeMap<String, String>,
    pids: BTreeMap<String, u32>,
    merged: Vec<String>,
    hooks: Vec<String>,
}

struct World {
    os: Os,
    comps: Mutex<BTreeMap<String, Comp>>,
    snapshots: Mutex<Vec<(String, CheckpointRef, Comp)>>,
    log: Mutex<Vec<String>>,
    /// What the merge check reports on main.
    git_state: Mutex<String>,
    changed_files: Mutex<Vec<String>>,
    fail_tag: Mutex<Option<&'static str>>,
    stuck: Mutex<bool>,
    /// Blocks inside the first `hook` until released.
    gate: Mutex<Option<mpsc::Receiver<()>>>,
    entered: Mutex<Option<mpsc::Sender<()>>>,
}

impl World {
    fn new(os: Os, names: &[&str]) -> Arc<Self> {
        let mut comps = BTreeMap::new();
        for name in names {
            let mut comp = Comp::default();
            comp.pids.insert("web".into(), 100);
            comps.insert((*name).to_owned(), comp);
        }
        Arc::new(Self {
            os,
            comps: Mutex::new(comps),
            snapshots: Mutex::default(),
            log: Mutex::default(),
            git_state: Mutex::new("ff".into()),
            changed_files: Mutex::default(),
            fail_tag: Mutex::default(),
            stuck: Mutex::default(),
            gate: Mutex::default(),
            entered: Mutex::default(),
        })
    }

    fn with<R>(&self, name: &str, f: impl FnOnce(&mut Comp) -> R) -> R {
        f(self.comps.lock().unwrap().get_mut(name).unwrap())
    }

    fn logged(&self, tag: &str) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.split(':').nth(1) == Some(tag))
            .count()
    }

    fn order(&self, tags: &[&str]) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter_map(|l| l.split(':').nth(1))
            .filter(|t| tags.contains(t))
            .map(str::to_owned)
            .collect()
    }
}

struct Fake(Arc<World>);

fn hash(data: &[u8]) -> String {
    format!("c:{}", String::from_utf8_lossy(data))
}

impl KeepEnv for Fake {
    fn os(&self, _: &str) -> Result<Os, String> {
        Ok(self.0.os)
    }

    fn run(&self, computer: &str, _who: RunAs, cmd: &GuestCmd) -> Result<ExecOut, String> {
        let w = &self.0;
        w.log.lock().unwrap().push(format!(
            "{computer}:{}:{}",
            cmd.tag,
            cmd.args.first().map_or("", |a| a.1.as_str())
        ));
        if *w.fail_tag.lock().unwrap() == Some(cmd.tag) {
            return Ok(ExecOut::failed(1, "boom"));
        }
        let out = match cmd.tag {
            "fingerprint" => {
                let paths =
                    String::from_utf8_lossy(cmd.stdin.as_deref().unwrap_or_default()).into_owned();
                w.with(computer, |c| {
                    let mut text = String::new();
                    for p in paths.lines() {
                        if let Some(t) = c.links.get(p) {
                            text += &format!("{p}\tL:{t}\n");
                        } else if let Some((d, _)) = c.files.get(p) {
                            text += &format!("{p}\t{}\n", hash(d));
                        }
                    }
                    text
                })
            }
            "crontab_read" => w.with(computer, |c| {
                c.crontab.get(cmd.get("user")).cloned().unwrap_or_default()
            }),
            "crontab_write" => {
                let text =
                    String::from_utf8_lossy(cmd.stdin.as_deref().unwrap_or_default()).into_owned();
                w.with(computer, |c| {
                    c.crontab.insert(cmd.get("user").to_owned(), text)
                });
                String::new()
            }
            "file_remove" => {
                w.with(computer, |c| c.files.remove(cmd.get("path")));
                String::new()
            }
            "symlink" => {
                w.with(computer, |c| {
                    c.links
                        .insert(cmd.get("path").to_owned(), cmd.get("target").to_owned())
                });
                String::new()
            }
            "pkg_install" => {
                let manager = cmd.get("manager").to_owned();
                for spec in cmd.get("specs").lines() {
                    let (name, version) = spec
                        .split_once('=')
                        .map(|(n, v)| (n.trim_end_matches('='), v.trim_start_matches('=')))
                        .or_else(|| spec.split_once('@'))
                        .unwrap_or((spec, ""));
                    w.with(computer, |c| {
                        c.pkgs
                            .insert(format!("{manager} {name}"), version.to_owned())
                    });
                }
                String::new()
            }
            "git_bundle" => {
                w.with(computer, |c| {
                    c.files.insert(
                        cmd.get("bundle").to_owned(),
                        (b"bundle".to_vec(), "600".into()),
                    )
                });
                String::new()
            }
            "git_changed" => w.changed_files.lock().unwrap().join("\n"),
            "git_precheck" => w.git_state.lock().unwrap().clone(),
            "git_merge" => {
                w.with(computer, |c| c.merged.push(cmd.get("repo").to_owned()));
                "abc1234\n".into()
            }
            "service_pid" => w
                .with(computer, |c| {
                    c.pids.get(cmd.get("name")).copied().unwrap_or(0)
                })
                .to_string(),
            "service_restart" => {
                if !*w.stuck.lock().unwrap() {
                    w.with(computer, |c| {
                        *c.pids.entry(cmd.get("name").to_owned()).or_insert(100) += 100
                    });
                }
                String::new()
            }
            "hook" => {
                if let Some(tx) = w.entered.lock().unwrap().take() {
                    let _ = tx.send(());
                    if let Some(rx) = w.gate.lock().unwrap().take() {
                        let _ = rx.recv_timeout(Duration::from_secs(10));
                    }
                }
                w.with(computer, |c| c.hooks.push(cmd.get("cmd").to_owned()));
                if cmd.get("cmd").contains("fail") {
                    return Ok(ExecOut::failed(2, "hook failed"));
                }
                String::new()
            }
            _ => String::new(),
        };
        Ok(ExecOut::ok(out))
    }

    fn read(&self, computer: &str, path: &str) -> Result<(Vec<u8>, String), String> {
        self.0
            .log
            .lock()
            .unwrap()
            .push(format!("{computer}:file_read:{path}"));
        self.0
            .with(computer, |c| c.files.get(path).cloned())
            .ok_or_else(|| format!("{path} is missing"))
    }

    fn write(&self, computer: &str, path: &str, data: &[u8], mode: &str) -> Result<(), String> {
        self.0
            .log
            .lock()
            .unwrap()
            .push(format!("{computer}:file_write:{path}"));
        self.0.with(computer, |c| {
            c.files
                .insert(path.to_owned(), (data.to_vec(), mode.to_owned()))
        });
        Ok(())
    }

    fn checkpoint_before_keep(&self, computer: &str, name: &str) -> Result<CheckpointRef, String> {
        let mut snaps = self.0.snapshots.lock().unwrap();
        let reference = CheckpointRef {
            id: format!("cp{}", snaps.len() + 1),
            name: name.to_owned(),
            created_at: "t".into(),
        };
        let state = self.0.comps.lock().unwrap().get(computer).unwrap().clone();
        snaps.push((computer.to_owned(), reference.clone(), state));
        self.0
            .log
            .lock()
            .unwrap()
            .push(format!("{computer}:checkpoint:{name}"));
        Ok(reference)
    }

    fn before_keep_checkpoints(&self, computer: &str) -> Result<Vec<CheckpointRef>, String> {
        Ok(self
            .0
            .snapshots
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.0 == computer)
            .map(|s| s.1.clone())
            .collect())
    }

    fn restore(&self, computer: &str, id: &str) -> Result<(), String> {
        let state = self
            .0
            .snapshots
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.1.id == id)
            .map(|s| s.2.clone())
            .ok_or("no such checkpoint")?;
        self.0
            .comps
            .lock()
            .unwrap()
            .insert(computer.to_owned(), state);
        self.0
            .log
            .lock()
            .unwrap()
            .push(format!("{computer}:restore:{id}"));
        Ok(())
    }
}

// ------------------------------------------------------------------ fake store

struct Store {
    base: Manifest,
    manifests: BTreeMap<String, Manifest>,
    crontab: Option<String>,
}

impl ManifestStore for Store {
    fn fork_time(&self, _: &str) -> Result<Manifest, String> {
        Ok(self.base.clone())
    }
    fn capture(&self, computer: &str) -> Result<Manifest, String> {
        self.manifests
            .get(computer)
            .cloned()
            .ok_or_else(|| "not captured".into())
    }
    fn fork_time_crontab(&self, _: &str, _: &str) -> Result<Option<String>, String> {
        Ok(self.crontab.clone())
    }
    fn source(&self, _: &str) -> Result<Option<String>, String> {
        Ok(Some("main".into()))
    }
}

fn m(text: &str) -> Manifest {
    Manifest::parse(&format!("META\tformat\t2\nMETA\thome\t/root\n{text}")).unwrap()
}

fn fixture(name: &str) -> Manifest {
    let path = format!(
        "{}/src/changes/fixtures/{name}.tsv.gz",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_string(&mut text)
        .unwrap();
    Manifest::parse(&text).unwrap()
}

fn store(base: &str, fork: &str, main: &str) -> Store {
    Store {
        base: m(base),
        manifests: BTreeMap::from([("fork".to_owned(), m(fork)), ("main".to_owned(), m(main))]),
        crontab: None,
    }
}

fn request(hooks: Hooks) -> Request {
    Request {
        fork: "fork".into(),
        target: Some("main".into()),
        hooks,
        dry_run: false,
    }
}

fn journal() -> (tempfile::TempDir, Journal) {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path().join("keep"));
    (dir, journal)
}

const WAIT: Duration = Duration::from_secs(5);

fn with_fork_files(world: &World, files: &[(&str, &str)]) {
    for (path, text) in files {
        world.with("fork", |c| {
            c.files
                .insert((*path).to_owned(), (text.as_bytes().to_vec(), "644".into()))
        });
    }
}

// ------------------------------------------------------------------ the recorded manifests

#[test]
fn the_recorded_cli_tools_task_plans_its_packages_files_and_commits() {
    let (base, fork) = (fixture("l3.parent"), fixture("l3.fork"));
    let changes = diff(&base, &fork, None, &IgnoreList::builtin());
    let plan = plan::build(&changes, &base, &fork, &base);
    assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
    let mut names: Vec<_> = plan
        .packages
        .iter()
        .map(|p| (p.manager.as_str(), p.name.as_str()))
        .collect();
    names.sort();
    assert_eq!(names, [("dpkg", "jq"), ("dpkg", "ripgrep")]);
    assert!(plan
        .files
        .iter()
        .any(|f| f.path == "/root/.bashrc" && f.action == plan::FileAction::Copy));
    assert_eq!(plan.repos.len(), 1);
    assert_eq!(plan.repos[0].repo, "/root/app");
    assert!(plan.repos[0].commits.is_some());
}

#[test]
fn a_file_main_changed_since_the_fork_is_a_conflict_unless_both_ended_the_same() {
    let (base, fork) = (fixture("l3.parent"), fixture("l3.fork"));
    let changes = diff(&base, &fork, None, &IgnoreList::builtin());
    let mut other = base.clone();
    other.files.get_mut("/root/.bashrc").unwrap().digest = "ee".repeat(32);
    let plan = plan::build(&changes, &base, &fork, &other);
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].item, "/root/.bashrc");
    assert!(plan.files.iter().all(|f| f.path != "/root/.bashrc"));
    // Main ended with the fork's own content: nothing to bring, nothing in the way.
    let same = plan::build(&changes, &base, &fork, &fork);
    assert!(same.conflicts.is_empty());
    assert!(same.files.iter().all(|f| f.path != "/root/.bashrc"));
}

#[test]
fn a_package_main_changed_since_the_fork_is_a_conflict() {
    let base = m("PKG\tdpkg\tjq\t1.6\nPKGM\tdpkg\tjq\n");
    let fork = m("PKG\tdpkg\tjq\t1.7\nPKGM\tdpkg\tjq\n");
    let main = m("PKG\tdpkg\tjq\t1.5\nPKGM\tdpkg\tjq\n");
    let changes = diff(&base, &fork, None, &IgnoreList::builtin());
    let plan = plan::build(&changes, &base, &fork, &main);
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].kind, "package");
}

// ------------------------------------------------------------------ keeps

const BASE: &str = "F\t/root/.bashrc\t10\t1\taa\nGIT\t/root/app\taaa\tmain\t0\t0\nDB\tpostgres\tapp\tschema\ts1\nPKG\tdpkg\tjq\t1.6\nPKGM\tdpkg\tjq\n";
const FORK: &str = "F\t/root/.bashrc\t12\t2\tbb\nGIT\t/root/app\tbbb\tmain\t0\t0\nDB\tpostgres\tapp\tschema\ts2\nPKG\tdpkg\tjq\t1.7\nPKGM\tdpkg\tjq\n";

#[test]
fn a_plain_keep_runs_in_order_migrates_and_restarts_with_new_process_ids() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    with_fork_files(&world, &[("/root/.bashrc", "export X=1\n")]);
    world.with("fork", |c| {
        c.files.insert(
            "/tmp/silo-keep-0.bundle".into(),
            (b"bundle".to_vec(), "600".into()),
        )
    });
    *world.changed_files.lock().unwrap() =
        vec!["src/a.js".into(), "migrations/002_users.sql".into()];
    let (_dir, journal) = journal();
    let hooks = Hooks {
        restart: vec!["web".into()],
        migrate: vec!["npm run migrate".into()],
        after: vec!["npm test".into()],
    };
    let outcome = super::keep(
        &Fake(world.clone()),
        &store(BASE, FORK, BASE),
        &journal,
        &request(hooks),
        WAIT,
    )
    .unwrap();
    let Outcome::Kept(report) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(report.checkpoint.as_deref(), Some("cp1"));
    assert_eq!(
        world.order(&[
            "checkpoint",
            "pkg_install",
            "git_merge",
            "file_write",
            "hook",
            "service_restart"
        ]),
        [
            "file_write",
            "checkpoint",
            "pkg_install",
            "git_merge",
            "file_write",
            "hook",
            "service_restart",
            "hook"
        ]
    );
    assert!(
        report
            .steps
            .iter()
            .any(|s| s == "restart web: pid 100 -> 200"),
        "{:?}",
        report.steps
    );
    world.with("main", |c| {
        assert_eq!(c.pkgs.get("dpkg jq").map(String::as_str), Some("1.7"));
        assert_eq!(c.files["/root/.bashrc"].0, b"export X=1\n");
        assert_eq!(c.hooks, ["npm run migrate", "npm test"]);
        assert_eq!(c.pids["web"], 200);
    });
    assert_eq!(journal.pending("main").unwrap(), None);
    assert_eq!(world.logged("git_bundle"), 1);
}

#[test]
fn a_divergent_keep_stops_before_anything_changes() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    let main = "F\t/root/.bashrc\t11\t3\tcc\nGIT\t/root/app\taaa\tmain\t0\t0\nDB\tpostgres\tapp\tschema\ts1\nPKG\tdpkg\tjq\t1.6\nPKGM\tdpkg\tjq\n";
    *world.changed_files.lock().unwrap() = vec!["migrations/1.sql".into()];
    let (_dir, journal) = journal();
    let outcome = super::keep(
        &Fake(world.clone()),
        &store(BASE, FORK, main),
        &journal,
        &request(Hooks::default()),
        WAIT,
    )
    .unwrap();
    let Outcome::Refused { refusal, .. } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(refusal.reason, "divergent");
    assert_eq!(refusal.conflicts[0].item, "/root/.bashrc");
    assert!(
        refusal.message.contains("main was not changed")
            || refusal.message.contains("was not changed")
    );
    for tag in [
        "checkpoint",
        "pkg_install",
        "git_merge",
        "service_restart",
        "hook",
    ] {
        assert_eq!(world.logged(tag), 0, "{tag} ran");
    }
    world.with("main", |c| assert!(c.files.is_empty() && c.pkgs.is_empty()));
}

#[test]
fn a_conflicting_git_merge_is_reported_and_main_is_untouched() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    world.with("fork", |c| {
        c.files.insert(
            "/tmp/silo-keep-0.bundle".into(),
            (b"b".to_vec(), "600".into()),
        )
    });
    *world.git_state.lock().unwrap() =
        "conflict\nCONFLICT (content): Merge conflict in a.js\n".into();
    *world.changed_files.lock().unwrap() = vec!["migrations/1.sql".into()];
    let (_dir, journal) = journal();
    let outcome = super::keep(
        &Fake(world.clone()),
        &store(BASE, FORK, BASE),
        &journal,
        &request(Hooks::default()),
        WAIT,
    )
    .unwrap();
    let Outcome::Refused { refusal, .. } = outcome else {
        panic!("{outcome:?}")
    };
    assert!(refusal
        .conflicts
        .iter()
        .any(|c| c.kind == "git" && c.detail.contains("a.js")));
    assert_eq!(world.logged("git_merge"), 0);
    assert_eq!(world.logged("checkpoint"), 0);
}

#[test]
fn a_database_change_nothing_migrates_is_refused_and_points_to_make_this_main() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    let (_dir, journal) = journal();
    let outcome = super::keep(
        &Fake(world.clone()),
        &store(BASE, FORK, BASE),
        &journal,
        &request(Hooks::default()),
        WAIT,
    )
    .unwrap();
    let Outcome::Refused { refusal, .. } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(refusal.reason, "database");
    assert!(refusal.hint.contains("Make this main"));
    assert_eq!(world.logged("checkpoint"), 0);
    assert_eq!(world.logged("pkg_install"), 0);
    // A migrate step covers it.
    let world = World::new(Os::Linux, &["fork", "main"]);
    with_fork_files(&world, &[("/root/.bashrc", "x")]);
    world.with("fork", |c| {
        c.files.insert(
            "/tmp/silo-keep-0.bundle".into(),
            (b"b".to_vec(), "600".into()),
        )
    });
    let hooks = Hooks {
        migrate: vec!["make migrate".into()],
        ..Hooks::default()
    };
    let outcome = super::keep(
        &Fake(world),
        &store(BASE, FORK, BASE),
        &journal,
        &request(hooks),
        WAIT,
    )
    .unwrap();
    assert!(matches!(outcome, Outcome::Kept(_)));
}

#[test]
fn a_dry_run_reports_the_plan_and_changes_nothing() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    world.with("fork", |c| {
        c.files.insert(
            "/tmp/silo-keep-0.bundle".into(),
            (b"b".to_vec(), "600".into()),
        )
    });
    *world.changed_files.lock().unwrap() = vec!["migrations/1.sql".into()];
    let (_dir, journal) = journal();
    let mut req = request(Hooks::default());
    req.dry_run = true;
    let outcome = super::keep(
        &Fake(world.clone()),
        &store(BASE, FORK, BASE),
        &journal,
        &req,
        WAIT,
    )
    .unwrap();
    let Outcome::DryRun(report) = outcome else {
        panic!("{outcome:?}")
    };
    assert!(report
        .summary
        .iter()
        .any(|l| l.contains("will keep packages: jq 1.7")));
    assert_eq!(world.logged("checkpoint"), 0);
    assert_eq!(world.logged("pkg_install"), 0);
}

const CRON_BASE: &str = "CRON\troot\t01\nCRONF\troot\t/var/spool/cron/crontabs/root\n";
const CRON_FORK: &str = "CRON\troot\t02\nCRONF\troot\t/var/spool/cron/crontabs/root\n";
const CRON_MAIN: &str = "CRON\troot\t03\nCRONF\troot\t/var/spool/cron/crontabs/root\n";

#[test]
fn a_crontab_is_merged_by_line_against_the_fork_time_text() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    world.with("fork", |c| {
        c.crontab
            .insert("root".into(), "* * * * * a\n5 1 * * * b\n".into())
    });
    world.with("main", |c| {
        c.crontab.insert(
            "root".into(),
            "* * * * * a\n0 1 * * * b\n0 2 * * * c\n0 3 * * * main-only\n".into(),
        )
    });
    let mut st = store(CRON_BASE, CRON_FORK, CRON_MAIN);
    st.crontab = Some("* * * * * a\n0 1 * * * b\n0 2 * * * c\n".into());
    let (_dir, journal) = journal();
    let outcome = super::keep(
        &Fake(world.clone()),
        &st,
        &journal,
        &request(Hooks::default()),
        WAIT,
    )
    .unwrap();
    assert!(matches!(outcome, Outcome::Kept(_)), "{outcome:?}");
    world.with("main", |c| {
        assert_eq!(
            c.crontab["root"],
            "* * * * * a\n0 3 * * * main-only\n5 1 * * * b\n"
        );
    });
}

#[test]
fn an_unchanged_main_crontab_takes_the_forks_whole() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    world.with("fork", |c| {
        c.crontab.insert("root".into(), "* * * * * new\n".into())
    });
    world.with("main", |c| {
        c.crontab.insert("root".into(), "* * * * * old\n".into())
    });
    let (_dir, journal) = journal();
    let st = store(CRON_BASE, CRON_FORK, CRON_BASE);
    super::keep(
        &Fake(world.clone()),
        &st,
        &journal,
        &request(Hooks::default()),
        WAIT,
    )
    .unwrap();
    world.with("main", |c| assert_eq!(c.crontab["root"], "* * * * * new\n"));
}

#[test]
fn macos_uses_launchctl_and_recreates_launch_agents_on_the_other_side() {
    let base = "LAGENT\tuser\tdev.old\taa\t/Users/a/Library/LaunchAgents/dev.old.plist\n";
    let fork = "LAGENT\tuser\tdev.new\tbb\t/Users/a/Library/LaunchAgents/dev.new.plist\n";
    let world = World::new(Os::Macos, &["fork", "main"]);
    with_fork_files(
        &world,
        &[("/Users/a/Library/LaunchAgents/dev.new.plist", "<plist/>")],
    );
    world.with("main", |c| {
        c.files.insert(
            "/Users/a/Library/LaunchAgents/dev.old.plist".into(),
            (b"o".to_vec(), "644".into()),
        )
    });
    let (_dir, journal) = journal();
    let hooks = Hooks {
        restart: vec!["dev.web".into()],
        ..Hooks::default()
    };
    let outcome = super::keep(
        &Fake(world.clone()),
        &store(base, fork, base),
        &journal,
        &request(hooks),
        WAIT,
    )
    .unwrap();
    let Outcome::Kept(report) = outcome else {
        panic!("{outcome:?}")
    };
    world.with("main", |c| {
        assert!(c
            .files
            .contains_key("/Users/a/Library/LaunchAgents/dev.new.plist"));
        assert!(!c
            .files
            .contains_key("/Users/a/Library/LaunchAgents/dev.old.plist"));
    });
    assert_eq!(world.logged("agent_load"), 1);
    assert_eq!(world.logged("agent_unload"), 1);
    assert!(report
        .steps
        .iter()
        .any(|s| s.starts_with("restart dev.web")));
    assert!(scripts::service_restart(Os::Macos, "x")
        .script
        .contains("launchctl kickstart"));
    assert!(scripts::service_restart(Os::Linux, "x")
        .script
        .contains("systemctl"));
    assert!(scripts::pkg_install(Os::Macos, "brew", &["jq".into()])
        .unwrap()
        .script
        .contains("brew install"));
    assert!(scripts::pkg_install(Os::Linux, "dpkg", &["jq=1".into()])
        .unwrap()
        .script
        .contains("apt-get"));
}

#[test]
fn a_service_that_does_not_come_back_as_a_new_process_fails_the_keep_and_main_is_put_back() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    with_fork_files(&world, &[("/root/.bashrc", "x")]);
    *world.stuck.lock().unwrap() = true;
    let (_dir, journal) = journal();
    let base = "F\t/root/.bashrc\t10\t1\taa\n";
    let fork = "F\t/root/.bashrc\t12\t2\tbb\n";
    let hooks = Hooks {
        restart: vec!["web".into()],
        ..Hooks::default()
    };
    let error = super::keep(
        &Fake(world.clone()),
        &store(base, fork, base),
        &journal,
        &request(hooks),
        WAIT,
    )
    .unwrap_err();
    assert!(
        error.contains("did not come back as a new running process"),
        "{error}"
    );
    assert!(error.contains("put back"), "{error}");
    world.with("main", |c| assert!(c.files.is_empty()));
}

#[test]
fn a_failing_step_puts_main_back_from_the_checkpoint() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    with_fork_files(&world, &[("/root/.bashrc", "new")]);
    world.with("main", |c| {
        c.files
            .insert("/root/keepme".into(), (b"k".to_vec(), "644".into()))
    });
    let (_dir, journal) = journal();
    let base = "F\t/root/.bashrc\t10\t1\taa\n";
    let fork = "F\t/root/.bashrc\t12\t2\tbb\n";
    let hooks = Hooks {
        after: vec!["fail please".into()],
        ..Hooks::default()
    };
    let error = super::keep(
        &Fake(world.clone()),
        &store(base, fork, base),
        &journal,
        &request(hooks),
        WAIT,
    )
    .unwrap_err();
    assert!(error.contains("put back"), "{error}");
    world.with("main", |c| {
        assert!(!c.files.contains_key("/root/.bashrc"));
        assert!(c.files.contains_key("/root/keepme"));
    });
    assert_eq!(world.logged("restore"), 1);
    assert_eq!(journal.pending("main").unwrap(), None);
}

#[test]
fn an_interrupted_keep_is_recovered_by_restoring_its_checkpoint() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    let env = Fake(world.clone());
    let (_dir, journal) = journal();
    // A keep that took its checkpoint, changed main, and then lost the engine.
    let checkpoint = env
        .checkpoint_before_keep("main", "Before keep of fork")
        .unwrap();
    journal
        .begin(&Entry {
            fork: "fork".into(),
            target: "main".into(),
            checkpoint_id: checkpoint.id.clone(),
            checkpoint_name: checkpoint.name,
        })
        .unwrap();
    world.with("main", |c| {
        c.pkgs.insert("dpkg half-applied".into(), "1".into())
    });
    assert!(super::recover_all(&env, &journal).is_empty());
    world.with("main", |c| assert!(c.pkgs.is_empty()));
    assert_eq!(journal.pending("main").unwrap(), None);

    // The next keep finds a journal too, and recovers before it starts.
    journal
        .begin(&Entry {
            fork: "fork".into(),
            target: "main".into(),
            checkpoint_id: "cp1".into(),
            checkpoint_name: "x".into(),
        })
        .unwrap();
    let before = world.logged("restore");
    super::keep(
        &env,
        &store("", "", ""),
        &journal,
        &request(Hooks::default()),
        WAIT,
    )
    .unwrap();
    assert_eq!(world.logged("restore"), before + 1);
}

#[test]
fn undo_restores_the_latest_before_keep_checkpoint_once() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    with_fork_files(&world, &[("/root/.bashrc", "new")]);
    let env = Fake(world.clone());
    let (_dir, journal) = journal();
    let base = "F\t/root/.bashrc\t10\t1\taa\n";
    let fork = "F\t/root/.bashrc\t12\t2\tbb\n";
    super::keep(
        &env,
        &store(base, fork, base),
        &journal,
        &request(Hooks::default()),
        WAIT,
    )
    .unwrap();
    world.with("main", |c| assert!(c.files.contains_key("/root/.bashrc")));
    let UndoOutcome::Undone {
        checkpoint,
        recovered,
        ..
    } = super::undo(&env, &journal, "main", WAIT).unwrap()
    else {
        panic!()
    };
    assert_eq!(checkpoint.id, "cp1");
    assert!(!recovered);
    world.with("main", |c| assert!(!c.files.contains_key("/root/.bashrc")));
    assert_eq!(
        super::undo(&env, &journal, "main", WAIT).unwrap(),
        UndoOutcome::NothingToUndo {
            computer: "main".into()
        }
    );
}

#[test]
fn two_keeps_into_the_same_main_wait_or_fail_cleanly() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    with_fork_files(&world, &[("/root/.bashrc", "new")]);
    let (_dir, journal) = journal();
    let base = "F\t/root/.bashrc\t10\t1\taa\n";
    let fork = "F\t/root/.bashrc\t12\t2\tbb\n";
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *world.entered.lock().unwrap() = Some(entered_tx);
    *world.gate.lock().unwrap() = Some(release_rx);
    let hooks = Hooks {
        after: vec!["slow".into()],
        ..Hooks::default()
    };

    let first = {
        let (world, journal, hooks) = (world.clone(), journal.clone(), hooks.clone());
        let st = store(base, fork, base);
        std::thread::spawn(move || super::keep(&Fake(world), &st, &journal, &request(hooks), WAIT))
    };
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();

    // The second does not get in while the first is mid-keep, and changes nothing.
    let st = store(base, fork, base);
    let snapshots = world.snapshots.lock().unwrap().len();
    let error = super::keep(
        &Fake(world.clone()),
        &st,
        &journal,
        &request(hooks.clone()),
        Duration::from_millis(100),
    )
    .unwrap_err();
    assert!(error.starts_with("busy:"), "{error}");
    assert_eq!(world.snapshots.lock().unwrap().len(), snapshots);

    // Given time, the second waits for the first and then runs.
    let second = {
        let (world, journal, hooks) = (world.clone(), journal.clone(), hooks.clone());
        let st = store(base, fork, base);
        std::thread::spawn(move || super::keep(&Fake(world), &st, &journal, &request(hooks), WAIT))
    };
    std::thread::sleep(Duration::from_millis(100));
    release_tx.send(()).unwrap();
    assert!(matches!(first.join().unwrap().unwrap(), Outcome::Kept(_)));
    assert!(matches!(second.join().unwrap().unwrap(), Outcome::Kept(_)));
    assert_eq!(journal.pending("main").unwrap(), None);
    assert_eq!(world.logged("checkpoint"), 2);
}
